//! §18 — Selective disclosure: field subset + audience + nonce.

#[tokio::test]
#[ignore = "pending PR 11 — presentation layer"]
async fn presentation_reveals_only_requested_fields() {
    // Create presentation with reveal = ["credentialSubject.claim.level"].
    // Output must not include the raw score or evidence_refs.
    unimplemented!("exercise commands::presentation::create_presentation")
}

#[tokio::test]
#[ignore = "pending PR 11 — presentation layer"]
async fn nonce_reuse_is_rejected_on_verification_side() {
    // Replay protection: same (audience, nonce) seen twice → reject.
    unimplemented!("two verify calls with same nonce/audience must fail the 2nd")
}

#[tokio::test]
#[ignore = "pending PR 11 — presentation layer"]
async fn audience_mismatch_rejected() {
    // Presentation bound to audience X, verified at audience Y → reject.
    unimplemented!("assert audience binding is enforced")
}
