#!/usr/bin/env node

const fs = require("fs");
const path = require("path");

const ROOT = path.resolve(__dirname, "../..");
const APP_DIR = path.resolve(__dirname, "..");
const TAURI_DIR = path.join(APP_DIR, "src-tauri");
const mode = process.argv[2] ?? "--check";

if (!["--check", "--write"].includes(mode) || process.argv.length > 3) {
  console.error("Usage: node scripts/sync-version.cjs [--check|--write]");
  process.exit(2);
}

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
const packageJsonPath = path.join(APP_DIR, "package.json");
const packageLockPath = path.join(APP_DIR, "package-lock.json");
const tauriConfigPath = path.join(TAURI_DIR, "tauri.conf.json");
const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, "utf8"));
const packageLock = JSON.parse(fs.readFileSync(packageLockPath, "utf8"));
const tauriConfig = JSON.parse(fs.readFileSync(tauriConfigPath, "utf8"));
const tauriCargo = fs.readFileSync(path.join(TAURI_DIR, "Cargo.toml"), "utf8");

if (!packageLock.packages?.[""]) {
  console.error('package-lock.json must contain packages[""]');
  process.exit(1);
}
if (!/^version\.workspace\s*=\s*true\s*$/m.test(tauriCargo)) {
  console.error("src-tauri/Cargo.toml must inherit version.workspace");
  process.exit(1);
}

const files = [
  {
    label: "package.json",
    path: packageJsonPath,
    document: packageJson,
    fields: [["version"]],
  },
  {
    label: "package-lock.json",
    path: packageLockPath,
    document: packageLock,
    fields: [["version"], ["packages", "", "version"]],
  },
  {
    label: "tauri.conf.json",
    path: tauriConfigPath,
    document: tauriConfig,
    fields: [["version"]],
  },
];

const valueAt = (document, keys) =>
  keys.reduce((value, key) => value?.[key], document);
const setAt = (document, keys, value) => {
  const parent = keys
    .slice(0, -1)
    .reduce((current, key) => current[key], document);
  parent[keys.at(-1)] = value;
};

const mismatches = [];
for (const file of files) {
  for (const keys of file.fields) {
    const observed = valueAt(file.document, keys);
    if (observed !== version) {
      const field = keys.length === 1 ? keys[0] : keys.join(".");
      mismatches.push({
        file,
        keys,
        message: `${file.label} ${field}: ${observed ?? "<missing>"} != ${version}`,
      });
    }
  }
}

if (mode === "--check" && mismatches.length > 0) {
  console.error(`GUI version contract does not match workspace ${version}:`);
  for (const mismatch of mismatches) {
    console.error(`  - ${mismatch.message}`);
  }
  console.error("Run `npm run sync-version` to update the mirrors explicitly.");
  process.exit(1);
}

if (mode === "--write") {
  const changedFiles = new Set();
  for (const mismatch of mismatches) {
    setAt(mismatch.file.document, mismatch.keys, version);
    changedFiles.add(mismatch.file);
  }
  for (const file of changedFiles) {
    fs.writeFileSync(file.path, `${JSON.stringify(file.document, null, 2)}\n`);
    console.log(`  Updated ${file.label}`);
  }
  console.log(`GUI version mirrors synchronized to workspace ${version}.`);
} else {
  console.log(`GUI version contract matches workspace ${version}.`);
}
