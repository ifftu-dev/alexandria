//! Adversarial regression tests for the founding-genesis wire format.
//!
//! Each case was first reproduced against the previous format: a DAO id that
//! hashed signature bytes, free-text member ids, invisible characters in
//! reviewed text, integers that JCS cannot encode exactly, and transport
//! spellings that must never verify. Only the public API is used, the same
//! surface a third-party verifier has.

use alexandria_verify::did::did_from_verifying_key;
use alexandria_verify::governance::*;
use ed25519_dalek::hazmat::{raw_sign, ExpandedSecretKey};
use ed25519_dalek::{Signer, SigningKey};

struct FounderKeys {
    identity: SigningKey,
    consensus: SigningKey,
    governance: SigningKey,
}

fn member_id(keys: &FounderKeys) -> String {
    did_from_verifying_key(&keys.identity.verifying_key()).0
}

fn founder_keys() -> Vec<FounderKeys> {
    let mut keys: Vec<_> = (0..GOVERNANCE_COMMITTEE_SIZE)
        .map(|index| FounderKeys {
            identity: SigningKey::from_bytes(&[20 + index as u8; 32]),
            consensus: SigningKey::from_bytes(&[40 + index as u8; 32]),
            governance: SigningKey::from_bytes(&[1 + index as u8; 32]),
        })
        .collect();
    keys.sort_by_key(member_id);
    keys
}

fn core_for(keys: &[FounderKeys]) -> FoundingGenesisCore {
    FoundingGenesisCore {
        genesis_version: GOVERNANCE_GENESIS_VERSION,
        protocol_version: GOVERNANCE_CERTIFICATE_VERSION,
        name: "Computing DAO".into(),
        scope: GenesisScope {
            scope_type: "subject".into(),
            scope_id: "computer-science".into(),
        },
        members: keys
            .iter()
            .map(|keys| GenesisMember {
                member_id: member_id(keys),
                identity_public_key_hex: hex::encode(keys.identity.verifying_key().to_bytes()),
                consensus_public_key_hex: hex::encode(keys.consensus.verifying_key().to_bytes()),
                governance_public_key_hex: hex::encode(keys.governance.verifying_key().to_bytes()),
            })
            .collect(),
        rules: GenesisRules {
            rules_version: "1".into(),
            committee_size: GOVERNANCE_COMMITTEE_SIZE as u8,
            receipt_threshold: GOVERNANCE_QUORUM as u8,
            outcome_threshold: GOVERNANCE_QUORUM as u8,
            proposal_approval_numerator: 2,
            proposal_approval_denominator: 3,
            minimum_turnout_count: 25,
        },
        qualification_policy: GenesisQualificationPolicy {
            policy_version: "1".into(),
            accepted_issuers: vec!["did:key:issuer-a".into(), "did:key:issuer-b".into()],
            accepted_assessment_evidence: vec!["assessment-credential".into()],
        },
        activation: GenesisActivation {
            cometbft_chain_id: "alexandria-computing-1".into(),
            initial_epoch: 0,
            initial_height: 1,
            activation_time_unix: 1_800_000_000,
        },
    }
}

fn sign_all(core: FoundingGenesisCore, keys: &[FounderKeys]) -> FoundingGenesisEnvelope {
    let bytes = core.acceptance_signing_bytes().unwrap();
    let acceptances = keys
        .iter()
        .map(|keys| FoundingAcceptance {
            member_id: member_id(keys),
            identity_signature_hex: hex::encode(keys.identity.sign(&bytes).to_bytes()),
            consensus_signature_hex: hex::encode(keys.consensus.sign(&bytes).to_bytes()),
            governance_signature_hex: hex::encode(keys.governance.sign(&bytes).to_bytes()),
        })
        .collect();
    FoundingGenesisEnvelope { core, acceptances }
}

fn genesis() -> (FoundingGenesisEnvelope, Vec<FounderKeys>) {
    let keys = founder_keys();
    (sign_all(core_for(&keys), &keys), keys)
}

fn canonical() -> (Vec<u8>, String) {
    let (envelope, _) = genesis();
    let id = envelope.verify(None).unwrap().dao_id().to_owned();
    (envelope.canonical_bytes().unwrap(), id)
}

fn replace_once(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains(from), "pattern {from} not found");
    text.replacen(from, to, 1).into_bytes()
}

/// A valid signature over the same bytes with a different deterministic
/// nonce. Ed25519 verification cannot tell it apart from the original.
fn resign(key: &SigningKey, message: &[u8]) -> String {
    let mut expanded = ExpandedSecretKey::from(&key.to_bytes());
    expanded.hash_prefix = [0xAA; 32];
    let signature = raw_sign::<sha2::Sha512>(&expanded, message, &key.verifying_key());
    hex::encode(signature.to_bytes())
}

#[test]
fn resigning_an_identical_core_cannot_mint_a_second_dao_id() {
    let (original, keys) = genesis();
    let verified = original.verify(None).unwrap();
    let bytes = original.core.acceptance_signing_bytes().unwrap();

    let mut resigned = original.clone();
    resigned.acceptances[3].governance_signature_hex = resign(&keys[3].governance, &bytes);
    resigned.acceptances[5].identity_signature_hex = resign(&keys[5].identity, &bytes);
    assert_ne!(resigned.acceptances, original.acceptances);

    let twin = resigned.verify(None).unwrap();
    assert_eq!(twin.dao_id(), verified.dao_id());
    assert_eq!(twin.genesis_hash(), verified.genesis_hash());
    assert_eq!(twin.committee(), verified.committee());
    assert_eq!(twin, verified);

    let original_bytes = original.canonical_bytes().unwrap();
    let resigned_bytes = resigned.canonical_bytes().unwrap();
    assert_ne!(original_bytes, resigned_bytes);
    for bytes in [original_bytes, resigned_bytes] {
        let (_, decoded) = decode_and_verify_genesis(&bytes, Some(verified.dao_id())).unwrap();
        assert_eq!(decoded.dao_id(), verified.dao_id());
    }
}

#[test]
fn a_stable_dao_id_still_requires_all_twenty_one_key_proofs() {
    let (original, _) = genesis();
    for index in 0..GOVERNANCE_COMMITTEE_SIZE {
        for field in 0..3 {
            let mut broken = original.clone();
            let acceptance = &mut broken.acceptances[index];
            let signature = match field {
                0 => &mut acceptance.identity_signature_hex,
                1 => &mut acceptance.consensus_signature_hex,
                _ => &mut acceptance.governance_signature_hex,
            };
            *signature = "00".repeat(64);
            assert_eq!(
                broken.verify(None),
                Err(GenesisError::InvalidAcceptances),
                "founder {index} key {field}"
            );
        }
    }

    let (mut six, _) = genesis();
    six.acceptances.pop();
    assert_eq!(six.verify(None), Err(GenesisError::InvalidAcceptances));
}

#[test]
fn a_genesis_cannot_claim_someone_elses_did_key() {
    let keys = founder_keys();
    let mut core = core_for(&keys);
    let victim = SigningKey::from_bytes(&[99; 32]);
    core.members[0].member_id = did_from_verifying_key(&victim.verifying_key()).0;
    core.members
        .sort_by(|left, right| left.member_id.cmp(&right.member_id));
    let error = core.acceptance_signing_bytes().unwrap_err();
    assert!(
        matches!(
            error,
            GenesisError::UnboundMemberId | GenesisError::InvalidMembers
        ),
        "{error:?}"
    );

    // A free-text label that keeps canonical member order still fails, on
    // the binding rather than on ordering.
    let mut label = core_for(&keys);
    label.members[GOVERNANCE_COMMITTEE_SIZE - 1].member_id = "did:key:zzVictimFounder".into();
    assert_eq!(label.validate(), Err(GenesisError::UnboundMemberId));
}

#[test]
fn display_spoofing_and_unbounded_labels_are_rejected() {
    let keys = founder_keys();
    for name in [
        "Computing\u{200B} DAO",
        "Computing DAO \u{202E}lanoiciffO",
        "Computing\u{2066} DAO\u{2069}",
        "Computing\u{00AD}DAO",
        "Computing\u{E0041}DAO",
        "Computing\nDAO",
    ] {
        let mut core = core_for(&keys);
        core.name = name.into();
        assert_eq!(
            core.validate(),
            Err(GenesisError::InvalidBinding),
            "{name:?}"
        );
    }

    let mut core = core_for(&keys);
    core.activation.cometbft_chain_id = "c".repeat(10_000);
    assert_eq!(core.validate(), Err(GenesisError::InvalidBinding));

    let mut core = core_for(&keys);
    core.qualification_policy.accepted_issuers = vec!["anything at all".into(), "not-a-did".into()];
    assert_eq!(
        core.validate(),
        Err(GenesisError::InvalidQualificationPolicy)
    );

    // Non-ASCII display names remain allowed.
    let mut core = core_for(&keys);
    core.name = "Informática DAO 计算".into();
    assert_eq!(core.validate(), Ok(()));
}

#[test]
fn integers_jcs_cannot_encode_exactly_are_rejected_before_signing() {
    let keys = founder_keys();
    let mut above = core_for(&keys);
    above.rules.minimum_turnout_count = (1u64 << 53) + 1;
    let mut collapsed = core_for(&keys);
    collapsed.rules.minimum_turnout_count = 1u64 << 53;
    assert_eq!(
        above.acceptance_signing_bytes(),
        Err(GenesisError::IntegerOutOfRange)
    );
    assert_eq!(
        collapsed.acceptance_signing_bytes(),
        Err(GenesisError::IntegerOutOfRange)
    );

    let mut height = core_for(&keys);
    height.activation.initial_height = u64::MAX;
    assert_eq!(height.validate(), Err(GenesisError::IntegerOutOfRange));
    let mut time = core_for(&keys);
    time.activation.activation_time_unix = i64::MAX;
    assert_eq!(time.validate(), Err(GenesisError::IntegerOutOfRange));

    let mut largest = core_for(&keys);
    largest.rules.minimum_turnout_count = MAX_JCS_SAFE_INTEGER;
    let envelope = sign_all(largest, &keys);
    let verified = envelope.verify(None).unwrap();
    let bytes = envelope.canonical_bytes().unwrap();
    let (decoded, decoded_verification) =
        decode_and_verify_genesis(&bytes, Some(verified.dao_id())).unwrap();
    assert_eq!(decoded, envelope);
    assert_eq!(decoded_verification, verified);
}

#[test]
fn duplicate_unknown_and_escaped_json_never_verifies() {
    let (bytes, id) = canonical();
    assert!(decode_and_verify_genesis(&bytes, Some(&id)).is_ok());

    let duplicate_name = replace_once(
        &bytes,
        "\"name\":\"Computing DAO\"",
        "\"name\":\"Computing DAO\",\"name\":\"Evil DAO\"",
    );
    assert!(decode_and_verify_genesis(&duplicate_name, None).is_err());

    let text = String::from_utf8(bytes.clone()).unwrap();
    let duplicate_top = format!("{{\"acceptances\":[],{}", &text[1..]);
    assert!(decode_and_verify_genesis(duplicate_top.as_bytes(), None).is_err());

    let unknown = format!("{},\"zzz\":1}}", &text[..text.len() - 1]);
    assert_eq!(
        decode_and_verify_genesis(unknown.as_bytes(), None).unwrap_err(),
        GenesisError::NonCanonicalEncoding
    );

    let escaped = replace_once(&bytes, "\"Computing DAO\"", "\"\\u0043omputing DAO\"");
    assert_eq!(
        decode_and_verify_genesis(&escaped, None).unwrap_err(),
        GenesisError::NonCanonicalEncoding
    );
    let surrogate = replace_once(&bytes, "\"Computing DAO\"", "\"\\ud800 DAO\"");
    assert!(decode_and_verify_genesis(&surrogate, None).is_err());

    let (envelope, _) = genesis();
    let struct_order = serde_json::to_vec(&envelope).unwrap();
    assert_eq!(
        decode_and_verify_genesis(&struct_order, None).unwrap_err(),
        GenesisError::NonCanonicalEncoding
    );
}

#[test]
fn alternative_number_and_whitespace_spellings_never_verify() {
    let (bytes, _) = canonical();
    for alternative in ["25.0", "2.5e1", "025", "25e0", "+25", " 25"] {
        let mutated = replace_once(
            &bytes,
            "\"minimum_turnout_count\":25",
            &format!("\"minimum_turnout_count\":{alternative}"),
        );
        assert!(
            decode_and_verify_genesis(&mutated, None).is_err(),
            "{alternative}"
        );
    }
    for alternative in ["1.8e9", "1800000000.0", "-0"] {
        let mutated = replace_once(
            &bytes,
            "\"activation_time_unix\":1800000000",
            &format!("\"activation_time_unix\":{alternative}"),
        );
        assert!(
            decode_and_verify_genesis(&mutated, None).is_err(),
            "{alternative}"
        );
    }

    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(&bytes);
    let mut newline = bytes.clone();
    newline.push(b'\n');
    let mut leading = vec![b' '];
    leading.extend_from_slice(&bytes);
    let mut trailing = bytes.clone();
    trailing.extend_from_slice(b"{}");
    for (label, mutated) in [
        ("bom", bom),
        ("newline", newline),
        ("leading", leading),
        ("trailing", trailing),
    ] {
        assert!(
            decode_and_verify_genesis(&mutated, None).is_err(),
            "{label}"
        );
    }
}

#[test]
fn hostile_nesting_and_structural_mutations_fail_without_panicking() {
    let (bytes, _) = canonical();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let depth = 100_000;
    let nested = format!(
        "{},\"zzz\":{}{}}}",
        &text[..text.len() - 1],
        "[".repeat(depth),
        "]".repeat(depth)
    );
    assert!(nested.len() <= MAX_GOVERNANCE_GENESIS_BYTES);
    assert!(decode_and_verify_genesis(nested.as_bytes(), None).is_err());
    let bare = format!("{}{}", "[".repeat(200_000), "]".repeat(50_000));
    assert!(decode_and_verify_genesis(bare.as_bytes(), None).is_err());

    // Structural single-byte substitutions and deletions at every byte that
    // is not a hex digit, plus a sample of hex positions. Hex runs are long
    // and uniform, and digit substitution inside signatures is covered by the
    // key-proof test above; mutating every one would make this loop dominate
    // the debug test run.
    let positions = (0..bytes.len())
        .filter(|&index| !bytes[index].is_ascii_hexdigit() || index % 16 == 0)
        .collect::<Vec<_>>();
    for index in positions {
        for substitute in [b'"', b'\\', 0xff] {
            if bytes[index] == substitute {
                continue;
            }
            let mut mutated = bytes.clone();
            mutated[index] = substitute;
            assert!(
                decode_and_verify_genesis(&mutated, None).is_err(),
                "substitution at {index}"
            );
        }
        let mut mutated = bytes.clone();
        mutated.remove(index);
        assert!(
            decode_and_verify_genesis(&mutated, None).is_err(),
            "deletion at {index}"
        );
    }
}
