//! Plugin manifest parsing and semantic validation.
//!
//! The manifest is the authoritative declaration of a plugin's identity,
//! capabilities, and kinds. It is signed by the author's DID-Key and
//! content-addressed by BLAKE3 of its raw bytes. The shape is frozen at
//! `api_version = "1"` — a manifest written in 2026 must still parse in 2046.

use crate::crypto::did::parse_did_key;
use crate::domain::plugin::{PluginCapability, PluginKind, PluginManifest};

/// Current host-supported manifest API version.
pub const SUPPORTED_API_VERSION: &str = "1";

/// Default iframe entry if the manifest doesn't override it.
pub const DEFAULT_ENTRY: &str = "ui/index.html";

/// Parse the manifest JSON bytes and run structural validation.
/// Does *not* verify the signature — that's `verifier::verify_manifest`.
pub fn parse_and_validate(bytes: &[u8]) -> Result<PluginManifest, String> {
    let manifest: PluginManifest =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid plugin manifest JSON: {e}"))?;
    validate(&manifest)?;
    Ok(manifest)
}

/// Semantic validation: fields within bounds, DID parses, capabilities
/// are in the allowlist, etc. This is what makes the "no shell, no
/// arbitrary net" invariant a runtime check, not a gentleman's agreement.
pub fn validate(manifest: &PluginManifest) -> Result<(), String> {
    if manifest.api_version != SUPPORTED_API_VERSION {
        return Err(format!(
            "unsupported plugin api_version '{}' (host supports '{}')",
            manifest.api_version, SUPPORTED_API_VERSION
        ));
    }

    if manifest.name.trim().is_empty() {
        return Err("plugin manifest 'name' must be non-empty".into());
    }
    if manifest.id.trim().is_empty() {
        return Err("plugin manifest 'id' must be non-empty".into());
    }
    if manifest.version.trim().is_empty() {
        return Err("plugin manifest 'version' must be non-empty".into());
    }
    if manifest.kinds.is_empty() {
        return Err("plugin manifest must declare at least one kind".into());
    }

    // Author DID must be a valid did:key:z... — parse_did_key is
    // exhaustive (checks multibase, multicodec, key length).
    parse_did_key(&manifest.author_did).map_err(|e| format!("invalid author_did: {e}"))?;

    // Plugin id should namespace under the author DID. We require the
    // form `<author_did>#<slug>` so that two authors can't collide on
    // the same slug and so the id tracks the DID for key rotation.
    let Some((did_part, slug)) = manifest.id.split_once('#') else {
        return Err("plugin id must be of the form '<author_did>#<slug>'".into());
    };
    if did_part != manifest.author_did {
        return Err("plugin id's DID portion must match author_did".into());
    }
    if slug.trim().is_empty() || !slug.chars().all(is_slug_char) {
        return Err(
            "plugin id's slug portion must be non-empty and contain only [a-z0-9_-]".into(),
        );
    }

    // Capabilities: serde already parsed them into the enum, so the
    // protocol-level allowlist is enforced structurally — unknown
    // capability strings fail at deserialization with a clear error.
    // But we still reject duplicates to keep the UX predictable.
    let mut seen = std::collections::HashSet::new();
    for cap in &manifest.capabilities {
        if !seen.insert(*cap) {
            return Err(format!("duplicate capability declared: {}", cap.as_str()));
        }
    }

    // Kinds: for Phase 1 only `interactive` is runnable. `graded`
    // is accepted in the manifest (so Phase 1-era plugins can be
    // forward-compatible) but loading it will be refused by the
    // host until Phase 2 ships the Wasmtime grader runtime.
    // We don't reject the manifest here — the *load* path is the
    // right place to decide what a Phase-1 host will actually run.

    // Platforms must be a subset of the known set.
    for p in &manifest.platforms {
        if !matches!(
            p.as_str(),
            "macos" | "windows" | "linux" | "ios" | "android"
        ) {
            return Err(format!("unknown platform '{p}' in manifest"));
        }
    }

    // Entry path must be relative, no traversal. This is re-checked
    // at the asset-protocol layer too, but fail early on install.
    validate_relative_path(&manifest.entry, "entry")?;
    if let Some(icon) = &manifest.icon_path {
        validate_relative_path(icon, "icon_path")?;
    }

    Ok(())
}

/// Phase-1 decision: is this manifest something the host will actually run?
/// Graded plugins require the Phase-2 runtime; we accept them into the
/// installed set (so they don't disappear when the user upgrades) but
/// the player refuses to mount them until the grader runtime is present.
pub fn is_loadable_in_phase_1(manifest: &PluginManifest) -> bool {
    manifest.kinds.contains(&PluginKind::Interactive)
}

pub fn capability_strings(manifest: &PluginManifest) -> Vec<String> {
    manifest
        .capabilities
        .iter()
        .map(|c| c.as_str().to_string())
        .collect()
}

pub fn parse_capability(s: &str) -> Option<PluginCapability> {
    PluginCapability::parse(s)
}

fn is_slug_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Reject path-traversal and absolute paths at manifest-validate time.
/// Paths are resolved relative to the bundle root.
fn validate_relative_path(p: &str, field: &str) -> Result<(), String> {
    if p.is_empty() {
        return Err(format!("manifest '{field}' must be non-empty"));
    }
    if p.starts_with('/') || p.starts_with('\\') {
        return Err(format!("manifest '{field}' must be relative, got '{p}'"));
    }
    for component in p.split(['/', '\\']) {
        if component == ".." {
            return Err(format!(
                "manifest '{field}' must not contain parent-directory references"
            ));
        }
        if component.contains('\0') {
            return Err(format!("manifest '{field}' contains a null byte"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest_json(api_version: &str) -> String {
        format!(
            r#"{{
                "id": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK#pitch-trainer",
                "version": "0.1.0",
                "api_version": "{api_version}",
                "host_min_version": "0.1.0",
                "name": "Pitch Trainer",
                "description": "Play the shown notes; get real-time feedback.",
                "author_did": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
                "kinds": ["interactive"],
                "capabilities": ["microphone"],
                "platforms": ["macos","linux","windows"],
                "entry": "ui/index.html"
            }}"#
        )
    }

    #[test]
    fn parses_valid_manifest() {
        let m = parse_and_validate(sample_manifest_json("1").as_bytes()).unwrap();
        assert_eq!(m.name, "Pitch Trainer");
        assert!(m.kinds.contains(&PluginKind::Interactive));
        assert_eq!(m.capabilities.len(), 1);
    }

    #[test]
    fn rejects_wrong_api_version() {
        let m = parse_and_validate(sample_manifest_json("2").as_bytes());
        assert!(m.is_err());
    }

    #[test]
    fn rejects_bad_did() {
        let bad = r#"{
            "id": "did:web:example.com#x",
            "version": "0.1.0",
            "api_version": "1",
            "host_min_version": "0.1.0",
            "name": "X",
            "author_did": "did:web:example.com",
            "kinds": ["interactive"]
        }"#;
        assert!(parse_and_validate(bad.as_bytes()).is_err());
    }

    #[test]
    fn rejects_id_not_namespaced_under_author_did() {
        let bad = r#"{
            "id": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK#x",
            "version": "0.1.0",
            "api_version": "1",
            "host_min_version": "0.1.0",
            "name": "X",
            "author_did": "did:key:z6MkubM4drVzMMYqS5wyWo2tqtWgLrGCMY4qNsEaUjHbLbAN",
            "kinds": ["interactive"]
        }"#;
        assert!(parse_and_validate(bad.as_bytes()).is_err());
    }

    #[test]
    fn rejects_path_traversal_in_entry() {
        let bad = format!(
            r#"{{
                "id": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK#x",
                "version": "0.1.0",
                "api_version": "1",
                "host_min_version": "0.1.0",
                "name": "X",
                "author_did": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
                "kinds": ["interactive"],
                "entry": "../outside/index.html"
            }}"#
        );
        assert!(parse_and_validate(bad.as_bytes()).is_err());
    }

    #[test]
    fn rejects_unknown_capability() {
        let bad = r#"{
            "id": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK#x",
            "version": "0.1.0",
            "api_version": "1",
            "host_min_version": "0.1.0",
            "name": "X",
            "author_did": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
            "kinds": ["interactive"],
            "capabilities": ["shell_exec"]
        }"#;
        assert!(parse_and_validate(bad.as_bytes()).is_err());
    }
}
