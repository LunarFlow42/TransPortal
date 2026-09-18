import { readdir, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { fileHash } from './seal.mjs';

export async function releaseChecksums(directory) {
  const files = (await readdir(directory, { withFileTypes: true }))
    .filter(file => file.isFile() && /\.(exe|msi|zip|dmg|deb|rpm|AppImage)$/.test(file.name))
    .map(file => file.name).sort();
  if (!files.length) throw new Error(`No release packages in ${directory}`);
  const lines = [];
  for (const file of files) {
    if (/[\r\n\\]/.test(file)) throw new Error('Unsupported checksum filename');
    lines.push(`${await fileHash(join(directory, file))}  ${file}`);
  }
  const output = join(directory, 'SHA256SUMS');
  await writeFile(output, lines.join('\n') + '\n', 'utf8');
  return output;
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  if (!process.argv[2]) throw new Error('Usage: node scripts/integrity/release-checksums.mjs <bundle directory>');
  console.log(await releaseChecksums(resolve(process.argv[2])));
}
