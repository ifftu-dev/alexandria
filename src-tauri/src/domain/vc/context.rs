//! Embedded JSON-LD contexts for offline credential verification.
//! Stubs — contexts are bundled as static strings in PR 4.

/// W3C Verifiable Credentials v1 context URI.
pub const W3C_VC_V1: &str = "https://www.w3.org/2018/credentials/v1";

/// Alexandria protocol v1 context URI.
pub const ALEXANDRIA_V1: &str = "https://alexandria.protocol/context/v1";

/// Return the embedded JSON-LD document for a given context URI, or
/// `None` if we don't ship a local copy.
pub fn lookup_context(_uri: &str) -> Option<&'static str> {
    unimplemented!("PR 4 — embedded JSON-LD contexts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "pending PR 4 — embedded JSON-LD contexts"]
    fn lookup_returns_w3c_vc_v1_context() {
        // Offline verification (spec §20) requires that the W3C and
        // Alexandria contexts are embedded — never fetched at runtime.
        let doc = lookup_context(W3C_VC_V1).expect("W3C context embedded");
        assert!(doc.contains("VerifiableCredential"));
    }

    #[test]
    #[ignore = "pending PR 4 — embedded JSON-LD contexts"]
    fn lookup_returns_alexandria_v1_context() {
        let doc = lookup_context(ALEXANDRIA_V1).expect("Alexandria context embedded");
        assert!(doc.contains("Alexandria") || doc.contains("alexandria"));
    }

    #[test]
    #[ignore = "pending PR 4 — embedded JSON-LD contexts"]
    fn lookup_unknown_context_returns_none() {
        assert!(lookup_context("https://example.com/unknown/v1").is_none());
    }
}
