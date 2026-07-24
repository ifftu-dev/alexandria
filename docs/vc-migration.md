# VC-First Cutover (Migration 040)

**Date:** 2026-04-24
**Branch:** `refactor/vc-first-migration`

This document is the authoritative account of the cutover from the
legacy SkillProof pipeline to a W3C Verifiable Credentials (VC) model.
It supersedes the pre-cutover descriptions in `architecture.md`,
`protocol-specification.md`, `skills-and-reputation.md`,
`database-schema.md`, and `vision.md` wherever those docs reference
SkillProof / EvidenceRecord / SkillAssessment artifacts.

## What changed

**Retired:**

- `skill_proofs`, `skill_proof_evidence`, `evidence_records`,
  `skill_assessments` tables (dropped by migration 040).
- The aggregator that produced SkillProofs from evidence
  (`evidence/aggregator.rs` — deleted).
- Evidence gossip topic (`/alexandria/evidence/1.0`) and handler
  (`p2p/evidence.rs` — deleted).
- `mint_skill_proof_nft` and `register_course_onchain` Cardano IPC
  commands and their tx builders.
- Multi-party attestation on evidence rows (`attestation_requirements`,
  `evidence_attestations`) and the evidence challenge committee
  (`evidence_challenges`, `challenge_votes`) — dropped pending rebuild
  against credentials (not evidence rows).
- Reputation join tables (`reputation_evidence`,
  `reputation_impact_deltas`) — will be rebuilt against credentials.

**New:**

- `credentials` gains four columns: `witness_tx_hash`,
  `witness_validator_script_hash`, `witness_validator_name`,
  `auto_issued`.
- `VerifiableCredential` struct gains an optional `witness` field
  carrying those three pieces of on-chain authorization state. The
  JWS covers the witness block when present.

**Staying:**

- `credentials`, `credential_status_lists`, `credential_anchors`,
  `key_registry`, `credentials_pending_verification`,
  `credential_suspension`, `credential_allowlist`.
- `reputation_assertions` (actor-level reputation store; inputs will
  repoint to credentials).
- VC gossip topics: `/alexandria/vc-did/1.0`,
  `/alexandria/vc-status/1.0`, `/alexandria/vc-presentation/1.0`,
  `/alexandria/pinboard/1.0`, and the `/alexandria/vc-fetch/1.0`
  request-response protocol.
- All governance/Aiken validators (`dao_registry`, `dao_minting`,
  `election`, `proposal`, `reputation_minting`, `soulbound`,
  `vote_minting`) — unchanged.

## New conceptual model

1. A learner's identity is a `did:key` Ed25519 derived from their
   BIP-39 mnemonic (unchanged).
2. Course elements are gradeable; `claim_course_completion` assembles
   completion leaves from the learner's graded `element_submissions`
   (**implemented**).
3. Element leaves aggregate to a course-completion Merkle root that is
   verified against the registered course template (gradeable elements
   in order, ≥0.6 pass) (**implemented**).
4. The learner submits the completion witness tx against the
   `completion.ak` validator, carrying the course ID and completion
   Merkle root. The completion validator **is deployed**
   (`COMPLETION_MINTING_REF_UTXO` populated).
5. The credential is **self-issued locally at claim time** into the
   learner's `credentials` table, whether or not the on-chain witness
   mint succeeds (`commands/completion.rs`: "Always self-issue
   locally"; the on-chain mint is best-effort — an upgrade, not a
   gate). A Blockfrost observer + auto-issuance loop (60s) run as a
   **secondary** path that backfills the witness tx hash once the mint
   confirms.
6. Verifiers check the VC's JWS signature **and** resolve the
   witness tx on Cardano to confirm the validator accepted the
   completion claim.

## Cardano's scope going forward

1. **VC integrity anchoring** — BLAKE3-of-VC metadata txs (label 1697)
   via `cardano/anchor_queue.rs` (wired).
2. **DAO governance** — election/proposal/vote txs via
   `cardano/gov_tx_builder.rs` and `cardano/onchain_queue.rs`
   (wired; reference scripts deployed on preprod — see
   `cardano/script_refs.rs`).
3. **Completion-witness minting** — `completion.ak` validator
   (deployed) + observer + auto-issuance (live).
4. **Challenge-stake escrow** — `challenge_escrow.ak` validator; lock
   and settle both work on preprod (escrow reference script deployed).
5. **CIP-68 soulbound reputation snapshots** — `soulbound_tx_builder.rs`
   / `submit_snapshot_tx`; mint works on preprod (reputation-minting
   reference script deployed).

## What compiles today

- `src-tauri` (lib + tests) — clean build.
- `cli` — clean build.
- Frontend (`vue-tsc -b --noEmit`) — clean typecheck.

## Session 2 additions — auto-issuance vertical slice

- `cardano/governance/validators/completion.ak` + `lib/alexandria/completion.ak` —
  CIP-25 minting policy with Merkle-root + learner-signature +
  validity-window checks. Compiled hash:
  `6380450179a6933acdf76213732f8626e1486b9ed5cc7fe7f46c98e0`.
- `src-tauri/src/cardano/completion.rs` — Blockfrost observer that
  decodes Plutus `Constr 0` inline datums into
  `completion_observations` rows. Idempotent by `(policy_id,
  asset_name_hex)`.
- `src-tauri/src/commands/auto_issuance.rs` — self-signs a
  `SelfAssertion` VC for each pending observation with the `Witness`
  block populated. Attestation gate below.
- `src-tauri/src/domain/completion.rs` — `element_leaf` /
  `merkle_root` that match the Aiken algorithm byte-for-byte.
- `src-tauri/src/cardano/completion_tx_builder.rs` — Conway tx
  builder that mints the completion token with the inline
  `CompletionDatum`. Gated on `COMPLETION_MINTING_REF_UTXO` being
  populated (deployed to preprod 2026-05-22, block 4736927).
- Migration 041 adds `completion_observations`.
- `src-tauri/src/commands/completion.rs` — frontend IPC:
  `preview_completion_root`, `submit_completion_witness`.

## Session 3 additions — rebuilt subsystems

- **Governance + opinion gating** (task #9): queries read the
  proficiency level out of `signed_vc_json` via `json_extract`;
  opinions require `apply+` credentials under the target subject.
- **Attestation** (task #17): rebuilt at
  `commands::attestation` + `completion_attestation_requirements` +
  `completion_attestations` tables (migration 042). Assessors sign
  the witness tx hash; the auto-issuance pipeline refuses to emit
  until the DAO-configured threshold of valid signatures is present.
- **Reputation engine** (task #15): rebuilt at
  `evidence::reputation`. `on_credential_accepted` is the single
  entry point, called after every issuance path. Learner rows
  mirror the max observed `SkillClaim.score`; instructor rows track
  the mean across all credentials they've issued at a given
  `(skill, level)`. Distribution metrics (median / p25 / p75 /
  variance / learner_count) **are now computed and persisted**;
  `commands::reputation::get_reputation` derives a sample-size
  confidence (`learner_count / (learner_count + 5)`) on read.
- **Challenge system** (task #16): rebuilt at
  `commands::challenge` + `credential_challenges` +
  `credential_challenge_votes` tables (migration 043). Targets a
  specific credential; 2/3 supermajority upholds → revocation via
  status-list bit flip. Stake escrow is now real: 5 ADA locks at the
  `challenge_escrow.ak` validator (migration 050 added `stake_status`
  + `settle_tx_hash`); the lock tx works on preprod, and on resolution
  the DAO authority settles (Refund → challenger / Forfeit →
  treasury). `CHALLENGE_ESCROW_REF_UTXO` is deployed on preprod
  (2026-05-22, block 4736927), so settlement is live-capable.

## Observer daemon wiring

`lib.rs` queue loop runs every 60s and now additionally invokes:

1. `cardano::completion::tick(&db, &bf, &policy_id)` — polls
   Blockfrost for new mints under
   `ALEXANDRIA_COMPLETION_POLICY_ID` and writes
   `completion_observations` rows.
2. `commands::auto_issuance::tick(&conn, &learner_key)` — emits
   VCs for observations whose attestation requirement is satisfied.

Both are silent no-ops if the env var is unset or no profile is
currently unlocked, matching the posture of the other cardano queues.

## Seed updates

Migration-time SQL seeds (`db::seed`) gain:
- One demo `completion_attestation_requirements` row on
  `course_civics_101` (required_attestors = 2).
- One pending `completion_observations` row so the frontend can
  exercise the "awaiting attestation" state.
- One demo `completion_attestations` row that partially satisfies
  the requirement.

## Frontend wiring

**Done.** The credential pages are rewired against the VC IPC surface
(`list_credentials`, `preview_completion_root`,
`submit_completion_witness`, `claim_course_completion`,
`list_reputation_rows`, `get_reputation`, `list_credential_challenges`,
`get_completion_attestation_status`, etc.), and the frontend exposes a
"Claim Credential" affordance that drives the completion-witness flow.

## Deploy prerequisites

The subsystem rebuilds are **done**: auto-issuance
(`commands::auto_issuance`), the credential-sourced reputation engine
(`evidence/reputation.rs`), credential challenges (`commands::challenge`,
status-list revocation), completion attestation (`commands::attestation`),
and the frontend rewire all ship.

All nine Aiken/Plutus v3 reference scripts are now deployed on
**preprod testnet** (2026-05-22, block 4736927) via
`cardano/governance/deploy_blockfrost.py` (a node-free deployer:
`cardano-cli build-raw` + Blockfrost submit), and `cardano/script_refs.rs`
carries their UTxOs. `ref_utxos_deployed()`, `completion_ref_deployed()`,
and `challenge_escrow_deployed()` all return `true`, so the soulbound
snapshot mint, challenge-stake settlement, completion-witness, and
governance tx builders all reference live scripts. The end-to-end
governance enforcement *flows* (election/proposal lifecycle) are still
maturing on top of the now-deployed validators.

To run the flows against preprod:

1. (Done — re-run only to redeploy.) Reference scripts are deployed;
   `cardano/governance/deploy_reference_scripts.sh` (node-based) or
   `deploy_blockfrost.py` (node-free) regenerate them and update
   `script_refs.rs`.
2. Configure Blockfrost: either set `cardano.blockfrost_project_id`
   in Settings → Cardano (recommended), or export
   `BLOCKFROST_PROJECT_ID`. Also export `ALEXANDRIA_COMPLETION_POLICY_ID`
   (= `6380450179a6933acdf76213732f8626e1486b9ed5cc7fe7f46c98e0` or
   a re-compiled hash) before starting the node.
