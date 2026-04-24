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
2. Course elements are plugin-defined; each plugin emits a
   deterministic completion-state hash on completion (**work
   scheduled**).
3. Element hashes aggregate to a course-completion Merkle root that
   matches what a registered course template declared (**work
   scheduled**).
4. The learner submits a Cardano tx that locks at the
   `completion.ak` validator script address, carrying the course ID
   and completion Merkle root (**validator work scheduled**).
5. An observer watches Blockfrost for confirmed txs at that script
   address. On confirmation it issues a self-signed VC to the
   learner's local `credentials` table, embedding the witness tx
   hash (**observer work scheduled**).
6. Verifiers check the VC's JWS signature **and** resolve the
   witness tx on Cardano to confirm the validator accepted the
   completion claim.

## Cardano's scope going forward

Two roles only:

1. **VC anchoring** — BLAKE3-of-VC metadata txs via
   `cardano/anchor_queue.rs` (already wired).
2. **DAO governance** — snapshot submissions, election/proposal/vote
   txs via `cardano/gov_tx_builder.rs`, `cardano/soulbound_tx_builder.rs`,
   and `cardano/onchain_queue.rs` (already wired).

Plus the new completion-witness role (coming online with the
validator + observer work) sitting on top of (1) as a dependency.

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
  populated (`DEPLOY_PENDING` until the deploy script has run).
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
  `(skill, level)`. No distribution metrics (variance / p25 / p75)
  yet — can be layered on with sufficient volume.
- **Challenge system** (task #16): rebuilt at `evidence::challenge`
  + `credential_challenges` + `credential_challenge_votes` tables
  (migration 043). Targets a specific credential; 2/3 supermajority
  upholds → revocation via status-list bit flip.

## Observer daemon wiring

`lib.rs` queue loop runs every 60s and now additionally invokes:

1. `cardano::completion::tick(&db, &bf, &policy_id)` — polls
   Blockfrost for new mints under
   `ALEXANDRIA_COMPLETION_POLICY_ID` and writes
   `completion_observations` rows.
2. `commands::auto_issuance::tick(&conn, &learner_key)` — emits
   VCs for observations whose attestation requirement is satisfied.

Both are silent no-ops if the env var is unset or the vault is
still locked, matching the posture of the other cardano queues.

## Seed updates

Migration-time SQL seeds (`db::seed`) gain:
- One demo `completion_attestation_requirements` row on
  `course_civics_101` (required_attestors = 2).
- One pending `completion_observations` row so the frontend can
  exercise the "awaiting attestation" state.
- One demo `completion_attestations` row that partially satisfies
  the requirement.

## Remaining frontend work

Still degraded — `list_credentials`-based pages are not rewired yet.
The IPC surface for rewiring is now complete:
`list_credentials`, `preview_completion_root`, `submit_completion_witness`,
`list_reputation_rows`, `list_credential_challenges`,
`get_completion_attestation_status`, etc.

## Deploy prerequisites

Before the live end-to-end flow will run:
1. Run `cardano/governance/deploy_reference_scripts.sh` against
   preprod. The script now includes `completion_minting`.
2. Update `COMPLETION_MINTING_REF_UTXO` in `cardano/script_refs.rs`
   with the returned tx hash.
3. Export `BLOCKFROST_PROJECT_ID` + `ALEXANDRIA_COMPLETION_POLICY_ID`
   (= `6380450179a6933acdf76213732f8626e1486b9ed5cc7fe7f46c98e0` or
   a re-compiled hash) before starting the node.
4. `src-tauri/src/commands/auto_issuance.rs` — observer → self-signed
   VC pipeline with witness block.
5. Repoint `evidence/reputation.rs` at credential-issuance events;
   re-enable the module.
6. Rebuild `evidence/challenge.rs` against `credentials` (status-list
   revocation instead of evidence deletion); re-enable.
7. Rebuild `evidence/attestation.rs` as an observer-side issuance
   gate; re-enable.
8. Frontend rewire: credentials page with witness-tx badges,
   skills/opinions pages querying `list_credentials` for the local
   DID.
