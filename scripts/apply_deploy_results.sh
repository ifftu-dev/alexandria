#!/usr/bin/env bash
# Post-deploy helper: reads the tx hashes written by
# deploy_reference_scripts.sh and patches script_refs.rs in place.
#
# Idempotent: re-running with the same deployment_results.json yields
# the same final state.
#
# Usage:
#   ./scripts/apply_deploy_results.sh
#     [cardano/governance/build/deploy/deployment_results.json]
#     [src-tauri/src/cardano/script_refs.rs]
set -eo pipefail

RESULTS="${1:-cardano/governance/build/deploy/deployment_results.json}"
REFS="${2:-src-tauri/src/cardano/script_refs.rs}"

if [ ! -f "$RESULTS" ]; then
    echo "error: $RESULTS not found — run deploy_reference_scripts.sh first" >&2
    exit 1
fi
if [ ! -f "$REFS" ]; then
    echo "error: $REFS not found" >&2
    exit 1
fi

echo "Patching $REFS from $RESULTS"

# Map of JSON key → Rust constant name. Order matches script_refs.rs
# for readability; apply_one is idempotent.
declare -a MAPPINGS=(
    "dao_registry:DAO_REGISTRY_REF_UTXO"
    "dao_minting:DAO_MINTING_REF_UTXO"
    "election:ELECTION_REF_UTXO"
    "proposal:PROPOSAL_REF_UTXO"
    "vote_minting:VOTE_MINTING_REF_UTXO"
    "reputation_minting:REPUTATION_MINTING_REF_UTXO"
    "soulbound:SOULBOUND_REF_UTXO"
    "completion_minting:COMPLETION_MINTING_REF_UTXO"
)

applied=0
skipped=0
for entry in "${MAPPINGS[@]}"; do
    name="${entry%%:*}"
    const="${entry##*:}"
    result=$(python3 -c "
import json, sys
with open('$RESULTS') as f:
    data = json.load(f)
v = data.get('$name')
if not v:
    sys.exit(2)
print(f\"{v.get('tx_hash', '')}\t{v.get('output_index', 0)}\")
" 2>/dev/null) || { skipped=$((skipped + 1)); continue; }

    tx_hash="${result%%$'\t'*}"
    idx="${result##*$'\t'}"

    if [ -z "$tx_hash" ]; then
        skipped=$((skipped + 1))
        continue
    fi

    # Replace `pub const NAME: (&str, u64) = (...);` regardless of prior value.
    python3 - "$REFS" "$const" "$tx_hash" "$idx" <<'PY'
import re, sys
path, const, tx, idx = sys.argv[1:5]
pattern = re.compile(
    rf'pub const {re.escape(const)}: \(&str, u64\) = \([^)]*\);'
)
replacement = f'pub const {const}: (&str, u64) = ("{tx}", {idx});'
with open(path) as f:
    text = f.read()
new, n = pattern.subn(replacement, text)
if n != 1:
    print(f"error: could not patch {const} in {path} (found {n} matches)", file=sys.stderr)
    sys.exit(3)
if new != text:
    with open(path, 'w') as f:
        f.write(new)
PY
    echo "  $const → ($tx_hash, $idx)"
    applied=$((applied + 1))
done

echo
echo "Patched: $applied, skipped: $skipped"
if [ $applied -gt 0 ]; then
    echo "Rebuild the backend: cargo check --manifest-path src-tauri/Cargo.toml"
fi
