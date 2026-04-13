//! JCS (RFC 8785) canonicalization for VC payloads prior to signing.
//!
//! Implemented on top of the `serde_json_canonicalizer` crate, which
//! produces UTF-8 output with keys sorted by UTF-16 code units and
//! numbers serialized per ECMA-262 — exactly the RFC 8785 contract
//! that lets issuer and verifier bit-for-bit agree on what was signed.

use super::VcError;
use serde_json::Value;

/// Canonicalize a JSON value to its JCS byte sequence. The output is
/// what the signature covers; verifiers re-canonicalize and compare.
pub fn canonicalize(value: &Value) -> Result<Vec<u8>, VcError> {
    serde_json_canonicalizer::to_vec(value)
        .map_err(|e| VcError::Canonicalize(format!("JCS encode failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonicalize_is_key_order_independent() {
        // JCS (RFC 8785) §3.2.3: object members MUST be sorted by key.
        // Two objects with the same keys in different source order
        // MUST canonicalize to the same bytes — otherwise issuer and
        // verifier cannot agree on what was signed.
        let a = json!({ "a": 1, "b": 2 });
        let b = json!({ "b": 2, "a": 1 });
        assert_eq!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }

    #[test]
    fn canonicalize_is_deterministic_across_calls() {
        let v = json!({ "foo": "bar", "n": 42 });
        let a = canonicalize(&v).unwrap();
        let b = canonicalize(&v).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn canonicalize_emits_utf8_without_bom() {
        // RFC 8785 §3.1: the output MUST be UTF-8 with no BOM.
        let v = json!({ "s": "naïve" });
        let bytes = canonicalize(&v).unwrap();
        assert_ne!(&bytes[..bytes.len().min(3)], b"\xEF\xBB\xBF");
        assert!(std::str::from_utf8(&bytes).is_ok());
    }

    #[test]
    fn canonicalize_preserves_array_order() {
        // Arrays are ordered; their order MUST NOT be rewritten.
        let v1 = json!([1, 2, 3]);
        let v2 = json!([3, 2, 1]);
        assert_ne!(canonicalize(&v1).unwrap(), canonicalize(&v2).unwrap());
    }
}
