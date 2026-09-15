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

use pallas_codec::utils::KeyValuePairs;
use pallas_primitives::{Metadatum, MetadatumLabel};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::cardano::blockfrost::BlockfrostClient;
use crate::cardano::submission::{self, Journal, Member, Operation, Submission, SubmissionStatus};
use crate::cardano::{anchor_tx, tx_builder};
use crate::domain::username_claim::{CardanoAnchor, UsernameClaim};

/// Auxiliary-data label for username claim batches.
pub const USERNAME_ANCHOR_LABEL: MetadatumLabel = 1698;

/// Claims per batch tx. ~100 bytes of metadata per claim keeps a full
/// batch well under the 16 KB aux-data ceiling.
pub const MAX_BATCH: usize = 80;

const OPERATION_KIND: &str = "username_anchor_batch";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchMember {
    username: String,
    signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchContext {
    version: u32,
    claims: Vec<BatchMember>,
}

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
    use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};

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

    let private_key = tx_builder::extended_private_key(&wallet.payment_key_extended)
        .map_err(|e| format!("payment key: {e}"))?;
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
pub async fn verify_anchor(
    blockfrost: &BlockfrostClient,
    claim: &UsernameClaim,
) -> Result<Option<bool>, String> {
    let Some(ref anchor) = claim.anchor else {
        return Ok(Some(false));
    };
    let Some(receipt) = blockfrost
        .get_transaction_receipt(&anchor.tx_hash)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    if !receipt.valid_contract || receipt.slot != anchor.slot {
        return Ok(Some(false));
    }
    let cbor = blockfrost
        .get_tx_cbor(&anchor.tx_hash)
        .await
        .map_err(|e| e.to_string())?;
    if tx_builder::compute_tx_hash(&cbor).map_err(|e| e.to_string())? != anchor.tx_hash {
        return Ok(Some(false));
    }
    let digest = claim_digest(claim);
    Ok(Some(
        cbor.windows(digest.len()).any(|w| w == digest.as_bytes()),
    ))
}

/// Batch-anchor every unanchored claim in the local cache. Returns the
/// number of claims anchored. The enriched claims republish to the DHT
/// through the caller (claims are keyed per-username there).
pub(crate) async fn tick(
    journal: &Journal,
    blockfrost: &Option<BlockfrostClient>,
    wallet: &Option<crate::crypto::wallet::Wallet>,
) -> Result<Vec<UsernameClaim>, String> {
    let Some(bf) = blockfrost else {
        return Ok(Vec::new());
    };
    let mut anchored = recover_batches(journal, bf).await?;

    // Verification pass: claims anchored by OTHER nodes arrive via the
    // DHT with anchor_verified = 0. Confirm their digests on-chain so
    // resolution can trust them (capped per tick).
    let unverified: Vec<(String, UsernameClaim)> = journal
        .run("username_anchor.unverified", |database| {
            let mut stmt = database
                .conn()
                .prepare(
                    "SELECT claim_json FROM username_claims
                 WHERE tier = 2 AND anchor_verified = 0 LIMIT 20",
                )
                .map_err(|e| e.to_string())?;
            let rows: Vec<(String, UsernameClaim)> = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .filter_map(|json| {
                    serde_json::from_str::<UsernameClaim>(&json)
                        .ok()
                        .map(|claim| (json, claim))
                })
                .collect();
            Ok(rows)
        })
        .await?;
    for (original_json, claim) in unverified {
        let ok = match verify_anchor(bf, &claim).await {
            Ok(Some(ok)) => ok,
            Ok(None) => continue,
            Err(error) => {
                log::debug!("username anchor verification pending: {error}");
                continue;
            }
        };
        journal.run("username_anchor.verification", move |database| {
            // Only mutate the exact snapshot whose evidence was checked.
            // A newer receipt, release, or owner claim must not be overwritten.
            if ok {
                let _ = database.conn().execute(
                    "UPDATE username_claims SET anchor_verified = 1 WHERE username = ?1 AND claim_json = ?2",
                    rusqlite::params![claim.username, original_json],
                );
            } else {
                // Forged or unconfirmed anchor — demote so ordering
                // falls back to the receipt/bare tier.
                let mut demoted = claim.clone();
                demoted.anchor = None;
                if let Ok(json) = serde_json::to_string(&demoted) {
                    let _ = database.conn().execute(
                        "UPDATE username_claims SET claim_json = ?2, tier = ?3
                         WHERE username = ?1 AND claim_json = ?4",
                        rusqlite::params![claim.username, json, demoted.tier(), original_json],
                    );
                }
            }
            Ok(())
        }).await?;
    }

    let Some(w) = wallet else {
        return Ok(anchored);
    };

    // Collect unanchored claims (tier < 2).
    let pending: Vec<UsernameClaim> = journal.run("username_anchor.pending", |database| {
        let mut stmt = database
            .conn()
            .prepare(
                "SELECT claim_json FROM username_claims WHERE tier < 2 AND NOT EXISTS
                   (SELECT 1 FROM chain_submission_members m WHERE m.network = 'cardano-preprod'
                    AND m.member_kind = 'username_claim' AND m.member_id = json_extract(claim_json, '$.sig'))
                 ORDER BY username LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([MAX_BATCH as i64], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .filter_map(|json| serde_json::from_str::<UsernameClaim>(&json).ok())
            .filter(|c| c.verify().is_ok() && c.release.is_none())
            .collect();
        Ok(rows)
    }).await?;
    if pending.is_empty() {
        return Ok(anchored);
    }

    let entries: Vec<(String, String)> = pending
        .iter()
        .map(|c| (c.username.clone(), claim_digest(c)))
        .collect();

    let tx = build_batch_anchor_tx(&entries, w, bf).await?;
    let context = BatchContext {
        version: 1,
        claims: pending
            .iter()
            .map(|claim| BatchMember {
                username: claim.username.clone(),
                signature: claim.sig.clone(),
            })
            .collect(),
    };
    let context_json = serde_json::to_string(&context).map_err(|e| e.to_string())?;
    let batch_id = crate::crypto::hash::entity_id(&[OPERATION_KIND, &context_json]);
    let members: Vec<Member<'_>> = context
        .claims
        .iter()
        .map(|claim| Member {
            kind: "username_claim",
            id: &claim.signature,
        })
        .collect();
    let operation = Operation {
        kind: OPERATION_KIND,
        id: &batch_id,
    };
    let submitted = submission::submit_once_with_members(
        journal,
        bf,
        operation,
        &tx.signed_cbor,
        &context_json,
        &members,
    )
    .await?;
    // An acknowledgement is not a verified anchor. Recovery attaches it
    // only after a ledger receipt identifies successful execution and slot.
    anchored.extend(
        journal
            .run("username_anchor.project", move |database| {
                let operation = Operation {
                    kind: OPERATION_KIND,
                    id: &batch_id,
                };
                project_batch(database.conn(), operation, &submitted)
            })
            .await?,
    );
    Ok(anchored)
}

async fn recover_batches(
    journal: &Journal,
    bf: &BlockfrostClient,
) -> Result<Vec<UsernameClaim>, String> {
    let ids = journal
        .run("username_anchor.scan", |database| {
            submission::unapplied_operations(database.conn(), OPERATION_KIND, 10)
        })
        .await?;
    let mut anchored = Vec::new();
    for id in ids {
        let operation = Operation {
            kind: OPERATION_KIND,
            id: &id,
        };
        match submission::reconcile(journal, bf, operation).await {
            Ok(Some(recovered)) => {
                let id = id.clone();
                anchored.extend(
                    journal
                        .run("username_anchor.recover", move |database| {
                            let operation = Operation {
                                kind: OPERATION_KIND,
                                id: &id,
                            };
                            project_batch(database.conn(), operation, &recovered)
                        })
                        .await?,
                )
            }
            Ok(None) => return Err("username batch checkpoint missing".into()),
            Err(error) => log::debug!("username batch reconciliation pending: {error}"),
        }
    }
    Ok(anchored)
}

fn project_batch(
    conn: &rusqlite::Connection,
    operation: Operation<'_>,
    submitted: &Submission,
) -> Result<Vec<UsernameClaim>, String> {
    if !matches!(
        submitted.status,
        SubmissionStatus::Confirmed | SubmissionStatus::FailedOnChain
    ) {
        return Ok(Vec::new());
    }
    let slot = submitted
        .confirmed_slot
        .ok_or("username anchor receipt has no slot")?;
    let context: BatchContext =
        serde_json::from_str(&submitted.context_json).map_err(|e| e.to_string())?;
    if context.version != 1 {
        return Err("unsupported username batch recovery version".into());
    }
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let mut anchored = Vec::new();
    if submitted.status == SubmissionStatus::Confirmed {
        for member in context.claims {
            let row: Option<(String, bool)> = tx
                .query_row(
                    "SELECT claim_json, anchor_verified FROM username_claims WHERE username = ?1",
                    [&member.username],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            let Some((json, verified)) = row else {
                continue;
            };
            let mut current: UsernameClaim =
                serde_json::from_str(&json).map_err(|e| e.to_string())?;
            if current.username != member.username
                || current.sig != member.signature
                || current.verify().is_err()
            {
                continue;
            }
            if verified
                && current
                    .anchor
                    .as_ref()
                    .is_some_and(|anchor| anchor.slot <= slot)
            {
                continue;
            }
            current.anchor = Some(CardanoAnchor {
                tx_hash: submitted.tx_hash.clone(),
                slot,
            });
            let json = serde_json::to_string(&current).map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE username_claims SET claim_json = ?2, tier = 2, anchor_verified = 1,
                        updated_at = datetime('now') WHERE username = ?1",
                rusqlite::params![member.username, json],
            )
            .map_err(|e| e.to_string())?;
            anchored.push(current);
        }
    }
    submission::mark_applied(&tx, operation)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(anchored)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;
    use crate::cardano::test_chain::{self, FakeChain, TestProfile, SHORT_LIMITS};
    use crate::crypto::did::derive_did_key;
    use crate::db::Database;
    use ed25519_dalek::SigningKey;

    #[tokio::test]
    async fn timed_out_batch_is_reconciled_without_rebuild_or_resubmission() {
        let profile = TestProfile::migrated();
        let original = claim(1, "ada_99");
        profile.with_conn(|conn| cache(conn, &original));
        let (chain, included) = FakeChain::stalled_submit(42).await;
        let client = Some(chain.client(SHORT_LIMITS));
        let wallet = Some(test_chain::wallet());
        let journal = profile.background();

        // The POST reaches the provider, then the client deadline expires.
        assert!(tick(&journal, &client, &wallet).await.unwrap().is_empty());
        let batch = profile
            .with_conn(|conn| submission::unapplied_operations(conn, OPERATION_KIND, 10).unwrap());
        assert_eq!(batch.len(), 1);
        let saved = profile.with_conn(|conn| {
            submission::lookup(
                conn,
                Operation {
                    kind: OPERATION_KIND,
                    id: &batch[0],
                },
            )
            .unwrap()
            .unwrap()
        });
        assert_eq!(saved.status, SubmissionStatus::OutcomeUnknown);

        // "Not found" neither anchors nor frees the reserved claim for a new batch.
        assert!(tick(&journal, &client, &wallet).await.unwrap().is_empty());
        assert_eq!(
            profile.with_conn(|conn| cached(conn, &original.username)),
            original
        );

        included.store(true, Ordering::Release);
        let anchored = tick(&journal, &client, &wallet).await.unwrap();
        assert_eq!(anchored.len(), 1);
        let anchor = anchored[0].anchor.as_ref().unwrap();
        assert_eq!(
            (anchor.tx_hash.as_str(), anchor.slot),
            (saved.tx_hash.as_str(), 42)
        );
        let address = &wallet.as_ref().unwrap().payment_address;
        assert_eq!(chain.count("POST /tx/submit"), 1);
        assert_eq!(chain.count(&format!("GET /addresses/{address}/utxos")), 1);
        assert_eq!(chain.count(&format!("GET /txs/{}", saved.tx_hash)), 2);
        assert_eq!(chain.requests().len(), 6);
    }

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

    fn cache(conn: &rusqlite::Connection, claim: &UsernameClaim) {
        conn.execute(
            "INSERT OR REPLACE INTO username_claims (username, did, claimed_at, tier, claim_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                claim.username,
                claim.did,
                claim.claimed_at,
                claim.tier(),
                serde_json::to_string(claim).unwrap()
            ],
        )
        .unwrap();
    }

    fn checkpoint(
        conn: &rusqlite::Connection,
        claims: &[UsernameClaim],
        status: &str,
    ) -> Submission {
        let context = BatchContext {
            version: 1,
            claims: claims
                .iter()
                .map(|claim| BatchMember {
                    username: claim.username.clone(),
                    signature: claim.sig.clone(),
                })
                .collect(),
        };
        conn.execute(
            "INSERT INTO chain_submissions
             (network, operation_kind, operation_id, tx_hash, signed_cbor, context_json, status, confirmed_slot)
             VALUES ('cardano-preprod', ?1, 'batch', ?2, X'00', ?3, ?4, ?5)",
            rusqlite::params![OPERATION_KIND, "a".repeat(64), serde_json::to_string(&context).unwrap(), status,
                matches!(status, "confirmed" | "failed_on_chain").then_some(42)],
        ).unwrap();
        submission::lookup(
            conn,
            Operation {
                kind: OPERATION_KIND,
                id: "batch",
            },
        )
        .unwrap()
        .unwrap()
    }

    fn cached(conn: &rusqlite::Connection, username: &str) -> UsernameClaim {
        let json: String = conn
            .query_row(
                "SELECT claim_json FROM username_claims WHERE username = ?1",
                [username],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn recovery_uses_actual_slot_preserves_release_and_skips_replaced_owner() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let original = claim(1, "ada_99");
        let replaced = claim(2, "bob_22");
        let saved = checkpoint(db.conn(), &[original.clone(), replaced], "confirmed");
        let mut released = original.clone();
        released.release(200, &SigningKey::from_bytes(&[1; 32]));
        cache(db.conn(), &released);
        let new_owner = claim(3, "bob_22");
        cache(db.conn(), &new_owner);
        let operation = Operation {
            kind: OPERATION_KIND,
            id: "batch",
        };
        let output = project_batch(db.conn(), operation, &saved).unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].release, released.release);
        assert_eq!(output[0].anchor.as_ref().unwrap().slot, 42);
        assert_eq!(cached(db.conn(), "bob_22"), new_owner);
        assert!(
            submission::unapplied_operations(db.conn(), OPERATION_KIND, 10)
                .unwrap()
                .is_empty()
        );
        assert!(project_batch(db.conn(), operation, &saved)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn interrupted_projection_rolls_back_all_claims_and_remains_recoverable() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let original = claim(1, "ada_99");
        cache(db.conn(), &original);
        let saved = checkpoint(db.conn(), std::slice::from_ref(&original), "confirmed");
        db.conn()
            .execute_batch(
                "CREATE TRIGGER fail_application BEFORE UPDATE OF applied_at ON chain_submissions
             BEGIN SELECT RAISE(ABORT, 'injected application failure'); END;",
            )
            .unwrap();
        let operation = Operation {
            kind: OPERATION_KIND,
            id: "batch",
        };
        assert!(project_batch(db.conn(), operation, &saved)
            .unwrap_err()
            .contains("injected application failure"));
        assert_eq!(cached(db.conn(), &original.username), original);
        assert_eq!(
            submission::unapplied_operations(db.conn(), OPERATION_KIND, 10)
                .unwrap()
                .len(),
            1
        );
        db.conn()
            .execute_batch("DROP TRIGGER fail_application")
            .unwrap();
        assert_eq!(
            project_batch(db.conn(), operation, &saved).unwrap().len(),
            1
        );
    }

    #[test]
    fn submission_acknowledgement_or_failed_script_does_not_create_verified_anchor() {
        for status in ["outcome_unknown", "submitted", "failed_on_chain"] {
            let db = Database::open_in_memory().unwrap();
            db.run_migrations().unwrap();
            let original = claim(1, "ada_99");
            cache(db.conn(), &original);
            let saved = checkpoint(db.conn(), std::slice::from_ref(&original), status);
            assert!(project_batch(
                db.conn(),
                Operation {
                    kind: OPERATION_KIND,
                    id: "batch"
                },
                &saved
            )
            .unwrap()
            .is_empty());
            assert_eq!(cached(db.conn(), &original.username), original);
            assert_eq!(
                submission::unapplied_operations(db.conn(), OPERATION_KIND, 10)
                    .unwrap()
                    .is_empty(),
                status == "failed_on_chain"
            );
        }
    }
}
