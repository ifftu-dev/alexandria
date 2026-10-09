use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostContent {
    pub title: String,
    pub body: String,
    pub post_kind: String,
    pub url: Option<String>,
    pub video_cid: Option<String>,
    pub thumbnail_cid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiscussionAction {
    Post { content: PostContent },
    Comment { body: String },
    EditPost { content: PostContent },
    EditComment { body: String },
    Delete,
    Vote { value: i8 },
    Report { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscussionPayload {
    pub version: u32,
    pub network_id: String,
    pub actor_did: String,
    pub entity_id: String,
    pub thread_id: String,
    pub parent_id: Option<String>,
    pub subject_field_id: String,
    pub nonce: String,
    pub revision: i64,
    pub created_at: i64,
    pub credential_proof_ids: Vec<String>,
    pub qualification_proofs: Vec<DiscussionProof>,
    pub action: DiscussionAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscussionEvent {
    pub discussion_version: u32,
    pub payload: DiscussionPayload,
    pub public_key: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionRequest {
    pub entity_id: Option<String>,
    pub thread_id: Option<String>,
    pub parent_id: Option<String>,
    pub subject_field_id: String,
    pub action: DiscussionAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionItem {
    pub id: String,
    pub thread_id: String,
    pub parent_id: Option<String>,
    pub subject_field_id: String,
    pub author_did: String,
    pub content: Option<PostContent>,
    pub body: String,
    pub created_at: i64,
    pub edited: bool,
    pub deleted: bool,
    pub score: i64,
    pub my_vote: i8,
    pub comment_count: i64,
    pub reported: bool,
    pub credential_proof_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionAccess {
    pub actor_did: String,
    pub eligible_fields: Vec<String>,
    pub governed_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscussionProof {
    pub credential: crate::domain::vc::VerifiableCredential,
    pub endorsement: Option<DiscussionEndorsement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscussionEndorsement {
    pub policy: crate::domain::attestation::CourseCompletionPolicy,
    pub binding: crate::domain::attestation::CourseCompletionBinding,
    pub endorsements: Vec<crate::domain::attestation::CourseCompletionEndorsement>,
}
impl DiscussionEndorsement {
    pub fn as_evidence(&self) -> alexandria_verify::trust::CourseEndorsementEvidence<'_> {
        alexandria_verify::trust::CourseEndorsementEvidence {
            policy: &self.policy,
            binding: &self.binding,
            endorsements: &self.endorsements,
        }
    }
}
