# Alexandria Governance Smart Contracts

On-chain governance for the Alexandria learning platform, implementing whitepaper Section 4. Written in [Aiken](https://aiken-lang.org) v1.1.21 targeting Plutus v3 (Conway era).

> **⚠ Status: compiled but not deployed.** The validators below are built and their hashes are hardcoded in `src-tauri/src/cardano/script_refs.rs`, but the reference UTxOs are still `DEPLOY_PENDING`. Until someone runs [`deploy_reference_scripts.sh`](#deploying-to-preprod) and updates `script_refs.rs`, `cardano::gov_tx_builder::validators_deployed()` returns `false` and the on-chain governance queue silently skips every item — nothing actually hits the chain. The off-chain lifecycle (DAOs, elections, proposals in SQLite, P2P gossip) works regardless; only Cardano-side settlement is gated on deployment.

## Validators

| Validator | Purpose |
|-----------|---------|
| `dao_registry` | Stores DAO state UTxOs (SubjectField and Subject DAOs). Enforces membership and parameter updates |
| `dao_minting` | Minting policy for DAO state tokens. One token per DAO, held at the registry |
| `election` | Election lifecycle: nomination → voting → finalization. Enforces quorum, deadlines, and seat allocation |
| `proposal` | Proposal lifecycle: draft → approve → vote → resolve. Supermajority and quorum enforcement |
| `reputation_minting` | CIP-68 soulbound reputation token minting. Reference + user token pairs anchored to skill proofs |
| `soulbound` | Spending validator for soulbound tokens. Only the platform can update; holders cannot transfer |
| `vote_minting` | Vote receipt token minting. One receipt per voter per election/proposal to prevent double-voting |

## Library Modules

| Module | Purpose |
|--------|---------|
| `types` | All on-chain types: DAO datums, election/proposal state, reputation metadata, redeemers |
| `reputation` | CIP-68 token name construction, reference token helpers |
| `state_token` | DAO state token name construction and UTxO lookup |
| `utils` | Quorum checks, majority/supermajority, deadline validation, top-N candidate selection |

## Building

Requires [Aiken](https://aiken-lang.org/installation-instructions) v1.1.21+.

```sh
aiken build
```

Produces `plutus.json` — the CIP-57 blueprint containing all validator scripts.

## Testing

```sh
aiken check
```

7 unit tests covering quorum, majority, supermajority, and top-N selection logic.

## Architecture

- **DAO hierarchy** mirrors the skill taxonomy: one DAO per Subject Field, one per Subject
- **Elections** select council members from nominees who hold sufficient SkillProofs
- **Proposals** require council approval before community vote
- **Reputation tokens** are CIP-68 soulbound NFTs — the reference token holds metadata (skill, proficiency, confidence), the user token is non-transferable
- **Vote receipts** prevent double-voting without requiring on-chain voter rolls

## Deploying to Preprod

A deployment script is provided to deploy all 7 validators as reference scripts on Cardano preprod testnet.

### Prerequisites

- `cardano-cli` installed (Conway-era compatible)
- A funded preprod wallet (needs ~40 tADA for 7 reference script UTxOs + fees)
- `BLOCKFROST_PROJECT_ID` environment variable set (get one from [blockfrost.io](https://blockfrost.io))

### Deploy

```sh
export BLOCKFROST_PROJECT_ID="preprodXXX..."
export DEPLOYER_SIGNING_KEY="$HOME/.cardano/deployer.skey"
export DEPLOYER_ADDRESS="addr_test1..."
./deploy_reference_scripts.sh
```

The script:
1. Extracts compiled UPLC from `plutus.json`
2. Wraps each as a PlutusScriptV3 envelope
3. Builds + signs + submits one reference script transaction per validator
4. Saves results to `build/deploy/deployment_results.json`

### After Deployment

Update `src-tauri/src/cardano/script_refs.rs` with the deployment tx hashes from `deployment_results.json`:

```rust
pub const DAO_REGISTRY_REF_UTXO: (&str, u64) = ("<tx_hash>", 0);
// ... repeat for all 7 validators
```

Once updated, `ref_utxos_deployed()` returns `true` and the governance on-chain queue begins submitting Plutus transactions automatically.

## Integration with the App

The Rust backend integrates with these validators through:

| Module | Purpose |
|--------|---------|
| `cardano/gov_tx_builder.rs` | 6 governance tx builders (CreateDao, OpenElection, CastVote, ResolveProposal, FinalizeElection, InstallCommittee) |
| `cardano/soulbound_tx_builder.rs` | CIP-68 soulbound reputation token minting |
| `cardano/onchain_queue.rs` | Persistent queue that dispatches governance actions to tx builders |
| `cardano/plutus_data.rs` | All datum/redeemer CBOR encoding for Plutus Data |
| `cardano/script_refs.rs` | Script hashes and reference UTxO locations |
| `commands/snapshot.rs` | `submit_snapshot_tx` command for soulbound minting |

See `docs/whitepaper-consolidated-v0.0.1.md` Section 4 for the full governance specification.
