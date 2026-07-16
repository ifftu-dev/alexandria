//! Stake-address → Cardano-payment-key registry (per `docs/stake-pubkey-registry.md`).
//!
//! Persistent replacement for the TOFU identity binding that
//! previously lived in `MessageValidator`. Every privileged-topic
//! gossip message (taxonomy, governance, Sentinel priors, plugin
//! attestations, goal templates, question banks) is authorized by
//! checking that the message's
//! `(stake_address, public_key)` pair appears in this registry within
//! its validity window.
//!
//! Two sources feed the registry:
//!
//! - **`bootstrap_registry.json`** — multisig-signed (2-of-3 founder
//!   keys) snapshot shipped with the release. Loaded on first boot.
//! - **On-chain `StakePubkeyRegistration` txs** — pulled in the
//!   background via Blockfrost. Chain entries authoritative on
//!   conflict.
//!
//! All lookups are stateless (`fn(conn, …)`). The validator pulls a
//! `Database` handle from `AppState` and forwards it into
//! [`lookup`] for each privileged-topic message.
//!
//! Founder verifier keys are hardcoded constants ([`SNAPSHOT_VERIFIERS`])
//! to close key-substitution attacks; rotating them requires a code
//! change + release.

use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::types::{
    SignedGossipMessage, TOPIC_GOAL_TEMPLATES, TOPIC_GOVERNANCE, TOPIC_PLUGIN_ATTESTATIONS,
    TOPIC_QUESTION_BANKS, TOPIC_SENTINEL_PRIORS, TOPIC_TAXONOMY,
};

/// Snapshot format version. Bump if the canonical-bytes layout changes
/// so old releases reject incompatible snapshots instead of silently
/// misreading them.
pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;

/// Required signatures from [`SNAPSHOT_VERIFIERS`] for a bootstrap
/// snapshot to be accepted. 2-of-3 multisig.
pub const SNAPSHOT_QUORUM: usize = 2;

/// Hardcoded Ed25519 verifier keys for the bootstrap snapshot signature.
///
/// **Solo-founder origin.** Generated 2026-05-25 via the
/// `snapshot_keygen` example on a single dev machine; all three secrets
/// currently live in `~/.alexandria-founder-keys/`. Until additional
/// cofounders rotate their own keys in, the practical trust threshold
/// on these signatures is single-key (one operator holds every secret),
/// even though the verifier still enforces the 2-of-3 quorum at the
/// code level. Rotate by replacing one or more entries below and
/// re-running the multisig signing ceremony.
///
/// Each entry is `(name, hex-encoded 32-byte Ed25519 public key)`. The
/// names are advisory only; signature verification ignores them and
/// just collects a set of valid keys.
pub const SNAPSHOT_VERIFIERS: &[(&str, &str)] = &[
    (
        "founder_a",
        "53483cedf2f537accf9f7bfaa17aab81b0c80767664d52129a070db0e9660312",
    ),
    (
        "founder_b",
        "6f6aaae8df089ae21120fc4eb96a362bc9bf0cc8e34639daaae6d7e24abdf19c",
    ),
    (
        "founder_c",
        "c5a88da0cbdfbfb57065549c347b6f7daf8ad60ed2b36258570fccd2e7ec6186",
    ),
];

/// Source of a registry row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntrySource {
    Chain,
    Snapshot,
}

impl EntrySource {
    /// SQL `source` column value. Stable string identifier; do not
    /// change without bumping the schema CHECK constraint in
    /// migration 052.
    pub fn as_str(self) -> &'static str {
        match self {
            EntrySource::Chain => "chain",
            EntrySource::Snapshot => "snapshot",
        }
    }
}

/// One `(stake_address, public_key, window)` binding in the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    pub stake_address: String,
    pub public_key_hex: String,
    pub valid_from: u64,
    pub valid_until: Option<u64>,
    pub source: EntrySource,
    pub on_chain_tx: Option<String>,
    pub snapshot_sig: Option<String>,
}

// ---------------------------------------------------------------------------
// Bootstrap snapshot
// ---------------------------------------------------------------------------

/// One entry inside `bootstrap_registry.json`. Maps 1:1 to a
/// [`RegistryEntry`] with `source = Snapshot`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub stake_address: String,
    pub public_key_hex: String,
    pub valid_from: u64,
    #[serde(default)]
    pub valid_until: Option<u64>,
    #[serde(default)]
    pub on_chain_tx: Option<String>,
}

/// A signature attached to a snapshot. `signer` is purely advisory —
/// verification iterates [`SNAPSHOT_VERIFIERS`] and counts matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSignature {
    pub signer: String,
    pub sig_hex: String,
}

/// The signed bootstrap snapshot envelope.
///
/// Verification re-canonicalizes `(version, issued_at, entries)` to
/// JCS bytes and accepts the snapshot iff ≥ [`SNAPSHOT_QUORUM`] of the
/// attached signatures verify under any distinct key in
/// [`SNAPSHOT_VERIFIERS`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapSnapshot {
    pub version: u32,
    pub issued_at: String,
    pub entries: Vec<SnapshotEntry>,
    pub signatures: Vec<SnapshotSignature>,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("snapshot parse failed: {0}")]
    SnapshotParse(String),
    #[error("snapshot version {0} unsupported (expected {SNAPSHOT_FORMAT_VERSION})")]
    SnapshotVersion(u32),
    #[error("snapshot quorum not met: {got} valid signatures, need {needed}")]
    SnapshotQuorum { got: usize, needed: usize },
    #[error("snapshot canonicalize failed: {0}")]
    SnapshotCanonicalize(String),
    #[error("invalid hex: {0}")]
    Hex(String),
    #[error("invalid verifier key in SNAPSHOT_VERIFIERS: {0}")]
    VerifierKey(String),
    #[error("db error: {0}")]
    Db(#[from] rusqlite::Error),
}

impl BootstrapSnapshot {
    /// Parse + verify a snapshot from raw JSON bytes. On success returns
    /// the snapshot stripped of its signatures (already verified).
    pub fn parse_and_verify(bytes: &[u8]) -> Result<Self, RegistryError> {
        let snap: BootstrapSnapshot = serde_json::from_slice(bytes)
            .map_err(|e| RegistryError::SnapshotParse(e.to_string()))?;
        if snap.version != SNAPSHOT_FORMAT_VERSION {
            return Err(RegistryError::SnapshotVersion(snap.version));
        }
        snap.verify_signatures()?;
        Ok(snap)
    }

    /// Run the multisig verification against [`SNAPSHOT_VERIFIERS`].
    pub fn verify_signatures(&self) -> Result<(), RegistryError> {
        // Resolve the hardcoded verifier keys once.
        let mut verifiers: Vec<VerifyingKey> = Vec::with_capacity(SNAPSHOT_VERIFIERS.len());
        for (name, hex_pk) in SNAPSHOT_VERIFIERS {
            let pk_bytes = hex::decode(hex_pk)
                .map_err(|e| RegistryError::VerifierKey(format!("{name}: {e}")))?;
            let pk_arr: [u8; 32] = pk_bytes
                .try_into()
                .map_err(|_| RegistryError::VerifierKey(format!("{name}: wrong length")))?;
            verifiers.push(
                VerifyingKey::from_bytes(&pk_arr)
                    .map_err(|e| RegistryError::VerifierKey(format!("{name}: {e}")))?,
            );
        }

        let signed_bytes = self.canonical_signed_bytes()?;

        // Count valid signatures, but never let a single verifier key
        // cover two slots (e.g. signer "founder_a" pasted twice).
        let mut used = vec![false; verifiers.len()];
        let mut valid = 0;
        for sig in &self.signatures {
            let sig_bytes = hex::decode(&sig.sig_hex)
                .map_err(|e| RegistryError::Hex(format!("signature {}: {e}", sig.signer)))?;
            let sig_arr: [u8; 64] = match sig_bytes.try_into() {
                Ok(a) => a,
                Err(_) => continue, // wrong length — skip, don't error
            };
            let parsed = Signature::from_bytes(&sig_arr);
            for (i, vk) in verifiers.iter().enumerate() {
                if used[i] {
                    continue;
                }
                if vk.verify(&signed_bytes, &parsed).is_ok() {
                    used[i] = true;
                    valid += 1;
                    break;
                }
            }
            if valid >= SNAPSHOT_QUORUM {
                break;
            }
        }

        if valid < SNAPSHOT_QUORUM {
            return Err(RegistryError::SnapshotQuorum {
                got: valid,
                needed: SNAPSHOT_QUORUM,
            });
        }
        Ok(())
    }

    /// JCS-canonical bytes of the signed portion (`version + issued_at +
    /// entries`). Re-derived on both sign and verify sides so issuer
    /// and verifier agree bit-for-bit.
    pub fn canonical_signed_bytes(&self) -> Result<Vec<u8>, RegistryError> {
        let v = serde_json::json!({
            "version": self.version,
            "issued_at": self.issued_at,
            "entries": self.entries,
        });
        crate::domain::vc::canonicalize::canonicalize(&v)
            .map_err(|e| RegistryError::SnapshotCanonicalize(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Lookup
// ---------------------------------------------------------------------------

/// Return `Ok(true)` iff the registry contains a row binding
/// `stake_address` to `public_key_hex` within a window covering `now`.
pub fn lookup(
    conn: &Connection,
    stake_address: &str,
    public_key_hex: &str,
    now: u64,
) -> Result<bool, rusqlite::Error> {
    let now_i64 = now as i64;
    conn.query_row(
        "SELECT 1 FROM stake_pubkey_registry \
         WHERE stake_address = ?1 \
           AND public_key_hex = ?2 \
           AND valid_from <= ?3 \
           AND (valid_until IS NULL OR valid_until > ?3) \
         LIMIT 1",
        params![stake_address, public_key_hex.to_lowercase(), now_i64],
        |_| Ok(()),
    )
    .optional()
    .map(|opt| opt.is_some())
}

/// Insert a snapshot entry. Idempotent — the PRIMARY KEY
/// `(stake_address, public_key_hex, valid_from)` collapses duplicates.
pub fn upsert_snapshot_entry(
    conn: &Connection,
    entry: &SnapshotEntry,
    snapshot_sig: Option<&str>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO stake_pubkey_registry \
         (stake_address, public_key_hex, valid_from, valid_until, source, on_chain_tx, snapshot_sig, last_verified) \
         VALUES (?1, ?2, ?3, ?4, 'snapshot', ?5, ?6, 0) \
         ON CONFLICT(stake_address, public_key_hex, valid_from) DO NOTHING",
        params![
            entry.stake_address,
            entry.public_key_hex.to_lowercase(),
            entry.valid_from as i64,
            entry.valid_until.map(|v| v as i64),
            entry.on_chain_tx,
            snapshot_sig,
        ],
    )?;
    Ok(())
}

/// Upsert a chain-confirmed entry. If a row with matching
/// `(stake_address, public_key_hex, valid_from)` already exists,
/// upgrade its `source` to `'chain'`, fill in `on_chain_tx`, and bump
/// `last_verified`. On a *conflicting* binding (same stake address +
/// window, different pubkey) the new chain row inserts alongside the
/// snapshot row and the snapshot row is evicted by the caller (see
/// `evict_contradicted_snapshot`).
pub fn upsert_chain_entry(
    conn: &Connection,
    stake_address: &str,
    public_key_hex: &str,
    valid_from: u64,
    valid_until: Option<u64>,
    on_chain_tx: &str,
    now: u64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO stake_pubkey_registry \
         (stake_address, public_key_hex, valid_from, valid_until, source, on_chain_tx, snapshot_sig, last_verified) \
         VALUES (?1, ?2, ?3, ?4, 'chain', ?5, NULL, ?6) \
         ON CONFLICT(stake_address, public_key_hex, valid_from) DO UPDATE SET \
            source = 'chain', \
            on_chain_tx = excluded.on_chain_tx, \
            last_verified = excluded.last_verified, \
            valid_until = excluded.valid_until",
        params![
            stake_address,
            public_key_hex.to_lowercase(),
            valid_from as i64,
            valid_until.map(|v| v as i64),
            on_chain_tx,
            now as i64,
        ],
    )?;
    Ok(())
}

/// Evict any snapshot row that binds `stake_address` within a window
/// overlapping `(valid_from, valid_until)` but to a *different* pubkey
/// than the chain-confirmed one. Called after a successful
/// [`upsert_chain_entry`].
pub fn evict_contradicted_snapshot(
    conn: &Connection,
    stake_address: &str,
    chain_pubkey_hex: &str,
    valid_from: u64,
    valid_until: Option<u64>,
) -> Result<usize, rusqlite::Error> {
    let chain_until = valid_until.map(|v| v as i64).unwrap_or(i64::MAX);
    conn.execute(
        "DELETE FROM stake_pubkey_registry \
         WHERE source = 'snapshot' \
           AND stake_address = ?1 \
           AND public_key_hex != ?2 \
           AND valid_from < ?4 \
           AND (valid_until IS NULL OR valid_until > ?3)",
        params![
            stake_address,
            chain_pubkey_hex.to_lowercase(),
            valid_from as i64,
            chain_until,
        ],
    )
}

/// Apply a parsed + verified [`BootstrapSnapshot`] to the registry,
/// inserting every entry with `source = 'snapshot'`. Idempotent.
///
/// Returns the number of new rows inserted (matches `INSERT … ON
/// CONFLICT DO NOTHING` semantics — collisions count as zero).
pub fn apply_bootstrap(
    conn: &Connection,
    snap: &BootstrapSnapshot,
) -> Result<usize, RegistryError> {
    // We can't read SQLite's "rows affected" reliably for ON CONFLICT
    // DO NOTHING — count by row diff instead.
    let before: i64 = conn.query_row("SELECT COUNT(*) FROM stake_pubkey_registry", [], |r| {
        r.get(0)
    })?;
    for entry in &snap.entries {
        upsert_snapshot_entry(conn, entry, None)?;
    }
    let after: i64 = conn.query_row("SELECT COUNT(*) FROM stake_pubkey_registry", [], |r| {
        r.get(0)
    })?;
    Ok((after - before).max(0) as usize)
}

/// `bootstrap_registry.json` bundled at build time. Loaded into every
/// profile DB by [`load_embedded_bootstrap`] when no DB rows exist yet.
/// Replace `src-tauri/resources/bootstrap_registry.json` to ship a new
/// committee set; the change requires a rebuild.
pub const EMBEDDED_BOOTSTRAP_JSON: &[u8] =
    include_bytes!("../../resources/bootstrap_registry.json");

/// Seed the registry from the bundled [`EMBEDDED_BOOTSTRAP_JSON`].
/// Idempotent — relies on the table's PRIMARY KEY to drop duplicates.
/// Empty / placeholder snapshots short-circuit silently.
pub fn load_embedded_bootstrap(conn: &Connection) -> Result<usize, RegistryError> {
    // Mirror the placeholder short-circuit in `load_bootstrap_if_present`.
    if let Ok(peek) = serde_json::from_slice::<BootstrapSnapshot>(EMBEDDED_BOOTSTRAP_JSON) {
        if peek.entries.is_empty() && peek.signatures.is_empty() {
            return Ok(0);
        }
    }
    match BootstrapSnapshot::parse_and_verify(EMBEDDED_BOOTSTRAP_JSON) {
        Ok(snap) => apply_bootstrap(conn, &snap),
        Err(RegistryError::VerifierKey(reason)) => {
            log::warn!(
                "embedded bootstrap skipped: verifier keys not configured ({reason}). \
                 Replace SNAPSHOT_VERIFIERS before public launch."
            );
            Ok(0)
        }
        Err(e) => Err(e),
    }
}

/// Load `bootstrap_registry.json` from `path` if present, verify its
/// signatures, and apply it to the registry. Missing file / parse
/// failure / placeholder verifier keys are logged at INFO and treated
/// as a no-op — privileged-topic gossip will simply have no bindings
/// until the on-chain refresh path supplies them.
///
/// This is the only side effect of the snapshot path on a fresh boot.
pub fn load_bootstrap_if_present(
    conn: &Connection,
    path: &std::path::Path,
) -> Result<usize, RegistryError> {
    if !path.exists() {
        log::info!(
            "bootstrap_registry.json not present at {} — skipping snapshot seed",
            path.display()
        );
        return Ok(0);
    }
    let bytes = std::fs::read(path).map_err(|e| RegistryError::SnapshotParse(e.to_string()))?;

    // Pre-launch placeholder: the shipped file may have no entries and
    // no signatures while founders are still finalizing keys. Detect
    // that shape up front and short-circuit before verification so we
    // don't trip the quorum check on an obviously-empty file.
    if let Ok(peek) = serde_json::from_slice::<BootstrapSnapshot>(&bytes) {
        if peek.entries.is_empty() && peek.signatures.is_empty() {
            log::info!("bootstrap_registry.json is the empty placeholder — no entries to seed yet");
            return Ok(0);
        }
    }

    match BootstrapSnapshot::parse_and_verify(&bytes) {
        Ok(snap) => {
            let inserted = apply_bootstrap(conn, &snap)?;
            log::info!(
                "applied bootstrap_registry.json: {} new rows from {} entries",
                inserted,
                snap.entries.len()
            );
            Ok(inserted)
        }
        Err(RegistryError::VerifierKey(reason)) => {
            // Pre-launch / dev: SNAPSHOT_VERIFIERS still holds
            // placeholder all-zero keys. Don't crash startup; just
            // refuse to seed and let the on-chain path take over once
            // it's wired.
            log::warn!(
                "bootstrap snapshot skipped: verifier keys not configured ({reason}). \
                 Replace SNAPSHOT_VERIFIERS before public launch."
            );
            Ok(0)
        }
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// Privileged topic gate
// ---------------------------------------------------------------------------

/// Topics whose authority hinges on `(stake_address, public_key)` being
/// in the registry. Non-privileged topics skip the check entirely so
/// arbitrary peers (profiles, opinions, catalog, …) can still gossip.
pub fn is_privileged_topic(topic: &str) -> bool {
    matches!(
        topic,
        TOPIC_TAXONOMY
            | TOPIC_GOVERNANCE
            | TOPIC_SENTINEL_PRIORS
            | TOPIC_PLUGIN_ATTESTATIONS
            | TOPIC_GOAL_TEMPLATES
            | TOPIC_QUESTION_BANKS
    )
}

/// Enforce the registry check for `message`. Returns `Ok(())` if the
/// topic is non-privileged or the binding is present; `Err(())` if a
/// privileged-topic binding is missing.
pub fn check_message(conn: &Connection, message: &SignedGossipMessage) -> Result<(), String> {
    if !is_privileged_topic(&message.topic) {
        return Ok(());
    }
    let pubkey_hex = hex::encode(&message.public_key);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("clock: {e}"))?
        .as_secs();
    let allowed = lookup(conn, &message.stake_address, &pubkey_hex, now)
        .map_err(|e| format!("registry lookup: {e}"))?;
    if !allowed {
        return Err(format!(
            "stake address '{}' has no registry binding for the signing pubkey at ts {now}",
            message.stake_address
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use ed25519_dalek::{Signer, SigningKey};

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn snapshot_with(entries: Vec<SnapshotEntry>) -> BootstrapSnapshot {
        BootstrapSnapshot {
            version: SNAPSHOT_FORMAT_VERSION,
            issued_at: "2026-05-25T00:00:00Z".into(),
            entries,
            signatures: vec![],
        }
    }

    fn sample_entry(addr: &str, pubkey: &str, from: u64, until: Option<u64>) -> SnapshotEntry {
        SnapshotEntry {
            stake_address: addr.into(),
            public_key_hex: pubkey.into(),
            valid_from: from,
            valid_until: until,
            on_chain_tx: None,
        }
    }

    // -- Lookup -----------------------------------------------------------

    #[test]
    fn lookup_returns_false_for_empty_table() {
        let db = test_db();
        let got = lookup(db.conn(), "stake1u_test", "deadbeef", 100).unwrap();
        assert!(!got);
    }

    #[test]
    fn lookup_inside_window_succeeds() {
        let db = test_db();
        let entry = sample_entry("stake1u_alice", "abcd", 100, Some(200));
        upsert_snapshot_entry(db.conn(), &entry, None).unwrap();
        assert!(lookup(db.conn(), "stake1u_alice", "abcd", 150).unwrap());
        assert!(lookup(db.conn(), "stake1u_alice", "abcd", 100).unwrap());
    }

    #[test]
    fn lookup_at_valid_until_is_exclusive() {
        let db = test_db();
        let entry = sample_entry("stake1u_alice", "abcd", 100, Some(200));
        upsert_snapshot_entry(db.conn(), &entry, None).unwrap();
        // valid_until is exclusive — exactly 200 is out.
        assert!(!lookup(db.conn(), "stake1u_alice", "abcd", 200).unwrap());
        assert!(!lookup(db.conn(), "stake1u_alice", "abcd", 201).unwrap());
    }

    #[test]
    fn lookup_before_valid_from_fails() {
        let db = test_db();
        let entry = sample_entry("stake1u_alice", "abcd", 100, Some(200));
        upsert_snapshot_entry(db.conn(), &entry, None).unwrap();
        assert!(!lookup(db.conn(), "stake1u_alice", "abcd", 99).unwrap());
    }

    #[test]
    fn lookup_open_ended_window_never_expires() {
        let db = test_db();
        let entry = sample_entry("stake1u_alice", "abcd", 100, None);
        upsert_snapshot_entry(db.conn(), &entry, None).unwrap();
        assert!(lookup(db.conn(), "stake1u_alice", "abcd", u64::MAX / 2).unwrap());
    }

    #[test]
    fn lookup_is_case_insensitive_on_pubkey() {
        let db = test_db();
        let entry = sample_entry("stake1u_alice", "AB12cd", 100, None);
        upsert_snapshot_entry(db.conn(), &entry, None).unwrap();
        // Stored lowercased; lookup also lowercases.
        assert!(lookup(db.conn(), "stake1u_alice", "ab12CD", 150).unwrap());
    }

    // -- Chain vs snapshot conflict resolution ----------------------------

    #[test]
    fn chain_upsert_upgrades_matching_snapshot_row() {
        let db = test_db();
        let entry = sample_entry("stake1u_alice", "abcd", 100, Some(200));
        upsert_snapshot_entry(db.conn(), &entry, Some("snap-sig")).unwrap();
        upsert_chain_entry(
            db.conn(),
            "stake1u_alice",
            "abcd",
            100,
            Some(200),
            "tx1",
            500,
        )
        .unwrap();

        let (source, on_chain_tx, last_verified): (String, Option<String>, i64) = db
            .conn()
            .query_row(
                "SELECT source, on_chain_tx, last_verified FROM stake_pubkey_registry \
                 WHERE stake_address = 'stake1u_alice'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(source, "chain");
        assert_eq!(on_chain_tx.as_deref(), Some("tx1"));
        assert_eq!(last_verified, 500);
    }

    #[test]
    fn contradicting_chain_evicts_snapshot_row() {
        let db = test_db();
        // Snapshot says alice's key is `aaaa`.
        upsert_snapshot_entry(
            db.conn(),
            &sample_entry("stake1u_alice", "aaaa", 100, Some(200)),
            None,
        )
        .unwrap();
        // Chain says it's actually `bbbb` for the same window.
        upsert_chain_entry(
            db.conn(),
            "stake1u_alice",
            "bbbb",
            100,
            Some(200),
            "tx-real",
            500,
        )
        .unwrap();
        let evicted =
            evict_contradicted_snapshot(db.conn(), "stake1u_alice", "bbbb", 100, Some(200))
                .unwrap();
        assert_eq!(evicted, 1);
        // The bad snapshot row is gone; the chain row remains.
        assert!(!lookup(db.conn(), "stake1u_alice", "aaaa", 150).unwrap());
        assert!(lookup(db.conn(), "stake1u_alice", "bbbb", 150).unwrap());
    }

    // -- Snapshot multisig verification -----------------------------------

    fn sign_snapshot(snap: &mut BootstrapSnapshot, keys: &[(usize, &SigningKey)]) {
        let bytes = snap.canonical_signed_bytes().unwrap();
        snap.signatures = keys
            .iter()
            .map(|(idx, sk)| {
                let sig = sk.sign(&bytes);
                SnapshotSignature {
                    signer: SNAPSHOT_VERIFIERS[*idx].0.to_string(),
                    sig_hex: hex::encode(sig.to_bytes()),
                }
            })
            .collect();
    }

    /// Replace `SNAPSHOT_VERIFIERS` in-test by stuffing the snapshot's
    /// known-good keys into a fresh set of `SigningKey`s; verify against
    /// the actual constants by constructing the verifier keys from the
    /// signing keys' public halves.
    ///
    /// Because `SNAPSHOT_VERIFIERS` is `const`, we can't swap it at
    /// runtime — instead the verify test path is exercised by a local
    /// helper [`verify_with_keys`] that mirrors [`BootstrapSnapshot::verify_signatures`]
    /// but accepts the verifier set as an argument.
    fn verify_with_keys(
        snap: &BootstrapSnapshot,
        verifiers: &[VerifyingKey],
        quorum: usize,
    ) -> bool {
        let bytes = snap.canonical_signed_bytes().unwrap();
        let mut used = vec![false; verifiers.len()];
        let mut valid = 0;
        for sig in &snap.signatures {
            let sig_bytes = match hex::decode(&sig.sig_hex) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let sig_arr: [u8; 64] = match sig_bytes.try_into() {
                Ok(a) => a,
                Err(_) => continue,
            };
            let parsed = Signature::from_bytes(&sig_arr);
            for (i, vk) in verifiers.iter().enumerate() {
                if used[i] {
                    continue;
                }
                if vk.verify(&bytes, &parsed).is_ok() {
                    used[i] = true;
                    valid += 1;
                    break;
                }
            }
        }
        valid >= quorum
    }

    fn three_keys() -> [SigningKey; 3] {
        // Deterministic test keys — never re-used in production.
        let seeds: [[u8; 32]; 3] = [[1; 32], [2; 32], [3; 32]];
        seeds.map(|s| SigningKey::from_bytes(&s))
    }

    #[test]
    fn snapshot_zero_signatures_fails_quorum() {
        let snap = snapshot_with(vec![sample_entry("a", "ab", 0, None)]);
        let keys = three_keys();
        let verifiers: Vec<VerifyingKey> = keys.iter().map(|k| k.verifying_key()).collect();
        assert!(!verify_with_keys(&snap, &verifiers, SNAPSHOT_QUORUM));
    }

    #[test]
    fn snapshot_one_signature_fails_quorum() {
        let mut snap = snapshot_with(vec![sample_entry("a", "ab", 0, None)]);
        let keys = three_keys();
        sign_snapshot(&mut snap, &[(0, &keys[0])]);
        let verifiers: Vec<VerifyingKey> = keys.iter().map(|k| k.verifying_key()).collect();
        assert!(!verify_with_keys(&snap, &verifiers, SNAPSHOT_QUORUM));
    }

    #[test]
    fn snapshot_two_signatures_meet_quorum() {
        let mut snap = snapshot_with(vec![sample_entry("a", "ab", 0, None)]);
        let keys = three_keys();
        sign_snapshot(&mut snap, &[(0, &keys[0]), (1, &keys[1])]);
        let verifiers: Vec<VerifyingKey> = keys.iter().map(|k| k.verifying_key()).collect();
        assert!(verify_with_keys(&snap, &verifiers, SNAPSHOT_QUORUM));
    }

    #[test]
    fn snapshot_three_signatures_meet_quorum() {
        let mut snap = snapshot_with(vec![sample_entry("a", "ab", 0, None)]);
        let keys = three_keys();
        sign_snapshot(&mut snap, &[(0, &keys[0]), (1, &keys[1]), (2, &keys[2])]);
        let verifiers: Vec<VerifyingKey> = keys.iter().map(|k| k.verifying_key()).collect();
        assert!(verify_with_keys(&snap, &verifiers, SNAPSHOT_QUORUM));
    }

    #[test]
    fn snapshot_same_key_twice_counts_as_one() {
        let mut snap = snapshot_with(vec![sample_entry("a", "ab", 0, None)]);
        let keys = three_keys();
        // Sign with founder_a twice — must NOT pass 2-of-3.
        sign_snapshot(&mut snap, &[(0, &keys[0]), (0, &keys[0])]);
        let verifiers: Vec<VerifyingKey> = keys.iter().map(|k| k.verifying_key()).collect();
        assert!(!verify_with_keys(&snap, &verifiers, SNAPSHOT_QUORUM));
    }

    #[test]
    fn snapshot_forged_signature_does_not_count() {
        let mut snap = snapshot_with(vec![sample_entry("a", "ab", 0, None)]);
        let keys = three_keys();
        // One real signature, one all-zeros sig hex from "founder_b".
        sign_snapshot(&mut snap, &[(0, &keys[0])]);
        snap.signatures.push(SnapshotSignature {
            signer: "founder_b".into(),
            sig_hex: "00".repeat(64),
        });
        let verifiers: Vec<VerifyingKey> = keys.iter().map(|k| k.verifying_key()).collect();
        assert!(!verify_with_keys(&snap, &verifiers, SNAPSHOT_QUORUM));
    }

    #[test]
    fn snapshot_tampered_entries_invalidate_signature() {
        let mut snap = snapshot_with(vec![sample_entry("a", "ab", 0, None)]);
        let keys = three_keys();
        sign_snapshot(&mut snap, &[(0, &keys[0]), (1, &keys[1])]);
        // Mutate the entries AFTER signing — sigs must no longer verify.
        snap.entries[0].public_key_hex = "ff".into();
        let verifiers: Vec<VerifyingKey> = keys.iter().map(|k| k.verifying_key()).collect();
        assert!(!verify_with_keys(&snap, &verifiers, SNAPSHOT_QUORUM));
    }

    // -- Privileged topic gate -------------------------------------------

    #[test]
    fn privileged_topics_recognized() {
        assert!(is_privileged_topic(TOPIC_TAXONOMY));
        assert!(is_privileged_topic(TOPIC_GOVERNANCE));
        assert!(is_privileged_topic(TOPIC_SENTINEL_PRIORS));
        assert!(is_privileged_topic(TOPIC_PLUGIN_ATTESTATIONS));
    }

    #[test]
    fn non_privileged_topics_pass_through() {
        use crate::p2p::types::{TOPIC_CATALOG, TOPIC_OPINIONS, TOPIC_PROFILES};
        assert!(!is_privileged_topic(TOPIC_CATALOG));
        assert!(!is_privileged_topic(TOPIC_PROFILES));
        assert!(!is_privileged_topic(TOPIC_OPINIONS));
    }
}
