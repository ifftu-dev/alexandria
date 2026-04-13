//! Build a metadata-only Cardano tx that embeds a credential hash as
//! an integrity anchor. Distinct from the NFT mint path (§12.3, §19).
//! Stub — implementation in PR 8.

use crate::crypto::did::Did;

/// CBOR bytes of a signed tx carrying:
/// `metadata: { alexandria_anchor: { credential_hash, issuer_did, issued_at } }`
pub struct AnchorTx {
    pub signed_cbor: Vec<u8>,
    pub tx_hash: String,
}

pub async fn build_anchor_metadata_tx(
    _credential_hash: &str,
    _issuer_did: &Did,
    _issued_at: &str,
    _wallet: &crate::crypto::wallet::Wallet,
    _blockfrost: &crate::cardano::blockfrost::BlockfrostClient,
) -> Result<AnchorTx, String> {
    unimplemented!("PR 8 — build anchor metadata tx")
}

#[cfg(test)]
mod tests {
    // Most behaviour here (fee estimation, UTxO selection, Blockfrost
    // chain-tip fetch) lives behind network and wallet fixtures that
    // only make sense once PR 8 lands. For PR 2 we just pin the type
    // surface: AnchorTx is a plain data container — its shape must
    // match what `anchor_queue::tick` stores when it records success.
    use super::*;

    #[test]
    fn anchor_tx_is_a_plain_data_container() {
        // Shape-level regression guard: if this compiles with these
        // field names + types, the queue processor's persistence code
        // in PR 8 will line up without churn.
        let tx = AnchorTx {
            signed_cbor: vec![0x82, 0x00, 0x00],
            tx_hash: "deadbeef".into(),
        };
        assert_eq!(tx.tx_hash, "deadbeef");
        assert_eq!(tx.signed_cbor.len(), 3);
    }
}
