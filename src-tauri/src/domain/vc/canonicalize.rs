//! JCS (RFC 8785) canonicalization for VC payloads prior to signing.
//! Stub — full implementation in PR 4 using `serde_json_canonicalizer`.

use super::VcError;
use serde_json::Value;

/// Canonicalize a JSON value to its JCS byte sequence. The output is
/// what the signature covers; verifiers re-canonicalize and compare.
pub fn canonicalize(_value: &Value) -> Result<Vec<u8>, VcError> {
    unimplemented!("PR 4 — VC schema + JCS")
}
