//! Build a metadata-only Cardano tx that embeds a credential hash as
//! an integrity anchor. Distinct from the NFT mint path (§12.3, §19).
//!
//! The tx body is the simplest possible: one ADA-only input from the
//! funding wallet, one output back to it (less fee), no mint, no datums,
//! no Plutus refs. The Alexandria treasury funds it when configured
//! (`cardano::treasury`); otherwise the issuer's own wallet does. Either
//! way the issuer's payment key, which is also its `did:key`, is a
//! required signer, so the anchor is attributable to the issuer and not
//! to whoever paid. Auxiliary data carries a single map under
//! `ALEXANDRIA_ANCHOR_LABEL` (1697) per `cardano::script_refs`:
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
use pallas_primitives::{Metadatum, MetadatumLabel};
use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};

use crate::cardano::script_refs::ALEXANDRIA_ANCHOR_LABEL;
use crate::cardano::treasury::TreasuryPayer;
use crate::cardano::tx_builder::{self, MIN_UTXO_LOVELACE, TTL_OFFSET};
use crate::cardano::types::UTxO;
use crate::crypto::did::Did;

/// CBOR bytes of a signed tx carrying:
/// `metadata: { 1697: { credential_hash, issuer_did, issued_at, v } }`.
pub struct AnchorTx {
    pub signed_cbor: Vec<u8>,
    pub tx_hash: String,
}

/// Build and sign an anchor tx for `credential_hash`.
///
/// `treasury`, when present, pays: its UTxO is spent and the change returns
/// to it, and it witnesses alongside the issuer. Without it the issuer's
/// wallet pays.
pub async fn build_anchor_metadata_tx(
    credential_hash: &str,
    issuer_did: &Did,
    issued_at: &str,
    wallet: &crate::crypto::wallet::Wallet,
    treasury: Option<&TreasuryPayer>,
    blockfrost: &crate::cardano::blockfrost::BlockfrostClient,
) -> Result<AnchorTx, String> {
    let funding_address = treasury
        .map(|t| t.address.as_str())
        .unwrap_or(wallet.payment_address.as_str());

    // 1. Query chain state in parallel — same pattern as tx_builder.
    let (utxos_res, params_res, tip_res) = tokio::join!(
        blockfrost.get_utxos(funding_address),
        blockfrost.get_protocol_params(),
        blockfrost.get_tip_slot(),
    );
    let utxos = utxos_res.map_err(|e| format!("get_utxos: {e}"))?;
    let params = params_res.map_err(|e| format!("get_protocol_params: {e}"))?;
    let tip_slot = tip_res.map_err(|e| format!("get_tip_slot: {e}"))?;

    if utxos.is_empty() {
        return Err(format!("no UTxOs at funding address {funding_address}"));
    }
    let fee = tx_builder::estimate_fee(&params, if treasury.is_some() { 2 } else { 1 });
    let selected = select_funding_utxo(&utxos, fee + MIN_UTXO_LOVELACE).ok_or_else(|| {
        format!(
            "insufficient funds at {funding_address}: no ADA-only UTxO holds {} lovelace",
            fee + MIN_UTXO_LOVELACE
        )
    })?;

    // 2. Parse + size the tx.
    let pallas_addr = PallasAddress::from_bech32(funding_address)
        .map_err(|e| format!("bad funding address: {e}"))?;
    let change = selected.lovelace() - fee;
    let input_tx_hash =
        tx_builder::parse_tx_hash(&selected.tx_hash).map_err(|e| format!("parse tx hash: {e}"))?;

    // 3. Build the body via pallas-txbuilder. No mint / no Plutus. The
    //    issuer is a required signer whoever pays.
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

    // 5. Sign: the issuer always, the treasury when it funded the input.
    let issuer_key = tx_builder::extended_private_key(&wallet.payment_key_extended)
        .map_err(|e| format!("payment key: {e}"))?;
    let signed_cbor = match treasury {
        Some(t) => tx_builder::sign_raw_tx_many(&with_metadata, &[&issuer_key, &t.key]),
        None => tx_builder::sign_raw_tx(&with_metadata, &issuer_key),
    }
    .map_err(|e| format!("sign: {e}"))?;
    let tx_hash = tx_builder::compute_tx_hash(&signed_cbor).map_err(|e| format!("hash: {e}"))?;

    Ok(AnchorTx {
        signed_cbor,
        tx_hash,
    })
}

/// The largest ADA-only UTxO holding at least `needed` lovelace.
///
/// The change output is lovelace-only, so spending a UTxO that carries other
/// assets would strand them; reference-script UTxOs (the treasury deploys
/// validators) must never be spent.
fn select_funding_utxo(utxos: &[UTxO], needed: u64) -> Option<&UTxO> {
    utxos
        .iter()
        .filter(|u| {
            !u.has_reference_script()
                && u.amount.iter().all(|a| a.unit == "lovelace")
                && u.lovelace() >= needed
        })
        .max_by_key(|u| u.lovelace())
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

    fn utxo(index: u64, lovelace: u64, token: bool, script: bool) -> UTxO {
        let mut amount = vec![crate::cardano::types::AmountEntry {
            unit: "lovelace".into(),
            quantity: lovelace.to_string(),
        }];
        if token {
            amount.push(crate::cardano::types::AmountEntry {
                unit: "ab".repeat(28),
                quantity: "1".into(),
            });
        }
        UTxO {
            tx_hash: "00".repeat(32),
            tx_index: index,
            amount,
            reference_script_hash: script.then(|| "cd".repeat(28)),
        }
    }

    #[test]
    fn funding_spends_the_largest_ada_only_utxo_and_never_a_script_or_token() {
        let utxos = [
            utxo(0, 90_000_000, false, true),
            utxo(1, 80_000_000, true, false),
            utxo(2, 6_000_000, false, false),
            utxo(3, 20_000_000, false, false),
        ];
        assert_eq!(select_funding_utxo(&utxos, 5_300_000).unwrap().tx_index, 3);
        assert!(select_funding_utxo(&utxos, 30_000_000).is_none());
        assert!(select_funding_utxo(&utxos[..2], 1).is_none());
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

    /// Live preprod anchor paid by the treasury. Ignored by default. The
    /// issuer is a freshly generated wallet with no funds, so acceptance
    /// shows that the treasury paid and the issuer only signed:
    ///   BLOCKFROST_PROJECT_ID=preprod… ALEXANDRIA_TREASURY_SKEY=… \
    ///   ALEXANDRIA_TREASURY_ADDR=… cargo test -p alexandria-node \
    ///     live_treasury_anchor_preprod -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_treasury_anchor_preprod() {
        let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
        let treasury = TreasuryPayer::from_env().expect("ALEXANDRIA_TREASURY_SKEY/_ADDR");
        let issuer = crate::crypto::wallet::generate_wallet().unwrap();
        let did = crate::crypto::did::derive_did_key(&issuer.signing_key);
        let issued_at = crate::clock::now_rfc3339();
        let hash = hex::encode(blake3::hash(issued_at.as_bytes()).as_bytes());

        let rt = tokio::runtime::Runtime::new().unwrap();
        let outcome = rt.block_on(async {
            let bf = crate::cardano::blockfrost::BlockfrostClient::new(pid).unwrap();
            let built =
                build_anchor_metadata_tx(&hash, &did, &issued_at, &issuer, Some(&treasury), &bf)
                    .await?;
            bf.submit_tx(&built.signed_cbor)
                .await
                .map_err(|e| format!("submit: {e}"))
        });
        println!(
            "LIVE_ANCHOR_RESULT: {outcome:?} hash={hash} issuer={}",
            did.as_str()
        );
        outcome.expect("anchor accepted on preprod");
    }
}
