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
//!
//! # Where a list lives
//!
//! `credentialStatus.statusListCredential` is a URL a verifier can fetch
//! (§14.11.2). A host serves issuer *i*'s list number *n* at
//! `{origin}/status-lists/{i}/{n}` — see [`list_url`] and [`parse_list_url`] —
//! and the document it returns is the signed `BitstringStatusListCredential`
//! whose `id` is that same URL. [`verify_fetched_list`] is the check a verifier
//! runs on what came back: the fetched document must be the list the
//! credential named, signed by the credential's issuer, before a bit in it
//! means anything. An issuer with no host names a `urn:` instead and the list
//! travels in the §20.4 bundle.

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

/// Path prefix under which a host serves status lists.
pub const STATUS_LIST_PATH: &str = "/status-lists";

/// Where a host serves one issuer's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListLocation {
    /// Scheme and authority, no trailing slash.
    pub origin: String,
    pub issuer: Did,
    pub number: u32,
}

/// The URL `origin` serves issuer `issuer`'s list `number` at.
///
/// `origin` is a scheme and authority (`https://cloud.example`); a trailing
/// slash is dropped so the same host never names one list two ways.
pub fn list_url(origin: &str, issuer: &Did, number: u32) -> String {
    format!(
        "{}{STATUS_LIST_PATH}/{}/{number}",
        origin.trim_end_matches('/'),
        issuer.as_str()
    )
}

/// Read `origin`, issuer and list number back out of a list URL.
///
/// `None` for anything that is not exactly the shape [`list_url`] produces:
/// a `urn:` id, a different path, a non-`did:key` issuer, or a number that is
/// not a positive integer. The scheme must be `https`, or `http` on loopback
/// for development.
pub fn parse_list_url(url: &str) -> Option<ListLocation> {
    let (scheme, rest) = url.split_once("://")?;
    let (authority, path) = rest.split_once('/')?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let host = authority.rsplit_once(':').map_or(authority, |(h, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() {
            h
        } else {
            authority
        }
    });
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    if !(scheme == "https" || (scheme == "http" && loopback)) {
        return None;
    }
    let mut segments = path.split('/');
    if segments.next()? != &STATUS_LIST_PATH[1..] {
        return None;
    }
    let issuer = crate::did::parse_did_key(segments.next()?).ok()?;
    let number_text = segments.next()?;
    if segments.next().is_some()
        || number_text.is_empty()
        || number_text.starts_with('0')
        || number_text.len() > 9
        || !number_text.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let number: u32 = number_text.parse().ok()?;
    Some(ListLocation {
        origin: format!("{scheme}://{authority}"),
        issuer,
        number,
    })
}

/// Check a status list document a verifier fetched against the reference that
/// led to it, and return its bitstring.
///
/// The document must be the list the credential named (`id == url`), must be
/// issued by the credential's issuer and signed by the key that issuer's
/// `did:key` resolves to, and must declare the status purpose the credential
/// uses it for. A list that fails any of these says nothing about the
/// credential — it is somebody else's list, or nobody's.
pub fn verify_fetched_list(
    document: &VerifiableCredential,
    url: &str,
    issuer: &Did,
    status_purpose: &str,
) -> Result<Vec<u8>, VcError> {
    if document.id.as_deref() != Some(url) {
        return Err(VcError::InvalidCredential(format!(
            "status list id {:?} is not the list the credential named",
            document.id
        )));
    }
    if &document.issuer != issuer {
        return Err(VcError::InvalidCredential(
            "status list is not issued by the credential issuer".into(),
        ));
    }
    let key = crate::did::resolve_did_key(issuer)
        .map_err(|e| VcError::InvalidCredential(format!("status list issuer: {e}")))?;
    let bits = verify_status_list_credential(document, &key)?;
    let purpose = document
        .credential_subject
        .properties
        .get("statusPurpose")
        .and_then(|p| p.as_str());
    if purpose != Some(status_purpose) {
        return Err(VcError::InvalidCredential(format!(
            "status list purpose {purpose:?} is not {status_purpose:?}"
        )));
    }
    Ok(bits)
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

    #[test]
    fn list_urls_round_trip_and_reject_other_shapes() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let issuer = derive_did_key(&key);
        let url = list_url("https://cloud.example/", &issuer, 1);
        assert_eq!(
            url,
            format!("https://cloud.example/status-lists/{}/1", issuer.as_str())
        );
        let location = parse_list_url(&url).unwrap();
        assert_eq!(location.origin, "https://cloud.example");
        assert_eq!(location.issuer, issuer);
        assert_eq!(location.number, 1);
        let local = parse_list_url(&list_url("http://127.0.0.1:8080", &issuer, 12)).unwrap();
        assert_eq!(local.origin, "http://127.0.0.1:8080");
        assert_eq!(local.number, 12);
        for bad in [
            format!("urn:alexandria:status-list:{}:1", issuer.as_str()),
            format!("http://cloud.example/status-lists/{}/1", issuer.as_str()),
            format!("https://cloud.example/lists/{}/1", issuer.as_str()),
            format!("https://cloud.example/status-lists/{}/0", issuer.as_str()),
            format!("https://cloud.example/status-lists/{}/01", issuer.as_str()),
            format!(
                "https://cloud.example/status-lists/{}/1/extra",
                issuer.as_str()
            ),
            format!("https://cloud.example/status-lists/{}/", issuer.as_str()),
            format!(
                "https://user@cloud.example/status-lists/{}/1",
                issuer.as_str()
            ),
            "https://cloud.example/status-lists/did:web:x/1".into(),
        ] {
            assert!(parse_list_url(&bad).is_none(), "{bad} should not parse");
        }
    }

    #[test]
    fn a_fetched_list_must_be_the_one_named_by_the_issuer() {
        let key = SigningKey::from_bytes(&[8; 32]);
        let issuer = derive_did_key(&key);
        let url = list_url("https://cloud.example", &issuer, 1);
        let mut bits = vec![0u8; MIN_BITS / 8];
        set_bit(&mut bits, 3, true).unwrap();
        let vc = status_list_credential(
            &url,
            &issuer,
            "revocation",
            &bits,
            "2026-10-09T00:00:00Z",
            &key,
        )
        .unwrap();
        let decoded = verify_fetched_list(&vc, &url, &issuer, "revocation").unwrap();
        assert_eq!(get_bit(&decoded, 3), Some(true));

        let other_url = list_url("https://cloud.example", &issuer, 2);
        assert!(verify_fetched_list(&vc, &other_url, &issuer, "revocation").is_err());
        assert!(verify_fetched_list(&vc, &url, &issuer, "suspension").is_err());
        let other = derive_did_key(&SigningKey::from_bytes(&[9; 32]));
        assert!(verify_fetched_list(&vc, &url, &other, "revocation").is_err());

        let mut tampered = vc.clone();
        tampered.credential_subject.properties.insert(
            "encodedList".into(),
            encode_list(&vec![0u8; MIN_BITS / 8]).unwrap().into(),
        );
        assert!(verify_fetched_list(&tampered, &url, &issuer, "revocation").is_err());
    }
}
