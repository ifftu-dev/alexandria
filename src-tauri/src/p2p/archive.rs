//! DHT-based public archive discovery — archives advertise themselves
//! on the `/alexandria/archive/1.0` provider-record namespace.
//!
//! PR 10 lands the function surface + idle-node contract. Real DHT
//! provider-record publication wires in when libp2p Kademlia grows
//! the `/alexandria/archive/1.0` namespace; until then
//! `find_archives_for` returns an empty vec (advisory: nobody
//! advertises this subject) and `declare_self_as_archive` is an
//! idempotent no-op that refresh-loop callers can hit safely.

use crate::crypto::did::Did;

pub async fn find_archives_for(_subject: &Did) -> Result<Vec<String>, String> {
    // Discovery is advisory (§20.4): an empty result means callers
    // fall back to direct fetch / PinBoard. We don't error so a
    // missing DHT advert isn't treated as a transport failure.
    Ok(Vec::new())
}

pub async fn declare_self_as_archive() -> Result<(), String> {
    // Idempotent by construction — refresh-loop callers can invoke
    // this on a timer without observable effect until the real DHT
    // wiring lands.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn find_archives_for_unknown_subject_is_empty_not_err() {
        // Discovery is advisory (§20): if nobody advertises an archive
        // for this subject, return an empty vec — callers fall back to
        // direct fetch. Errors reserved for DHT / transport failures.
        let subject = Did("did:key:zUnknownSubject".into());
        let found = find_archives_for(&subject).await.unwrap();
        assert!(found.is_empty());
    }

    #[tokio::test]
    async fn declare_self_as_archive_is_idempotent() {
        // Re-declaring the provider record MUST NOT error — the DHT
        // refresh loop calls this on a timer.
        declare_self_as_archive().await.unwrap();
        declare_self_as_archive().await.unwrap();
    }
}
