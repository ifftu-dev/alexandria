//! Cardano reputation snapshot transaction builders.
//!
//! Builds CIP-68 soulbound token minting transactions for on-chain
//! reputation anchoring. Two tokens per mint:
//!   - Reference NFT (label 100): at soulbound script address with inline datum
//!   - User token (label 222): in learner's wallet
//!
//! Uses the pallas decode-modify-reencode pattern for inline datum
//! injection (pallas-txbuilder doesn't support inline datums natively).

use pallas_codec::minicbor;
use pallas_codec::utils::KeyValuePairs;
use pallas_primitives::{Metadatum, MetadatumLabel};

use crate::domain::reputation::{cip68, OnChainSkillScore, ReputationRole};

use super::tx_builder::TxBuildError;

/// Governance/reputation metadata label.
const REPUTATION_LABEL: MetadatumLabel = 1694;

/// Build the base asset name for a reputation soulbound token.
///
/// Format: subject_id(16 bytes) + role_byte(1 byte) = 17 bytes.
/// The subject_id is truncated/padded to exactly 16 bytes.
pub fn reputation_base_name(subject_id: &str, role: &ReputationRole) -> Vec<u8> {
    let role_byte = match role {
        ReputationRole::Instructor => cip68::ROLE_INSTRUCTOR,
        ReputationRole::Learner => cip68::ROLE_LEARNER,
        ReputationRole::Assessor => cip68::ROLE_ASSESSOR,
        ReputationRole::Author => cip68::ROLE_AUTHOR,
        ReputationRole::Mentor => cip68::ROLE_MENTOR,
    };

    // Take first 16 bytes of subject_id (or pad with zeros)
    let id_bytes = subject_id.as_bytes();
    let mut base = [0u8; 17];
    let copy_len = id_bytes.len().min(16);
    base[..copy_len].copy_from_slice(&id_bytes[..copy_len]);
    base[16] = role_byte;

    base.to_vec()
}

/// Build the CIP-68 reference token asset name (label 100 prefix + base name).
pub fn reference_asset_name(base_name: &[u8]) -> Vec<u8> {
    let mut name = Vec::with_capacity(4 + base_name.len());
    name.extend_from_slice(&cip68::REFERENCE_LABEL_PREFIX);
    name.extend_from_slice(base_name);
    name
}

/// Build the CIP-68 user token asset name (label 222 prefix + base name).
pub fn user_asset_name(base_name: &[u8]) -> Vec<u8> {
    let mut name = Vec::with_capacity(4 + base_name.len());
    name.extend_from_slice(&cip68::USER_LABEL_PREFIX);
    name.extend_from_slice(base_name);
    name
}

/// Encode a ReputationDatum as Plutus Data CBOR bytes.
///
/// The datum structure (matching the Aiken soulbound validator):
/// ```text
/// Constr(0, [
///     owner:           ByteArray(28),     -- payment key hash
///     subject_id:      ByteArray(16),     -- subject identifier
///     role:            Int,               -- 0=instructor, 1=learner, ...
///     skills:          List<SkillScore>,  -- nested constructor
///     computation_spec: Int,              -- 2 for v2
///     window_start:    Int,               -- POSIX milliseconds
///     window_end:      Int,               -- POSIX milliseconds
/// ])
///
/// SkillScore = Constr(0, [
///     skill_id:       ByteArray(16),
///     proficiency:    Int(0-5),
///     impact_score:   Int(scaled 10^6),
///     confidence:     Int(scaled 10^4),
///     evidence_count: Int,
/// ])
/// ```
pub fn encode_reputation_datum(
    owner_key_hash: &[u8; 28],
    subject_id: &str,
    role: &ReputationRole,
    skills: &[OnChainSkillScore],
    window_start_ms: i64,
    window_end_ms: i64,
) -> Result<Vec<u8>, TxBuildError> {
    // We encode as raw CBOR manually using minicbor since pallas doesn't
    // have a high-level Plutus Data builder.
    //
    // Plutus Data encoding:
    //   Constr(0, fields) => CBOR tag 121 + array of fields
    //   ByteArray => CBOR bytes
    //   Int => CBOR integer
    //   List => CBOR array

    let mut buf = Vec::new();
    let mut encoder = minicbor::Encoder::new(&mut buf);

    // Constr(0, ...) = CBOR tag 121
    encoder
        .tag(minicbor::data::Tag::new(121))
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    // 7 fields in the constructor
    encoder
        .array(7)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    // Field 0: owner (28 bytes)
    encoder
        .bytes(owner_key_hash)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    // Field 1: subject_id (16 bytes, truncated/padded)
    let sid_bytes = subject_id.as_bytes();
    let mut sid_padded = [0u8; 16];
    let copy_len = sid_bytes.len().min(16);
    sid_padded[..copy_len].copy_from_slice(&sid_bytes[..copy_len]);
    encoder
        .bytes(&sid_padded)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    // Field 2: role (integer enum)
    let role_int: i64 = match role {
        ReputationRole::Instructor => 0,
        ReputationRole::Learner => 1,
        ReputationRole::Assessor => 2,
        ReputationRole::Author => 3,
        ReputationRole::Mentor => 4,
    };
    encoder
        .i64(role_int)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    // Field 3: skills (List<SkillScore>)
    encoder
        .array(skills.len() as u64)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    for skill in skills {
        // Each SkillScore = Constr(0, [skill_id, proficiency, impact, confidence, evidence_count])
        encoder
            .tag(minicbor::data::Tag::new(121))
            .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
        encoder
            .array(5)
            .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

        // skill_id bytes (decode from hex, pad to 16)
        let skill_bytes = hex::decode(&skill.skill_id_bytes).unwrap_or_default();
        let mut skill_padded = [0u8; 16];
        let slen = skill_bytes.len().min(16);
        skill_padded[..slen].copy_from_slice(&skill_bytes[..slen]);
        encoder
            .bytes(&skill_padded)
            .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

        encoder
            .i64(skill.proficiency as i64)
            .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
        encoder
            .i64(skill.impact_score)
            .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
        encoder
            .i64(skill.confidence)
            .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
        encoder
            .i64(skill.evidence_count)
            .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    }

    // Field 4: computation_spec (2 = v2)
    encoder
        .i64(2)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    // Field 5: window_start (POSIX ms)
    encoder
        .i64(window_start_ms)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    // Field 6: window_end (POSIX ms)
    encoder
        .i64(window_end_ms)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    Ok(buf)
}

/// Build metadata for a reputation snapshot transaction.
///
/// Records the snapshot action under the governance/reputation label (1694).
pub fn build_snapshot_metadata(
    snapshot_id: &str,
    subject_id: &str,
    role: &str,
    action: &str,
    skill_count: i64,
) -> KeyValuePairs<MetadatumLabel, Metadatum> {
    let fields = vec![
        (
            Metadatum::Text("type".into()),
            Metadatum::Text("reputation_snapshot".into()),
        ),
        (
            Metadatum::Text("snapshot_id".into()),
            Metadatum::Text(snapshot_id.into()),
        ),
        (
            Metadatum::Text("subject_id".into()),
            Metadatum::Text(subject_id.into()),
        ),
        (Metadatum::Text("role".into()), Metadatum::Text(role.into())),
        (
            Metadatum::Text("action".into()),
            Metadatum::Text(action.into()),
        ),
        (
            Metadatum::Text("skill_count".into()),
            Metadatum::Int(skill_count.into()),
        ),
        (Metadatum::Text("spec".into()), Metadatum::Text("v2".into())),
    ];

    KeyValuePairs::from(vec![(
        REPUTATION_LABEL,
        Metadatum::Map(KeyValuePairs::from(fields)),
    )])
}

/// Proficiency level string to on-chain enum index (0-5).
pub fn proficiency_to_index(level: &str) -> u8 {
    match level {
        "remember" => 0,
        "understand" => 1,
        "apply" => 2,
        "analyze" => 3,
        "evaluate" => 4,
        "create" => 5,
        _ => 2, // default to apply
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reputation_base_name_correct_length() {
        let name = reputation_base_name("computer_science", &ReputationRole::Instructor);
        assert_eq!(name.len(), 17);
        assert_eq!(name[16], cip68::ROLE_INSTRUCTOR);
    }

    #[test]
    fn reputation_base_name_short_subject() {
        let name = reputation_base_name("cs", &ReputationRole::Learner);
        assert_eq!(name.len(), 17);
        assert_eq!(name[0], b'c');
        assert_eq!(name[1], b's');
        assert_eq!(name[2], 0); // padding
        assert_eq!(name[16], cip68::ROLE_LEARNER);
    }

    #[test]
    fn reference_asset_name_has_correct_prefix() {
        let base = reputation_base_name("test_subject", &ReputationRole::Instructor);
        let ref_name = reference_asset_name(&base);
        assert_eq!(&ref_name[..4], &cip68::REFERENCE_LABEL_PREFIX);
        assert_eq!(ref_name.len(), 4 + 17);
    }

    #[test]
    fn user_asset_name_has_correct_prefix() {
        let base = reputation_base_name("test_subject", &ReputationRole::Learner);
        let usr_name = user_asset_name(&base);
        assert_eq!(&usr_name[..4], &cip68::USER_LABEL_PREFIX);
        assert_eq!(usr_name.len(), 4 + 17);
    }

    #[test]
    fn role_bytes_unique() {
        let roles = [
            ReputationRole::Instructor,
            ReputationRole::Learner,
            ReputationRole::Assessor,
            ReputationRole::Author,
            ReputationRole::Mentor,
        ];
        let bytes: Vec<u8> = roles
            .iter()
            .map(|r| reputation_base_name("test", r)[16])
            .collect();
        // All role bytes should be unique
        let mut unique = bytes.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), bytes.len());
    }

    #[test]
    fn encode_reputation_datum_produces_valid_cbor() {
        let owner = [0x42u8; 28];
        let skills = vec![OnChainSkillScore {
            skill_id_bytes: hex::encode([0xAB; 16]),
            proficiency: 2,
            impact_score: 850_000, // 0.85 * 10^6
            confidence: 7500,      // 0.75 * 10^4
            evidence_count: 5,
        }];

        let result = encode_reputation_datum(
            &owner,
            "algorithms",
            &ReputationRole::Instructor,
            &skills,
            1_700_000_000_000,
            1_700_100_000_000,
        );
        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert!(!bytes.is_empty());

        // Should start with CBOR tag 121 (Constr 0)
        // Tag 121 = major type 6 (tag), value 121
        // In CBOR: 0xd8 0x79
        assert_eq!(bytes[0], 0xd8);
        assert_eq!(bytes[1], 0x79);
    }

    #[test]
    fn encode_empty_skills_list() {
        let owner = [0x00u8; 28];
        let result = encode_reputation_datum(&owner, "test", &ReputationRole::Learner, &[], 0, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn snapshot_metadata_structure() {
        let meta = build_snapshot_metadata("snap_1", "algo", "instructor", "mint", 5);
        assert_eq!(meta.len(), 1);
        let (label, _) = &meta[0];
        assert_eq!(*label, REPUTATION_LABEL);
    }

    #[test]
    fn proficiency_to_index_all_levels() {
        assert_eq!(proficiency_to_index("remember"), 0);
        assert_eq!(proficiency_to_index("understand"), 1);
        assert_eq!(proficiency_to_index("apply"), 2);
        assert_eq!(proficiency_to_index("analyze"), 3);
        assert_eq!(proficiency_to_index("evaluate"), 4);
        assert_eq!(proficiency_to_index("create"), 5);
        assert_eq!(proficiency_to_index("unknown"), 2); // default
    }

    #[test]
    fn datum_multiple_skills() {
        let owner = [0x01u8; 28];
        let skills = vec![
            OnChainSkillScore {
                skill_id_bytes: hex::encode([0x01; 16]),
                proficiency: 0,
                impact_score: 100_000,
                confidence: 5000,
                evidence_count: 1,
            },
            OnChainSkillScore {
                skill_id_bytes: hex::encode([0x02; 16]),
                proficiency: 3,
                impact_score: 500_000,
                confidence: 8000,
                evidence_count: 10,
            },
            OnChainSkillScore {
                skill_id_bytes: hex::encode([0x03; 16]),
                proficiency: 5,
                impact_score: 950_000,
                confidence: 9500,
                evidence_count: 25,
            },
        ];

        let result = encode_reputation_datum(
            &owner,
            "computer_science",
            &ReputationRole::Instructor,
            &skills,
            1_700_000_000_000,
            1_700_200_000_000,
        );
        assert!(result.is_ok());
    }
}
