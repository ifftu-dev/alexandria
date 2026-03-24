# Alexandria (Mark 3) -- Security Audit

**Date**: 2026-02-24 (updated 2026-03-23)
**Scope**: Full Rust backend (`src-tauri/src/`), Tauri configuration, Cargo dependencies, Vue frontend (`src/`), CI/CD workflows
**Files audited**: Every file in `crypto/`, `p2p/`, `commands/`, `db/`, `cardano/`, `evidence/`, `ipfs/`, plus `lib.rs`, `tauri.conf.json`, `capabilities/default.json`, both `Cargo.toml` files, all Vue components and composables

**Summary**: 1 critical, 7 high, 10 medium, 9 low, 5 informational findings.

### Remediation status (2026-03-23)

| Finding | Status | Notes |
|---------|--------|-------|
| C-1 | **FIXED** | `keystore.rs` now uses Argon2id (64MB/3iter/4lanes) |
| H-1 | **FIXED** | `signing.rs` now signs SHA-256(topic\|\|timestamp\|\|stake_address\|\|payload) |
| H-2 | **FIXED** | `governance.rs` verifies sender is committee/chair + transaction-wrapped |
| H-3 | DEFERRED | Requires identity attestation protocol; TOFU mitigates partially |
| H-4 | **FIXED** | `validation.rs` uses LRU cache with capacity eviction |
| H-5 | **FIXED** | All v-html sites sanitized with DOMPurify |
| H-6 | PARTIAL | CI warns on placeholder; keypair must be generated manually |
| H-7 | **FIXED** | `p2p_publish` command removed from IPC surface |
| M-1 | **FIXED** | Salt file now includes HMAC-SHA256 integrity tag |
| M-2 | **FIXED** | Wallet implements Drop with zeroization; Clone removed |
| M-3 | **FIXED** | 12-char minimum password enforced in generate/restore |
| M-4 | **FIXED** | Mnemonic cleared in onUnmounted + timeout |
| M-5 | **FIXED** | Proposal status validated against allowlist |
| M-6 | **FIXED** | Per-peer token-bucket rate limiter added (20msg/60s) |
| M-7 | **FIXED** | Column names validated via `sanitize_column_name()` |
| M-8 | **FIXED** | Restrictive CSP enabled in tauri.conf.json |
| M-9 | **FIXED** | Session password auto-clears after 15min timeout |
| M-10 | DEFERRED | Safety comment adequate; Mutex invariant maintained |
| L-1 | DEFERRED | Env var is standard practice |
| L-2 | DEFERRED | Architectural limitation |
| L-3 | **FIXED** | Salt generation uses OsRng directly |
| L-4 | **FIXED** | cargo-audit added to CI workflow |
| L-5 | N/A | Test-only code, no action needed |
| L-6 | DEFERRED | Needs integration test with Stronghold |
| L-7 | **FIXED** | SSRF blocklist rejects private/loopback IPs |
| L-8 | **FIXED** | Fonts bundled locally, CDN links removed |
| L-9 | **FIXED** | `.unwrap()` replaced with `.map_err()` across all commands |

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

**Partial mitigation**: The mobile keystore (`keystore_portable.rs`) already uses Argon2id with 64 MB memory cost and 3 iterations. This finding applies only to the desktop Stronghold path (`keystore.rs`).

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

### H-5: Cross-Site Scripting (XSS) via `v-html` with untrusted content

**Files**:
- `src/components/course/TextContent.vue:53` — `v-html="content"` renders HTML loaded from IPFS/inline content
- `src/components/course/CourseCard.vue:19` — `v-html="course.thumbnail_svg"`
- `src/pages/dashboard/Courses.vue:250` — `v-html="courseMap[enrollment.course_id]?.thumbnail_svg"`
- `src/pages/Home.vue:177` — `v-html="enrolledCourseMap[enrollment.course_id]?.thumbnail_svg"`

`TextContent.vue` renders raw HTML fetched from IPFS gateways or inline content via `v-html`. SVG thumbnails stored in the database are also rendered with `v-html`. Any course author can embed `<script>`, `<iframe>`, `<svg onload="...">`, or other XSS payloads. Since CSP is disabled (M-8), this runs with full Tauri IPC privileges.

**Impact**: Full Tauri IPC access. A malicious course author could steal the user's mnemonic via `invoke('export_mnemonic')`, mint NFTs, publish to the P2P network, or perform any other privileged operation. This is the primary exploitation vector for the disabled CSP (M-8).

**Fix**: Sanitize all HTML before rendering with `v-html`. Use DOMPurify. For SVG thumbnails, use a strict SVG sanitizer that strips `<script>`, event handlers, and `<foreignObject>`.

---

### H-6: Updater public key is a placeholder

**File**: `src-tauri/tauri.conf.json:62`

The Tauri updater signature verification key is set to `"pubkey": "PLACEHOLDER_PUBKEY"`. If the updater is activated before this is replaced with a real key, the app may accept unsigned updates or fail to verify updates.

**Impact**: Potential malicious update injection if the updater endpoint is compromised or MITM'd.

**Fix**: Generate a proper signing keypair and replace the placeholder before any release.

---

### H-7: `p2p_publish` allows raw unsigned message publishing

**File**: `src-tauri/src/commands/p2p.rs:238-249`

The `p2p_publish` command publishes raw bytes to any gossip topic without signing. While the receiving peers validate signatures, a compromised frontend (via XSS from H-5) could publish arbitrary data to the P2P network, potentially disrupting the gossip protocol or exploiting parsing bugs in other nodes.

**Impact**: Network abuse, reputation damage to the user's PeerId.

**Fix**: Remove or restrict `p2p_publish` to only accept pre-signed envelopes, or require that the topic/payload pass through the same signing pipeline used by `publish_catalog`, `publish_evidence`, etc.

---

## MEDIUM

### M-1: Salt file has no integrity protection

**File**: `src-tauri/src/crypto/keystore.rs:96-97, 137-140`

The random salt is written to `vault_salt.bin` as a plain file (line 97). When loading, it is read back without any integrity check (line 138). If an attacker modifies the salt file, the derived key changes and the vault will not open (denial of service). If the attacker replaces the salt with a known value, they can pre-compute key tables for common passwords.

**Impact**: Denial of service (corrupted salt locks user out of vault permanently). If salt is replaced with a known value, precomputation attacks on the KDF become possible.

**Fix**: Store a MAC (HMAC or Blake2b-256) of the salt alongside it, or embed the salt inside the Stronghold snapshot itself (which is already integrity-protected).

---

### M-2: Wallet struct does not zeroize secret key material on drop

**File**: `src-tauri/src/crypto/wallet.rs:31-48`

The `Wallet` struct contains `mnemonic: String`, `signing_key: SigningKey`, and `payment_key_extended: [u8; 64]`. The struct derives `Clone` (line 31) and does not implement `Zeroize` or `Drop`. When `Wallet` instances are dropped, the secret key material is not guaranteed to be zeroed in memory. The `Keystore` correctly uses `Zeroizing<String>` for the password (line 63), but `Wallet` does not follow this pattern.

Additionally, `leak_into_bytes` calls in `wallet.rs:161,168` extract raw key material. The returned byte arrays are stored in the struct which does NOT implement `Zeroize`.

**Impact**: The mnemonic, signing key, and payment key (which control all funds and identity) may be recoverable from a memory dump, core dump, or swap file after the wallet is dropped.

**Fix**: Wrap sensitive fields in `Zeroizing<>`, remove the `Clone` derive, and implement `Drop` with explicit zeroization. Ensure all intermediate `String` copies during derivation also use `Zeroizing`.

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

**Impact**: If an XSS vulnerability exists in the frontend (or in any loaded content), the attacker has unrestricted access to the Tauri IPC bridge. Given that the IPC bridge exposes sensitive operations (vault unlock, mnemonic export, NFT minting), this significantly amplifies the impact of any frontend vulnerability. **Note**: Specific XSS vectors have been identified -- see H-5.

**Fix**: Set a restrictive CSP: `"default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; connect-src ipc: http://ipc.localhost https://cardano-preprod.blockfrost.io; img-src 'self' data:"`. Adjust as needed for the frontend framework.

---

### M-9: Biometric password stored as plain string in frontend session

**File**: `src/composables/useBiometricVault.ts:5`

`let sessionBiometricPassword: string | null = null` stores the vault password as a plain JavaScript string in module scope for the entire app session. This is the session-based fallback when keychain entitlements are missing.

**Impact**: Any XSS attack (see H-5) can trivially read this variable. Even without XSS, JavaScript heap snapshots or debugging tools can extract it.

**Fix**: Minimize the window this value is held. Clear it after a timeout. Fixing CSP (M-8) and sanitizing v-html (H-5) mitigates the XSS vector.

---

### M-10: `unsafe impl Send + Sync for Database`

**File**: `src-tauri/src/db/mod.rs:30-31`

Manual `unsafe impl Send for Database {}` and `unsafe impl Sync for Database {}`. The safety relies on external Mutex synchronization + SQLite FULL_MUTEX mode, and `lib.rs` wraps `Database` in `Arc<std::sync::Mutex<Database>>`.

**Impact**: If the Mutex discipline is ever broken (e.g., refactoring that exposes `Database` without the mutex), this could cause undefined behavior.

**Fix**: Document the invariant more prominently. Consider a newtype wrapper that enforces the mutex at the type level.

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

### L-7: IPFS content resolver accepts arbitrary URLs (SSRF risk)

**File**: `src-tauri/src/ipfs/resolver.rs:162-181`, `src-tauri/src/ipfs/cid.rs:68-69`

The content resolver accepts `http://` and `https://` URLs as identifiers and fetches them via the gateway client. A course author could embed a URL pointing to an internal/private IP.

**Impact**: The reqwest client will attempt to connect to any URL, potentially probing internal networks (SSRF).

**Fix**: Validate that URLs point to expected IPFS gateways, or add a blocklist for private IP ranges (127.0.0.0/8, 10.0.0.0/8, 192.168.0.0/16, etc.).

---

### L-8: Google Fonts loaded from external CDN

**File**: `index.html:8-9`

Fonts are loaded from `fonts.googleapis.com` / `fonts.gstatic.com` at runtime.

**Impact**: Privacy concern (Google sees each user's IP on app launch). Availability concern (app typography degrades without network). Minor attack surface if the CDN is compromised.

**Fix**: Bundle the fonts locally in the app.

---

### L-9: `.unwrap()` on Mutex lock throughout command handlers

**Files**: Pervasive across `src-tauri/src/commands/` -- at least 20+ instances of `state.db.lock().unwrap()`.

If any thread panics while holding the database mutex, the mutex becomes poisoned and all subsequent `.unwrap()` calls will panic, crashing the app.

**Impact**: Denial of service (app crash). No data loss since SQLite WAL mode ensures durability.

**Fix**: Replace `.unwrap()` with `.map_err()` to return a user-facing error instead of crashing.

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

All 6 topics have individually tuned scoring parameters. Taxonomy has the strongest invalid message penalty (`-50.0` with slow decay `0.3`). IP colocation penalty discourages Sybil attacks from the same IP. Thresholds are properly ordered (graylist < publish < gossip < 0). All parameters pass libp2p's built-in validation.

---

### I-5: Evidence score range validation

**File**: `src-tauri/src/p2p/evidence.rs:57-62`

Evidence announcements validate that `score` is in `[0.0, 1.0]` before storing. This prevents invalid evidence data from entering the local database via gossip.

---

## Remediation priority

| # | Finding | Effort | Impact |
|---|---------|--------|--------|
| 1 | C-1: Replace HMAC-SHA512 KDF with Argon2id | Low | Eliminates offline brute-force |
| 2 | H-5+M-8: Sanitize v-html content AND set restrictive CSP | Low | **Blocks XSS-to-RCE chain (wallet theft)** |
| 3 | H-1: Sign timestamp+topic+stake_address in gossip | Medium | Prevents replay and field tampering |
| 4 | H-2: Add authority verification for committee updates | Medium | Prevents governance takeover |
| 5 | H-3: Verify public_key to stake_address binding | Medium | Prevents identity spoofing |
| 6 | H-4: Replace dedup cache clear with LRU eviction | Low | Eliminates replay window |
| 7 | H-6: Replace updater placeholder pubkey | Low | Prevents unsigned update injection |
| 8 | H-7: Restrict p2p_publish to signed envelopes | Low | Prevents network abuse via XSS |
| 9 | M-3: Add password strength requirements | Low | Prevents trivially weak passwords |
| 10 | M-2: Zeroize all secret material in Wallet struct | Low | Protects secrets in memory |
| 11 | M-9: Clear biometric session password after timeout | Low | Reduces XSS exposure window |
| 12 | M-5: Validate proposal status from gossip | Low | Prevents governance manipulation |
| 13 | M-7: Validate sync JSON column names | Low | Prevents SQL injection via column names |
| 14 | M-6: Add per-peer rate limiting | Medium | Prevents resource exhaustion |
| 15 | M-1: Protect salt file integrity | Low | Prevents DoS and precomputation |
| 16 | L-7: Add SSRF blocklist to IPFS resolver | Low | Prevents internal network probing |
