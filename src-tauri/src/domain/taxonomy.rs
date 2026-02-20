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
