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
  # Build the offline wasmoon (Lua 5.4 -> WASM) bundle: embed glue.wasm as
  # base64 and patch the emscripten loader's fetch() to instantiate the embedded
  # bytes (this build doesn't whitelist wasmBinary/instantiateWasm). CSP-safe:
  # Lua runs inside wasm (wasm-unsafe-eval), no JS eval, no network.
  local work; work="$(mktemp -d)"
  ( cd "$work"
    npm init -y >/dev/null 2>&1
    npm i wasmoon@1.16.0 esbuild@0.24.0 >/dev/null 2>&1
    node -e 'const fs=require("fs");const b=fs.readFileSync("node_modules/wasmoon/dist/glue.wasm").toString("base64");fs.writeFileSync("wasmb64.js","export default \""+b+"\";")'
    cat > entry.js <<'JS'
import { LuaFactory } from "wasmoon";
import B64 from "./wasmb64.js";
globalThis.__CJLUA_WASM = Uint8Array.from(atob(B64), (c) => c.charCodeAt(0));
globalThis.CodejudgeLua = { newFactory: () => new LuaFactory("glue.wasm") };
JS
    cat > build.mjs <<'JS'
import * as esbuild from "esbuild";
import fs from "fs";
const SHIM = 'Promise.resolve(new Response(globalThis.__CJLUA_WASM,{headers:{"content-type":"application/wasm"}}))';
const patch = { name: "offline-wasm", setup(b) { b.onLoad({ filter: /wasmoon[\/\\]dist[\/\\]index\.js$/ }, async (a) => {
  let code = await fs.promises.readFile(a.path, "utf8");
  for (const v of ['fetch(a,{credentials:"same-origin"})', 'fetch(c,{credentials:"same-origin"})']) {
    if (!code.includes(v)) throw new Error("wasm fetch patch target missing: " + v);
    code = code.split(v).join(SHIM);
  }
  return { contents: code, loader: "js" };
}); } };
await esbuild.build({ entryPoints: ["entry.js"], bundle: true, format: "iife", platform: "browser", minify: true, outfile: process.argv[2], external: ["url", "module", "fs", "path", "crypto"], plugins: [patch] });
JS
    node build.mjs "$plug/ui/vendor/lua.js" >/dev/null 2>&1
  )
  rm -rf "$work"
  rm -f "$plug/ui/vendor/fengari-web.js"
  echo "  built lua.js ($(du -h "$plug/ui/vendor/lua.js" | cut -f1))"
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
