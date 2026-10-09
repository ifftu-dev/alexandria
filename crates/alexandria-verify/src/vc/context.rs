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

/// Alexandria credentials declare only the W3C v2 context.
///
/// Every term this envelope adds (`skillId`, `level`, `score`,
/// `evidenceRefs`, `integrity`, `witness`, the credential classes) is
/// resolved by the v2 context's default vocabulary,
/// `https://www.w3.org/ns/credentials/issuer-dependent#`, which the Data
/// Model defines for exactly this purpose. A JSON-LD processor therefore
/// expands the document without fetching anything that is not a W3C
/// document, and nothing that is not resolvable is declared.
pub const ALEXANDRIA_CONTEXTS: &[&str] = &[W3C_VC_V2];

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
    },
    "DataIntegrityProof": {
      "@id": "https://w3id.org/security#DataIntegrityProof",
      "@context": {
        "@protected": true,
        "id": "@id",
        "type": "@type",
        "created": { "@id": "http://purl.org/dc/terms/created", "@type": "http://www.w3.org/2001/XMLSchema#dateTime" },
        "cryptosuite": { "@id": "https://w3id.org/security#cryptosuite", "@type": "https://w3id.org/security#cryptosuiteString" },
        "proofPurpose": { "@id": "https://w3id.org/security#proofPurpose", "@type": "@vocab" },
        "proofValue": { "@id": "https://w3id.org/security#proofValue", "@type": "https://w3id.org/security#multibase" },
        "verificationMethod": { "@id": "https://w3id.org/security#verificationMethod", "@type": "@id" }
      }
    },
    "BitstringStatusListEntry": {
      "@id": "https://www.w3.org/ns/credentials/status#BitstringStatusListEntry",
      "@context": {
        "@protected": true,
        "id": "@id",
        "type": "@type",
        "statusPurpose": "https://www.w3.org/ns/credentials/status#statusPurpose",
        "statusListIndex": "https://www.w3.org/ns/credentials/status#statusListIndex",
        "statusListCredential": { "@id": "https://www.w3.org/ns/credentials/status#statusListCredential", "@type": "@id" }
      }
    },
    "BitstringStatusListCredential": "https://www.w3.org/ns/credentials/status#BitstringStatusListCredential",
    "BitstringStatusList": {
      "@id": "https://www.w3.org/ns/credentials/status#BitstringStatusList",
      "@context": {
        "@protected": true,
        "id": "@id",
        "type": "@type",
        "statusPurpose": "https://www.w3.org/ns/credentials/status#statusPurpose",
        "encodedList": { "@id": "https://www.w3.org/ns/credentials/status#encodedList", "@type": "https://w3id.org/security#multibase" }
      }
    }
  }
}"#;

/// Return the embedded JSON-LD document for a given context URI, or
/// `None` if we don't ship a local copy.
pub fn lookup_context(uri: &str) -> Option<&'static str> {
    match uri {
        W3C_VC_V2 => Some(W3C_VC_V2_DOC),
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
        for term in [
            "validFrom",
            "validUntil",
            "DataIntegrityProof",
            "cryptosuite",
            "proofValue",
            "BitstringStatusListEntry",
            "statusListIndex",
            "encodedList",
        ] {
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

    /// The only context a credential declares is one a verifier can resolve.
    #[test]
    fn every_declared_context_is_a_w3c_document() {
        for uri in ALEXANDRIA_CONTEXTS {
            assert!(uri.starts_with("https://www.w3.org/"), "{uri}");
            assert!(lookup_context(uri).is_some(), "{uri} is not embedded");
        }
    }

    /// A Data Integrity proof document round-trips through the envelope.
    #[test]
    fn a_data_integrity_credential_deserialises() {
        let doc = serde_json::json!({
            "@context": ["https://www.w3.org/ns/credentials/v2"],
            "id": "urn:uuid:di",
            "type": ["VerifiableCredential", "FormalCredential"],
            "issuer": "did:key:z6MkIssuer",
            "validFrom": "2026-01-01T00:00:00Z",
            "credentialSubject": { "id": "did:key:z6MkSubject" },
            "proof": {
                "type": "DataIntegrityProof",
                "cryptosuite": "eddsa-jcs-2022",
                "created": "2026-01-01T00:00:00Z",
                "verificationMethod": "did:key:z6MkIssuer#z6MkIssuer",
                "proofPurpose": "assertionMethod",
                "proofValue": "z3FXQjecWufY46yg5abdVZsXqLhxhueuSoZgNSARiKBsrHtHd7m7bLv1JvmRf8wVPdvmJtTdZnHSUtrTv9hnKaZ8W"
            }
        });
        let vc: VerifiableCredential = serde_json::from_value(doc).expect("parses");
        assert_eq!(vc.proof.cryptosuite, "eddsa-jcs-2022");
        assert!(vc.proof.proof_value.starts_with('z'));
    }
}
