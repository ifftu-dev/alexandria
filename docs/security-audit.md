# Alexandria Mark 3 -- Security Audit

**Date**: 2026-02-24
**Scope**: Full Rust backend (`src-tauri/src/`), Tauri configuration, Cargo dependencies
**Files audited**: Every file in `crypto/`, `p2p/`, `commands/`, `db/`, `cardano/`, `evidence/`, `ipfs/`, plus `lib.rs`, `tauri.conf.json`, `capabilities/default.json`, both `Cargo.toml` files

**Summary**: 1 critical, 4 high, 8 medium, 6 low, 5 informational findings.

---

## CRITICAL

### C-1: HMAC-SHA512 used as password KDF instead of memory-hard function

**File**: `src-tauri/src/crypto/keystore.rs:269-278`

The vault encryption key is derived from the user's password via a single pass of HMAC-SHA512:

```rust
fn derive_key(password: &str, salt: &[u8]) -> Result<KeyProvider, KeystoreError> {
    let mut mac = HmacSha512::new_from_slice(salt)
        .map_err(|e| KeystoreError::Memory(format!("HMAC init failed: {e}")))?;
    mac.update(password.as_bytes());
    let result = mac.finalize().into_bytes();
    let key_bytes = Zeroizing::new(result[..32].to_vec());
    KeyProvider::try_from(key_bytes).map_err(|e| KeystoreError::Memory(format!("{e:?}")))
}
```

HMAC-SHA512 is a fast hash. An attacker with the `.stronghold` snapshot file and `vault_salt.bin` can brute-force passwords at billions of attempts per second on commodity GPUs. The code comment at line 268 acknowledges this: `"Future: upgrade to argon2id for memory-hard KDF (brute-force resistance)"`.

**Impact**: If the snapshot file is exfiltrated (malware, stolen backup, unencrypted cloud sync), the entire BIP-39 mnemonic (and thus all Cardano funds + identity) can be recovered by brute-forcing the password offline. Even a strong password offers inadequate protection against GPU-accelerated HMAC-SHA512 attacks.

**Fix**: Replace `derive_key` with Argon2id (`argon2` crate). Recommended parameters: `m_cost = 65536` (64 MB), `t_cost = 3`, `p_cost = 4`. The salt and output size can remain unchanged.

---

## HIGH

### H-1: Gossip message timestamp not included in signed payload

**File**: `src-tauri/src/p2p/signing.rs:24-45`

`sign_gossip_message` signs only the raw `payload` bytes (line 30). The `timestamp`, `topic`, and `stake_address` fields of the `SignedGossipMessage` envelope are NOT included in the signed data:

```rust
let signed = core_signing::sign(&payload, signing_key); // Signs ONLY payload
let timestamp = SystemTime::now()...;
SignedGossipMessage {
    topic: topic.to_string(),
    payload,
    signature: signed.signature,  // Signature covers payload only
    public_key: signed.public_key,
    stake_address: stake_address.to_string(),
    timestamp,  // NOT signed -- can be tampered
}
```

An attacker who intercepts a valid signed message can modify the timestamp to any value while the signature remains valid. This undermines the freshness check in `validation.rs:94-115`. The dedup cache (payload-hash-based) catches exact replays, but an attacker can also modify the `stake_address` field to impersonate another identity.

The stress test at `stress.rs` explicitly acknowledges this: `"Freshness check directly (signature check would pass because the timestamp isn't included in the signed payload)"`.

**Impact**: Replay attacks with fresh timestamps bypass the +/-5 minute window. Identity field tampering enables impersonation.

**Fix**: Construct a canonical signed payload: `topic + timestamp_bytes + stake_address + sha256(payload)`. Sign this canonical message. Verifiers reconstruct and verify.

---

### H-2: Committee updates via gossip have no authority verification

**File**: `src-tauri/src/p2p/governance.rs:190-234`

When a `CommitteeUpdated` gossip announcement arrives, `handle_committee_updated` DELETE-and-replaces the entire committee membership for the DAO with zero authentication:

```rust
fn handle_committee_updated(db: &Database, dao_id: &str, members: &[String]) -> Result<(), String> {
    // ... checks DAO exists ...
    db.conn().execute("DELETE FROM governance_dao_members WHERE dao_id = ?1", ...)?;
    for addr in members {
        db.conn().execute("INSERT INTO governance_dao_members ...", ...)?;
    }
}
```

There is NO check that the gossip message sender is authorized to make this change. The `governance_dao_members` table controls who can sign taxonomy updates (checked by `taxonomy.rs:198-209`). This creates a privilege escalation chain: send fake committee update -> become committee member -> sign taxonomy updates -> corrupt the skill graph.

**Impact**: Full takeover of DAO governance. An attacker can replace the entire committee with their own addresses, then push arbitrary taxonomy changes that all nodes accept.

**Fix**: Committee updates must be authenticated via on-chain proof (verify the `on_chain_tx` field refers to a real transaction) or require multi-sig from the existing committee. At minimum, verify the gossip message sender is a current committee/chair member before processing.

---

### H-3: Public key not verified against claimed stake address

**File**: `src-tauri/src/p2p/validation.rs:78-91`

The validation pipeline verifies that the `payload` was signed by the `public_key` embedded in the message. However, nothing verifies that the `public_key` corresponds to the claimed `stake_address`.

The `SignedGossipMessage` struct (`types.rs:28-42`) includes both `public_key` (Ed25519, 32 bytes) and `stake_address` (bech32 Cardano address). The system trusts `stake_address` for identity (authority checks, sync_log records, committee membership). But an attacker can sign a message with their own key and set `stake_address` to any arbitrary value.

**Impact**: Identity spoofing. An attacker can impersonate any stake address -- a committee member, a trusted instructor, or any learner -- by simply setting the `stake_address` field. This bypasses all identity-based access controls including the taxonomy authority check.

**Fix**: Verify that the `public_key` corresponds to the claimed `stake_address` by deriving the stake address from the public key (via the same CIP-1852 derivation path used in `wallet.rs`) and comparing. Alternatively, require a Cardano-specific proof linking the Ed25519 key to the stake address.

---

### H-4: Dedup cache full clear creates replay window

**File**: `src-tauri/src/p2p/validation.rs:126-133`

When the dedup cache reaches 100,000 entries, the entire cache is cleared at once:

```rust
if seen.len() >= DEDUP_CACHE_MAX {
    log::info!("Dedup cache reached {DEDUP_CACHE_MAX} entries, clearing");
    seen.clear();
}
```

This creates an instant replay window where ALL previously-seen messages become re-processable. Combined with H-1 (timestamps not signed), the attacker can set fresh timestamps on old messages. Even without H-1, messages within the +/-5 minute freshness window that were previously deduplicated become valid again.

**Impact**: An attacker who has been collecting valid signed messages can wait for the cache clear and replay all of them, causing duplicate catalog entries, evidence records, or governance actions.

**Fix**: Replace `HashSet` with an LRU cache (`lru` crate) with a TTL of 10 minutes (2x the freshness window). Evict entries individually based on age, never clear the entire cache at once.

---

## MEDIUM

### M-1: Salt file has no integrity protection

**File**: `src-tauri/src/crypto/keystore.rs:96-97, 137-140`

The random salt is written to `vault_salt.bin` as a plain file (line 97). When loading, it is read back without any integrity check (line 138). If an attacker modifies the salt file, the derived key changes and the vault will not open (denial of service). If the attacker replaces the salt with a known value, they can pre-compute key tables for common passwords.

**Impact**: Denial of service (corrupted salt locks user out of vault permanently). If salt is replaced with a known value, precomputation attacks on the KDF become possible.

**Fix**: Store a MAC (HMAC or Blake2b-256) of the salt alongside it, or embed the salt inside the Stronghold snapshot itself (which is already integrity-protected).

---

### M-2: Mnemonic field in Wallet struct not zeroized on drop

**File**: `src-tauri/src/crypto/wallet.rs:34`

The `Wallet` struct stores the mnemonic as a plain `String`:

```rust
pub struct Wallet {
    pub mnemonic: String,  // Plain String, not Zeroizing<String>
    ...
}
```

When the `Wallet` is dropped, Rust deallocates the memory but does not zero it. The mnemonic may persist in freed memory until overwritten by a subsequent allocation. The `Keystore` correctly uses `Zeroizing<String>` for the password (line 63), but `Wallet.mnemonic` does not follow this pattern.

**Impact**: The mnemonic (which controls all funds and identity) may be recoverable from a memory dump, core dump, or swap file after the wallet is dropped.

**Fix**: Change `pub mnemonic: String` to `pub mnemonic: Zeroizing<String>`. Ensure all intermediate `String` copies during derivation also use `Zeroizing`.

---

### M-3: No password strength enforcement

**File**: `src-tauri/src/crypto/keystore.rs:86`, `src-tauri/src/commands/identity.rs:137`

Neither `Keystore::create()` nor the `generate_wallet`/`unlock_vault` commands enforce any minimum password complexity. A user can set an empty string as their vault password. Combined with C-1 (weak KDF), this means the vault could be cracked instantly.

**Impact**: Users with weak or empty passwords have effectively unencrypted vaults.

**Fix**: Add minimum password requirements before calling `Keystore::create()`: minimum 8 characters, reject common passwords. Enforce in the command handlers before the crypto operations.

---

### M-4: Mnemonic phrase returned over IPC in plaintext

**File**: `src-tauri/src/commands/identity.rs:192-196, 271-274`

The `generate_wallet` command returns the mnemonic phrase to the frontend in a `GenerateWalletResponse` struct (line 192). The `export_mnemonic` command returns it as a plain `String` (line 274). These travel over Tauri's IPC bridge as JSON.

While Tauri IPC is internal to the process (not network-exposed), the mnemonic may be logged by developer tools, persisted in JS memory, or captured by browser devtools if CSP is not enforced (see M-8).

**Impact**: The mnemonic may be exposed in the frontend's memory space, developer console logs, or IPC debug traces.

**Fix**: Mark the mnemonic field in the frontend response as sensitive (do not log it). Ensure the frontend zeroes the mnemonic from memory after displaying it. Consider adding a confirmation step where the user proves they wrote it down before dismissing the display.

---

### M-5: Proposal status set directly from gossip without validation

**File**: `src-tauri/src/p2p/governance.rs:149-183`

The `status` field from a `ProposalResolved` gossip message is written directly to the database without validating its value:

```rust
fn handle_proposal_resolved(db, proposal_id, status, votes_for, votes_against, on_chain_tx) {
    db.conn().execute(
        "UPDATE governance_proposals SET status = ?1, votes_for = ?2, votes_against = ?3, ...",
        params![status, votes_for, votes_against, on_chain_tx, proposal_id],
    )...;
}
```

A malicious peer can set `status` to any arbitrary string (e.g., `"approved"` for a proposal that was actually rejected). The `votes_for` and `votes_against` counts are also trusted from the gossip message without verification.

**Impact**: An attacker can falsely mark proposals as approved or rejected, manipulating DAO governance decisions without actually winning votes.

**Fix**: Validate that `status` is one of the allowed values (`"approved"`, `"rejected"`, `"expired"`). Verify vote counts against on-chain evidence or require multi-sig from committee members.

---

### M-6: No gossip rate limiting per peer

**File**: `src-tauri/src/p2p/network.rs:260-484`

The swarm event loop processes incoming gossip messages without any per-peer rate limiting. GossipSub's peer scoring (configured in `scoring.rs`) will eventually penalize misbehaving peers, but the scoring decay intervals are on the order of seconds, and the invalid message penalty requires messages to actually fail validation. A peer can flood valid-signature messages at high volume before scoring kicks in.

**Impact**: A malicious peer can send thousands of messages per second, consuming CPU on signature verification and database operations before GossipSub scoring suppresses them. This is a resource exhaustion vector.

**Fix**: Add a per-peer message rate limiter in the swarm event loop (e.g., a token bucket allowing 10 messages/second/peer). Drop excess messages before validation.

---

### M-7: Unsanitized column names in sync dynamic SQL

**File**: `src-tauri/src/p2p/sync.rs:591-595`

In `apply_row_update` (line 575-633), column names from the sync JSON data are interpolated directly into SQL SET clauses:

```rust
for (key, val) in obj {
    if key == "id" { continue; }
    set_clauses.push(format!("{key} = ?{idx}"));
    ...
}
```

While the table name is sanitized via `sanitize_table_name` (line 638-655), the column names from the JSON keys are not validated. A malicious sync peer could send a crafted JSON key. The same pattern exists in `apply_row_insert` (line 506-570) where column names are used in the INSERT statement (line 537).

Note: The values are properly parameterized, and SQLite's `conn.execute` does not support multi-statement execution, which limits the blast radius. The most likely impact is SQL errors or data corruption in the target table.

**Impact**: Potential SQL injection via crafted JSON keys in sync payloads.

**Fix**: Validate that all JSON keys match a whitelist of known column names for each syncable table. Reject sync payloads with unknown keys.

---

### M-8: CSP set to null in Tauri configuration

**File**: `src-tauri/tauri.conf.json:27-29`

The Content Security Policy is explicitly disabled:

```json
"security": {
  "csp": null
}
```

This means the webview can load scripts from any source, make network requests to any origin, and execute inline scripts without restriction.

**Impact**: If an XSS vulnerability exists in the frontend (or in any loaded content), the attacker has unrestricted access to the Tauri IPC bridge. Given that the IPC bridge exposes sensitive operations (vault unlock, mnemonic export, NFT minting), this significantly amplifies the impact of any frontend vulnerability.

**Fix**: Set a restrictive CSP: `"default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' https://cardano-preprod.blockfrost.io"`. Adjust as needed for the frontend framework.

---

## LOW

### L-1: Blockfrost API key stored in environment variable

**File**: `src-tauri/src/commands/cardano.rs:175-176`

The Blockfrost API key is read from an environment variable. Environment variables are visible to all processes running under the same user and may be logged by process managers.

**Impact**: The API key could be leaked via process inspection. Blockfrost keys are rate-limited but could be abused for API exhaustion if stolen.

**Fix**: Store the API key in the Stronghold vault or in an encrypted config file.

---

### L-2: Client-trusted integrity scores

**File**: `src-tauri/src/commands/integrity.rs:91-134, 175-191`

The `integrity_submit_snapshot` command accepts `integrity_score` and all sub-scores directly from the frontend. The `integrity_end_session` command accepts `overall_integrity_score` from the frontend. These scores are stored in the database as-is.

A cheating user can submit perfect integrity scores (1.0 for everything) regardless of their actual behavior during the assessment.

**Impact**: The integrity monitoring system can be trivially bypassed by a modified frontend or by directly calling the Tauri IPC commands with fabricated scores.

**Fix**: This is a known architectural limitation -- client-side integrity is inherently limited. For higher-stakes assessments, the system relies on the challenge/attestation mechanism (third-party verification). Consider adding anomaly detection on score patterns (e.g., rejecting scores that are suspiciously uniform at 1.0).

---

### L-3: `rand::thread_rng()` used for salt generation

**File**: `src-tauri/src/crypto/keystore.rs:282-287`

`rand::thread_rng()` returns a `ThreadRng` which wraps `OsRng` with ChaCha reseeding -- this IS cryptographically secure. However, for key-generation-critical code, using `OsRng` directly is considered best practice as it provides the minimum abstraction over the OS CSPRNG.

**Impact**: Negligible in practice. `ThreadRng` is CSPRNG-backed.

**Fix**: Replace `rand::thread_rng().fill_bytes(&mut salt)` with `rand::rngs::OsRng.fill_bytes(&mut salt)` for clarity.

---

### L-4: No dependency vulnerability scanning

**File**: `src-tauri/Cargo.toml`

The project does not have `cargo-audit` or `cargo-deny` configured for automated dependency vulnerability scanning. The dependency tree is large (libp2p alone brings in hundreds of transitive dependencies), and cryptographic libraries have had past advisories.

**Impact**: Known vulnerabilities in dependencies may go undetected.

**Fix**: Add `cargo-audit` to CI. Consider adding `deny.toml` for `cargo-deny` with advisory checks enabled.

---

### L-5: Test key material in stress tests uses predictable patterns

**File**: `src-tauri/src/p2p/stress.rs`

Stress tests generate keys via `SigningKey::generate(&mut rand::thread_rng())` which is fine for testing, but fixtures use predictable values like `vec![0; 32]` for public keys. Acceptable for tests but these fixtures must never leak into production code paths.

**Impact**: None in production. Informational for test hygiene.

**Fix**: No action needed -- test-only code. Ensure `#[cfg(test)]` gating remains in place.

---

### L-6: Stronghold error message matching for password detection

**File**: `src-tauri/src/crypto/keystore.rs:155-164`

Incorrect password detection relies on string matching against Stronghold's error messages:

```rust
if msg.contains("Decrypt") || msg.contains("decrypt")
    || msg.contains("IntegrityError") || msg.contains("integrity")
    || msg.contains("InvalidData") {
    KeystoreError::IncorrectPassword
} else {
    KeystoreError::Stronghold(msg)
}
```

If Stronghold changes its error message format in a future version, the wrong password case could fall through to a generic error.

**Impact**: Poor UX (user sees a generic error instead of "incorrect password"). No security impact.

**Fix**: Check if newer versions of `iota_stronghold` provide typed errors for decryption failure. If not, this pattern is a reasonable workaround but should be covered by integration tests.

---

## INFO (Positive findings)

### I-1: All SQL queries use parameterized statements

**Files**: All files in `commands/`, `p2p/catalog.rs`, `p2p/evidence.rs`, `p2p/governance.rs`, `p2p/taxonomy.rs`, `p2p/sync.rs`, `evidence/`

Every SQL query across the entire codebase uses `params![]` for value binding. No string interpolation of user-supplied values into SQL. The only dynamic SQL is in `p2p/sync.rs` where table names are sanitized via an allowlist (`sanitize_table_name` at line 638-655). Column names from sync JSON are the exception (see M-7).

---

### I-2: Foreign keys enabled on database

**File**: `src-tauri/src/db/mod.rs:31`

`PRAGMA foreign_keys = ON` is set on every connection. This prevents orphaned records and enforces referential integrity across the 30+ tables in the schema.

---

### I-3: Minimal Tauri capability configuration

**File**: `src-tauri/capabilities/default.json`

The capabilities file only grants `core:default` and `core:window:allow-show`. No filesystem access, no shell access, no clipboard access, no HTTP fetch from the frontend. This is a well-configured minimal permission set.

---

### I-4: GossipSub peer scoring well-configured

**File**: `src-tauri/src/p2p/scoring.rs`

All 5 topics have individually tuned scoring parameters. Taxonomy has the strongest invalid message penalty (`-50.0` with slow decay `0.3`). IP colocation penalty discourages Sybil attacks from the same IP. Thresholds are properly ordered (graylist < publish < gossip < 0). All parameters pass libp2p's built-in validation.

---

### I-5: Evidence score range validation

**File**: `src-tauri/src/p2p/evidence.rs:57-62`

Evidence announcements validate that `score` is in `[0.0, 1.0]` before storing. This prevents invalid evidence data from entering the local database via gossip.

---

## Remediation priority

| # | Finding | Effort | Impact |
|---|---------|--------|--------|
| 1 | C-1: Replace HMAC-SHA512 KDF with Argon2id | Low | Eliminates offline brute-force |
| 2 | H-1: Sign timestamp+topic+stake_address in gossip | Medium | Prevents replay and field tampering |
| 3 | H-2: Add authority verification for committee updates | Medium | Prevents governance takeover |
| 4 | H-3: Verify public_key to stake_address binding | Medium | Prevents identity spoofing |
| 5 | H-4: Replace dedup cache clear with LRU eviction | Low | Eliminates replay window |
| 6 | M-8: Set restrictive CSP | Low | Limits XSS blast radius |
| 7 | M-3: Add password strength requirements | Low | Prevents trivially weak passwords |
| 8 | M-2: Zeroize mnemonic in Wallet struct | Low | Protects secrets in memory |
| 9 | M-5: Validate proposal status from gossip | Low | Prevents governance manipulation |
| 10 | M-7: Validate sync JSON column names | Low | Prevents SQL injection via column names |
| 11 | M-6: Add per-peer rate limiting | Medium | Prevents resource exhaustion |
| 12 | M-1: Protect salt file integrity | Low | Prevents DoS and precomputation |
