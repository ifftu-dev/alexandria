//! The measurement corpus: labelled sessions on disk, listed by a manifest
//! that pins every entry by digest.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::MeasurementError;
use crate::integrity::{derive_window, SessionEvidence, WindowFeatures};

/// Version of the corpus manifest and entry format.
pub const CORPUS_FORMAT: u32 = 1;

/// The label of a session with no attack in it.
pub const CLEAN_LABEL: &str = "clean";

/// Where a corpus entry came from. Nothing is ever collected from an
/// ordinary session, so there is no variant for one.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// Hand-written or generated. Contract tests only, never certification.
    Synthetic,
    /// A scripted attack session run by a maintainer.
    RedTeam,
    /// A clean session whose participant opted in explicitly, separately
    /// from ordinary use.
    ConsentedClean,
}

/// One committed snapshot of a session: the raw features a client submitted,
/// or a flag the backend recorded itself.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusWindow {
    /// `None` for a snapshot the backend wrote (see `backend_flags`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<WindowFeatures>,
    /// Flags the backend recorded on its own, such as the tamper flags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backend_flags: Vec<String>,
    /// The committed snapshot row as captured. Kept for audit; the evaluator
    /// re-derives flags from `features` and never reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<serde_json::Value>,
}

impl CorpusWindow {
    /// The flags this window commits under the current scoring code.
    pub fn flags(&self) -> Vec<String> {
        let mut flags = self
            .features
            .as_ref()
            .map(|f| derive_window(f).flags)
            .unwrap_or_default();
        for flag in &self.backend_flags {
            if !flags.contains(flag) {
                flags.push(flag.clone());
            }
        }
        flags
    }
}

/// A session's full snapshot sequence with its ground truth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusEntry {
    pub id: String,
    /// [`CLEAN_LABEL`], or the name of the scripted attack.
    pub label: String,
    pub provenance: Provenance,
    /// The trust profile the session ran under.
    pub profile: String,
    pub platform: String,
    pub device_class: String,
    #[serde(default)]
    pub interview: bool,
    /// A verification step (probe, consistency) failed in this session.
    #[serde(default)]
    pub verification_failed: bool,
    pub windows: Vec<CorpusWindow>,
}

impl CorpusEntry {
    pub fn is_clean(&self) -> bool {
        self.label == CLEAN_LABEL
    }

    /// Replays the session through the scoring code.
    pub fn evidence(&self) -> SessionEvidence {
        SessionEvidence {
            windows: self.windows.iter().map(CorpusWindow::flags).collect(),
            verification_failed: self.verification_failed,
            interview: self.interview,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let id = &self.id;
        for (field, value) in [
            ("id", id),
            ("label", &self.label),
            ("profile", &self.profile),
            ("platform", &self.platform),
            ("device_class", &self.device_class),
        ] {
            if value.trim().is_empty() {
                return Err(format!("entry {id}: {field} is empty"));
            }
        }
        match (self.provenance, self.is_clean()) {
            (Provenance::RedTeam, true) => {
                return Err(format!("entry {id}: a red-team session cannot be clean"))
            }
            (Provenance::ConsentedClean, false) => {
                return Err(format!(
                    "entry {id}: a consented clean session must be labelled clean"
                ))
            }
            _ => {}
        }
        if self.windows.is_empty() {
            return Err(format!("entry {id}: no windows"));
        }
        for window in &self.windows {
            if let Some(features) = &window.features {
                features
                    .validate()
                    .map_err(|e| format!("entry {id}: {e}"))?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryRef {
    /// Relative to the manifest's directory.
    pub path: String,
    /// `blake3:<hex>` of the entry file's exact bytes. Empty until sealed.
    #[serde(default)]
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusManifest {
    pub format: u32,
    pub corpus_id: String,
    #[serde(default)]
    pub description: String,
    pub entries: Vec<EntryRef>,
    /// `blake3:<hex>` over the entry list (see [`corpus_digest`]). Empty
    /// until sealed.
    #[serde(default)]
    pub digest: String,
}

/// A loaded corpus whose every entry matched its manifest digest.
#[derive(Debug, Clone)]
pub struct Corpus {
    pub manifest: CorpusManifest,
    pub entries: Vec<CorpusEntry>,
}

/// `blake3:<hex>` of some bytes: the content address used for corpus
/// entries, corpora and certification manifests.
pub fn digest_bytes(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

/// The corpus digest: [`digest_bytes`] over one `path NUL digest LF` line
/// per entry, in manifest order.
pub fn corpus_digest(entries: &[EntryRef]) -> String {
    let mut lines = Vec::new();
    for entry in entries {
        lines.extend_from_slice(entry.path.as_bytes());
        lines.push(0);
        lines.extend_from_slice(entry.digest.as_bytes());
        lines.push(b'\n');
    }
    digest_bytes(&lines)
}

fn read(path: &Path) -> Result<Vec<u8>, MeasurementError> {
    std::fs::read(path).map_err(|source| MeasurementError::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub(super) fn parse<T: serde::de::DeserializeOwned>(
    path: &Path,
    bytes: &[u8],
) -> Result<T, MeasurementError> {
    serde_json::from_slice(bytes).map_err(|source| MeasurementError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn entry_path(base: &Path, relative: &str) -> Result<PathBuf, MeasurementError> {
    let rel = Path::new(relative);
    let plain = rel
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if relative.is_empty() || !plain {
        return Err(MeasurementError::Invalid(format!(
            "entry path {relative} must be relative and stay inside the corpus directory"
        )));
    }
    Ok(base.join(rel))
}

fn read_manifest(path: &Path) -> Result<CorpusManifest, MeasurementError> {
    let manifest: CorpusManifest = parse(path, &read(path)?)?;
    if manifest.format != CORPUS_FORMAT {
        return Err(MeasurementError::Invalid(format!(
            "corpus format {} is not supported (expected {CORPUS_FORMAT})",
            manifest.format
        )));
    }
    Ok(manifest)
}

impl Corpus {
    /// Loads a corpus and refuses it unless every entry file and the entry
    /// list match the digests the manifest pins.
    pub fn load(manifest_path: &Path) -> Result<Self, MeasurementError> {
        let manifest = read_manifest(manifest_path)?;
        let base = manifest_path.parent().unwrap_or(Path::new("."));
        let mut entries = Vec::with_capacity(manifest.entries.len());
        let mut ids = std::collections::BTreeSet::new();
        for entry_ref in &manifest.entries {
            let path = entry_path(base, &entry_ref.path)?;
            let bytes = read(&path)?;
            let actual = digest_bytes(&bytes);
            if actual != entry_ref.digest {
                return Err(MeasurementError::DigestMismatch {
                    what: entry_ref.path.clone(),
                    expected: entry_ref.digest.clone(),
                    actual,
                });
            }
            let entry: CorpusEntry = parse(&path, &bytes)?;
            entry.validate().map_err(MeasurementError::Invalid)?;
            if !ids.insert(entry.id.clone()) {
                return Err(MeasurementError::Invalid(format!(
                    "entry id {} appears twice",
                    entry.id
                )));
            }
            entries.push(entry);
        }
        let actual = corpus_digest(&manifest.entries);
        if actual != manifest.digest {
            return Err(MeasurementError::DigestMismatch {
                what: "corpus".into(),
                expected: manifest.digest.clone(),
                actual,
            });
        }
        Ok(Self { manifest, entries })
    }

    pub fn digest(&self) -> &str {
        &self.manifest.digest
    }

    /// A corpus with any synthetic entry can never certify a signal.
    pub fn is_synthetic(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.provenance == Provenance::Synthetic)
    }
}

/// Recomputes every entry digest and the corpus digest, and rewrites the
/// manifest. Used after adding or editing entries; the change then shows up
/// in review as a digest diff.
pub fn seal(manifest_path: &Path) -> Result<CorpusManifest, MeasurementError> {
    let mut manifest = read_manifest(manifest_path)?;
    let base = manifest_path.parent().unwrap_or(Path::new("."));
    for entry_ref in &mut manifest.entries {
        let path = entry_path(base, &entry_ref.path)?;
        let bytes = read(&path)?;
        let entry: CorpusEntry = parse(&path, &bytes)?;
        entry.validate().map_err(MeasurementError::Invalid)?;
        entry_ref.digest = digest_bytes(&bytes);
    }
    manifest.digest = corpus_digest(&manifest.entries);
    let mut json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| MeasurementError::Invalid(e.to_string()))?;
    json.push('\n');
    std::fs::write(manifest_path, json).map_err(|source| MeasurementError::Io {
        path: manifest_path.to_path_buf(),
        source,
    })?;
    Ok(manifest)
}
