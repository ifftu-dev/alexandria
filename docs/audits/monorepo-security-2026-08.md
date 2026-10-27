# Alexandria Monorepo — Security Audit Index

Status: implemented
Updated: 2026-10-27

**Date**: 2026-08-18
**Scope**: every Alexandria repository.

Every finding in this audit has been fixed, or accepted with written reasoning,
in each repository. Per-repo remediation tables are at the top of each repo's
audit file; the combined summary is [below](#what-was-fixed).

This file is the entry point. Per-repository findings live with the code they
describe; this document holds the index, the cross-repository themes, the
audits of the two static web properties (too small to warrant their own files),
and the combined priority ordering.

---

## Where the findings are

| Repository | Audit | Pass | Result |
|------------|-------|------|--------|
| `alexandria/` | [`alexandria/docs/audits/security-2026.md`](security-2026.md) | Pass 1 (2026-02-24/03-23), **Pass 2 (2026-08-18)** | Pass 2: 1 critical, 4 high, 6 medium, 4 low |
| `alexandria-cloud/` | [`alexandria-cloud/docs/audits/security-2026-08.md`](https://github.com/ifftu-dev/alexandria-cloud/blob/main/docs/audits/security-2026-08.md) | First (2026-08-18) | 2 high, 5 medium, 4 low |
| `alexandria-relay/` | [`alexandria-relay/docs/audits/security-2026-08.md`](https://github.com/ifftu-dev/alexandria-relay/blob/main/docs/audits/security-2026-08.md) | First (2026-08-18) | **1 critical**, 1 high, 3 medium, 3 low |
| `alexandria-monitoring/` | [`alexandria-monitoring/docs/audits/security-2026-08.md`](https://github.com/ifftu-dev/alexandria-monitoring/blob/main/docs/audits/security-2026-08.md) | First (2026-08-18) | 2 high, 3 medium, 3 low |
| `website/` | § below | First (2026-08-18) | 2 medium, 2 low |
| `ifftu` (site) | § below | First (2026-08-18) | 1 medium, 1 low |

---

## The three findings to fix first

Across every repository, in order:

### 1. `alexandria-relay` `AR-C1` — CRITICAL — public `GET /peers` deanonymises every user

An unauthenticated, plaintext HTTP endpoint on the public internet returns
`peer_id → source_ip` for every connected peer. `src/main.rs:1009-1027`,
published by `fly.toml:40-53`. The fix is to unmount one route. Nothing else in
this audit is both this severe and this cheap.

Compounded by `alexandria-monitoring` `AM-H1` (dashboard fails open) and
`AM-H2` (peer IPs shipped to `ipapi.co`/`ipwho.is`): together these three turn a
PeerId observed in gossip into a city, and in the relay's case an exact IP.

### 2. `alexandria` `P2-C1` — CRITICAL — plugin sandbox escape

A community plugin bundle can ship a crafted `grader.*.cwasm`. Install copies it
verbatim, the "regenerate it locally" step is best-effort and silently skipped
when the shipped `grader.wasm` fails to compile, and the grader loader prefers
the on-disk `.cwasm` and `unsafe`-deserialises it — arbitrary native code in the
process that holds the unlocked vault. `plugins/registry.rs:145` +
`plugins/wasm_runtime.rs:380`.

### 3. `alexandria-cloud` `AC-H1` + `alexandria` `P2-M6` — dependency advisories on live paths

`quick-xml` and `lopdf` carry three CVSS-7.5 advisories and sit behind an upload
endpoint that parses before it authenticates. `quinn-proto` carries a CVSS-8.7
remote DoS on the relay's QUIC listener. DOMPurify — the sanitizer the app's
original XSS-to-wallet-theft finding was closed with — has a published bypass.
All four are lockfile bumps.

---

## What was fixed

One remediation commit per repository:

| Repo | Commit | Covers |
|------|--------|--------|
| `alexandria` | `2710b0f` | P2-C1 … P2-L4, all 15 |
| `alexandria-cloud` | `cd78122` | AC-H1 … AC-L4, all 11 |
| `alexandria-relay` | `fadd771` | AR-C1 … AR-L3, all 8 |
| `alexandria-monitoring` | `80566f6` | AM-H1 … AM-L3, all 8 |
| `website` | `a4a559c` | W-M1, W-M2, W-L1, W-L2 |
| `ifftu` | `a470cb2` | W-M3, W-L3 |

**Both criticals are closed.** `AR-C1` — `/peers` now requires a bearer token
*and* returns a truncated network prefix rather than a peer's address, so
neither control is load-bearing alone. `P2-C1` — three independent layers:
bundle `.cwasm` files are purged at install, purged again on every precompile
failure, and the loader refuses to deserialize any artifact without a digest
this machine wrote.

**Verification.** 1403 tests pass in `alexandria` (75 of them new), 32 suites
in `alexandria-cloud`, 11 in `alexandria-relay`, 6 in `alexandria-monitoring`.
`cargo fmt --check`, `cargo clippy -D warnings` and `vue-tsc` are clean in
every Rust/TS repo. `cargo audit` and `npm audit` report **zero** across all
six.

**Two findings changed on contact with the code**, and both are worth reading
rather than skimming:

- **The `tract-nnef` advisory was understated.** This audit said it was "not
  remotely reachable today" because Sentinel models are `include_bytes!`
  -bundled. That was wrong: `sentinel_load_dao_classifier` accepts ONNX bytes
  the frontend fetched over the network, and its own doc comment deferred
  verification to the caller. The upgrade is blocked by a dependency conflict
  (tract pins `half =2.4.1`; the media stack needs `^2.5`), so the fix is at
  the call site — the command now refuses any blob whose BLAKE3 is not a
  ratified `sentinel_priors.cid`.
- **`AM-M1` was simply wrong.** `observer/Cargo.lock` exists; the crate has no
  root manifest, so `cargo audit` just has to run from `observer/`. Run there
  it found nine vulnerabilities, which are now fixed or documented.

**Written down as still open**, deliberately:

- `AM-M2` — per-attempt throttling on the monitoring dashboard's basic auth.
  The credential is now mandatory with a 16-character minimum, but throttling
  means replacing `ValidateRequestHeaderLayer`, which has no hook for it.
- `script-src 'unsafe-inline'` on both static sites. Nuxt and Astro emit
  inline scripts and a static build has no nonce; removing it needs build-side
  CSP hash support rather than a header edit. Shipping a policy that blanks
  the site would have been worse than shipping an honest one.
- Fifteen dependency advisories accepted across four `.cargo/audit.toml`
  files. Each names why it is not exploitable in that repo and what would make
  it exploitable again — including one, `quick-xml`, that is *accepted* in
  `alexandria` and was *fixed* in `alexandria-cloud`, because in one it parses
  build-time protocol definitions and in the other it parses uploaded `.docx`
  files.
- `clippy::items_after_test_module` in `cli/src/tui/app.rs` — pre-existing,
  not a security issue, surfaced only because this pass ran clippy with
  `--all-targets` where CI does not. An attempt to fix it split a doc comment
  from its function and was reverted rather than left half-done.

Everything under each repo's "Not covered in this pass" section remains
un-audited; that list is unchanged.

---

## Follow-up pass — 2026-08-20

Re-assessed on `main` after the audit branches merged. Everything from the
original pass is intact: all 15 regression tests present, `cargo audit` clean in
all four Rust crates, `npm audit` clean in all five JS projects, all 66 CI
action refs still SHA-pinned, both audit jobs still running.

Two things came out of it.

### Do NOT upgrade `arrayref` past 0.3.9

`arrayref 0.3.9` is **yanked**, which normally reads as "move to the next
version". Here it is the opposite: **0.3.10 is the one to avoid.**

From the crates.io index (`~/.cargo/registry/index/.../ar/ra/arrayref`),
comparing every 0.3.x release:

| Version | Normal (runtime) dependencies |
|---------|-------------------------------|
| 0.3.0 – 0.3.9 | *none* — `quickcheck` only, and only as a dev-dependency |
| **0.3.10** | **`proc-macro1 ^1.0.107`** — a normal, runtime dependency |

`proc-macro1` does not exist on crates.io. The real crate is `proc-macro2`, and
`^1.0.107` mimics its version line. So a tiny, universally-depended-on utility
crate — `arrayref` reaches this monorepo through both `blake3` and `multiaddr`,
so it is in the build graph of the app, the cloud service, the relay and the
observer — suddenly acquired a runtime proc-macro dependency it has no reason
to have, under a typosquatted name. That is the shape of a compromised publish,
and the yank on 0.3.9 is consistent with the registry responding to it.

The practical effect today is that `cargo update -p arrayref` **fails to
resolve** (`error: no matching package named 'proc-macro1' found`) because the
typosquat is no longer fetchable. Cargo's fallback is worse than doing nothing:
in `alexandria-relay` and `alexandria-monitoring` a plain
`cargo update -p arrayref` silently *downgraded* to 0.3.4, and in `alexandria`
it dragged `ffmpeg-sys-next` 8.1.0 → 9.0.0 and `cc` 1.2.67 → 1.4.3 as
collateral. All four lockfiles were reverted; none of that is on `main`.

**Standing instruction: leave `arrayref` pinned at 0.3.9 in every lockfile.**
A yanked version still builds — the yank is advisory and the lockfile pin is
what governs. If a future `cargo update` moves it, revert that hunk. Revisit
only if the maintainers publish a 0.3.11 whose dependency list is empty again.

### `AC-L4` was wrongly marked FIXED

`alexandria-cloud/docs/security-audit.md` claimed `ttf-parser` left the tree
with the `pdf-extract` 0.12 upgrade. It did not —
`pdf-extract 0.12.0 → lopdf 0.42.0 → ttf-parser 0.25.1` is still there. The
severity is unchanged (unmaintained warning, not a vulnerability, and only
reachable by an authenticated caller after `AC-H1`), but the status line was
false and is now corrected to ACCEPTED with the reason.

### The one code change since the audit is a good one

`AC-M4`, the replay defence added in the audit, had a real defect: Ed25519 is
deterministic, so two *honest* requests to the same path in the same second
signed to identical bytes and the seen-proof set refused the second as a
replay. That broke evidence release sent in batches, and any retry. The fix
adds a required per-request nonce inside the signed challenge
(`method\npath\ntimestamp\nnonce`), correctly: the nonce is mandatory rather
than optional, it is covered by the signature so it cannot be swapped to dodge
the seen-set, cheap checks still run before expensive ones, and all three
client call sites were updated with no stragglers on the old format.

---

## Pass 3 — 2026-08-20, un-audited surface

The first two passes re-verified the same surfaces. This one went into what
they explicitly left out: `cardano/`, `assessment/`, `aggregation/`,
`classroom/`, `sentinel/`, `cli/` and the full 385-command IPC layer in
`alexandria`, plus the retention, growth and outbound-request paths in the
three servers. Priority order per request: alexandria, relay, monitoring,
cloud.

**Summary**: 0 critical, 1 high, 4 medium, 3 low. Baseline unchanged:

### Remediation status (2026-08-29)

| Finding | Status | Where |
|---------|--------|-------|
| P3-H1 | **FIXED** | `alexandria` `697c5a8` — `KeyDistribution` gated on `sender_is_moderator`; send path refuses instead of falling back to plaintext |
| P3-M1 | **FIXED** | `alexandria-cloud` `4e489ce` — new `egress` module (lifted from `P2-M5`), `AppState.http` built with a per-hop redirect check, `set_endpoint` uses the guard |
| P3-M2 | **FIXED** | `alexandria-relay` `e1537bf` — per-IP 30/60s window on `/username/:name`, bounded tracked set, 3 tests |
| P3-M3 | **CODE FIXED, DEPLOY STAGED** | `alexandria` `697c5a8` fixes `pays_to` (output must carry the escrow input's full lovelace; 12/12 aiken tests). Script hash `ead373d2…` → `2f499b73…`. `f5cf5bf` stages the deploy: `cardano/governance/deploy_escrow_only.sh` deploys **only** this validator — the stock script would replace all nine, and a local `aiken build` (v1.1.23 vs the v1.1.21 that built what is on-chain) changes every hash, so `plutus.json` was spliced to carry only the escrow change. Dry-run verified against the treasury (710-byte script, 7 tADA lock). **To finish:** `cd cardano/governance && ./deploy_escrow_only.sh`, then `./scripts/apply_escrow_ref.sh <tx> <idx>`, commit. The broadcast signs with the treasury key and was left for a person to run — the session's permission classifier blocked it, correctly. |
| P3-M4 | **NOT IN SCOPE** | `alexandria-monitoring` — excluded from this remediation request. Still open. |
| P3-L1 | **FIXED** | `alexandria` `697c5a8` — migration 081 persists escrow pkhs at lock; settle derives the recipient, IPC param removed |
| P3-L2 | **FIXED** | `alexandria` `697c5a8` — `plugin_install_from_file` constrained to Downloads/Documents/Desktop |
| P3-L3 | **OPEN (architectural)** | unchanged since Pass 1 `L-2` |

Worktrees: `alexandria-p3-wt`, `alexandria-relay-p3-wt`, `alexandria-cloud-p3-wt`,
one commit each, unpushed. Verification: 1173 alexandria lib tests, 14 relay,
32 cloud suites, 12 aiken — all green; fmt + clippy clean in all three.

One thing surfaced while fixing L1 that was not in the audit:
`lock_challenge_stake` defaults `treasury_key_hash` to the **challenger's own
key** when the frontend omits it (`None => wallet.payment_key_hash`), so a
`Forfeit` on such a stake refunds the challenger. The new columns make it
visible; the default itself is left as-is because it is documented as
"solo operator / testing" and changing it is a product decision.
`cargo audit` zero in all four Rust crates, all Pass 1/2 invariants intact.

| # | Sev | Repo | Finding |
|---|-----|------|---------|
| P3-H1 | HIGH | alexandria | Classroom `KeyDistribution` gossip is unauthenticated — any peer overwrites a member's stored group key, silently downgrading their next message to plaintext |
| P3-M1 | MEDIUM | cloud | ATS webhook client follows redirects and never checks the resolved address — the `https://` prefix check is the whole SSRF guard |
| P3-M2 | MEDIUM | relay | `/username/:name` is a public, enumerable, unthrottled username→DID oracle |
| P3-M3 | MEDIUM | alexandria (on-chain) | `challenge_escrow.ak` `pays_to` checks recipient address only, not amount — settle can pay 1 lovelace and keep the rest |
| P3-M4 | MEDIUM | monitoring | Per-peer `device_labels` grows without bound from attacker-controlled `identify` strings |
| P3-L1 | LOW | alexandria | `settle_challenge_stake` takes `recipient_key_hash` from the frontend instead of reading it from the escrow datum |
| P3-L2 | LOW | alexandria | `plugin_install_from_file` accepts any directory path from the webview |
| P3-L3 | LOW | alexandria | Client-trusted integrity scores (Pass 1 `L-2`) still unchanged — restated so it is not lost |

### P3-H1 — classroom group-key substitution → silent plaintext downgrade

`classroom/manager.rs:346-363`. Every other `ClassroomMetaEvent` arm checks
`sender_is_member`; `KeyDistribution` checks only that the *target* is us:

```rust
ClassroomMetaEvent::KeyDistribution { classroom_id, stake_address, encrypted_group_key, key_version } => {
    if local_address.as_deref() != Some(stake_address.as_str()) { return; }
    db.conn().execute("INSERT OR REPLACE INTO classroom_group_keys ...")
```

No check that the sender is the instructor, a member, or anyone. Any peer on
the classroom meta topic can `INSERT OR REPLACE` our row for any classroom.

It is not key *substitution* in the cryptographic sense — the send path
(`commands/classroom.rs:1035-1060`) opens `group_key_enc` with the local
`content_crypto` key, so an attacker-supplied blob fails to decrypt. But look
at what happens on failure: `_ => (content.clone(), false, 0)`. **The message
is sent in plaintext.** So the attack is: overwrite the victim's stored key
with garbage, and every subsequent message they send to that classroom goes
out unencrypted, with `is_encrypted: false`, and nothing tells them.

Given the product context — classrooms with minors, a design note that says
group traffic is confidential — a one-message unauthenticated downgrade
primitive is the worst shape for this bug to take.

**Fix**: require `sender_is_member` on `KeyDistribution` like every sibling
arm, and additionally require that the sender hold the instructor role (only
the instructor rotates keys). Separately, make the send path **refuse** rather
than fall back when a group key exists but fails to open: a classroom that has
ever had a key must never silently emit plaintext.

### P3-M1 — ATS webhook SSRF via redirect

`http/bulk.rs:469` is the only check on a customer-supplied webhook URL:
`if !req.url.starts_with("https://")`. Delivery (`ats.rs:174,306`) then posts
through `AppState.http`, which `bin/server.rs:53` builds as
`reqwest::Client::new()` — default policy, follows up to ten redirects, no
address check on any hop.

So an org admin registers `https://attacker.example/hook`, which 302s to
`http://169.254.169.254/latest/meta-data/` or the Fly private network, and the
service delivers a signed payload of that org's candidates and verification
results to it. The tenancy boundary holds — an org only exfiltrates its *own*
data — but it turns the service into an internal-network probe from a
low-privilege authenticated position, and the payload is signed with the
org's hosted key, which lends it credibility wherever it lands.

**Fix**: the same guard `alexandria`'s content resolver now has — parse with
`url`, refuse userinfo, resolve and reject non-global addresses, and a custom
`redirect::Policy` that re-runs the check per hop. Build it once into
`AppState.http`. Pass 2 fixed exactly this bug in `alexandria` (`P2-M5`); the
fix should be lifted, not rewritten.

### P3-M2 — relay username oracle

`src/main.rs:1071`. `GET /username/:name` returns `{available, did,
received_at}` for any name, with no auth and — unlike the p2p receipt path,
which got a per-PeerId budget in Pass 2 — no HTTP-side limiter of any kind
(no `tower` rate/concurrency layer on the router). The namespace is
`[a-z0-9_]{3,32}`, so the whole registry is enumerable, and each hit is a
SQLite read.

Two consequences: a complete username→DID→first-seen-time directory of every
user for the cost of a loop, and a free CPU/IO amplifier on the same listener
the relay's real work shares.

Left public in Pass 2 on the reasoning that clients resolve names against it.
That is right; *unthrottled* is not. **Fix**: `tower::limit::RateLimitLayer`
or a per-IP window on this route; consider whether `received_at` needs to be
public at all.

### P3-M3 — escrow validator checks address, not amount

`cardano/governance/validators/challenge_escrow.ak:45-55`:

```aiken
fn pays_to(tx: Transaction, pkh: VerificationKeyHash) -> Bool {
  list.any(tx.outputs, fn(o) { ... VerificationKey(h) -> h == pkh ... })
}
```

`Refund -> authorized && pays_to(tx, d.challenger)`. Any output to the
challenger's address satisfies it — 1 lovelace does. The `authorized` signer
(the operator) can therefore settle a `Refund` paying the challenger a dust
output and route the rest of the 5 ADA stake to themselves, and the ledger
accepts it. Same for `Forfeit` and the treasury.

Today the operator is trusted, so this is an integrity gap rather than a live
theft — but the validator is the thing that is supposed to make the operator
*not need* to be trusted, and it currently does not.

**Fix**: `pays_to` should check `value.lovelace_of(o.value) >= stake` where
`stake` is read from the escrow input's own value (or carried in the datum).
Requires a redeploy of the validator, so a new script hash.

### P3-M4 — unbounded per-peer growth in the observer

`observer/src/main.rs:668-678`. Every distinct `agent_version` string a peer
reports via `identify` is pushed onto `device_labels` with no cap. The string
is attacker-controlled and up to whatever libp2p allows per message. A peer
that rotates its agent string on every identify push grows its own entry
without limit, and the observer holds every peer it has ever seen for
`PEER_EVICTION_AGE_SECS`.

**Fix**: cap `device_labels` (8 is plenty for "multi-device installs sharing a
PeerId"), truncate each label to, say, 128 chars, and drop the oldest on
overflow.

### P3-L1 — `settle_challenge_stake` trusts the frontend's recipient

`commands/challenge.rs:227-283`. `recipient_key_hash` comes over IPC and is
only `parse_key_hash`'d. The on-chain validator (`P3-M3` above) rejects a
wrong recipient, so this is not fund theft — but the correct value is
deterministic from the escrow datum, and reading it there instead of trusting
the webview removes an entire class of "frontend compromised → tx built with
wrong destination" bugs. It also stops depending on the validator being the
*only* thing standing between a compromised UI and a misdirected settle.

### P3-L2 — `plugin_install_from_file` takes an arbitrary path

`commands/plugins.rs:33-46`. `directory: String` from the webview becomes
`PathBuf::from(&directory)` and is handed to `install_from_directory`. It is
rate-limited and the install path verifies the manifest signature, so a
hostile path cannot install a bundle the author did not sign. What it *can*
do is make the app read and stat any directory the process can, which is a
small disclosure primitive. Pair it with the narrowed `fs` capability from
`P2-H3` by validating the path sits under `$DOWNLOAD`/`$DOCUMENT`/`$DESKTOP`.

### P3-L3 — client-trusted integrity scores, restated

`commands/integrity.rs:211,367-380`. Unchanged since Pass 1 (`L-2`, deferred
as architectural). Restated here only so it does not quietly fall out of the
open list: `integrity_submit_snapshot` and `integrity_end_session` still write
whatever score the frontend sends.

### Verified clean this pass

- All dynamic SQL is placeholder-bound; the two `format!`-built statements
  interpolate a compile-time column literal and a checked table name.
- `export_mnemonic` requires vault-unlocked **and** password re-auth **and** is
  rate-limited — the right three gates.
- Holder `directory_url` is constrained to `https://` or loopback, and the
  loopback check is exact-host, not prefix.
- `plugin_read_asset_data_url` goes through the hardened `resolve_asset`.
- CLI vault reads the password from a file or an interactive prompt, never
  argv or env; the CLI's `Command::new` sites take fixed programs.
- `arrayref` still pinned at 0.3.9 everywhere (see the follow-up note above).

---

## Cross-repository themes

### Theme 1 — Trust is asserted in comments and not enforced in code

The pattern recurs across three repositories, always in the same shape: a
module header states a security property, and the code below it does not
implement that property.

| Where | The claim | The code |
|-------|-----------|----------|
| `p2p/vc_status.rs:1-7` | "Receivers verify the issuer ... to validate the inner signature against" | No inner signature is ever validated (`P2-H1`) |
| `plugins/registry.rs:157` | "Never trust a `.cwasm` a community bundle might ship" | The regeneration that would enforce it is best-effort (`P2-C1`) |
| `plugins/wasm_runtime.rs:371` | "`SAFETY`: `path` was written by `precompile_grader` on this machine" | The install path does not establish that invariant (`P2-C1`) |
| `p2p/vc_fetch.rs:1-5` | "Authority-respecting — a subject opts in per credential" | Authority is a self-asserted field in the request body (`P2-H2`) |
| `p2p/pinboard.rs` | Commitments carry `signature` and `public_key` | Neither is ever checked; local writes store the literal `"unsigned"` (`P2-M4`) |
| `observer/main.rs:2107` | The router variable is named `protected` | It is protected only if an env var happens to be set (`AM-H1`) |

The comments are unusually good throughout this codebase — they explain
*reasoning*, not mechanics, and several findings in this audit were found
precisely because a comment stated an invariant that could then be checked. The
lesson is not to write fewer of them. It is that a stated invariant should come
with the test that fails when it breaks. `alexandria`'s
`validation.rs:584-606` — a test named
`privileged_topic_with_unlocked_profile_fails_closed` that pins a fail-closed
decision — is the pattern to copy.

### Theme 2 — Opt-in security controls that fail open

`MONITOR_BASIC_AUTH` unset ⇒ dashboard public (`AM-H1`). `METRICS_TOKEN` unset
⇒ metrics off, which is the *right* direction and shows the team knows the
distinction. `MessageValidator::new()` fails open on the identity check where
`with_db` fails closed — deliberate, documented, and warned about per-message,
but still two constructors with opposite security postures.

Default to refusing. `alexandria-cloud/src/config.rs:112-175` is the model
already present in this monorepo: `DEV_AUTH` in a release build is refused, a
default `SESSION_SECRET` is refused, a short `METRICS_TOKEN` is refused, and
`DATABASE_URL == ADMIN_DATABASE_URL` is refused with an explanation of why it
would silently disable tenancy. Every service should start that way.

### Theme 3 — Authorisation on unauthenticated identifiers

`vc-fetch` authorises on a DID in the request body (`P2-H2`). `vc-did` and
`vc-status` act on a DID in the payload without relating it to the signer
(`P2-M1`, `P2-H1`). PinBoard trusts `pinner_did` (`P2-M4`). Guardian
`ActivityPull` acts on a `link_id` alone (`P2-L3`).

In every case the authenticated identity was available and unused — the libp2p
`PeerId` from the Noise handshake, or the envelope signature. The counterexample
is `device_sync.rs`, whose header sets out a four-step chain (transport-
authenticated PeerId → per-peer key lookup → AEAD → same-user check) and
implements all four. That module is the template.

### Theme 4 — Resource limits applied at one layer and not the next

`alexandria-relay` caps DHT records in memory at 16,384 and mirrors to disk with
no cap at all (`AR-M1`). `alexandria-cloud` caps the compressed upload at 12 MB
and decompresses without a limit (`AC-M1`). `vc-status` caps nothing on the
decoded bitmap (`P2-H1`). `key_registry` and `pinboard_observations` accept
unbounded unauthenticated inserts (`P2-M1`, `P2-M4`).

The connection-layer limiting in the relay (`AR-I1`) is genuinely thorough and
tuned against production incidents — which is what makes the persistence-layer
gaps stand out. When a limit is added, the question to ask is which layer
*behind* it is now the unbounded one.

### Theme 5 — Dependency scanning exists but is not acted on

`cargo-audit` is in `alexandria`'s CI (Pass 1 `L-4`, marked FIXED) and the tree
still carries four vulnerabilities and 28 warnings. `alexandria-monitoring` has
no root `Cargo.lock`, so it cannot be scanned at all (`AM-M1`). `npm audit`
finds issues in three of five JS projects.

Add `cargo-deny` with an explicit `deny.toml` alongside `cargo-audit`, so
unmaintained-crate warnings are triaged into "accepted, here is why" rather
than accumulating until nobody reads the output. Record the no-fix-available
acceptances (`rsa` RUSTSEC-2023-0071, `hickory-proto` RUSTSEC-2026-0118)
explicitly.

---

## Web properties

Both are static marketing sites. No authentication, no user data at rest, no
database — the attack surface is the response headers and, for `website`, two
Netlify functions.

### `website/` (Nuxt, Netlify)

**W-M1 — MEDIUM — no Content-Security-Policy or HSTS.**
`netlify.toml` sets `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`
and `Referrer-Policy: strict-origin-when-cross-origin` — a good start, and more
than `alexandria-cloud`'s console has. Missing: `Content-Security-Policy`,
`Strict-Transport-Security`, `Permissions-Policy`. The site carries deep links
into the desktop/mobile app (`/guardian/*`, `/course/*`, `/classroom/*`,
`/open`), so a script-injection here has a path toward app users.

*Fix*: add to the `for = "/*"` block:

```toml
Content-Security-Policy = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
Strict-Transport-Security = "max-age=31536000; includeSubDomains; preload"
Permissions-Policy = "camera=(), microphone=(), geolocation=(), interest-cohort=()"
```

**W-M2 — MEDIUM — the Netlify functions have no rate limiting or origin check.**
`netlify/functions/early-access.ts` and `enquiry.ts` accept anonymous `POST`
from any origin and each triggers outbound Plunk API calls — a contact upsert
and an email send. There is a honeypot (`botField`) and strict input
validation, but nothing bounds request volume. An attacker can burn the Plunk
quota, mail-bomb arbitrary addresses with the confirmation email, or pollute
the contact list.

*Fix*: rate-limit by IP at the edge; require an `Origin` matching the site;
consider a proof-of-work or CAPTCHA on the send path. Note that Plunk's
per-address deduplication limits mail-bombing somewhat but does not stop quota
exhaustion.

**W-L1 — LOW — control characters are not stripped from the email subject.**
`netlify/functions/enquiry.ts:149`:

```ts
subject: `${audience} enquiry — ${organisation || email}`,
```

`clean()` (`:42-44`) trims and truncates but does not strip `\r`/`\n`. The
value reaches Plunk as JSON rather than raw SMTP, so header injection depends
on Plunk's own handling — but the body of the same message is carefully
`escapeHtml`'d, and the subject should be treated with the same care.

*Fix*: `value.replace(/[\r\n\t\0]/g, ' ')` inside `clean()`.

**W-L2 — LOW — `nanoid` <3.3.18 and a `brace-expansion` advisory.**
Build-time only. `npm audit fix`.

**Positive**: input handling in both functions is genuinely careful — strict
allowlists for `role` and `platforms` rather than storing what was sent, a
regex plus a 254-character cap on the address, a honeypot that returns success
rather than an error, `escapeHtml` on every interpolated value in the
notification body, and `subscribed: false` on enquiries with the comment *"an
enquiry is not consent to be marketed at."*

### `ifftu` (Astro, Netlify)

The site lives in the `ifftu-dev/ifftu` repository.

**W-M3 — MEDIUM — same missing headers as W-M1.** `netlify.toml:47-54` sets the
same three headers and omits CSP, HSTS and Permissions-Policy. Same fix.

**W-L3 — LOW — 12 npm advisories, 10 high.** Chiefly `vite` 7.0.0–7.3.3
(GHSA-fx2h-pf6j-xcff, `server.fs.deny` bypass on Windows alternate paths) and
`launch-editor` (GHSA-v6wh-96g9-6wx3, NTLMv2 hash disclosure via UNC paths on
Windows). Both are dev-server issues, so production exposure is nil — but a
developer running `npm run dev` on Windows is exposed. `npm audit fix`.

---

## Combined remediation ordering

| # | Repo | Finding | Effort | Why here |
|---|------|---------|--------|----------|
| 1 | relay | `AR-C1` unmount public `/peers` | Trivial | Live deanonymisation of every user; one route |
| 2 | relay | `AR-H1` bump `quinn-proto` ≥0.11.15 | Trivial | CVSS 8.7 remote DoS on a public listener |
| 3 | alexandria | `P2-C1` strip `.cwasm` at install | Low | Sandbox escape to host RCE |
| 4 | cloud | `AC-H1` bump `quick-xml`/`lopdf`; auth before parse | Low | 3× CVSS 7.5 on an anonymous path |
| 5 | alexandria | `P2-M6` DOMPurify `npm audit fix` | Trivial | The sanitizer closing the XSS→wallet chain is bypassable |
| 6 | monitoring | `AM-H1` require auth at startup | Low | Network map is one env var from public |
| 7 | cloud | `AC-H2` add `Secure` to the session cookie | Trivial | Session capture = cross-tenant read |
| 8 | alexandria | `P2-H1` authenticate `vc-status` documents | Medium | Forged un-revocation restores revoked credentials |
| 9 | alexandria | `P2-H2` bind `vc-fetch` requestor to a proof | Medium | Pull-path privacy model is bypassable |
| 10 | alexandria | `P2-H4` grant-based `allow`; origin-checked delegate | Low | Silent macOS camera/mic, minors in scope |
| 11 | monitoring | `AM-H2` local GeoIP or IP truncation | Medium | Third-party transfer of user IPs |
| 12 | alexandria | `P2-H3` narrow the `fs` capability scope | Low | Vault readable from frontend on Linux/Windows |
| 13 | relay | `AR-M1` cap the SQLite DHT mirror | Low | ~1 GB disk per 16k requests |
| 14 | cloud | `AC-M1` cap decompressed docx | Low | Zip-bomb OOM |
| 15 | cloud | `AC-M2` + web `W-M1`/`W-M3` security headers | Low | One change of shape, three places |
| 16 | monitoring | `AM-M1` commit `Cargo.lock` | Trivial | Makes the rest of its dependency risk visible |
| — | all | Remaining medium/low findings | — | See per-repo priority tables |

---

## Coverage and gaps

**Audited this pass**: all Rust and TypeScript source in `alexandria-cloud`,
`alexandria-relay`, `alexandria-monitoring`, `website` and `ifftu`; in
`alexandria`, the VC layer, plugin system, Tauri configuration, capability set,
macOS webview delegates, gossip signing/validation/registry, guardian and
device-sync protocols, content resolver, and CI workflows; dependency trees
across every repository (`cargo audit` on three of four Rust repos, `npm audit`
on five JS projects).

**Verified as non-issues**: no secrets are tracked in git —
`alexandria/keys/`, `alexandria/src-tauri/.env` and `alexandria-cloud/.env` are
all `.gitignore`d and untracked, confirmed with `git check-ignore` and
`git ls-files`. No hardcoded credentials matched a secret-pattern sweep across
every repository. No workflow uses `pull_request_target` or interpolates
attacker-controlled text into a `run:` block.

**Known gaps**, carried forward:

- `alexandria/src-tauri/src/cardano/` — transaction builders, `gov_onchain.rs`,
  `operator.rs`, treasury handling, Plutus data encoders. Never audited; the
  on-chain governance bridge landed after Pass 1.
- `alexandria/src-tauri/src/{assessment,aggregation}/` — anti-gaming (§15) and
  independence weighting are adversarial by design and have had no pass.
- `alexandria/src-tauri/src/sentinel/` — the proctoring pipeline beyond model
  loading, and the client-trust boundary recorded as Pass 1 `L-2`.
- `alexandria/src-tauri/src/classroom/` — group-key distribution.
- `alexandria/cli/` — `alex vault` and the credential subcommands.
- `alexandria/crates/{live,iroh-moq,moq-media}` — the vendored media stack.
- `alexandria-cloud/src/{billing,invoice,subscriptions}.rs` — metering
  integrity, and `src/ats.rs` outbound webhook SSRF.
- Retention policy for peer IPs and geolocation in `alexandria-monitoring`.
- Whether a malicious relay can influence client relay selection — needs the
  relay and `alexandria`'s `relay_discovery`/`registry_chain` read together.
