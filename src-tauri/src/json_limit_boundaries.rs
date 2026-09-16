//! Exact-boundary coverage for every untrusted JSON limit set in the workspace.
//!
//! The bounded parser's per-dimension checks are tested generically in
//! `alexandria_verify::json`. What that cannot catch is a call site's values
//! drifting: nothing failed if course-document depth went from 16 to 64.
//!
//! So the intended values are written out below as literals, independent of
//! the constants they describe. Every boundary document is built from the
//! intended value and parsed against the real constant, and each document one
//! past a limit must be refused *for that dimension* — a byte-limit refusal of
//! an over-length array would otherwise pass while proving nothing about the
//! array limit. A constant that drifts either way now fails.
//!
//! The first version of this test read each limit from the constant itself and
//! built its documents from that, which made it self-referential: raising
//! course-document depth to 17 moved the expectation with it and the test still
//! passed. A limit can only be pinned by stating it somewhere else.
//!
//! Some dimensions cannot be isolated, because a document one past the limit
//! does not fit inside the byte limit at all. Those are not skipped silently:
//! they are listed in `UNISOLATABLE` and asserted, and their values are still
//! pinned by the equality check.

use alexandria_verify::course::COMPLETION_ENDORSEMENT_JSON_LIMITS;
use alexandria_verify::json::{parse_untrusted, JsonLimits, UntrustedJsonError};
use alexandria_verify::vc::CREDENTIAL_JSON_LIMITS;

use crate::content_store::course::COURSE_DOCUMENT_JSON_LIMITS;
use crate::content_store::profile::PROFILE_DOCUMENT_JSON_LIMITS;
use crate::network_profile::NETWORK_PROFILE_JSON_LIMITS;
use crate::p2p::types::{
    GOSSIP_ENVELOPE_JSON_LIMITS, GOSSIP_PAYLOAD_JSON_LIMITS, PEER_EXCHANGE_JSON_LIMITS,
};

const KIB: usize = 1024;

const fn limits(
    max_bytes: usize,
    max_depth: usize,
    max_array_len: usize,
    max_object_entries: usize,
    max_string_bytes: usize,
) -> JsonLimits {
    JsonLimits {
        max_bytes,
        max_depth,
        max_array_len,
        max_object_entries,
        max_string_bytes,
    }
}

/// (name, the real constant, the intended values written out independently).
/// Changing a limit on purpose means changing it here too, which is the point.
const LIMIT_SETS: [(&str, JsonLimits, JsonLimits); 8] = [
    (
        "credential",
        CREDENTIAL_JSON_LIMITS,
        limits(256 * KIB, 32, 4096, 256, 64 * KIB),
    ),
    (
        "completion endorsement",
        COMPLETION_ENDORSEMENT_JSON_LIMITS,
        limits(128 * KIB, 8, 64, 16, 1024),
    ),
    (
        "network profile",
        NETWORK_PROFILE_JSON_LIMITS,
        limits(64 * KIB, 8, 256, 64, 4096),
    ),
    (
        "profile document",
        PROFILE_DOCUMENT_JSON_LIMITS,
        limits(64 * KIB, 2, 16, 16, 16 * KIB),
    ),
    (
        "course document",
        COURSE_DOCUMENT_JSON_LIMITS,
        limits(1024 * KIB, 16, 4096, 64, 64 * KIB),
    ),
    (
        "gossip envelope",
        GOSSIP_ENVELOPE_JSON_LIMITS,
        limits(64 * KIB, 2, 64 * KIB, 16, 1024),
    ),
    (
        "gossip payload",
        GOSSIP_PAYLOAD_JSON_LIMITS,
        limits(64 * KIB, 32, 4096, 256, 64 * KIB),
    ),
    (
        "peer exchange",
        PEER_EXCHANGE_JSON_LIMITS,
        limits(64 * KIB, 2, 64, 8, 1024),
    ),
];

/// Dimensions whose one-past-the-limit document cannot fit inside the set's
/// byte limit, so the byte limit — not this dimension — would decide.
const UNISOLATABLE: &[(&str, &str, usize)] = &[
    ("gossip envelope", "array", 64 * KIB),
    ("gossip payload", "string", 64 * KIB),
];

fn array_of(len: usize) -> Vec<u8> {
    let mut doc = String::from("[");
    for index in 0..len {
        if index > 0 {
            doc.push(',');
        }
        doc.push('0');
    }
    doc.push(']');
    doc.into_bytes()
}

fn object_of(entries: usize) -> Vec<u8> {
    let body: Vec<String> = (0..entries).map(|i| format!("\"k{i}\":0")).collect();
    format!("{{{}}}", body.join(",")).into_bytes()
}

fn nested(depth: usize) -> Vec<u8> {
    format!("{}{}", "[".repeat(depth), "]".repeat(depth)).into_bytes()
}

fn string_of(bytes: usize) -> Vec<u8> {
    format!("[\"{}\"]", "a".repeat(bytes)).into_bytes()
}

fn padded_to(bytes: usize) -> Vec<u8> {
    let mut doc = b"[]".to_vec();
    doc.resize(bytes, b' ');
    doc
}

type Build = fn(usize) -> Vec<u8>;
type Refusal = fn(&UntrustedJsonError, usize) -> bool;

fn dimensions(intended: &JsonLimits) -> [(&'static str, usize, Build, Refusal); 5] {
    [
        (
            "bytes",
            intended.max_bytes,
            padded_to,
            |e, max| matches!(e, UntrustedJsonError::TooLarge { max: m } if *m == max),
        ),
        (
            "depth",
            intended.max_depth,
            nested,
            |e, max| matches!(e, UntrustedJsonError::TooDeep { max: m } if *m == max),
        ),
        (
            "array",
            intended.max_array_len,
            array_of,
            |e, max| matches!(e, UntrustedJsonError::TooManyElements { max: m } if *m == max),
        ),
        (
            "object",
            intended.max_object_entries,
            object_of,
            |e, max| matches!(e, UntrustedJsonError::TooManyEntries { max: m } if *m == max),
        ),
        (
            "string",
            intended.max_string_bytes,
            string_of,
            |e, max| matches!(e, UntrustedJsonError::StringTooLong { max: m } if *m == max),
        ),
    ]
}

#[test]
fn every_limit_set_has_its_intended_values() {
    for (set, actual, intended) in LIMIT_SETS {
        assert_eq!(actual, intended, "{set} limits changed");
    }
}

#[test]
fn every_limit_accepts_its_boundary_and_refuses_one_more() {
    let mut unisolatable = Vec::new();
    for (set, actual, intended) in LIMIT_SETS {
        for (dimension, value, build, refused_for_this) in dimensions(&intended) {
            let over = build(value + 1);
            // The byte dimension is the exception: its over-limit document is
            // meant to exceed the byte limit.
            if dimension != "bytes" && over.len() > intended.max_bytes {
                unisolatable.push((set, dimension, value));
                continue;
            }

            let at = build(value);
            assert!(
                parse_untrusted(&at, &actual).is_ok(),
                "{set} {dimension}: a document exactly at {value} must be accepted"
            );
            match parse_untrusted(&over, &actual) {
                Err(error) if refused_for_this(&error, value) => {}
                Err(error) => panic!(
                    "{set} {dimension}: one past {value} was refused, but for the wrong \
                     reason ({error}); the {dimension} limit itself is untested"
                ),
                Ok(_) => panic!("{set} {dimension}: one past {value} was accepted"),
            }
        }
    }
    assert_eq!(
        unisolatable, UNISOLATABLE,
        "the set of limits that cannot be tested in isolation changed"
    );
}
