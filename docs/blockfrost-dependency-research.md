# Blockfrost dependency: is it needed, and what replaces it?

Status: research snapshot (2026-11-10); provider tiers, limits and crate versions reflect the research date.

**Scope:** every Blockfrost touchpoint across the five repositories, the minimal
set of chain operations the app actually performs, and alternatives judged
against that set. Code claims cite `file:line` (paths relative to `alexandria/`
unless prefixed with another repository). External claims cite primary sources
(vendor docs, specs, source code). Research only: nothing was built, configured
or submitted, and no request used a Blockfrost key.

## Bottom line

1. **Blockfrost is not needed by the protocol, but it is a hard dependency of the
   code today.** Every chain feature in the app goes through one client with a
   hard-coded preprod base URL (`src-tauri/src/cardano/blockfrost.rs:168`), no
   failover and no second backend. The `Ledger` port
   (`src-tauri/src/cardano/ledger.rs:23-41`) covers only anchoring; five other
   features call `BlockfrostClient` directly.
2. **No key ships in release builds, so in a packaged app every chain feature is
   off** until the user signs up at Blockfrost and pastes a project id
   (`blockfrost.rs:21-33`, `README.md:52`). That avoids key abuse but makes the
   decentralised anchoring path depend on each user holding a third-party
   account.
3. **The app needs about ten operations.** Only three of them strictly need an
   indexer: UTxOs at an address, latest metadata transaction by label, and assets
   minted under a policy. Submission, evaluation, tip and inclusion proofs can come
   from the Cardano network itself (`pallas-network` N2N, local `uplc`
   evaluation, wall-clock slots, Mithril).
4. **The double-spend retries seen on preprod are partly ours to fix.** Wallet
   coin selection always picks the largest ADA-only UTxO
   (`src-tauri/src/cardano/anchor_tx.rs:349-358`) and nothing excludes inputs of
   our own in-flight transactions. Any indexer lags the mempool, so the fix is to
   track spent outpoints locally from the submission journal, whichever provider
   is used.
5. **Recommended path:** widen the `Ledger` port to every caller, move provider
   endpoints into the network profile as an ordered list, then add backends in
   this order: Dolos MiniBF (Blockfrost-compatible, self-hostable, keyless),
   Koios (keyless public tier), and Blockfrost as an optional user-key backend.
   Then add N2N submission to public relays and local script evaluation. Cloud,
   relay and monitoring need nothing.

## 1. Current usage in code

### 1.1 Client, configuration and key handling

| What | Where | Notes |
|---|---|---|
| Client | `src-tauri/src/cardano/blockfrost.rs` (823 lines), `reqwest` 0.12 (`src-tauri/Cargo.toml:142`) | `project_id` header on every call |
| Base URL | `blockfrost.rs:168` `PREPROD_BASE_URL` constant | **Not in the network profile.** `preprod.json` holds only `cardano_network` and `cardano_network_magic` (lines 5-6) plus anchor-sponsor coordinates. Test-only override at `blockfrost.rs:184-196` |
| Key resolution | `blockfrost.rs:21-33` | Per-device setting `cardano.blockfrost_project_id` (`src-tauri/src/settings/registry.rs:352-358`), then the `BLOCKFROST_PROJECT_ID` env var |
| Env loading | `src-tauri/src/lib.rs:623-626`, `src-tauri/Cargo.toml:49-51` | `dotenvy` reads a local `.env` (dev only), a no-op in packaged builds |
| Baked into builds? | No `option_env!`/`env!` for the key; no CI secret in `.github/workflows/` | **No shipped key.** Release users must supply their own (`docs/building.md:124-132`) |
| Timeouts | `blockfrost.rs:133-153` | 10 s connect, 30 s total. A timed-out submit is journaled and reconciled, never treated as a rejection |
| Webview CSP | `src-tauri/tauri.conf.json:38` `connect-src … https://*.blockfrost.io` | Vestigial: no frontend code calls Blockfrost (`src/` has only a comment at `src/types/index.ts:1621`) |
| Feature gate | `src-tauri/src/commands/completion.rs:554-555`, `lib.rs:906-955`, `src-tauri/src/commands/p2p.rs:283-304` | "Chain configured" is defined as "a project id resolves" |
| Prior audit | `docs/audits/security-2026.md:415-421` (L-1) | The key in an env var is visible to same-user processes |

**Key-in-client concern:** it does not apply today, because nothing is shipped.
It would apply the moment a release embedded a project key. Blockfrost keys are
bearer tokens sent in a header (Blockfrost OpenAPI, "Authentication"), so
anyone could extract one from the binary. All installs would also share one
daily quota. The free Starter plan allows 50,000 requests/day
([blockfrost.io](https://blockfrost.io)). The completion observer alone polls at
least once a minute (`src-tauri/src/cardano/completion.rs:100-109`), or 1,440
requests per node per day, so about 35 always-on nodes would exhaust a Starter
key. Do not ship a key. Make the keyless backends below the default.

### 1.2 Endpoints and the features that depend on them

| Endpoint | Client fn (`blockfrost.rs`) | Purpose | App call sites (feature) |
|---|---|---|---|
| `GET /addresses/{addr}/utxos` | `get_utxos` :227 | UTxOs at address | `anchor_tx.rs:96`, `anchor_queue.rs:725` (credential anchoring: issuer wallet and sponsor pool); `username_anchor.rs:93` (username batch anchoring); `completion_tx_builder.rs:157` (completion mint); `p2p/registry_chain.rs:161` (stake-pubkey registry read); `plutus_spend.rs:119`, `stake_pubkey.rs:278` (examples only, see 1.3) |
| `GET /epochs/latest/parameters` | `get_protocol_params` :253 | Fees, cost models | Same builders as above (`anchor_tx.rs:97`, `username_anchor.rs:94`, `completion_tx_builder.rs:158`) |
| `GET /blocks/latest` | `get_tip_slot` :275 | TTL / validity window | Same builders plus `anchor_queue.rs:238` (batch expiry) |
| `POST /utils/txs/evaluate` | `evaluate_tx` :395 | Plutus execution units | `anchor_tx.rs:172` (sponsor-pool spend), `completion_tx_builder.rs:269`, `plutus_spend.rs:253` |
| `POST /tx/submit` | `submit_tx` :302 | Broadcast | Via `Ledger::submit` in `submission.rs:273-290` (anchors, completion, usernames); `completion_tx_builder.rs:436` (test) |
| `GET /txs/{hash}` | `get_transaction_receipt` :635 | Inclusion, `valid_contract`, `block_time` | `Ledger::receipt` (`ledger.rs:64`), used by `submission.rs:521-527` reconcile and `ledger.rs:81` `observe_anchor`; `username_anchor.rs:166` |
| `GET /txs/{hash}/cbor` | `get_tx_cbor` :600 | Signed bytes (anchor metadata, registry datum) | `ledger.rs:70`; `username_anchor.rs:176`; `p2p/registry_chain.rs:241` |
| `GET /txs/{hash}/utxos` | `get_tx_utxos` :543 | Inline datums, input addresses (provenance) | `completion.rs:166`; `relay_registry_chain.rs:68`; `p2p/registry_chain.rs:172` |
| `GET /metadata/txs/labels/{label}` | `get_metadata_by_label` :568 | Latest relay-registry tx | `relay_registry_chain.rs:52` (relay username-receipt issuers) |
| `GET /assets/policy/{policy}` (paged) | `list_policy_assets` :438 | Discover completion mints | `completion.rs:109` (completion observer, every 60 s tick) |
| `GET /assets/{unit}/history` | `get_asset_history` :509 | Mint tx of a witness | `completion.rs:162` |
| `GET /assets/{unit}/addresses` | `get_utxo_by_asset` :349 | State UTxO by token | **No caller** (dead) |

All `cardano/*` paths above are under `src-tauri/src/cardano/`, and `p2p/` is
under `src-tauri/src/p2p/`. Callers are wired from the 60 s background tick
(`lib.rs:906-1075`), profile startup (`lib.rs:339-358`, hourly by default per
`registry.rs:372-380`) and the relay-registry refresh (`commands/p2p.rs:283-306`).

### 1.3 Abstractions, scripts and other repositories

| Area | Finding |
|---|---|
| `Ledger` port | `ledger.rs:23-41`: `utxos`, `protocol_params`, `tip_slot`, `evaluate`, `submit`, `receipt`, `tx_cbor`. Implementations: `BlockfrostClient` (`ledger.rs:43`) and `FakeLedger` (tests). Used by anchoring and by `submission.rs` (`submit_once`/`reconcile`), so completion and username submission already go through it. The module doc already anticipates "direct relay submission and Mithril-certified reads" (`ledger.rs:4-6`; `docs/adr/0005-sponsored-batched-anchoring.md:26,37`; `docs/sponsored-anchoring-spec.md:209`) |
| Second port | `p2p/registry_chain.rs:124` `ChainFetcher`, with only `BlockfrostFetcher` (:140) implementing it |
| Bypassing the port | `username_anchor.rs:85,159,193`, `completion.rs:104-109`, `completion_tx_builder.rs:117+`, `completion_queue.rs:178`, `completion_recovery.rs:168`, `relay_registry_chain.rs`, `plutus_spend.rs:97-121`, `stake_pubkey.rs:264-280` take `&BlockfrostClient` directly |
| Governance | No in-app governance transaction. `plutus_spend::build_spend_*` is called only from `src-tauri/examples/`; `gov_tx_builder.rs:1-4` says the governance builders were deleted. `stake_pubkey::build_registration_tx` is example-only too (`src-tauri/examples/preprod_registration_roundtrip.rs:191`) |
| Operator tooling | `cardano/governance/deploy_blockfrost.py:18-19,49,133`, `cardano/governance/deploy_reference_scripts.sh:34,99,133,182`, `scripts/preflight.sh:28-29,70-73`, `scripts/env.completion.example:18`, and `src-tauri/examples/{deploy_completion,preprod_anchor,preprod_registration_roundtrip}.rs`. Each operator supplies their own key from the environment |
| Verifier crate | `crates/alexandria-verify/src/vc/anchor.rs:25-28`: "reads no chain", so the caller supplies the observed transaction |
| Pallas crates | `src-tauri/Cargo.toml:116-123`: pallas 0.35 `wallet`, `addresses`, `crypto`, `txbuilder`, `primitives`, `traverse`, `codec`. **`pallas-network` is not a dependency.** 0.35.1 exists; the current stable line is 1.4.0 ([crates.io](https://crates.io/crates/pallas-network)) |
| `alexandria-cloud` | No Cardano client (`alexandria-cloud/Cargo.toml:10-13`). Its `project_id` hits are Firebase (`alexandria-cloud/src/push/mod.rs:421-455`) |
| `alexandria-relay`, `alexandria-monitoring` | No Blockfrost or chain client. The relay only carries anchor references (`alexandria-relay/src/username_reg.rs:58`) |
| `website` | Copy only (`website/pages/technology.vue:11,308`) |

### 1.4 Observed operational issues (live preprod)

| Issue | Cause in our code / provider | Provider-independent fix |
|---|---|---|
| Transient connect/request timeouts | Single endpoint, no failover (`blockfrost.rs:168`) | Ordered endpoint list with failover; the 10 s/30 s deadlines stay |
| Address index lag: a just-spent UTxO still returned, causing double-spend (`BadInputsUTxO`) on retry | `/addresses/{addr}/utxos` reflects confirmed chain state only ([Blockfrost OpenAPI](https://github.com/blockfrost/openapi/blob/master/openapi.yaml)). Its `/mempool/addresses/{address}` shows only txs submitted through Blockfrost and is hosted-only. Our selector is deterministic: the largest ADA-only UTxO (`anchor_tx.rs:349-358`), so it re-picks the same input. The sponsor selector reseeds per tip (`anchor_tx.rs:304`) but also cannot see in-flight spends | Derive "pending spent" outpoints from the inputs of journaled, unconfirmed signed txs (`submission.rs` already stores exact bytes). Exclude them until confirmed or past `invalid_from_slot`. Optionally chain on our own pending change outputs |

## 2. Minimal required operation set

| # | Operation | Used by | Needs an indexer? | Indexer-free option |
|---|---|---|---|---|
| R1 | UTxOs at address (own wallet, sponsor pool, registration script) | All builders, registry | **Yes** (or a local node / own chain follower) | Own chain-sync follower from a known start point (heavy; see 2.6) |
| R2 | Protocol parameters | Fee/collateral calc | Partly | Changes only at epoch boundaries, so cache it. N2C only otherwise |
| R3 | Tip slot | TTL/validity | No | Wall clock: preprod slots are 1 s since Shelley (genesis `slotLength`), and the profile already carries a slot offset (`anchor_sponsor.slot_offset_secs`) |
| R4 | Evaluate execution units | Sponsor spend, completion mint | No | Local phase-2 evaluation with `uplc::tx::eval_phase_two` ([aiken `crates/uplc/src/tx.rs`](https://github.com/aiken-lang/aiken/blob/main/crates/uplc/src/tx.rs)). Aiken's workspace pins pallas 0.35, the same as the app ([Cargo.toml](https://github.com/aiken-lang/aiken/blob/main/Cargo.toml)) |
| R5 | Submit | Everything that writes | No | N2N tx-submission to public relays (section 3.5) |
| R6 | Inclusion receipt (+ `valid_contract`, block time) | Reconcile, anchor observation | Partly | Mithril tx-inclusion proofs (section 3.6), or N2N chain-sync/block-fetch watch for our own hashes |
| R7 | Tx CBOR by hash | Anchor and username verification | Partly | For our own txs the journal already has the bytes. For imported receipts the bytes could travel with the receipt, since `parse_anchor_tx` already checks hash equality (`ledger.rs:106-114`) |
| R8 | Tx outputs/inputs by hash (datums, provenance) | Completion, relay registry, stake registry | Yes, unless R7 bytes are available (decode with pallas) | `get_tx_cbor` + `pallas-traverse` covers outputs. **Input addresses need the spent outputs**, which still means an indexer |
| R9 | Latest tx by metadata label | Relay registry | **Yes** | None practical client-side |
| R10 | Assets under policy + mint history | Completion observer | **Yes** | None practical client-side |

The irreducible indexer surface is R1, R8 (input side), R9 and R10, all of them
reads. Writes (R4, R5) and liveness (R3) need no third party.

## 3. Alternatives

### 3.1 Coverage matrix (R1-R10)

| Option | R1 utxos | R2 params | R3 tip | R4 eval | R5 submit | R6 receipt | R7 cbor | R8 tx utxos | R9 label | R10 policy/history |
|---|---|---|---|---|---|---|---|---|---|---|
| Blockfrost hosted | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| Koios public | `address_utxos` | `cli_protocol_params` | `tip` | `/ogmios` `evaluateTransaction` | `submittx` | `tx_status`, `tx_info` | `tx_cbor` | `tx_utxos`/`tx_info` | `tx_by_metalabel` | `policy_asset_list`, `asset_txs`/`policy_asset_mints` |
| Dolos MiniBF | Y | Y | Y | **N** (gRPC `EvalTx` instead) | Y | Y | Y | Y | Y | `/assets/policy` Y; `/history` **N** (`/assets/{unit}/txs` instead) |
| Dolos UTxO RPC | `SearchUtxos` | `ReadParams` | tip in responses | `EvalTx` | `SubmitTx` | `WaitForTx`/`ReadTx` | `ReadTx` | `ReadTx` | N | predicate search only |
| Ogmios (+ node) | `queryLedgerState/utxo` by addresses | Y | Y | Y | Y | N (no history) | N | N | N | N |
| Kupo (+ node/Ogmios) | Y (patterns) | N | N | N | N | partial | N | datums/scripts | N | policy patterns |
| `pallas-network` N2N | N | N | header tip | N | **Y** | chain-sync watch | via block-fetch | via block-fetch | N | N |
| Mithril client | N | N | N | N | N | **certified inclusion** | N | N | N | N |

Sources: Koios paths from the preprod OpenAPI
([koiosapi-preprod.yaml](https://github.com/cardano-community/koios-artifacts/blob/main/specs/results/koiosapi-preprod.yaml));
MiniBF coverage from [docs.txpipe.io/dolos/apis/minibf](https://docs.txpipe.io/dolos/apis/minibf);
UTxO RPC methods from [query.proto](https://github.com/utxorpc/spec/blob/main/proto/utxorpc/v1alpha/query/query.proto)
and [submit.proto](https://github.com/utxorpc/spec/blob/main/proto/utxorpc/v1alpha/submit/submit.proto);
Ogmios from [ogmios.json](https://github.com/CardanoSolutions/ogmios/blob/master/docs/static/ogmios.json)
("restricted to some output references or addresses"); others below.

### 3.2 Per-option assessment

| Option | Hosting | Key? | Limits / resources | Mobile on-device client? | Maturity / licence | Notes |
|---|---|---|---|---|---|---|
| **Blockfrost** hosted | Hosted (single company) | Yes, per network | Starter 50,000 req/day, 1 project ([blockfrost.io](https://blockfrost.io)); 10 req/s per IP, burst 500 ([OpenAPI "Limits"](https://github.com/blockfrost/openapi/blob/master/openapi.yaml)) | Yes (HTTPS) | Production | Mempool endpoints hosted-only and Blockfrost-submitted txs only |
| Blockfrost RYO | Self-host | No | cardano-node + db-sync + Postgres; 64 GB+ RAM, 8+ cores, 250 GB+ SSD ([RYO README](https://github.com/blockfrost/blockfrost-backend-ryo)) | n/a (server) | Production | Too heavy for community operators |
| **Koios** | Hosted, distributed across instances ("no central instance", [koios.rest](https://koios.rest/)) | **Public tier: none**; Free/paid: Bearer token | Public 5,000 req/day, 100 req/10 s; Free (token) 50,000/day ([tiers](https://koios.rest/tiers.html)); 1,000-row pages, 30 s query timeout, **request body capped at 1 KB public / 5 KB registered** ([OpenAPI "Limits"](https://github.com/cardano-community/koios-artifacts/blob/main/specs/results/koiosapi-preprod.yaml)) | Yes (HTTPS) | Production; spec CC-BY-4.0 | Mainnet/preprod/preview. The 1 KB body cap may block `/ogmios` evaluation and possibly `submittx` of Plutus txs on the public tier (unverified for `submittx`). `queryLedgerState/utxo` is removed from its Ogmios proxy |
| **Dolos** | Self-host, light | No (operator may front with auth) | Under 2 GB RAM on mainnet ([performance](https://docs.txpipe.io/dolos/operations/performance)); ledger-only/sliding/archive storage profiles; syncs over N2N from a public relay, slowly ("few hours to several days"), or via Mithril/snapshot ([relay bootstrap](https://docs.txpipe.io/dolos/bootstrap/relay)) | No (server). Its APIs are mobile-friendly | Apache-2.0, v1.6.3 (2026-11-03) ([repo](https://github.com/txpipe/dolos)) | "Relies on honest upstream peers rather than performing full consensus validation" (README). MiniBF is Blockfrost-compatible, so our client works with a base-URL change. It forwards submitted txs upstream over N2N with `pallas-network` (`src/sync/submit.rs` in the Dolos repo) |
| UTxO RPC (spec + SDK) | Interface; served by Dolos, Demeter | Depends on server | gRPC/gRPC-Web | Yes (gRPC on mobile OK) | Rust crate `utxorpc` 0.14.0, Apache-2.0 ([crates.io](https://crates.io/crates/utxorpc)) | Typed, includes `EvalTx` and `WaitForTx` stages (acknowledged → mempool → network → confirmed). Lacks label/policy history queries, so it does not replace R9/R10 |
| **Ogmios** | Self-host beside a cardano-node | No | Full node resources | No | MPL-2.0, v7.0 for node 11.0.1 ([README](https://github.com/CardanoSolutions/ogmios)) | UTxO-by-address read from live ledger state (no indexer lag), plus evaluation with rich errors. No history |
| **Kupo** | Self-host, needs node/Ogmios (N2C) | No | 256 MB-2 GB RAM, up to ~220 GB disk on mainnet unpruned, ~12 GB pruned ([README](https://github.com/CardanoSolutions/kupo)) | No | Production | Pattern-based UTxO/datum index; pairs with Ogmios |
| **`pallas-network` N2N** | Embedded in the app; talks to public relays | No | TCP to relay | **Yes** (pure Rust TCP) | Apache-2.0; 0.35.1 matches our pallas line; a `pallas-network2` rewrite is in progress ([README](https://github.com/txpipe/pallas/blob/main/pallas-network/README.md)) | See 3.5 |
| **Mithril** client | Embedded; aggregator per network | No | Proof download only | Rust and WASM libraries | Production | See 3.6 |
| Maestro | Hosted | Yes (free account) | Paid "turbo submit" ([TS SDK](https://github.com/maestro-org/typescript-sdk)) | Yes | Production | Centralised; same shape as Blockfrost |
| Demeter | Hosted Ogmios, Kupo, Blockfrost-compatible API and UTxO RPC, "powered by Dolos", mainnet/preprod/preview ([demeter.run](https://demeter.run/)) | Yes (hostname label or `dmtr-api-key`) | Free tier; enrollment needed past 30 days | Yes | Production | Centralised, but Dolos-backed, so the same API can be self-hosted |

### 3.3 Koios

Koios is the only keyless public option and covers R1-R10 at preprod
(`https://preprod.koios.rest/api/v1`, per the OpenAPI `servers`). It is a good
default read backend for low-volume clients. Caveats: per-IP limits of 5,000/day
and 100 per 10 s (fine for one desktop, not for a heavy completion-observer
cadence). The 1 KB public body cap means writes and evaluation should not rely
on it; use N2N submit and local evaluation instead. Responses are PostgREST
JSON, so the app needs a second response mapper (not Blockfrost-compatible).

### 3.4 Dolos (and UTxO RPC)

Dolos is the decentralisation-friendly replacement for the *indexer* role:
light enough for any community member or org to run, keyless, and
Blockfrost-compatible, so `BlockfrostClient` works against it with only
base-URL and optional-auth changes. Gaps against our list: no
`/utils/txs/evaluate` (use local `uplc` or Dolos gRPC `EvalTx`), and no
`/assets/{unit}/history` (use `/assets/{unit}/txs`, or decode the mint tx found
via `/assets/policy`). Trust: Dolos follows upstream peers without full
consensus validation, so a client should prefer several Dolos instances (or
cross-check with Koios or Mithril) over trusting one.

### 3.5 Direct N2N with `pallas-network` (no local node)

| Capability over N2N to a public relay | Possible? | Basis |
|---|---|---|
| Tx submission | **Yes.** `PeerClient` has a `txsubmission` client over TCP ([facades.rs](https://github.com/txpipe/pallas/blob/main/pallas-network/src/facades.rs)), and Dolos submits upstream this way | Network spec §3.9: pull-based tx-submission v2 between nodes ([network-spec.pdf](https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec/network-spec.pdf)) |
| Rejection reasons | **No.** Only the node-to-client local tx-submission has `MsgRejectTx (reason)` (spec §3.12). N2N silently omits invalid txs | Validate and evaluate locally first, then confirm inclusion by watching |
| Chain-sync / block-fetch | **Yes** (headers + block bodies) | Spec §3.3.3, §3.7-3.8 |
| UTxO by address, protocol params | **No.** Local State Query "is part of the node-to-client protocol; hence, it is only used by local (and thus trusted) clients" (spec §3.13). `NodeClient` connects only over a Unix socket or named pipe (`facades.rs`) | Needs an indexer, a local node, or our own follower |

**N2N submit is the cheapest decentralisation win.** Submit to two or three
relays plus the indexer backend in parallel; the tx hash is deterministic, so
duplicates are harmless. Relay addresses belong in the network profile.

### 3.6 Mithril for confirmation and verification

The Mithril Rust client exposes `cardano_transaction().get_proofs(hashes)` with
local `verify()` and certificate-chain verification. Recent transactions come
back as `non_certified_transactions` until signed
([client library](https://mithril.network/doc/manual/develop/nodes/mithril-client-library);
[WASM](https://mithril.network/doc/manual/develop/nodes/mithril-client-library-wasm)).
That gives trust-minimised R6 with a delay. Combined with signed bytes carried
in the receipt (R7), it lets a verifier check an imported anchor with no indexer
at all, which matches the existing ADR follow-up.

### 3.7 Own chain follower (for completeness)

A client could follow N2N chain-sync and block-fetch from a known point (wallet
creation, or the sponsor pool's reference outpoint from the profile) and keep
its own UTxO set for a handful of addresses. Technically possible, but it needs
continuous block download (about 4,300 blocks/day at 20 s average) and catch-up
on every resume, which is costly on mobile. Not recommended before the steps
below.

## 4. Recommendation

**Answer:** Blockfrost is not required. It is today's only implementation of
a small read and submit surface. Keep it as an optional backend for users or
operators who already hold a key, and stop making it the precondition for any
chain feature.

### 4.1 Path (in order of value per effort)

| Step | Change | Fixes |
|---|---|---|
| 1 | **Track pending spends.** Exclude inputs of journaled, unconfirmed signed txs from `select_ada_only` and `select_sponsor_utxo` until confirmed or past `invalid_from_slot` | Observed double-spends, on any provider |
| 2 | **Endpoints in the profile.** Replace `PREPROD_BASE_URL` with an ordered `chain_providers` list in `preprod.json` (kind: `blockfrost-compatible` / `koios`; URL; `auth: none \| user_key`), read per AGENTS.md's profile rule. Make the key optional. Fail over on connect error, timeout, 5xx or 429 | Single point of failure, timeouts, keyless by default |
| 3 | **One port.** Widen `Ledger` (add `tx_utxos`, `latest_by_label`, `policy_assets`, `asset_mint_tx`). Route `username_anchor`, `completion*`, `relay_registry_chain`, `ChainFetcher`, `plutus_spend` and `stake_pubkey` through it. Delete dead `get_utxo_by_asset` and the CSP `*.blockfrost.io` entry | Lets backends be added without touching features |
| 4 | **Backends:** Blockfrost-compatible (covers hosted Blockfrost, Dolos MiniBF and Demeter with the same client), then Koios. Run at least one project or community Dolos instance and list it in the profile | Keyless operation for all users |
| 5 | **Local evaluation** via `uplc::tx::eval_phase_two` (resolved inputs plus the reference-script UTxO the profile already names). Tip from wall clock. Cache protocol params per epoch | Removes R3/R4 from the provider. Fewer requests |
| 6 | **N2N submit** with `pallas-network` 0.35.x `PeerClient` to relays listed in the profile, in parallel with provider submit. Treat inclusion (R6) as the only success signal | Submission no longer depends on any API |
| 7 | **Mithril proofs** for anchor confirmation and imported-receipt verification. Optionally carry signed bytes in receipts | Trust-minimised verification (ADR 0005 follow-up) |
| 8 | User setting "chain provider" (pick or add an endpoint, optional key), replacing the Blockfrost-only `cardano.blockfrost_project_id` | User control, consistent with the decentralisation ethos |

### 4.2 By audience

| Audience | Needs | Recommendation |
|---|---|---|
| App (desktop/mobile end user) | R1-R10. Writes only when issuing, anchoring or minting | Keyless default: profile-listed Dolos MiniBF and Koios. N2N submit. Local evaluation. Blockfrost only if the user adds a key. Never ship a key |
| Operator and deploy scripts (`cardano/governance/*`, `scripts/preflight.sh`, `src-tauri/examples/`) | UTxOs, params, submit, occasional evaluation | Low priority. Operators can keep their own Blockfrost key, or point the same scripts at a Dolos MiniBF URL (same paths, no `project_id`) |
| Cloud | None (no Cardano client) | No change |
| Relay, monitoring | None | Optionally host a Dolos instance next to the relay as community infrastructure. That changes no wire protocol |

## Open questions

- Does Koios's 1 KB public body cap apply to `submittx`? A sponsor-pool
  Plutus tx likely exceeds 1 KB. Test on preprod with a throwaway tx before relying on it.
- Should anchor receipts carry signed tx bytes, so verifiers need only a Mithril
  proof? That would be a protocol-spec change.
- Which public preprod relays go into the profile, and who runs the first
  project Dolos instance?
