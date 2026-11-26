import { chmod, lstat, readdir } from 'node:fs/promises';
import { join } from 'node:path';

const EXECUTABLE_ARTIFACT_PATTERN = /\.(exe|appimage)$/i;

/**
 * Compute POSIX mode bits with execute permission added wherever read
 * permission is present (plus owner execute), preserving existing read/write
 * bits.
 *
 * @param {number} mode
 * @returns {number}
 */
export function computeExecutableMode(mode) {
  const permissions = mode & 0o777;
  return permissions | ((permissions & 0o444) >> 2) | 0o100;
}

/**
 * Ensure a built binary or installer has POSIX execute bits on non-Windows
 * hosts so artifacts produced by cross-compilers (such as makensis on Linux)
 * can be executed directly over SMB/Samba shares.
 *
 * @param {string} filePath
 * @param {{ platform?: string, statFn?: typeof lstat, chmodFn?: typeof chmod }} [options]
 * @returns {Promise<boolean>}
 */
export async function ensureExecutable(filePath, {
  platform = process.platform,
  statFn = lstat,
  chmodFn = chmod,
} = {}) {
  if (platform === 'win32') return false;
  const info = await statFn(filePath);
  if (!info.isFile() || info.isSymbolicLink()) return false;
  const current = info.mode & 0o777;
  const next = computeExecutableMode(info.mode);
  if (next === current) return false;
  await chmodFn(filePath, next);
  return true;
}

/**
 * Ensure executable release artifacts (.exe, .AppImage) directly inside a
 * directory have POSIX execute permissions set.
 *
 * @param {string} directory
 * @param {{ platform?: string, statFn?: typeof lstat, chmodFn?: typeof chmod }} [options]
 * @returns {Promise<string[]>}
 */
export async function ensureDirectoryExecutables(directory, options = {}) {
  if ((options.platform ?? process.platform) === 'win32') return [];
  let entries;
  try {
    entries = await readdir(directory, { withFileTypes: true });
  } catch (error) {
    if (error && typeof error === 'object' && 'code' in error && error.code === 'ENOENT') {
      return [];
    }
    throw error;
  }
  const updated = [];
  for (const entry of entries) {
    if (!entry.isFile() || !EXECUTABLE_ARTIFACT_PATTERN.test(entry.name)) continue;
    const fullPath = join(directory, entry.name);
    if (await ensureExecutable(fullPath, options)) {
      updated.push(fullPath);
    }
  }
  return updated;
}
