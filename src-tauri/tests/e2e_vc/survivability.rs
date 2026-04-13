//! §20.4 — exported credentials verify without any Alexandria services.

#[tokio::test]
#[ignore = "pending PR 12 — survivability / export"]
async fn exported_bundle_verifies_with_offline_tooling() {
    // `alex credentials export --out b.jsonld` produces a bundle with
    // VCs + DID docs + status lists. A vanilla W3C verifier (digital-
    // bazaar/vc-js, piped via subprocess in CI) accepts the bundle
    // with no Alexandria services running.
    unimplemented!("shell out to digitalbazaar/vc-js in CI or assert offline verification")
}

#[tokio::test]
#[ignore = "pending PR 12 — survivability / export"]
async fn export_bundle_is_deterministic() {
    // Same credential set + same export time → byte-identical bundle.
    // Needed so bundles round-trip through archival storage.
    unimplemented!("export twice with fixed clock, assert byte-equality")
}
