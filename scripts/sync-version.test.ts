import { describe, expect, it } from 'vitest';
import { spawn } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const scriptPath = resolve(root, 'scripts/sync-version.mjs');

function runSync(arg: string) {
  return new Promise<{ code: number | null; stdout: string; stderr: string }>((resolveResult) => {
    const child = spawn(process.execPath, [scriptPath, arg], {
      cwd: root,
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
    // Keep current 0.1.0 version idempotent
    const res1 = await runSync('v0.1.0');
    expect(res1.code).toBe(0);
    expect(res1.stdout).toMatch(/Version successfully synced: 0\.1\.0 -> 0\.1\.0/);

    const res2 = await runSync('refs/tags/v0.1.0');
    expect(res2.code).toBe(0);
    expect(res2.stdout).toMatch(/Version successfully synced: 0\.1\.0 -> 0\.1\.0/);
  });
});
