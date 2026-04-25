#!/usr/bin/env bash
# Preflight check for deploy_reference_scripts.sh. Verifies all
# prerequisites are in place and prints any blockers.
#
# Usage:
#   source scripts/env.completion    # your local copy, not committed
#   ./scripts/preflight.sh
set -o pipefail

ok=0
fail=0
check() {
    local name="$1"
    local pass="$2"
    local detail="$3"
    if [ "$pass" = "y" ]; then
        echo "  ✓ $name"
        ok=$((ok + 1))
    else
        echo "  ✗ $name — $detail"
        fail=$((fail + 1))
    fi
}

echo "=== preflight: completion-witness deploy ==="

# Env vars
[ -n "${BLOCKFROST_PROJECT_ID:-}" ] && check "BLOCKFROST_PROJECT_ID set" y "" \
    || check "BLOCKFROST_PROJECT_ID set" n "set to your preprod project id"
[ -n "${DEPLOYER_SIGNING_KEY:-}" ] && check "DEPLOYER_SIGNING_KEY set" y "" \
    || check "DEPLOYER_SIGNING_KEY set" n "path to a cardano-cli .skey file"
[ -n "${DEPLOYER_ADDRESS:-}" ] && check "DEPLOYER_ADDRESS set" y "" \
    || check "DEPLOYER_ADDRESS set" n "deployer's bech32 preprod address"

# Files
if [ -n "${DEPLOYER_SIGNING_KEY:-}" ] && [ -f "$DEPLOYER_SIGNING_KEY" ]; then
    check "signing key file exists" y ""
else
    check "signing key file exists" n "$DEPLOYER_SIGNING_KEY not readable"
fi

# Tools
command -v aiken   >/dev/null 2>&1 && check "aiken on PATH" y ""       || check "aiken on PATH" n "install aiken"
command -v cardano-cli >/dev/null 2>&1 && check "cardano-cli on PATH" y "" || check "cardano-cli on PATH" n "install cardano-cli"
command -v curl    >/dev/null 2>&1 && check "curl on PATH" y ""         || check "curl on PATH" n "install curl"
command -v python3 >/dev/null 2>&1 && check "python3 on PATH" y ""      || check "python3 on PATH" n "install python3"

# Plutus build present
if [ -f cardano/governance/plutus.json ]; then
    check "plutus.json built" y ""
else
    check "plutus.json built" n "run 'aiken build' inside cardano/governance"
fi

# Compiled completion-minting validator present
if [ -f cardano/governance/plutus.json ]; then
    if python3 -c "
import json, sys
with open('cardano/governance/plutus.json') as f:
    data = json.load(f)
titles = {v['title'] for v in data.get('validators', [])}
sys.exit(0 if 'completion.completion_minting.mint' in titles else 1)
" 2>/dev/null; then
        check "completion_minting in plutus.json" y ""
    else
        check "completion_minting in plutus.json" n "re-run 'aiken build'"
    fi
fi

# Funds — only probed when Blockfrost credentials are in place.
if [ -n "${BLOCKFROST_PROJECT_ID:-}" ] && [ -n "${DEPLOYER_ADDRESS:-}" ]; then
    lovelace=$(curl -s -H "project_id: $BLOCKFROST_PROJECT_ID" \
        "https://cardano-preprod.blockfrost.io/api/v0/addresses/$DEPLOYER_ADDRESS" \
        | python3 -c "
import json, sys
try:
    data = json.load(sys.stdin)
    total = sum(int(a['quantity']) for a in data.get('amount', []) if a['unit'] == 'lovelace')
    print(total)
except Exception:
    print(0)
" 2>/dev/null)
    if [ "${lovelace:-0}" -ge 50000000 ]; then
        check "deployer has >= 50 tADA" y "($((lovelace / 1000000)) tADA)"
    else
        check "deployer has >= 50 tADA" n "fund via https://docs.cardano.org/cardano-testnet/tools/faucet/ (have: $((${lovelace:-0} / 1000000)) tADA)"
    fi
fi

echo
echo "Summary: $ok ok, $fail blocking"
if [ $fail -eq 0 ]; then
    echo "Ready. Run: ./cardano/governance/deploy_reference_scripts.sh"
fi
[ $fail -eq 0 ]
