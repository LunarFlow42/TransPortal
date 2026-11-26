import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { spawn } from 'node:child_process';
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
let fixture: string;
let scriptPath: string;

beforeEach(async () => {
  fixture = await mkdtemp(resolve(tmpdir(), 'transportal-version-test-'));
  await mkdir(resolve(fixture, 'scripts'));
  await mkdir(resolve(fixture, 'src-tauri'));
  scriptPath = resolve(fixture, 'scripts/sync-version.mjs');
  await copyFile(resolve(root, 'scripts/sync-version.mjs'), scriptPath);
  await writeFile(resolve(fixture, 'package.json'), JSON.stringify({ name: 'transportal', version: '0.1.0' }) + '\n');
  await writeFile(resolve(fixture, 'src-tauri/tauri.conf.json'), '{"version":"0.1.0"}\n');
  await writeFile(resolve(fixture, 'src-tauri/Cargo.toml'), '[package]\nname = "transportal"\nversion = "0.1.0"\n');
  await writeFile(resolve(fixture, 'src-tauri/Cargo.lock'), '[[package]]\nname = "transportal"\nversion = "0.1.0"\n');
});
afterEach(async () => { await rm(fixture, { recursive: true, force: true }); });

function runSync(arg: string) {
  return new Promise<{ code: number | null; stdout: string; stderr: string }>((resolveResult) => {
    const child = spawn(process.execPath, [scriptPath, arg], {
      cwd: fixture,
      stdio: 'pipe',
    });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (d) => (stdout += String(d)));
    child.stderr.on('data', (d) => (stderr += String(d)));
    child.on('close', (code) => {
      resolveResult({ code, stdout, stderr });
    });
  });
}

describe('sync-version script tests', () => {
  it('rejects invalid version formats', async () => {
    const res = await runSync('invalid-version');
    expect(res.code).toBe(1);
    expect(res.stderr).toMatch(/Invalid semver/);
  });

  it('accepts v-prefixed and raw versions', async () => {
    // Only the temporary fixture is mutated, never the repository's version.
    const res1 = await runSync('v0.1.0');
    expect(res1.code).toBe(0);
    expect(res1.stdout).toMatch(/Version successfully synced: 0\.1\.0 -> 0\.1\.0/);

    const res2 = await runSync('refs/tags/v0.1.0');
    expect(res2.code).toBe(0);
    expect(res2.stdout).toMatch(/Version successfully synced: 0\.1\.0 -> 0\.1\.0/);
  });

  it('updates all four fixture files without changing the real project', async () => {
    const before = await readFile(resolve(root, 'package.json'), 'utf8');
    const result = await runSync('v1.2.3');
    expect(result.code).toBe(0);
    for (const file of ['package.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock']) {
      expect(await readFile(resolve(fixture, file), 'utf8')).toContain('1.2.3');
    }
    expect(await readFile(resolve(root, 'package.json'), 'utf8')).toBe(before);
  });
});
