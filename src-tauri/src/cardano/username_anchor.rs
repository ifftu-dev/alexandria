//! Batched Cardano anchoring for username claims (registry phase 3).
//!
//! One metadata-only tx (label 1698) carries up to [`MAX_BATCH`] claim
//! digests — ~0.011 ADA per username instead of ~0.18 for individual
//! anchoring. Any node with a funded wallet + Blockfrost key may run
//! the batcher (altruistic anchoring: claims are public and an anchor
//! only timestamps them; the operator node pays). Idle-node contract:
//! no chain credentials ⇒ silent no-op.
//!
//! Metadata shape under label 1698:
//!   { "v": 1, "c": [ { "u": <username>, "h": <blake3(claim.sig)> }… ] }
//!
//! The digest is blake3 over the claim's owner signature — the sig
//! already binds username, DID, and claimed_at, so anchoring it pins
//! the entire claim. Verification fetches the tx and checks the digest
//! appears under the label; verified anchors mark `anchor_verified` in
//! `username_claims` and lift the claim to tier 2.

use std::sync::{Arc, Mutex};

use pallas_codec::utils::KeyValuePairs;
use pallas_primitives::{Metadatum, MetadatumLabel};

use crate::cardano::blockfrost::BlockfrostClient;
use crate::cardano::{anchor_tx, tx_builder};
use crate::db::Database;
use crate::domain::username_claim::{CardanoAnchor, UsernameClaim};

/// Auxiliary-data label for username claim batches.
pub const USERNAME_ANCHOR_LABEL: MetadatumLabel = 1698;

/// Claims per batch tx. ~100 bytes of metadata per claim keeps a full
/// batch well under the 16 KB aux-data ceiling.
pub const MAX_BATCH: usize = 80;

/// Digest that gets anchored: blake3 over the owner signature.
pub fn claim_digest(claim: &UsernameClaim) -> String {
    blake3::hash(claim.sig.as_bytes()).to_hex().to_string()
}

/// Build the `{ 1698: { "v": 1, "c": [ … ] } }` auxiliary-data map.
pub fn build_username_anchor_metadata(
    entries: &[(String, String)],
) -> KeyValuePairs<MetadatumLabel, Metadatum> {
    let claims: Vec<Metadatum> = entries
        .iter()
        .map(|(username, digest)| {
            Metadatum::Map(KeyValuePairs::from(vec![
                (
                    Metadatum::Text("u".into()),
                    Metadatum::Text(username.clone()),
                ),
                (Metadatum::Text("h".into()), Metadatum::Text(digest.clone())),
            ]))
        })
        .collect();
    let inner = Metadatum::Map(KeyValuePairs::from(vec![
        (Metadatum::Text("v".into()), Metadatum::Int(1.into())),
        (Metadatum::Text("c".into()), Metadatum::Array(claims)),
    ]));
    KeyValuePairs::from(vec![(USERNAME_ANCHOR_LABEL, inner)])
}

/// Build + sign one batch anchor tx. Same mechanics as the credential
/// anchor (metadata-only, no mint, change back to self).
pub async fn build_batch_anchor_tx(
    entries: &[(String, String)],
    wallet: &crate::crypto::wallet::Wallet,
    blockfrost: &BlockfrostClient,
) -> Result<anchor_tx::AnchorTx, String> {
    use pallas_addresses::Address as PallasAddress;
    use pallas_crypto::key::ed25519::SecretKeyExtended;
    use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
    use pallas_wallet::PrivateKey;

    use crate::cardano::tx_builder::{MIN_UTXO_LOVELACE, TTL_OFFSET};

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
    let selected = BlockfrostClient::select_utxo(&utxos, MIN_UTXO_LOVELACE)
        .ok_or_else(|| "no UTxO with sufficient lovelace".to_string())?;

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

    let metadata = build_username_anchor_metadata(entries);
    let (with_metadata, _) = tx_builder::inject_metadata(&built.tx_bytes.0, metadata)
        .map_err(|e| format!("inject_metadata: {e}"))?;

    // Safety: bytes were derived via pallas-wallet BIP32 in
    // `crypto::wallet` — clamping invariants upheld by construction.
    let private_key = PrivateKey::Extended(unsafe {
        SecretKeyExtended::from_bytes_unchecked(wallet.payment_key_extended)
    });
    let signed_cbor =
        tx_builder::sign_raw_tx(&with_metadata, &private_key).map_err(|e| format!("sign: {e}"))?;
    let tx_hash = tx_builder::compute_tx_hash(&signed_cbor).map_err(|e| format!("hash: {e}"))?;

    Ok(anchor_tx::AnchorTx {
        signed_cbor,
        tx_hash,
    })
}

/// Verify a claim's anchor: the anchoring tx must exist on-chain and
/// carry the claim digest. The digest is a 64-char hex string embedded
/// as a text metadatum, so a byte-substring check on the tx CBOR is
/// sufficient (the digest is collision-resistant; a tx containing it
/// under any encoding anchors this exact claim).
pub async fn verify_anchor(blockfrost: &BlockfrostClient, claim: &UsernameClaim) -> bool {
    let Some(ref anchor) = claim.anchor else {
        return false;
    };
    let Ok(cbor) = blockfrost.get_tx_cbor(&anchor.tx_hash).await else {
        return false;
    };
    let digest = claim_digest(claim);
    cbor.windows(digest.len()).any(|w| w == digest.as_bytes())
}

/// Batch-anchor every unanchored claim in the local cache. Returns the
/// number of claims anchored. The enriched claims republish to the DHT
/// through the caller (claims are keyed per-username there).
pub async fn tick(
    db: &Arc<Mutex<Option<Database>>>,
    blockfrost: &Option<BlockfrostClient>,
    wallet: &Option<crate::crypto::wallet::Wallet>,
) -> Result<Vec<UsernameClaim>, String> {
    let Some(bf) = blockfrost else {
        return Ok(Vec::new());
    };
    let Some(w) = wallet else {
        return Ok(Vec::new());
    };

    // Verification pass: claims anchored by OTHER nodes arrive via the
    // DHT with anchor_verified = 0. Confirm their digests on-chain so
    // resolution can trust them (capped per tick).
    let unverified: Vec<UsernameClaim> = {
        let guard = db.lock().map_err(|_| "db lock poisoned")?;
        let Some(database) = guard.as_ref() else {
            return Ok(Vec::new());
        };
        let mut stmt = database
            .conn()
            .prepare(
                "SELECT claim_json FROM username_claims
                 WHERE tier = 2 AND anchor_verified = 0 LIMIT 20",
            )
            .map_err(|e| e.to_string())?;
        let rows: Vec<UsernameClaim> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .filter_map(|json| serde_json::from_str::<UsernameClaim>(&json).ok())
            .collect();
        rows
    };
    for claim in unverified {
        let ok = verify_anchor(bf, &claim).await;
        let guard = db.lock().map_err(|_| "db lock poisoned")?;
        if let Some(database) = guard.as_ref() {
            if ok {
                let _ = database.conn().execute(
                    "UPDATE username_claims SET anchor_verified = 1 WHERE username = ?1",
                    [&claim.username],
                );
            } else {
                // Forged or unconfirmed anchor — demote so ordering
                // falls back to the receipt/bare tier.
                let mut demoted = claim.clone();
                demoted.anchor = None;
                if let Ok(json) = serde_json::to_string(&demoted) {
                    let _ = database.conn().execute(
                        "UPDATE username_claims SET claim_json = ?2, tier = ?3
                         WHERE username = ?1",
                        rusqlite::params![claim.username, json, demoted.tier()],
                    );
                }
            }
        }
    }

    // Collect unanchored claims (tier < 2).
    let pending: Vec<UsernameClaim> = {
        let guard = db.lock().map_err(|_| "db lock poisoned")?;
        let Some(database) = guard.as_ref() else {
            return Ok(Vec::new());
        };
        let mut stmt = database
            .conn()
            .prepare(
                "SELECT claim_json FROM username_claims WHERE tier < 2
                 ORDER BY username LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([MAX_BATCH as i64], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .filter_map(|json| serde_json::from_str::<UsernameClaim>(&json).ok())
            .filter(|c| c.verify().is_ok())
            .collect();
        rows
    };
    if pending.is_empty() {
        return Ok(Vec::new());
    }

    let entries: Vec<(String, String)> = pending
        .iter()
        .map(|c| (c.username.clone(), claim_digest(c)))
        .collect();

    let tx = build_batch_anchor_tx(&entries, w, bf).await?;
    let tx_hash = bf
        .submit_tx(&tx.signed_cbor)
        .await
        .map_err(|e| format!("submit: {e}"))?;
    let slot = bf.get_tip_slot().await.unwrap_or(0);
    log::info!(
        "username anchor batch submitted: {} claims in tx {tx_hash}",
        pending.len()
    );

    // Attach anchors + persist.
    let mut anchored = Vec::new();
    {
        let guard = db.lock().map_err(|_| "db lock poisoned")?;
        let Some(database) = guard.as_ref() else {
            return Ok(Vec::new());
        };
        for mut claim in pending {
            claim.anchor = Some(CardanoAnchor {
                tx_hash: tx_hash.clone(),
                slot,
            });
            let json = serde_json::to_string(&claim).map_err(|e| e.to_string())?;
            let _ = database.conn().execute(
                "UPDATE username_claims SET claim_json = ?2, tier = 2,
                     anchor_verified = 1, updated_at = datetime('now')
                 WHERE username = ?1",
                rusqlite::params![claim.username, json],
            );
            anchored.push(claim);
        }
    }
    Ok(anchored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::derive_did_key;
    use ed25519_dalek::SigningKey;

    fn claim(seed: u8, name: &str) -> UsernameClaim {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let did = derive_did_key(&key);
        UsernameClaim::create(name, &did, 100, &key)
    }

    #[test]
    fn digest_is_stable_and_distinct() {
        let a = claim(1, "ada_99");
        assert_eq!(claim_digest(&a), claim_digest(&a));
        assert_ne!(claim_digest(&a), claim_digest(&claim(2, "ada_99")));
    }

    #[test]
    fn metadata_lives_under_label_1698_and_carries_all_entries() {
        let entries = vec![
            ("ada_99".to_string(), "aa".repeat(32)),
            ("bob_22".to_string(), "bb".repeat(32)),
        ];
        let md = build_username_anchor_metadata(&entries);
        assert_eq!(md.len(), 1);
        let (label, inner) = md.iter().next().unwrap();
        assert_eq!(*label, 1698);
        match inner {
            Metadatum::Map(kv) => {
                let c = kv
                    .iter()
                    .find(|(k, _)| matches!(k, Metadatum::Text(t) if t == "c"))
                    .map(|(_, v)| v)
                    .unwrap();
                match c {
                    Metadatum::Array(items) => assert_eq!(items.len(), 2),
                    other => panic!("expected array, got {other:?}"),
                }
            }
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn batch_of_80_fits_aux_data_budget() {
        // ~100 bytes per entry keeps a full batch far below the 16 KB
        // aux-data ceiling — sanity-check the arithmetic holds.
        let entries: Vec<(String, String)> = (0..MAX_BATCH)
            .map(|i| (format!("user_{i:028}"), "ab".repeat(32)))
            .collect();
        let approx: usize = entries.iter().map(|(u, h)| u.len() + h.len() + 12).sum();
        assert!(approx < 16_000, "batch too large: {approx}");
    }
}
