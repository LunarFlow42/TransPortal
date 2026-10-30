#!/usr/bin/env node
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const rawTag = process.argv[2];

if (!rawTag) {
  console.error('Usage: node scripts/sync-version.mjs <version-or-tag>');
  process.exit(1);
}

// Strip Git ref or 'v' prefix: "refs/tags/v0.1.1" -> "0.1.1", "v0.1.1" -> "0.1.1"
const version = rawTag.replace(/^refs\/tags\//, '').replace(/^v/, '').trim();
if (!/^\d+\.\d+\.\d+(?:-[\w.-]+)?(?:\+[\w.-]+)?$/.test(version)) {
  console.error(`Invalid semver version: "${version}" (from "${rawTag}")`);
  process.exit(1);
}

console.log(`Syncing project version to ${version}...`);

// 1. package.json
const pkgPath = resolve(root, 'package.json');
const pkg = JSON.parse(readFileSync(pkgPath, 'utf8'));
const oldVersion = pkg.version;
pkg.version = version;
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n', 'utf8');

// 2. src-tauri/tauri.conf.json
const tauriConfPath = resolve(root, 'src-tauri/tauri.conf.json');
let tauriContent = readFileSync(tauriConfPath, 'utf8');
tauriContent = tauriContent.replace(
  /("version"\s*:\s*")[^"]+(")/,
  `$1${version}$2`
);
writeFileSync(tauriConfPath, tauriContent, 'utf8');

// 3. src-tauri/Cargo.toml
const cargoPath = resolve(root, 'src-tauri/Cargo.toml');
let cargoContent = readFileSync(cargoPath, 'utf8');
cargoContent = cargoContent.replace(
  /(\[package\][\s\S]*?version\s*=\s*")[^"]+(")/,
  `$1${version}$2`
);
writeFileSync(cargoPath, cargoContent, 'utf8');

// 4. src-tauri/Cargo.lock (if present)
const lockPath = resolve(root, 'src-tauri/Cargo.lock');
if (existsSync(lockPath)) {
  let lockContent = readFileSync(lockPath, 'utf8');
  lockContent = lockContent.replace(
    /(name = "transportal"\r?\nversion = ")[^"]+(")/,
    `$1${version}$2`
  );
  writeFileSync(lockPath, lockContent, 'utf8');
}

console.log(`✔ Version successfully synced: ${oldVersion} -> ${version}`);
console.log(`  - package.json`);
console.log(`  - src-tauri/tauri.conf.json`);
console.log(`  - src-tauri/Cargo.toml`);
console.log(`  - src-tauri/Cargo.lock`);
