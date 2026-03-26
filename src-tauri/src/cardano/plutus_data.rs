//! Plutus Data CBOR encoding for governance datums and redeemers.
//!
//! Mirrors the Aiken types from `cardano/governance/lib/alexandria/types.ak`.
//! Uses pallas_codec::minicbor for manual Plutus Data encoding, following the pattern
//! established in `snapshot.rs::encode_reputation_datum`.
//!
//! Plutus Data encoding rules:
//! - `Constr(n, fields)` for n in 0..6 → CBOR tag (121+n) + array
//! - `Constr(n, fields)` for n >= 7 → CBOR tag 102 + [n, fields]
//! - ByteArray → CBOR bytes
//! - Int → CBOR integer
//! - List<T> → CBOR array
//! - Bool → Constr(1,[]) for True, Constr(0,[]) for False

use crate::cardano::tx_builder::TxBuildError;

// ---- Helpers ----

/// Encode a Plutus Data constructor with index 0-6.
fn begin_constr(
    encoder: &mut pallas_codec::minicbor::Encoder<&mut Vec<u8>>,
    tag: u8,
    num_fields: u64,
) -> Result<(), TxBuildError> {
    encoder
        .tag(pallas_codec::minicbor::data::Tag::new(121 + tag as u64))
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    encoder
        .array(num_fields)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    Ok(())
}

/// Encode a Plutus Data Bool (True = Constr(1,[]), False = Constr(0,[])).
fn encode_bool(
    encoder: &mut pallas_codec::minicbor::Encoder<&mut Vec<u8>>,
    val: bool,
) -> Result<(), TxBuildError> {
    begin_constr(encoder, if val { 1 } else { 0 }, 0)
}

/// Encode a Plutus Data integer.
fn encode_int(
    encoder: &mut pallas_codec::minicbor::Encoder<&mut Vec<u8>>,
    val: i64,
) -> Result<(), TxBuildError> {
    encoder
        .i64(val)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    Ok(())
}

/// Encode Plutus Data bytes.
fn encode_bytes(
    encoder: &mut pallas_codec::minicbor::Encoder<&mut Vec<u8>>,
    val: &[u8],
) -> Result<(), TxBuildError> {
    encoder
        .bytes(val)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    Ok(())
}

// ---- Proficiency / Role enums ----

/// Map Bloom's proficiency level string to Plutus constructor index.
pub fn proficiency_to_tag(level: &str) -> u8 {
    match level {
        "remember" => 0,
        "understand" => 1,
        "apply" => 2,
        "analyze" => 3,
        "evaluate" => 4,
        "create" => 5,
        _ => 0,
    }
}

// ---- DaoDatum / DaoRedeemer ----

/// Encode a `DaoDatum` as Plutus Data CBOR.
///
/// Fields (10): scope, reputation_policy, membership_subjects,
/// min_membership_proficiency, state_token_policy, committee,
/// committee_size, election_interval, term_start, term_end
#[allow(clippy::too_many_arguments)]
pub fn encode_dao_datum(
    scope_type: &str,
    scope_id: &[u8],
    reputation_policy: &[u8; 28],
    membership_subjects: &[&[u8]],
    min_membership_proficiency: &str,
    state_token_policy: &[u8; 28],
    committee: &[&[u8; 28]],
    committee_size: i64,
    election_interval_ms: i64,
    term_start_ms: i64,
    term_end_ms: i64,
) -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    let mut encoder = pallas_codec::minicbor::Encoder::new(&mut buf);

    begin_constr(&mut encoder, 0, 10)?;

    // Field 0: scope (DaoScope)
    let scope_tag: u8 = if scope_type == "subject_field" { 0 } else { 1 };
    begin_constr(&mut encoder, scope_tag, 1)?;
    encode_bytes(&mut encoder, scope_id)?;

    // Field 1: reputation_policy (28 bytes)
    encode_bytes(&mut encoder, reputation_policy)?;

    // Field 2: membership_subjects (List<ByteArray>)
    encoder
        .array(membership_subjects.len() as u64)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    for subj in membership_subjects {
        encode_bytes(&mut encoder, subj)?;
    }

    // Field 3: min_membership_proficiency
    begin_constr(
        &mut encoder,
        proficiency_to_tag(min_membership_proficiency),
        0,
    )?;

    // Field 4: state_token_policy (28 bytes)
    encode_bytes(&mut encoder, state_token_policy)?;

    // Field 5: committee (List<VerificationKeyHash>)
    encoder
        .array(committee.len() as u64)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    for vkh in committee {
        encode_bytes(&mut encoder, *vkh)?;
    }

    // Fields 6-9: committee_size, election_interval, term_start, term_end
    encode_int(&mut encoder, committee_size)?;
    encode_int(&mut encoder, election_interval_ms)?;
    encode_int(&mut encoder, term_start_ms)?;
    encode_int(&mut encoder, term_end_ms)?;

    Ok(buf)
}

/// Encode a `DaoRedeemer` as Plutus Data CBOR.
pub fn encode_dao_redeemer(
    action: &str,
    election_ref: Option<(&[u8], u64)>,
) -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    let mut encoder = pallas_codec::minicbor::Encoder::new(&mut buf);

    match action {
        "create" => begin_constr(&mut encoder, 0, 0)?,
        "update" => begin_constr(&mut encoder, 1, 0)?,
        "install_committee" => {
            let (tx_hash, idx) = election_ref.ok_or_else(|| {
                TxBuildError::Cbor("install_committee requires election_ref".into())
            })?;
            begin_constr(&mut encoder, 2, 1)?;
            // OutputReference = Constr(0, [tx_id, idx])
            begin_constr(&mut encoder, 0, 2)?;
            // TransactionId = Constr(0, [hash])
            begin_constr(&mut encoder, 0, 1)?;
            encode_bytes(&mut encoder, tx_hash)?;
            encode_int(&mut encoder, idx as i64)?;
        }
        _ => {
            return Err(TxBuildError::Cbor(format!(
                "unknown dao redeemer: {action}"
            )))
        }
    }

    Ok(buf)
}

// ---- ElectionDatum / ElectionRedeemer ----

/// Encode an `ElectionDatum` as Plutus Data CBOR.
///
/// Fields (13): dao_state_token_policy, dao_state_token_name, election_id,
/// phase, seats, nominee_min_proficiency, voter_min_proficiency,
/// membership_subjects, reputation_policy, nominees, nomination_end,
/// voting_end, vote_receipt_policy
#[allow(clippy::too_many_arguments)]
pub fn encode_election_datum(
    dao_policy: &[u8; 28],
    dao_token_name: &[u8],
    election_id: i64,
    phase: &str,
    seats: i64,
    nominee_min_proficiency: &str,
    voter_min_proficiency: &str,
    membership_subjects: &[&[u8]],
    reputation_policy: &[u8; 28],
    nominees: &[(&[u8; 28], bool)],
    nomination_end_ms: i64,
    voting_end_ms: i64,
    vote_receipt_policy: &[u8; 28],
) -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    let mut encoder = pallas_codec::minicbor::Encoder::new(&mut buf);

    begin_constr(&mut encoder, 0, 13)?;

    encode_bytes(&mut encoder, dao_policy)?;
    encode_bytes(&mut encoder, dao_token_name)?;
    encode_int(&mut encoder, election_id)?;

    // phase: Nomination=0, Voting=1, Finalized=2
    let phase_tag: u8 = match phase {
        "nomination" => 0,
        "voting" => 1,
        "finalized" => 2,
        _ => 0,
    };
    begin_constr(&mut encoder, phase_tag, 0)?;

    encode_int(&mut encoder, seats)?;
    begin_constr(&mut encoder, proficiency_to_tag(nominee_min_proficiency), 0)?;
    begin_constr(&mut encoder, proficiency_to_tag(voter_min_proficiency), 0)?;

    // membership_subjects
    encoder
        .array(membership_subjects.len() as u64)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    for subj in membership_subjects {
        encode_bytes(&mut encoder, subj)?;
    }

    encode_bytes(&mut encoder, reputation_policy)?;

    // nominees: List<Nominee>
    encoder
        .array(nominees.len() as u64)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    for (key_hash, accepted) in nominees {
        begin_constr(&mut encoder, 0, 2)?;
        encode_bytes(&mut encoder, *key_hash)?;
        encode_bool(&mut encoder, *accepted)?;
    }

    encode_int(&mut encoder, nomination_end_ms)?;
    encode_int(&mut encoder, voting_end_ms)?;
    encode_bytes(&mut encoder, vote_receipt_policy)?;

    Ok(buf)
}

/// Encode an `ElectionRedeemer` as Plutus Data CBOR.
pub fn encode_election_redeemer(action: &str, extra: Option<i64>) -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    let mut encoder = pallas_codec::minicbor::Encoder::new(&mut buf);

    match action {
        "open" => begin_constr(&mut encoder, 0, 0)?,
        "accept_nomination" => {
            let idx = extra.ok_or_else(|| {
                TxBuildError::Cbor("accept_nomination requires nominee_index".into())
            })?;
            begin_constr(&mut encoder, 1, 1)?;
            encode_int(&mut encoder, idx)?;
        }
        "start_voting" => begin_constr(&mut encoder, 2, 0)?,
        "finalize" => begin_constr(&mut encoder, 3, 0)?,
        _ => {
            return Err(TxBuildError::Cbor(format!(
                "unknown election redeemer: {action}"
            )))
        }
    }

    Ok(buf)
}

// ---- ProposalDatum / ProposalRedeemer ----

/// Encode a `ProposalDatum` as Plutus Data CBOR.
///
/// Fields (14): dao_state_token_policy, dao_state_token_name, proposal_id,
/// author, status, category, min_vote_proficiency, membership_subjects,
/// reputation_policy, content_cid, voting_deadline, votes_for,
/// votes_against, vote_receipt_policy
#[allow(clippy::too_many_arguments)]
pub fn encode_proposal_datum(
    dao_policy: &[u8; 28],
    dao_token_name: &[u8],
    proposal_id: i64,
    author: &[u8; 28],
    status: &str,
    category: &str,
    min_vote_proficiency: &str,
    membership_subjects: &[&[u8]],
    reputation_policy: &[u8; 28],
    content_cid: &[u8],
    voting_deadline_ms: i64,
    votes_for: i64,
    votes_against: i64,
    vote_receipt_policy: &[u8; 28],
) -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    let mut encoder = pallas_codec::minicbor::Encoder::new(&mut buf);

    begin_constr(&mut encoder, 0, 14)?;

    encode_bytes(&mut encoder, dao_policy)?;
    encode_bytes(&mut encoder, dao_token_name)?;
    encode_int(&mut encoder, proposal_id)?;
    encode_bytes(&mut encoder, author)?;

    // status: Draft=0, Published=1, Approved=2, Rejected=3, Cancelled=4
    let status_tag: u8 = match status {
        "draft" => 0,
        "published" => 1,
        "approved" => 2,
        "rejected" => 3,
        "cancelled" => 4,
        _ => 0,
    };
    begin_constr(&mut encoder, status_tag, 0)?;

    // category: CurriculumChange=0, AssessmentStandards=1, PolicyChange=2,
    // TreasuryAllocation=3(recipient,amount), GeneralMotion=4
    let cat_tag: u8 = match category {
        "curriculum_change" | "curriculum" => 0,
        "assessment_standards" | "assessment" => 1,
        "policy_change" | "policy" => 2,
        "treasury_allocation" | "treasury" => 3,
        "general_motion" | "general" | "governance" | "technical" | "other" => 4,
        _ => 4,
    };
    begin_constr(&mut encoder, cat_tag, 0)?;

    begin_constr(&mut encoder, proficiency_to_tag(min_vote_proficiency), 0)?;

    // membership_subjects
    encoder
        .array(membership_subjects.len() as u64)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    for subj in membership_subjects {
        encode_bytes(&mut encoder, subj)?;
    }

    encode_bytes(&mut encoder, reputation_policy)?;
    encode_bytes(&mut encoder, content_cid)?;
    encode_int(&mut encoder, voting_deadline_ms)?;
    encode_int(&mut encoder, votes_for)?;
    encode_int(&mut encoder, votes_against)?;
    encode_bytes(&mut encoder, vote_receipt_policy)?;

    Ok(buf)
}

/// Encode a `ProposalRedeemer` as Plutus Data CBOR.
pub fn encode_proposal_redeemer(
    action: &str,
    vote_for: Option<bool>,
) -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    let mut encoder = pallas_codec::minicbor::Encoder::new(&mut buf);

    match action {
        "submit_draft" => begin_constr(&mut encoder, 0, 0)?,
        "approve" => begin_constr(&mut encoder, 1, 0)?,
        "cancel" => begin_constr(&mut encoder, 2, 0)?,
        "vote" => {
            let in_favor =
                vote_for.ok_or_else(|| TxBuildError::Cbor("vote requires vote_for".into()))?;
            begin_constr(&mut encoder, 3, 1)?;
            encode_bool(&mut encoder, in_favor)?;
        }
        "resolve" => begin_constr(&mut encoder, 4, 0)?,
        _ => {
            return Err(TxBuildError::Cbor(format!(
                "unknown proposal redeemer: {action}"
            )))
        }
    }

    Ok(buf)
}

// ---- Vote Receipt Redeemer ----

/// Encode a `VoteReceiptRedeemer` as Plutus Data CBOR.
pub fn encode_vote_receipt_redeemer(action: &str) -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    let mut encoder = pallas_codec::minicbor::Encoder::new(&mut buf);

    match action {
        "mint" => begin_constr(&mut encoder, 0, 0)?,
        "burn" => begin_constr(&mut encoder, 1, 0)?,
        _ => {
            return Err(TxBuildError::Cbor(format!(
                "unknown vote receipt redeemer: {action}"
            )))
        }
    }

    Ok(buf)
}

// ---- Soulbound Redeemer ----

/// Encode a `SoulboundRedeemer` as Plutus Data CBOR.
pub fn encode_soulbound_redeemer(action: &str) -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    let mut encoder = pallas_codec::minicbor::Encoder::new(&mut buf);

    match action {
        "update" => begin_constr(&mut encoder, 0, 0)?,
        "revoke" => begin_constr(&mut encoder, 1, 0)?,
        _ => {
            return Err(TxBuildError::Cbor(format!(
                "unknown soulbound redeemer: {action}"
            )))
        }
    }

    Ok(buf)
}

// ---- Reputation Minting Redeemer ----

/// Encode a `ReputationMintRedeemer` as Plutus Data CBOR.
pub fn encode_reputation_mint_redeemer(action: &str) -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    let mut encoder = pallas_codec::minicbor::Encoder::new(&mut buf);

    match action {
        "mint" => begin_constr(&mut encoder, 0, 0)?,
        "burn" => begin_constr(&mut encoder, 1, 0)?,
        _ => {
            return Err(TxBuildError::Cbor(format!(
                "unknown reputation mint redeemer: {action}"
            )))
        }
    }

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_dao_redeemer_create() {
        let bytes = encode_dao_redeemer("create", None).unwrap();
        // Constr(0, []) = tag 121 + empty array
        assert!(!bytes.is_empty());
        // First byte should be tag marker (0xd8 = tag follows, 0x79 = 121)
        assert_eq!(bytes[0], 0xd8);
        assert_eq!(bytes[1], 0x79); // 121
    }

    #[test]
    fn encode_proposal_redeemer_vote() {
        let bytes = encode_proposal_redeemer("vote", Some(true)).unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_election_redeemer_accept() {
        let bytes = encode_election_redeemer("accept_nomination", Some(3)).unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn proficiency_mapping() {
        assert_eq!(proficiency_to_tag("remember"), 0);
        assert_eq!(proficiency_to_tag("apply"), 2);
        assert_eq!(proficiency_to_tag("create"), 5);
    }
}
