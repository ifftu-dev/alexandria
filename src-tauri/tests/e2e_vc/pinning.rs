//! 5-tier eviction precedence:
//!   1. Subject-authored — NEVER evict
//!   2. PinBoard-committed — NEVER evict while commitment stands
//!   3. DID docs + status lists for known issuers — NEVER evict
//!   4. Active-enrollment courses — evict only on unenroll
//!   5. Cache — LRU within per-type quota
//!
//! Under storage pressure, higher tiers retain while lower tiers evict.

#[tokio::test]
#[ignore = "pending PR 10 — tiered pinning"]
async fn cache_evicts_before_pinboard_content() {
    // Quota full → run maybe_evict → cache items drop, pinboard items stay.
    unimplemented!("seed pins across tiers, induce pressure, assert precedence")
}

#[tokio::test]
#[ignore = "pending PR 10 — tiered pinning"]
async fn subject_authored_content_never_evicts() {
    // Even at 100% quota with all other tiers empty, subject-authored
    // content is preserved. Eviction fails instead.
    unimplemented!("force over-quota with only own-credentials pinned")
}

#[tokio::test]
#[ignore = "pending PR 10 — tiered pinning"]
async fn did_docs_and_status_lists_retained_for_verification() {
    // If we hold a VC from issuer X, X's DID doc + status list must
    // survive eviction regardless of cache pressure.
    unimplemented!("verify did_doc + status_list pins survive LRU pass")
}

#[tokio::test]
#[ignore = "pending PR 10 — tiered pinning"]
async fn revoked_commitment_demotes_to_cache_tier() {
    // After revocation, previously-pinboard content is reclassified
    // and becomes cache-evictable.
    unimplemented!("revoke commitment then run eviction")
}
