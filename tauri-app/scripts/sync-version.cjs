#!/usr/bin/env node

const fs = require("fs");
const path = require("path");

const ROOT = path.resolve(__dirname, "../..");
const APP_DIR = path.resolve(__dirname, "..");
const TAURI_DIR = path.join(APP_DIR, "src-tauri");

const rootCargoPath = path.join(ROOT, "Cargo.toml");
const rootCargo = fs.readFileSync(rootCargoPath, "utf8");
const workspacePackage = rootCargo.match(
  /\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/,
);
const versionMatch = workspacePackage?.[1].match(
  /^version\s*=\s*"([^"]+)"\s*$/m,
);

if (!versionMatch) {
  console.error("Failed to read [workspace.package].version from Cargo.toml");
  process.exit(1);
}

const version = versionMatch[1];
console.log(`Syncing GUI version: ${version}`);

const updateJsonVersion = (filePath, label) => {
  const document = JSON.parse(fs.readFileSync(filePath, "utf8"));
  if (document.version === version) return;
  document.version = version;
  fs.writeFileSync(filePath, `${JSON.stringify(document, null, 2)}\n`);
  console.log(`  Updated ${label}`);
};

updateJsonVersion(path.join(APP_DIR, "package.json"), "package.json");
updateJsonVersion(path.join(TAURI_DIR, "tauri.conf.json"), "tauri.conf.json");

const packageLockPath = path.join(APP_DIR, "package-lock.json");
const packageLock = JSON.parse(fs.readFileSync(packageLockPath, "utf8"));
let packageLockChanged = false;
if (packageLock.version !== version) {
  packageLock.version = version;
  packageLockChanged = true;
}
if (packageLock.packages?.[""]?.version !== version) {
  packageLock.packages[""].version = version;
  packageLockChanged = true;
}
if (packageLockChanged) {
  fs.writeFileSync(packageLockPath, `${JSON.stringify(packageLock, null, 2)}\n`);
  console.log("  Updated package-lock.json");
}

const tauriCargo = fs.readFileSync(path.join(TAURI_DIR, "Cargo.toml"), "utf8");
if (!/^version\.workspace\s*=\s*true\s*$/m.test(tauriCargo)) {
  console.error("src-tauri/Cargo.toml must inherit version.workspace");
  process.exit(1);
}

console.log("GUI version sync complete.");
