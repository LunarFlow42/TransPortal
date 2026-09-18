import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { copyFile, lstat, mkdir, rename, writeFile } from 'node:fs/promises';
import { basename, dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export async function fileHash(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}

async function atomicWrite(path, contents) {
  const temporary = `${path}.tmp-${process.pid}`;
  await writeFile(temporary, contents, { encoding: 'utf8' });
  await rename(temporary, path);
}

export async function seal(binary, version, { installerManifest = true } = {}) {
  binary = resolve(binary);
  const info = await lstat(binary);
  if (!info.isFile() || info.isSymbolicLink() || !info.size) throw new Error('Expected a regular, built executable');
  if (!/^\d+\.\d+\.\d+(?:-[\w.-]+)?(?:\+[\w.-]+)?$/.test(version)) throw new Error('Invalid application version');
  const manifest = {
    schema: 1, algorithm: 'sha256', application_version: version,
    file: basename(binary), file_size: info.size, sha256: await fileHash(binary),
  };
  const after = await lstat(binary);
  if (after.size !== info.size || after.mtimeMs !== info.mtimeMs) throw new Error('Executable changed while computing its release hash');
  const json = JSON.stringify(manifest, null, 2) + '\n';
  if (installerManifest) {
    // Keep the exact bytes NSIS reads for verification: Tauri restores the
    // build-directory executable's original bundle marker after packaging.
    const snapshot = join(dirname(binary), 'integrity-bundle');
    await mkdir(snapshot, { recursive: true });
    const snapshotBinary = join(snapshot, basename(binary));
    await copyFile(binary, snapshotBinary);
    if (await fileHash(snapshotBinary) !== manifest.sha256) throw new Error('Executable changed while preparing the package snapshot');
    await atomicWrite(join(snapshot, 'integrity.json'), json);
    await atomicWrite(`${binary}.integrity.json`, json);
  }
  // The release wrapper refreshes only these two files after Tauri restores its
  // original executable. Never replace the already-packaged manifest then.
  await atomicWrite(join(dirname(binary), 'integrity.json'), json);
  await atomicWrite(`${binary}.sha256`, `${manifest.sha256}  ${manifest.file}\n`);
  return manifest;
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  const [binary, version] = process.argv.slice(2);
  if (!binary || !version) throw new Error('Usage: node scripts/integrity/seal.mjs <built executable> <version>');
  const manifest = await seal(binary, version);
  console.log(`SHA-256 sealed: ${manifest.file} (${manifest.file_size} bytes)`);
}
