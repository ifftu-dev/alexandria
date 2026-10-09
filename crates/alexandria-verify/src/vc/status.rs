//! Bitstring Status List v1.0: the revocation list a credential names.
//!
//! A status list is a bitstring. Bit *n* is the status of the credential whose
//! `credentialStatus.statusListIndex` is *n*; it lives in byte `n / 8` at bit
//! position `n % 8` counted from the most significant bit — the list reads
//! left to right, which is the order the specification defines and the one
//! that interoperates with every other implementation.
//!
//! The list is published as a credential of its own (`BitstringStatusListCredential`)
//! whose subject carries the bitstring GZIP-compressed and multibase
//! base64url encoded (`u…`). A list is at least 131,072 bits so that the
//! position of any one credential in it reveals nothing about how many were
//! issued.

use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use std::io::{Read, Write};

use super::{
    sign::{hash_data, multibase_base58btc_decode, sign_credential, UnsignedCredential},
    CredentialSubject, Proof, VcError, VerifiableCredential, BITSTRING_STATUS_LIST,
    BITSTRING_STATUS_LIST_CREDENTIAL, DATA_INTEGRITY_PROOF, EDDSA_JCS_2022,
};
use crate::did::Did;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};

/// The specification's minimum list size, in bits.
pub const MIN_BITS: usize = 131_072;
/// The largest decoded list this crate will accept: 1 MiB, or 8,388,608
/// entries. A compressed list is small; the decoded one must be bounded.
pub const MAX_DECODED_BYTES: usize = 1 << 20;

/// Read bit `index`, most significant bit first within each byte.
pub fn get_bit(bits: &[u8], index: usize) -> Option<bool> {
    let byte = bits.get(index / 8)?;
    Some((byte >> (7 - (index % 8))) & 1 == 1)
}

/// Set bit `index`, most significant bit first within each byte.
pub fn set_bit(bits: &mut [u8], index: usize, value: bool) -> Result<(), VcError> {
    let byte = bits
        .get_mut(index / 8)
        .ok_or_else(|| VcError::InvalidCredential(format!("status index {index} out of range")))?;
    let mask = 1u8 << (7 - (index % 8));
    if value {
        *byte |= mask;
    } else {
        *byte &= !mask;
    }
    Ok(())
}

/// `u` + base64url (no padding) of the GZIP-compressed bitstring.
pub fn encode_list(bits: &[u8]) -> Result<String, VcError> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(bits)
        .and_then(|_| encoder.finish())
        .map(|compressed| format!("u{}", super::sign::b64url(&compressed)))
        .map_err(|e| VcError::InvalidCredential(format!("gzip: {e}")))
}

/// Decode an `encodedList` back to its bitstring, refusing anything that
/// is not multibase base64url or that would decompress past the limit.
pub fn decode_list(encoded: &str) -> Result<Vec<u8>, VcError> {
    let body = encoded.strip_prefix('u').ok_or_else(|| {
        VcError::InvalidCredential("encodedList is not multibase base64url".into())
    })?;
    let compressed = super::sign::b64url_decode(body)
        .ok_or_else(|| VcError::InvalidCredential("encodedList is not base64url".into()))?;
    let mut out = Vec::new();
    GzDecoder::new(compressed.as_slice())
        .take(MAX_DECODED_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| VcError::InvalidCredential(format!("gunzip: {e}")))?;
    if out.len() > MAX_DECODED_BYTES {
        return Err(VcError::InvalidCredential(
            "status list is larger than 1 MiB".into(),
        ));
    }
    Ok(out)
}

/// Build and sign the `BitstringStatusListCredential` for `bits`.
///
/// `list_id` is the URL the issued credentials name in
/// `credentialStatus.statusListCredential`; the subject is `{list_id}#list`.
pub fn status_list_credential(
    list_id: &str,
    issuer: &Did,
    status_purpose: &str,
    bits: &[u8],
    created: &str,
    key: &SigningKey,
) -> Result<VerifiableCredential, VcError> {
    // Lists shorter than the specification's minimum are zero-padded: the
    // padding adds unset entries and changes no existing bit.
    let mut padded = bits.to_vec();
    if padded.len() * 8 < MIN_BITS {
        padded.resize(MIN_BITS / 8, 0);
    }
    let bits = padded.as_slice();
    let mut properties = serde_json::Map::new();
    properties.insert("type".into(), BITSTRING_STATUS_LIST.into());
    properties.insert("statusPurpose".into(), status_purpose.into());
    properties.insert("encodedList".into(), encode_list(bits)?.into());
    let credential = VerifiableCredential {
        context: vec![super::context::W3C_VC_V2.into()],
        id: Some(list_id.to_string()),
        type_: vec![
            "VerifiableCredential".into(),
            BITSTRING_STATUS_LIST_CREDENTIAL.into(),
        ],
        issuer: issuer.clone(),
        valid_from: created.to_string(),
        valid_until: None,
        credential_subject: CredentialSubject {
            id: Did(format!("{list_id}#list")),
            properties,
        },
        credential_status: None,
        terms_of_use: None,
        witness: None,
        integrity: None,
        proof: Proof::unsigned(created),
    };
    sign_credential(UnsignedCredential { credential }, key, issuer)
}

/// Check a status list credential's shape and signature against `key`, and
/// return its bitstring.
pub fn verify_status_list_credential(
    credential: &VerifiableCredential,
    key: &VerifyingKey,
) -> Result<Vec<u8>, VcError> {
    if !credential
        .type_
        .iter()
        .any(|t| t == BITSTRING_STATUS_LIST_CREDENTIAL)
    {
        return Err(VcError::InvalidCredential(
            "not a BitstringStatusListCredential".into(),
        ));
    }
    if credential.proof.type_ != DATA_INTEGRITY_PROOF
        || credential.proof.cryptosuite != EDDSA_JCS_2022
    {
        return Err(VcError::InvalidCredential("unsupported proof".into()));
    }
    let signature = multibase_base58btc_decode(&credential.proof.proof_value)
        .and_then(|b| Signature::from_slice(&b).ok())
        .ok_or_else(|| VcError::Signature("proofValue is not an Ed25519 signature".into()))?;
    key.verify_strict(&hash_data(credential)?, &signature)
        .map_err(|_| VcError::Signature("status list signature does not verify".into()))?;
    let subject = &credential.credential_subject.properties;
    if subject.get("type").and_then(|t| t.as_str()) != Some(BITSTRING_STATUS_LIST) {
        return Err(VcError::InvalidCredential(
            "subject is not a BitstringStatusList".into(),
        ));
    }
    let encoded = subject
        .get("encodedList")
        .and_then(|e| e.as_str())
        .ok_or_else(|| VcError::InvalidCredential("subject has no encodedList".into()))?;
    decode_list(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::did::derive_did_key;

    #[test]
    fn bits_are_most_significant_first() {
        let mut bits = vec![0u8; 2];
        set_bit(&mut bits, 0, true).unwrap();
        set_bit(&mut bits, 9, true).unwrap();
        assert_eq!(
            bits,
            [0x80, 0x40],
            "index 0 → 0x80, index 9 → byte 1 bit 6 → 0x40"
        );
        assert_eq!(get_bit(&bits, 0), Some(true));
        assert_eq!(get_bit(&bits, 1), Some(false));
        assert_eq!(get_bit(&bits, 9), Some(true));
        assert_eq!(get_bit(&bits, 16), None);
        set_bit(&mut bits, 9, false).unwrap();
        assert_eq!(bits, [0x80, 0x00]);
        assert!(set_bit(&mut bits, 16, true).is_err());
    }

    #[test]
    fn encoded_lists_round_trip_and_are_bounded() {
        let mut bits = vec![0u8; MIN_BITS / 8];
        set_bit(&mut bits, 131_071, true).unwrap();
        let encoded = encode_list(&bits).unwrap();
        assert!(encoded.starts_with('u'));
        assert!(
            encoded.len() < 200,
            "16 KiB of zeros compresses to almost nothing"
        );
        assert_eq!(decode_list(&encoded).unwrap(), bits);
        assert!(decode_list("zNotBase64url").is_err());
        assert!(decode_list("u!!!").is_err());
        let huge = vec![0u8; MAX_DECODED_BYTES + 1];
        assert!(decode_list(&encode_list(&huge).unwrap()).is_err());
    }

    #[test]
    fn a_status_list_credential_is_signed_and_reads_back() {
        let key = SigningKey::from_bytes(&[5; 32]);
        let issuer = derive_did_key(&key);
        let mut bits = vec![0u8; MIN_BITS / 8];
        set_bit(&mut bits, 42, true).unwrap();
        let vc = status_list_credential(
            "urn:uuid:list-1",
            &issuer,
            "revocation",
            &bits,
            "2026-10-09T00:00:00Z",
            &key,
        )
        .unwrap();
        let json = serde_json::to_value(&vc).unwrap();
        assert_eq!(json["type"][1], "BitstringStatusListCredential");
        assert_eq!(json["credentialSubject"]["type"], "BitstringStatusList");
        assert_eq!(json["credentialSubject"]["id"], "urn:uuid:list-1#list");
        assert_eq!(json["credentialSubject"]["statusPurpose"], "revocation");
        let decoded = verify_status_list_credential(&vc, &key.verifying_key()).unwrap();
        assert_eq!(get_bit(&decoded, 42), Some(true));
        assert_eq!(get_bit(&decoded, 41), Some(false));
        let other = SigningKey::from_bytes(&[6; 32]);
        assert!(verify_status_list_credential(&vc, &other.verifying_key()).is_err());
        let mut short = vec![0u8; 8];
        set_bit(&mut short, 9, true).unwrap();
        let padded = status_list_credential(
            "urn:uuid:short",
            &issuer,
            "revocation",
            &short,
            "2026-10-09T00:00:00Z",
            &key,
        )
        .unwrap();
        let decoded = verify_status_list_credential(&padded, &key.verifying_key()).unwrap();
        assert_eq!(
            decoded.len() * 8,
            MIN_BITS,
            "short lists are padded to the minimum"
        );
        assert_eq!(get_bit(&decoded, 9), Some(true));
    }
}
