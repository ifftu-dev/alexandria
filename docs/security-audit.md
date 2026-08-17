# Alexandria — Security Audit

> **Two passes are recorded in this file.**
>
> - **Pass 1 (2026-02-24, remediation verified 2026-03-23)** — the original
>   audit of the pre-VC codebase. Findings `C-1` … `I-5`. Everything from
>   "Pass 1" down to "Deferred scope" below is that pass, unchanged.
> - **Pass 2 (2026-08-18)** — covers the VC-layer, plugin-system, Tauri-config
>   and dependency surface that Pass 1 explicitly deferred. Findings `P2-*`.
>   **[Jump to Pass 2](#pass-2--2026-08-18)**.
>
> Repo-level audits for the other monorepo components live in
> `../../docs/security-audit.md` (index), `alexandria-cloud/docs/security-audit.md`,
> `alexandria-relay/docs/security-audit.md` and
> `alexandria-monitoring/docs/security-audit.md`.

---

## Pass 1 — 2026-02-24 (remediation verified 2026-03-23)

**Scope**: Full Rust backend (`src-tauri/src/`), Tauri configuration, Cargo dependencies, Vue frontend (`src/`), CI/CD workflows
**Files audited**: Every file in `crypto/`, `p2p/`, `commands/`, `db/`, `cardano/`, `evidence/`, `content_store/`, plus `lib.rs`, `tauri.conf.json`, `capabilities/default.json`, both `Cargo.toml` files, all Vue components and composables

**Summary**: 1 critical, 7 high, 10 medium, 9 low, 5 informational findings.

### Remediation status (2026-03-23)

| Finding | Status | Notes |
|---------|--------|-------|
| C-1 | **FIXED** | `keystore.rs` now uses Argon2id (64MB/3iter/4lanes) |
| H-1 | **FIXED** | `signing.rs` now signs SHA-256(topic\|\|timestamp\|\|stake_address\|\|payload) |
| H-2 | **FIXED** | `governance.rs` verifies sender is committee/chair + transaction-wrapped |
| H-3 | **FIXED** | TOFU replaced by persistent `stake_pubkey_registry` seeded from a multisig-signed bootstrap snapshot and reconciled against on-chain `stake_pubkey_registration` UTxOs (witness-verified). See `docs/stake-pubkey-registry.md`. |
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

**Multi-user note (2026-05-19)**: With per-profile vaults, one stolen `.stronghold` file now compromises only that one profile's BIP-39 mnemonic — other profiles on the same device remain independently encrypted. This reduces blast radius but does not address the underlying weak KDF on desktop.

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
- `src/components/course/TextContent.vue:53` — `v-html="content"` renders HTML loaded from the content store/inline content
- `src/components/course/CourseCard.vue:19` — `v-html="course.thumbnail_svg"`
- `src/pages/dashboard/Courses.vue:250` — `v-html="courseMap[enrollment.course_id]?.thumbnail_svg"`
- `src/pages/Home.vue:177` — `v-html="enrolledCourseMap[enrollment.course_id]?.thumbnail_svg"`

`TextContent.vue` renders raw HTML fetched from a public URL origin or inline content via `v-html`. SVG thumbnails stored in the database are also rendered with `v-html`. Any course author can embed `<script>`, `<iframe>`, `<svg onload="...">`, or other XSS payloads. Since CSP is disabled (M-8), this runs with full Tauri IPC privileges.

**Impact**: Full Tauri IPC access. A malicious course author could steal the user's mnemonic via `invoke('export_mnemonic')`, mint NFTs, publish to the P2P network, or perform any other privileged operation. This is the primary exploitation vector for the disabled CSP (M-8).

**Fix**: Sanitize all HTML before rendering with `v-html`. Use DOMPurify. For SVG thumbnails, use a strict SVG sanitizer that strips `<script>`, event handlers, and `<foreignObject>`.

---

### H-6: Updater public key

**File**: `src-tauri/tauri.conf.json:74`

The Tauri updater signature verification key now holds a real base64-encoded minisign public key (no longer the `"PLACEHOLDER_PUBKEY"` string the original finding described). Per the remediation table, CI warns on a placeholder value and the signing keypair must still be generated/managed manually.

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

**File**: `src-tauri/src/crypto/keystore.rs:86`, `src-tauri/src/commands/profile.rs` (`create_profile`, `restore_profile_with_mnemonic`, `unlock_profile`)

> **Status note (2026-05-19):** the multi-user refactor replaced the
> legacy `generate_wallet`/`unlock_vault` IPC commands with
> `create_profile`/`restore_profile_with_mnemonic`/`unlock_profile`
> in `commands/profile.rs`. The 12-character minimum password
> validation now lives in `validate_password()` at the top of that
> file; this finding remains as historical context for the
> `Keystore::create()` API itself, which still does not enforce
> complexity at the type level.

Neither `Keystore::create()` nor the profile-lifecycle commands enforce any minimum password complexity beyond the 12-character length check. A user can set an extremely common password. Combined with C-1 (weak KDF), this means the vault could be cracked instantly.

**Impact**: Users with weak or empty passwords have effectively unencrypted vaults.

**Fix**: Add minimum password requirements before calling `Keystore::create()`: minimum 8 characters, reject common passwords. Enforce in the command handlers before the crypto operations.

---

### M-4: Mnemonic phrase returned over IPC in plaintext

**File**: `src-tauri/src/commands/profile.rs` (`create_profile`, `restore_profile_with_mnemonic`), `src-tauri/src/commands/identity.rs` (`export_mnemonic`)

> **Status note (2026-05-19):** post-multi-user refactor, the
> mnemonic is now returned by `create_profile` (as
> `CreateProfileResponse.mnemonic`) and `export_mnemonic`. The
> threat surface is unchanged.

The `create_profile` command returns the freshly-generated mnemonic phrase to the frontend in a `CreateProfileResponse` struct. The `export_mnemonic` command returns it as a plain `String`. These travel over Tauri's IPC bridge as JSON.

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

### L-7: Content resolver accepts arbitrary URLs (SSRF risk)

**File**: `src-tauri/src/content_store/resolver.rs:45` (`fn reject_private_url`, called at `:278`)

The content resolver accepts `http://` and `https://` URLs as identifiers and fetches them via the public URL HTTP client. A course author could embed a URL pointing to an internal/private IP.

**Impact**: The reqwest client will attempt to connect to any URL, potentially probing internal networks (SSRF).

**Fix**: Validate that URLs point to expected public URL origins, or add a blocklist for private IP ranges (127.0.0.0/8, 10.0.0.0/8, 192.168.0.0/16, etc.).

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

**Files**: All files in `commands/`, `p2p/catalog.rs`, `p2p/governance.rs`, `p2p/taxonomy.rs`, `p2p/sync.rs`, `evidence/`

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

Of the 15 `TOPIC_*` constants, 14 have individually tuned scoring parameters (all except `TOPIC_PEER_EXCHANGE`, which is intentionally unscored). Taxonomy has the strongest invalid message penalty (`-50.0` with slow decay `0.3`). IP colocation penalty discourages Sybil attacks from the same IP. Thresholds are properly ordered (graylist < publish < gossip < 0). All parameters pass libp2p's built-in validation.

---

### I-5: Evidence score range validation

**File**: `src-tauri/src/evidence/thresholds.rs`, `src-tauri/src/evidence/reputation.rs` (e.g. `reputation.rs:164,253` clamp scores to `[0.0, 1.0]`)

Evidence scores are constrained to `[0.0, 1.0]` before use. This prevents invalid evidence data from entering the local database. (The original `p2p/evidence.rs` gossip-validation site was deleted in the VC migration; the score-range logic now lives in the `evidence/` module.)

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
| 16 | L-7: Add SSRF blocklist to content resolver | Low | Prevents internal network probing |

---

## Deferred scope: VC-layer modules (PRs 2–19, post-audit)

> **Status (2026-08-18): CLOSED — this deferred scope was audited in
> [Pass 2](#pass-2--2026-08-18) below.** All five follow-up questions listed
> here were answered; four of them turned up live findings (`P2-C1`, `P2-H1`,
> `P2-H2`, `P2-M4`). The text below is kept as the original statement of
> deferred scope.

This audit's snapshot date (2026-03-23) precedes the VC-first
credential migration that landed across PRs 2–19. The remediation
status table above is accurate as-of-date — every "FIXED" claim
verified against the current source — but the following modules
were added after the audit and **have not been independently
reviewed**:

- `crypto/did.rs` — `did:key` derivation, parsing, key registry
- `domain/vc/{mod,canonicalize,context,sign,verify}.rs` — JCS
  canonicalisation, Ed25519Signature2020 detached JWS, §13.2
  acceptance predicate
- `aggregation/{mod,weights,level,independence,antigaming,config}.rs`
  — §14 trust aggregation + §15 anti-gaming
- `commands/{credentials,presentation,pinning,aggregation}.rs` —
  IPC surface for VC issuance/verification/export, presentation
  envelopes, PinBoard commitments, aggregated-state queries
- `p2p/{vc_did,vc_status,vc_fetch,presentation,pinboard,archive}.rs`
  — gossip handlers + libp2p request-response protocol
  `/alexandria/vc-fetch/1.0`
- `cardano/{anchor_queue,anchor_tx}.rs` — credential integrity
  anchoring on Cardano (label 1697 metadata transactions)
- `content_store/pinboard.rs` — PinBoard-driven 5-tier eviction policy

A follow-up audit pass should specifically cover:

1. **Authority verification** on the four new gossip topics
   (`vc-did`, `vc-status`, `vc-presentation`, `pinboard`) — does
   each handler verify that the message author is the legitimate
   subject/issuer for the operation?
2. **Replay/dedup** on the request-response `/alexandria/vc-fetch/1.0`
   protocol — outbound responses are not deduplicated by the
   gossip layer; nonces in `FetchRequest` are not currently
   validated for freshness.
3. **Selective-disclosure canonicalisation** — verify that
   presentation envelopes (`p2p/presentation.rs`) bind audience +
   nonce into the JCS-canonical signed payload, and that
   `presentations_seen` is consulted before acceptance.
4. **Allowlist amplification** — does the per-credential
   `credential_allowlist` table properly gate fetch responses, and
   does the `'public'` sentinel correctly fan out without inadvertent
   amplification of unbounded fetch traffic?
5. **Anchor queue resilience** — `cardano::anchor_queue` does
   exponential backoff but persists `last_error`; ensure errors
   don't leak Blockfrost API tokens into log lines.

---
---

# Pass 2 — 2026-08-18

**Scope**: everything Pass 1 deferred, plus everything added since
2026-03-23 — the VC gossip/request-response layer, the community plugin
system (manifest → install → `plugin://` asset protocol → Wasmtime grader),
the current Tauri security configuration and capability set, the macOS
webview delegates, and the Rust + npm dependency trees.

**Files audited**: `p2p/{vc_did,vc_status,vc_fetch,presentation,pinboard,archive,signing,validation,types,registry,guardian,device_sync}.rs`,
`crypto/{key_registry,did,keystore,pairing}.rs`,
`crates/alexandria-verify/src/vc/verify.rs`, `domain/vc/mod.rs`,
`content_store/{pinboard,resolver,http}.rs`,
`plugins/{registry,verifier,manifest,asset_protocol,wasm_runtime}.rs`,
`commands/p2p.rs`, `src-tauri/tauri.conf.json`,
`src-tauri/capabilities/default.json`, `macos_media_delegate.rs`,
`src/components/plugin/{PluginIframe,PluginHost}.vue`,
`.github/workflows/*.yml`, `Cargo.lock`, `package-lock.json`.

**Summary**: 1 critical, 4 high, 6 medium, 4 low, 3 informational.

### Remediation status (2026-08-18, branch `fix/security-audit-2026-08`)

| Finding | Status | Notes |
|---------|--------|-------|
| P2-C1 | **FIXED** | Three independent layers, because one was what failed before: (1) `purge_precompiled_graders` deletes every `.cwasm` right after `copy_tree`, unconditionally; (2) it runs again on every `write_precompiled_grader` failure path — the branch that used to leave the attacker's file in place; (3) `compile_and_persist` records a BLAKE3 sidecar and `load_module` refuses to `deserialize` any artifact without a matching one, so the `SAFETY` comment is now backed by a check. Test: `a_bundle_cannot_smuggle_in_a_precompiled_grader`. |
| P2-H1 | **FIXED** | `vc-status` documents carry a detached Ed25519 `proof` over `canonical_status_bytes(list_id, issuer, version, bits)`, verified against the issuer's self-resolving `did:key`. Unsigned and impostor-signed documents are dropped. `build_signed_status_document` is the only supported producer, so the two sides cannot drift. Bitmap capped at 1 MiB. Six tests including bitmap- and version-tampering. |
| P2-H2 | **FIXED** | `FetchRequest` carries a `proof` over `canonical_fetch_bytes(credential_id, requestor, nonce)`; `handle_fetch_request` verifies it before consulting the subject or the allowlist. Test `claiming_to_be_the_subject_without_the_key_is_refused` covers exactly the reported attack. |
| P2-H3 | **FIXED** | `fs:allow-read-file` narrowed from `$HOME/**` to `$DOWNLOAD`, `$DOCUMENT`, `$DESKTOP` — the directories the two call sites actually pick files from — with dotfiles denied. An allowlist rather than a denylist, since the denylist's misses (`$HOME/.local/share`, `$HOME/AppData`) were the finding. |
| P2-H4 | **FIXED** | The iframe `allow` attribute is built from **granted** capabilities, not declared ones, and the macOS delegate answers from a host-recorded grant table (`plugin_set_media_grants` / `plugin_clear_media_grants`) instead of granting unconditionally. Grants are cleared on plugin teardown. Device-orientation stays auto-granted, now with the reasoning written down rather than left as "symmetry". |
| P2-M1 | **FIXED** | `handle_did_message` requires that the envelope's public key be the one the announced DID resolves to. Closes the key-rollback primitive and the unauthenticated `key_registry` insert that manufactured P2-H1's precondition. Four tests. |
| P2-M2 | **FIXED** | `PluginManifest.files` pins BLAKE3 per bundle-relative path, enforced at install in both directions — a changed file **and** an unlisted extra file are both refused. Optional, so pre-existing manifests still install. Four tests. |
| P2-M3 | **FIXED** | `is_valid_plugin_cid` requires 64 lowercase hex, checked in `asset_protocol::handle` before the CSP header is built and again in `resolve_asset` before the path join. Closes the traversal and the `;`-in-CID CSP injection together. |
| P2-M4 | **FIXED** | Commitments are verified on ingest against `pinner_did`'s resolved key, and signed at declaration time rather than written as `"unsigned"` and fixed up later — so the durable state is never a row peers would refuse. Four tests. |
| P2-M5 | **FIXED** | Rebuilt on the `url` crate: userinfo refused, host resolved and **every** resolved address checked, allowlist-of-public rather than blocklist-of-private, IPv4-mapped IPv6 normalised. The redirect policy re-runs the same check on each hop. Writing the tests caught a bug in the fix itself — `to_ipv4()` maps `::1` to the routable `0.0.0.1` — now `to_ipv4_mapped()` only, with a regression case. |
| P2-M6 | **PARTIAL** | Fixed: DOMPurify (via `npm audit fix`, now 3.4.13), `nanoid`, `wasmtime` 36.0.12 → 36.0.13, `webbrowser` 1.2.1 → 1.2.4. `npm audit` reports zero. Accepted with written reasoning in `.cargo/audit.toml`: `tract-nnef`, `rkyv`, two `quick-xml` copies, two `hickory-proto`. See the correction below — the tract entry is not what the audit said it was. |
| P2-L1 | **FIXED** | `encrypted` and `key_id` folded into `canonical_signed_bytes`, length-prefixed so `None` and `Some("")` differ. Three tests. |
| P2-L2 | **FIXED** | Dedup keys on `(topic, stake_address, timestamp, payload)`. Tests cover the suppression primitive (two authors, identical payload) and confirm a verbatim replay is still caught. |
| P2-L3 | **FIXED** | `ActivityPull` requires a `sealed_marker` opening to `pull:<link_id>`, matching what `Revoke` already required. Two tests. |
| P2-L4 | **FIXED** | All 66 third-party action references across seven workflows pinned to full commit SHAs with the tag in a trailing comment. |

`cargo audit` and `npm audit` both report zero, and `npm audit` was added
alongside the existing `cargo audit` CI job.

### Correction to P2-M6 (tract-nnef)

The finding said the `tract-nnef` OOB-read advisory was "not remotely reachable
today" because models are `include_bytes!`-bundled. **That was wrong.**
`sentinel_load_dao_classifier` (`commands/sentinel_ml.rs`) accepts arbitrary
ONNX bytes over IPC that the frontend fetched from the network, and its doc
comment explicitly deferred verification to the caller — so attacker-supplied
model bytes did reach the parser.

The upgrade is genuinely blocked: tract 0.21.16 pins `half = "=2.4.1"` while
naga 27 (via wgpu → eframe → the vendored `moq-media`) needs `half ^2.5`, and
no version satisfies both. So the fix is at the call site instead — the command
now refuses any blob whose BLAKE3 is not a ratified `sentinel_priors.cid`, and
`.cargo/audit.toml` records that this mitigation is what the acceptance rests
on.

### Not fixed

- **P2-M6 residue** — the six accepted advisories above. Each names in
  `.cargo/audit.toml` why it is not exploitable here and what would change that.
- **`clippy::items_after_test_module` in `cli/src/tui/app.rs`** — pre-existing
  and unrelated to security; surfaced only because this pass ran clippy with
  `--all-targets`, which CI does not. An attempt to move the trailing item
  above the test module split a doc comment from its function, so it was
  reverted rather than left half-done.
- Everything under "Not re-audited in Pass 2" below.

| # | Severity | Finding |
|---|----------|---------|
| P2-C1 | CRITICAL | Attacker-supplied `.cwasm` survives plugin install and is `unsafe`-deserialized → native code execution outside the Wasmtime sandbox |
| P2-H1 | HIGH | `vc-status` gossip has no issuer authentication — any peer can forge or erase any issuer's revocation list |
| P2-H2 | HIGH | `vc-fetch` trusts a self-asserted `requestor` DID — allowlist and private-credential gating are bypassable |
| P2-H3 | HIGH | `fs:allow-read-file` scoped to `$HOME/**`; the deny list misses the Linux/Windows app-data directory holding the vault |
| P2-H4 | HIGH | macOS WKWebView auto-grants camera/microphone to any plugin that *declares* the capability, bypassing the in-app consent prompt |
| P2-M1 | MEDIUM | `vc-did` gossip accepts unauthenticated key-rotation records for arbitrary DIDs (key-rollback + unbounded table growth) |
| P2-M2 | MEDIUM | Plugin CID covers only `manifest.json` — UI bundle contents are neither content-addressed nor attested |
| P2-M3 | MEDIUM | `plugin_cid` is not charset-validated before `Path::join` and CSP-header interpolation |
| P2-M4 | MEDIUM | PinBoard commitments are ingested from gossip without verifying the signature they carry |
| P2-M5 | MEDIUM | SSRF blocklist in `content_store::resolver` is bypassable (userinfo, integer-literal IPs, DNS names, redirects) |
| P2-M6 | MEDIUM | Known-vulnerable dependencies: DOMPurify sanitizer bypass, `rkyv`, `tract-nnef`, `wasmtime`, `webbrowser`, `nanoid` |
| P2-L1 | LOW | `encrypted` / `key_id` envelope fields are outside the signed canonical bytes |
| P2-L2 | LOW | Gossip dedup key is the payload hash alone — cross-topic and cross-author collisions |
| P2-L3 | LOW | Guardian `ActivityPull` builds and seals a full snapshot before proving key possession |
| P2-L4 | LOW | GitHub Actions pinned by tag, not by commit SHA |
| P2-I1 | INFO | Wasmtime grader sandbox is correctly configured (zero imports, fuel, memory cap, deterministic) |
| P2-I2 | INFO | Device-sync and guardian pairing authentication chains are sound |
| P2-I3 | INFO | Pass 1 remediations re-verified: Argon2id KDF, canonical signed envelope, LRU dedup, restrictive CSP |

---

## CRITICAL

### P2-C1: Untrusted precompiled `.cwasm` survives plugin install and is executed as native code

**Files**: `src-tauri/src/plugins/registry.rs:145` (`copy_tree`), `:75-91`
(`write_precompiled_grader`), `src-tauri/src/plugins/wasm_runtime.rs:380`
(`Module::deserialize_file`)

`install_from_directory` copies the entire community bundle verbatim:

```rust
copy_tree(src_dir, &dest_dir).map_err(|e| format!("failed to copy plugin bundle: {e}"))?;
```

`copy_tree` (`registry.rs:722-744`) filters symlinks only. It does **not**
filter `grader.<os>-<arch>-<backend>.cwasm`, the precompiled-native-code
artifact. The code immediately after is intended to neutralise that — the
comment at `:157-158` says *"Never trust a `.cwasm` a community bundle might
ship — regenerate it locally"* — but the regeneration is best-effort:

```rust
fn write_precompiled_grader(dest_dir: &Path, grader_bytes: &[u8]) {
    match crate::plugins::wasm_runtime::precompile_grader(grader_bytes) {
        Ok(cwasm) => { /* overwrites the shipped file */ }
        Err(e) => log::warn!("failed to precompile grader (will JIT on first grade): {e}"),
    }
}
```

On the error branch the attacker's file is left in place. At grade time,
`wasm_runtime.rs:378-390` **prefers** the on-disk `.cwasm`:

```rust
// SAFETY: `path` was written by `precompile_grader` on this machine ...
match unsafe { Module::deserialize_file(&self.engine, path) } {
```

The `SAFETY` comment states an invariant the install path does not establish.
Wasmtime documents `deserialize` as unsafe precisely because its version/config
header is a compatibility check, not an authenticity check.

**Exploit**: publish a bundle with (a) a validly signed `manifest.json`
declaring a grader, (b) a `grader.wasm` whose BLAKE3 matches
`manifest.grader.cid` but which fails Cranelift validation, and (c) a crafted
`grader.<os>-<arch>-cranelift.cwasm`. The blake3 check at
`commands/plugins.rs:639-643` compares hashes and passes. `precompile_grader`
fails, leaving (c) intact. The first grade `deserialize`s (c).

**Impact**: arbitrary native code execution in the host process on first
grading — full escape from the Wasmtime sandbox the whole plugin design rests
on. The process holds the unlocked vault, the Cardano signing key and the full
IPC surface.

**Fix** (defence in depth, all three):
1. In `install_from_directory`, after `copy_tree`, delete every `*.cwasm` under
   `dest_dir` — or reject the bundle outright if one is present. A bundle has
   no legitimate reason to ship one.
2. Make `write_precompiled_grader` remove any existing `.cwasm` on the failure
   branch rather than logging and returning.
3. Record the BLAKE3 of each `.cwasm` this machine writes (e.g. in
   `plugin_installed`), and verify it before `deserialize_file`. Then the
   `SAFETY` comment is backed by a check rather than by an assumption.

---

## HIGH

### P2-H1: `vc-status` gossip applies revocation lists with no issuer authentication

**File**: `src-tauri/src/p2p/vc_status.rs:34-105`, dispatched at
`src-tauri/src/commands/p2p.rs:192`

The module header claims *"Receivers verify the issuer is known to their
`key_registry` (otherwise they have no public key to validate the inner
signature against)"*. No inner signature is validated anywhere in the file.
The only gate is existence:

```rust
let known: i64 = db.conn().query_row(
    "SELECT COUNT(*) FROM key_registry WHERE did = ?1", ...)?;
if known == 0 { return Ok(StatusIngest::IgnoredUnknownIssuer); }
```

`TOPIC_VC_STATUS` is not in `registry::is_privileged_topic`
(`p2p/registry.rs:477-487`), so the envelope-level identity binding does not
apply either. Any peer that can publish on the topic can therefore write any
issuer's status list, with attacker-chosen `version` and `bits`:

```rust
"... ON CONFLICT(list_id) DO UPDATE SET version = excluded.version, bits = excluded.bits, ..."
```

`verify::verify_credential` reads those bits directly
(`crates/alexandria-verify/src/vc/verify.rs:73-83`) and sets `revoked`.

**Impact**, all remotely triggerable by any network participant:
- **Mass forged revocation** — set every bit for a real issuer and every
  credential that issuer ever signed reads as revoked on every receiving node.
- **Forged un-revocation** — publish `version = prev + 1` with zeroed bits and
  genuine revocations disappear. This is the more damaging direction: a
  revoked credential silently returns to `Accept`.
- **Permanent lockout** — publish `version = i64::MAX` and the rollback guard
  (`if parsed.version <= prev`) then rejects every subsequent update from the
  legitimate issuer, forever.
- The `key_registry` precondition is not a real barrier: `P2-M1` lets any peer
  insert any DID into that table first.

`bits` is also unbounded — `base64::decode` then straight into the row, with
no cap on the decoded length.

**Fix**: require the status document to carry an issuer signature over
`(list_id, version, bits)` and verify it against the issuer DID's key before
the upsert; or add `TOPIC_VC_STATUS` to the privileged-topic set and require
that the envelope signer resolve to the `issuer` DID. Cap the decoded `bits`
length.

---

### P2-H2: `vc-fetch` authorises on a self-asserted requestor DID

**File**: `src-tauri/src/p2p/vc_fetch.rs:41-80`, wired at
`src-tauri/src/p2p/network.rs:2020-2046`

The access-control decision is made against a field of the request body:

```rust
pub struct FetchRequest {
    pub credential_id: String,
    pub requestor: Did,   // self-asserted, unsigned
    pub nonce: String,    // never read
}

if req.requestor.as_str() == subject_did
    || is_allowlisted(db, &req.credential_id, req.requestor.as_str())
```

Nothing proves the caller controls `requestor`. The libp2p `PeerId` — which
*is* authenticated, by the Noise handshake — is available at the call site
(`network.rs:2020` binds `peer`) and is discarded.

Subject DIDs are public: they are broadcast on `TOPIC_VC_DID`, appear inside
every credential, and are the primary key of the talent index. So any peer can
send `requestor = <the subject's own DID>` and take branch 2, retrieving any
credential the node holds regardless of the `credential_allowlist`.

**Impact**: the entire per-credential privacy model on the pull path is
bypassable. Private credentials — the default state — are readable by any peer
that knows or guesses a `credential_id`.

The `nonce` field is accepted and never checked, so there is also no freshness
or replay binding on this protocol.

**Fix**: make the request prove control of `requestor` — either sign
`(credential_id, requestor, nonce, timestamp)` with the requestor's DID key and
verify it here, or bind the requestor DID to the authenticated `PeerId` at
connection time and pass the `PeerId` into `handle_fetch_request`. Track
`nonce` in a seen-cache with a freshness window.

---

### P2-H3: `fs:allow-read-file` grants `$HOME/**` with a deny list that misses the vault

**File**: `src-tauri/capabilities/default.json`

Pass 1's finding `I-3` recorded the capability set as "no filesystem access".
That is no longer true:

```json
{ "identifier": "fs:allow-read-file",
  "allow": [{ "path": "$HOME/**" }],
  "deny":  [ "$HOME/.ssh/**", "$HOME/.gnupg/**", "$HOME/.aws/**",
             "$HOME/.config/**", "$HOME/Library/**" ] }
```

The frontend can read any file under `$HOME` except those five subtrees. The
deny list is macOS-shaped: `$HOME/Library/**` covers the app-data directory on
macOS, but on Linux Tauri's app-data path is `$HOME/.local/share/<bundle>/`
and on Windows it is under `$HOME/AppData/`. Neither is denied. That directory
holds the per-profile `.stronghold` snapshot, `vault_salt.bin` and the SQLite
database.

Also absent from the deny list: `$HOME/.local/share/keyrings`,
`$HOME/.mozilla`, `$HOME/.password-store`, `$HOME/.gitconfig`,
`$HOME/.docker/config.json`, and every cryptocurrency wallet directory.

**Impact**: on Linux and Windows, any frontend code-execution primitive — the
XSS class Pass 1's `H-5` addressed, or a compromised dependency in the Vue
bundle — reads the encrypted vault and salt straight off disk and can then
brute-force offline at leisure. Argon2id (`C-1`, fixed) raises the cost of that
offline attack but does not prevent the exfiltration.

**Fix**: replace the `$HOME/**` allow with the narrowest set of scopes the two
call sites actually need. Only `useSkillBootstrap.ts:39` and
`PluginHost.vue:302` use the plugin, and both read a file the user just chose
in a native picker — that is `$DOWNLOAD`, `$DOCUMENT` and `$DESKTOP`, not all
of `$HOME`. If a broad scope must stay, add `$HOME/.local/share/**`,
`$HOME/AppData/**` and `$HOME/.local/state/**` to the deny list.

---

### P2-H4: macOS auto-grants camera and microphone on capability *declaration*, not on consent

**Files**: `src-tauri/src/macos_media_delegate.rs:31-105`,
`src/components/plugin/PluginIframe.vue:119-139,492`

Two independent decisions combine badly.

The Vue host delegates the Permissions-Policy feature to the iframe based on
what the plugin's manifest *declares*:

```ts
const allowAttribute = computed(() => {
  const map = { microphone: 'microphone', camera: 'camera', ... }
  for (const cap of props.declaredCapabilities) { ... }   // declared, not granted
})
```

and the macOS `WKUIDelegate` answers every capture request with an
unconditional grant:

```rust
const WK_PERMISSION_DECISION_GRANT: i64 = 1;
unsafe extern "C-unwind" fn request_media_capture(
    _this, _cmd, _webview, _origin, _frame, _capture_type, decision_handler) {
    // `_origin` and `_capture_type` are both ignored
    (*decision_handler).call((WK_PERMISSION_DECISION_GRANT,));
}
```

The in-app consent UX (`PluginHost.vue` → `PermissionPrompt.vue`) runs over the
postMessage bridge. A plugin is not obliged to use the bridge. It can call
`navigator.mediaDevices.getUserMedia()` directly from its own iframe: the
feature policy is already delegated at mount time, and WebKit grants without
prompting.

**Impact**: on macOS, any installed plugin that lists `camera` or `microphone`
in its manifest can capture audio/video silently, with neither an OS prompt nor
the app's own prompt. Given the product context — proctored assessment,
learners including minors, guardian links — this is the worst place for a
silent capture path.

**Fix**:
1. Derive `allowAttribute` from *granted* capabilities, not declared ones. The
   iframe is already keyed on `allowAttribute`, so promoting a grant re-mounts
   it with the feature enabled.
2. In `request_media_capture`, check `_origin` is a `plugin://` origin and
   consult a host-side grant table keyed by plugin CID and capture type before
   returning `GRANT`; return `WK_PERMISSION_DECISION_DENY` otherwise.

---

## MEDIUM

### P2-M1: `vc-did` gossip accepts unauthenticated key rotations for arbitrary DIDs

**File**: `src-tauri/src/p2p/vc_did.rs:28-94`, dispatched at
`src-tauri/src/commands/p2p.rs:190`

`handle_did_message` never relates the message sender to the DID in the
payload. `message.public_key` and `message.stake_address` are unused. A
rotation announcement for any DID from any peer closes that DID's open registry
row and opens a new one:

```rust
if let Some(rotated_to) = parsed.rotated_to {
    conn.execute("UPDATE key_registry SET valid_until = ?2, rotated_by = ?3 \
                  WHERE did = ?1 AND valid_until IS NULL", ...)?;
    conn.execute("INSERT OR IGNORE INTO key_registry \
                  (did, key_id, public_key_hex, valid_from, valid_until, rotated_by) \
                  VALUES (?1, ?2, '', ?3, NULL, NULL)", ...)?;
```

The inserted row carries an **empty** `public_key_hex`. In
`resolve_issuer_key` (`alexandria-verify/src/vc/verify.rs:138-149`) an empty
key fails `verifying_key_from_slice` and the resolver falls through to
`did:key` self-resolution — which is why this is not a signature-forgery bug.

It is a **key-rollback** bug. After a legitimate local rotation
(`crypto/key_registry.rs:81-145` stores the real post-rotation pubkey), a
forged gossip rotation closes that row and inserts an empty-key row covering
the present. Verification then falls back to `did:key` self-resolution — the
*pre-rotation* key. If the rotation was performed because the old key was
compromised, an attacker holding it can restore its acceptance network-wide.

Secondarily, the bare-announcement branch is an unauthenticated
`INSERT OR IGNORE` into `key_registry` with no rate limit or cap — unbounded
local table growth, and the row that satisfies `P2-H1`'s "known issuer" gate.

**Fix**: require that the envelope signer resolve to the announced DID —
`did_from_verifying_key(&message.public_key) == parsed.did` for `did:key`, or a
registry-backed binding otherwise. Refuse rotations whose new row would carry
an empty `public_key_hex`; carry the rotated-to key material in the payload and
verify it. Cap unauthenticated DID inserts per peer.

---

### P2-M2: Plugin CID covers only the manifest, not the bundle

**File**: `src-tauri/src/plugins/verifier.rs:15-21`, `registry.rs:117-125`

```rust
pub fn compute_plugin_cid(manifest_bytes: &[u8]) -> String {
    blake3::hash(manifest_bytes).to_hex().to_string()
}
```

The identity of a plugin — the value courses pin, the value the Plugin DAO
attests over on `TOPIC_PLUGIN_ATTESTATIONS`, the value that becomes the
`plugin://<cid>` origin — is a hash of `manifest.json` alone. The manifest
commits to the grader via `PluginGraderRef { cid, blake3 }`
(`domain/plugin.rs:165-170`) and that *is* checked at grade time
(`commands/plugins.rs:639-643`). Nothing commits to `ui/index.html` or any
other bundle file.

**Impact**: two bundles with byte-identical manifests, identical signatures and
identical CIDs can ship completely different iframe code. A DAO attestation
over `(plugin_cid, grader_cid)` therefore says nothing about the UI the learner
actually runs, and a compromised distribution path can swap the UI without
invalidating the author signature or the attestation.

**Fix**: add a `files` map to the manifest — relative path → BLAKE3 — covering
every file in the bundle, verified at install and on each asset read; or make
the CID a Merkle root over the bundle tree rather than a hash of one file.

---

### P2-M3: `plugin_cid` is unvalidated before path join and CSP interpolation

**File**: `src-tauri/src/plugins/registry.rs:748-776`,
`src-tauri/src/plugins/asset_protocol.rs:51-127`

`resolve_asset` guards the *relative path* and not the CID:

```rust
if relative_path.starts_with('/') || relative_path.contains("..") { return Err(...) }
let root = plugins_dir.join(plugin_cid);          // plugin_cid unchecked
let requested = root.join(relative_path);
if !canonical_requested.starts_with(&canonical_root) { return Err(...) }
```

The containment check compares against `canonical_root`, which is itself
derived from the untrusted `plugin_cid` — so a CID that escapes moves the root
along with it and the check passes trivially. The only thing standing between
that and an arbitrary-file-read is URL normalisation in the webview, which
differs per platform (`plugin://<cid>/…` on WKWebView/webkit2gtk vs
`http://plugin.localhost/<cid>/…` on WebView2/Android) and is not a security
control this code owns.

Separately, the CID is interpolated into a response header value:

```rust
let csp = PLUGIN_CSP_TEMPLATE.replace("{cid}", &plugin_cid).replace("{nonce}", &nonce);
```

A CID containing `;` injects CSP directives into that plugin's own policy.

**Fix**: validate `plugin_cid` against `^[0-9a-f]{64}$` (it is a BLAKE3 hex
digest) at the top of `asset_protocol::handle` and again in `resolve_asset`,
rejecting anything else before either the join or the header build.

---

### P2-M4: PinBoard commitments are ingested without verifying their signature

**Files**: `src-tauri/src/p2p/pinboard.rs:20-24`,
`src-tauri/src/content_store/pinboard.rs:96-118`

`PinboardCommitment` carries `signature` and `public_key` fields. The ingest
path stores them and checks neither:

```rust
pub fn handle_pinboard_message(db: &Database, message: &SignedGossipMessage) -> Result<(), String> {
    let commit: PinboardCommitment = serde_json::from_slice(&message.payload)?;
    crate::content_store::pinboard::record_observation(db.conn(), &commit)
}
```

`record_observation` → `insert_observation` is an unconditional
`INSERT OR IGNORE`. There is no signature verification anywhere in the module,
and the local declaration path writes the literal string `"unsigned"` into the
column (`content_store/pinboard.rs:32-34`) — so the field is a control that
exists in the schema and is enforced nowhere.

**Impact today** is bounded: `list_pinners_for` is not yet consulted by
`content_store::storage`'s eviction engine, which still keys off `auto_unpin`.
So the live impact is an unauthenticated, uncapped, attacker-controlled write
into `pinboard_observations` — local DB growth and poisoned redundancy
reporting.

**Impact when §12/§20.4 eviction lands** is data loss: forged commitments
claiming that N peers pin a subject would let a node conclude its own copy is
redundant and evict content that in fact exists nowhere else.

**Fix**: verify `signature` over the canonical commitment fields against
`public_key`, and check that `public_key` resolves to `pinner_did`, before
`record_observation`. Do it now, before the eviction policy starts trusting the
table.

---

### P2-M5: SSRF blocklist is bypassable

**File**: `src-tauri/src/content_store/resolver.rs:45-105`, called at `:278`

Pass 1's `L-7` was marked **FIXED**; the fix is a string-parsed host check and
several standard bypasses get through it:

```rust
let authority = url.split("://").nth(1)...split('/').next()...;
let host = /* strips port, unwraps [..] for v6 */;
let blocked_hosts = ["localhost", "0.0.0.0"];
if let Ok(ip) = host.parse::<std::net::IpAddr>() { /* private-range checks */ }
```

- **Userinfo** — `http://example.com@127.0.0.1/x` yields
  `host = "example.com@127.0.0.1"`, which is neither in `blocked_hosts` nor
  parseable as an `IpAddr`, so it is allowed. reqwest then connects to
  127.0.0.1.
- **Integer / octal literals** — `http://2130706433/` is loopback to every
  resolver but `parse::<IpAddr>()` rejects it, so it is allowed.
- **DNS names that resolve privately** — `127.0.0.1.nip.io`,
  `metadata.google.internal`, or any attacker-controlled A record pointing at
  169.254.169.254. The check never resolves the name.
- **IPv4-mapped IPv6** — `[::ffff:127.0.0.1]` parses as `V6`, and
  `Ipv6Addr::is_loopback()` is false for the mapped form.
- **Redirects** — `content_store/http.rs:36-45` builds the client with a
  timeout and no `redirect::Policy`, so reqwest's default follows up to 10
  hops. A public URL that 302s to `http://169.254.169.254/…` is never
  re-checked.

**Fix**: parse with the `url` crate rather than by splitting; reject any URL
with userinfo; resolve the host and check *every* resolved address against the
private/reserved ranges (including `to_ipv4_mapped()`); set
`redirect::Policy::custom(...)` that re-runs the same check on each hop, or
`Policy::none()`.

---

### P2-M6: Known-vulnerable dependencies

`cargo audit` and `npm audit --omit=dev`, run 2026-08-18.

| Advisory | Crate/pkg | Severity | Relevance |
|----------|-----------|----------|-----------|
| GHSA-55q2-fjhq-7xh7 | `dompurify` ≤3.4.12 | moderate | **The sanitizer Pass 1 `H-5` relies on.** `IN_PLACE` hook removal leaves a detached subtree executable → XSS. Upgrade first. |
| RUSTSEC-2026-0235 | `rkyv` <0.8.17 | — | OOB read on archives containing `Rc`/`Arc`. |
| RUSTSEC-2026-0217 | `tract-nnef` | 6.1 med | Integer overflow → OOB read on model load. Models are `include_bytes!`-bundled (`sentinel/face_detect.rs:32`), so not remotely reachable today — but adversarial-prior federation is the direction of travel. |
| RUSTSEC-2026-0222 | `wasmtime` | 3.8 low | Type-index mixup between engines. |
| RUSTSEC-2026-0257 | `webbrowser` <1.2.2 | — | `BROWSER` argument injection on Unix. |
| GHSA-2v37-7h3g-55p8 | `nanoid` <3.3.18 | high | Infinite loop on zero-size generator. |
| RUSTSEC-2026-0253 / -0002 | `lru` 0.12.5 & 0.18.1 | unsound | Panic-safety UAF in `pop()`. `lru` is the dedup cache from Pass 1 `H-4`. |

Roughly 28 further `unmaintained` warnings (the GTK3 binding family, `paste`,
`bincode`, the `unic-*` family). Those are hygiene, not exposure.

**Fix**: `npm audit fix` for DOMPurify and nanoid; bump `rkyv`, `wasmtime`,
`webbrowser` and `tract`; add `cargo-deny` alongside the existing `cargo-audit`
CI step (Pass 1 `L-4`) with an explicit `deny.toml` so unmaintained-crate
warnings are triaged rather than accumulated.

---

## LOW

### P2-L1: `encrypted` and `key_id` are outside the signed canonical bytes

**File**: `src-tauri/src/p2p/signing.rs:28-40`, `p2p/types.rs:122-127`

`canonical_signed_bytes` covers `topic || timestamp || stake_address ||
payload`. `SignedGossipMessage` has two more fields:

```rust
#[serde(default)] pub encrypted: bool,
#[serde(default)] pub key_id: Option<String>,
```

Both are attacker-mutable without invalidating the signature. No consumer reads
either today — every producer writes `encrypted: false` and no code branches on
it — so this is latent rather than live. It becomes exploitable the moment
something selects a decryption key from `key_id` or branches on `encrypted`,
which is exactly what those fields exist for.

**Fix**: fold both into the canonical hash now, before a consumer appears.

---

### P2-L2: Dedup key is the payload hash alone

**File**: `src-tauri/src/p2p/validation.rs:215-231`

```rust
let hash = hex::encode(blake2b_256(&message.payload));
```

Two consequences: an identical payload published by two different authors, or
on two different topics, is dropped as a duplicate — so one peer can pre-seed
the cache to suppress another's message. And because the LRU holds 100k
entries, a peer that floods unique payloads evicts genuine entries and reopens
a replay window for anything still inside the ±5-minute freshness window.

**Fix**: key on `blake2b_256(topic || stake_address || timestamp || payload)`,
and give entries a TTL of twice the freshness window so eviction is driven by
age rather than by an attacker's fill rate.

---

### P2-L3: Guardian `ActivityPull` works before proving key possession

**File**: `src-tauri/src/p2p/guardian.rs:~370-395`

`ActivityPull { link_id }` carries no sealed marker. The handler looks the key
up by `link_id`, then builds a full activity snapshot across all six
`GUARDIAN_SYNC_TABLES` and seals it. Confidentiality holds — only the real
guardian can open the reply — but any peer that learns a `link_id` can make the
ward serialise its entire synced dataset on demand. `Revoke` gets this right
(`sealed_marker` must open to `b"revoke:<link_id>"`); `ActivityPull` should
match it.

**Fix**: require a sealed marker on `ActivityPull` too, and rate-limit per link.

---

### P2-L4: GitHub Actions pinned by tag

**File**: `.github/workflows/*.yml`

`actions/checkout@v5`, `actions/setup-node@v6`, `swatinem/rust-cache@v2`,
`android-actions/setup-android@v4`, `reactivecircus/android-emulator-runner@v2`
and `dtolnay/rust-toolchain@stable` (`ci.yml:523`) are mutable references. The
release workflows hold `TAURI_SIGNING_PRIVATE_KEY` and the Apple signing
material, so a compromised action version reaches code-signing secrets.

Positives worth recording: triggers are `pull_request` (never
`pull_request_target`), `permissions: contents: read` is set at workflow level,
and no workflow interpolates attacker-controlled text (`github.head_ref`, PR
title/body) into a `run:` block.

**Fix**: pin every third-party action to a full commit SHA with the tag in a
trailing comment; enable Dependabot for `github-actions`.

---

## INFO (positive findings)

### P2-I1: The Wasmtime grader sandbox is correctly built

**File**: `src-tauri/src/plugins/wasm_runtime.rs`

`Linker::new(&self.engine)` with no host functions defined — the guest has zero
imports and therefore no syscall surface at all. `consume_fuel(true)` with a
per-grade budget makes termination independent of wall-clock. `StoreLimits`
caps memory. The config is pinned for determinism, and
`pulley_matches_native_fuel_and_score` asserts that the iOS interpreter and the
native JIT burn identical fuel and produce identical scores — a strong
regression guard. This design is sound; `P2-C1` is a flaw in what gets *loaded*
into it, not in the sandbox itself.

### P2-I2: Device-sync and pairing authentication chains are sound

**Files**: `src-tauri/src/p2p/device_sync.rs`, `src-tauri/src/crypto/pairing.rs`,
`src-tauri/src/p2p/sync.rs:1035-1061`

The documented four-step chain holds up: the `PeerId` is Noise-authenticated,
the shared key is looked up per peer, the payload is AES-256-GCM sealed so a
non-paired peer can neither forge nor read, and same-user is checked
separately. Pairing codes carry a 256-bit `OsRng` key, are stored only as
`blake2b_256` of the code string, are single-use (`take_pending_pairing`
deletes unconditionally) and expire. `copy_tree` skipping symlinks
(`registry.rs:729-735`) is the right call too.

### P2-I3: Pass 1 remediations re-verified

Spot-checked against current source: `crypto/keystore.rs:298-312` uses Argon2id
at 64 MB / 3 iterations / 4 lanes (`C-1`); `p2p/signing.rs:28-40` signs a
canonical hash over every envelope field, with tamper tests per field (`H-1`);
`p2p/validation.rs:82,215-231` uses a capacity-bounded `LruCache` with no
full-clear (`H-4`); `tauri.conf.json` carries a restrictive CSP with a
nonce-based `style-src` (`M-8`); the updater `pubkey` is a real minisign key
(`H-6`). The `stake_pubkey_registry` identity gate (`H-3`) is present and
fails *closed* when a privileged-topic message arrives with no active profile
DB — a good default, and test-enforced at
`validation.rs:584-606`.

---

## Pass 2 remediation priority

| # | Finding | Effort | Why first |
|---|---------|--------|-----------|
| 1 | P2-C1: strip `.cwasm` from bundles at install | Low | Sandbox escape → host RCE. Three-line fix. |
| 2 | P2-M6 (DOMPurify): `npm audit fix` | Low | The sanitizer behind the `H-5` XSS-to-wallet chain is itself bypassable. |
| 3 | P2-H1: authenticate `vc-status` documents | Medium | Forged un-revocation silently restores revoked credentials network-wide. |
| 4 | P2-H2: bind `vc-fetch` requestor to a proof or PeerId | Medium | Whole pull-path privacy model is currently decorative. |
| 5 | P2-H4: grant-based `allow`, origin-checked delegate | Low | Silent camera/mic on macOS, with minors in scope. |
| 6 | P2-H3: narrow the `fs` scope | Low | Vault readable from the frontend on Linux/Windows. |
| 7 | P2-M1: bind `vc-did` rotations to the signer | Medium | Key-rollback undoes a compromise-driven rotation. |
| 8 | P2-M3: validate `plugin_cid` charset | Low | One regex; closes a traversal and a CSP-injection at once. |
| 9 | P2-M4: verify PinBoard signatures | Low | Cheap now; becomes data loss once eviction consults the table. |
| 10 | P2-M5: rebuild the SSRF guard on `url` + resolution | Medium | Current guard is bypassed by four one-line payloads. |
| 11 | P2-M2: content-address the whole bundle | Medium | Attestations mean little while they cover only the manifest. |
| 12 | P2-L1: fold `encrypted`/`key_id` into the signature | Low | Fix before a consumer starts trusting them. |
| 13 | P2-L2, P2-L3, P2-L4 | Low | Hardening. |

---

## Not re-audited in Pass 2

Named so the next pass knows where the gaps are:

- `cardano/` — the transaction builders, `gov_onchain.rs`, `operator.rs`,
  treasury handling and the Plutus data encoders. Pass 1 covered `blockfrost.rs`
  only, and the on-chain governance bridge landed after it.
- `assessment/` and `aggregation/` — anti-gaming (§15) and independence
  weighting are adversarial by design and have never had a dedicated pass.
- `classroom/` — group-key distribution and the encrypted classroom topics.
- `sentinel/` — the integrity/proctoring pipeline beyond the model-loading
  surface, and the client-trust boundary Pass 1 recorded as `L-2`.
- `cli/` — `alex vault` and the credential subcommands.
- `crates/{live,iroh-moq,moq-media}` — the vendored media stack.
