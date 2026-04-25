# Operational scripts

Helpers for the runtime-prerequisite work that has to happen on a
machine with your Blockfrost credentials and `cardano-cli` installed.

## End-to-end deploy walkthrough

```bash
# 1. Fill in your Blockfrost project id + deployer address.
cp scripts/env.completion.example scripts/env.completion
$EDITOR scripts/env.completion
source scripts/env.completion

# 2. Sanity check.
./scripts/preflight.sh

# 3. Deploy reference scripts on preprod. Costs ~40 tADA total
#    (5 ADA × 8 reference scripts), takes a few minutes per script
#    while the chain confirms.
./cardano/governance/deploy_reference_scripts.sh

# 4. Patch script_refs.rs in place from the deployment_results.json
#    written by step 3. Idempotent.
./scripts/apply_deploy_results.sh

# 5. Rebuild the backend with the populated reference UTxOs.
cargo check --manifest-path src-tauri/Cargo.toml

# 6. Source the same env file when starting the node so the
#    completion observer can see ALEXANDRIA_COMPLETION_POLICY_ID.
source scripts/env.completion
cargo tauri dev
```

`scripts/env.completion` is gitignored locally so secrets stay
out of source control. Keep `scripts/env.completion.example` as
the public template.

## What each script does

| Script | Job |
|---|---|
| `env.completion.example` | Documents the required env vars and pre-fills `ALEXANDRIA_COMPLETION_POLICY_ID` (deterministic from `plutus.json`). |
| `preflight.sh` | Checks env vars, key file existence, tool availability, plutus.json freshness, and deployer balance. Exits non-zero if anything is missing. |
| `apply_deploy_results.sh` | Reads the JSON the deploy script writes (`build/deploy/deployment_results.json`) and patches `script_refs.rs` in place with each `(tx_hash, output_index)` pair. Idempotent. |
