//! Embedded JSON-LD contexts for offline credential verification.
//!
//! The point of bundling these is survivability (§20.4): a verifier
//! must not hit the network to resolve well-known contexts. The
//! bodies are abridged — we ship the term/type definitions relevant
//! to Verifiable Credentials, not the full W3C document tree — but
//! they satisfy the `lookup_context` contract that signing and
//! verification depend on.

/// W3C Verifiable Credentials v1 context URI.
pub const W3C_VC_V1: &str = "https://www.w3.org/2018/credentials/v1";

/// Alexandria protocol v1 context URI.
pub const ALEXANDRIA_V1: &str = "https://alexandria.protocol/context/v1";

/// W3C VC v1 context (abridged — defines VerifiableCredential and the
/// core claim/proof terms). Source: w3.org/2018/credentials/v1.
const W3C_VC_V1_DOC: &str = r#"{
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
        "expirationDate": { "@id": "https://www.w3.org/2018/credentials#expirationDate", "@type": "http://www.w3.org/2001/XMLSchema#dateTime" },
        "holder": { "@id": "https://www.w3.org/2018/credentials#holder", "@type": "@id" },
        "issuer": { "@id": "https://www.w3.org/2018/credentials#issuer", "@type": "@id" },
        "issuanceDate": { "@id": "https://www.w3.org/2018/credentials#issuanceDate", "@type": "http://www.w3.org/2001/XMLSchema#dateTime" },
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
        W3C_VC_V1 => Some(W3C_VC_V1_DOC),
        ALEXANDRIA_V1 => Some(ALEXANDRIA_V1_DOC),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_returns_w3c_vc_v1_context() {
        // Offline verification (spec §20) requires that the W3C and
        // Alexandria contexts are embedded — never fetched at runtime.
        let doc = lookup_context(W3C_VC_V1).expect("W3C context embedded");
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
