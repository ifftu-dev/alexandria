//! Access grants: an organisation asks to see some of a learner's skills, the
//! learner decides, and the learner can take it back.
//!
//! Three messages:
//!
//! * [`AccessRequest`] — sent by the organisation, carried with an
//!   [`crate::org::OrgProof`] so the learner can see who is really asking.
//! * [`GrantStatement`] — signed by the learner. `grant` answers a request,
//!   `update` changes whether the grant keeps itself in sync, and `revoke`
//!   ends it. The organisation's service holds the latest one it has
//!   accepted; a revoke obliges it to delete what the grant let it hold.
//! * A [`crate::talent::SignedTalentIndexRecord`] narrowed to the granted
//!   skills — the snapshot itself. With `sync` on, the learner's app sends a
//!   fresh one whenever those skills change.
//!
//! Statements are signed the way hiring answers are: Ed25519 over
//! `GRANT_FORMAT || 0x00 || JCS(statement)`, keyed by the learner's own
//! `did:key`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

pub const GRANT_FORMAT: &str = "alexandria-grant/1";

/// How far a statement's `created` may sit from the verifier's clock.
pub const STATEMENT_SKEW_SECS: i64 = 300;

/// The most skills one request may ask for.
pub const MAX_SKILLS: usize = 64;

const MAX_STATEMENT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AccessRequest {
    pub id: String,
    /// [`crate::org::audience`] of the asking organisation.
    pub audience: String,
    pub nonce: String,
    /// The asking organisation's published name.
    pub organization: String,
    pub subject_did: String,
    pub skill_ids: Vec<String>,
    pub purpose: String,
    pub network_id: String,
    pub created_at: i64,
    pub expires_at: i64,
    /// Learning records asked for, beside or instead of skills. Omitted from
    /// the JSON when absent, so a request without it keeps its bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub records: Option<RecordScope>,
}

/// The kinds of learning record a share can cover.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RecordKind {
    /// Launch, progress, completion, pass, fail and score.
    Completion,
    /// The individual responses a package reported. Only ever beside
    /// `completion`, and only when the learner chose it.
    Interaction,
}

/// The xAPI account (`homePage`, `name`) statements under a share name the
/// learner by.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecordAccount {
    pub home_page: String,
    pub name: String,
}

/// A record share: which courses' learning records, of which kinds.
///
/// In a request, `account` is the identifier the organisation proposes (its
/// own systems' id for the learner), or absent. In a grant it is required
/// and is the account the records will carry: the proposed one if the
/// learner approved it, otherwise the one the learner's app derived for this
/// organisation (or the learner's own DID, if they chose that).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecordScope {
    pub course_ids: Vec<String>,
    pub kinds: Vec<RecordKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<RecordAccount>,
}

/// The most courses one record scope may name.
pub const MAX_COURSES: usize = 64;

/// The longest account name (the longest any tested xAPI store keeps).
pub const MAX_ACCOUNT_NAME_LEN: usize = 50;

/// The longest account home page (the xAPI IRI cap Alexandria uses).
pub const MAX_ACCOUNT_HOME_PAGE_LEN: usize = 2083;

impl RecordScope {
    /// Shape only: 1 to [`MAX_COURSES`] distinct, non-empty course ids;
    /// distinct kinds including `completion`; a well-formed account if any.
    pub fn validate(&self) -> Result<(), AccessError> {
        let bad = |what: &str| Err(AccessError::Malformed(format!("record scope: {what}")));
        if self.course_ids.is_empty() {
            return bad("names no course");
        }
        if self.course_ids.len() > MAX_COURSES {
            return bad("too many courses");
        }
        if self
            .course_ids
            .iter()
            .any(|id| id.trim().is_empty() || id.len() > 128)
        {
            return bad("course id");
        }
        let mut seen = std::collections::HashSet::new();
        if !self.course_ids.iter().all(|id| seen.insert(id)) {
            return bad("duplicate course");
        }
        let mut kinds = std::collections::HashSet::new();
        if !self.kinds.iter().all(|k| kinds.insert(k)) {
            return bad("duplicate kind");
        }
        if !kinds.contains(&RecordKind::Completion) {
            return bad("completion records are always part of a share");
        }
        if let Some(account) = &self.account {
            if account.home_page.trim().is_empty()
                || account.home_page.len() > MAX_ACCOUNT_HOME_PAGE_LEN
                || account.home_page.chars().any(char::is_control)
            {
                return bad("account home page");
            }
            if account.name.trim().is_empty()
                || account.name.chars().count() > MAX_ACCOUNT_NAME_LEN
                || account.name.chars().any(char::is_control)
            {
                return bad("account name");
            }
        }
        Ok(())
    }

    /// Whether every course and kind here is also in `wider`.
    pub fn within(&self, wider: &RecordScope) -> bool {
        self.course_ids.iter().all(|c| wider.course_ids.contains(c))
            && self.kinds.iter().all(|k| wider.kinds.contains(k))
    }

    pub fn includes(&self, kind: RecordKind) -> bool {
        self.kinds.contains(&kind)
    }
}

/// Check a request's shape: it asks for 1 to [`MAX_SKILLS`] distinct skills,
/// a well-formed record scope, or both.
pub fn validate_request(request: &AccessRequest) -> Result<(), AccessError> {
    if request.skill_ids.is_empty() && request.records.is_none() {
        return Err(AccessError::Malformed(
            "asks for no skills and no records".into(),
        ));
    }
    if request.skill_ids.len() > MAX_SKILLS {
        return Err(AccessError::Malformed("too many skills".into()));
    }
    let mut seen = std::collections::HashSet::new();
    if !request.skill_ids.iter().all(|id| seen.insert(id)) {
        return Err(AccessError::Malformed("duplicate skill".into()));
    }
    match &request.records {
        Some(scope) => scope.validate(),
        None => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GrantAction {
    Grant,
    Update,
    Revoke,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GrantStatement {
    pub format: String,
    pub action: GrantAction,
    /// Chosen by the learner when granting; the same id on update and revoke.
    pub grant_id: String,
    /// The request a `grant` answers.
    pub request_id: String,
    pub audience: String,
    /// The request's nonce, so a grant cannot be replayed onto another request.
    pub nonce: String,
    pub subject_did: String,
    /// What the organisation may see. A subset of what it asked for.
    pub skill_ids: Vec<String>,
    /// Whether the learner's app keeps the snapshot current.
    pub sync: bool,
    /// Unix seconds.
    pub created: i64,
    /// The learning records the organisation may receive. A subset of what
    /// it asked for; omitted from the JSON when absent. A revoke carries
    /// none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub records: Option<RecordScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SignedGrantStatement {
    pub statement: GrantStatement,
    /// base64url Ed25519 signature over [`signing_bytes`].
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessError {
    Malformed(String),
    BadSignature,
    Stale,
    /// The statement does not answer this request.
    Mismatch(&'static str),
    Expired,
    /// The snapshot is not the subject's, or shows skills the grant does not cover.
    Snapshot(String),
}

impl std::fmt::Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(e) => write!(f, "malformed grant: {e}"),
            Self::BadSignature => write!(f, "grant signature does not verify"),
            Self::Stale => write!(f, "grant was signed too long ago or in the future"),
            Self::Mismatch(what) => write!(f, "grant does not answer this request: {what}"),
            Self::Expired => write!(f, "the request has expired"),
            Self::Snapshot(e) => write!(f, "snapshot rejected: {e}"),
        }
    }
}

impl std::error::Error for AccessError {}

pub fn signing_bytes(statement: &GrantStatement) -> Result<Vec<u8>, AccessError> {
    let mut bytes = GRANT_FORMAT.as_bytes().to_vec();
    bytes.push(0);
    bytes.extend(
        serde_json_canonicalizer::to_vec(statement)
            .map_err(|e| AccessError::Malformed(e.to_string()))?,
    );
    if bytes.len() > MAX_STATEMENT_BYTES {
        return Err(AccessError::Malformed("statement exceeds 64 KiB".into()));
    }
    Ok(bytes)
}

/// Sign a statement with the learner's key. Refuses a key that is not the
/// subject's, for the same reason [`crate::talent::sign_record`] does.
pub fn sign(
    statement: &GrantStatement,
    key: &ed25519_dalek::SigningKey,
) -> Result<SignedGrantStatement, AccessError> {
    use ed25519_dalek::Signer;
    if crate::did::derive_did_key(key).as_str() != statement.subject_did {
        return Err(AccessError::Malformed(
            "signing key is not the subject's".into(),
        ));
    }
    let sig = key.sign(&signing_bytes(statement)?);
    Ok(SignedGrantStatement {
        statement: statement.clone(),
        signature: URL_SAFE_NO_PAD.encode(sig.to_bytes()),
    })
}

/// Check a statement's shape, freshness and signature. `now` is unix seconds.
pub fn verify(signed: &SignedGrantStatement, now: i64) -> Result<(), AccessError> {
    use ed25519_dalek::Verifier;
    let s = &signed.statement;
    if s.format != GRANT_FORMAT {
        return Err(AccessError::Malformed("unknown format".into()));
    }
    if s.grant_id.trim().is_empty() || s.grant_id.len() > 128 {
        return Err(AccessError::Malformed("grant id".into()));
    }
    if s.action != GrantAction::Revoke && s.skill_ids.is_empty() && s.records.is_none() {
        return Err(AccessError::Malformed(
            "grants no skills and no records".into(),
        ));
    }
    if let Some(scope) = &s.records {
        scope.validate()?;
        if scope.account.is_none() {
            return Err(AccessError::Malformed(
                "record scope: a grant names the account its records carry".into(),
            ));
        }
    }
    if s.skill_ids.len() > MAX_SKILLS {
        return Err(AccessError::Malformed("too many skills".into()));
    }
    let mut seen = std::collections::HashSet::new();
    if !s.skill_ids.iter().all(|id| seen.insert(id)) {
        return Err(AccessError::Malformed("duplicate skill".into()));
    }
    if (s.created - now).abs() > STATEMENT_SKEW_SECS {
        return Err(AccessError::Stale);
    }
    let key = crate::did::resolve_did_key(&crate::did::Did(s.subject_did.clone()))
        .map_err(|_| AccessError::Malformed("subject is not a did:key".into()))?;
    let sig: [u8; 64] = URL_SAFE_NO_PAD
        .decode(&signed.signature)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| AccessError::Malformed("signature encoding".into()))?;
    key.verify(
        &signing_bytes(s)?,
        &ed25519_dalek::Signature::from_bytes(&sig),
    )
    .map_err(|_| AccessError::BadSignature)
}

/// Check that a `grant` statement answers `request`. `now` is unix seconds.
pub fn check_answers(
    request: &AccessRequest,
    statement: &GrantStatement,
    now: i64,
) -> Result<(), AccessError> {
    if statement.action != GrantAction::Grant {
        return Err(AccessError::Mismatch("not a grant"));
    }
    if statement.request_id != request.id {
        return Err(AccessError::Mismatch("request id"));
    }
    if statement.audience != request.audience {
        return Err(AccessError::Mismatch("audience"));
    }
    if statement.nonce != request.nonce {
        return Err(AccessError::Mismatch("nonce"));
    }
    if statement.subject_did != request.subject_did {
        return Err(AccessError::Mismatch("subject"));
    }
    if !statement
        .skill_ids
        .iter()
        .all(|id| request.skill_ids.contains(id))
    {
        return Err(AccessError::Mismatch(
            "grants a skill that was not asked for",
        ));
    }
    // The learner may share fewer courses and kinds than were asked for,
    // never more. The account is the learner's choice: the one proposed, or
    // their own; nothing here can tell a derived one from any other.
    match (&statement.records, &request.records) {
        (None, _) => {}
        (Some(_), None) => return Err(AccessError::Mismatch("records were not asked for")),
        (Some(granted), Some(asked)) if !granted.within(asked) => {
            return Err(AccessError::Mismatch(
                "shares records that were not asked for",
            ))
        }
        _ => {}
    }
    if now >= request.expires_at {
        return Err(AccessError::Expired);
    }
    Ok(())
}

/// Check that a later statement belongs to the grant `current` describes.
pub fn check_follows(current: &GrantStatement, next: &GrantStatement) -> Result<(), AccessError> {
    if next.action == GrantAction::Grant {
        return Err(AccessError::Mismatch("a grant cannot be granted twice"));
    }
    if next.grant_id != current.grant_id
        || next.request_id != current.request_id
        || next.audience != current.audience
        || next.nonce != current.nonce
        || next.subject_did != current.subject_did
    {
        return Err(AccessError::Mismatch("belongs to another grant"));
    }
    if next.created < current.created {
        return Err(AccessError::Stale);
    }
    if next.action == GrantAction::Update
        && !next
            .skill_ids
            .iter()
            .all(|id| current.skill_ids.contains(id))
    {
        return Err(AccessError::Mismatch("an update cannot widen a grant"));
    }
    if next.action == GrantAction::Update {
        match (&next.records, &current.records) {
            (None, _) => {}
            (Some(_), None) => return Err(AccessError::Mismatch("an update cannot widen a grant")),
            (Some(narrowed), Some(held)) => {
                if !narrowed.within(held) {
                    return Err(AccessError::Mismatch("an update cannot widen a grant"));
                }
                // Records already sent carry the account; a new one would
                // split the learner in two at the organisation.
                if narrowed.account != held.account {
                    return Err(AccessError::Mismatch(
                        "an update cannot change the account records carry",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Check a snapshot against the grant it is sent under.
pub fn check_snapshot(
    grant: &GrantStatement,
    snapshot: &crate::talent::SignedTalentIndexRecord,
    verification_time: &str,
) -> Result<(), AccessError> {
    crate::talent::verify_record(snapshot, verification_time)
        .map_err(|e| AccessError::Snapshot(format!("{e:?}")))?;
    if snapshot.record.subject_did != grant.subject_did {
        return Err(AccessError::Snapshot("not the grant's subject".into()));
    }
    if let Some(extra) = snapshot
        .record
        .skills
        .iter()
        .find(|s| !grant.skill_ids.contains(&s.skill_id))
    {
        return Err(AccessError::Snapshot(format!(
            "{} is not covered by the grant",
            extra.skill_id
        )));
    }
    if snapshot.record.bio.is_some() {
        return Err(AccessError::Snapshot(
            "a grant does not cover the bio".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    const NOW: i64 = 1_791_000_000;

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn did() -> String {
        crate::did::derive_did_key(&key()).as_str().to_string()
    }

    fn request() -> AccessRequest {
        AccessRequest {
            id: "req-1".into(),
            audience: crate::org::audience("alexandria.ifftu.dev"),
            nonce: "n-1".into(),
            organization: "IFFTU".into(),
            subject_did: did(),
            skill_ids: vec!["skill_big_o".into(), "skill_javascript".into()],
            purpose: "Engineering Manager".into(),
            network_id: "preprod".into(),
            created_at: NOW - 60,
            expires_at: NOW + 86_400,
            records: None,
        }
    }

    fn grant(skills: &[&str]) -> GrantStatement {
        let r = request();
        GrantStatement {
            format: GRANT_FORMAT.into(),
            action: GrantAction::Grant,
            grant_id: "grant-1".into(),
            request_id: r.id,
            audience: r.audience,
            nonce: r.nonce,
            subject_did: did(),
            skill_ids: skills.iter().map(|s| s.to_string()).collect(),
            sync: true,
            created: NOW,
            records: None,
        }
    }

    const COURSE_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const COURSE_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const COURSE_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn hr_account() -> RecordAccount {
        RecordAccount {
            home_page: "https://hr.example.org".into(),
            name: "E-1042".into(),
        }
    }

    fn pairwise_account() -> RecordAccount {
        RecordAccount {
            home_page: "urn:uuid:044eac38-2871-47c7-a8d8-5c25a8a9479c".into(),
            name: "z6MkpairwiseIdentifierMadeOnlyForThisOrganisatio".into(),
        }
    }

    fn records(
        courses: &[&str],
        kinds: &[RecordKind],
        account: Option<RecordAccount>,
    ) -> RecordScope {
        RecordScope {
            course_ids: courses.iter().map(|c| c.to_string()).collect(),
            kinds: kinds.to_vec(),
            account,
        }
    }

    /// An organisation asks for completion and per-answer records from two
    /// courses, proposing its HR identifier, and no skills.
    fn records_request() -> AccessRequest {
        AccessRequest {
            skill_ids: Vec::new(),
            records: Some(records(
                &[COURSE_A, COURSE_B],
                &[RecordKind::Completion, RecordKind::Interaction],
                Some(hr_account()),
            )),
            ..request()
        }
    }

    fn records_grant(scope: RecordScope) -> GrantStatement {
        GrantStatement {
            skill_ids: Vec::new(),
            records: Some(scope),
            ..grant(&[])
        }
    }

    fn malformed(statement: &GrantStatement, now: i64) -> bool {
        matches!(
            verify(&sign(statement, &key()).unwrap(), now),
            Err(AccessError::Malformed(_))
        )
    }

    #[test]
    fn a_record_only_share_verifies_and_may_narrow_what_was_asked() {
        let request = records_request();
        validate_request(&request).unwrap();
        // One course, completion level only, under the identifier made for
        // this organisation rather than the one it proposed.
        let narrower = records_grant(records(
            &[COURSE_A],
            &[RecordKind::Completion],
            Some(pairwise_account()),
        ));
        verify(&sign(&narrower, &key()).unwrap(), NOW).unwrap();
        check_answers(&request, &narrower, NOW).unwrap();
        // Or everything, under the identifier the organisation proposed.
        let proposed = records_grant(records(
            &[COURSE_A, COURSE_B],
            &[RecordKind::Completion, RecordKind::Interaction],
            Some(hr_account()),
        ));
        check_answers(&request, &proposed, NOW).unwrap();
    }

    #[test]
    fn a_record_share_cannot_reach_past_what_was_asked() {
        let skills_only = request();
        let request = records_request();
        let wider = records_grant(records(
            &[COURSE_A, COURSE_C],
            &[RecordKind::Completion],
            Some(pairwise_account()),
        ));
        assert!(matches!(
            check_answers(&request, &wider, NOW),
            Err(AccessError::Mismatch(_))
        ));
        let completion_only = AccessRequest {
            records: Some(records(&[COURSE_A], &[RecordKind::Completion], None)),
            ..records_request()
        };
        let with_answers = records_grant(records(
            &[COURSE_A],
            &[RecordKind::Completion, RecordKind::Interaction],
            Some(pairwise_account()),
        ));
        assert!(matches!(
            check_answers(&completion_only, &with_answers, NOW),
            Err(AccessError::Mismatch(_))
        ));
        // A request for skills alone cannot be answered with records.
        let skills_and_records = GrantStatement {
            skill_ids: vec!["skill_big_o".into()],
            ..with_answers
        };
        assert!(matches!(
            check_answers(&skills_only, &skills_and_records, NOW),
            Err(AccessError::Mismatch(_))
        ));
    }

    #[test]
    fn a_grant_names_skills_or_records_and_a_record_scope_is_well_formed() {
        let nothing = GrantStatement {
            skill_ids: Vec::new(),
            ..grant(&[])
        };
        assert!(malformed(&nothing, NOW));
        for bad in [
            records(&[], &[RecordKind::Completion], Some(pairwise_account())),
            records(
                &[COURSE_A, COURSE_A],
                &[RecordKind::Completion],
                Some(pairwise_account()),
            ),
            records(&[COURSE_A], &[], Some(pairwise_account())),
            records(
                &[COURSE_A],
                &[RecordKind::Interaction],
                Some(pairwise_account()),
            ),
            records(
                &[COURSE_A],
                &[RecordKind::Completion, RecordKind::Completion],
                Some(pairwise_account()),
            ),
            // A grant says which account the records will carry.
            records(&[COURSE_A], &[RecordKind::Completion], None),
            records(
                &[COURSE_A],
                &[RecordKind::Completion],
                Some(RecordAccount {
                    home_page: String::new(),
                    name: "x".into(),
                }),
            ),
            records(
                &[COURSE_A],
                &[RecordKind::Completion],
                Some(RecordAccount {
                    home_page: "https://hr.example.org".into(),
                    name: "n".repeat(MAX_ACCOUNT_NAME_LEN + 1),
                }),
            ),
        ] {
            assert!(malformed(&records_grant(bad.clone()), NOW), "{bad:?}");
        }
        // A request names what it asks for, too; its account is optional.
        validate_request(&request()).unwrap();
        assert!(validate_request(&AccessRequest {
            records: None,
            ..records_request()
        })
        .is_err());
        assert!(validate_request(&AccessRequest {
            records: Some(records(&[], &[RecordKind::Completion], None)),
            ..records_request()
        })
        .is_err());
        validate_request(&AccessRequest {
            records: Some(records(&[COURSE_A], &[RecordKind::Completion], None)),
            ..records_request()
        })
        .unwrap();
    }

    #[test]
    fn a_record_share_narrows_on_update_and_keeps_its_identifier() {
        let current = records_grant(records(
            &[COURSE_A, COURSE_B],
            &[RecordKind::Completion, RecordKind::Interaction],
            Some(pairwise_account()),
        ));
        let narrow = GrantStatement {
            action: GrantAction::Update,
            created: NOW + 10,
            records: Some(records(
                &[COURSE_B],
                &[RecordKind::Completion],
                Some(pairwise_account()),
            )),
            ..current.clone()
        };
        check_follows(&current, &narrow).unwrap();
        verify(&sign(&narrow, &key()).unwrap(), NOW + 10).unwrap();

        let widen = GrantStatement {
            records: Some(records(
                &[COURSE_B, COURSE_C],
                &[RecordKind::Completion],
                Some(pairwise_account()),
            )),
            ..narrow.clone()
        };
        assert!(check_follows(&current, &widen).is_err());
        let more_kinds = GrantStatement {
            records: Some(records(
                &[COURSE_B],
                &[RecordKind::Completion, RecordKind::Interaction],
                Some(pairwise_account()),
            )),
            ..narrow.clone()
        };
        assert!(check_follows(&narrow, &more_kinds).is_err());
        let other_identifier = GrantStatement {
            records: Some(records(
                &[COURSE_B],
                &[RecordKind::Completion],
                Some(hr_account()),
            )),
            ..narrow.clone()
        };
        assert!(check_follows(&current, &other_identifier).is_err());

        // A skills grant cannot grow a record scope later.
        let skills = grant(&["skill_big_o"]);
        let grown = GrantStatement {
            action: GrantAction::Update,
            created: NOW + 10,
            records: Some(records(
                &[COURSE_A],
                &[RecordKind::Completion],
                Some(pairwise_account()),
            )),
            ..skills.clone()
        };
        assert!(check_follows(&skills, &grown).is_err());

        // A revoke carries no scope and ends the share.
        let revoke = GrantStatement {
            action: GrantAction::Revoke,
            created: NOW + 20,
            records: None,
            ..narrow.clone()
        };
        check_follows(&narrow, &revoke).unwrap();
        verify(&sign(&revoke, &key()).unwrap(), NOW + 20).unwrap();
    }

    #[test]
    fn a_statement_without_a_record_scope_signs_the_bytes_it_always_did() {
        // Statements made before record shares never carried the field;
        // leaving it out keeps their canonical bytes and their signatures.
        let bytes = signing_bytes(&grant(&["skill_big_o"])).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("records"));
    }

    #[test]
    fn a_signed_grant_verifies_and_answers_its_request() {
        let signed = sign(&grant(&["skill_big_o"]), &key()).unwrap();
        verify(&signed, NOW).unwrap();
        check_answers(&request(), &signed.statement, NOW).unwrap();
    }

    #[test]
    fn tampering_staleness_and_another_key_are_refused() {
        let mut signed = sign(&grant(&["skill_big_o"]), &key()).unwrap();
        assert_eq!(verify(&signed, NOW + 3_600), Err(AccessError::Stale));
        signed.statement.sync = false;
        assert_eq!(verify(&signed, NOW), Err(AccessError::BadSignature));
        let other = SigningKey::from_bytes(&[8; 32]);
        assert!(sign(&grant(&["skill_big_o"]), &other).is_err());
    }

    #[test]
    fn a_grant_cannot_reach_past_what_was_asked() {
        let wider = grant(&["skill_big_o", "skill_rust"]);
        assert!(matches!(
            check_answers(&request(), &wider, NOW),
            Err(AccessError::Mismatch(_))
        ));
        let mut elsewhere = grant(&["skill_big_o"]);
        elsewhere.nonce = "other".into();
        assert!(check_answers(&request(), &elsewhere, NOW).is_err());
        assert_eq!(
            check_answers(&request(), &grant(&["skill_big_o"]), NOW + 90_000),
            Err(AccessError::Expired)
        );
    }

    #[test]
    fn updates_and_revokes_follow_their_grant_and_cannot_widen_it() {
        let current = grant(&["skill_big_o"]);
        let mut update = current.clone();
        update.action = GrantAction::Update;
        update.sync = false;
        update.created = NOW + 10;
        check_follows(&current, &update).unwrap();

        let mut widen = update.clone();
        widen.skill_ids.push("skill_javascript".into());
        assert!(check_follows(&current, &widen).is_err());

        let mut revoke = update.clone();
        revoke.action = GrantAction::Revoke;
        revoke.skill_ids.clear();
        check_follows(&current, &revoke).unwrap();
        verify(&sign(&revoke, &key()).unwrap(), NOW + 10).unwrap();

        let mut foreign = revoke;
        foreign.grant_id = "grant-2".into();
        assert!(check_follows(&current, &foreign).is_err());
    }

    #[test]
    fn a_snapshot_shows_only_granted_skills() {
        use crate::talent::{sign_record, PublishedSkill, TalentIndexRecord};
        let skill = |id: &str| PublishedSkill {
            skill_id: id.into(),
            name: id.into(),
            bloom_level: 0,
            level: 5,
            issuer_clusters: 0,
        };
        let record = |skills: Vec<PublishedSkill>| TalentIndexRecord {
            subject_did: did(),
            skills,
            display_name: Some("Demo Learner".into()),
            bio: None,
            valid_until: "2027-01-01T00:00:00Z".into(),
        };
        let at = "2026-10-12T00:00:00Z";
        let g = grant(&["skill_big_o"]);
        let ok = sign_record(&record(vec![skill("skill_big_o")]), &key(), at).unwrap();
        check_snapshot(&g, &ok, at).unwrap();
        let extra = sign_record(
            &record(vec![skill("skill_big_o"), skill("skill_javascript")]),
            &key(),
            at,
        )
        .unwrap();
        assert!(check_snapshot(&g, &extra, at).is_err());
    }
}
