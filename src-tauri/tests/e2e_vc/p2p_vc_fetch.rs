//! Pull-based credential fetch — authority, allowlist, replay.

#[tokio::test]
#[ignore = "pending PR 9 — vc-fetch request-response"]
async fn public_credential_fetch_returns_vc() {
    // Subject marks credential as public → any peer fetching by ID
    // receives the signed VC.
    unimplemented!("exercise p2p::vc_fetch::handle_fetch_request with public policy")
}

#[tokio::test]
#[ignore = "pending PR 9 — vc-fetch request-response"]
async fn private_credential_fetch_returns_unauthorized() {
    // Default: private. Non-allowlisted requestor → FetchResponse::Unauthorized.
    unimplemented!("assert Unauthorized for non-allowlisted requestor")
}

#[tokio::test]
#[ignore = "pending PR 9 — vc-fetch request-response"]
async fn allowlisted_requestor_receives_private_credential() {
    // Subject explicitly allowlists requestor_did → receives VC.
    unimplemented!("set per-credential allowlist and confirm Ok variant")
}
