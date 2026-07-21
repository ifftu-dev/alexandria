#!/usr/bin/env node

// Guard the open-core boundary.
//
// The `ee/` subtrees are proprietary (IFFTU Enterprise License); everything
// else is MIT. The license is written as "if that directory exists", so a
// community distribution may ship with `ee/` deleted outright — and the
// build, the tests, and the type-check all still have to work.
//
// This script enforces the invariants that make that true. See
// docs/enterprise-boundary.md for the policy these rules implement.
//
// Fails (exit 1) when:
//   - core Rust references `crate::ee` / `ee::` without a `#[cfg(feature = "ee")]`
//   - `mod ee` is declared anywhere other than the single gated site in lib.rs
//   - a module imported via `@ee/` has no counterpart in BOTH src/ee and
//     src/ee-stub  -> the community build would break when ee/ is stripped
//   - anything outside src/ee reaches past the alias with a literal `@/ee/`
//   - a file under an ee/ tree is missing the Enterprise SPDX header
//   - a file under src/ee-stub is missing the MIT SPDX header
//
// Run from the alexandria/ directory (see package.json "check:ee-boundary").

import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const failures = [];

const EE_SPDX = "SPDX-License-Identifier: LicenseRef-IFFTU-Enterprise";
const MIT_SPDX = "SPDX-License-Identifier: MIT";

const RS_EE_DIR = "src-tauri/src/ee";
const TS_EE_DIR = "src/ee";
const TS_STUB_DIR = "src/ee-stub";

function fail(msg) {
  failures.push(msg);
}

function exists(rel) {
  return fs.existsSync(path.join(root, rel));
}

function walk(rel, filter, out = []) {
  const abs = path.join(root, rel);
  if (!fs.existsSync(abs)) return out;
  for (const entry of fs.readdirSync(abs, { withFileTypes: true })) {
    const childRel = path.join(rel, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules" || entry.name === "target") continue;
      walk(childRel, filter, out);
    } else if (filter(entry.name)) {
      out.push(childRel);
    }
  }
  return out;
}

const readText = (rel) => fs.readFileSync(path.join(root, rel), "utf8");

// --- 1. Rust: every core reference to `ee` must be cfg-gated --------------
// The community build is the default `cargo build`. If core ever names `ee`
// unconditionally, that build stops compiling — this catches it before the
// compiler has to.

const rustFiles = walk("src-tauri/src", (n) => n.endsWith(".rs")).filter(
  (f) => !f.startsWith(RS_EE_DIR + path.sep),
);

for (const file of rustFiles) {
  const lines = readText(file).split("\n");
  lines.forEach((line, i) => {
    const code = line.replace(/\/\/.*$/, ""); // ignore trailing comments
    if (!/\b(crate::ee\b|(?<![:\w])ee::)/.test(code)) return;

    // Gate may sit on the same line or the line above (the `mod ee;` shape).
    const prev = i > 0 ? lines[i - 1] : "";
    const gated = /#\[cfg\(feature\s*=\s*"ee"\)\]/.test(line + prev);
    if (!gated) {
      fail(
        `${file}:${i + 1}: references \`ee\` without #[cfg(feature = "ee")] — ` +
          `this breaks the community build.\n    ${line.trim()}`,
      );
    }
  });
}

// `mod ee` must be declared exactly once, gated, in lib.rs.
const modDecls = [];
for (const file of rustFiles) {
  readText(file)
    .split("\n")
    .forEach((line, i) => {
      if (/^\s*(pub\s+)?mod\s+ee\s*;/.test(line)) modDecls.push(`${file}:${i + 1}`);
    });
}
if (modDecls.length === 0) {
  fail(`no \`mod ee;\` declaration found — expected exactly one in src-tauri/src/lib.rs`);
} else if (modDecls.length > 1) {
  fail(
    `\`mod ee;\` declared ${modDecls.length} times (${modDecls.join(", ")}) — ` +
      `there must be exactly one seam.`,
  );
} else if (!modDecls[0].startsWith("src-tauri/src/lib.rs:")) {
  fail(`\`mod ee;\` must live in src-tauri/src/lib.rs, found at ${modDecls[0]}`);
}

// --- 2. Frontend: `@ee/` imports must resolve in both trees ---------------
// `@ee` is an alias that points at src/ee-stub (community) or src/ee
// (enterprise). Importing through it is fine and expected. What is NOT fine
// is importing a module that only one side implements — the other build
// breaks. Both trees must carry a counterpart.

const tsFiles = walk("src", (n) => /\.(ts|tsx|vue)$/.test(n));
const importRe = /from\s+['"]@ee\/([^'"]+)['"]|import\(\s*['"]@ee\/([^'"]+)['"]\s*\)/g;

for (const file of tsFiles) {
  const text = readText(file);
  for (const m of text.matchAll(importRe)) {
    const spec = (m[1] ?? m[2]).replace(/\.(ts|js)$/, "");
    for (const dir of [TS_EE_DIR, TS_STUB_DIR]) {
      const hit = [".ts", ".tsx", "/index.ts"].some((ext) => exists(`${dir}/${spec}${ext}`));
      // src/ee may legitimately be absent in a stripped community checkout;
      // only hold it to the contract when the tree is present.
      if (dir === TS_EE_DIR && !exists(TS_EE_DIR)) continue;
      if (!hit) {
        fail(`${file}: imports '@ee/${spec}' but ${dir}/${spec}.ts does not exist.`);
      }
    }
  }

  // Reaching past the alias hard-codes the enterprise tree into core.
  if (!file.startsWith(TS_EE_DIR + path.sep)) {
    const lines = text.split("\n");
    lines.forEach((line, i) => {
      if (/from\s+['"]@\/ee\//.test(line)) {
        fail(
          `${file}:${i + 1}: imports '@/ee/...' directly. Use the '@ee/' alias ` +
            `so the community build resolves to the MIT stub.\n    ${line.trim()}`,
        );
      }
    });
  }
}

// --- 3. SPDX headers ------------------------------------------------------
// Scoped to the boundary trees. The wider codebase predates this convention;
// enforcing it repo-wide is a separate cleanup, not a boundary guard.

const headerChecks = [
  { dir: RS_EE_DIR, ext: /\.rs$/, want: EE_SPDX, label: "Enterprise" },
  { dir: TS_EE_DIR, ext: /\.(ts|tsx|vue)$/, want: EE_SPDX, label: "Enterprise" },
  { dir: TS_STUB_DIR, ext: /\.(ts|tsx|vue)$/, want: MIT_SPDX, label: "MIT" },
];

for (const { dir, ext, want, label } of headerChecks) {
  for (const file of walk(dir, (n) => ext.test(n))) {
    const head = readText(file).split("\n").slice(0, 5).join("\n");
    if (!head.includes(want)) {
      fail(`${file}: missing ${label} SPDX header (\`${want}\`) in the first 5 lines.`);
    }
  }
}

// Every ee/ tree must carry its license alongside the code it covers.
for (const dir of [RS_EE_DIR, TS_EE_DIR]) {
  if (exists(dir) && !exists(`${dir}/LICENSE.md`)) {
    fail(`${dir}/LICENSE.md is missing — the enterprise carve-out must ship its license.`);
  }
}

// --- report ---------------------------------------------------------------
if (failures.length > 0) {
  console.error("\nEnterprise boundary violations:\n");
  for (const f of failures) console.error(`  ✘ ${f}`);
  console.error(`\n${failures.length} violation(s). See docs/enterprise-boundary.md.\n`);
  process.exit(1);
}

console.log("Enterprise boundary OK.");
