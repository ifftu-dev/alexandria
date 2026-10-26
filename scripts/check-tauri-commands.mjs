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
//   - the generated frontend command-name union is stale                   -> drift
//   - the generated chat vocabulary (src/generated/chat-vocab.ts) is stale   -> drift
//   - a security-sensitive retired command is restored anywhere            -> regression
//   - a Rust module outside src-tauri/src/commands (other than lib.rs)
//     imports the commands layer                                          -> direction
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
  // Any `::`-separated path in the handler list, taking the final segment.
  // Was `commands::\w+::(\w+)`, which missed commands registered from outside
  // `commands::` — notably the cfg-gated `crate::ee::…` ones, so they looked
  // unregistered to the invoked-but-not-registered check below.
  const re = /(?:\w+::)+(\w+)\s*,/g;
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
    if (/\.test\.[^/]+$/.test(file) || file.includes(`${path.sep}__tests__${path.sep}`)) continue;
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

function extractRetired() {
  const raw = JSON.parse(readText("scripts/retired-tauri-commands.json"));
  return new Set(Object.keys(raw).filter((k) => k !== "_comment"));
}

// --- generated frontend command names ------------------------------------
// This is deliberately derived from the backend handler registration rather
// than maintained as a second hand-written command list. It makes misspelled
// command literals fail TypeScript while preserving the existing generic
// result typing until Rust-derived argument/result generation lands.
const generatedCommandsPath = "src/generated/tauri-commands.ts";

function renderGeneratedCommands(commands) {
  const names = [...commands].sort();
  return [
    "// Generated by scripts/check-tauri-commands.mjs --write.",
    "// Do not edit by hand; the source of truth is tauri::generate_handler!.",
    "",
    "export const TAURI_COMMANDS = [",
    ...names.map((name) => `  ${JSON.stringify(name)},`),
    "] as const",
    "",
    "export type TauriCommand = (typeof TAURI_COMMANDS)[number]",
    "",
  ].join("\n");
}

// --- generated chat vocabulary -------------------------------------------
// Vocabularies the backend decides (error codes, proposal kinds, fence tags,
// locations, protocols, vendor host lists) come from src-tauri/src/ai_chat/
// vocab.rs. Its `chat_vocab_snapshot_is_current` test writes the snapshot
// below; this renders the snapshot as const arrays and union types so the
// webview never retypes a backend vocabulary.
const chatVocabSnapshotPath = "scripts/chat-vocab.json";
const generatedChatVocabPath = "src/generated/chat-vocab.ts";

// name -> [snapshot key, TypeScript union name, doc comment]
const chatVocabularies = [
  ["CHAT_ERROR_CODES", "chat_error_codes", "ChatErrorCode",
    "The code before the first colon of a chat or sign-in error. `rate_limited` and `http` carry a numeric detail after a second colon."],
  ["PROPOSAL_KINDS", "proposal_kinds", "ProposalKind",
    "What a chat may propose to save or change: the `kind` of a chat artifact body."],
  ["REPLY_FENCE_TAGS", "reply_fence_tags", "ReplyFenceTag",
    "Tags of the fenced blocks a reply may end with; each becomes a card."],
  ["CHAT_LOCATIONS", "chat_locations", "ChatLocation",
    "Where chat says a request goes."],
  ["PROTOCOLS", "protocols", "Protocol",
    "The wire protocol a connection speaks."],
  ["RESTRICTS_APPS_FOR_MINORS_HOSTS", "restricts_apps_for_minors_hosts", null,
    "Developer API hosts whose terms restrict apps that minors are likely to use."],
];

function readChatVocabSnapshot() {
  const snapshot = JSON.parse(readText(chatVocabSnapshotPath));
  const problems = [];
  const isNames = (value) => Array.isArray(value) && value.every((v) => typeof v === "string");
  for (const [, key, union] of chatVocabularies) {
    // A list that backs a union type must have at least one member; a plain
    // host list may be empty.
    if (!isNames(snapshot[key]) || (union && snapshot[key].length === 0)) problems.push(key);
  }
  const locations = snapshot.connection_locations;
  if (
    !Array.isArray(locations) ||
    locations.length === 0 ||
    !locations.every(
      (l) => typeof l?.name === "string" && (l.chat_location === null || typeof l.chat_location === "string"),
    )
  ) {
    problems.push("connection_locations");
  }
  if (problems.length) {
    throw new Error(`${chatVocabSnapshotPath} is missing or malformed: ${problems.join(", ")}`);
  }
  return snapshot;
}

function renderChatVocab(snapshot) {
  const lines = [
    "// Generated by scripts/check-tauri-commands.mjs --write from scripts/chat-vocab.json.",
    "// Do not edit by hand; the source of truth is src-tauri/src/ai_chat/vocab.rs,",
    "// whose `chat_vocab_snapshot_is_current` test keeps the snapshot current.",
    "",
  ];
  for (const [name, key, union, doc] of chatVocabularies) {
    lines.push(`/** ${doc} */`);
    lines.push(`export const ${name} = [`);
    lines.push(...snapshot[key].map((value) => `  ${JSON.stringify(value)},`));
    lines.push("] as const");
    if (union) lines.push("", `export type ${union} = (typeof ${name})[number]`);
    lines.push("");
  }
  lines.push(
    "/** Where a saved connection's endpoint runs, in the Studio connection's own names. */",
    "export const CONNECTION_LOCATIONS = [",
    ...snapshot.connection_locations.map((l) => `  ${JSON.stringify(l.name)},`),
    "] as const",
    "",
    "export type ConnectionLocation = (typeof CONNECTION_LOCATIONS)[number]",
    "",
    "/** The place chat names for each connection location; null when chat cannot use it. */",
    "export const CHAT_LOCATION_OF_CONNECTION = {",
    ...snapshot.connection_locations.map((l) => `  ${l.name}: ${JSON.stringify(l.chat_location)},`),
    "} as const satisfies Record<ConnectionLocation, ChatLocation | null>",
    "",
  );
  return lines.join("\n");
}

// --- diff -----------------------------------------------------------------
const registered = extractRegistered();
const invoked = extractInvoked();
const allow = extractAllowlist();
const retired = extractRetired();
const generatedCommands = renderGeneratedCommands(registered);
const failures = [];
let generatedChatVocab;
try {
  generatedChatVocab = renderChatVocab(readChatVocabSnapshot());
} catch (error) {
  failures.push(`Cannot render the chat vocabulary: ${error.message}`);
}

if (process.argv?.includes("--write")) {
  fs.mkdirSync(path.dirname(path.join(root, generatedCommandsPath)), { recursive: true });
  fs.writeFileSync(path.join(root, generatedCommandsPath), generatedCommands);
  if (generatedChatVocab !== undefined) {
    fs.writeFileSync(path.join(root, generatedChatVocabPath), generatedChatVocab);
  }
}

const orphans = [...registered]
  .filter((c) => !invoked.has(c) && !allow.has(c))
  .sort();
const stale = [...allow].filter((c) => invoked.has(c)).sort();
const dangling = [...allow].filter((c) => !registered.has(c)).sort();
const restored = [...retired]
  .filter((c) => registered.has(c) || invoked.has(c) || allow.has(c))
  .sort();
// The direction that actually breaks at runtime: the frontend calls a command
// the backend never registered, so the button silently does nothing until
// someone clicks it. Every other check here is about tidiness; this one is
// about the app working.
const missing = [...invoked].filter((c) => !registered.has(c)).sort();

let checkedInGeneratedCommands;
try {
  checkedInGeneratedCommands = readText(generatedCommandsPath);
} catch (error) {
  failures.push(`Missing generated command names: ${generatedCommandsPath} (${error.message}).`);
}
if (checkedInGeneratedCommands !== undefined && checkedInGeneratedCommands !== generatedCommands) {
  failures.push(
    `Generated command names are stale: ${generatedCommandsPath}. ` +
      "Run `npm run generate:tauri-commands` and commit the result.",
  );
}
if (generatedChatVocab !== undefined) {
  let checkedInChatVocab;
  try {
    checkedInChatVocab = readText(generatedChatVocabPath);
  } catch (error) {
    failures.push(`Missing generated chat vocabulary: ${generatedChatVocabPath} (${error.message}).`);
  }
  if (checkedInChatVocab !== undefined && checkedInChatVocab !== generatedChatVocab) {
    failures.push(
      `Generated chat vocabulary is stale: ${generatedChatVocabPath}. ` +
        "Run `npm run generate:chat-vocab` and commit the result.",
    );
  }
}
const profilePolicy = JSON.parse(readText("src/composables/profile-command-policy.json"));
const unscoped = new Set(Object.keys(profilePolicy.unscoped_commands));
const implementations = new Map();
for (const file of fs.readdirSync(path.join(root, "src-tauri/src/commands"))) {
  if (!file.endsWith(".rs")) continue;
  const source = readText(`src-tauri/src/commands/${file}`);
  const scopedState = /use crate::profile::scope::ProfileState as State;/.test(source);
  const commandPattern = /#\[tauri::command[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*pub (?:async )?fn (\w+)\s*\(([\s\S]*?)\)\s*(?:->|\{)/g;
  for (const match of source.matchAll(commandPattern)) {
    const scoped = /\bProfileLease\b/.test(match[2]) ||
      (scopedState && /(?<!tauri::)\bState\s*</.test(match[2]));
    const rows = implementations.get(match[1]) ?? [];
    rows.push({ file, scoped });
    implementations.set(match[1], rows);
  }
}
for (const command of registered) {
  const rows = implementations.get(command);
  if (!rows?.length) {
    failures.push(`Cannot verify profile scope for registered command ${command}.`);
    continue;
  }
  for (const row of rows) {
    if (row.scoped === unscoped.has(command)) {
      failures.push(`Profile scope mismatch: ${row.file}::${command}; require a lease or an explicit device/lifecycle exemption.`);
    }
  }
}
for (const command of unscoped) {
  if (!registered.has(command) || !profilePolicy.unscoped_commands[command]?.trim()) {
    failures.push(`Invalid or unexplained profile-scope exemption: ${command}.`);
  }
}
// --- dependency direction -------------------------------------------------
// Commands depend on the modules below them, never the reverse: no Rust
// module outside src-tauri/src/commands may name the commands layer.
// Behaviour another module needs moves into the module named for it.
// lib.rs is exempt: it registers the commands and runs the background tasks
// that call them. Comments are blanked first (keeping line numbers), so docs
// may point at a command. This is a text check, not name resolution.
const backendRoot = "src-tauri/src";
const commandsLayer = /\bcommands\s*::|\bcrate\s*::\s*commands\b|\bcrate\s*::\s*\{[^}]*\bcommands\b/g;

function walkRust(dir, out) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walkRust(full, out);
    else if (entry.name.endsWith(".rs")) out.push(full);
  }
}

function blankComments(source) {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, (comment) => comment.replace(/[^\n]/g, " "))
    .replace(/\/\/[^\n]*/g, "");
}

const rustFiles = [];
walkRust(path.join(root, backendRoot), rustFiles);
for (const file of rustFiles) {
  const rel = path.relative(root, file).split(path.sep).join("/");
  if (rel === `${backendRoot}/lib.rs` || rel.startsWith(`${backendRoot}/commands/`)) continue;
  const code = blankComments(readText(rel));
  for (const match of code.matchAll(commandsLayer)) {
    const line = code.slice(0, match.index).split("\n").length;
    failures.push(
      `A module outside the commands layer imports the commands layer: ${rel}:${line}. ` +
        "Move what it needs into the module that owns it and have the command call that.",
    );
  }
}

const frontendFiles = [];
walk(path.join(root, "src"), frontendFiles);
for (const file of frontendFiles) {
  if (file === path.join(root, "src/composables/useLocalApi.ts")) continue;
  const source = fs.readFileSync(file, "utf8");
  if (/import\s*\{[^}]*\binvoke\b[^}]*\}\s*from\s*['"]@tauri-apps\/api\/core['"]/.test(source)) {
    failures.push(`Direct frontend IPC bypasses the session bridge: ${path.relative(root, file)}.`);
  }
}
if (missing.length) {
  failures.push(
    `Invoked by the frontend but NOT registered (${missing.length}):\n` +
      missing.map((c) => `    - ${c}`).join("\n") +
      "\n  These fail at runtime the moment the calling UI is used.\n" +
      "  Fix: add the command to generate_handler! in lib.rs, or correct the " +
      "invoke() name.",
  );
}
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
if (restored.length) {
  failures.push(
    `Retired authority commands restored (${restored.length}):\n` +
      restored.map((c) => `    - ${c}`).join("\n") +
      "\n  These commands were removed because their legacy trust model was unsafe.",
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
