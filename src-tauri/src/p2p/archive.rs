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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "pending PR 10 — archive DHT discovery"]
    async fn find_archives_for_unknown_subject_is_empty_not_err() {
        // Discovery is advisory (§20): if nobody advertises an archive
        // for this subject, return an empty vec — callers fall back to
        // direct fetch. Errors reserved for DHT / transport failures.
        let subject = Did("did:key:zUnknownSubject".into());
        let found = find_archives_for(&subject).await.unwrap();
        assert!(found.is_empty());
    }

    #[tokio::test]
    #[ignore = "pending PR 10 — archive provider record"]
    async fn declare_self_as_archive_is_idempotent() {
        // Re-declaring the provider record MUST NOT error — the DHT
        // refresh loop calls this on a timer.
        declare_self_as_archive().await.unwrap();
        declare_self_as_archive().await.unwrap();
    }
}
