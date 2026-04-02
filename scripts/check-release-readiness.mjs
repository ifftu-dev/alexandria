#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const root = process.cwd();

function readText(relativePath) {
  return fs.readFileSync(path.join(root, relativePath), "utf8");
}

function readJson(relativePath) {
  return JSON.parse(readText(relativePath));
}

function extractCargoPackageVersion(relativePath) {
  const text = readText(relativePath);
  const match = text.match(/^version = "([^"]+)"/m);
  if (!match) {
    throw new Error(`Could not find package version in ${relativePath}`);
  }
  return match[1];
}

function extractWorkflowInputDefault(workflowText, inputName) {
  const pattern = new RegExp(
    `${inputName}:\\n(?:[ \\t]+.+\\n)*?[ \\t]+default:\\s*(.+)`,
    "m",
  );
  const match = workflowText.match(pattern);
  return match ? match[1].trim().replace(/^['"]|['"]$/g, "") : null;
}

const failures = [];

const packageJson = readJson("package.json");
const tauriConfig = readJson("src-tauri/tauri.conf.json");
const androidConfig = readJson("src-tauri/tauri.android.conf.json");
const windowsDesktopConfig = readJson("src-tauri/tauri.desktop.windows.conf.json");

const cargoVersion = extractCargoPackageVersion("src-tauri/Cargo.toml");
const versions = {
  package_json: packageJson.version,
  cargo_toml: cargoVersion,
  tauri_conf: tauriConfig.version,
};

if (new Set(Object.values(versions)).size !== 1) {
  failures.push(
    `Version mismatch across release metadata: ${Object.entries(versions)
      .map(([key, value]) => `${key}=${value}`)
      .join(", ")}`,
  );
}

const updaterPubkey = tauriConfig.plugins?.updater?.pubkey ?? "";
if (!updaterPubkey || updaterPubkey === "PLACEHOLDER_PUBKEY") {
  failures.push("Updater public key is still unset or using PLACEHOLDER_PUBKEY.");
}

const commandsMod = readText("src-tauri/src/commands/mod.rs");
if (/pub use tutoring_stubs as tutoring;/.test(commandsMod)) {
  failures.push(
    "Android still routes tutoring commands through src-tauri/src/commands/tutoring_stubs.rs.",
  );
}

const androidFeatures = androidConfig.build?.features ?? [];
if (!Array.isArray(androidFeatures) || androidFeatures.length === 0) {
  failures.push("Android build config does not enable any mobile tutoring feature set.");
}

const windowsFeatures = windowsDesktopConfig.build?.features ?? [];
if (Array.isArray(windowsFeatures) && windowsFeatures.includes("tutoring-video")) {
  failures.push(
    "Windows desktop still builds with tutoring-video (no AEC), which does not match macOS/Linux desktop media parity.",
  );
}

const releaseMobileWorkflow = readText(".github/workflows/release-mobile.yml");
if (extractWorkflowInputDefault(releaseMobileWorkflow, "include_ios") !== "true") {
  failures.push("Release (Mobile) does not include iOS by default.");
}

if (failures.length > 0) {
  console.error("Release readiness check failed:");
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  process.exit(1);
}

console.log("Release readiness check passed.");
