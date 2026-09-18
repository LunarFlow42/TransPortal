import { spawn } from 'node:child_process';
import { readdir } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { releaseChecksums } from './integrity/release-checksums.mjs';
import { seal } from './integrity/seal.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
if (args.includes('--no-bundle')) throw new Error('build:release requires a package; use tauri build --no-bundle for an intermediate executable');
const code = await new Promise((accept, reject) => {
  const child = spawn(process.execPath, [join(root, 'node_modules/@tauri-apps/cli/tauri.js'), 'build', ...args],
    { cwd: root, stdio: 'inherit', windowsHide: true });
  child.on('error', reject);
  child.on('exit', code => accept(code ?? 1));
});
if (code !== 0) process.exit(code);
const targetIndex = args.findIndex(arg => arg === '--target' || arg === '-t');
const target = targetIndex >= 0 ? args[targetIndex + 1] : args.find(arg => arg.startsWith('--target='))?.slice('--target='.length) || '';
const metadata = await new Promise((accept, reject) => {
  const child = spawn('cargo', ['metadata', '--offline', '--no-deps', '--format-version', '1'],
    { cwd: join(root, 'src-tauri'), stdio: ['ignore', 'pipe', 'inherit'], windowsHide: true });
  let output = '';
  child.stdout.setEncoding('utf8').on('data', chunk => { output += chunk; });
  child.on('error', reject);
  child.on('exit', code => {
    if (code !== 0) { reject(new Error('Could not locate Cargo build output')); return; }
    try { accept(JSON.parse(output)); } catch (error) { reject(error); }
  });
});
const targetRoot = metadata.target_directory;
const buildDirectory = join(targetRoot, target || '', args.includes('--debug') || args.includes('-d') ? 'debug' : 'release');
const bundleRoot = join(buildDirectory, 'bundle');
if (target ? target.includes('windows') : process.platform === 'win32') {
  const application = metadata.packages.find(pkg => resolve(pkg.manifest_path) === join(root, 'src-tauri', 'Cargo.toml'));
  const binary = application?.targets.find(target => target.kind.includes('bin'));
  if (!binary) throw new Error('Could not find the application executable in Cargo metadata');
  await seal(join(buildDirectory, `${binary.name}.exe`), application.version, { installerManifest: false });
}
for (const directory of await readdir(bundleRoot, { withFileTypes: true })) {
  if (directory.isDirectory() && ['nsis', 'msi', 'dmg', 'deb', 'rpm', 'appimage'].includes(directory.name)) {
    console.log(`Release checksums: ${await releaseChecksums(join(bundleRoot, directory.name))}`);
  }
}
