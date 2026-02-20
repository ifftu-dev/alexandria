use pallas_codec::minicbor;
use pallas_crypto::hash::{Hash, Hasher};
use pallas_primitives::alonzo::NativeScript;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PolicyError {
    #[error("CBOR encoding failed: {0}")]
    CborEncode(String),
    #[error("invalid key hash length: expected 28 bytes, got {0}")]
    InvalidKeyHash(usize),
}

/// Create a simple signature (`sig`) NativeScript policy.
///
/// This is the learner-owned policy model (Option A): each learner derives
/// their own policy key from their wallet. The native script requires only
/// the learner's payment key signature to authorize minting.
///
/// Equivalent to cardano-cli JSON:
/// ```json
/// { "type": "sig", "keyHash": "<payment_key_hash>" }
/// ```
pub fn create_sig_policy(payment_key_hash: &[u8; 28]) -> NativeScript {
    let hash = Hash::<28>::from(*payment_key_hash);
    NativeScript::ScriptPubkey(hash)
}

/// Compute the policy ID from a NativeScript.
///
/// Policy ID = Blake2b-224(0x00 || CBOR(script))
/// where 0x00 is the native script tag byte.
pub fn compute_policy_id(script: &NativeScript) -> Result<Hash<28>, PolicyError> {
    let mut script_cbor = Vec::new();
    minicbor::encode(script, &mut script_cbor)
        .map_err(|e| PolicyError::CborEncode(e.to_string()))?;

    // Prepend the native script tag byte (0x00)
    let mut tagged = Vec::with_capacity(1 + script_cbor.len());
    tagged.push(0x00);
    tagged.extend_from_slice(&script_cbor);

    Ok(Hasher::<224>::hash(&tagged))
}

/// Generate an asset name for a SkillProof NFT.
///
/// Format: "AlexProof" + first 8 chars of proof_id (max 32 bytes per Cardano spec).
/// Example: "AlexProofabc12345"
pub fn skill_proof_asset_name(proof_id: &str) -> Vec<u8> {
    let suffix = if proof_id.len() >= 8 {
        &proof_id[..8]
    } else {
        proof_id
    };
    format!("AlexProof{}", suffix).into_bytes()
}

/// Generate an asset name for a Course registration NFT.
///
/// Format: "AlexCourse" + first 8 chars of course_id (max 32 bytes per Cardano spec).
/// Example: "AlexCourseabc12345"
pub fn course_asset_name(course_id: &str) -> Vec<u8> {
    let suffix = if course_id.len() >= 8 {
        &course_id[..8]
    } else {
        course_id
    };
    format!("AlexCourse{}", suffix).into_bytes()
}

/// Convert a policy ID hash to a hex string.
pub fn policy_id_hex(policy_id: &Hash<28>) -> String {
    hex::encode(policy_id.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_sig_policy_produces_script_pubkey() {
        let key_hash = [0xABu8; 28];
        let script = create_sig_policy(&key_hash);
        match &script {
            NativeScript::ScriptPubkey(h) => {
                assert_eq!(h.as_ref(), &key_hash);
            }
            _ => panic!("expected ScriptPubkey variant"),
        }
    }

    #[test]
    fn policy_id_is_deterministic() {
        let key_hash = [0x42u8; 28];
        let script = create_sig_policy(&key_hash);
        let id1 = compute_policy_id(&script).expect("policy id 1");
        let id2 = compute_policy_id(&script).expect("policy id 2");
        assert_eq!(id1, id2);
    }

    #[test]
    fn different_keys_produce_different_policy_ids() {
        let script1 = create_sig_policy(&[0x01u8; 28]);
        let script2 = create_sig_policy(&[0x02u8; 28]);
        let id1 = compute_policy_id(&script1).expect("id1");
        let id2 = compute_policy_id(&script2).expect("id2");
        assert_ne!(id1, id2);
    }

    #[test]
    fn policy_id_is_28_bytes() {
        let script = create_sig_policy(&[0xFFu8; 28]);
        let id = compute_policy_id(&script).expect("id");
        assert_eq!(id.as_ref().len(), 28);
    }

    #[test]
    fn policy_id_hex_is_56_chars() {
        let script = create_sig_policy(&[0xAAu8; 28]);
        let id = compute_policy_id(&script).expect("id");
        let hex = policy_id_hex(&id);
        assert_eq!(hex.len(), 56);
        // All lowercase hex chars
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn skill_proof_asset_name_format() {
        let name = skill_proof_asset_name("abc12345def67890");
        let s = String::from_utf8(name.clone()).unwrap();
        assert_eq!(s, "AlexProofabc12345");
        assert!(name.len() <= 32);
    }

    #[test]
    fn course_asset_name_format() {
        let name = course_asset_name("xyz98765abc12345");
        let s = String::from_utf8(name.clone()).unwrap();
        assert_eq!(s, "AlexCoursexyz98765");
        assert!(name.len() <= 32);
    }

    #[test]
    fn short_id_asset_names() {
        let name = skill_proof_asset_name("abc");
        assert_eq!(String::from_utf8(name).unwrap(), "AlexProofabc");

        let name = course_asset_name("x");
        assert_eq!(String::from_utf8(name).unwrap(), "AlexCoursex");
    }
}
