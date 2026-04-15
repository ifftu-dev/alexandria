//! Opinion domain types for the Field Commentary feature.
//!
//! An **opinion** is a signed, scoped, credentialed video take on a
//! specific `subject_field`. Posting is gated on the author holding at
//! least one `skill_proof` (level >= apply) in a skill under that
//! subject field. Opinions are chronological within a subject page —
//! not a global feed — and are moderated through the existing
//! `evidence_challenges` mechanism extended with `target_type='opinion'`.
//!
//! ## Two representations
//!
//! - **`OpinionAnnouncement`**: the signed gossip payload broadcast on
//!   `/alexandria/opinions/1.0`. Receivers verify the signature, the
//!   author's credential proofs, and upsert into the local `opinions`
//!   table (or queue in `opinions_pending_verification` if the
//!   referenced skill-proofs haven't synced yet).
//!
//! - **`OpinionRow`**: a row as materialised from the local SQLite
//!   table. Includes local metadata (`received_at`, `withdrawn`).

use serde::{Deserialize, Serialize};

/// Maximum length of the summary string (app-layer enforced — the
/// database column has no hard limit to keep schema migration cheap).
pub const MAX_SUMMARY_CHARS: usize = 280;

/// The signed announcement a node publishes on `TOPIC_OPINIONS` to
/// share an opinion with the network.
///
/// The payload (every field except `signature` and `public_key`) is
/// serialised canonically and signed with the author's Ed25519 key
/// derived from their Cardano stake key. Receivers verify the
/// signature AND check that every entry in `credential_proof_ids`
/// corresponds to a known `skill_proof` from the same
/// `author_address` in a skill under `subject_field_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpinionAnnouncement {
    /// Deterministic opinion ID: `blake2b(author_address + video_cid)`.
    pub opinion_id: String,
    /// Cardano stake address of the author.
    pub author_address: String,
    /// Subject field the opinion is scoped to.
    pub subject_field_id: String,
    /// Title (free text).
    pub title: String,
    /// Optional summary (<= `MAX_SUMMARY_CHARS`).
    pub summary: Option<String>,
    /// BLAKE3 hash of the video blob on iroh.
    pub video_cid: String,
    /// Optional BLAKE3 hash of a thumbnail image.
    pub thumbnail_cid: Option<String>,
    /// Duration of the video in seconds.
    pub duration_seconds: Option<i64>,
    /// IDs of the author's skill proofs that qualify them to post.
    /// At least one must correspond to a skill under `subject_field_id`.
    pub credential_proof_ids: Vec<String>,
    /// Unix timestamp of publication.
    pub published_at: i64,
    /// Ed25519 signature over the payload (hex, 128 chars).
    pub signature: String,
    /// Ed25519 public key of the signer (hex, 64 chars).
    pub public_key: String,
}

impl OpinionAnnouncement {
    /// Fields that are signed — everything except `signature` and
    /// `public_key`. The canonical JSON of this struct is what the
    /// signature covers. Using a distinct type avoids accidentally
    /// signing over the signature field itself.
    pub fn payload(&self) -> OpinionPayload {
        OpinionPayload {
            opinion_id: self.opinion_id.clone(),
            author_address: self.author_address.clone(),
            subject_field_id: self.subject_field_id.clone(),
            title: self.title.clone(),
            summary: self.summary.clone(),
            video_cid: self.video_cid.clone(),
            thumbnail_cid: self.thumbnail_cid.clone(),
            duration_seconds: self.duration_seconds,
            credential_proof_ids: self.credential_proof_ids.clone(),
            published_at: self.published_at,
        }
    }
}

/// The unsigned portion of an opinion — this is what the Ed25519
/// signature covers. Kept as a separate type so the signing/verifying
/// code never accidentally includes the signature in its own input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpinionPayload {
    pub opinion_id: String,
    pub author_address: String,
    pub subject_field_id: String,
    pub title: String,
    pub summary: Option<String>,
    pub video_cid: String,
    pub thumbnail_cid: Option<String>,
    pub duration_seconds: Option<i64>,
    pub credential_proof_ids: Vec<String>,
    pub published_at: i64,
}

/// A row in the local `opinions` SQLite table (materialised state).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpinionRow {
    pub id: String,
    pub author_address: String,
    pub subject_field_id: String,
    pub title: String,
    pub summary: Option<String>,
    pub video_cid: String,
    pub thumbnail_cid: Option<String>,
    pub duration_seconds: Option<i64>,
    pub credential_proof_ids: Vec<String>,
    pub signature: String,
    pub public_key: Option<String>,
    pub published_at: String,
    pub received_at: String,
    pub withdrawn: bool,
    pub withdrawn_reason: Option<String>,
    pub on_chain_tx: Option<String>,
    /// Where this opinion came from. `"ai_generated"` marks seeded
    /// example content; `None` means user-created. Added in migration 031.
    #[serde(default)]
    pub provenance: Option<String>,
}

/// Request payload for `publish_opinion` — what the frontend sends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishOpinionRequest {
    pub subject_field_id: String,
    pub title: String,
    pub summary: Option<String>,
    pub video_cid: String,
    pub thumbnail_cid: Option<String>,
    pub duration_seconds: Option<i64>,
    /// Which of the author's existing skill proofs they want to stake
    /// on this opinion. Must be non-empty; at least one must be under
    /// `subject_field_id`.
    pub credential_proof_ids: Vec<String>,
}

/// DAO-signed withdrawal record — propagated via governance gossip so
/// well-behaved nodes flip `opinions.withdrawn=1` and unpin the video.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpinionWithdrawal {
    pub opinion_id: String,
    pub dao_id: String,
    /// One of: `"challenge_upheld"`, `"author_request"`,
    /// `"policy_violation"`.
    pub reason: String,
    /// Signature from the subject-field DAO committee.
    pub dao_signature: String,
    pub withdrawn_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_announcement() -> OpinionAnnouncement {
        OpinionAnnouncement {
            opinion_id: "op_test_001".into(),
            author_address: "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu85qsqfy".into(),
            subject_field_id: "sf_cs".into(),
            title: "Why functional-first is the wrong default for CS1".into(),
            summary: Some("A counter-take to Racket-first curricula.".into()),
            video_cid: "a".repeat(64),
            thumbnail_cid: None,
            duration_seconds: Some(420),
            credential_proof_ids: vec!["proof_001".into()],
            published_at: 1_700_000_000,
            signature: "deadbeef".into(),
            public_key: "cafebabe".into(),
        }
    }

    #[test]
    fn payload_excludes_signature_fields() {
        let ann = sample_announcement();
        let payload = ann.payload();
        // The payload type has no signature/public_key fields — confirmed
        // by the compiler. This test asserts the round-trip carries the
        // signing-relevant fields unchanged.
        assert_eq!(payload.opinion_id, "op_test_001");
        assert_eq!(payload.author_address, ann.author_address);
        assert_eq!(payload.video_cid, ann.video_cid);
        assert_eq!(payload.credential_proof_ids, vec!["proof_001".to_string()]);
    }

    #[test]
    fn announcement_serde_roundtrip() {
        let ann = sample_announcement();
        let json = serde_json::to_string(&ann).unwrap();
        let parsed: OpinionAnnouncement = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.opinion_id, ann.opinion_id);
        assert_eq!(parsed.signature, "deadbeef");
    }

    #[test]
    fn publish_request_serde_roundtrip() {
        let req = PublishOpinionRequest {
            subject_field_id: "sf_cs".into(),
            title: "My take".into(),
            summary: None,
            video_cid: "b".repeat(64),
            thumbnail_cid: None,
            duration_seconds: Some(300),
            credential_proof_ids: vec!["proof_a".into(), "proof_b".into()],
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: PublishOpinionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.title, "My take");
        assert_eq!(parsed.credential_proof_ids.len(), 2);
    }
}
