// SPDX-License-Identifier: AGPL-3.0-or-later
//! Storing a credential somebody else issued: verify it, then keep it.
//!
//! Every door a received credential comes through — a file or blob the
//! learner imports (`commands::import`), a credential an organisation issued
//! through Alexandria Cloud ([`super::collect`]) — ends here, so they all
//! verify the same way and store the same columns.
//!
//! Deliberately type-agnostic: it stores any valid VC and leaves policy about
//! *what a credential means* to the readers. This only decides whether it is
//! real.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::vc::{
    verify_credential_db, AcceptanceDecision, CredentialSubject, EntitlementClaim, RoleClaim,
    SkillClaim, VerifiableCredential, VerificationPolicy, STATUS_PURPOSE_REVOCATION,
};

/// What [`receive`] did with a credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Receipt {
    /// Verified and newly stored.
    Stored,
    /// Verified; a credential with this id was already stored.
    AlreadyPresent,
    /// The decision is pending, for these reasons (a status list missing, for
    /// instance). Nothing was stored.
    Pending(Vec<&'static str>),
    /// Verification refused it, for these reasons. Nothing was stored.
    Rejected(Vec<&'static str>),
}

/// The policy a received credential must satisfy.
///
/// `allowed_types` is empty on purpose, which means "no type constraint". This
/// is a general inbox: an entitlement, a membership role, a skill credential
/// from an institution are all legitimate things to be handed, and deciding
/// which ones *matter* belongs to whoever reads them later. Everything else is
/// strict — an expired, revoked, suspended or superseded credential is not
/// worth storing.
pub fn policy() -> VerificationPolicy {
    VerificationPolicy {
        reject_expired: true,
        require_integrity_anchor: false,
        allowed_types: vec![],
        reject_suspended: true,
        reject_superseded: true,
    }
}

/// The `claim_kind` column value for a subject, mirroring `Claim::kind_str`.
///
/// A received credential arrives in its on-disk W3C shape with the claim
/// inlined in the subject, so the discriminator has to be recovered from the
/// marker properties rather than read off a `Claim` enum. Order matters: the
/// markers are distinct (`skillId`, `role`, `entitlementPlan`), and anything
/// carrying none of them is a custom claim, which is legitimate.
fn claim_kind_of(subject: &CredentialSubject) -> &'static str {
    if SkillClaim::extract(subject).is_some() {
        "skill"
    } else if RoleClaim::extract(subject).is_some() {
        "role"
    } else if EntitlementClaim::extract(subject).is_some() {
        "entitlement"
    } else {
        "custom"
    }
}

/// Verify a credential and store it.
///
/// Verification happens **before** the insert, and a credential that is not
/// accepted is not stored. A credential that does not verify is not a
/// credential, and letting one land in the table would put the burden of
/// re-checking on every future reader — one of which would eventually forget.
///
/// Idempotent on the envelope `id`: receiving the same credential again is
/// [`Receipt::AlreadyPresent`], answered from the store before any
/// verification. Delivery channels retry.
///
/// Errors are for a credential with no envelope id or class, and for the
/// database failing; a verification outcome is never an error.
pub fn receive(conn: &Connection, vc: &VerifiableCredential, now: &str) -> Result<Receipt, String> {
    let credential_id = vc
        .id
        .clone()
        .ok_or_else(|| "credential has no envelope id".to_string())?;

    // A credential already held was verified when it arrived; a retry is
    // answered from the store, not by verifying it again.
    let already: Option<String> = conn
        .query_row(
            "SELECT id FROM credentials WHERE id = ?1",
            params![credential_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if already.is_some() {
        return Ok(Receipt::AlreadyPresent);
    }

    let result = verify_credential_db(conn, vc, now, &policy());
    let reasons = crate::credentials::bundle::rejection_reasons(&result);
    match result.acceptance_decision {
        AcceptanceDecision::Accept => {}
        AcceptanceDecision::Pending => return Ok(Receipt::Pending(reasons)),
        // Report the failed checks rather than a bare "invalid": a credential
        // refused as expired and one refused for a bad signature call for
        // very different responses from the user.
        AcceptanceDecision::Reject if reasons.is_empty() => {
            return Ok(Receipt::Rejected(vec!["rejected by policy"]))
        }
        AcceptanceDecision::Reject => return Ok(Receipt::Rejected(reasons)),
    }

    // Derive the denormalized columns from the credential body. Nothing here
    // is trusted input in its own right — every one of these is recomputed
    // from the payload whose signature was just verified.
    let claim_kind = claim_kind_of(&vc.credential_subject);
    let skill_id = SkillClaim::extract(&vc.credential_subject).map(|s| s.skill_id);
    let provenance = SkillClaim::extract(&vc.credential_subject)
        .and_then(|s| s.provenance)
        .map(|p| p.as_str().to_string());
    let credential_type = vc
        .type_
        .iter()
        .find(|t| t.as_str() != "VerifiableCredential")
        .cloned()
        .ok_or_else(|| "credential names no class beyond VerifiableCredential".to_string())?;
    // The denormalised slot is the revocation entry's; every entry stays in
    // the signed JSON, which is what verification reads.
    let (list_id, list_index) = match vc.status_entry(STATUS_PURPOSE_REVOCATION) {
        Some(s) => (
            Some(s.status_list_credential.clone()),
            Some(s.status_list_index.clone()),
        ),
        None => (None, None),
    };
    // Same derivation `issue_credential` uses, reused rather than reimplemented
    // so a received credential and a locally issued one hash identically.
    let integrity_hash = crate::domain::vc::integrity_hash_of(vc)?;
    let signed_json = serde_json::to_string(vc).map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO credentials \
         (id, issuer_did, subject_did, credential_type, claim_kind, skill_id, \
          issuance_date, expiration_date, signed_vc_json, integrity_hash, \
          status_list_id, status_list_index, provenance) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            credential_id,
            vc.issuer.as_str(),
            vc.credential_subject.id.as_str(),
            credential_type,
            claim_kind,
            skill_id,
            vc.valid_from,
            vc.valid_until,
            signed_json,
            integrity_hash,
            list_id,
            list_index,
            provenance,
        ],
    )
    .map_err(|e| format!("insert received credential: {e}"))?;

    Ok(Receipt::Stored)
}
