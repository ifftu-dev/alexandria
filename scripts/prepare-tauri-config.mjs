#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const mode = process.argv[2];

if (mode !== "desktop-validation") {
  console.error(`Unsupported prepare-tauri-config mode: ${mode ?? "(missing)"}`);
  process.exit(1);
}

const configPath = path.resolve("src-tauri/tauri.conf.json");
const config = JSON.parse(fs.readFileSync(configPath, "utf8"));

config.bundle ??= {};
config.bundle.createUpdaterArtifacts = false;

if (config.plugins && Object.prototype.hasOwnProperty.call(config.plugins, "updater")) {
  delete config.plugins.updater;
}

fs.writeFileSync(configPath, `${JSON.stringify(config, null, 2)}\n`);
