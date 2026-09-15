//! Network stress tests.
//!
//! The original stress suite exercised evidence gossip, skill-proof
//! aggregation, multi-party attestation, and a challenge committee under
//! simulated load. Those authority paths were retired. Rather than keep an
//! unrunnable test file on disk, the suite is cleared to this stub and will
//! be restored in VC-first form once the rebuilt subsystems land.
//!
//! Tracked follow-up:
//!   - VC gossip stress (publish rate, dedup cache, spam resistance)
//!   - Credential sync merge races
