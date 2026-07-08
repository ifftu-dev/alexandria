#!/usr/bin/env node

// Guard against orphan Tauri commands.
//
// Cross-checks three sets:
//   registered  = commands wired into tauri::generate_handler! in src-tauri/src/lib.rs
//   invoked     = command-name string literals passed to invoke()/tauriInvoke() in src/**
//   allowlist   = commands that intentionally have no frontend caller yet
//                 (scripts/tauri-command-allowlist.json)
//
// Fails (exit 1) when:
//   - a registered command has no invoke() caller AND is not allowlisted  -> orphan
//   - an allowlisted command now HAS a caller                             -> stale allowlist
//   - an allowlisted command is no longer registered                      -> dangling allowlist
//
// Run from the alexandria/ directory (see package.json "check:tauri-commands").

import fs from "node:fs";
import path from "node:path";

const root = process.cwd();

function readText(rel) {
  return fs.readFileSync(path.join(root, rel), "utf8");
}

// --- registered set -------------------------------------------------------
// Registration lines look like `commands::<module>::<name>,` (one per line,
// with `//` comment lines interspersed). We scope to the generate_handler!
// block so unrelated `commands::` mentions elsewhere can't leak in.
function extractRegistered() {
  const lib = readText("src-tauri/src/lib.rs");
  const start = lib.indexOf("generate_handler!");
  if (start === -1) {
    throw new Error("could not find generate_handler! in src-tauri/src/lib.rs");
  }
  // Walk from the macro's opening bracket to its matching close.
  const open = lib.indexOf("[", start);
  let depth = 0;
  let end = -1;
  for (let i = open; i < lib.length; i++) {
    const c = lib[i];
    if (c === "[") depth++;
    else if (c === "]") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  if (end === -1) throw new Error("unbalanced generate_handler! bracket");
  const block = lib.slice(open, end);
  const set = new Set();
  const re = /commands::\w+::(\w+)/g;
  let m;
  while ((m = re.exec(block)) !== null) set.add(m[1]);
  return set;
}

// --- invoked set ----------------------------------------------------------
// Recursively walk src/ for .ts/.vue files and capture the first string
// literal argument of any invoke()/tauriInvoke()/<alias>() call, where alias
// is anything imported as `invoke as X` from @tauri-apps/api/core.
function walk(dir, out) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules") continue;
      walk(full, out);
    } else if (/\.(ts|vue|tsx|js)$/.test(entry.name)) {
      out.push(full);
    }
  }
}

function extractInvoked() {
  const files = [];
  walk(path.join(root, "src"), files);
  const set = new Set();
  const aliasRe =
    /import\s*\{[^}]*\binvoke\s+as\s+(\w+)[^}]*\}\s*from\s*['"]@tauri-apps\/api\/core['"]/g;
  for (const file of files) {
    const text = fs.readFileSync(file, "utf8");
    // Base call idents: bare `invoke` (wrapper + direct) plus any core alias.
    const idents = new Set(["invoke", "tauriInvoke"]);
    let a;
    while ((a = aliasRe.exec(text)) !== null) idents.add(a[1]);
    const identGroup = [...idents].map((i) => i.replace(/[^\w]/g, "")).join("|");
    // <ident>[<T>]( 'literal' — matches wrapped/commented call forms:
    //   invoke(\n  'cmd', ...)                 (literal on its own line)
    //   invoke<{ a: number; b: number }>('cmd')(object-type generic)
    //   invoke<Record<string, unknown> | null>('cmd')  (nested/union generic)
    //   invoke(\n  // note\n  'cmd', ...)       (comment before the literal)
    // GAP spans whitespace and JS comments; the generic class excludes only
    // parens/quotes so it stops at the real call paren.
    const GAP = "(?:\\s|//[^\\n]*|/\\*[\\s\\S]*?\\*/)*";
    const callRe = new RegExp(
      `\\b(?:${identGroup})${GAP}(?:<[^()'"\`]*)?${GAP}\\(${GAP}(['"\`])([a-zA-Z_][\\w]*)\\1`,
      "g",
    );
    let m;
    while ((m = callRe.exec(text)) !== null) set.add(m[2]);
    aliasRe.lastIndex = 0;
  }
  return set;
}

// --- allowlist ------------------------------------------------------------
function extractAllowlist() {
  const raw = JSON.parse(readText("scripts/tauri-command-allowlist.json"));
  return new Set(Object.keys(raw).filter((k) => k !== "_comment"));
}

// --- diff -----------------------------------------------------------------
const registered = extractRegistered();
const invoked = extractInvoked();
const allow = extractAllowlist();

const orphans = [...registered]
  .filter((c) => !invoked.has(c) && !allow.has(c))
  .sort();
const stale = [...allow].filter((c) => invoked.has(c)).sort();
const dangling = [...allow].filter((c) => !registered.has(c)).sort();

const failures = [];
if (orphans.length) {
  failures.push(
    `Registered but never invoked and not allowlisted (${orphans.length}):\n` +
      orphans.map((c) => `    - ${c}`).join("\n") +
      "\n  Fix: add a frontend invoke() caller, or add it to " +
      "scripts/tauri-command-allowlist.json with a reason.",
  );
}
if (stale.length) {
  failures.push(
    `Allowlisted commands that NOW have a frontend caller (${stale.length}):\n` +
      stale.map((c) => `    - ${c}`).join("\n") +
      "\n  Fix: remove these from scripts/tauri-command-allowlist.json — " +
      "the UI gap is closed.",
  );
}
if (dangling.length) {
  failures.push(
    `Allowlisted commands that are no longer registered (${dangling.length}):\n` +
      dangling.map((c) => `    - ${c}`).join("\n") +
      "\n  Fix: remove these from scripts/tauri-command-allowlist.json.",
  );
}

if (failures.length) {
  console.error("Tauri command guard failed:\n");
  for (const f of failures) console.error(`- ${f}\n`);
  console.error(
    `Summary: ${registered.size} registered, ${invoked.size} invoked, ${allow.size} allowlisted.`,
  );
  process.exit(1);
}

console.log(
  `Tauri command guard passed: ${registered.size} registered, ` +
    `${invoked.size} invoked, ${allow.size} allowlisted (all accounted for).`,
);
