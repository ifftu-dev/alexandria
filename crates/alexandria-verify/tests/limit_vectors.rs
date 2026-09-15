//! Cross-language vectors for untrusted credential JSON limits.
//!
//! `tests/vectors/limits/` holds exact byte files, and `manifest.json` names
//! the outcome each must produce. `independent-verifier.mjs` checks the same
//! bytes with a parser written from the vectors README alone, so the limits are
//! a documented contract rather than a property of this crate.
//!
//! # Regenerating
//!
//! ```sh
//! ALEXANDRIA_REGENERATE_VECTORS=1 cargo test -p alexandria-verify --test limit_vectors
//! ```
//!
//! Every case derives from the credential in `vectors/01-valid.json`, so
//! regeneration is deterministic and a changed file means a limit moved.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use alexandria_verify::json::UntrustedJsonError;
use alexandria_verify::vc::{decode_credential, CREDENTIAL_JSON_LIMITS};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Limits {
    max_bytes: usize,
    max_depth: usize,
    max_array_len: usize,
    max_object_entries: usize,
    max_string_bytes: usize,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    file: String,
    description: String,
    expect: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    limits: Limits,
    cases: Vec<Case>,
}

const OUTCOMES: &[&str] = &[
    "accept",
    "too_large",
    "too_deep",
    "too_many_elements",
    "too_many_entries",
    "string_too_long",
    "duplicate_key",
    "unsafe_number",
    "invalid",
];

fn limits_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/limits")
}

fn outcome(bytes: &[u8]) -> &'static str {
    match decode_credential(bytes) {
        Ok(_) => "accept",
        Err(UntrustedJsonError::TooLarge { .. }) => "too_large",
        Err(UntrustedJsonError::TooDeep { .. }) => "too_deep",
        Err(UntrustedJsonError::TooManyElements { .. }) => "too_many_elements",
        Err(UntrustedJsonError::TooManyEntries { .. }) => "too_many_entries",
        Err(UntrustedJsonError::StringTooLong { .. }) => "string_too_long",
        Err(UntrustedJsonError::DuplicateKey(_)) => "duplicate_key",
        Err(UntrustedJsonError::UnsafeNumber(_)) => "unsafe_number",
        Err(UntrustedJsonError::Invalid(_)) => "invalid",
        Err(UntrustedJsonError::InvalidLimits(reason)) => {
            panic!("credential limits are invalid: {reason}")
        }
    }
}

fn valid_credential() -> String {
    let vector: serde_json::Value =
        serde_json::from_str(include_str!("vectors/01-valid.json")).unwrap();
    serde_json::to_string(&vector["credential"]).unwrap()
}

/// The valid credential with `member` as its first top-level entry.
fn with_member(member: &str) -> Vec<u8> {
    let encoded = valid_credential();
    let rest = encoded.strip_prefix('{').unwrap();
    format!("{{{member},{rest}").into_bytes()
}

/// The valid credential with `member` as the first entry of its subject.
fn with_subject_member(member: &str) -> Vec<u8> {
    let encoded = valid_credential();
    let anchor = "\"credentialSubject\":{";
    let at = encoded.find(anchor).unwrap() + anchor.len();
    format!("{}{member},{}", &encoded[..at], &encoded[at..]).into_bytes()
}

fn padded_to(len: usize) -> Vec<u8> {
    let mut bytes = valid_credential().into_bytes();
    assert!(bytes.len() <= len);
    bytes.resize(len, b' ');
    bytes
}

fn build_all() -> Vec<(&'static str, &'static str, Vec<u8>, &'static str)> {
    let limits = CREDENTIAL_JSON_LIMITS;
    let nested = |levels: usize| format!("\"x\":{}{}", "[".repeat(levels), "]".repeat(levels));
    let list = |len: usize| format!("\"list\":[{}]", vec!["0"; len].join(","));
    let existing = serde_json::from_str::<serde_json::Value>(&valid_credential())
        .unwrap()
        .as_object()
        .unwrap()
        .len();
    let filler = |count: usize| {
        (0..count)
            .map(|index| format!("\"filler{index}\":0"))
            .collect::<Vec<_>>()
            .join(",")
    };
    let half = limits.max_string_bytes / 2;
    let mut invalid_utf8 = with_member("\"note\":\"x\"");
    let position = invalid_utf8.iter().position(|byte| *byte == b'x').unwrap();
    invalid_utf8[position] = 0xff;
    let mut with_bom = vec![0xef, 0xbb, 0xbf];
    with_bom.extend_from_slice(valid_credential().as_bytes());
    let mut trailing = valid_credential().into_bytes();
    trailing.extend_from_slice(b"{}");

    vec![
        ("01-valid.json", "The unmodified valid credential.", valid_credential().into_bytes(), "accept"),
        ("02-bytes-at-limit.json", "Padded with trailing whitespace to exactly the byte limit.", padded_to(limits.max_bytes), "accept"),
        ("03-bytes-over-limit.json", "One byte over the byte limit, refused before parsing.", padded_to(limits.max_bytes + 1), "too_large"),
        ("04-depth-at-limit.json", "A member nested to exactly the depth limit; the credential object is depth 1.", with_member(&nested(limits.max_depth - 1)), "accept"),
        ("05-depth-over-limit.json", "An empty array opened one level beyond the depth limit.", with_member(&nested(limits.max_depth)), "too_deep"),
        ("06-array-at-limit.json", "An array with exactly the element limit.", with_member(&list(limits.max_array_len)), "accept"),
        ("07-array-over-limit.json", "An array with one element over the limit.", with_member(&list(limits.max_array_len + 1)), "too_many_elements"),
        ("08-entries-at-limit.json", "The credential object with exactly the entry limit.", with_member(&filler(limits.max_object_entries - existing)), "accept"),
        ("09-entries-over-limit.json", "The credential object with one entry over the limit.", with_member(&filler(limits.max_object_entries - existing + 1)), "too_many_entries"),
        ("10-string-at-limit-multibyte.json", "A string of two-byte characters decoding to exactly the string limit in UTF-8 bytes.", with_member(&format!("\"note\":\"{}\"", "\u{e9}".repeat(half))), "accept"),
        ("11-string-over-limit.json", "The same string with one more byte.", with_member(&format!("\"note\":\"{}a\"", "\u{e9}".repeat(half))), "string_too_long"),
        ("12-string-escapes-at-limit.json", "Escaped characters are counted by their decoded UTF-8 bytes, not their escaped spelling.", with_member(&format!("\"note\":\"{}\"", "\\u00e9".repeat(half))), "accept"),
        ("13-string-escapes-over-limit.json", "The same escaped string with one more decoded byte.", with_member(&format!("\"note\":\"{}a\"", "\\u00e9".repeat(half))), "string_too_long"),
        ("14-duplicate-top-level-key.json", "The issuer key appears twice.", with_member("\"issuer\":\"did:key:z6MkOther\""), "duplicate_key"),
        ("15-duplicate-nested-key.json", "A key repeated inside the credential subject.", with_subject_member("\"skillId\":\"other\""), "duplicate_key"),
        ("16-integers-at-safe-limit.json", "Integers at exactly plus and minus 2^53-1.", with_member("\"seats\":[9007199254740991,-9007199254740991]"), "accept"),
        ("17-integer-over-safe-limit.json", "An integer of 2^53.", with_member("\"seats\":9007199254740992"), "unsafe_number"),
        ("18-negative-integer-over-safe-limit.json", "An integer of -2^53.", with_member("\"seats\":-9007199254740992"), "unsafe_number"),
        ("19-exponent-over-safe-magnitude.json", "A finite exponent literal whose magnitude exceeds 2^53-1.", with_member("\"seats\":1e300"), "unsafe_number"),
        ("20-exponent-overflow.json", "An exponent literal that overflows a double is malformed.", with_member("\"seats\":1e400"), "invalid"),
        ("21-trailing-bytes.json", "A second JSON value after the credential.", trailing, "invalid"),
        ("22-byte-order-mark.json", "A UTF-8 byte-order mark before the credential.", with_bom, "invalid"),
        ("23-lone-surrogate-escape.json", "A leading surrogate escape with no trailing surrogate.", with_member("\"note\":\"\\ud800x\""), "invalid"),
        ("24-invalid-utf8.json", "A string containing a byte that is not valid UTF-8.", invalid_utf8, "invalid"),
    ]
}

fn manifest() -> Manifest {
    let limits = CREDENTIAL_JSON_LIMITS;
    Manifest {
        limits: Limits {
            max_bytes: limits.max_bytes,
            max_depth: limits.max_depth,
            max_array_len: limits.max_array_len,
            max_object_entries: limits.max_object_entries,
            max_string_bytes: limits.max_string_bytes,
        },
        cases: build_all()
            .into_iter()
            .map(|(file, description, _, expect)| Case {
                file: file.into(),
                description: description.into(),
                expect: expect.into(),
            })
            .collect(),
    }
}

#[test]
fn limit_vectors_match_this_implementation() {
    if std::env::var("ALEXANDRIA_REGENERATE_VECTORS").is_ok() {
        regenerate();
        return;
    }
    let dir = limits_dir();
    let raw = fs::read_to_string(dir.join("manifest.json")).unwrap_or_else(|error| {
        panic!("read limit manifest: {error}. Regenerate with ALEXANDRIA_REGENERATE_VECTORS=1")
    });
    let recorded: Manifest = serde_json::from_str(&raw).unwrap();
    assert_eq!(recorded, manifest(), "limit manifest is stale; regenerate");

    for (file, _, bytes, expect) in build_all() {
        let on_disk = fs::read(dir.join(file)).unwrap();
        assert!(on_disk == bytes, "{file}: bytes are stale; regenerate");
        assert_eq!(outcome(&on_disk), expect, "{file}");
    }
}

#[test]
fn the_limit_suite_exercises_every_outcome_at_each_boundary() {
    let cases = build_all();
    for kind in OUTCOMES {
        assert!(
            cases.iter().any(|(_, _, _, expect)| expect == kind),
            "no vector produces {kind}"
        );
    }
    for over in [
        "too_large",
        "too_deep",
        "too_many_elements",
        "too_many_entries",
        "string_too_long",
    ] {
        let at_limit = cases
            .iter()
            .position(|(_, _, _, expect)| *expect == over)
            .and_then(|index| index.checked_sub(1))
            .map(|index| cases[index].3);
        assert_eq!(
            at_limit,
            Some("accept"),
            "{over} must follow its accepted exact-boundary case"
        );
    }
}

fn regenerate() {
    let dir = limits_dir();
    fs::create_dir_all(&dir).unwrap();
    for entry in fs::read_dir(&dir).unwrap().flatten() {
        fs::remove_file(entry.path()).unwrap();
    }
    for (file, _, bytes, _) in build_all() {
        fs::write(dir.join(file), bytes).unwrap();
    }
    let manifest = serde_json::to_string_pretty(&manifest()).unwrap();
    fs::write(dir.join("manifest.json"), manifest + "\n").unwrap();
    println!(
        "wrote {} limit vectors to {}",
        build_all().len(),
        dir.display()
    );
}
