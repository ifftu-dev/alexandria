#!/usr/bin/env bash
# Deploy ONLY the challenge_escrow validator as a reference script on preprod.
#
# The stock deploy_blockfrost.py deploys every validator in two batches. This
# wrapper deploys one, because only the escrow validator changed (P3-M3) and
# redeploying the other eight would replace on-chain scripts that are correct.
#
# Signs with the treasury key and broadcasts — run it yourself. Dry-run first:
#   DRY_RUN=1 ./deploy_escrow_only.sh
set -euo pipefail
cd "$(dirname "$0")"
# Credentials live in the main `alexandria/` checkout (gitignored), not in a
# worktree. Default there; override with CREDS_ROOT or the three env vars.
ROOT="$(cd ../.. && pwd)"
CREDS_ROOT="${CREDS_ROOT:-$ROOT}"
[ -f "$CREDS_ROOT/src-tauri/.env" ] || CREDS_ROOT="$(cd "$ROOT/.." && pwd)/alexandria"
[ -f "$CREDS_ROOT/src-tauri/.env" ] || { echo "no src-tauri/.env under $ROOT or $CREDS_ROOT — set CREDS_ROOT" >&2; exit 1; }
export BLOCKFROST_PROJECT_ID="${BLOCKFROST_PROJECT_ID:-$(grep '^BLOCKFROST_PROJECT_ID=' "$CREDS_ROOT/src-tauri/.env" | cut -d= -f2-)}"
export DEPLOYER_SIGNING_KEY="${DEPLOYER_SIGNING_KEY:-$CREDS_ROOT/keys/treasury.skey}"
export DEPLOYER_ADDRESS="${DEPLOYER_ADDRESS:-$(cat "$CREDS_ROOT/keys/treasury.addr")}"
python3 - <<'PY'
import deploy_blockfrost as d, json, os
sizes, files = d.extract_scripts()
utxos=[u for u in d.bf_get(f"addresses/{d.ADDR}/utxos?count=100") if all(a["unit"]=="lovelace" for a in u["amount"])]
utxos.sort(key=lambda u:-int(u["amount"][0]["quantity"]))
r=d.deploy_batch(["challenge_escrow"], utxos[0], sizes, files)
print("result:", r)
if not os.environ.get("DRY_RUN"):
    json.dump(r, open(os.path.join(d.OUT,"deployment_results_escrow.json"),"w"), indent=2)
    h,i=r["challenge_escrow"].split("#")
    print(f"\nNow run:  ./scripts/apply_escrow_ref.sh {h} {i}")
PY
