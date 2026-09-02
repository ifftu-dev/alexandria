#!/usr/bin/env bash
# Patch CHALLENGE_ESCROW_REF_UTXO in script_refs.rs after deploy_escrow_only.sh.
# Usage: ./scripts/apply_escrow_ref.sh <tx_hash> <output_index>
set -euo pipefail
[ $# -eq 2 ] || { echo "usage: $0 <tx_hash> <output_index>" >&2; exit 1; }
python3 - "$(dirname "$0")/../src-tauri/src/cardano/script_refs.rs" "$1" "$2" <<'PY'
import re,sys
path,tx,idx=sys.argv[1:4]
t=open(path).read()
new,n=re.subn(r'pub const CHALLENGE_ESCROW_REF_UTXO: \(&str, u64\) = \([^)]*\);',
              f'pub const CHALLENGE_ESCROW_REF_UTXO: (&str, u64) = ("{tx}", {idx});', t, flags=re.S)
assert n==1, f"expected 1 match, got {n}"
open(path,"w").write(new); print(f"CHALLENGE_ESCROW_REF_UTXO -> ({tx}, {idx})")
PY
