# Alexandria Governance Smart Contracts

On-chain governance for the Alexandria learning platform, implementing Section 10 (Governance) of `docs/protocol-specification.md`. Written in [Aiken](https://aiken-lang.org) v1.1.21 targeting Plutus v3 (Conway era).

> **Status: retained deployment artifacts on preprod.** Eight validators are deployed as CIP-33 reference scripts (parameterized validators applied; hashes + ref UTxOs in `src-tauri/src/cardano/script_refs.rs`). Deployment and isolated transition tests do not make every flow part of the current release authority path.
>
> **Release governance is not active.** The app can verify and explicitly pin a seven-founder genesis, and it rejects inbound governance state until committee outcome certificates are implemented. The app's local election/proposal commands and operator governance queue are deleted; the deployed validators remain reviewable upgrade artifacts. See `docs/protocol-specification.md` §10.

## Validators

| Validator | Purpose |
|-----------|---------|
| `dao_registry` | Stores DAO state UTxOs (SubjectField and Subject DAOs). Enforces membership and parameter updates |
| `dao_minting` | Minting policy for DAO state tokens. One token per DAO, held at the registry |
| `election` | Election lifecycle: nomination → voting → finalization. Enforces quorum, deadlines, and seat allocation |
| `proposal` | Proposal lifecycle: draft → approve → vote → resolve. Supermajority and quorum enforcement |
| `reputation_minting` | Historical CIP-68 reputation-token minting policy; new snapshots use signed `DerivedCredential` VCs |
| `soulbound` | Historical spending validator for CIP-68 reputation tokens |
| `vote_minting` | Vote receipt token minting. One receipt per voter per election/proposal to prevent double-voting |
| `completion` | Completion-witness minting policy keyed to the learner and course completion root |

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

Run `aiken check` for the current validator test count and results.

## Architecture

- **DAO hierarchy** mirrors the skill taxonomy: one DAO per Subject Field, one per Subject
- **Elections** are designed to select council members from nominees satisfying the DAO qualification policy
- **Proposals** require council approval before community vote
- **Reputation snapshots** are now signed `DerivedCredential` VCs with optional canonical-hash anchoring. The CIP-68 reputation validators remain historical deployment artifacts.
- **Vote receipts** prevent double-voting without requiring on-chain voter rolls

## Deploying to Preprod

A deployment script is provided to deploy the 8 retained validators as reference scripts on Cardano preprod testnet.

### Prerequisites

- `cardano-cli` installed (Conway-era compatible)
- A funded preprod wallet with enough test ADA for 8 reference-script outputs and fees
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
// ... repeat for all retained validators
```

Once updated, `ref_utxos_deployed()` reports that the references are populated. Release governance remains disabled until its authority certificate path is complete.

## Integration with the App

The Rust backend integrates with these validators through:

| Module | Purpose |
|--------|---------|
| `cardano/gov_tx_builder.rs` | Shared Plutus helpers (script addresses, field injection, script-hash parsing); the governance tx builders are deleted |
| `cardano/plutus_data.rs` | Soulbound, reputation-mint and completion datum/redeemer CBOR encoding; the governance encoders are deleted |
| `cardano/script_refs.rs` | Script hashes and reference UTxO locations |
| `commands/snapshot.rs` | Creates signed `DerivedCredential` snapshots and optionally queues their canonical credential hash for anchoring |

See `docs/protocol-specification.md` Section 10 (Governance) for the full governance specification.
