//! Build a metadata-only Cardano tx that embeds a credential hash as
//! an integrity anchor. Distinct from the NFT mint path (§12.3, §19).
//!
//! The tx body is the simplest possible: one input from the wallet's
//! payment address, one output back to the same address (less fee),
//! no mint, no datums, no Plutus refs. Auxiliary data carries a
//! single map under `ALEXANDRIA_ANCHOR_LABEL` (1697) per
//! `cardano::script_refs`:
//!
//! ```text
//! {
//!   1697: {
//!     "credential_hash": <hex>,
//!     "issuer_did":      <did:key:z…>,
//!     "issued_at":       <ISO 8601>,
//!     "v":               1
//!   }
//! }
//! ```
//!
//! Metadata is injected via `tx_builder::inject_metadata` — the
//! pallas-txbuilder `StagingTransaction` API doesn't accept aux
//! data directly so we recompute the body's `auxiliary_data_hash`
//! after embedding.

use pallas_addresses::Address as PallasAddress;
use pallas_codec::utils::KeyValuePairs;
use pallas_crypto::key::ed25519::SecretKeyExtended;
use pallas_primitives::{Metadatum, MetadatumLabel};
use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
use pallas_wallet::PrivateKey;

use crate::cardano::script_refs::ALEXANDRIA_ANCHOR_LABEL;
use crate::cardano::tx_builder::{self, MIN_UTXO_LOVELACE, TTL_OFFSET};
use crate::crypto::did::Did;

/// CBOR bytes of a signed tx carrying:
/// `metadata: { 1697: { credential_hash, issuer_did, issued_at, v } }`.
pub struct AnchorTx {
    pub signed_cbor: Vec<u8>,
    pub tx_hash: String,
}

pub async fn build_anchor_metadata_tx(
    credential_hash: &str,
    issuer_did: &Did,
    issued_at: &str,
    wallet: &crate::crypto::wallet::Wallet,
    blockfrost: &crate::cardano::blockfrost::BlockfrostClient,
) -> Result<AnchorTx, String> {
    // 1. Query chain state in parallel — same pattern as tx_builder.
    let (utxos_res, params_res, tip_res) = tokio::join!(
        blockfrost.get_utxos(&wallet.payment_address),
        blockfrost.get_protocol_params(),
        blockfrost.get_tip_slot(),
    );
    let utxos = utxos_res.map_err(|e| format!("get_utxos: {e}"))?;
    let params = params_res.map_err(|e| format!("get_protocol_params: {e}"))?;
    let tip_slot = tip_res.map_err(|e| format!("get_tip_slot: {e}"))?;

    if utxos.is_empty() {
        return Err("no UTxOs at payment address".into());
    }
    let selected =
        crate::cardano::blockfrost::BlockfrostClient::select_utxo(&utxos, MIN_UTXO_LOVELACE)
            .ok_or_else(|| "no UTxO with sufficient lovelace".to_string())?;

    // 2. Parse + size the tx.
    let pallas_addr = PallasAddress::from_bech32(&wallet.payment_address)
        .map_err(|e| format!("bad payment address: {e}"))?;
    let input_lovelace = selected.lovelace();
    let fee = tx_builder::estimate_fee(&params, 1);
    if input_lovelace < fee + MIN_UTXO_LOVELACE {
        return Err(format!(
            "insufficient funds: need {} lovelace, have {}",
            fee + MIN_UTXO_LOVELACE,
            input_lovelace
        ));
    }
    let change = input_lovelace - fee;
    let input_tx_hash =
        tx_builder::parse_tx_hash(&selected.tx_hash).map_err(|e| format!("parse tx hash: {e}"))?;

    // 3. Build the body via pallas-txbuilder. No mint / no Plutus.
    let staging = StagingTransaction::new()
        .input(Input::new(input_tx_hash, selected.tx_index))
        .output(Output::new(pallas_addr, change))
        .disclosed_signer(pallas_crypto::hash::Hash::<28>::from(
            wallet.payment_key_hash,
        ))
        .fee(fee)
        .invalid_from_slot(tip_slot + TTL_OFFSET)
        .network_id(0); // preprod

    let built = staging
        .build_conway_raw()
        .map_err(|e| format!("build_conway_raw: {e}"))?;

    // 4. Build + inject the anchor metadata.
    let metadata = build_anchor_metadata(credential_hash, issuer_did, issued_at);
    let (with_metadata, _hash_after_inject) =
        tx_builder::inject_metadata(&built.tx_bytes.0, metadata)
            .map_err(|e| format!("inject_metadata: {e}"))?;

    // 5. Sign.
    // Safety: bytes were derived via pallas-wallet BIP32 in `crypto::wallet`
    // — the BIP32-Ed25519 clamping invariants are upheld by construction.
    let private_key = PrivateKey::Extended(unsafe {
        SecretKeyExtended::from_bytes_unchecked(wallet.payment_key_extended)
    });
    let signed_cbor =
        tx_builder::sign_raw_tx(&with_metadata, &private_key).map_err(|e| format!("sign: {e}"))?;
    let tx_hash = tx_builder::compute_tx_hash(&signed_cbor).map_err(|e| format!("hash: {e}"))?;

    Ok(AnchorTx {
        signed_cbor,
        tx_hash,
    })
}

/// Build the `{ 1697: { … } }` auxiliary-data map. Public so tests +
/// the `anchor_queue` processor + the `preprod_anchor` example can
/// re-derive it for snapshotting.
pub fn build_anchor_metadata(
    credential_hash: &str,
    issuer_did: &Did,
    issued_at: &str,
) -> KeyValuePairs<MetadatumLabel, Metadatum> {
    let inner = Metadatum::Map(KeyValuePairs::from(vec![
        (
            Metadatum::Text("credential_hash".into()),
            Metadatum::Text(credential_hash.into()),
        ),
        (
            Metadatum::Text("issuer_did".into()),
            Metadatum::Text(issuer_did.as_str().into()),
        ),
        (
            Metadatum::Text("issued_at".into()),
            Metadatum::Text(issued_at.into()),
        ),
        // Schema version — bumps when the inner shape changes.
        (Metadatum::Text("v".into()), Metadatum::Int(1.into())),
    ]));
    KeyValuePairs::from(vec![(ALEXANDRIA_ANCHOR_LABEL, inner)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_tx_is_a_plain_data_container() {
        let tx = AnchorTx {
            signed_cbor: vec![0x82, 0x00, 0x00],
            tx_hash: "deadbeef".into(),
        };
        assert_eq!(tx.tx_hash, "deadbeef");
        assert_eq!(tx.signed_cbor.len(), 3);
    }

    #[test]
    fn anchor_metadata_lives_under_label_1697() {
        let kvs = build_anchor_metadata(
            "0123456789abcdef",
            &Did("did:key:zIssuerTest".into()),
            "2026-04-13T00:00:00Z",
        );
        let labels: Vec<MetadatumLabel> = kvs.iter().map(|(l, _)| *l).collect();
        assert_eq!(labels, vec![ALEXANDRIA_ANCHOR_LABEL]);
        assert_eq!(ALEXANDRIA_ANCHOR_LABEL, 1697);
    }

    #[test]
    fn anchor_metadata_carries_required_fields() {
        let kvs = build_anchor_metadata(
            "deadbeef",
            &Did("did:key:zIssuerXyz".into()),
            "2026-04-13T00:00:00Z",
        );
        let inner = match &kvs.iter().next().unwrap().1 {
            Metadatum::Map(m) => m.clone(),
            other => panic!("expected Map, got {:?}", other),
        };
        let keys: Vec<String> = inner
            .iter()
            .filter_map(|(k, _)| match k {
                Metadatum::Text(s) => Some(s.to_string()),
                _ => None,
            })
            .collect();
        assert!(keys.contains(&"credential_hash".to_string()));
        assert!(keys.contains(&"issuer_did".to_string()));
        assert!(keys.contains(&"issued_at".to_string()));
        assert!(keys.contains(&"v".to_string()));
    }

    #[test]
    fn anchor_metadata_is_deterministic() {
        // Two builds with the same inputs ⇒ identical CBOR. Required
        // for the §20.4 survivability bundle to be content-addressable
        // even when it includes anchor records as evidence.
        let a = build_anchor_metadata("h1", &Did("did:key:zA".into()), "2026-04-13T00:00:00Z");
        let b = build_anchor_metadata("h1", &Did("did:key:zA".into()), "2026-04-13T00:00:00Z");
        // KeyValuePairs doesn't impl PartialEq directly across all
        // variants, so encode and compare bytes.
        use pallas_codec::minicbor;
        let mut ba = Vec::new();
        let mut bb = Vec::new();
        minicbor::encode(&a, &mut ba).unwrap();
        minicbor::encode(&b, &mut bb).unwrap();
        assert_eq!(ba, bb);
    }
}
