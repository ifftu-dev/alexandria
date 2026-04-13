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
