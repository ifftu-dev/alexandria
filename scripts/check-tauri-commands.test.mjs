import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import vm from "node:vm";

const root = process.cwd();
const script = fs.readFileSync(path.join(root, "scripts/check-tauri-commands.mjs"), "utf8")
  .replace(/^import .*;\n/gm, "");

function runGuard(overrides = {}) {
  const messages = [];
  let exitCode = 0;
  const exit = Symbol("guard exit");
  try {
    vm.runInNewContext(script, {
      fs: {
        readdirSync: fs.readdirSync,
        readFileSync(file, encoding) {
          const original = fs.readFileSync(file, encoding);
          const override = overrides[path.relative(root, file)];
          return override ? override(original) : original;
        },
        mkdirSync: fs.mkdirSync,
        writeFileSync: fs.writeFileSync,
      },
      path,
      process: {
        cwd: () => root,
        argv: [],
        exit(code) { exitCode = code; throw exit; },
      },
      console: {
        log: message => messages.push(message),
        error: message => messages.push(message),
      },
    });
  } catch (error) {
    if (error !== exit) throw error;
  }
  return { exitCode, output: messages.join("\n") };
}

function removeLease(source) {
  const scoped = "use crate::profile::scope::ProfileState as State;";
  assert.ok(source.includes(scoped), "fixture must remove a real lease");
  return source.replace(scoped, "use tauri::State;");
}

test("current command scope policy passes", () => {
  assert.equal(runGuard().exitCode, 0);
});

test("removing a backend command lease fails the guard", () => {
  const result = runGuard({ "src-tauri/src/commands/settings.rs": removeLease });
  assert.equal(result.exitCode, 1);
  assert.match(result.output, /Profile scope mismatch: settings.rs::/);
});

test("mobile-only scope regressions are checked on desktop", () => {
  const result = runGuard({ "src-tauri/src/commands/tutoring_mobile.rs": removeLease });
  assert.equal(result.exitCode, 1);
  assert.match(result.output, /Profile scope mismatch: tutoring_mobile.rs::/);
});

test("new unexplained exemptions fail the guard", () => {
  const result = runGuard({
    "src/composables/profile-command-policy.json": source => {
      const policy = JSON.parse(source);
      policy.unscoped_commands.check_health = "";
      return JSON.stringify(policy);
    },
  });
  assert.equal(result.exitCode, 1);
  assert.match(result.output, /Invalid or unexplained profile-scope exemption: check_health/);
});

test("direct frontend transport imports fail the guard", () => {
  const result = runGuard({
    "src/App.vue": source => "import { invoke as bypass } from '@tauri-apps/api/core'\n" + source,
  });
  assert.equal(result.exitCode, 1);
  assert.match(result.output, /Direct frontend IPC bypasses the session bridge: src\/App.vue/);
});

test("stale generated command names fail the guard", () => {
  const result = runGuard({
    "src/generated/tauri-commands.ts": source => source.replace('  "check_health",\n', ""),
  });
  assert.equal(result.exitCode, 1);
  assert.match(result.output, /Generated command names are stale/);
});

test("retired authority commands cannot be registered again", () => {
  const result = runGuard({
    "src-tauri/src/lib.rs": source => source.replace(
      "commands::health::check_health,",
      "commands::health::check_health,\n            commands::plugins::plugin_ingest_attestation,",
    ),
  });
  assert.equal(result.exitCode, 1);
  assert.match(result.output, /Retired authority commands restored/);
  assert.match(result.output, /plugin_ingest_attestation/);
});
