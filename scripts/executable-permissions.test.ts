import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { chmod, mkdir, mkdtemp, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  computeExecutableMode,
  ensureDirectoryExecutables,
  ensureExecutable,
} from './executable-permissions.mjs';

let fixture: string;

beforeEach(async () => {
  fixture = await mkdtemp(join(tmpdir(), 'transportal-exec-perm-'));
});

afterEach(async () => {
  await rm(fixture, { recursive: true, force: true });
});

describe('computeExecutableMode', () => {
  it('adds execute bits wherever read permission is granted', () => {
    expect(computeExecutableMode(0o100664)).toBe(0o775);
    expect(computeExecutableMode(0o100644)).toBe(0o755);
    expect(computeExecutableMode(0o100640)).toBe(0o750);
    expect(computeExecutableMode(0o100600)).toBe(0o700);
    expect(computeExecutableMode(0o100755)).toBe(0o755);
  });
});

describe('ensureExecutable', () => {
  it('is a no-op on win32', async () => {
    const chmodFn = vi.fn();
    const statFn = vi.fn();
    const changed = await ensureExecutable(join(fixture, 'setup.exe'), {
      platform: 'win32',
      statFn,
      chmodFn,
    });
    expect(changed).toBe(false);
    expect(statFn).not.toHaveBeenCalled();
    expect(chmodFn).not.toHaveBeenCalled();
  });

  it('sets execute permissions on regular files on POSIX hosts', async () => {
    const filePath = join(fixture, 'TransPortal_0.1.3_x64-setup.exe');
    await writeFile(filePath, 'stub');
    const chmodFn = vi.fn().mockResolvedValue(undefined);
    const statFn = vi.fn().mockResolvedValue({
      isFile: () => true,
      isSymbolicLink: () => false,
      mode: 0o100664,
    });

    const changed = await ensureExecutable(filePath, {
      platform: 'linux',
      statFn,
      chmodFn,
    });
    expect(changed).toBe(true);
    expect(chmodFn).toHaveBeenCalledWith(filePath, 0o775);
  });

  it('skips chmod when execute bits are already present or entry is not a regular file', async () => {
    const chmodFn = vi.fn();
    const executableChanged = await ensureExecutable('/tmp/ready.exe', {
      platform: 'linux',
      statFn: vi.fn().mockResolvedValue({
        isFile: () => true,
        isSymbolicLink: () => false,
        mode: 0o100755,
      }),
      chmodFn,
    });
    const symlinkChanged = await ensureExecutable('/tmp/link.exe', {
      platform: 'linux',
      statFn: vi.fn().mockResolvedValue({
        isFile: () => false,
        isSymbolicLink: () => true,
        mode: 0o120777,
      }),
      chmodFn,
    });

    expect(executableChanged).toBe(false);
    expect(symlinkChanged).toBe(false);
    expect(chmodFn).not.toHaveBeenCalled();
  });

  if (process.platform !== 'win32') {
    it('updates file mode on disk on POSIX platforms', async () => {
      const filePath = join(fixture, 'real-installer.exe');
      await writeFile(filePath, 'stub');
      await chmod(filePath, 0o644);
      const changed = await ensureExecutable(filePath);
      expect(changed).toBe(true);
      const info = await stat(filePath);
      expect(info.mode & 0o777).toBe(0o755);
    });
  }
});

describe('ensureDirectoryExecutables', () => {
  it('updates only .exe and .AppImage artifacts and ignores non-executables', async () => {
    const setupExe = join(fixture, 'TransPortal_0.1.3_x64-setup.exe');
    const appImage = join(fixture, 'TransPortal_0.1.3_amd64.AppImage');
    const checksum = join(fixture, 'TransPortal_0.1.3_x64-setup.exe.sha256');
    const msi = join(fixture, 'TransPortal_0.1.3_x64.msi');
    const nestedDir = join(fixture, 'nested.exe');
    await writeFile(setupExe, 'exe');
    await writeFile(appImage, 'appimage');
    await writeFile(checksum, 'sha');
    await writeFile(msi, 'msi');
    await mkdir(nestedDir);

    const chmodFn = vi.fn().mockResolvedValue(undefined);
    const statFn = vi.fn().mockResolvedValue({
      isFile: () => true,
      isSymbolicLink: () => false,
      mode: 0o100664,
    });

    const updated = await ensureDirectoryExecutables(fixture, {
      platform: 'linux',
      statFn,
      chmodFn,
    });

    expect(updated.sort()).toEqual([appImage, setupExe].sort());
    expect(chmodFn).toHaveBeenCalledTimes(2);
    expect(chmodFn).toHaveBeenCalledWith(setupExe, 0o775);
    expect(chmodFn).toHaveBeenCalledWith(appImage, 0o775);
  });

  it('returns an empty list when directory is missing or host is win32', async () => {
    expect(await ensureDirectoryExecutables(join(fixture, 'missing'), { platform: 'linux' })).toEqual([]);
    expect(await ensureDirectoryExecutables(fixture, { platform: 'win32' })).toEqual([]);
  });
});
