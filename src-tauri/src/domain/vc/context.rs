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
