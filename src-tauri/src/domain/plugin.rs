//! Plugin domain types.
//!
//! Phase 1 of the community plugin system (see
//! `/Users/hack/.claude/plans/prancy-bubbling-grove.md`). A plugin is an
//! iroh blob with a signed manifest plus a `ui/` bundle that renders inside
//! a sandboxed iframe. In Phase 2 a `grader.wasm` is added for credential-
//! eligible assessments. For Phase 1 we only deal with interactive plugins,
//! so manifest parsing is the only authoritative contract the backend needs.
//!
//! The manifest shape is frozen at `api_version = "1"` — the permanence
//! guarantee requires a pinned manifest from 2026 to still parse in 2046.
//! New fields are always additive and optional.

use serde::{Deserialize, Serialize};

/// A capability a plugin can declare and that the host can enforce at the
/// sandbox boundary. Only the capabilities listed here are declarable in v1;
/// adding more requires a host release (the protocol-level allowlist is
/// the "no shell, no arbitrary net" invariant from the design doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    Microphone,
    Camera,
    Midi,
    Fullscreen,
    Clipboard,
    Storage,
    MlInference,
}

impl PluginCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            PluginCapability::Microphone => "microphone",
            PluginCapability::Camera => "camera",
            PluginCapability::Midi => "midi",
            PluginCapability::Fullscreen => "fullscreen",
            PluginCapability::Clipboard => "clipboard",
            PluginCapability::Storage => "storage",
            PluginCapability::MlInference => "ml_inference",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "microphone" => Some(Self::Microphone),
            "camera" => Some(Self::Camera),
            "midi" => Some(Self::Midi),
            "fullscreen" => Some(Self::Fullscreen),
            "clipboard" => Some(Self::Clipboard),
            "storage" => Some(Self::Storage),
            "ml_inference" => Some(Self::MlInference),
            _ => None,
        }
    }
}

/// Kind of element a plugin provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    /// UI only. Progress-tracked, never credential-eligible.
    Interactive,
    /// Requires a deterministic WASM grader (Phase 2+). Eligible for
    /// credential issuance subject to Plugin DAO attestation.
    Graded,
}

/// The signed manifest that identifies a plugin bundle. Parsed from
/// `manifest.json` inside the bundle directory.
///
/// The `manifest_sig_b64` field is populated separately from the bundle's
/// `manifest.sig` file during install — it is not part of the manifest JSON
/// itself because the signature is over the canonical JSON bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// `did:key:<author>#<slug>` — uniquely names the plugin within its author's DID.
    pub id: String,
    /// Semver. Human-facing; pinning is by CID.
    pub version: String,
    /// Host ABI version the plugin targets. Frozen at `"1"` for v1.
    pub api_version: String,
    /// Minimum host version required (semver).
    pub host_min_version: String,
    /// Display name.
    pub name: String,
    /// One-line summary.
    pub description: Option<String>,
    /// `did:key:z...` of the plugin author. The signature is verified
    /// against the Ed25519 key embedded in this DID.
    pub author_did: String,
    /// Which element kinds the plugin provides.
    pub kinds: Vec<PluginKind>,
    /// Declared capabilities. Per-capability consent is required at runtime.
    #[serde(default)]
    pub capabilities: Vec<PluginCapability>,
    /// Optional grader reference (Phase 2+). Ignored in Phase 1 even if set.
    pub grader: Option<PluginGraderRef>,
    /// CID of the JSON Schema that describes the content payload. Optional.
    pub content_schema_cid: Option<String>,
    /// CID of the JSON Schema that describes the submission payload. Optional.
    pub submission_schema_cid: Option<String>,
    /// Taxonomy hints for discovery UI (Phase 3).
    #[serde(default)]
    pub subject_tags: Vec<String>,
    /// Advertised platform support matrix. Each string is one of
    /// `macos`, `windows`, `linux`, `ios`, `android`. An empty list is
    /// treated as "best effort everywhere" but the UI surfaces it.
    #[serde(default)]
    pub platforms: Vec<String>,
    /// Relative path to a small icon inside the bundle (optional).
    pub icon_path: Option<String>,
    /// Relative path to the iframe entry HTML. Defaults to `ui/index.html`.
    #[serde(default = "default_entry")]
    pub entry: String,
}

fn default_entry() -> String {
    "ui/index.html".to_string()
}

/// Reference to a Phase-2 WASM grader. Kept here so Phase 1 manifests can
/// already carry it forward without breaking the type — but Phase 1 does
/// not execute graders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginGraderRef {
    pub cid: String,
    pub blake3: String,
    #[serde(default = "default_grader_entrypoint")]
    pub entrypoint: String,
}

fn default_grader_entrypoint() -> String {
    "grade".to_string()
}

/// A plugin installed on this node. Mirrors the `plugin_installed` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlugin {
    /// BLAKE3 CID of the plugin bundle (root directory). Identity.
    pub plugin_cid: String,
    /// Display name lifted from the manifest for convenience.
    pub name: String,
    pub version: String,
    pub author_did: String,
    /// Where the extracted bundle lives on disk. Absolute path, inside
    /// `app_data_dir/plugins/<plugin_cid>/`.
    pub install_path: String,
    /// How the plugin got here. `"local_file"` in Phase 1;
    /// `"p2p"` / `"builtin"` in later phases.
    pub source: String,
    /// Full manifest JSON as stored at install time. Re-parsing on load
    /// means the in-memory shape can evolve without re-encoding rows.
    pub manifest_json: String,
    /// Installed-at ISO timestamp.
    pub installed_at: String,
}

/// A persisted per-plugin permission record. Mirrors `plugin_permissions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginPermissionRecord {
    pub plugin_cid: String,
    pub capability: String,
    /// `"once"`, `"session"`, `"always"`.
    pub scope: String,
    pub granted_at: String,
    /// ISO timestamp; `None` for `"always"`.
    pub granted_until: Option<String>,
}

// ---- Phase 3: plugin discovery + DAO attestation ------------------------

/// Plugin announcement payload broadcast on `/alexandria/plugins/1.0`.
///
/// Authors send one of these per published plugin version. The full bundle
/// bytes are *not* on the gossip topic — only the manifest CID + summary
/// metadata. Receivers fetch the bundle on demand via the iroh blob store
/// when the user clicks Install.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAnnouncement {
    pub plugin_cid: String,
    pub manifest_cid: String,
    pub author_did: String,
    pub name: String,
    pub version: String,
    pub api_version: String,
    pub description: Option<String>,
    pub kinds: Vec<PluginKind>,
    pub capabilities: Vec<PluginCapability>,
    pub subject_tags: Vec<String>,
    pub platforms: Vec<String>,
    pub has_grader: bool,
    pub grader_cid: Option<String>,
    /// Author-stamped publish time (ISO-8601). Receivers compare against
    /// any existing row to keep the latest announcement.
    pub announced_at: String,
}

/// A row in the local `plugin_catalog` table — what the discovery UI reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCatalogEntry {
    pub plugin_cid: String,
    pub name: String,
    pub version: String,
    pub author_did: String,
    pub description: Option<String>,
    pub api_version: String,
    pub kinds: Vec<PluginKind>,
    pub capabilities: Vec<PluginCapability>,
    pub subject_tags: Vec<String>,
    pub platforms: Vec<String>,
    pub has_grader: bool,
    pub grader_cid: Option<String>,
    /// `"gossip"` | `"builtin"` | `"local"`.
    pub source: String,
    pub announced_at: String,
    pub last_seen_at: String,
}

/// Plugin DAO attestation event broadcast on
/// `/alexandria/plugin-attestations/1.0`. A multi-sig committee binds a
/// `(plugin_cid, grader_cid)` pair as credential-eligible.
///
/// `signatures` is a list of per-member Ed25519 signatures over
/// `BLAKE3(plugin_cid || grader_cid || canonical_attestation_terms_json)`.
/// Verification requires N-of-M signatures from the listed
/// `committee_pubkeys` to validate, where N is the policy-defined
/// threshold (default 5-of-7; see `plugins::attestation`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAttestationEvent {
    pub plugin_cid: String,
    pub grader_cid: String,
    /// JSON object with freeform terms — the canonical bytes of this
    /// field (serde_json::to_vec) are part of the signed message.
    pub attestation_terms: serde_json::Value,
    /// Committee members' Ed25519 public keys (32-byte hex). The
    /// (committee_pubkeys, threshold) pair *is* the DAO at the moment
    /// of attestation; key rotation produces a new attestation event.
    pub committee_pubkeys: Vec<String>,
    /// Hex-encoded Ed25519 signatures (64 bytes each), one per signer
    /// who endorsed. Order is paired with `signer_indices`.
    pub signatures: Vec<String>,
    /// Indices into `committee_pubkeys` for each signature in `signatures`.
    pub signer_indices: Vec<u32>,
    pub issued_at: String,
}

/// A stored plugin attestation row (mirrors `plugin_attestations`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPluginAttestation {
    pub plugin_cid: String,
    pub grader_cid: String,
    pub attestation_terms: serde_json::Value,
    pub committee_pubkeys: Vec<String>,
    pub issued_at: String,
    pub advisory_kind: Option<String>,
    pub advisory_message: Option<String>,
}

/// Verifier-policy view of "is this plugin attested for credentials?"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAttestationStatus {
    pub plugin_cid: String,
    pub attested: bool,
    /// Some(record) when `attested` is true.
    pub attestation: Option<StoredPluginAttestation>,
    /// Active advisories for this plugin — non-blocking but surfaced
    /// in UI ("known flawed", "deprecated", etc.).
    pub advisories: Vec<PluginAdvisoryRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAdvisoryRecord {
    pub id: String,
    pub plugin_cid: String,
    pub kind: String,
    pub message: String,
    pub issued_at: String,
}
