//! DHT-based public archive discovery — archives advertise themselves
//! on the `/alexandria/archive/1.0` provider-record namespace.
//! Stub — implementation in PR 10.

use crate::crypto::did::Did;

pub async fn find_archives_for(_subject: &Did) -> Result<Vec<String>, String> {
    unimplemented!("PR 10 — archive DHT discovery")
}

pub async fn declare_self_as_archive() -> Result<(), String> {
    unimplemented!("PR 10 — declare archive provider record")
}
