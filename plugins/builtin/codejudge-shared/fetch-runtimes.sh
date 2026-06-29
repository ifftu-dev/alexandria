#!/usr/bin/env bash
# Deferred-fetch: download the pinned, third-party in-browser runtimes for the
# codejudge language plugins into each bundle's ui/vendor/ (gitignored) and copy
# the shared problem bank in. Runtimes are NOT committed — run this once before
# `cargo build` (or in CI) so builtins.rs can include_bytes! them into the app.
#
# End users never fetch anything: the bytes are embedded at build time and the
# plugin iframe runs fully offline (CSP connect-src 'none').
#
# Usage:  plugins/builtin/codejudge-shared/fetch-runtimes.sh [lua|javascript|python|all]
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BUILTIN="$(cd "$HERE/.." && pwd)"
PROBLEMS="$HERE/problems"

# --- pinned versions ---
CM_VER="5.65.16"                                  # CodeMirror 5 (single-file, no bundler)
CM="https://cdnjs.cloudflare.com/ajax/libs/codemirror/$CM_VER"
FENGARI="https://cdn.jsdelivr.net/npm/fengari-web@0.1.4/dist/fengari-web.js"

fetch() { # url dest
  echo "  fetch $(basename "$2")"
  curl -fsSL "$1" -o "$2"
}

vendor_common() { # plugin_dir cm_mode_path
  local plug="$1" mode="$2"
  local v="$plug/ui/vendor"
  mkdir -p "$v"
  fetch "$CM/codemirror.min.js"  "$v/codemirror.js"
  fetch "$CM/codemirror.min.css" "$v/codemirror.css"
  fetch "$CM/theme/material-darker.min.css" "$v/theme-material-darker.css"
  fetch "$CM/$mode" "$v/mode.js"
  # CSP forbids fetch/XHR in the iframe, so bake the problem bank into a JS
  # global the UI can read synchronously (demo/standalone fallback; real
  # elements receive their problem from the host's content_inline).
  python3 - "$PROBLEMS" "$plug/ui/problems.js" <<'PY'
import json, glob, os, sys
src, dest = sys.argv[1], sys.argv[2]
bank = {}
for f in sorted(glob.glob(os.path.join(src, "*.json"))):
    p = json.load(open(f))
    bank[p["id"]] = p
with open(dest, "w") as fh:
    fh.write("window.CODEJUDGE_PROBLEMS = " + json.dumps(bank, ensure_ascii=False) + ";\n")
print(f"  baked {len(bank)} problems -> problems.js")
PY
}

do_lua() {
  echo "== codejudge-lua =="
  local plug="$BUILTIN/codejudge-lua"
  vendor_common "$plug" "mode/lua/lua.min.js"
  fetch "$FENGARI" "$plug/ui/vendor/fengari-web.js"
}

do_javascript() {
  echo "== codejudge-javascript =="
  local plug="$BUILTIN/codejudge-javascript"
  vendor_common "$plug" "mode/javascript/javascript.min.js"
  # Build the offline QuickJS bundle: the singlefile-browser variant embeds the
  # wasm as base64 (no fetch) and esbuild bundles it to an IIFE that sets
  # globalThis.CodejudgeQuickJS. Pinned, no eval — CSP-safe in the iframe.
  local work; work="$(mktemp -d)"
  ( cd "$work"
    npm init -y >/dev/null 2>&1
    npm i quickjs-emscripten-core@0.31.0 @jitl/quickjs-singlefile-browser-release-sync@0.31.0 esbuild@0.24.0 >/dev/null 2>&1
    cat > entry.js <<'JS'
import { newQuickJSWASMModuleFromVariant } from "quickjs-emscripten-core";
import variant from "@jitl/quickjs-singlefile-browser-release-sync";
globalThis.CodejudgeQuickJS = { getQuickJS: () => newQuickJSWASMModuleFromVariant(variant) };
JS
    npx esbuild entry.js --bundle --format=iife --platform=browser --minify \
      --outfile="$plug/ui/vendor/quickjs.js" >/dev/null 2>&1
  )
  rm -rf "$work"
  echo "  built quickjs.js ($(du -h "$plug/ui/vendor/quickjs.js" | cut -f1))"
}

do_python() {
  echo "== codejudge-python =="
  local plug="$BUILTIN/codejudge-python"
  vendor_common "$plug" "mode/python/python.min.js"
  echo "  NOTE: Pyodide vendoring added when the Python runner lands."
}

bake_problems() { # dest_js
  python3 - "$PROBLEMS" "$1" <<'PY'
import json, glob, os, sys
src, dest = sys.argv[1], sys.argv[2]
bank = {}
for f in sorted(glob.glob(os.path.join(src, "*.json"))):
    p = json.load(open(f)); bank[p["id"]] = p
os.makedirs(os.path.dirname(dest), exist_ok=True)
open(dest, "w").write("window.CODEJUDGE_PROBLEMS = " + json.dumps(bank, ensure_ascii=False) + ";\n")
print(f"  baked {len(bank)} problems -> {os.path.basename(dest)}")
PY
}

do_multilang() {
  echo "== codejudge-multilang =="
  bake_problems "$BUILTIN/codejudge-multilang/ui/problems.js"
}

case "${1:-all}" in
  lua) do_lua ;;
  javascript) do_javascript ;;
  python) do_python ;;
  multilang) do_multilang ;;
  all) do_lua; do_javascript; do_multilang ;;
  *) echo "usage: $0 [lua|javascript|python|multilang|all]" >&2; exit 2 ;;
esac
echo "done."
