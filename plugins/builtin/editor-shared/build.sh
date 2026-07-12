#!/usr/bin/env bash
# Build the editor plugins' runtime artifacts. For each language:
#   1. compile the Rust runner crate to wasm32-unknown-unknown (Boa/RustPython)
#   2. import-stub it to zero imports (wasmstub) -> grader/dist/<lang>_grader.wasm
#   3. base64-inline the stubbed wasm -> ui/vendor/runner-wasm.js (worker loads it)
#   4. build the shared CodeMirror 6 bundle -> ui/vendor/cm6.js
#   5. write the grader CID (BLAKE3) into manifest.json (grader.cid + grader.blake3)
#
# The stubbed wasm serves BOTH the host grader (Wasmtime) and in-browser live
# eval (Worker), so a passing live run equals the graded result by construction.
# End users fetch nothing: builtins.rs include_bytes! the results at build time.
#
# Usage: editor-shared/build.sh [javascript|all]
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BUILTIN="$(cd "$HERE/.." && pwd)"

# --- shared tools ---
build_wasmstub() {
  echo "== wasmstub tool =="
  ( cd "$HERE/tools/wasmstub" && cargo build --release >/dev/null )
}

build_cm6() {
  echo "== CodeMirror 6 bundle =="
  ( cd "$HERE/cm6-build" && npm install --silent && npm run build --silent )
}

# --- per-language runner build ---
# args: <slug> <crate_wasm_name>
build_lang() {
  local slug="$1" wasm="$2"
  local plug="$BUILTIN/$slug"
  echo "== $slug =="

  ( cd "$plug/grader" && cargo build --target wasm32-unknown-unknown --release >/dev/null )

  local raw="$plug/grader/target/wasm32-unknown-unknown/release/$wasm.wasm"
  local dist="$plug/grader/dist/$wasm.wasm"
  mkdir -p "$plug/grader/dist"
  local cid
  cid="$("$HERE/tools/wasmstub/target/release/wasmstub" "$raw" "$dist" | sed -n 's/^OUTPUT_BLAKE3=//p')"
  echo "  grader CID $cid"

  # base64-inline the stubbed wasm for the worker
  mkdir -p "$plug/ui/vendor"
  python3 - "$dist" "$plug/ui/vendor/runner-wasm.js" <<'PY'
import base64, sys
b = open(sys.argv[1], 'rb').read()
with open(sys.argv[2], 'w') as f:
    f.write("// AUTO-GENERATED. Base64-inlined grader wasm (import-stubbed, zero imports).\n")
    f.write("// Instantiated with an empty import object in the runner worker; no fetch.\n")
    f.write("self.ALEX_RUNNER_WASM_B64 = \"%s\";\n" % base64.b64encode(b).decode())
PY

  cp "$HERE/cm6-build/dist/cm6.js" "$plug/ui/vendor/cm6.js"

  # write CID into the manifest
  python3 - "$plug/manifest.json" "$cid" <<'PY'
import json, sys
p, cid = sys.argv[1], sys.argv[2]
m = json.load(open(p))
m["grader"]["cid"] = cid
m["grader"]["blake3"] = cid
json.dump(m, open(p, 'w'), indent=2)
open(p, 'a').write("\n")
PY
  echo "  wrote ui/vendor/{runner-wasm.js,cm6.js} + manifest.grader.cid"
}

target="${1:-all}"
build_wasmstub
build_cm6
case "$target" in
  javascript|all) build_lang editor-javascript editor_javascript_grader ;;
esac
case "$target" in
  typescript|all)
    # The TS grader embeds sucrase via include_str!; refresh it from the bundle.
    cp "$HERE/cm6-build/dist/sucrase.js" "$BUILTIN/editor-typescript/grader/src/sucrase.js"
    build_lang editor-typescript editor_typescript_grader
    ;;
esac
case "$target" in
  cpp|all)
    # The C/C++ grader embeds JSCPP via include_str!; refresh it from the bundle.
    cp "$HERE/cm6-build/dist/jscpp.js" "$BUILTIN/editor-cpp/grader/src/jscpp.js"
    build_lang editor-cpp editor_cpp_grader
    ;;
esac
echo "done."
