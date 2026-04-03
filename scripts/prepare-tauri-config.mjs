#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const mode = process.argv[2];
const configPath = path.resolve("src-tauri/tauri.conf.json");

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function writeJson(filePath, value) {
  fs.writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`);
}

function normalizeVersion(rawVersion) {
  const version = rawVersion.replace(/^v/, "");
  const semverPattern =
    /^\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?(?:\+[0-9A-Za-z.-]+)?$/;

  if (!semverPattern.test(version)) {
    throw new Error(
      `Release version "${rawVersion}" is not a valid semver version after normalization`,
    );
  }

  return version;
}

function applyDesktopValidationConfig() {
  const config = readJson(configPath);

  config.bundle ??= {};
  config.bundle.createUpdaterArtifacts = false;

  if (config.plugins && Object.prototype.hasOwnProperty.call(config.plugins, "updater")) {
    delete config.plugins.updater;
  }

  writeJson(configPath, config);
}

function syncVersionAcrossReleaseMetadata(rawVersion) {
  const version = normalizeVersion(rawVersion);

  const packagePath = path.resolve("package.json");
  const packageLockPath = path.resolve("package-lock.json");
  const cargoPath = path.resolve("src-tauri/Cargo.toml");

  const packageJson = readJson(packagePath);
  packageJson.version = version;
  writeJson(packagePath, packageJson);

  const packageLockJson = readJson(packageLockPath);
  packageLockJson.version = version;
  if (packageLockJson.packages && packageLockJson.packages[""]) {
    packageLockJson.packages[""].version = version;
  }
  writeJson(packageLockPath, packageLockJson);

  const tauriConfig = readJson(configPath);
  tauriConfig.version = version;
  writeJson(configPath, tauriConfig);

  const cargoToml = fs.readFileSync(cargoPath, "utf8");
  const updatedCargoToml = cargoToml.replace(
    /^version = "[^"]+"/m,
    `version = "${version}"`,
  );

  if (cargoToml === updatedCargoToml) {
    throw new Error("Could not update src-tauri/Cargo.toml version");
  }

  fs.writeFileSync(cargoPath, updatedCargoToml);
}

switch (mode) {
  case "desktop-validation":
    applyDesktopValidationConfig();
    break;
  case "sync-version":
    if (!process.argv[3]) {
      console.error("Missing version argument for sync-version mode");
      process.exit(1);
    }
    syncVersionAcrossReleaseMetadata(process.argv[3]);
    break;
  default:
    console.error(`Unsupported prepare-tauri-config mode: ${mode ?? "(missing)"}`);
    process.exit(1);
}
