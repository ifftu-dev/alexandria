//! Embedded JSON-LD contexts for offline credential verification.
//!
//! The point of bundling these is survivability (§20.4): a verifier
//! must not hit the network to resolve well-known contexts. The
//! bodies are abridged — we ship the term/type definitions relevant
//! to Verifiable Credentials, not the full W3C document tree — but
//! they satisfy the `lookup_context` contract that signing and
//! verification depend on.

/// W3C Verifiable Credentials v2 context URI.
///
/// v2, not v1, because the envelope uses `validFrom` / `validUntil` — those are
/// v2 terms and the v1 context does not define them. Declaring v1 while using
/// them is what this constant used to do, and it is worse than cosmetic: a
/// JSON-LD-aware verifier expanding against v1 drops undefined terms, and a
/// dropped `validUntil` reads as "never expires".
pub const W3C_VC_V2: &str = "https://www.w3.org/ns/credentials/v2";

/// Alexandria protocol v1 context URI.
pub const ALEXANDRIA_V1: &str = "https://alexandria.protocol/context/v1";

/// W3C VC v2 context (abridged — defines VerifiableCredential and the
/// core claim/proof terms). Source: w3.org/ns/credentials/v2.
const W3C_VC_V2_DOC: &str = r#"{
  "@context": {
    "@version": 1.1,
    "@protected": true,
    "id": "@id",
    "type": "@type",
    "VerifiableCredential": {
      "@id": "https://www.w3.org/2018/credentials#VerifiableCredential",
      "@context": {
        "@version": 1.1,
        "@protected": true,
        "id": "@id",
        "type": "@type",
        "credentialSchema": { "@id": "https://www.w3.org/2018/credentials#credentialSchema", "@type": "@id" },
        "credentialStatus": { "@id": "https://www.w3.org/2018/credentials#credentialStatus", "@type": "@id" },
        "credentialSubject": { "@id": "https://www.w3.org/2018/credentials#credentialSubject", "@type": "@id" },
        "evidence": { "@id": "https://www.w3.org/2018/credentials#evidence", "@type": "@id" },
        "validUntil": { "@id": "https://www.w3.org/2018/credentials#validUntil", "@type": "http://www.w3.org/2001/XMLSchema#dateTime" },
        "holder": { "@id": "https://www.w3.org/2018/credentials#holder", "@type": "@id" },
        "issuer": { "@id": "https://www.w3.org/2018/credentials#issuer", "@type": "@id" },
        "validFrom": { "@id": "https://www.w3.org/2018/credentials#validFrom", "@type": "http://www.w3.org/2001/XMLSchema#dateTime" },
        "proof": { "@id": "https://w3id.org/security#proof", "@type": "@id", "@container": "@graph" },
        "termsOfUse": { "@id": "https://www.w3.org/2018/credentials#termsOfUse", "@type": "@id" }
      }
    }
  }
}"#;

/// Alexandria v1 context — defines our claim taxonomy (skill, role,
/// custom) and derived-state output shape per spec §16.
const ALEXANDRIA_V1_DOC: &str = r#"{
  "@context": {
    "@version": 1.1,
    "@protected": true,
    "alexandria": "https://alexandria.protocol/context/v1#",
    "FormalCredential": "alexandria:FormalCredential",
    "AssessmentCredential": "alexandria:AssessmentCredential",
    "AttestationCredential": "alexandria:AttestationCredential",
    "RoleCredential": "alexandria:RoleCredential",
    "DerivedCredential": "alexandria:DerivedCredential",
    "SelfAssertion": "alexandria:SelfAssertion",
    "claim": { "@id": "alexandria:claim", "@type": "@id" },
    "kind": "alexandria:kind",
    "skillId": "alexandria:skillId",
    "level": { "@id": "alexandria:level", "@type": "http://www.w3.org/2001/XMLSchema#integer" },
    "score": { "@id": "alexandria:score", "@type": "http://www.w3.org/2001/XMLSchema#double" },
    "evidenceRefs": { "@id": "alexandria:evidenceRefs", "@container": "@set" },
    "rubricVersion": "alexandria:rubricVersion",
    "assessmentMethod": "alexandria:assessmentMethod"
  }
}"#;

/// Return the embedded JSON-LD document for a given context URI, or
/// `None` if we don't ship a local copy.
pub fn lookup_context(uri: &str) -> Option<&'static str> {
    match uri {
        W3C_VC_V2 => Some(W3C_VC_V2_DOC),
        ALEXANDRIA_V1 => Some(ALEXANDRIA_V1_DOC),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_returns_w3c_vc_v2_context() {
        // Offline verification (spec §20) requires that the W3C and
        // Alexandria contexts are embedded — never fetched at runtime.
        let doc = lookup_context(W3C_VC_V2).expect("W3C context embedded");
        assert!(doc.contains("VerifiableCredential"));
    }

    #[test]
    fn lookup_returns_alexandria_v1_context() {
        let doc = lookup_context(ALEXANDRIA_V1).expect("Alexandria context embedded");
        assert!(doc.contains("Alexandria") || doc.contains("alexandria"));
    }

    #[test]
    fn lookup_unknown_context_returns_none() {
        assert!(lookup_context("https://example.com/unknown/v1").is_none());
    }
}

#[cfg(test)]
mod v2_conformance_tests {
    use super::*;
    use crate::vc::VerifiableCredential;

    /// The declared context must define every term the envelope actually emits.
    ///
    /// This is the invariant that was broken: the envelope carried `validFrom`
    /// and `validUntil` while declaring the v1 context, which defines neither.
    /// A JSON-LD-aware verifier drops undefined terms, and a dropped
    /// `validUntil` means an expired credential reads as one that never
    /// expires — so the mismatch was a security bug wearing a typo's clothes.
    #[test]
    fn the_declared_context_defines_the_terms_the_envelope_emits() {
        let doc = lookup_context(W3C_VC_V2).expect("v2 context embedded");
        for term in ["validFrom", "validUntil"] {
            assert!(
                doc.contains(term),
                "the envelope emits `{term}` but the declared context does not define it"
            );
        }
        for gone in ["issuanceDate", "expirationDate"] {
            assert!(
                !doc.contains(gone),
                "`{gone}` is a v1 term and nothing emits it any more"
            );
        }
    }

    #[test]
    fn the_context_uri_is_the_v2_one() {
        assert_eq!(W3C_VC_V2, "https://www.w3.org/ns/credentials/v2");
    }

    /// Credentials issued before the move still verify.
    ///
    /// Verification is JCS over the JSON document and never inspects
    /// `@context`, so a credential that declared v1 keeps verifying against its
    /// own signature. Pinned as a test because "we changed the wire format" and
    /// "existing credentials still work" are only compatible by accident
    /// otherwise — and a credential that stops verifying is one a learner
    /// cannot use.
    #[test]
    fn a_credential_declaring_the_old_context_still_deserialises() {
        let old = serde_json::json!({
            "@context": ["https://www.w3.org/2018/credentials/v1"],
            "id": "urn:uuid:legacy",
            "type": ["VerifiableCredential", "FormalCredential"],
            "issuer": "did:key:z6MkLegacyIssuer",
            "validFrom": "2026-01-01T00:00:00Z",
            "credentialSubject": { "id": "did:key:z6MkLegacySubject" },
            "proof": {
                "type": "Ed25519Signature2020",
                "created": "2026-01-01T00:00:00Z",
                "verificationMethod": "did:key:z6MkLegacyIssuer#key-1",
                "proofPurpose": "assertionMethod",
                "jws": "header..sig"
            }
        });
        let vc: VerifiableCredential =
            serde_json::from_value(old).expect("a v1-context credential must still parse");
        assert_eq!(vc.context[0], "https://www.w3.org/2018/credentials/v1");
    }
}
