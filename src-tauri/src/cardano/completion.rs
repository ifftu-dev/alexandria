//! Completion-witness observer (post-migration 040 VC-first path).
//!
//! Watches Blockfrost for mints under the `completion_minting` policy
//! (compiled from `cardano/governance/validators/completion.ak`) and
//! turns each newly-observed mint into a [`CompletionObservation`] row
//! in the local DB. The auto-issuance pipeline (see
//! `commands::auto_issuance`) then signs a Verifiable Credential for
//! each pending row and stamps the VC's `witness` block with the mint
//! tx hash + validator script hash.
//!
//! ## Configuration
//!
//! The policy ID (= validator script hash) is read from the
//! `ALEXANDRIA_COMPLETION_POLICY_ID` environment variable. When unset
//! the observer is a no-op, matching the optional-Cardano posture of
//! `cardano::anchor_queue`.
//!
//! ## Datum parsing
//!
//! The output carrying the minted completion token has an inline
//! datum encoding [`crate::cardano::completion::OnChainCompletionDatum`].
//! The on-chain shape (from `lib/alexandria/completion.ak`):
//!
//! ```aiken
//! pub type CompletionDatum {
//!   subject_pubkey: ByteArray,
//!   course_id: ByteArray,
//!   completion_root: ByteArray,
//!   timestamp: Int,
//! }
//! ```
//!
//! Encoded as a Plutus `Constr 0 [ByteString, ByteString, ByteString, Int]`.
//! We parse that shape directly from the hex-encoded CBOR returned by
//! Blockfrost — no script-side validation is repeated here (the
//! validator already enforced the invariants on-chain).

use std::sync::{Arc, Mutex};

use pallas_codec::minicbor::Decoder;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::blockfrost::{BlockfrostClient, BlockfrostError};
use crate::db::Database;

/// Script hash of the `completion.ak` validator (aka the policy ID
/// of `completion_minting`). Stable across network environments as
/// long as the Aiken source doesn't change.
///
/// Wired in as a constant for test ergonomics; the runtime observer
/// reads the env var so operators can redeploy with a different
/// compiled hash without rebuilding the binary.
pub const WITNESS_VALIDATOR_NAME: &str = "completion_minting";

#[derive(Error, Debug)]
pub enum CompletionError {
    #[error("blockfrost: {0}")]
    Blockfrost(#[from] BlockfrostError),
    #[error("db: {0}")]
    Db(String),
    #[error("datum decode: {0}")]
    Datum(String),
    #[error("asset name malformed: expected {expected} bytes, got {got}")]
    AssetName { expected: usize, got: usize },
    #[error("missing output with completion token on tx {0}")]
    MissingWitnessOutput(String),
    #[error("completion observer is disabled (ALEXANDRIA_COMPLETION_POLICY_ID unset)")]
    Disabled,
}

/// A completion-mint observation: one row per `(policy_id, asset_name)`
/// that the app has ever seen on-chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionObservation {
    pub policy_id: String,
    pub asset_name_hex: String,
    pub tx_hash: String,
    /// Hex-encoded 32-byte Ed25519 pubkey from the datum.
    pub subject_pubkey: String,
    /// Hex-encoded course identifier (16 bytes by convention, but we
    /// don't enforce that off-chain — the validator doesn't either).
    pub course_id: String,
    /// Hex-encoded blake2b-256 merkle root.
    pub completion_root: String,
    /// ISO 8601 string derived from `datum.timestamp` (POSIX ms).
    pub completion_time: String,
    pub credential_id: Option<String>,
    pub observed_at: String,
    pub issued_at: Option<String>,
}

/// One tick of the observer loop.
///
/// Takes the shared `Database` handle so that await points never
/// straddle a held connection — rusqlite `Connection` is not `Send`.
/// Locks are taken briefly around existence checks and inserts.
///
/// Returns the number of *new* observations persisted. Already-seen
/// `(policy_id, asset_name)` pairs are silently skipped — idempotent
/// by design so the observer can run every minute without producing
/// duplicate VCs.
pub async fn tick(
    db: &Arc<Mutex<Option<Database>>>,
    bf: &BlockfrostClient,
    policy_id: &str,
) -> Result<usize, CompletionError> {
    let assets = bf.list_policy_assets(policy_id).await?;

    let mut new_rows = 0;
    for asset in assets {
        // Quantity "0" = burned in full; skip.
        if asset.quantity == "0" {
            continue;
        }
        let Some(asset_name_hex) = asset.asset.strip_prefix(policy_id) else {
            continue;
        };
        // Skip if already stored.
        let already = {
            let guard = db
                .lock()
                .map_err(|_| CompletionError::Db("poisoned".into()))?;
            match guard.as_ref() {
                Some(d) => observation_exists(d.conn(), policy_id, asset_name_hex)?,
                None => return Ok(new_rows),
            }
        };
        if already {
            continue;
        }

        match observe_one(bf, &asset.asset, policy_id, asset_name_hex).await {
            Ok(obs) => {
                let guard = db
                    .lock()
                    .map_err(|_| CompletionError::Db("poisoned".into()))?;
                if let Some(d) = guard.as_ref() {
                    insert_observation(d.conn(), &obs)?;
                    new_rows += 1;
                }
            }
            Err(e) => {
                log::warn!("completion::tick: skipping asset {} ({e})", asset.asset);
            }
        }
    }

    Ok(new_rows)
}

/// Fetch the mint tx and decode its inline datum into a full
/// observation record.
async fn observe_one(
    bf: &BlockfrostClient,
    asset_unit: &str,
    policy_id: &str,
    asset_name_hex: &str,
) -> Result<CompletionObservation, CompletionError> {
    let history = bf
        .get_asset_history(asset_unit)
        .await?
        .ok_or_else(|| CompletionError::Datum("no mint history for asset".into()))?;

    let utxos = bf.get_tx_utxos(&history.tx_hash).await?;

    // Find the output carrying this token; that's the one with the
    // inline CompletionDatum. The validator guarantees there is
    // exactly one mint per tx, but it does NOT constrain which output
    // the token lands on — only that SOME output carries it with the
    // correct datum.
    let asset_unit_owned = asset_unit.to_string();
    let output = utxos
        .outputs
        .into_iter()
        .find(|o| o.amount.iter().any(|a| a.unit == asset_unit_owned))
        .ok_or_else(|| CompletionError::MissingWitnessOutput(history.tx_hash.clone()))?;

    let datum_hex = output.inline_datum.ok_or_else(|| {
        CompletionError::Datum(format!(
            "output at index {} has no inline datum",
            output.output_index
        ))
    })?;

    let datum = decode_completion_datum(&datum_hex)?;

    Ok(CompletionObservation {
        policy_id: policy_id.to_string(),
        asset_name_hex: asset_name_hex.to_string(),
        tx_hash: history.tx_hash,
        subject_pubkey: hex::encode(&datum.subject_pubkey),
        course_id: hex::encode(&datum.course_id),
        completion_root: hex::encode(&datum.completion_root),
        completion_time: format_posix_ms(datum.timestamp),
        credential_id: None,
        observed_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        issued_at: None,
    })
}

/// Intermediate representation of the on-chain datum before we
/// hex-encode the byte fields.
#[derive(Debug)]
struct DecodedCompletionDatum {
    subject_pubkey: Vec<u8>,
    course_id: Vec<u8>,
    completion_root: Vec<u8>,
    timestamp: i64,
}

/// Decode `CompletionDatum` from hex-encoded Plutus-data CBOR.
///
/// Plutus encodes a record as `Constr 0 [...]`. Tag 121 in CBOR is
/// `Constr 0` (Plutus convention: tag = 121 + constructor_index for
/// indices < 7). The payload is an array of four items in declaration
/// order: `subject_pubkey`, `course_id`, `completion_root`,
/// `timestamp`.
fn decode_completion_datum(hex_str: &str) -> Result<DecodedCompletionDatum, CompletionError> {
    let bytes =
        hex::decode(hex_str).map_err(|e| CompletionError::Datum(format!("invalid hex: {e}")))?;
    let mut d = Decoder::new(&bytes);

    let tag = d
        .tag()
        .map_err(|e| CompletionError::Datum(format!("no cbor tag: {e}")))?;
    if u64::from(tag) != 121 {
        return Err(CompletionError::Datum(format!(
            "expected Constr 0 (tag 121), got tag {}",
            u64::from(tag)
        )));
    }

    let len = d
        .array()
        .map_err(|e| CompletionError::Datum(format!("not an array: {e}")))?;
    if len != Some(4) {
        return Err(CompletionError::Datum(format!(
            "expected 4 fields, got {len:?}"
        )));
    }

    let subject_pubkey = d
        .bytes()
        .map_err(|e| CompletionError::Datum(format!("subject_pubkey: {e}")))?
        .to_vec();
    let course_id = d
        .bytes()
        .map_err(|e| CompletionError::Datum(format!("course_id: {e}")))?
        .to_vec();
    let completion_root = d
        .bytes()
        .map_err(|e| CompletionError::Datum(format!("completion_root: {e}")))?
        .to_vec();
    let timestamp = d
        .i64()
        .map_err(|e| CompletionError::Datum(format!("timestamp: {e}")))?;

    Ok(DecodedCompletionDatum {
        subject_pubkey,
        course_id,
        completion_root,
        timestamp,
    })
}

fn format_posix_ms(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| format!("@ms={ms}"))
}

// ----- DB helpers -----

fn observation_exists(
    conn: &Connection,
    policy_id: &str,
    asset_name_hex: &str,
) -> Result<bool, CompletionError> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM completion_observations \
             WHERE policy_id = ?1 AND asset_name_hex = ?2",
            params![policy_id, asset_name_hex],
            |row| row.get(0),
        )
        .map_err(|e| CompletionError::Db(e.to_string()))?;
    Ok(count > 0)
}

fn insert_observation(
    conn: &Connection,
    obs: &CompletionObservation,
) -> Result<(), CompletionError> {
    conn.execute(
        "INSERT INTO completion_observations ( \
            policy_id, asset_name_hex, tx_hash, subject_pubkey, \
            course_id, completion_root, completion_time, credential_id, \
            observed_at, issued_at \
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, NULL)",
        params![
            obs.policy_id,
            obs.asset_name_hex,
            obs.tx_hash,
            obs.subject_pubkey,
            obs.course_id,
            obs.completion_root,
            obs.completion_time,
            obs.observed_at,
        ],
    )
    .map_err(|e| CompletionError::Db(e.to_string()))?;
    Ok(())
}

/// List observations that have not yet been resolved into a VC. The
/// auto-issuance pipeline consumes this list each tick.
pub fn pending_observations(
    conn: &Connection,
) -> Result<Vec<CompletionObservation>, CompletionError> {
    let mut stmt = conn
        .prepare(
            "SELECT policy_id, asset_name_hex, tx_hash, subject_pubkey, \
                course_id, completion_root, completion_time, credential_id, \
                observed_at, issued_at \
             FROM completion_observations \
             WHERE credential_id IS NULL \
             ORDER BY observed_at ASC",
        )
        .map_err(|e| CompletionError::Db(e.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(CompletionObservation {
                policy_id: row.get(0)?,
                asset_name_hex: row.get(1)?,
                tx_hash: row.get(2)?,
                subject_pubkey: row.get(3)?,
                course_id: row.get(4)?,
                completion_root: row.get(5)?,
                completion_time: row.get(6)?,
                credential_id: row.get(7)?,
                observed_at: row.get(8)?,
                issued_at: row.get(9)?,
            })
        })
        .map_err(|e| CompletionError::Db(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| CompletionError::Db(e.to_string()))?;
    Ok(rows)
}

/// Mark an observation as resolved by stamping the issued VC id.
/// Called by the auto-issuance pipeline once the VC has been signed,
/// anchored (optional), and written to `credentials`.
pub fn mark_issued(
    conn: &Connection,
    policy_id: &str,
    asset_name_hex: &str,
    credential_id: &str,
) -> Result<(), CompletionError> {
    conn.execute(
        "UPDATE completion_observations \
         SET credential_id = ?3, issued_at = datetime('now') \
         WHERE policy_id = ?1 AND asset_name_hex = ?2",
        params![policy_id, asset_name_hex, credential_id],
    )
    .map_err(|e| CompletionError::Db(e.to_string()))?;
    Ok(())
}

/// Look up the observation for a given credential, if any. Handy for
/// verifiers that want to cross-check a VC's `witness.tx_hash`.
pub fn find_by_credential(
    conn: &Connection,
    credential_id: &str,
) -> Result<Option<CompletionObservation>, CompletionError> {
    conn.query_row(
        "SELECT policy_id, asset_name_hex, tx_hash, subject_pubkey, \
            course_id, completion_root, completion_time, credential_id, \
            observed_at, issued_at \
         FROM completion_observations WHERE credential_id = ?1",
        params![credential_id],
        |row| {
            Ok(CompletionObservation {
                policy_id: row.get(0)?,
                asset_name_hex: row.get(1)?,
                tx_hash: row.get(2)?,
                subject_pubkey: row.get(3)?,
                course_id: row.get(4)?,
                completion_root: row.get(5)?,
                completion_time: row.get(6)?,
                credential_id: row.get(7)?,
                observed_at: row.get(8)?,
                issued_at: row.get(9)?,
            })
        },
    )
    .optional()
    .map_err(|e| CompletionError::Db(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use pallas_codec::minicbor::data::Tag as CborTag;
    use pallas_codec::minicbor::Encoder;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        db
    }

    fn encode_datum(
        subject_pubkey: &[u8],
        course_id: &[u8],
        completion_root: &[u8],
        timestamp: i64,
    ) -> String {
        let mut buf = Vec::new();
        let mut e = Encoder::new(&mut buf);
        e.tag(CborTag::new(121)).unwrap();
        e.array(4).unwrap();
        e.bytes(subject_pubkey).unwrap();
        e.bytes(course_id).unwrap();
        e.bytes(completion_root).unwrap();
        e.i64(timestamp).unwrap();
        hex::encode(&buf)
    }

    #[test]
    fn decode_roundtrips_datum_fields() {
        let pk = [0x11u8; 32];
        let cid = [0x22u8; 16];
        let root = [0x33u8; 32];
        let ts = 1_714_000_000_000i64;

        let hex_str = encode_datum(&pk, &cid, &root, ts);
        let decoded = decode_completion_datum(&hex_str).unwrap();

        assert_eq!(decoded.subject_pubkey, pk);
        assert_eq!(decoded.course_id, cid);
        assert_eq!(decoded.completion_root, root);
        assert_eq!(decoded.timestamp, ts);
    }

    #[test]
    fn decode_rejects_wrong_constr_tag() {
        let mut buf = Vec::new();
        let mut e = Encoder::new(&mut buf);
        e.tag(CborTag::new(122)).unwrap(); // Constr 1, not 0
        e.array(4).unwrap();
        e.bytes(&[]).unwrap();
        e.bytes(&[]).unwrap();
        e.bytes(&[]).unwrap();
        e.i64(0).unwrap();
        let hex_str = hex::encode(&buf);

        let err = decode_completion_datum(&hex_str).unwrap_err();
        assert!(matches!(err, CompletionError::Datum(_)));
    }

    #[test]
    fn observation_crud_roundtrip() {
        let db = test_db();
        let obs = CompletionObservation {
            policy_id: "6380450179a6933acdf76213732f8626e1486b9ed5cc7fe7f46c98e0".into(),
            asset_name_hex: "aa".repeat(16),
            tx_hash: "b0".repeat(32),
            subject_pubkey: "11".repeat(32),
            course_id: "22".repeat(16),
            completion_root: "33".repeat(32),
            completion_time: "2026-04-24T12:00:00Z".into(),
            credential_id: None,
            observed_at: "2026-04-24 12:00:00".into(),
            issued_at: None,
        };

        insert_observation(db.conn(), &obs).expect("insert");
        assert!(observation_exists(db.conn(), &obs.policy_id, &obs.asset_name_hex).unwrap());

        let pending = pending_observations(db.conn()).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].tx_hash, obs.tx_hash);
        assert!(pending[0].credential_id.is_none());

        mark_issued(
            db.conn(),
            &obs.policy_id,
            &obs.asset_name_hex,
            "urn:uuid:test-cred",
        )
        .expect("mark");

        let still_pending = pending_observations(db.conn()).unwrap();
        assert!(still_pending.is_empty());

        let found = find_by_credential(db.conn(), "urn:uuid:test-cred")
            .unwrap()
            .expect("observation");
        assert_eq!(found.tx_hash, obs.tx_hash);
        assert!(found.issued_at.is_some());
    }

    #[test]
    fn format_posix_ms_roundtrip_zero() {
        assert_eq!(format_posix_ms(0), "1970-01-01T00:00:00Z");
    }
}
