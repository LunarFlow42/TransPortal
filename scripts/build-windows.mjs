#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function run(cmd, args, opts = {}) {
  const display = `${cmd} ${args.join(' ')}`;
  console.log(`\n🚀 Executing: ${display}`);
  const res = spawnSync(cmd, args, { cwd: root, stdio: 'inherit', ...opts });
  if (res.status !== 0) {
    console.error(`❌ Command failed with exit code ${res.status}: ${display}`);
    process.exit(res.status || 1);
  }
}

function checkTool(tool, installHint) {
  const res = spawnSync('which', [tool], { encoding: 'utf8' });
  if (res.status !== 0 || !res.stdout.trim()) {
    console.error(`\n❌ Missing required tool: "${tool}"`);
    console.error(`💡 Install hint: ${installHint}\n`);
    process.exit(1);
  }
}

console.log('====================================================');
console.log('  TransPortal Windows (x86_64) Cross-Compile Build  ');
console.log('====================================================');

// 1. 检查宿主系统必要工具链
checkTool('makensis', 'sudo apt install -y nsis (Debian/Ubuntu)');
checkTool('clang', 'sudo apt install -y clang (Debian/Ubuntu)');
checkTool('lld', 'sudo apt install -y lld (Debian/Ubuntu)');
checkTool('cargo-xwin', 'cargo install --locked cargo-xwin 或 uv tool install cargo-xwin');

// 2. 检查 Rust target
const targetCheck = spawnSync('rustup', ['target', 'list', '--installed'], { encoding: 'utf8' });
if (!targetCheck.stdout.includes('x86_64-pc-windows-msvc')) {
  console.log('📦 Installing rust target: x86_64-pc-windows-msvc...');
  run('rustup', ['target', 'add', 'x86_64-pc-windows-msvc']);
}

// 3. 准备 Windows 平台的 ONNX 离线资源包
console.log('\n📦 Staging Windows ONNX runtime & model resources...');
run('node', ['scripts/ocr/prepare.mjs', '--platform', 'windows-x86_64']);

// 4. 构建前端静态资源
console.log('\n🎨 Building frontend static bundle with Vite...');
run('pnpm', ['build']);

// 5. 运行完整的 Tauri 打包 (注入离线模式，避免 localhost 拒绝连接)
console.log('\n🔨 Packaging Windows NSIS installer via Tauri & cargo-xwin...');
run('pnpm', [
  'tauri',
  'build',
  '--runner',
  'cargo-xwin',
  '--target',
  'x86_64-pc-windows-msvc',
  '--config',
  '{"build":{"devUrl":null,"beforeBuildCommand":null}}',
]);

// 6. 输出结果与计算 SHA-256
const tauriConf = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8'));
const version = tauriConf.version;
const nsisDir = join(root, 'src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis');
const installerPath = join(nsisDir, `TransPortal_${version}_x64-setup.exe`);

if (existsSync(installerPath)) {
  const buffer = readFileSync(installerPath);
  const hash = createHash('sha256').update(buffer).digest('hex');
  const sha256Path = `${installerPath}.sha256`;
  writeFileSync(sha256Path, `${hash}  TransPortal_${version}_x64-setup.exe\n`);

  console.log('\n====================================================');
  console.log('✅ Build Success! Windows NSIS Installer Generated:');
  console.log(`📁 File:   ${installerPath}`);
  console.log(`⚖️ Size:   ${(buffer.length / (1024 * 1024)).toFixed(2)} MB`);
  console.log(`🔑 SHA256: ${hash}`);
  console.log(`📄 Hash:   ${sha256Path}`);
  console.log('====================================================\n');
} else {
  console.warn(`\n⚠️ Build completed but installer not found at: ${installerPath}`);
}
