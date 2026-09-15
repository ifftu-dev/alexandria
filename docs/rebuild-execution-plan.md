# Alexandria rebuild: execution plan

Prepared: 2026-09-15. Audience: an implementing coding agent working in short, independently verifiable sessions.

**Status: ready for implementation handoff, with deployment inputs intentionally pending.** Policy-approved issuers and demo organisation hosted custody are approved. The OIDC provider is intentionally unselected; the infrastructure budget remains pending. This document incorporates the existing worktree review. Writing it has not changed application code, provisioned infrastructure, or activated a protocol.

Navigation:

- [Starting evidence](#2-starting-evidence-preserve-then-refresh) and [decision register](#3-decision-register-and-genuine-blockers)
- [Architecture contracts](#4-architecture-contracts-implement-these-once) and [dependency order](#5-package-map-and-dependency-order)
- [Foundation packages](#6-m0--preserve-and-finish-the-foundation) and [trust/deletion packages](#7-m1--authentic-claims-and-deletion)
- [Integration, headless and restore packages](#8-m2--network-cloud-headless-execution-and-restore)
- [Hosted world](#9-m3--the-smallest-real-hosted-world) and [committee implementation](#10-m4--committee-consensus-and-independently-verifiable-outcomes)
- [Full world and platform verification](#11-m5--complete-the-world-and-verify-all-platforms)
- [Verification commands](#13-commands-and-verification-gates) and [small-model handoff templates](#15-handoff-protocol-for-small-model-execution)

## 1. Read this first

### 1.1 Outcome

Deliver an Alexandria preprod environment in which a fresh installation can discover real hosted courses, learn offline, obtain authentic credentials, optionally anchor evidence on Cardano preprod, exchange explicitly released evidence with Alexandria Cloud, restore history to another device, and participate in explicitly pinned committee governance. The same build and world must be demonstrable on macOS, Windows, Linux, iOS, and Android.

Retain the assessment-remediation foundation. Delete obsolete authority implementations and fabricated product seeds. Build the smallest complete hosted journey before expanding the persona population. Keep security properties explicit at every API, storage, and network boundary.

This is an execution plan, not a claim that the proposed consensus integration or deployment has already been proved safe. The committee protocol has a mandatory technical design-and-proof package before implementation may depend on its certificate formats.

### 1.2 Rules for the implementing agent

1. Read the repository's applicable `AGENTS.md` before editing. Resolve conflicting instructions using the current user conversation, not an old planning document.
2. Work on one package below at a time. Read its dependencies, invariants, tests, and completion condition before changing code. Split a package into smaller commits when it crosses several independent boundaries.
3. Inspect current files and Git state. Paths and function names here are entry points, not permission to assume old line numbers or an unchanged checkout.
4. Preserve other work. Never run blanket `git reset --hard`, `git clean`, mass checkout/revert, or broad staging over a dirty tree. Do not overwrite a paused patch, archive, key, deployment configuration, or a colleague's changes.
5. Do not implement an alternative authorization model to make a test pass. Do not lower quorum, bypass a proof, enable fake seeds, treat a provider acknowledgement as confirmation, or accept unsigned data as authority.
6. Missing evidence, invalid evidence, unavailable services, and database failure must be distinct outcomes. A failure must not become an empty list, `available: true`, `trusted: true`, or a fabricated success.
7. Use the current bounded database executor and profile leases for application SQL. Never hold a database lock across network, filesystem, native-media, or other async waits. Do not replace SQLite's exclusive connection access with `RwLock<Connection>`.
8. All Rust must pass formatting and strict Clippy. All Vue/TypeScript must pass the existing strict typecheck. No `any`, suppression comments, new lint ignores, or blanket dependency upgrades to conceal failures.
9. Product text uses the existing localization system and design system. Do not show implementation jargon or cryptographic success labels that overstate the user's assurance.
10. Public/shared Rust verification belongs in `alexandria-verify`; it must remain free of application storage, network clients, Tauri, and platform-media dependencies.
11. Local hermetic tests may use controlled peers, a fake chain transport, and a test OIDC issuer. Hosted demos must use the real services and real OIDC. Test fixtures must not become production bypasses.
12. Assess documentation after each code package. The request authorizes creation of this plan. It does not automatically authorize unrelated README, protocol, security-audit, or AGENTS changes; prepare their exact changes and obtain the approval required by AGENTS when needed. Do not ask again for a documentation scope the user subsequently authorizes.
13. Run the relevant focused checks during development and the milestone gate before claiming completion. Record commands, exit codes, skips, platform/features, and source revisions. An ignored test is not a pass.
14. Do not claim a phone was verified because a host build or cross-target lint passed. Do not claim a hosted flow passed because the client talked to a mock.
15. The plan describes separable work streams, not authorization to spawn agents. Follow the session's delegation rules. A single smaller model should execute sequentially.
16. Secrets belong in protected files, a secret manager, or the deployment secret store. Never print mnemonics, service tokens, database credentials, treasury keys, or private validator state into Git, test logs, screenshots, or the handoff report.
17. When blocked, identify the exact package, missing fact, evidence, and dependent work. Continue unrelated authorized work. Do not repeatedly ask for preferences that the user already selected.

### 1.3 Repository layout and path notation

Workspace root: `/Users/hack/Documents/Personal/Code/alexandria-mark3`.

These are independent Git repositories. There is no root monorepo commit that atomically contains all changes.

| Alias used below | Starting directory | Role |
| --- | --- | --- |
| `APP` | `worktrees/assessment-remediation/` | Main app, CLI, verifier, in-tree media crates; primary implementation worktree |
| `CLOUD` | `worktrees/alexandria-cloud-ux/` | Cloud worktree containing unfinished backend and console changes; preserve and assess before building on it |
| `CLOUD_MAIN` | `alexandria-cloud/` | Cloud main checkout; do not silently replace CLOUD's work with this baseline |
| `RELAY` | `alexandria-relay/` | Relay, username receipt registry, deployment configuration |
| `MONITOR` | `alexandria-monitoring/` | Observer and monitoring web application |

For example, `APP/src-tauri/src/cardano/submission.rs` means that path inside the app worktree. Existing `crates/live`, `crates/iroh-moq`, and `crates/moq-media` are in APP. Do not modify the separate old `iroh-live-patched` repository on the assumption it is still the active app dependency.

In package read lists, app Rust module paths such as `cardano/submission.rs` are relative to `APP/src-tauri/src/`; app frontend paths are relative to `APP/src/`. Service Rust module paths are relative to that service's `src/` (the observer uses `MONITOR/observer/src/`). Manifests, workflows and scripts are relative to the stated repository root. Braced lists are reading shorthand, not literal filenames.

Use isolated service worktrees/branches when beginning service edits. Record the selected paths. Never create a second checkout of a dirty tree and pretend the dirty changes came along automatically.

## 2. Starting evidence: preserve, then refresh

### 2.1 Reviewed source state

| Surface | Reviewed revision/state |
| --- | --- |
| App main/base | `e4e92484cc6ad126a60a8102a06d70589ff37253` |
| App remediation HEAD | `e8cf7b2369208d0c0b289c14d349046023b2780d`, branch `plan/assessment-remediation`, 20 commits beyond base |
| App dirty work | 30 modified tracked files; new `src-tauri/src/cardano/test_chain.rs` and `src-tauri/src/p2p/inbound.rs` |
| Relay | `9c9ec65c11af275872435e77ff06608f27fba140` |
| Monitoring | `c0edcf1a589b304671fa5ea506c871567108eb97` |
| Cloud main / cloud-UX base | `ee2c4f89cc79d63c24b64de551cc7458394337c0`; cloud-UX has substantial uncommitted work |

The app's committed diff contained 284 changed files. Counts describe review scope, not quality or a target for additional churn.

Paused artifacts were found at:

```text
/private/tmp/claude-501/-Users-hack-Documents-Personal-Code-alexandria-mark3/
  4d85ed8f-c324-46f4-b939-94fe7f19cd9e/scratchpad/paused-1224/
    uncommitted.patch
    untracked.tgz
    d5-cleanup-proposal.md
```

The inventory also exists in the parent `scratchpad/`. Locate these again if the path changed. `/private/tmp` is not durable storage. Do not apply the patch blindly: the working tree already contains paused work.

### 2.2 Review-time checks

The current app worktree, including dirty changes, passed:

- `cargo +1.91.0 fmt --all -- --check`
- `cargo +1.91.0 clippy --workspace --all-targets --locked -- -D warnings`
- `cargo +1.91.0 test --workspace --locked`: 1,756 passed, zero failed, 16 ignored, no `SKIP` diagnostics found
- `npx vue-tsc -b --noEmit`
- `npm test`: 182 passed
- `node scripts/check-tauri-commands.mjs`: 365 registered commands accounted for
- `npm run build`
- `git diff --check`

Patched dependency warnings and frontend bundle warnings remained. These checks did not verify iOS/Android devices, Windows/Linux runtime behavior, real hosted services, or the future committee. They must be refreshed after changes. Do not spend a session rerunning unchanged checks simply to restate this historical result.

### 2.3 Foundation to keep

- `db/executor.rs`: one bounded executor; 16/8/8 waiting slots; 8:4:1 learner/instructor/background fairness.
- `profile/{operations,scope}.rs`, lifecycle and worker tests: serialized transitions, admission closure, profile-session leases, cancellation ownership, drain before resource replacement.
- `cardano/submission.rs` and queue/recovery integrations: durable exact signed transaction identity and uncertain-outcome reconciliation.
- `content_store/http.rs`, `resolver.rs`, genesis retrieval: connection-time public-address filtering and bounded retrieval. The production proxy bypass still needs fixing.
- `crates/alexandria-verify/src/governance.rs` and app genesis adapters: canonical genesis, core-derived DAO ID, founding proofs, explicit preview/pin/export. Existing receipt/close shapes need replacement or adaptation after G02; they are not CometBFT proofs.
- Diagnostics and Sentinel lifecycle changes; classroom canonical-owner authorization and transactional updates; media FFI ownership improvements.
- Command-name generation and scope checks. **Argument/result typing is not complete**: `useLocalApi.ts` still exposes `invoke<T>(command, args?: Record<string, unknown>)`.

## 3. Decision register and genuine blockers

### 3.1 Carried-forward user decisions

These are requirements, not questions to ask again:

| ID | Requirement |
| --- | --- |
| U01 | Existing product/test data is disposable. Delete obsolete implementations and historical compatibility machinery; do not construct migration/repair systems to preserve fake authority. |
| U02 | macOS, Windows, Linux, iOS, and Android are in scope. |
| U03 | Hosted preprod demo; real Cardano preprod, treasury funding and Blockfrost; no mainnet transactions. |
| U04 | Cloud on Fly Machines with Neon Postgres 16, real OIDC; no hosted DEV_AUTH or mock identity provider. |
| U05 | CometBFT now, a deterministic Rust ABCI application, seven equal-power validators, five required for quorum. Intended version is CometBFT v0.40; compatibility must be demonstrated. |
| U06 | The initial demo committee may have one operator, visibly labelled `Demo committee — single operator`. This exception is limited to the demo and supersedes the older independent-host requirement for that demo only. Production independence is not implemented by seven keys on one account. |
| U07 | No reduced quorum, operator override, timeout finalization, or alternate authority. Fewer than five participating validators means no progress. |
| U08 | A vote is `Submitted` only with a verifiable timely receipt after log commitment. Five distinct member observations must meet the unchanged cutoff. Fifth-earliest valid member time is descriptive certified time. |
| U09 | Member clock-error bound is 30 seconds, with authenticated time sources and fail-closed signing when unhealthy. No cutoff grace period; warn about last-30-second submissions. Do not replace this with ordinary block-time semantics without a user decision. |
| U10 | Exact vote-specific signed eligibility/status evidence is public only after explicit consent. No unrelated history, imaginary redaction, or claim that network copies can be recalled. |
| U11 | Governance qualification uses a fixed, reviewable DAO policy. Historical eligibility is evaluated at certified submission, not silently against today's policy/status. |
| U12 | Immediate offline learner self-claims; genuine instructor endorsement is separate and requires the instructor's key. Optional chain witnesses must not block learning. |
| U13 | Lock hides private content immediately, shows cleanup progress, and admits no next unlock until cleanup succeeds. |
| U14 | Chain timeout/cancellation preserves original signed bytes and outcome-unknown. Reconcile by original identity; no automatic replacement or re-POST. Production Blockfrost connect/total request limits: 10/30 seconds. |
| U15 | Reputation snapshots are explicit as-of snapshots of eligible evidence, signed as DerivedCredentials and optionally anchored through the credential-hash path. Do not restore CIP-68 reputation-token minting. |
| U16 | Keep canonical genesis and multi-source content-addressed locators; discovery, import, retrieval, or matching names never auto-pin trust. |
| U17 | Delete credential challenges/escrow now; issuer-signed status lists supply revocation. Defer challenge reintroduction until a reviewed committee-certificate design. |
| U18 | Keep bundled taxonomy as labelled built-in data and built-in plugins trusted by exact bundled content identity. No fabricated personas/credentials/authority on profile unlock. |
| U19 | Thin real sponsor flow: signed role specification, local assessment, signed result, explicit evidence release, server verification. |
| U20 | Persona import uses mnemonic restore plus real device pairing/sync for history. A mnemonic alone is not a history backup. |

The attached rebuild plan supersedes earlier demands to preserve obsolete test data. It does **not** remove the need to preserve current source work or to reconcile already-submitted real preprod transactions before a destructive reset.

### 3.2 Decisions from this planning session and remaining questions

| ID | Question | Answer/status | Blocks |
| --- | --- | --- | --- |
| Q01 | Which non-governance credentials may grant privileges such as field-opinion posting? | **Approved:** policy-approved issuers grant privileges. Authentic credentials from other issuers remain visible at lower trust. | Resolved. Implement this rule in T02/T03. |
| Q02 | May the demo organisation opt into cloud custody of its organisation signing key? | **Approved:** hosted custody for the demo organisation. Preserve external/self-custody support. | Resolved. C04 uses the existing encrypted hosted-key mechanism with explicit opt-in. |
| Q03 | What monthly infrastructure cap and real OIDC provider/account should be used? | **Provider intentionally open:** user selected “Leave provider open for now” after reviewing free/open-source options. No spending cap has been supplied. Software licence fees can be zero; hosted compute/database/email are separate costs. | Billable deployment and final provider-specific hosted setup. Prepare a priced topology and actual provider options before requesting the remaining decision. Do not repeatedly ask during independent source work. |

Q01 and Q02 are settled; do not ask them again. Record the eventual Q03 selection before provider-specific deployment or billable provisioning. Do not ask the user to put secrets in chat; collect only provider/account identifiers and use protected configuration for credentials.

Free-software options verified on 2026-09-15: [Keycloak](https://www.keycloak.org/) provides standard OIDC and [authentik's free edition](https://goauthentik.io/pricing/) includes OIDC. Neither is selected. The provider-specific subsections below are conditional runbooks, not permission to choose one silently. For self-hosting, include a dedicated provider service and persistent database, version-pinned configuration export, protected administrator access, and real browser login in C03/O01. Do not replace it with a local mock for the hosted exit gate.

### 3.3 Implementation defaults proposed by this plan

These make the plan executable without reopening routine engineering choices. They are not claims of previously approved product policy:

- One immutable logical profile network: `preprod`. Signed objects also bind a protocol domain and, where applicable, a deployment/world or committee instance. Merely changing URLs is not network isolation.
- Keep source history; proposed app implementation branch `rebuild/foundation`, preserved WIP branch `wip/paused-1224`. Check for existing names and do not overwrite them. Merge using a normal reviewed merge; do not rewrite published history.
- Initial minimal governance supports yes/no proposals, fixed policy, and fixed validator membership. No abstention, runoff, election, epoch transition, or dynamic validator update is exposed. The demo policy proposes a minimum of five distinct eligible voters; encode it in the reviewed genesis/opening rather than hiding it in a constant.
- First effect types are plugin endorsement and taxonomy amendment. Every other old governance effect remains absent.
- The first hosted learner journey precedes the full world population. All planned personas and five platforms remain final requirements.
- Initial committee evidence is retained for the lifetime of its demo instance and exportable for offline verification. There is no pruning feature in this rebuild. Put storage costs into Q03; do not silently delete public proof evidence to meet a budget.
- World rerun acceptance means **no duplicate semantic effects and no repeated chain submissions**. Health checks, audit access logs, and refreshed transport metadata need not be literally write-free.
- Built-in corpus stays in the repository as content; author identities and signed published documents are generated at world build time.

### 3.4 Technical gates that cannot be replaced by guesses

G01/G02 must settle actual CometBFT/Rust compatibility, receipt-time evidence, proof heights, complete vote-set verification, and authenticated clock operations. If the chosen stack cannot express U08/U09 safely, report the concrete incompatibility and a bounded alternative for a user decision. Do not silently substitute a simpler timestamp or build a new consensus algorithm.

The exact OIDC provider, real treasury access, deployment account, signing credentials, and physical test devices are operational inputs. Source work can finish without them; their end-to-end gates cannot be marked passed without them.

## 4. Architecture contracts: implement these once

### 4.1 Trust is not signature validity

Use a shared evaluation boundary that returns evidence and reason codes, not a Boolean `trusted` flag derived from a database row. Suggested internal result categories:

```text
Invalid(reason)
Pending(missing_evidence)
VerifiedSelfClaim(verified_credential)
VerifiedCourseEndorsement(credential, exact_course_cid, verified_author)
PolicyQualified(credential, course_cid, policy_hash, issuer_binding, status_evidence)
```

Names can follow existing type conventions. Preserve the distinctions. Signature validity proves the issuer signed the object. Policy qualification additionally proves that this issuer and assessment evidence satisfy a specific policy for a specific action/scope/time.

Evaluation order:

1. Enforce byte/depth/list/numeric limits before expensive work.
2. Parse with the canonical format's duplicate-key and numeric rules. Validate domain/network/version.
3. Verify the credential proof and historical key binding. Match signed issuer, subject, skill, and course identity; do not trust indexed columns over signed content.
4. Verify issuer-signed status evidence and its applicable time/version. A missing or stale status source is not proof of non-revocation.
5. If course-backed, fetch/verify the exact signed course CID and the author's identity binding. Course title, mutable course ID alone, or HTTP host is insufficient.
6. Classify provenance. Subject/issuer inequality alone is not independence. Unknown issuers do not gain DAO authority by publishing a course.
7. Evaluate the action's exact policy. Record policy hash, scope, calculation version, and evidence identities used.

Local commands, remote ingest, aggregation, opinion eligibility, governance, cloud verification, and audits must use the same underlying proof rules. Policy choices can differ by action, but must be explicit and testable.

An accepted-issuer list is itself authority. T03 must define its versioned policy manifest, scope/action binding, authenticated source and explicit trust selection before wiring it into privilege checks. For the initial demo, use immutable, reviewed subject policies containing the exact demo instructor identities; bind their content digests in the selected network configuration. Governance instead uses the qualification policy bound to the explicitly pinned genesis/opening. Do not infer policy approval from a course author's signature, an IdP login, a mutable local issuer row, or an endpoint response. Missing applicable policy means no privilege and an actionable explanation; it does not hide authentic credentials. Policy replacement must be an explicit trust transition with cache invalidation, not silent background refresh. This is a scoped demo bootstrap, not a new universal issuer authority.

Caches are projections: key/invalidate them using credential identity, status version, course CID, policy hash, and calculation version. Removing the last eligible input must remove or mark the old cached result unavailable; never leave a stale high score because recomputation found zero rows.

### 4.2 Profile and database ownership

- Each sensitive operation owns a current profile lease for its whole resource lifetime.
- Database jobs own/clone their lease; stale queued jobs do not start.
- Lock closes admission before cleanup and cancels only futures whose owned resources are safe to drop.
- Blocking/native tasks must be owned and joined; abandoning their awaiting future is not shutdown.
- Drain leases and owned workers before replacing the database, vault, content key, blob directory or endpoints.
- Two-phase operations do short transactional reads/checkpoints, external work outside the executor, then fenced transactional projection. Bind the projection to the immutable input version originally read.
- Every retry after overload checks whether a durable operation already exists. Retrying must not allocate a new logical operation ID.

### 4.3 Network and service identity

Proposed `NetworkProfile` contents:

```text
schema_version, network_id, profile_revision,
cardano_network, cardano_network_magic,
relays[{peer_id, dns_name, port, fallback_ips, registry_https_origin}],
receipt_issuer_keys, stake_registry_founder_keys,
signed_bootstrap_registry_identity, subject_qualification_policy_digests,
cloud_https_origin, cloud_service_id,
governance_locator, committee_instance_id, committee_https_endpoints,
protocol_namespace, optional_governance_anchor_address
```

Do not put API keys or private keys in this file. Store immutable `network_id` in each profile. Changes to endpoints may be versioned discovery configuration; changes to trust roots, network, or committee instance require an explicit reviewed trust transition, not an endpoint refresh.

Represent unavailable optional services explicitly. The first learning world can have governance disabled while G01–G05 are unfinished; missing committee fields must disable governance, not prevent ordinary learning or introduce placeholder trust roots. Required fields are validated for each activated capability.

Reject signed data for another network/domain/committee instance even if the same demo keys are reused. Both libp2p topics and request/response protocols must be namespaced consistently across app, relay, observer, and headless peers. Do not dual-subscribe to legacy authority topics for compatibility. Do not restore retired evidence gossip.

Public app-to-service calls require HTTPS and bounded request/response behavior. A public IP fallback for libp2p dialing must not become an HTTPS certificate-validation bypass. Loopback-only test transport is a distinct test configuration.

### 4.4 Cloud proof-of-possession and release binding

Replace the duplicated newline challenge with one versioned shared canonical format. Proposed fields:

```text
domain = alexandria/cloud-request/v2
network_id, service_id, audience_origin, subject_did,
method, canonical_path_and_query,
issued_at, expires_at, nonce,
body_digest, content_type
```

Specify path escaping/query normalization once. Reject duplicate/ambiguous semantic fields. Either sign the exact transmitted body bytes or canonicalize once and transmit those same bytes; do not sign one serialization and send another. Hash the empty body for bodyless requests. Bound input length before decoding/hash work. The recipient validates its configured service identity and allowed origin; it must not use an attacker-supplied Host header to define the expected audience.

Replay spending uses an atomic unique insert into Postgres after proof verification. Retain each spent nonce/proof until its signed validity window has expired, including the accepted clock-skew margin. No eviction of still-live records. Database unavailability/capacity failure refuses the request. Do not release data while replay checking is unavailable.

Distinguish anti-replay from business idempotency. Mutations carry a stable operation ID and payload digest. A retried operation with a new proof but the same operation ID returns its previous result; a different payload under the same ID conflicts. Commit mutation and its idempotency outcome atomically. Authorization and tenant scope are rechecked on retries. A spent proof itself is never accepted twice.

An organisation role specification and a learner result are separate signed artifacts. A learner signature is not an organisation endorsement; an organisation signature does not prove a local device was untampered. Report the actual assurance achieved and require the specified evidence before granting higher assurance.

### 4.5 Data classifications for deletion and sync

Inventory each table/column using these categories before dropping anything:

| Category | Treatment |
| --- | --- |
| Authoritative signed artifact | Preserve its exact bytes in new workflows; verify before projection; sync through a typed verifier-backed adapter where allowed |
| Derived projection/cache | Recompute from verified source artifacts; never sync as authority |
| Local private learning state | Use explicitly allowed paired-device merges; no public gossip |
| Local key, consent, trust pin, device state, chain journal | Do not add to generic device-table replication |
| Built-in trusted content | Verify exact bundled identity and label provenance; do not fabricate external signatures |
| Obsolete/fake authority | Delete code, schema and fixtures after replacing tests with legitimate builders |

Current generic `SYNCABLE_TABLES` contains only `enrollments`, `element_progress`, and `course_notes`, with settings handled separately. Credential/history restoration is therefore new implementation work, not a documented capability that already exists.

### 4.6 Chain reset and demo identity

- Original journal identity and signed bytes remain encrypted and locally scoped. No generic cloud/gossip/device-sync transfer of the chain journal.
- The world controller may keep a protected offline recovery archive of its own persona journals for reset/recovery. This is a demo operator backup, not production cloud custody of learner wallets.
- Before destructive reset, stop writers, reconcile outstanding transactions, and archive recoverable state. If an outcome remains unknown, refuse destruction of its only recovery record.
- Deterministic keys do not make time-derived course IDs, fresh tx bodies, enrolments or cloud UUIDs idempotent. Persist stable logical IDs and exact signed artifact identities in the world ledger.
- Use one chain-writing owner per demo persona at a time. Exporting a learner for active device use puts the hosted copy into history-serving/read-only mode for chain and assessment mutations. Do not claim distributed wallet coordination has been implemented.
- A committee reset uses fresh instance/chain identity and signing state. Never restart a chain at height 1 with old validator keys and an old chain ID after deleting anti-double-sign state.

## 5. Package map and dependency order

`Ready` means dependencies and applicable answers exist. `Done` means code, tests, and recorded evidence meet the exit condition. Source completion and hosted/device verification are separate statuses.

A source dependency requires the consumed interface/behavior and its relevant local checks to be complete. It does not require a later deployment/device check that the consuming package itself enables. Record that distinction in the ledger. In particular: C03's tested OIDC source/configuration feeds O01, and O01's deployment enables C03's real browser gate; N02's local service tests feed O01, which enables N02's external port/persistence checks. These are staged gates, not circular reasons to stop. Neither package is finally verified until its remaining checks pass.

| Milestone | Packages | Exit |
| --- | --- | --- |
| M0 — recoverable foundation | F01–F04 | WIP safe, pending fixes integrated, trustworthy checks |
| M1 — authentic core | T01–T04, D01–D03 | No active legacy authority/fake startup seeds; shared proof rules; fresh baseline schema |
| M2 — integration contracts | N01–N03, C01–C04, H01–H03, X01–X02 | Real transport/auth/restore boundaries exercised locally; headless core usable |
| M3 — smallest hosted world | W01–W03, O01 | One real hosted learner/instructor/cloud/restore journey with audited artifacts |
| M4 — committee protocol and runtime | G01–G05 | Real committed receipt/close, complete offline verification, fault/restart tests |
| M5 — full demo and platforms | W04, G06, V01–V03 | All personas/effects and five-platform evidence on one build ID |
| M6 — review and integration | R01 | Scoped reviewed changes and authorized documentation; accurate completion record |

Scheduling:

- Start G01 immediately after F01, independent of app cleanup. G02 follows G01. Do not defer discovering a consensus incompatibility until hosting/UI work is complete.
- Complete F02/F03 before pruning overlapping journal/inbound code. Preserve all dirty work first.
- T01 closes immediate authority exposures; T02/T03 build the replacement semantics. D01/D02 remove obsolete code. D03 squashes the schema after these structures stabilize.
- N01 is a prerequisite for final network-bound signatures. C01/C02 may be developed against temporary test profiles before final hosted endpoint values exist.
- H01 starts after F02/F03 and the core trust interface is settled. H03 restoration is required for W03, not a late documentation chore.
- C03 hosted completion and O01 depend on Q03. C04 uses approved Q02 hosted custody; T03 implements approved Q01 policy-qualified privileges.
- W01/W02 build without governance activation. G03–G05 use a minimal separate committee fixture; they must not depend on the full persona world. W04/G06 connect the two once both pass.
- Shared files (`lib.rs`, `network.rs`, schema, verifier formats) get sequential changes. Even with authorized multiple agents, assign one owner per shared boundary and integrate at package boundaries.

For a single implementing model, begin with F01, then G01. If G01 is blocked on an external input or confirmed incompatibility, record it and continue F02 → F03 → F04 → T01 → N01's configuration contract → T02 → T03 → T04. Run G02 once G01/T04 are ready, then continue the ready deletion/integration packages. Do not implement speculative committee production code while its design gate is blocked. A session may complete only one slice of a large package; its ledger must identify the exact next slice.

## 6. M0 — preserve and finish the foundation

### F01 — Preserve snapshots and establish execution records

**Dependencies:** none. **Scope:** all five checkout paths; no application behavior change.

1. Record each repository's branch, full HEAD SHA, status, staged diff, unstaged diff, untracked-file list, submodule/worktree status, and current toolchain. Do not print secrets from untracked files.
2. Compare the paused archive to the current APP tree by file hashes/content before applying anything. Inspect tar entries for absolute paths or `..`; extract into a new protected temporary directory only if needed.
3. Create a durable WIP snapshot on a new non-conflicting branch. Include intended source/untracked Rust files and the cleanup inventory, not `node_modules`, build outputs, `.DS_Store`, keys, or arbitrary scratch contents. Inspect the exact staged file list and diff before committing. Preserve a checksum manifest for the original paused patch/archive in protected local storage.
4. Preserve CLOUD's dirty source separately. It includes OIDC, configuration, egress, routes and tenancy-related changes, not just visual work. Do not merge it automatically or discard its tests.
5. Establish implementation branches/worktrees and a release manifest recording every repository revision. APP may be renamed to `rebuild/foundation` after checking collisions. Existing WIP commits may be non-buildable but must be labelled accordingly.
6. Start a machine-readable progress ledger in an agreed task-artifact location, using section 15's schema. Keep logs outside tracked source unless their inclusion is explicitly intended. This plan's creation does not automatically permit unrelated documentation edits.

**Verify/exit:** every intended source change can be recovered from a durable Git object or checked backup; current working files are byte-identical to the captured snapshot; no secret staged; next package and repository ownership recorded. Never delete the paused originals as part of this package.

### F02 — Finish the journal executor and bounded provider requests

**Dependencies:** F01. **Read:** `cardano/{submission,blockfrost,test_chain,anchor_queue,completion_queue,completion_recovery,username_anchor}.rs`; `p2p/registry_chain.rs`; `profile/worker_tests.rs`; affected examples and integration tests.

1. Review and integrate the existing dirty `Journal { executor, lease, workload }` refactor; do not reimplement a second journal.
2. Preserve the distinction between a pre-send retryable executor refusal, a failed pre-send checkpoint, and an uncertain result after a committed checkpoint. The transport must not run if durable checkpointing failed.
3. Keep network I/O between short database jobs. Recovery projection and its applied marker commit together, with stale acknowledgements unable to downgrade confirmed/failed-on-chain state.
4. Apply 10-second connect and 30-second total Blockfrost limits through the shared client, covering every surviving production submission caller. Bound pagination/pass work and stop on profile closure.
5. Preserve original transaction identity before rebuilding anything on retry. Handle batch membership conflicts transactionally. Exclude obsolete governance/escrow behavior from later surviving paths, but do not lose dirty tests before F01.
6. Keep the controlled `test_chain.rs` transport test-only; no externally configured bypass that makes hosted chain confirmation synthetic.

**Required tests:** checkpoint disk failure produces zero POSTs; executor overload is retryable and produces zero POSTs; timeout after POST leaves outcome-unknown; repeated invocation preserves exact bytes and sends no second POST; late response after lock cannot mutate the next profile; restart resolves the original hash by GET; provider not-found/error does not authorize replacement; duplicate batch membership rolls back; actual ledger slot and script failure are preserved.

**Gate:** `G-APP-RUST` plus focused submission/worker tests. Exit includes a source inventory showing no surviving raw submission path bypasses the journal.

### F03 — Finish inbound executor dispatch and store-close recovery

**Dependencies:** F02. **Read:** dirty `p2p/{inbound,network,validation}.rs`, `commands/p2p.rs`, `classroom/manager.rs`, `content_store/node.rs`, `lib.rs`.

1. Integrate the existing bounded inbound dispatcher (gossip queue capacity 64), bounded request job set, and per-job profile lease. Do not start one unbounded task per message.
2. Keep decode/expensive proof work outside SQL where safe, then atomically apply state-dependent checks and changes. Bind the verified payload to the applied version.
3. Overload replies must be retryable busy where the protocol supports errors. Other protocols must not answer `NotFound`/`NotOwner` merely because the database was busy. Record drops/counters without logging private payloads.
4. Review the existing store-close retry. Confirm against the pinned iroh/irpc implementation which actor-channel closure errors prove the database was dropped. Accept only those errors; other failures keep cleanup unconfirmed.
5. Retry pending cleanup on the next lock/unlock attempt before repointing the store. Clearing the content key and hiding private views must not wait for a successful network close.
6. Track the dispatcher and request tasks in lifecycle ownership so shutdown cancels/joins them before profile replacement.

**Required tests:** stale-profile inbound write refused; bounded queue/job count under a burst; Tokio remains responsive while SQL is blocked; busy does not look like absence; metadata-owner authorization preserved; failed close refuses reuse; subsequent successful close releases the file lock and permits reopen; repeated cleanup is safe.

**Gate:** `G-APP-RUST`, `G-APP-WEB`, affected mobile compile checks. Message-recovery guarantees are completed in N03; do not describe this queue alone as reliable at-least-once delivery.

### F04 — Close proxy bypass and make CI gate the actual workspace

**Dependencies:** F01; serialize with other HTTP edits. **Read:** `content_store/{http,resolver}.rs`, `.github/workflows/ci.yml`, `scripts/check.sh`, existing command/scope guards.

1. Make both production guarded HTTP clients disable proxy discovery/configuration. The current production builder passes `false` while the test seam uses `true`; remove that behavior difference.
2. Keep public-IP filtering at actual connection-time DNS, reviewed-source HTTPS rules, redirect policy, streaming byte caps, cancellation and no-TLS-bypass behavior.
3. Add a regression exercising the production construction path with proxy settings isolated in a subprocess; avoid concurrent process-global environment mutation in tests. Assert the proxy is not used. Do not make a real private-network request to demonstrate SSRF.
4. Fix CI change detection: the workflow trigger lists `crates/**`, but its backend/security classifier does not currently classify those paths. Include all workspace crates and the new committee/world/core packages when introduced. Test a verifier-only change, media-only change, command-only change, frontend-only change, and future package changes against the classifier.
5. Ensure required CSP and audit checks are explicit. `scripts/check.sh` can skip audit when the tool is absent; a release gate must report that as unverified, not fully green.

**Gate:** focused SSRF/CI-classifier tests, `G-APP-RUST`, `G-APP-WEB`. M0 exit: all captured pending foundation work integrated or explicitly accounted for; no active proxy bypass; affected source changes actually trigger CI.

## 7. M1 — authentic claims and deletion

### T01 — Remove immediate challenge/plugin authority exposures

**Dependencies:** F01; overlapping dirty challenge/escrow work must first be preserved. **Read:** `commands/challenge.rs`, `evidence/challenge.rs`, `plugins/attestation.rs`, `commands/plugins.rs`, registered handlers, inbound plugin handling.

1. Delete challenge vote/resolve/expiry/escrow commands, routes, UI entry points, and production workers. Remove their registrations and frontend calls together. Delete builders/recovery modules only once no surviving path references them. D03 drops the obsolete tables.
2. Remove arbitrary `plugin_ingest_attestation` IPC and event-supplied-key authority. Until G06 is complete, only exact bundled plugin/grader identities confer built-in credential eligibility. Other plugins may remain installable under the product's existing sandbox rules but do not acquire a trusted endorsement label.
3. Verify pinned committee origin for any retained certificate parser; a parser existing in the library must not activate incoming legacy multisig events. No debug feature or diagnostics mode can restore authority.
4. Use issuer-signed status lists for revocation; remove all remaining paths where a local challenge result sets authoritative revocation. Inventory sync/import paths as well as commands.

**Required tests:** removed commands cannot be invoked; one local wallet cannot revoke another issuer's VC; five event-supplied keys cannot attest a plugin; repeated copies of one key at distinct indices cannot satisfy quorum; forged persisted attestation flags do not affect credential trust; built-in CID mismatch fails.

**Gate:** Rust/frontend checks and command/scope guards. This package must close exposure, not merely remove the menu.

### T02 — Shared verification and exact course-version endorsement

**Dependencies:** T01, N01 contract definition (final endpoint values unnecessary). **Read:** `domain/course_document.rs`, `commands/{courses,completion,attestation,auto_issuance,credentials}.rs`, VC proof modules, course ingestion and key-registry verification.

1. Implement section 4.1's shared verification result types in the smallest suitable pure module. Reuse the verifier's cryptography/canonicalization; do not add a second signature implementation.
2. Extend the author-signed course payload with completion requirements: exact attestor identities/authorized key bindings, distinct threshold, evidence requirements and format version. Validate threshold against unique permitted identities; reject duplicate or malformed identities and unbounded lists.
3. Bind enrolment/attempt/completion to exact course document CID/version. Author updates produce new signed content and cannot retroactively rewrite the assessed version or requirement.
4. Remove set/remove requirement IPC and the mutable authority table. Keep projections only if their source CID is verified and immutable. Query failure is not `required_attestors = 0`.
5. Bind each attestation to the network/domain, subject, exact course CID, completion/attempt evidence identity, and relevant witness identity if used. Do not accept an arbitrary signature over unrelated tx bytes as course completion endorsement.
6. Preserve immediate offline self-claims. No requirement means self-attested completion; even if a learner verifies an assessment locally, the app must not manufacture an instructor signature.
7. Build real instructor acquisition through authenticated addressed requests or an explicit sign/import workflow; a hosted instructor must verify the requested evidence and course binding before signing.

**Required tests:** unlisted attestor refused; malformed DID fails rather than bypassing consistency check; one attestor counts once; threshold bounds; altered subject/course/evidence/network fails; newer course requirements do not affect old completion; missing status/key binding is pending; offline self-claim succeeds without any network; instructor unavailable does not block local completion.

**Gate:** verifier tests, focused course/completion tests, `G-APP-RUST`, `G-APP-WEB`. Both locally-created and remotely-received artifacts traverse equivalent verification.

### T03 — Eligibility, scoring and snapshot semantics

**Dependencies:** T02; Q01 is approved and must be implemented. **Read:** `db/opinion_eligibility.rs`, `commands/{aggregation,reputation,evidence,opinions,talent_index,snapshot}.rs`, `evidence/reputation.rs`, `p2p/opinions.rs`, aggregation configuration and snapshot codecs.

1. Write the action-to-policy matrix before changing scoring: private progress, visible credential provenance, opinion posting, talent-index claims, governance eligibility, and role evidence. Implement the policy authority/manifest binding from section 4.1, with immutable scope/action/version, accepted issuer identities, applicable course/evidence rules and status freshness requirements. Keep new policy administration UI out of this package; no unauthenticated mutation command may edit accepted issuers.
2. Route local opinion authorization and inbound opinion acceptance through the same policy evaluator. Derive the actor from the verified signing identity; a request-supplied DID or somebody else's qualifying credential cannot authorize the action.
3. Verify proofs when loading aggregation/reputation inputs, not merely JSON shape or a `verified` column. Match indexed subject/skill to signed content. Exclude revoked/invalid input and correctly classify self/unknown-author claims.
4. Remove any independence confidence boost for self-issued claims. Do not drop private learning progress simply because it lacks external endorsement. Do not invent new numeric confidence weights; preserve existing approved math where meaningful and flag any unresolved policy-dependent adjustment.
5. Invalidate all downstream caches on source/status/policy changes, including the empty-evidence case. Freeze as-of input IDs, policy/calculation version and scores in a signed DerivedCredential for snapshots; use the existing credential-hash optional anchor path.
6. Delete historic public-derived issuer cleanup machinery only after new scoring no longer depends on its views/triggers. Rewrite its general adversarial tests with actual malicious/invalid fixtures; do not discard the security coverage with the obsolete schema.
7. Label offline verification as verification of supplied historical evidence. Do not claim knowledge of today's remote revocations while offline.

**Required tests:** bad-proof row cannot inflate score; unrelated subject cannot post; self-issued VC cannot satisfy the selected privileged policy; two controlled identities plus an arbitrary course do not bypass a policy-approved issuer requirement; policy-scoped accepted issuer succeeds; no cross-scope privilege; revoking/removing the last eligible VC clears cached qualification; direct and cached readers agree; snapshot signed inputs/version never change after creation.

**Gate:** focused aggregation/opinion/snapshot/antigaming tests, verifier tests, full app gates. Policy-qualified privilege checks are required for completion; signature validity or issuer/subject inequality alone does not pass this package.

### T04 — Shared format vectors and adversarial input limits

**Dependencies:** T02, N01 interface. **Scope:** APP verifier plus app/cloud protocol adapters.

Create the common canonical-vector harness, adversarial-limit helpers and course-endorsement fixtures first. This initial T04 completion is the prerequisite for downstream packages. C02, C04 and G02/G04 must then add their request-authentication, role-specification/result and governance vectors respectively as part of their own completion gates; T04 does not wait for those future implementations. Include both valid vectors and single-field tampering cases. Each consumer must validate the same exact bytes. Keep fixture private keys test-only and unmistakably unrelated to hosted personas.

Set explicit byte/depth/list/numeric limits for every new untrusted object and test exact-boundary acceptance plus one-over rejection. Use safe integer/string encodings across JavaScript and Rust. Reuse the current 256 KiB genesis limit and locator limits; do not silently broaden existing product limits. For new committee bundles, define and measure their limits in G02 before enabling endpoints.

**Exit:** cross-language/server/client vectors pass; wrong domain, network, signer, duplicated JSON key, unsafe number, hostile nesting, or oversized evidence never reaches an authority mutation. Format changes bump a version and are intentionally incompatible; there is no unsigned/legacy fallback.

### D01 — Delete legacy governance implementations

**Dependencies:** F02/F03 and T01. **Read:** preserved cleanup inventory, all six `legacy-*` feature references, routes/handlers, queue code, gossip subscriptions, schema references.

1. Inventory references using `rg` across Rust, Vue/TS, tests, scripts, manifests and generated files before deletion. Mark what is authority, discovery, content, or projection.
2. Delete old local elections/proposals/operator bootstrap/on-chain governance builders and their UI. Remove old taxonomy/Sentinel-prior/integrity/content-ratification authority and message handlers.
3. Remove all six legacy features and conditional sites. Do not mass-remove `cfg` lines while leaving their previously gated bodies active.
4. Keep genesis verifier/pinning/locator functionality and authentic user content workflows. Keep pending/provisional UI only where a real workflow exists; remove dead successful-looking pages.
5. Enumerate topic/protocol removals intentionally, update subscribers/senders and observer configuration together with N01. Any remaining legacy envelope is rejected before mutation, with no fallback to the old handler.
6. Remove dead Tauri registrations, CLI commands, frontend imports, routes, i18n keys and generated command entries. Preserve useful generic transaction/authorization regressions in surviving modules.

**Required tests:** release/default/debug builds expose no deleted authority; forbidden legacy messages cause no DB/UI/sync-log mutation; no queue constructs obsolete governance txs; no fallback installs an operator committee; genesis preview/retrieval/pin tests still pass.

**Exit:** no active `legacy-*` feature or legacy authority implementation. Searches may still match historical documentation, explicitly negative test data, or this plan; classify those matches rather than deleting blindly.

### D02 — Remove fake startup seeds and obsolete compatibility

**Dependencies:** D01, T02/T03 for scoring references. **Read:** `db/seed*.rs`, `lib.rs`, profile migration/keystore/content fallback code, CLI profile selection, frontend localStorage imports, bootstrap catalog code.

1. Move useful lesson/media corpus to `APP/demo-world/content/` as non-authoritative source content. Preserve required media licences/attribution. Do not move fake signed rows into a new seed module.
2. Delete fake persona, credential, opinion, governance and plugin-demo seed insertion and startup/backfill/rebind calls. Remove `dev-seed`, automatic public-catalog bootstrap and unlock-time demo-media downloads.
3. Keep the built-in taxonomy and exact-CID built-in plugin installation through an explicit, idempotent trusted bundle path. A new profile contains only required system/bootstrap records, not fake people or achievements.
4. Delete obsolete single-profile migration, legacy plaintext DB/keystore/content fallback, legacy username single-receipt compatibility, obsolete role conversion, localStorage migration and redirects. Handle each boundary in a separate reviewable change. Fail with a clear reset/import error on unsupported data.
5. Delete CIP-68 snapshot construction/recovery only after checking no current original preprod submission depends on it; protect any unresolved original transaction via section 4.6. Do not delete the general submission journal or current credential-backed snapshot path.
6. Replace seed-dependent tests with minimal `#[cfg(test)]` builders using real signatures whenever a test concerns verification. Unsigned malformed test rows remain useful negative fixtures.

**Required tests:** fresh profile has zero fake domain rows; no unlock network downloads for demo content; invalid/plaintext storage does not silently open; required taxonomy/plugins still install offline; profile lock/unlock works without seeds; credential issuance and assessment tests do not rely on production fake fixtures.

**Exit:** no production seed/backfill path can recreate fake authority. Deletion diff and surviving feature map recorded; line delta is informational.

### D03 — Create a fresh baseline schema with explicit identity

**Dependencies:** D01/D02 and stable T02/T03 structures. **Read:** `db/{schema,schema_tests,mod}.rs`, CLI migration runner and DB commands; surviving SQL across APP.

1. Inventory surviving tables, views, triggers, indexes, foreign keys and uniqueness/check constraints. Categorize authority/derived/private/local-only data using section 4.5. Identify every remaining reader of old scoring views before dropping them.
2. Build `MIGRATION_001_BASELINE` from the desired final schema, not a textual concatenation of 90 migrations. Remove obsolete governance/challenge columns and tables; retain current genesis trust anchors, credential/status state, lifecycle/settings, valid learning data, and current submission journal structures.
3. Give the new schema family an explicit identity in metadata. An old database with migration number `1` must not be mistaken for this new baseline. Validate schema family/history before issuing normal queries.
4. Keep the atomic migration runner for future schema evolution. Disposable old data permits a clean starting schema; it is not permission to abandon atomic schema management permanently.
5. Refuse incompatible old/unknown/future schema families with an actionable profile-reset message. Do not auto-delete files or silently migrate unsupported data.
6. Make CLI and app use the same baseline, initialization and validation code. Rewrite schema tests around invariants, rollback, encryption/reopen, foreign keys, uniqueness and idempotent initialization.

**Required tests:** new app/CLI DB schemas agree; initialize twice safely; old version-1 and version-90 DBs both refuse; unknown future family refuses; failed baseline transaction leaves no partial schema/version marker; encrypted file reopen works; forbidden fake/legacy tables absent; surviving SQL and FKs valid.

**Gate:** full `G-APP-RUST` and `G-APP-WEB`. M1 is not complete with intentionally failing intermediate SQL readers.

## 8. M2 — network, cloud, headless execution, and restore

### N01 — Versioned network profiles and protocol isolation

**Dependencies:** F01; contract can be defined before D03. **Read:** `p2p/{discovery,relay_registry,network,types,registry_chain}.rs`, profile creation/metadata, Cardano configuration, service callers, relay/observer startup.

1. Implement and validate the profile schema in section 4.3, initially from `APP/resources/networks/preprod.json`. Treat unresolved real values as deployment inputs: refuse activation if a required value is missing rather than shipping placeholder trust keys.
2. Delete hard-coded `RELAYS`, `GENESIS_ISSUERS` and governance-address ownership from runtime code. Centralize the approved preprod configuration without putting secrets in the resource file.
3. Persist immutable profile network identity on creation/restore and validate on every open. Profile restore must explicitly select/confirm the network; the same mnemonic does not justify silently switching it.
4. Add desktop/mobile libp2p DNS transport and fallback dialing using the verified configuration. Try public IP literals as libp2p fallbacks with pinned peer identity. Bound resolution/dial attempts and cancellation. A DNS failure must not loop forever during lock.
5. Define the surviving gossip and request/response namespace once; update app, headless node, relay-supported protocols and observer. Remove legacy authority subscriptions intentionally. Existing user relay additions remain subject to the network's identity/trust rules.
6. Add a cross-network test profile using different domain/instance values and, deliberately, the same test keys. Reject its claims, cloud proofs, committee proofs and sync envelopes on preprod.

**Required tests:** missing/malformed network file fails startup cleanly; DNS success; DNS failure with working pinned-peer IP fallback; peer-ID mismatch rejected; network cannot change in place; wrong-network signatures rejected; no plaintext public HTTP fallback or TLS bypass.

**Exit:** one typed source supplies all service/network configuration; peer transport works on desktop and mobile targets. Store configuration digest in the release manifest.

### N02 — Authenticated relay lookup and monitoring disclosure

**Dependencies:** N01, T04 for shared vectors. **Scope:** RELAY, MONITOR, app username/Identify code.

1. Make app registry calls use the configured HTTPS origin. Remove `resolve_username_did_via_relay`'s unsigned DID-binding fallback. Availability and resolution must distinguish confirmed occupied, confirmed available, unknown, busy, and unavailable.
2. Design a versioned signed lookup response binding network, relay issuer, normalized requested username, result type, current claim/release identity, issued/expiry time, and request nonce. Return the original claimant-signed claim/relay receipt where applicable. An old registration receipt alone does not prove current ownership after release/reassignment; a negative result also needs an authenticated freshness binding if called authoritative.
3. Verify normalized name, nonce, issuer against configured issuers, signature, lifetime, original claimant binding, and release state before caching. Cache exact signed bytes and expiry; no indefinite trust from a JSON DID field.
4. Provide persistent registry/DHT state in every active relay region. If a configured region cannot allocate storage, mark it non-authoritative/unready for issuing new receipts or choose a reviewed region; do not silently run an ephemeral refusal history under the same authoritative key.
5. Remove public plaintext 9090 exposure from Fly service ports while retaining internal health checks. Keep administration endpoints authenticated. Rate limiting may use `Fly-Client-IP` only from the trusted Fly ingress path; direct/untrusted header injection must not select an arbitrary limiter identity. Test spoofing, missing headers and IPv6 normalization.
6. Drive observer relay list/protocol namespace from the same network configuration. Add bounded login throttling. Do not trust client-supplied user-agent/device labels as authorization.
7. Change app Identify to platform/version information without hardware model. Remove the Apple model mapping if unused. Add Settings → Network disclosure explaining public peer observation. Minimize persisted observer data, log sensitive fields only under the explicit existing diagnostics policy, and document retention changes only with authorization.

**Required tests:** unsigned/tampered/stale/wrong-name/wrong-network lookup rejected; release/reassignment invalidates prior current-holder status; invalid JSON or 5xx never becomes availability; valid signed negative result works; duplicate claim conflicts survive relay restart; spoofed client-IP header cannot defeat limiting; unknown/busy remains distinguishable; observer only receives intended platform fields.

**Gate:** `G-RELAY`, `G-MONITOR`, changed app gates, and later `G-HOSTED`. Test the deployed external 9090 port is closed when O01 runs; source configuration alone is not that proof.

### N03 — Recover dropped authoritative gossip

**Dependencies:** F03, N01, T02. **Read:** `p2p/inbound.rs`, `p2p/{catalog,vc_did,vc_status,opinions,pinboard,device_sync,sync}.rs`, classroom events, existing fetch protocols and persistence.

1. Produce an event recovery matrix: event type, authoritative source, durable storage, dedupe key, fetch/replay mechanism, expiry, and max recovery budget. Cover catalog updates, DID/key rotations, VC revocation/status, classroom membership/key changes/messages, opinion publication/withdrawal, and newly retained protocol events.
2. Treat gossip as a notification that state may have changed. Reuse existing authenticated fetch/sync methods to recover missed state. Where no recovery exists, add a narrow versioned request/response path with shared limits and authority checks; do not invent a global raw-table sync endpoint.
3. For local state that must be advertised after a crash, store a durable publication intention in the same transaction as the domain change. Delivery retries carry the same signed artifact ID, use bounded backoff, obey profile closure and expire only under an explicit artifact policy.
4. Store receive progress only after durable verified application, not when gossip is received. A drop must not advance a cursor or poison deduplication so a later fetch is ignored.
5. Make status/key/membership updates monotonic by authenticated version and detect conflicting equal-version content. Do not apply arrival-time LWW to security authority.
6. Expose queue/drop/recovery metrics. Old-profile work may neither publish private data nor update the next profile after lock.

**Required tests:** deliberately overflow queue, miss revocation, reconnect and converge; miss classroom key rotation and recover without plaintext fallback; lost publication after local commit resumes once; duplicate replay has one semantic effect; peer failure/backpressure is bounded; conflicting version quarantined/rejected; old profile cannot resume another profile's outbox.

**Exit:** every surviving authoritative event has either a tested recovery path or an explicitly unavailable feature. Do not claim eventual convergence based solely on queue size or a successful happy-path gossip test.

### C01 — Integrate the cloud worktree and pin shared verification

**Dependencies:** F01, T04/N01 contract. **Read:** CLOUD's complete diff, especially `auth/oidc.rs`, `config.rs`, `egress.rs`, `db/mod.rs`, HTTP routes and `tests/route_authorisation.rs`; CLOUD CI and Dockerfile.

1. Review CLOUD's dirty backend and UI changes against CLOUD_MAIN, preserve intended changes, and resolve actual conflicts in an isolated implementation checkout. Keep stylistic cleanup separate from auth/tenant behavior changes.
2. Audit every route added/modified by the UX work for session authentication, tenant checks, CSRF where applicable, body limits and role permissions. UI concealment does not protect a route.
3. Pin `alexandria-verify` to the exact app repository revision containing the required shared formats, including its crate path in the dependency resolution supported by Cargo. Confirm a standalone cloud checkout/Docker build resolves it without sibling local paths. Do not point CI at a mutable branch or a local app main with different formats.
4. Reserve verifier version 0.2.0 for the incompatible format set, but publish only after vectors and formats freeze in R01. Re-pin the immutable Git revision when a shared format changes during implementation.
5. Retain owner/runtime database-role separation and real Postgres tests. Review CI's existing DCO requirement; use configured contributor identity for new sign-off where authorized, never invent an identity or rewrite historical commits merely to satisfy the checker.

**Required tests:** standalone cloud build; existing route-authorization and tenancy suites with real Postgres; unauthorized new UX API access denied; verifier vectors match APP. No tests silently skipped for lack of database.

**Gate:** `G-CLOUD`. Existing cloud-UX unfinished work must be accounted for before first deployment; this is not permission for a fresh UI redesign.

### C02 — Durable replay protection, bounded egress, and shared request proofs

**Dependencies:** C01, N01, T04. **Read:** CLOUD `pull.rs`, evidence/presentation/talent/holder routes, `egress.rs`, tenancy/idempotency helpers; APP `commands/{holder_pull,holder_release,talent_index}.rs`.

1. Implement section 4.4 in the shared verifier and both adapters. Enumerate every route using old `verify_proof` and every app signer; migrate them together and remove v1 acceptance.
2. Replace process-local replay state with a Postgres table and unique replay key, signed expiry metadata, and expiry-only cleanup. Bound request admission and table growth without evicting live proofs. Use checked timestamp arithmetic to avoid overflow on hostile values.
3. Keep global replay bookkeeping separate from application tenant privileges. Limit the runtime role's operations on this table; do not give request handlers an owner/BYPASSRLS connection.
4. Add stable idempotency keys and payload-digest conflicts to mutation routes, returning a durable previous result for a fresh authorized retry. Clarify and test the transaction order between proof spending, operation reservation and business mutation.
5. Review URL retrieval for uploaded job descriptions, OIDC discovery/JWKS where configurable, webhooks and presentation/holder fetches. Use approved HTTPS destinations, connect-time address checks, bounded DNS/redirect/body behavior and no proxy bypass where the destination is untrusted. Provider allowlists do not justify accepting arbitrary private redirect targets.
6. Return bounded public error codes; do not expose raw SQL/internal errors, credentials, proofs, or private release payloads in responses/logs.

**Required tests:** two processes race the same valid proof and exactly one succeeds; restart replay fails; live-proof capacity pressure fails closed; valid next nonce succeeds; huge/future/overflow timestamp rejected; altered body/query/recipient/audience/network rejected; two tenants cannot replay across each other; DB unavailable releases nothing; retry same operation has one effect; same ID with changed payload conflicts; SSRF redirect/proxy/DNS cases fail in controlled tests.

**Gate:** `G-CLOUD`, shared vectors, focused app cloud-client tests. Cryptographic proof plus a nonce does not replace tenant authorization.

### C03 — Real OIDC and runtime database permissions

**Dependencies:** C01; provider-specific hosted completion depends on Q03. **Read:** CLOUD `auth/{profile,oidc,session}.rs`, `http/auth_routes.rs`, configuration, database initialization and existing OIDC/tenancy tests.

1. Keep one standards-based OIDC engine. Configure the selected provider through the existing profile abstraction, adding only the issuer/profile facts needed. Do not build a custom password or JWT issuer in Alexandria.
2. Use authorization code flow with PKCE, state and nonce validation, exact issuer/audience checks, signature/JWKS validation, bounded discovery, secure callback handling and session cookies. Restrict callback destinations; reject open redirects and mismatched issuers.
3. Preserve organisation membership/role authorization in Alexandria; an identity-provider login or arbitrary email domain does not automatically select/administer a tenant. Test invited member, wrong tenant, disabled member and revoked access.
4. If Keycloak is selected: use a dedicated preprod realm, a confidential server-side client where appropriate, exact HTTPS redirect URI, no wildcard callbacks, version-pinned container and exportable non-secret realm/client settings. Protect admin credentials separately; persist provider state; keep database credentials separate from Alexandria's RLS role. Use a supported release verified at execution time rather than an unverified image tag in this plan.
5. If authentik is selected: provide equivalent provider/client, redirect, persistent-state and administrator protections using its standard OIDC interface. Do not carry both production adapters when one generic issuer profile suffices.
6. Configure Neon owner role for migrations and restricted runtime role for requests. Test with the actual runtime role; connecting tests as owner can conceal an RLS defect. Define pooled/direct endpoints explicitly if migrations or transaction-local tenant context require them.
7. Hosted startup refuses DEV_AUTH/mock-idp configuration. Development conveniences must be compiled/configured so a hosted image cannot silently choose them when a secret is missing.
8. The demo can use pre-provisioned real IdP accounts; it does not need a marketing sign-up funnel. Account creation/password delivery must use protected setup, not committed credentials.

**Required tests:** OIDC state/nonce/PKCE failure; token issuer/audience/expiry/signature failure; JWKS rotation; callback replay; wrong-tenant login; disabled membership; session logout/expiry; runtime RLS under connection reuse/concurrent tenants; hosted DEV_AUTH refused; real browser login after deployment.

**Gate:** `G-CLOUD`; hosted OIDC remains pending until actual provider setup and browser flow pass.

### C04 — Signed sponsor specification, local run and verified release

**Dependencies:** C02, T02/T04; Q02 is resolved in favor of demo hosted custody. **Read:** CLOUD `org_key.rs`, role/run routes, `http/{api,evidence}.rs`; APP `commands/{role_assessment,assessment,holder_release}.rs`, issuance/integrity policy.

1. Define `SignedRoleSpecification`: network/domain, org DID, immutable spec ID/version/content digest, role/skill mapping, exact course/assessment/grader identity, allowed assurance/evidence rules, run binding/lifetime and issuer key binding. Sign canonical bytes under the organisation's selected key.
2. For the demo, opt the organisation into hosted custody using the existing encrypted-key mechanism and explicit recorded acknowledgement. Keep keys encrypted under a deployment KEK outside the DB, redact Debug/log output, and retain external signing for self-custody organisations.
3. App fetches through the configured authenticated directory, verifies org identity/spec/lifetime before showing consent, and runs the actual existing local assessment/grader path. No IPC accepts arbitrary passing scores as a completed run.
4. Define a learner-signed result binding subject, org/run, exact spec and grader digests, attempt ID, actual scores/evidence digests, assurance achieved, timestamps and protocol version. A learner never needs the organisation private key on-device.
5. Release only explicitly selected evidence to the named organisation/run with an exact preview. Cloud verifies request proof, subject/run ownership, spec signature, result signature, evidence consistency and achieved assurance before atomically recording results and audit events.
6. Replace the development `run_record` fabrication route with the real verified ingestion flow, or remove it and route the console to the new endpoint. Do not expose synthetic score mutation behind an authenticated organisation session.
7. If an organisation-issued RoleCredential is desired by the existing flow, have the organisation signer issue it only after verification. Represent a candidate-signed run result separately; never label it organisation-issued without that signature.
8. Withdrawal removes future authorised access to the released evidence and updates dependent views according to existing rules. It cannot recall copies already disclosed or erase a valid signature. Do not make a missing/withdrawn frame itself proof of misconduct.

**Required tests:** candidate cannot sign as org; org/spec mismatch; altered grader/course/score/evidence fails; fabricated result cannot pass required assurance; replay has one result; invalid evidence cannot update run status; unrelated org cannot release/view/withdraw; no consent sends nothing; withdrawn evidence is inaccessible through list/detail/export/old links.

**Gate:** app/cloud focused tests and `G-CLOUD`; later hosted sponsor journey. No fake assurance claim may be added to make the demo look complete.

### H01 — Create a real headless boundary without a parallel application

**Dependencies:** F02/F03, T02 core interface. **Read:** `lib.rs` AppState/start/stop, profile handlers, executor/leases, crypto/vault initialization, p2p event emission, native tutoring dependencies, `tests/guardian_e2e.rs`.

1. Produce a dependency map showing which state and commands actually require Tauri, UI events, camera/audio, or global process state. List module-level singleton assumptions that prevent two persona nodes in one process.
2. Implement a minimal headless node interface with owned profile resources, executor, lifecycle, network configuration, clock and event sink. Reuse business operations; do not call Tauri IPC handlers by constructing fake UI state.
3. First prove profile create/restore, start/stop, content store and P2P with two isolated nodes on headless Linux. No display server, camera permission, microphone initialization or GUI event loop may be required.
4. Expose `app_lib::headless::Node` as a facade if practical. If APP's crate dependencies force GUI/native initialization/linkage, extract the necessary runtime into `crates/alexandria-core` and have both app and headless facade depend on it. Perform the move in small compilable slices. Do not duplicate models/database/schema or start an unrestricted whole-repository rewrite.
5. Define an event-sink trait for outward notifications and explicit adapters for Tauri and headless execution. Core code must not hold an AppHandle; UI delivery failures do not roll back an already committed domain action.
6. Keep media/tutoring optional and lazy for headless nodes. World serving needs blobs, catalog, attestations and addressed protocols; it does not automatically start video or Sentinel capture.

**Required tests:** two personas have separate keys/DB/blob stores; stopping A does not stop B; cancelled profile creation rolls back; lock drains jobs; Linux headless process works without display/media devices; same core operation through app and headless adapters has equivalent signed output for fixed inputs.

**Exit:** minimal headless smoke binary with bounded resource ownership, not the complete world generator. Record memory per idle/active persona before choosing hosting sizes.

### H02 — Expose business operations and a minimal CLI

**Dependencies:** H01, T02/T03. **Read:** existing `*_impl` functions for profiles, courses, assessment, enrolment, completion, credentials, classroom, guardian, pairing, username, talent index and releases.

Extract narrowly typed service functions in this order: profile create/restore; publish exact signed course; enrol/start/submit/grade assessment; create self-completion; request/sign/apply instructor endorsement; request/query optional witness; username claim; holder/talent release; classroom/guardian; pairing. Use injectable time for local learning history and tests only. Production signing/chain/committee timestamps must come from their real approved clocks.

Add CLI commands `profile create`, `profile restore`, `p2p status`, and the minimal operation commands needed by the world builder. Read passwords/mnemonics through a protected input mechanism; default output is redacted machine-readable status and public identities, not secrets. Keep startup registration/permission checks equivalent to the app.

Expose a shared stake-public-key registration operation around the existing `cardano/stake_pubkey.rs` builder, submission journal and registry verification. Do not write a confirmed registration row directly in demo code.

**Required tests:** malformed request/unknown policy refused identically by CLI and app; caller identity derived from key; course publication traverses normal signing/storage; grading cannot be replaced by fixture scores; all chain operations journal before send; protected input absent causes a clear failure rather than a default password.

**Exit:** world code can call business operations without SQL seeding or UI automation. No generic `execute_sql`, `set_score`, `set_verified`, or unrestricted signing service is introduced.

### H03 — Restore authentic history through paired-device sync

**Dependencies:** H01/H02, N03, T02/T04. **Read:** `domain/sync.rs`, `p2p/{device_sync,sync}.rs`, pairing crypto/commands, guardian/classroom history, credential/status storage and blob fetch.

1. Create a table/artifact coverage matrix. Mark exact required fields for courses, enrolments/progress, attempts/completions, signed VCs/status/key bindings, optional chain references, classroom memberships/messages, guardian state, released evidence references and settings.
2. Keep allowed private mutable learning tables under explicit merge rules. Add verifier-backed signed-artifact synchronization for credentials/course documents/status/key history; do not add every DB table to generic LWW replication.
3. Recompute derived proficiency and snapshot display from verified restored evidence. Acquire blobs by content address and normal ownership/permission checks. Preserve tombstones/version semantics for deletion/revocation where supported.
4. Pair only with authenticated proof of the same subject and an explicit short-lived one-time pairing code. Bound request size, pages, cursor lifetime and outstanding work; verify before advancing cursors.
5. Do not transfer raw vault secrets, device keys, chain journals, user consent, diagnostics sessions, or trust pins through generic sync. A restored client separately previews/pins the applicable genesis. Explain history coverage honestly in the UI.
6. Use the world controller's single-writer/export mode in section 4.6. Restoring history must not replay assessment or chain commands and create duplicate credentials/transactions.

**Required tests:** a new empty device restores real credential/progress history and verifies it; malicious peer cannot insert unsigned credentials or trusted pins; cross-user/network pairing refused; interrupted paging resumes idempotently; revoked VC stays revoked; stale pages cannot overwrite newer signed state; lock cancels safely; blob absence is pending content, not missing ownership proof.

**Exit:** the minimal learner persona is usable on a second device with the original history. List any later persona-specific coverage still needed in W04; do not represent it as finished.

### X01 — Generate complete IPC contracts

**Dependencies:** T02/T03 core DTOs; expand alongside subsequent packages. **Read:** `useLocalApi.ts`, generated command union, command guard, Rust handlers/DTOs, profile scope policy, platform-specific command registrations.

1. Prototype one narrow domain with a Rust-derived binding generator. Compare a suitable Tauri/Specta integration with a schema-driven generator that derives from actual Rust DTOs. Check current official docs and pin the chosen versions. Required outcomes are below; do not choose a tool solely because its name appears in an old plan.
2. Generate a command map relating name to argument and result types. The public wrapper must infer the result and reject wrong arguments. Caller-supplied `invoke<T>` must not be a way to assert arbitrary result types.
3. Preserve profile-session headers, stale-response rejection, and unscoped-command policy in the typed transport. Generated functions must not bypass the wrapper by calling raw `tauriInvoke` without session fencing.
4. Model serialization exactly: optional/nullable, camel-cased IPC argument names where Tauri expects them, snake_case domain fields, enum tagging, byte transport and integers safe at the JS boundary. Platform-gated command availability must be checked for actual build targets.
5. Expand domain by domain; regenerate rather than hand-edit emitted files. Keep a deterministic check mode used in CI. Test intended compile failures through a compiler fixture harness, without `@ts-expect-error` or `@ts-ignore` in app code.

**Required tests:** valid call infers exact result; wrong argument name/type, missing required argument, wrong result assignment and unknown command fail compilation; session fencing remains enforced; generation drift fails CI; platform command inventory matches registration.

**Exit:** all shipped frontend calls have actual argument/result contracts. A generated list of names alone is not this package's completion condition.

### X02 — Bound bulk content and preserve private-media lifetime

**Dependencies:** F03, H01 where shared lifecycle changes apply. **Read:** `commands/content.rs`, content resolver/storage, asset protocol, PDF/video/download consumers, existing profile media-cache tests.

1. Inventory all byte-array IPC consumers. Separate small JSON/model payloads from bulk PDFs/media/export. Measure existing peak memory and copy overhead with 1/16/64 MiB fixtures.
2. Move bulk paths to binary IPC or profile-scoped file/stream delivery using existing safe materialization where possible. Preserve MIME/CID/path validation, CSP, scoped asset authorization, cancellation and content encryption boundaries.
3. Bound allocation/concurrency for local, peer and HTTP inputs, not just Content-Length. Do not broaden maximum supported file sizes or reduce offline language support to make benchmarks pass.
4. Track any temporary plaintext files under profile lifecycle ownership; revoke access immediately on lock and remove files on cleanup/eviction/failure. Protect against stale URLs, range requests and traversal.
5. Keep small structured consumers correctly typed. Preserve installed offline script/font coverage; inspect mixed imports and only split bundles where measurements justify it.

**Required tests:** oversized local/peer/HTTP path rejected before unbounded buffering; cancellation removes private temporary state; locked profile assets inaccessible; profile B cannot access A's asset URL; PDF/video/export works; retained language scripts render offline.

**Exit:** bounded transport with measured memory and correctness. Absolute device performance budgets remain V02's measurement/decision gate, not invented promises.

## 9. M3 — the smallest real hosted world

### W01 — Deterministic world specification, state ledger and audit

**Dependencies:** H02, N01, T02. **Create:** `APP/demo-world/` workspace crate with binary `alexandria-world`; content corpus directory retained from D02.

Proposed command surface, **to be implemented before these commands are treated as available**:

```text
alexandria-world plan --spec world.toml
alexandria-world build --spec world.toml --max-requests <explicit-count>
alexandria-world audit --spec world.toml
alexandria-world serve --spec world.toml
alexandria-world export-persona <name> --output <protected-directory>
alexandria-world reset --spec world.toml --world-id <exact-current-id>
```

1. Define `world.toml` with schema/version, network, world ID, corpus digests, persona names/roles, course assignments, service references, requested artifacts and required proof gates. Exclude secret seed/password/token values.
2. Derive persona entropy from a secret world seed using a versioned HKDF context containing network/world/persona/purpose. Separate wallet, node and validator purposes. Protect the seed outside Git; an ignored `keys/demo-world.seed` is acceptable only with restricted permissions and a checked backup. Never derive keys from public names alone.
3. Store an atomically updated ledger with stable operation IDs, artifact CIDs, intended inputs/digests, transaction references/status, remote operation IDs and audit outcomes. Exact recovery material remains encrypted as in section 4.6. Do not keep secrets in `world-state.json`.
4. Implement each ensure-step as read/verify existing state → act only if absent → persist durable identity/checkpoint → verify outcome → mark complete. A network timeout leaves pending/unknown and is not an excuse to generate a new ID.
5. Separate `plan` (read-only projected actions/resource requests), `build` (bounded real operations), `audit` (reverification, no automatic repair), and `serve` (normal node services). Audit failure must produce a nonzero result with public artifact IDs and bounded reasons.
6. Respect a shared Blockfrost request budget including retries and pagination. Reaching it checkpoints and stops resumably; it must not fail an already-signed tx into a replacement path. Do not hard-code the earlier 100 tADA estimate as a verified cost.

**Required tests:** deterministic IDs for fixed spec/seed; changed inputs conflict explicitly; crash before/after each ledger step recovers; second build creates zero duplicate artifacts/POSTs; exhausted request budget stops resumably; corrupted ledger causes re-verification/refusal, not trust; audit rejects tampered proof/tx mapping.

**Exit:** machine-readable plan/build/audit with protected secrets and recoverable state. Add new workspace paths to CI coverage from F04.

### W02 — Build one instructor, one learner, one course

**Dependencies:** W01, H02, T03, N02/N03. **Use:** existing preprod registration/completion examples as references, not alternative bypass APIs.

Ordered steps:

1. Create real profiles through core APIs; obtain public identities and verify derived bindings.
2. Query actual preprod balances and fund only the required deficit from the authorized demo treasury, with a stable operation/journal identity. Do not assume faucet availability or print the treasury key.
3. Register stake public keys through the real builder/journal and verify chain receipts/registry ingestion. Initial registry founder signatures use protected real demo founder keys, exact network binding and signed snapshot verification.
4. Import source lesson blobs, build an author-signed course with real grader/content identities and attestor requirements, and publish through ordinary storage/catalog paths.
5. Learner discovers/fetches/verifies the course, enrols, submits actual responses and runs the real grader. No direct insertion of progress/score/credential rows.
6. Issue immediate self-completion; obtain a genuine instructor endorsement through T02; optionally request a preprod completion witness/credential anchor through the existing journal. Verify actual confirmed outcome before labelling it anchored.
7. Exercise an authenticated username claim/lookup and its selected anchored tier, within request limits.
8. Audit each source artifact and derived displayed state. For every externally trusted claim, report issuer, subject, course/policy identity, proof validity and optional chain reference separately.

**Required tests:** no fake identifiers/signatures; valid offline credential verification after evidence capture; course tampering fails; instructor rejection leaves only self claim; real tx confirmed only from receipt; interrupted step resumes without duplicate transaction; second complete build has no duplicate semantic effects.

**Exit:** one real local/controller world using hosted network endpoints. Hosted long-lived availability is W03/O01, not implied by a successful build while the developer laptop is running.

### W03 — Serve, release to cloud and restore on a fresh device

**Dependencies:** W02, C02/C03/C04, H03, O01.

1. Deploy the world-serving process to the dedicated demo-peers app with persistent profile/blob volumes and protected vault inputs. Keep instructor/course providers online independently of the developer laptop.
2. Serve normal catalog/blob/attestation and pairing protocols. Enforce addressed requester authorization and evidence validation. Do not create an unrestricted remote sign-any-bytes or approve-any-assessment endpoint.
3. Use a real cloud tenant and OIDC-authenticated operator to publish one signed role specification. Learner runs it locally, previews/releases its result/evidence, and the cloud verifies/records it. Exercise directory/talent publication, presentation share, withdrawal and export using actual app clients.
4. Export the learner's mnemonic through a protected output file and generate an expiring pairing code. Do not put mnemonic/code in ordinary logs. Put the hosted learner copy into history-serving mode before active use elsewhere.
5. On a fresh device, restore the profile and network, pair, synchronize permitted history, fetch blobs, independently preview/pin trust roots, and recompute displayed proficiency. Verify credentials offline from the captured evidence after network disconnect.
6. Start a fresh unrelated learner profile as well: it sees hosted courses through discovery and may learn without importing a privileged persona.

**Hosted acceptance:** with controller laptop offline, a new macOS device discovers the course within the inherited target of 60 seconds under recorded network conditions; restores original credential/history; verifies supplied credentials offline; cloud sees exactly the consented release; withdrawal removes subsequent access; second world run creates no duplicate effects. Record actual latency and failures, not only screenshots.

**Exit:** the complete smallest journey passes on the hosted deployment. This is the first useful demo milestone, before expanding the whole world.

### O01 — Reproducible hosting, budget and service health

**Dependencies:** C01/C02, N02, W01; final provider and spending authorization in Q03.

1. Prepare a resource/cost worksheet from current official provider pricing at execution time: existing/new relay machines and volumes, observer, cloud, Neon DB/storage/egress/backups, IdP compute/database/email, demo peers and later seven committee nodes. Show monthly always-on cost, demo-only runtime alternative, currency/taxes, free-tier limits and a contingency. Do not assume a free tier has unlimited capacity or permanent availability.
2. Present the concrete topology and cap for unresolved Q03 before paid provisioning. Reuse existing authorized resources where suitable. No deployment to mainnet or production identity/service names.
3. Record app names, regions, machine sizes, image digests, persistent volumes, public/internal ports, health endpoints, network/instance IDs, secret names (not values) and database roles in the release manifest. Cloud target remains Fly `fra` plus Neon unless the user changes it.
4. Prepare idempotent infrastructure/configuration scripts with read-only plan output. Separate migration credentials from runtime secrets. Health checks must verify readiness for authority issuance: correct network, required persistent state, DB role and key availability. A process alive with an empty ephemeral receipt store is not registry-ready.
5. Build containers from clean exact revisions and start them against disposable test DBs before hosted deploy. Run database migrations once with the owner role; runtime uses restricted roles. Pin dependency and image versions/digests.
6. Deploy in dependency order: database/IdP → cloud → persistent relay/observer changes → demo peers → app profile with actual endpoints. Compatibility-breaking changes use the isolated new preprod namespace/instance; do not leave mixed legacy authority active.
7. Verify external HTTPS/certificate behavior, closed plaintext/admin ports, readiness/liveness, real OIDC, relay persistence after restart, service replay across two cloud instances, and observer disclosure. Capture bounded diagnostics without private artifacts.
8. Add alerts for failed health/readiness, expiring certificates, DB availability/storage, relay receipt-store failure, cloud replay-store failure, exhausted request budget and low demo treasury. Prefer existing monitoring infrastructure; do not add a large observability stack without need.
9. Define backup/restore and rollback for configuration/DB/profile state. Rollback must retain replay records and original chain/consensus signing state; reverting a volume to an old snapshot is not safe replay/double-sign recovery.

**Exit:** exact deployed revisions and health evidence recorded; cost within an explicit approved cap; no secret exposure; actual hosted contract gate passes. A built Docker image is not evidence of deployment.

## 10. M4 — committee consensus and independently verifiable outcomes

### 10.1 Scope and separation of duties

CometBFT owns consensus rounds, ordering, locking, validator precommits, block persistence and catch-up. Alexandria owns proposal/vote authorization, exact evidence disclosure, qualification policy, receipt interpretation, state transitions, proofs of application state and client verification. Do not implement an alternative consensus or select the first five HTTP responses as a substitute for committed consensus.

Create `APP/committee/` as a workspace package. It may depend on `alexandria-verify`, deterministic storage and the selected ABCI/RPC adapters. It must not link Tauri, the application database, media stack, a live Blockfrost client, cloud OIDC or app profile state into the state machine.

Consensus-controlled state must not depend on local wall clock, iteration over unordered maps, host locale, random UUID generation, floating-point arithmetic, local SQL row order, remote credential retrieval, or HTTP results. Evidence acquisition happens before submission or in an adapter outside deterministic application execution. The exact input bytes are then validated by every node.

### 10.2 Required artifact bindings

The following are **requirements for G02**, not a finalized wire format. G02 must produce exact serialization, field types, bounds, signing domains, hash algorithms and test vectors before downstream code treats them as a contract.

| Artifact | Required binding |
| --- | --- |
| Founding genesis | Existing core-derived DAO ID, scope, seven distinct identities and role keys, initial policy/rules, all seven acceptances and 21 key-possession proofs, explicit network/instance/CometBFT genesis binding |
| Proposal opening | DAO/instance, proposal ID, exact proposal/effect content identity, policy/rules hash, opening reference, cutoff, minimum distinct-voter count, yes/no tally rules |
| Vote intent | Domain/network/DAO/instance, proposal/opening identity, voter identity, yes/no choice, exact evidence manifest, consent binding, stable intent ID and signature |
| Submission receipt | Exact vote and accepted state, historical qualification evidence, policy/opening, containing block/tx index, correct signed-header/commit and application acceptance proof, five timely distinct-member observations under U08/U09 |
| Close certificate | Proposal/opening, authenticated closed range/prefix, complete accepted-vote set root/count, exact tally/outcome/effect digest, application-state proof at the correct committed height, required quorum/time evidence |
| Offline verification bundle | Pinned genesis/instance, required headers/commits and linkage, accepted vote/evidence set or authenticated complete replay, inclusion/state proofs and algorithm versions; bounded self-contained bytes |

Published evidence bytes must exactly match the consent preview. The client cannot silently replace them after a preview if a credential/status update occurs; it must request a new review of changed bytes.

### G01 — Prove the selected Rust/CometBFT stack

**Dependencies:** F01 only; start early. **Scope:** disposable spike in `committee/` or a task scratch package, shared verifier fixture only. No hosted committee yet.

1. Verify current official CometBFT v0.40 release/checksum and the exact Rust crate/API versions. Record the tested binary digest, Rust versions, ABCI dialect, protobuf packages, socket/gRPC framing and light-client verification APIs. Do not confuse a Rust crate version with the CometBFT daemon version.
2. Build an actual ABCI handshake, InitChain, CheckTx, ProcessProposal, FinalizeBlock, Commit and restart/Info cycle. A successful crate compilation is not compatibility evidence.
3. Commit a tiny deterministic state change, retrieve its block/commit/result/state proof, and verify it independently using only the trusted test genesis plus supplied proof bytes. Test the post-state commitment at H+1.
4. Demonstrate the selected client verifier on macOS and compile for Windows/Linux/iOS/Android. Run an identical known-answer vector on actual supported target runtimes when available. Treat `no_std` as a property to verify, not an assumed crate name; I/O-free is mandatory regardless.
5. Inspect whether the ABCI input exposes the timestamp/signature data needed by U08/U09. List what is available at each height/callback and what would require explicit in-transaction evidence. Do not call CometBFT RPC from deterministic application methods to fill a missing input.
6. Test startup replay after app/daemon disagree by one persisted height. Confirm how crash recovery reproduces the same application hash and result sequence.

**Exit artifacts:** exact dependency lock, runnable minimal daemon/app fixture, accepted-state proof vector, cross-target compile/runtime matrix, height/timing sequence diagram and explicit compatibility conclusion.

**Stop condition:** if v0.40 support or required proof semantics cannot be demonstrated, do not quietly downgrade. Present the actual failure and bounded alternatives (compatible adapter, supported version, or a reviewed requirement change). Finish unrelated app/cloud work while this gate is unresolved.

### G02 — Freeze the receipt, cutoff, completeness and clock protocol

**Dependencies:** G01, U08–U11, T04. **This is a protocol-design package. A smaller model must not mark it complete by writing plausible structs.**

Write a concrete executable protocol contract, using pure functions and test vectors to resolve each item below. It may be a proposed technical appendix to this plan while normative docs await authorization. Have the designated reviewer/user assess any remaining semantic choices before activating them; ordinary proof implementation that meets already selected rules does not require another preference question.

1. **Accepted versus included.** Define whether ProcessProposal rejects any block with an invalid vote, whether failed transactions may be committed, and which application result/state proof establishes that a specific vote counts. Test invalid/duplicate transactions in otherwise valid blocks. A Merkle proof of raw transaction bytes must never alone label a failed vote accepted.
2. **Correct heights.** For a transaction executed at H, identify the header/commit that authenticates the resulting state and transaction result. `Close` at H cannot prove its new outcome against H's pre-execution AppHash. Specify all header links, tx indexes and proof keys; test off-by-one height substitution. The [v0.40 ABCI methods specification](https://github.com/cometbft/cometbft/blob/v0.40.0/spec/abci/abci%2B%2B_methods.md) documents next-block state commitment and separate transaction results.
3. **Timely member observations.** Define the canonical source/set of member timestamp evidence, exact domain being signed, height/round/block binding, when it becomes visible to the app, and how it is committed before a close can depend on it. Distinguish raw consensus precommit timestamps from Alexandria's old receipt attestations. Do not assume they are interchangeable. Preserve five independently valid pre-cutoff observations; merely using block time changes U08.
4. **No circular timing dependency.** Draw the sequence from VoteIntent through candidate block, consensus commit, observation evidence, accepted application state and final receipt. At each stage list available deterministic inputs. If acceptance depends on evidence only created after execution, explicitly represent a pending stage and a later authenticated transition; do not read future data or label the pending stage Submitted.
5. **Close barrier and omission resistance.** Define how the state machine establishes that every receipt-eligible vote before cutoff is covered by the closed range, including evidence delivered after cutoff, and how pending candidate votes resolve without an omission opportunity. Specify behavior if the needed evidence is unavailable. A timeout cannot silently drop a timely quorum-receipted vote or finalize an incomplete range. If the proposed receipt scheme cannot establish this, G02 remains blocked.
6. **Complete accepted set.** Use a deterministic accepted-vote manifest/root and count tied to application state, or a complete authenticated replay procedure. Specify leaf keys, ordering, duplicate handling, proposal filtering and count binding. Recompute the manifest from all supplied votes and compare it to the committed root/count before tallying. Individual inclusion proofs cannot prove completeness.
7. **Clock health.** Select concrete authenticated time acquisition supported by deployment, source diversity, estimated-error calculation, sampling age and failure behavior. Keep it outside deterministic execution. Commit required reviewable health claims/evidence with relevant signatures so clients enforce the declared bound. A signed claim is not external proof of physical UTC; state the Byzantine clock assumptions honestly. Define which signatures stop when unhealthy without introducing inconsistent deterministic validation.
8. **Eligibility time and status.** Specify historical issuer/key/status evidence sufficient at certified submission, with expiry/freshness bounds fixed by the opening policy. A late revocation must not silently rewrite prior eligibility; evidence proving invalidity at submission must be rejected under the selected historical rule. Do not accept an arbitrarily old non-revoked status snapshot.
9. **Deterministic tally.** Minimal ballot is yes/no, one accepted ballot per policy-defined voter identity per proposal. Use checked integer arithmetic: approval only if `3 * yes >= 2 * (yes + no)` and total distinct eligible ballots meets the immutable minimum. Define zero votes, malformed counts, duplicate devices, conflicting intents, before-open/after-cutoff and repeated Close. The five-validator certificate threshold is unrelated to voter turnout.
10. **Authenticated store/proofs.** Select and specify an actual deterministic Merkle store/proof algorithm. SQLite rows plus a hash column are not a proof-serving implementation. Freeze canonical state keys/encoding and verify unsupported algorithms/proof types are rejected.
11. **Evidence limits and lifetime.** Bound tx bytes, evidence list/count/depth, request bytes, proof/header counts, proof-serving pages and aggregate offline bundle size. Measure representative qualification artifacts before choosing limits. Until a pruning policy exists, retain evidence for the instance lifetime, with monitored disk budget. Full disks stop admission safely rather than deleting history required for audit.
12. **Freshness versus historical verification.** Separate offline proof validity at its certified height from claims about the latest chain or current revocation. Specify light-client trusted state/time assumptions and how an old pinned instance is handled after reset or trust expiry. No silent trust refresh from an endpoint response.

**Required executable adversarial scenarios:** wrong DAO/network/instance/round/block; duplicate signer/key; altered time/clock health; wrong state height; included-but-rejected vote; missing earliest/latest vote; same-length substituted vote list; forged status; stale qualification evidence; accepted vote delivered late; close while candidate evidence missing; restart between receipt stages; four/three split; tampered outcome/effect; pre-cutoff signature with unhealthy clock.

**Exit:** precise format vectors, deterministic reference transition/tally verifier, an actual accepted-vote/close proof from G01's daemon, and every scenario has a specified expected result. Any unresolved product/trust semantic is explicitly blocked. Do not create G03/G04 production formats before this exit.

### G03 — Implement the deterministic committee application

**Dependencies:** G02. **Scope:** `APP/committee/`, shared verification crate, protocol tests.

1. Implement state keys for genesis/instance, policies, proposals, vote intents/accepted ballots, receipt stages defined by G02, closed manifests, outcomes and supported effect records. Use stable explicit encodings and versioning.
2. Implement OpenProposal, SubmitVote, any required G02 receipt-evidence transition, and Close. Authenticate proposal creation under the frozen genesis/opening policy; neither RPC access nor a logged-in cloud user grants proposal authority.
3. Share pure validation between CheckTx, ProcessProposal and FinalizeBlock while respecting their different states. Recheck state-dependent validity in deterministic execution. Proposal simulation must not persist effects before Commit.
4. Persist the whole application transition atomically with height/hash metadata. Expose correct Info after restart. Test interruption before/during/after Commit and daemon replay; duplicate execution of a committed height must not count again.
5. Implement authenticated store/proof queries exactly as G02 froze them. Return bounded typed not-found/pending/proof responses; never synthesize an outcome from an incomplete local cache.
6. Keep fixed validator set and fixed protocol version. Reject unimplemented elections, epoch updates, quorum changes, arbitrary execution payloads and unsupported effect types.

**Required tests:** same ordered input yields byte-identical state/hash across fresh runs and hosts; rejected transaction leaves state unchanged; eligibility check and ballot insertion atomic; duplicate voter/device does not increase turnout; cross-proposal evidence rejected; threshold boundaries checked; count overflow fails; crash replay yields same state/proofs; no external calls in deterministic execution.

**Gate:** strict committee/verifier checks and real G01-daemon integration. Do not gate semantic correctness solely on self-generated fixtures; preserve independent golden vectors.

### G04 — Produce receipts, close bundles and offline client verification

**Dependencies:** G03, T04. **Scope:** committee proof service; verifier; app client persistence.

1. Implement the G02 proof pipeline from actual daemon blocks/commits and application state. Do not hand-assemble a successful certificate from test keys in production.
2. Bind exact accepted intent/evidence, opening policy, instance, historical qualification, member observations and correct state height. Produce complete vote manifests with authenticated paging if needed; incomplete download remains pending.
3. Implement offline verification in `alexandria-verify`: pinned genesis/instance → block/commit linkage → state/result proof → exact accepted set → all required evidence → deterministic tally → effect identity. Return distinct invalid/pending/verified outcomes with bounded reasons.
4. Add persistent client submission states such as Draft, Queued, IncludedPendingProof, Submitted, ExpiredUnsubmitted, AwaitingClose and Finalized; use existing UI naming conventions. Only a fully verified receipt allows Submitted. Use stable intent IDs and bounded retries across endpoints; a different endpoint must not cause a new vote.
5. Lock/restart must retain queued intent and consent-bound exact bytes privately without submitting changed content. Consent cannot be inferred from a cached unrelated opt-in. A user declining required disclosure may keep learning.
6. Export an offline verification bundle containing the necessary public proof evidence. The exporter must not accidentally include unrelated private credentials or secret material.

**Required tests:** valid independent offline verifier succeeds with networking disabled; missing manifest page stays pending; tampered/drop/substitute vote fails; wrong proof height or algorithm fails; endpoint lies about success and UI remains queued/pending; retry/restart preserves one intent; finalization waits for valid close; expired unreceipted vote is not counted.

**Exit:** actual receipt and close proofs verified by a separate client process, with no trust in an HTTP success response or mutable database flag.

### G05 — Seven-node fault testing and hosted committee operations

**Dependencies:** G04; paid hosting additionally O01/Q03. **Scope:** committee harness, deployment/ops, proof API.

1. Generate seven distinct validator consensus keys and separate governance keys, with real founding acceptance/proofs and explicit CometBFT genesis binding. Keep validator private keys and signing state out of the public world specification and ordinary persona exports.
2. First run seven controlled local processes with separate persistent directories and network fault injection. This is a hermetic verification environment, not the hosted demo replacement.
3. Test five online → progress; four online → no progress; restore a fifth after three nodes were down → progress resumes safely. Test a 4/3 partition cannot produce conflicting final outcomes. Do not claim liveness with only four nodes.
4. Inject restarts/crashes at consensus signing-state persistence, ABCI Commit, proof materialization and receipt persistence boundaries. Corrupt/restore storage deliberately in disposable tests to prove fail-closed behavior. A restarted signer must never sign conflicting blocks at the same height/round.
5. Bound authenticated submission/proof endpoints and per-subject/resource use. Authentication must be based on the allowed learner/protocol identity rather than making cloud login a new voting authority. Learners do not access raw administrative CometBFT RPC.
6. Deploy seven named apps across at least three approved regions with persistent daemon and ABCI state. Disable auto-stop for quorum-critical nodes. Secure peer/admin ports and key access. Pin binaries/images/configuration and genesis per node.
7. Monitor height agreement, participation/quorum, pending proposals, unhealthy clocks, disk, proof availability, double-sign refusal and backup health. Show the single-operator demo label in the app and operations output.
8. Backups and failover must not clone an active validator identity or rewind anti-double-sign state. Define one active signer per key, fencing, recovery procedure, and new-instance reset rule. Test restore on disposable identities before relying on it.

**Gate:** automated seven-process partition/crash suite; hosted health and receipt/close checks on the exact seven deployed instances; key custody/clock-state/recovery evidence. No production-independent-committee claim.

### G06 — Governance UI and first real effects

**Dependencies:** G04/G05, T01/T03, W03. **Scope:** app pages/composables/commands, verifier, plugin/taxonomy consumers, world builder.

1. Keep existing explicit genesis preview/pin/locator flow. Show complete material trust facts and require actual confirmation; do not auto-pin a world-exported persona's committee.
2. Implement proposal list/detail, exact vote/evidence preview, persistent submission states and certificate inspection (instance, policy, heights, signer participation, outcome). Provide meaningful pending/error recovery without success-looking placeholders.
3. Apply `plugin_endorsement` only from a verified final effect bound to the exact plugin and grader content identities and applicable scope. Retain the built-in provenance distinction. Do not route it through the removed event-supplied committee-key API.
4. Apply `taxonomy_amendment` only after fetching/validating the exact committed diff against its declared base version. Validate IDs, references, cycles/constraints as appropriate and apply atomically. Missing diff bytes mean a certified effect awaiting content, not arbitrary local modification.
5. Store final certificate/effect identity and applied marker atomically; duplicate receipt, reconnect or restart cannot apply twice. UI reads verified projections; forged local rows do not activate effects without backing certificate verification.
6. Add world ensure-steps for one accepted proposal, genuine eligible votes, one close, and both real effect kinds. World code goes through public core/client APIs and actual committee endpoints, never direct committee-state writes.

**Required tests:** declined consent sends no vote/evidence; exact preview matches upload; unrelated credential never disclosed; forged/old/wrong-instance certificate changes nothing; repeated effect applied once; taxonomy diff mismatch fails; endorsed plugin becomes eligible only after final verification; unavailable committee leaves persistent pending UI.

**Exit:** learner device pins genesis, votes, receives a real receipt, verifies a final outcome offline and observes its authorized effect. Repeat on all five platforms in V01.

## 11. M5 — complete the world and verify all platforms

### W04 — Full persona world, reset and demo scripts

**Dependencies:** W03, G06; H03 extensions as each persona requires.

Expand the already working minimal world in separate scenario slices. For each slice, use the same core operations as the real product, add audit coverage, prove rerun idempotency and extend paired-history coverage before moving on.

| Scenario | Real operations/evidence required |
| --- | --- |
| Second instructor | Separate real keys/registration, authored signed course, policy-scoped qualification authority; no shared arbitrary instructor signing key |
| Four learner states | New; in progress; completed and anchored; credential-rich with talent index. All progress comes from actual attempts and all claims have truthful provenance. |
| Guardian/minor pair | Real invite, one-time acceptance, signed guardian/link evidence, proper account gating, authenticated activity sync/revoke; no directly seeded permission row |
| Classroom | Real owner/create/join/approve/member/key distribution, encrypted messages and ownership checks; hosted owner available; recovery after missed membership/key events |
| Sponsor | Real tenant/IdP login, explicit demo hosted org-key custody, signed role spec, local run, exact release, cloud verification, withdrawal and export |
| Three registry founders | Distinct protected founder keys and verified network-bound bootstrap registry; no fabricated registration rows |
| Seven committee members | G05 genesis and node identities, actual consent/qualification where they vote, G06 proposals/effects; label common operator |
| Tutoring | Manual two-device audio/video session using actual permissions/media stack; no automated claim of verified video from a headless stub |

All privileged opinion/qualification demos use Q01's approved issuer policy. Initial accepted issuer identities are derived from the protected demo instructor keys and included in explicit scope policies/genesis; do not auto-trust every course discovered on the network. A fresh user with only self-claims still has working learning/onboarding and an honest explanation of unmet privilege requirements.

Complete `export-persona` with a manifest of transferable history, original source artifact IDs, network/instance and pairing expiry. Never export committee/operator secrets through the normal learner export command. For active exported personas, enforce the one-writer demo rule.

Implement reset only after a read-only reset plan:

1. Verify exact world ID and owned resource IDs; refuse broad reset against shared/unlabelled service resources.
2. Stop persona writers and scheduled workers; obtain a consistent encrypted source/journal backup and public ledger checksum.
3. Reconcile all known signed chain operations. Preserve unknown operations and refuse destructive removal of their sole recovery state. Reuse verified chain registrations/anchors through their original identities; do not assume a missing local DB means a transaction never happened.
4. Reset only that world's profiles/Neon branch/service namespaces. Do not wipe a shared relay volume to reset one persona; either use a dedicated demo relay instance or retain shared registry history. Already-signed receipts remain historical evidence even if a local registry DB is removed.
5. A committee restart/reset follows G05's signer-state rules and a new chain/instance identity where applicable, with new explicit client pinning. Do not preserve a misleading old trusted instance while starting a different history.
6. Rebuild and audit; compare intended semantic artifact identities and count actual requests/fees. Resumable partial reset is explicit in the ledger.

Create demo scripts as executable scenario definitions plus human-readable steps after documentation approval: learner, instructor endorsement, guardian/minor, classroom, sponsor/talent, and governance certificate inspection. Scripts begin with `audit` and identify the same world/release build ID.

**Required tests:** each scenario's negative authorization case; repeated full build no duplicates; interrupted reset resumes/refuses safely; unknown chain outcome prevents unsafe deletion; fresh device restore for each required scenario; changed world spec cannot silently reuse incompatible artifacts; reset cannot delete unrelated tenants/profiles/relay history.

**Exit:** full requested world is real, hosted and manually demoable. Every authoritative claim audits; every cache can be explained/rebuilt; listed unsupported history is not silently presented as restored.

### V01 — Five-platform smoke matrix on one build identity

**Dependencies:** M3 baseline, then repeat affected cases through G06/W04. **Read:** current desktop/mobile shared workflows, platform config, `scripts/android-build.sh`, platform media patches. Begin compilation/runtime smoke early; do not discover mobile failures only at M5.

Each result records app revision, verifier revision, built feature set, OS/device, packaged artifact hash, network configuration digest, world ID, service image/revision manifest and committee instance if used.

| Platform | Minimum evidence |
| --- | --- |
| macOS | Packaged app on actual hardware; complete learner/restore/cloud/governance journey; profile and media cleanup |
| Windows | Native Windows build and manual packaged-app run; correct supported tutoring feature set and path handling; no claim based on macOS cross-compilation alone |
| Linux | CI native build plus supported `tauri-driver` automation for core UI; packaged runtime smoke; headless world/committee execution without GUI dependencies |
| iOS | `aarch64-apple-ios` compile/lint with `tutoring-video-ios`, actual device run, foreground/background/lock transitions, offline proof vector and native media FFI |
| Android | Current NDK recipe and `tutoring-video-android` where required; emulator with host GPU plus physical-device smoke; suspend/resume, permissions and native media/proof tests |

Required scenarios on every supported platform:

1. Fresh empty profile/create/restore, offline start and course viewing.
2. DNS relay connection, verified catalog/course fetch, graceful network loss.
3. Real assessment, self-completion, instructor endorsement and truthful pending/confirmed witness states.
4. Cloud proof/consent/release/withdraw using actual configured services; OIDC browser login where relevant to the cloud console.
5. Pairing/history restore and offline credential verification with required evidence captured.
6. Genesis review/pin, queued vote, timely receipt, pending quorum outage, resumed finalization and offline close verification.
7. Profile A lock while SQL/HTTP/blob/native work is active, private content hidden immediately, cleanup complete before B opens, stale callbacks cannot reveal or mutate A/B data.
8. Suspend/resume or process termination around a pending operation; recovery preserves original transaction/intent identities.
9. Tutor camera/mic permission grant/refusal, start/stop/restart, actual two-device session; no callbacks into dropped FFI objects.
10. Display of supported scripts/fonts offline, localized pending/error states and basic accessibility of consent/proof screens.

**Exit:** each required cell has a pass/fail/blocked record and evidence. An unavailable physical device remains blocked. Cross-target Clippy is valuable but not device acceptance.

### V02 — Performance and resource measurement

**Dependencies:** representative M3 world, N03/X02; full dataset pass after W04. **Read:** `examples/db_contention_baseline.rs`, original D2 measurement contract, executor metrics and production workload paths.

1. Measure current p50/p95/p99 queue wait, SQL execution, IPC completion, dashboard usability, lock private-view hiding, cleanup time, unlock time, RSS, CPU, disk usage and sync recovery. Separate cold/warm runs and network delay from local work.
2. Use the original proposed workloads as reproducible test inputs, not approved SLAs: learner 10k catalog/25k credentials/8 controlled peers; instructor 100k catalog/250k credentials and a representative signed governance history/64 controlled peers; steady 10 events/s and burst 100 events/s; content 1/16/64 MiB with one/four transfers.
3. Generate valid signed/related performance fixtures through test builders or archived real artifacts. Do not benchmark mostly empty JSON rows or bypass the verification path under load. Record generation cost separately from steady-state operation.
4. Verify bounded memory/queues and background progress under interactive contention. Oversized/backpressured inputs must fail or recover explicitly, not silently accumulate.
5. Include idle and active per-persona headless resource use, cloud proof/DB load, and seven-node committee resource use/proof sizes. Use results to size O01 instead of guessing all nodes fit the smallest machine.
6. Propose concrete physical-device budgets from measured results for the user's still-open D2 performance choice. Preserve the selected executor capacity/fairness unless evidence supports a specifically approved change. Do not weaken key derivation, encryption, validation or language support for benchmark numbers.

**Exit:** reproducible fixture/commands and measured tables with environment, sample count, distributions and limitations. Accepted budget decisions recorded separately. Unapproved targets are not labelled passed or failed product SLAs.

### V03 — Adversarial end-to-end and release gate

**Dependencies:** W04, G06, C02, V01; relevant performance evidence from V02.

Run the following against exact release candidates, with controlled destructive scenarios isolated to demo/test resources:

- Forged/tampered/replayed/wrong-network course, credential, attestation, username response, role spec/result, cloud request, governance receipt/close/effect.
- Q01 two-identity arbitrary-course qualification attack and cross-subject/cross-scope/cross-tenant authorization attempts.
- Cloud replay across concurrent instances, cache pressure, process restart, database outage and duplicate mutation retry.
- Dropped status/classroom updates under bounded ingest overload, followed by authenticated recovery.
- Profile lock/switch and process crash between checkpoint/send/projection; no replacement POST and no cross-profile writes.
- Lost/expired pairing code, wrong-user restore, partial history, missing blobs/status evidence and changed policy; no auto-pinning or manufactured history.
- Five-validator progress, four-validator stall, 4/3 partition safety, restored-quorum recovery, unhealthy clock, disk full, daemon/ABCI crash and validator restore without double-signing.
- Offline proof verification with required evidence complete, missing and tampered; freshness claims remain accurate.
- Evidence consent decline, withdrawal and export; no unrelated private credential/frame leak in network/log/export output.

Run all applicable gates in section 13. Capture the actual feature/platform matrix and exclusions. No new warning suppression, ignored tests, trusted SQL fixtures or feature flags may be added to obtain a green release.

**Exit:** release manifest plus reproducible verification record; unresolved failures block the affected release claim. Source completion does not erase hosted/device blockers.

## 12. M6 — integrate, document, and hand off

### R01 — Final review, documentation and repository integration

**Dependencies:** all required source packages and claimed hosted/device gates.

1. Review the whole resulting change against user requirements and the removed-feature inventory. Search for legacy authority, fake seeding, public plaintext fallbacks, old proof formats, raw score/verification mutation, and accidental generated-file drift.
2. Review the release manifest across APP/CLOUD/RELAY/MONITOR and any new deployment repositories. Confirm the cloud's verifier dependency is the exact tested format revision and the distributed app network file matches actual services/trust identities.
3. Freeze verifier 0.2.0 formats/vectors after committee and service contracts pass. Publishing a package requires the relevant existing user authorization; do not publish merely because a version field changed. If published, pin cloud and rerun standalone contract/build checks.
4. Prepare exact documentation updates for approval: architecture, protocol, security audit, schema, credential/proficiency semantics, stake registry, username registry, settings/network disclosure, relay/monitor/cloud READMEs, headless/demo commands, pairing history coverage, validator operations and reset/recovery.
5. Preserve historical audit statements with dates/status, rather than rewriting them as if the previous code never existed. Update current AGENTS/README guidance only within approved scope; remove outdated unsafe or misleading examples once authorized.
6. Create scoped commits and draft review artifacts according to each repository's process. Keep no AI attribution and use existing contributor/DCO conventions. Do not bundle dirty unrelated cloud-UX work or `.DS_Store` into a remediation commit.
7. Before merge/deployment, show the actual revisions, tests, topology/cost, artifacts and unresolved limitations. Reuse authorization already present; request only genuinely missing final action/decision. Do not treat this planning request alone as authorization to publish a live system.
8. Keep rollback instructions alongside the release artifact, preserving irreversible external state. A Git rollback cannot undo a public disclosure, signed relay receipt, chain transaction or consensus signing history.

**Final done condition:** the user can reproduce the hosted demo, inspect real proofs, restore documented history, and run the complete scenario set on the verified platform matrix, with no undocumented policy/authority fallback. If OIDC/budget/device inputs remain unavailable, state exactly which code is complete and which deployment/runtime acceptance remains blocked.

## 13. Commands and verification gates

### 13.1 Safe command execution

Run each command from its stated repository root and inspect its actual exit code. Redirecting output to a log is fine; a later `tail` succeeding must not conceal a failed preceding command. Record the original exit status separately. Do not interpolate secrets or untrusted strings into shell code.

Examples below are existing commands unless explicitly labelled proposed. Do not execute placeholders literally. Prefer the existing package scripts/CI recipes over inventing compiler flags. All-target host checks do not enable every platform/feature.

### G-APP-RUST — app workspace

From APP, using the verified installed Rust 1.91.0 toolchain:

```sh
cargo +1.91.0 fmt --all -- --check
cargo +1.91.0 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.91.0 test --workspace --locked
git diff --check
```

Focused examples (choose those affected by the package):

```sh
cargo +1.91.0 test -p alexandria-node --lib cardano::submission --locked
cargo +1.91.0 test -p alexandria-node --lib profile:: --locked
cargo +1.91.0 test -p alexandria-node --lib content_store:: --locked
cargo +1.91.0 test -p alexandria-node --lib p2p::inbound --locked
cargo +1.91.0 test -p alexandria-node --test guardian_e2e --locked
cargo +1.91.0 test -p alexandria-node --test e2e_vc --locked
cargo +1.91.0 test -p alexandria-verify --locked
```

After deletion/extraction, update focused filters to actual test targets. A command matching zero tests is not a successful regression reproduction. Use CI's Linux `tutoring-video-static` recipe for that configuration; a default macOS Clippy pass is not coverage of it.

### G-APP-WEB — frontend and boundary guards

From APP:

```sh
npx vue-tsc -b --noEmit
npm test
npm run build
node scripts/check-tauri-commands.mjs
node --test scripts/check-tauri-commands.test.mjs
node scripts/i18n/check-parity.mjs
node scripts/i18n/check-no-raw-text.mjs
node scripts/check-csp-style-hashes.mjs
```

Run the new binding reproducibility check once X01 implements it. Use `npm ci` for a clean dependency install if needed; do not run it repeatedly on an unchanged environment. Do not replace the lockfile gratuitously.

### G-MOBILE — cross-target and device checks

Use `.github/workflows/mobile-shared.yml`, platform Tauri configuration and `scripts/android-build.sh` for actual SDK/NDK/deployment target setup. Resolve installed tools and target versions; do not copy a stale absolute SDK path from a chat log.

After valid target environment setup, indicative checks are:

```sh
cargo +1.91.0 clippy -p alexandria-node --lib --target aarch64-apple-ios --features tutoring-video-ios --locked -- -D warnings
cargo +1.91.0 clippy -p alexandria-node --lib --target aarch64-linux-android --features tutoring-video-android --locked -- -D warnings
```

Do not set the generic `SYSROOT` environment variable to the Android NDK directory: the Clippy driver interprets it as a Rust sysroot. Use the existing NDK recipe and target-specific compiler/linker configuration. Simulator/device and emulator/physical results are separate. Verify current target/feature names after any manifest change.

### G-RELAY

From the selected RELAY implementation worktree:

```sh
cargo +1.91.0 fmt --all -- --check
cargo +1.91.0 clippy --all-targets --locked -- -D warnings
cargo +1.91.0 test --locked
cargo +1.91.0 audit --file Cargo.lock
docker build -t alexandria-relay:rebuild-review .
```

Also run the repository's current CI/release toolchain gates; if its declared MSRV differs from the common toolchain, verify the supported MSRV rather than silently bumping it. Container build/start and persistent-volume restart tests are separate.

### G-MONITOR

From MONITOR/observer:

```sh
cargo +1.91.0 fmt --all -- --check
cargo +1.91.0 clippy --all-targets --locked -- -D warnings
cargo +1.91.0 test --locked
cargo +1.91.0 audit --file Cargo.lock
```

From MONITOR/web:

```sh
npx vue-tsc -b --noEmit
npm test
npm run build
```

Build/start the repository Docker image following its CI recipe, then test public health, authenticated APIs, throttling and configured network observation.

### G-CLOUD

Read the selected CLOUD worktree's `.github/workflows/ci.yml` and `docker-compose.yml`. Provision a disposable local Postgres 16 test database with separate owner and restricted application roles. Set `ADMIN_DATABASE_URL` and `DATABASE_URL` through protected environment configuration. Do not run integration tests against the hosted/shared demo database unless a specific destructive test environment is selected.

From CLOUD:

```sh
cargo +1.91.0 fmt --all -- --check
cargo +1.91.0 clippy --all-targets --locked -- -D warnings
cargo +1.91.0 test --locked
cargo +1.91.0 audit --file Cargo.lock
docker build -t alexandria-cloud:rebuild-review .
```

From CLOUD/web:

```sh
npx vue-tsc -b --noEmit
npm run build
npm audit --audit-level=high
```

There is no current cloud-web `npm test` script in the reviewed worktree. Do not claim it ran. Add focused UI/browser coverage where C03/C04 consent/auth behavior needs it, using an explicit test harness. For Rust integration tests, search the log for `SKIPPED`/missing DB diagnostics and require actual execution. A local no-DB convenience skip is not G-CLOUD success.

A real Postgres test proving tenant isolation is required; a mocked SQL result cannot establish RLS. Container startup must prove migrations run with the owner role and requests run with the restricted role, and that the console assets are actually present.

### G-HOSTED — actual service contract tests

Implement `APP/src-tauri/tests/cloud_contract.rs` or the equivalent extracted-core integration target. Hosted tests are explicit opt-in (`#[ignore]` plus configured URLs/network/world IDs), not accidentally run by ordinary unit tests. Read-only health/audit and state-changing contract cases must be separated and operate on a dedicated test world/tenant.

Once implemented, the expected invocation is conceptually:

```sh
cargo +1.91.0 test -p alexandria-node --test cloud_contract --locked -- --ignored
```

Required protected configuration includes the exact preprod cloud URL, service/network identity, dedicated tenant/world identity and test credentials. The proposed `ALEXANDRIA_CLOUD_URL` alone is insufficient to identify safe mutation scope. Test code must fail if the configured instance is not explicitly preprod/test-owned.

Cases: holder directory; talent publish/update/remove; presentation share/expiry; role spec/result; evidence release/withdraw; export; wrong-subject/tenant/audience/network; cross-instance replay; restart persistence. Do not mark these passed against a local mock or DEV_AUTH.

### G-COMMITTEE — actual consensus and proof gates

Once the new package exists, use its real package name/targets from Cargo metadata. Required gates: strict fmt/Clippy/unit tests, golden-vector verification, daemon/ABCI handshake/restart, seven-process partition/crash tests, independent offline bundle verification, target builds and device proof vectors, then hosted seven-node smoke.

The test harness must make node-count/partition/crash scheduling reproducible and record logs by public node identity. Separate random test runs from a deterministic failing seed. No test may obtain success by reducing quorum, freezing a node's wall clock without its protocol evidence, or writing accepted outcomes directly to storage.

### G-AUDIT — dependency and build provenance

Refresh Rust and JS advisories from current official advisory sources. Record version, feature/target reachability and next action for unresolved issues. Do not append broad ignores or treat every transitive warning as remotely exploitable. Preserve necessary native patches until a tested replacement exists. Pin final artifact hashes/revisions; include compiler and platform features in build provenance.

## 14. Traceability, exclusions, and stop rules

### 14.1 Coverage of the attached rebuild plan

| Original phase/work | This plan |
| --- | --- |
| Baseline/WIP, close retry, proxy fix | F01–F04 |
| Legacy governance, compatibility, baseline schema, fake seeds | D01–D03 plus T01 |
| Completion/plugin/eligibility security closures | T01–T04 |
| Paused journal and inbound executor work | F02/F03, with recovery completed by N03 |
| Network profiles, relay and monitoring | N01/N02 |
| Cloud hosting/auth/shared verifier/contracts | C01–C04, O01, G-HOSTED |
| Headless core, real world, serve/export/reset | H01–H03, W01–W04 |
| Real CometBFT governance and effects | G01–G06 |
| Five-platform verification/demo/docs | V01–V03, W04, R01 |
| Earlier unfinished complete IPC typing | X01 |
| Earlier bulk media/private-asset lifetime and performance | X02, V02 |
| Earlier as-of reputation snapshot semantics | T03 |
| Earlier durable republication/loss recovery | N03 |

### 14.2 Explicitly deferred

- Mainnet deployment and actual production-independent committee recruitment.
- Elections, runoffs, epoch transitions, dynamic validator membership and governance recovery authorities.
- Credential challenges/escrow, Sentinel prior ratification and old content-ratification governance.
- Evidence pruning/retention changes beyond lifetime retention for the demo instance; no claim of indefinite production retention economics.
- A broad redesign of cloud business/billing workflows beyond integration/security fixes and preservation of the existing cloud-UX work.
- General multi-device distributed wallet-spend coordination; the demo uses a controlled single writer per persona.
- New zero-knowledge/selective-disclosure schemes, threshold-signature schemes or a custom consensus implementation.
- New global operator-controlled qualification authority, background learner monitoring, or penalties for diagnostics/evidence withdrawal.

### 14.3 Stop immediately for the affected operation when

- Existing source/secret/state would be destroyed without a verified preservation path.
- The configured network/tenant/world/committee identity is unknown or not the authorized target.
- A proposed shortcut would change issuer trust, quorum, cutoff, privacy/consent, scoring policy, or chain recovery semantics.
- A receipt/proof needs uncommitted, unavailable or future information that the deterministic application cannot verify.
- Required trust-root or certificate format values are placeholders.
- Real chain submission outcome is unknown and the next action would erase its only recovery material or build a replacement.
- A required test is skipped, crashes, matches zero cases or is run on the wrong platform while being used as completion evidence.
- Hosted provisioning would incur unspecified charges or requires an unselected OIDC provider/account.

Report a precise blocker and continue an independent ready package. Do not expand the scope into a new architectural rewrite to avoid asking about a genuinely unresolved product decision.

## 15. Handoff protocol for small-model execution

### 15.1 Session-start prompt

Use this when assigning an implementation session; replace the package ID and recorded paths with actual values:

```text
Execute package <ID> from docs/rebuild-execution-plan.md.
Read sections 1, 3, 4, the package itself, its dependency completion records,
and the relevant verification gate before editing. Read applicable AGENTS.md.
Inspect Git status and preserve unrelated work. Do not redo completed packages.
Use the shared core/verifier/executor boundaries, not a second implementation.
Implement the smallest coherent slice, run meaningful focused regressions,
then the required package/milestone checks. Record exact source revisions,
commands, exit codes, remaining blockers and the next ready package.
Do not call a mock/skip/host compile a hosted or device pass.
Do not modify unrelated documentation without its required authorization.
Ask only if a material unresolved decision actually blocks this package.
```

### 15.2 Progress record schema

Proposed task artifact `execution-state.json` (create during F01 in the agreed task-artifact location; not an existing file):

```json
{
  "plan_version": 1,
  "network_id": "preprod",
  "world_id": null,
  "repositories": {
    "app": {"path": "", "branch": "", "head": "", "dirty": true},
    "cloud": {"path": "", "branch": "", "head": "", "dirty": true},
    "relay": {"path": "", "branch": "", "head": "", "dirty": false},
    "monitor": {"path": "", "branch": "", "head": "", "dirty": false}
  },
  "decisions": {"Q01": "approved", "Q02": "approved", "Q03": "provider intentionally open; budget pending"},
  "packages": {
    "F01": {
      "status": "not_started",
      "source_complete": false,
      "verification_complete": false,
      "commits": [],
      "changed_files": [],
      "invariants_checked": [],
      "checks": [],
      "blockers": [],
      "next_action": "preserve source snapshots"
    }
  },
  "next_ready_package": "F01"
}
```

Allowed package states: `not_started`, `in_progress`, `source_complete`, `verified`, `blocked`. A source-complete package with missing hosted/device evidence is not verified. Each check entry contains `command`, `cwd`, `revision`, `features`, `platform`, `exit_code`, `test_count`, `ignored_count`, `log_path`, and `observed_result`. Use null for unavailable data, not fabricated zeros.

Store no secrets in this record. A public artifact CID or transaction hash may be included when intended; private learner evidence and mnemonic output may not.

### 15.3 Session-end report

```text
Package and slice:
Repositories / before-and-after revisions:
Concrete behavior changed:
Files changed:
Security/lifecycle invariants preserved:
Focused tests and actual results:
Milestone/platform checks and actual results:
Skipped/unavailable checks:
Documentation assessment and authorized changes, if any:
Dirty or unfinished work preserved:
Remaining blockers / unresolved decisions:
Next exact action and ready package:
```

### 15.4 Review checklist before marking a package verified

- Does the code implement the named behavior through the normal product path?
- Are caller identity, network, policy, exact artifact version and historical proof bindings checked where relevant?
- Do local and remote adapters share the rule rather than duplicate a weaker version?
- Are SQL multi-write changes atomic and external operations outside locks?
- Can lock, cancellation, crash, duplicate delivery and retry leave a recoverable state?
- Is negative evidence distinguishable from missing/busy/error?
- Are trust labels and UI states backed by verified artifacts?
- Did all required tests actually execute on the claimed environment?
- Are no secrets, unrelated changes, fake-authority rows or warning suppressions included?
- Is the next model given a recoverable snapshot and a precise next step?

## 16. Reference material and final acceptance

Local evidence and prior decisions:

- `APP/docs/assessment-remediation-plan.md`: historical D1–D18 decisions and progress; new disposable-data and demo single-operator decisions override their older conflicting portions as stated here.
- The user's attached `Alexandria rebuild plan`: scope and original phases, all traced in section 14.1. This execution plan is self-contained and must not depend on the attachment remaining in a temporary upload directory.
- Applicable root/subdirectory AGENTS; actual Cargo manifests, CI and platform recipes; protected F01 snapshots and original source revisions.

Technical references verified during the review/planning session:

- [CometBFT v0.40 ABCI methods](https://github.com/cometbft/cometbft/blob/v0.40.0/spec/abci/abci%2B%2B_methods.md): application execution/result/state commitment semantics. Verify exact selected release behavior in G01.
- [CometBFT v0.40 BFT time](https://github.com/cometbft/cometbft/blob/v0.40.0/spec/consensus/bft-time.md): useful background; it does not itself implement Alexandria's distinct five-member receipt-time policy.
- [Rust ABCI application interface](https://github.com/cometbft/tendermint-rs/blob/main/abci/src/application.rs): upstream was using v0_38 ABCI types when inspected. Recheck and pin actual versions; do not infer runtime compatibility from this link alone.
- [Keycloak](https://www.keycloak.org/) and [authentik editions](https://goauthentik.io/pricing/): open-source/free-software provider options, deliberately unselected by the user. Hosting costs are separate.

Final acceptance is the conjunction of all required outcomes, not a line count or test count:

1. Source work is durable, scoped and reviewable across all repositories.
2. A fresh profile has working empty onboarding, real built-ins and zero fabricated achievements/authority.
3. Privileges require the approved scope policy and verified evidence; authentic lower-trust claims remain honestly visible.
4. Lock/restart/overload preserve isolation, bounded resources and original chain/consensus operation identities.
5. Hosted relay/cloud/observer/world services use the correct network, persistent state, authentication and explicit privacy disclosures.
6. A real OIDC login and cross-instance cloud contract tests pass on the selected deployment.
7. The complete real persona world builds/resumes/audits without duplicate semantic effects and exports documented history through real pairing/sync.
8. A real seven-node committee produces independently verified timely receipts, complete close proofs and the two supported effects; four nodes cannot make progress or trigger a fallback.
9. The same released world/build identity is verified on macOS, Windows, Linux, iOS and Android with actual runtime evidence.
10. Documentation and final integration are completed within the user's granted scope, with remaining limits stated explicitly.
