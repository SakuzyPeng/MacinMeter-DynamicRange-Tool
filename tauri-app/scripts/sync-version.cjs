#!/usr/bin/env node
/**
 * 版本同步脚本
 * 从主 Cargo.toml 读取版本号，同步到 Tauri 配置文件
 */

const fs = require('fs');
const path = require('path');

// scripts/ -> tauri-app/ -> MacinMeter-DynamicRange-Tool/
const ROOT = path.resolve(__dirname, '../..');
const TAURI_DIR = path.resolve(__dirname, '../src-tauri');

// 读取主 Cargo.toml 的版本号
const mainCargoPath = path.join(ROOT, 'Cargo.toml');
const mainCargo = fs.readFileSync(mainCargoPath, 'utf-8');
const versionMatch = mainCargo.match(/^version\s*=\s*"([^"]+)"/m);

if (!versionMatch) {
  console.error('Failed to read version from main Cargo.toml');
  process.exit(1);
}

const version = versionMatch[1];
console.log(`Syncing version: ${version}`);

// 同步 tauri.conf.json
const tauriConfPath = path.join(TAURI_DIR, 'tauri.conf.json');
const tauriConf = JSON.parse(fs.readFileSync(tauriConfPath, 'utf-8'));
if (tauriConf.version !== version) {
  tauriConf.version = version;
  fs.writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + '\n');
  console.log(`  Updated tauri.conf.json`);
}

// 同步 src-tauri/Cargo.toml
const tauriCargoPath = path.join(TAURI_DIR, 'Cargo.toml');
let tauriCargo = fs.readFileSync(tauriCargoPath, 'utf-8');
const newTauriCargo = tauriCargo.replace(
  /^(version\s*=\s*)"[^"]+"/m,
  `$1"${version}"`
);
if (tauriCargo !== newTauriCargo) {
  fs.writeFileSync(tauriCargoPath, newTauriCargo);
  console.log(`  Updated src-tauri/Cargo.toml`);
}

console.log('Version sync complete.');
