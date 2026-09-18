import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { seal } from './seal.mjs';
import { releaseChecksums } from './release-checksums.mjs';

const sha256Abc = 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad';
async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'transportal-integrity-'));
  t.after(async () => {
    assert.equal(dirname(resolve(directory)), resolve(tmpdir()));
    assert.ok(basename(directory).startsWith('transportal-integrity-'));
    await rm(directory, { recursive: true, force: true });
  });
  return directory;
}

test('seals the final bytes into the installer manifest and adjacent reference', async (t) => {
  const directory = await fixture(t);
  const executable = join(directory, 'transportal.exe');
  await writeFile(executable, 'abc');
  assert.deepEqual(await seal(executable, '0.1.0'), {
    schema: 1, algorithm: 'sha256', application_version: '0.1.0',
    file: 'transportal.exe', file_size: 3, sha256: sha256Abc,
  });
  const reference = await readFile(join(directory, 'integrity.json'), 'utf8');
  assert.equal(await readFile(`${executable}.integrity.json`, 'utf8'), reference);
  await writeFile(executable, 'abcd');
  assert.equal(await readFile(join(directory, 'integrity.json'), 'utf8'), reference);
  assert.notEqual((await seal(executable, '0.1.0')).sha256, sha256Abc);
});

test('release checksum file covers the completed package and uses sha256sum format', async (t) => {
  const directory = await fixture(t);
  await writeFile(join(directory, 'TransPortal_0.1.0_x64-setup.exe'), 'abc');
  await writeFile(join(directory, 'unrelated.log'), 'not a release artifact');
  await writeFile(join(directory, 'SHA256SUMS'), 'stale checksum');
  const output = await releaseChecksums(directory);
  assert.equal(await readFile(output, 'utf8'), `${sha256Abc}  TransPortal_0.1.0_x64-setup.exe\n`);
});

test('restoring the build-directory binary does not overwrite the packaged baseline', async (t) => {
  const directory = await fixture(t);
  const executable = join(directory, 'transportal.exe');
  await writeFile(executable, 'abc');
  await seal(executable, '0.1.0');
  await writeFile(executable, 'abd');
  const standalone = await seal(executable, '0.1.0', { installerManifest: false });
  assert.equal(JSON.parse(await readFile(`${executable}.integrity.json`, 'utf8')).sha256, sha256Abc);
  assert.equal(await readFile(join(directory, 'integrity-bundle/transportal.exe'), 'utf8'), 'abc');
  assert.equal(JSON.parse(await readFile(join(directory, 'integrity-bundle/integrity.json'), 'utf8')).sha256, sha256Abc);
  assert.equal(JSON.parse(await readFile(join(directory, 'integrity.json'), 'utf8')).sha256, standalone.sha256);
  assert.notEqual(standalone.sha256, sha256Abc);
});

test('sealing rejects missing or non-file inputs instead of writing a usable baseline', async (t) => {
  const directory = await fixture(t);
  await assert.rejects(seal(directory, '0.1.0'), /regular/);
  await assert.rejects(seal(join(directory, 'missing.exe'), '0.1.0'), /ENOENT/);
  await assert.rejects(readFile(join(directory, 'integrity.json')), /ENOENT/);
});
