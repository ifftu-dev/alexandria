//! Exact-boundary and one-over checks for untrusted credential documents.
//!
//! Every case starts from the signed credential in `vectors/01-valid.json` and
//! changes one structural property, so a failure names exactly one limit.

use alexandria_verify::json::UntrustedJsonError;
use alexandria_verify::vc::{decode_credential, CREDENTIAL_JSON_LIMITS};

fn valid_credential() -> serde_json::Value {
    let vector: serde_json::Value =
        serde_json::from_str(include_str!("vectors/01-valid.json")).unwrap();
    vector["credential"].clone()
}

/// The valid credential with `member` inserted as its first top-level entry.
fn with_member(member: &str) -> Vec<u8> {
    let encoded = serde_json::to_string(&valid_credential()).unwrap();
    let rest = encoded.strip_prefix('{').unwrap();
    format!("{{{member},{rest}").into_bytes()
}

fn padded_to(len: usize) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&valid_credential()).unwrap();
    assert!(bytes.len() <= len);
    bytes.resize(len, b' ');
    bytes
}

#[test]
fn the_valid_vector_decodes() {
    let credential = decode_credential(&serde_json::to_vec(&valid_credential()).unwrap()).unwrap();
    assert!(credential.issuer.as_str().starts_with("did:key:"));
}

#[test]
fn byte_limit_accepts_the_boundary_and_refuses_one_more() {
    let max = CREDENTIAL_JSON_LIMITS.max_bytes;
    assert!(decode_credential(&padded_to(max)).is_ok());
    assert_eq!(
        decode_credential(&padded_to(max + 1)).unwrap_err(),
        UntrustedJsonError::TooLarge { max }
    );
}

#[test]
fn nesting_accepts_the_boundary_and_refuses_one_more() {
    let max = CREDENTIAL_JSON_LIMITS.max_depth;
    // The credential object itself is depth 1.
    let nested = |arrays: usize| format!("\"x\":{}{}", "[".repeat(arrays), "]".repeat(arrays));
    assert!(decode_credential(&with_member(&nested(max - 1))).is_ok());
    assert_eq!(
        decode_credential(&with_member(&nested(max))).unwrap_err(),
        UntrustedJsonError::TooDeep { max }
    );
}

#[test]
fn string_length_accepts_the_boundary_and_refuses_one_more() {
    let max = CREDENTIAL_JSON_LIMITS.max_string_bytes;
    let note = |len: usize| format!("\"note\":\"{}\"", "a".repeat(len));
    assert!(decode_credential(&with_member(&note(max))).is_ok());
    assert_eq!(
        decode_credential(&with_member(&note(max + 1))).unwrap_err(),
        UntrustedJsonError::StringTooLong { max }
    );
}

#[test]
fn array_length_accepts_the_boundary_and_refuses_one_more() {
    let max = CREDENTIAL_JSON_LIMITS.max_array_len;
    let list = |len: usize| format!("\"list\":[{}]", vec!["0"; len].join(","));
    assert!(decode_credential(&with_member(&list(max))).is_ok());
    assert_eq!(
        decode_credential(&with_member(&list(max + 1))).unwrap_err(),
        UntrustedJsonError::TooManyElements { max }
    );
}

#[test]
fn object_entries_accept_the_boundary_and_refuse_one_more() {
    let max = CREDENTIAL_JSON_LIMITS.max_object_entries;
    let existing = valid_credential().as_object().unwrap().len();
    let filler = |count: usize| {
        (0..count)
            .map(|index| format!("\"filler{index}\":0"))
            .collect::<Vec<_>>()
            .join(",")
    };
    assert!(decode_credential(&with_member(&filler(max - existing))).is_ok());
    assert_eq!(
        decode_credential(&with_member(&filler(max - existing + 1))).unwrap_err(),
        UntrustedJsonError::TooManyEntries { max }
    );
}

#[test]
fn duplicate_keys_unsafe_numbers_and_trailing_bytes_are_refused() {
    assert_eq!(
        decode_credential(&with_member("\"issuer\":\"did:key:z6MkOther\"")).unwrap_err(),
        UntrustedJsonError::DuplicateKey("issuer".into())
    );
    assert!(decode_credential(&with_member("\"seats\":9007199254740991")).is_ok());
    assert!(matches!(
        decode_credential(&with_member("\"seats\":9007199254740992")),
        Err(UntrustedJsonError::UnsafeNumber(_))
    ));
    let mut trailing = serde_json::to_vec(&valid_credential()).unwrap();
    trailing.extend_from_slice(b"{}");
    assert!(matches!(
        decode_credential(&trailing),
        Err(UntrustedJsonError::Invalid(_))
    ));
}
