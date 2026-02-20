//! Taxonomy domain types for the distributed skill graph.
//!
//! The taxonomy defines the skill DAG: Subject Fields → Subjects → Skills
//! with prerequisite edges. It is a DAO-ratified, versioned document stored
//! on IPFS. Updates propagate via `/alexandria/taxonomy/1.0` gossip.
//!
//! Two announcement types:
//!
//! - **`TaxonomyUpdate`**: A ratified taxonomy version — the full set of
//!   changes signed by the DAO committee. Nodes apply it to their local
//!   skill tables after validating the signature chain.
//!
//! - **`TaxonomyProposal`**: A proposed change (not yet ratified). Other
//!   DAO committee members can review it. Does NOT modify local tables.

use serde::{Deserialize, Serialize};

/// A ratified taxonomy update broadcast on `/alexandria/taxonomy/1.0`.
///
/// Per spec §8.1-8.2: the DAO committee signs a new taxonomy version
/// after supermajority approval. The CID points to the full taxonomy
/// document on IPFS. Nodes validate the signature chain (`previous_cid`)
/// before applying.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyUpdate {
    /// Monotonically increasing version number.
    pub version: i64,
    /// BLAKE3 hash (iroh) or IPFS CID of the full taxonomy document.
    pub cid: String,
    /// CID of the previous taxonomy version (for chain validation).
    pub previous_cid: Option<String>,
    /// DAO committee members who ratified this version (stake addresses).
    pub ratified_by: Vec<String>,
    /// Unix timestamp of ratification.
    pub ratified_at: i64,
    /// Changes included in this version.
    pub changes: TaxonomyChanges,
}

/// The set of changes in a taxonomy version.
///
/// Each field contains added/modified/removed items. Removals are
/// soft-deletes (the skill ID is preserved but marked inactive).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyChanges {
    /// New or modified subject fields.
    #[serde(default)]
    pub subject_fields: Vec<TaxonomySubjectField>,
    /// New or modified subjects.
    #[serde(default)]
    pub subjects: Vec<TaxonomySubject>,
    /// New or modified skills.
    #[serde(default)]
    pub skills: Vec<TaxonomySkill>,
    /// New prerequisite edges (skill_id, prerequisite_id).
    #[serde(default)]
    pub prerequisites: Vec<(String, String)>,
    /// Removed prerequisite edges (skill_id, prerequisite_id).
    #[serde(default)]
    pub removed_prerequisites: Vec<(String, String)>,
}

/// A subject field in a taxonomy update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomySubjectField {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// A subject in a taxonomy update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomySubject {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub subject_field_id: String,
}

/// A skill in a taxonomy update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomySkill {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub subject_id: String,
    pub bloom_level: String,
}

/// A taxonomy version record as stored in the local `taxonomy_versions` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyVersion {
    pub version: i64,
    pub cid: String,
    pub previous_cid: Option<String>,
    pub ratified_by: Option<String>,
    pub ratified_at: Option<String>,
    pub signature: Option<String>,
    pub applied_at: String,
}

/// A taxonomy version document stored on IPFS.
///
/// This is the full artifact produced by the ratification workflow.
/// Stored as JSON on IPFS, with the CID anchored on-chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyDocument {
    /// Monotonically increasing version number.
    pub version: i64,
    /// BLAKE3 hash (iroh) of this document.
    pub root_cid: String,
    /// CID of the previous version (for chain validation).
    pub previous_cid: Option<String>,
    /// DAO committee members who ratified (stake addresses).
    pub ratified_by: Vec<String>,
    /// ISO 8601 timestamp of ratification.
    pub ratified_at: String,
    /// Ed25519 signature of the document content.
    pub signature: String,
    /// The actual taxonomy changes.
    pub content: TaxonomyChanges,
}

/// Parameters for proposing a taxonomy change via governance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposeTaxonomyParams {
    /// DAO ID to submit the proposal under.
    pub dao_id: String,
    /// Human-readable title for the proposal.
    pub title: String,
    /// Description of what this change does.
    pub description: Option<String>,
    /// The taxonomy changes being proposed.
    pub changes: TaxonomyChanges,
}

/// Preview of what a taxonomy change would do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyPreview {
    /// Number of subject fields added/modified.
    pub subject_fields_affected: i64,
    /// Number of subjects added/modified.
    pub subjects_affected: i64,
    /// Number of skills added/modified.
    pub skills_affected: i64,
    /// Number of prerequisite edges added.
    pub prerequisites_added: i64,
    /// Number of prerequisite edges removed.
    pub prerequisites_removed: i64,
    /// Whether any existing skills would be modified (vs only new ones).
    pub has_modifications: bool,
    /// Skill IDs that would be newly created.
    pub new_skill_ids: Vec<String>,
    /// Skill IDs that would be modified.
    pub modified_skill_ids: Vec<String>,
}

/// Result of publishing a ratified taxonomy version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyPublishResult {
    /// The new version number assigned.
    pub version: i64,
    /// IPFS CID (BLAKE3) of the published taxonomy document.
    pub content_cid: String,
    /// Number of changes applied to local skill tables.
    pub changes_applied: i64,
}
