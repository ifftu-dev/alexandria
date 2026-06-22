//! Reader for the on-chain relay registry (metadata label
//! [`REGISTRY_LABEL`]). The governance key publishes a metadata-only tx
//! listing the relay PeerIds authorized to issue username receipts; this
//! module fetches the latest such tx via Blockfrost, verifies it was
//! authored by the pinned governance address, and feeds the result into
//! [`relay_registry::set_onchain_issuers`].
//!
//! Trust model: a label-[`REGISTRY_LABEL`] tx counts only if one of its
//! *inputs* is spent from [`GOV_ADDRESS`] — only the gov key can do
//! that, so a relay cannot publish a registry that names itself. Genesis
//! issuers stay trusted regardless (see [`relay_registry`]), so naming
//! keeps working when the chain is unreachable or the registry is empty.
//!
//! Publishing is intentionally NOT in the app: the gov key lives outside
//! the client and updates are issued with `cardano-cli`. This module is
//! read-only.

use libp2p::PeerId;

use super::blockfrost::BlockfrostClient;
use crate::p2p::relay_registry::{self, GOV_ADDRESS, REGISTRY_LABEL};

/// Metadata payload shape under the label: `{ "v": 1, "seq": N,
/// "r": [ "<peer_id>", … ] }`. Returns `(seq, peer_ids)`.
///
/// Integers may arrive from Blockfrost as JSON numbers or strings
/// (metadata encodings differ); both are accepted.
pub fn parse_registry_metadata(json: &serde_json::Value) -> Option<(u64, Vec<String>)> {
    let seq = json.get("seq").and_then(|s| {
        s.as_u64()
            .or_else(|| s.as_str().and_then(|t| t.parse().ok()))
    })?;
    let relays = json.get("r")?.as_array()?;
    let peer_ids: Vec<String> = relays
        .iter()
        .filter_map(|v| v.as_str())
        // Only keep well-formed PeerIds — a malformed entry can never
        // match a real relay anyway, dropping it keeps the set clean.
        .filter(|s| s.parse::<PeerId>().is_ok())
        .map(|s| s.to_string())
        .collect();
    Some((seq, peer_ids))
}

/// Fetch the authoritative issuer set from chain. Returns the
/// `(seq, peer_ids)` of the highest-seq registry tx that was authored by
/// [`GOV_ADDRESS`], or `None` when none exists / chain unreachable.
pub async fn fetch_authorized_issuers(blockfrost: &BlockfrostClient) -> Option<(u64, Vec<String>)> {
    let txs = blockfrost
        .get_metadata_by_label(REGISTRY_LABEL)
        .await
        .ok()?;

    let mut best: Option<(u64, Vec<String>)> = None;
    for tx in txs {
        let Some((seq, peer_ids)) = parse_registry_metadata(&tx.json_metadata) else {
            continue;
        };
        // Skip work if it can't beat what we already have.
        if best.as_ref().is_some_and(|(s, _)| seq <= *s) {
            continue;
        }
        // Provenance: a non-collateral input must come from the gov
        // address. Only the gov key can spend the gov UTxO, so this
        // proves authorship without trusting the metadata itself.
        let Ok(utxos) = blockfrost.get_tx_utxos(&tx.tx_hash).await else {
            continue;
        };
        let authored_by_gov = utxos
            .inputs
            .iter()
            .any(|i| !i.collateral && i.address == GOV_ADDRESS);
        if !authored_by_gov {
            continue;
        }
        best = Some((seq, peer_ids));
    }
    best
}

/// Fetch the registry from chain and, on success, install it as the
/// on-chain issuer set. Returns the installed `(seq, peer_ids)` so the
/// caller can cache it (last-known-good). A failed/empty fetch leaves
/// the current set untouched — genesis issuers always remain trusted.
pub async fn refresh_from_chain(blockfrost: &BlockfrostClient) -> Option<(u64, Vec<String>)> {
    let (seq, issuers) = fetch_authorized_issuers(blockfrost).await?;
    relay_registry::set_onchain_issuers(issuers.clone());
    Some((seq, issuers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_peer() -> String {
        libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id()
            .to_string()
    }

    #[test]
    fn parses_seq_and_relays() {
        let a = valid_peer();
        let b = valid_peer();
        let md = json!({ "v": 1, "seq": 7, "r": [a, b] });
        let (seq, relays) = parse_registry_metadata(&md).expect("parses");
        assert_eq!(seq, 7);
        assert_eq!(relays.len(), 2);
    }

    #[test]
    fn accepts_stringified_seq() {
        let md = json!({ "v": 1, "seq": "42", "r": [valid_peer()] });
        assert_eq!(parse_registry_metadata(&md).unwrap().0, 42);
    }

    #[test]
    fn drops_malformed_peer_ids() {
        let good = valid_peer();
        let md = json!({ "v": 1, "seq": 1, "r": [good, "not-a-peer-id", ""] });
        let (_, relays) = parse_registry_metadata(&md).unwrap();
        assert_eq!(relays.len(), 1);
    }

    #[test]
    fn missing_fields_yield_none() {
        assert!(parse_registry_metadata(&json!({ "v": 1, "r": [] })).is_none()); // no seq
        assert!(parse_registry_metadata(&json!({ "v": 1, "seq": 1 })).is_none());
        // no r
    }
}
