import { spawn } from 'node:child_process';
import { copyFile, mkdir, readFile, readdir, realpath, rm } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const osAliases = { win32: 'windows', darwin: 'macos' };
const archAliases = { x64: 'x86_64', arm64: 'aarch64' };
const rawOs = process.env.TAURI_ENV_PLATFORM || process.platform;
const rawArch = process.env.TAURI_ENV_ARCH || process.arch;
const os = osAliases[rawOs] || rawOs;
const arch = archAliases[rawArch] || rawArch;
const platforms = arch === 'universal' && os === 'macos'
  ? ['macos-x86_64', 'macos-aarch64'] : [`${os}-${arch}`];
const extra = process.argv.slice(2);
const platformOption = extra.indexOf('--platform');
if (platformOption >= 0) platforms.splice(0, platforms.length, extra[platformOption + 1]);
const destinationOption = extra.indexOf('--assets-dir');
const source = destinationOption >= 0 ? resolve(root, extra[destinationOption + 1]) : resolve(root, 'src-tauri/resources/ocr');

for (const platform of platforms) {
  const args = ['run', '--manifest-path', 'src-tauri/Cargo.toml', '--example', 'ocr_probe', '--',
    'prepare', '--platform', platform, ...extra];
  const code = await new Promise((accept, reject) => {
    const child = spawn('cargo', args, { cwd: root, stdio: 'inherit', windowsHide: true });
    child.on('error', reject);
    child.on('exit', (code) => accept(code ?? 1));
  });
  if (code !== 0) process.exit(code);
}

// Stage a strict whitelist for the target platform. Other platform caches,
// partial downloads and debug files must never inflate the application bundle.
const manifest = JSON.parse(await readFile(resolve(root, 'src-tauri/resources/ocr/manifest.json'), 'utf8'));
const bundle = resolve(root, 'src-tauri/resources/ocr-bundle');
await mkdir(bundle, { recursive: true });
if ((await realpath(bundle)).toLowerCase() !== bundle.toLowerCase()) throw new Error('OCR bundle directory must not be a symbolic link');
for (const entry of await readdir(bundle, { withFileTypes: true })) {
  if (entry.name === 'README.md') continue;
  const target = resolve(bundle, entry.name);
  if (dirname(target) !== bundle) throw new Error('Invalid OCR staging path');
  await rm(target, { recursive: true, force: true });
}
for (const platform of platforms) {
  const runtime = manifest.runtimes.find(runtime => runtime.platform === platform);
  if (!runtime) throw new Error(`No runtime manifest for ${platform}`);
  const sourcePack = join(source, manifest.id, platform);
  const destinationPack = join(bundle, manifest.id, platform);
  await mkdir(join(destinationPack, 'licenses'), { recursive: true });
  const files = [...manifest.models, ...runtime.files].map(file => file.name);
  for (const file of [...files, 'NOTICE.md', 'manifest.json']) {
    await copyFile(join(sourcePack, file), join(destinationPack, file));
  }
  for (const file of await readdir(join(root, 'src-tauri/resources/ocr/licenses'))) {
    await copyFile(join(sourcePack, 'licenses', file), join(destinationPack, 'licenses', file));
  }
}
console.log(`OCR bundle prepared: ${platforms.join(', ')}`);
