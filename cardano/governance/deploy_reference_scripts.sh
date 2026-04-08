#!/usr/bin/env bash
# deploy_reference_scripts.sh — Deploy Aiken governance validators as reference
# scripts on Cardano preprod testnet.
#
# Prerequisites:
#   - aiken CLI installed and validators compiled (aiken build)
#   - cardano-cli installed
#   - BLOCKFROST_PROJECT_ID env var set
#   - DEPLOYER_SIGNING_KEY env var pointing to a .skey file with preprod tADA
#   - DEPLOYER_ADDRESS env var with the bech32 address for the signing key
#
# Each validator is deployed as a reference script UTxO with 5 ADA locked.
# After deployment, update script_refs.rs with the output tx hashes.
#
# Usage:
#   export BLOCKFROST_PROJECT_ID="preprodXXX..."
#   export DEPLOYER_SIGNING_KEY="$HOME/.cardano/deployer.skey"
#   export DEPLOYER_ADDRESS="addr_test1..."
#   ./deploy_reference_scripts.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PLUTUS_JSON="$SCRIPT_DIR/plutus.json"
OUTPUT_DIR="$SCRIPT_DIR/build/deploy"
NETWORK="--testnet-magic 1"
LOCKED_ADA=5000000  # 5 ADA per reference script UTxO

# Validate env
: "${BLOCKFROST_PROJECT_ID:?Set BLOCKFROST_PROJECT_ID}"
: "${DEPLOYER_SIGNING_KEY:?Set DEPLOYER_SIGNING_KEY to a .skey file path}"
: "${DEPLOYER_ADDRESS:?Set DEPLOYER_ADDRESS to the deployer bech32 address}"

BF_URL="https://cardano-preprod.blockfrost.io/api/v0"

mkdir -p "$OUTPUT_DIR"

# Validators to deploy (title prefix from plutus.json → filename)
VALIDATORS=(
  "dao_minting.dao_minting.mint:dao_minting"
  "dao_registry.dao_registry.spend:dao_registry"
  "election.election.spend:election"
  "proposal.proposal.spend:proposal"
  "vote_minting.vote_minting.mint:vote_minting"
  "reputation_minting.reputation_minting.mint:reputation_minting"
  "soulbound.soulbound.spend:soulbound"
)

echo "=== Alexandria Governance Validator Deployment ==="
echo "Network: Cardano Preprod"
echo "Deployer: $DEPLOYER_ADDRESS"
echo ""

# Extract compiled scripts from plutus.json
echo "--- Extracting compiled scripts ---"
for entry in "${VALIDATORS[@]}"; do
  title="${entry%%:*}"
  name="${entry##*:}"

  # Extract compiledCode for this validator
  code=$(python3 -c "
import json, sys
with open('$PLUTUS_JSON') as f:
    data = json.load(f)
for v in data['validators']:
    if v['title'] == '$title':
        print(v['compiledCode'])
        sys.exit(0)
print('NOT_FOUND', file=sys.stderr)
sys.exit(1)
")

  if [ -z "$code" ]; then
    echo "ERROR: Validator '$title' not found in plutus.json"
    exit 1
  fi

  # Write as a Plutus V3 script envelope
  cat > "$OUTPUT_DIR/${name}.plutus" <<ENVELOPE
{
  "type": "PlutusScriptV3",
  "description": "Alexandria governance: ${name}",
  "cborHex": "$(python3 -c "
import cbor2
code = bytes.fromhex('$code')
# Double-CBOR wrap: Plutus script = CBOR(bytes(compiled_code))
wrapped = cbor2.dumps(code)
print(wrapped.hex())
")"
}
ENVELOPE

  echo "  Extracted: ${name} ($(echo -n "$code" | wc -c | tr -d ' ') hex chars)"
done

echo ""
echo "--- Querying deployer UTxOs ---"
UTXOS=$(curl -s "$BF_URL/addresses/$DEPLOYER_ADDRESS/utxos" \
  -H "project_id: $BLOCKFROST_PROJECT_ID")

echo "$UTXOS" | python3 -c "
import json, sys
utxos = json.load(sys.stdin)
total = sum(int(a['quantity']) for u in utxos for a in u['amount'] if a['unit'] == 'lovelace')
print(f'  Found {len(utxos)} UTxO(s), total: {total / 1_000_000:.2f} ADA')
needed = ${#VALIDATORS[@]} * $LOCKED_ADA + 5_000_000  # scripts + fees
if total < needed:
    print(f'  WARNING: Need at least {needed / 1_000_000:.2f} ADA, have {total / 1_000_000:.2f}')
    sys.exit(1)
"

echo ""
echo "--- Building deployment transactions ---"

RESULTS_FILE="$OUTPUT_DIR/deployment_results.json"
echo "{" > "$RESULTS_FILE"

for i in "${!VALIDATORS[@]}"; do
  entry="${VALIDATORS[$i]}"
  name="${entry##*:}"
  script_file="$OUTPUT_DIR/${name}.plutus"

  echo "  Deploying: ${name}..."

  # Build the reference script output address (send to deployer's own address)
  TX_RAW="$OUTPUT_DIR/${name}_tx.raw"
  TX_SIGNED="$OUTPUT_DIR/${name}_tx.signed"

  # Query protocol parameters
  cardano-cli conway query protocol-parameters $NETWORK \
    --out-file "$OUTPUT_DIR/protocol-params.json" 2>/dev/null || \
  curl -s "$BF_URL/epochs/latest/parameters" \
    -H "project_id: $BLOCKFROST_PROJECT_ID" > "$OUTPUT_DIR/protocol-params.json"

  # Build transaction with reference script
  cardano-cli conway transaction build \
    $NETWORK \
    --tx-in "$(echo "$UTXOS" | python3 -c "
import json, sys
utxos = json.load(sys.stdin)
for u in utxos:
    ada = int([a['quantity'] for a in u['amount'] if a['unit'] == 'lovelace'][0])
    if ada >= 10000000:
        print(f\"{u['tx_hash']}#{u['tx_index']}\")
        break
")" \
    --tx-out "$DEPLOYER_ADDRESS+$LOCKED_ADA" \
    --tx-out-reference-script-file "$script_file" \
    --change-address "$DEPLOYER_ADDRESS" \
    --out-file "$TX_RAW" 2>&1 || {
      echo "  FAILED to build tx for ${name}"
      continue
    }

  # Sign
  cardano-cli conway transaction sign \
    $NETWORK \
    --signing-key-file "$DEPLOYER_SIGNING_KEY" \
    --tx-body-file "$TX_RAW" \
    --out-file "$TX_SIGNED"

  # Submit
  TX_HASH=$(cardano-cli conway transaction submit \
    $NETWORK \
    --tx-file "$TX_SIGNED" 2>&1 && \
    cardano-cli conway transaction txid --tx-file "$TX_SIGNED")

  echo "  Submitted: ${name} -> ${TX_HASH}"

  # The reference script is at output index 0 (the explicit --tx-out)
  SEPARATOR=""
  if [ "$i" -lt "$((${#VALIDATORS[@]} - 1))" ]; then
    SEPARATOR=","
  fi
  echo "  \"${name}\": { \"tx_hash\": \"${TX_HASH}\", \"output_index\": 0 }${SEPARATOR}" >> "$RESULTS_FILE"

  # Wait for UTxO set to update before next deployment (max 30 seconds)
  PREV_COUNT=$(echo "$UTXOS" | python3 -c "import json,sys; print(len(json.load(sys.stdin)))")
  for _attempt in $(seq 1 15); do
    sleep 2
    UTXOS=$(curl -s "$BF_URL/addresses/$DEPLOYER_ADDRESS/utxos" \
      -H "project_id: $BLOCKFROST_PROJECT_ID")
    NEW_COUNT=$(echo "$UTXOS" | python3 -c "import json,sys; print(len(json.load(sys.stdin)))")
    if [ "$NEW_COUNT" != "$PREV_COUNT" ]; then
      break
    fi
  done
done

echo "}" >> "$RESULTS_FILE"

echo ""
echo "=== Deployment Complete ==="
echo "Results saved to: $RESULTS_FILE"
echo ""
echo "Next steps:"
echo "  1. Wait for confirmations (~20 seconds on preprod)"
echo "  2. Update src-tauri/src/cardano/script_refs.rs with the tx hashes from:"
echo "     $RESULTS_FILE"
echo "  3. Rebuild the app"
