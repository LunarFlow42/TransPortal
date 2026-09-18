import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { useBootstrap } from './useBootstrap';
import { useAppStore } from '../store/useAppStore';
import { loadSettings, saveSettings } from '../store/persistence';
import type { OcrConfig, PersistedSettings } from '../types';

const lifecycle = vi.hoisted(() => ({
  effects: [] as Array<() => (() => void) | undefined>,
  initialized: vi.fn(),
}));

// Run the effect lifecycle without a DOM; the real store, async native calls,
// persistence subscription and cancellation behavior remain under test.
vi.mock('react', async (importOriginal) => ({
  ...await importOriginal<typeof import('react')>(),
  useEffect: (effect: () => (() => void) | undefined) => lifecycle.effects.push(effect),
  useRef: (current: unknown) => ({ current }),
  useState: (initial: unknown) => [initial, lifecycle.initialized],
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), Channel: class {} }));
vi.mock('@tauri-apps/api/event', () => ({ emit: vi.fn(), listen: vi.fn() }));
vi.mock('../store/persistence', () => ({ loadSettings: vi.fn(), saveSettings: vi.fn() }));

const cleanups: Array<() => void> = [];
const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => { resolve = accept; });
  return { promise, resolve };
};
const mount = () => {
  useBootstrap(true);
  const effect = lifecycle.effects[lifecycle.effects.length - 1];
  const cleanup = effect()!;
  cleanups.push(cleanup);
  return { effect, cleanup };
};

beforeEach(() => {
  vi.clearAllMocks();
  lifecycle.effects.length = 0;
  useAppStore.setState({ hotkey: 'Alt+Q', doubleCopyEnabled: true, ocrEngine: 'windows', ocrLanguage: 'auto', settingsError: null });
  vi.mocked(loadSettings).mockReset().mockResolvedValue({});
  vi.mocked(saveSettings).mockReset().mockResolvedValue();
  vi.mocked(emit).mockReset().mockResolvedValue();
  vi.mocked(invoke).mockReset().mockImplementation(async (command, args) => {
    if (command === 'get_ocr_config') return { engine: 'windows', language: null };
    if (command === 'set_ocr_config') return (args as { config: OcrConfig }).config;
    return undefined;
  });
});

afterEach(() => {
  cleanups.splice(0).forEach((cleanup) => cleanup());
});

describe('OCR settings startup', () => {
  it('waits for native restoration before enabling edits, then saves subsequent engine changes', async () => {
    const restore = deferred<OcrConfig>();
    vi.mocked(loadSettings).mockResolvedValue({ ocrEngine: 'onnx', ocrLanguage: 'auto' });
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === 'get_ocr_config') return { engine: 'windows', language: null };
      if (command === 'set_ocr_config') {
        const config = (args as { config: OcrConfig }).config;
        return config.engine === 'onnx' ? restore.promise : config;
      }
      return undefined;
    });
    mount();
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('set_ocr_config', {
      config: { engine: 'onnx', language: null },
    }));
    expect(lifecycle.initialized).not.toHaveBeenCalledWith(true);
    expect(invoke).not.toHaveBeenCalledWith('finish_startup_checks');
    expect(saveSettings).not.toHaveBeenCalled();

    restore.resolve({ engine: 'onnx', language: null });
    await vi.waitFor(() => expect(lifecycle.initialized).toHaveBeenCalledWith(true));
    expect(invoke).toHaveBeenCalledWith('finish_startup_checks');
    expect(emit).toHaveBeenCalledWith('transportal://settings-changed', {
      hotkey: 'Alt+Q', doubleCopyEnabled: true, ocrEngine: 'onnx', ocrLanguage: 'auto',
    });
    expect(saveSettings).not.toHaveBeenCalled();
    await useAppStore.getState().setOcrEngine('windows');
    expect(saveSettings).toHaveBeenCalledExactlyOnceWith({ ocrEngine: 'windows' });
  });

  it('a disposed startup cannot unlock the persistence writer for its replacement', async () => {
    const first = deferred<Partial<PersistedSettings>>();
    const second = deferred<Partial<PersistedSettings>>();
    vi.mocked(loadSettings).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const { effect, cleanup } = mount();
    cleanup();
    cleanups.push(effect()!);
    first.resolve({ ocrEngine: 'onnx' });
    await first.promise;
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(lifecycle.initialized).not.toHaveBeenCalledWith(true);
    expect(invoke).not.toHaveBeenCalled();

    second.resolve({});
    await vi.waitFor(() => expect(lifecycle.initialized).toHaveBeenCalledWith(true));
    expect(lifecycle.initialized.mock.calls.filter(([ready]) => ready)).toHaveLength(1);
  });

  it('reports startup and save failures while allowing a later engine change', async () => {
    vi.mocked(loadSettings).mockRejectedValueOnce(new Error('settings unavailable'));
    mount();
    await vi.waitFor(() => expect(lifecycle.initialized).toHaveBeenCalledWith(true));
    expect(useAppStore.getState().settingsError).toContain('settings unavailable');

    vi.mocked(saveSettings).mockRejectedValueOnce(new Error('disk unavailable'));
    await useAppStore.getState().setOcrEngine('onnx');
    await vi.waitFor(() => expect(useAppStore.getState().settingsError).toContain('disk unavailable'));
    expect(useAppStore.getState().ocrEngine).toBe('onnx');
  });

  it('restores a disabled double-copy shortcut before startup checks and saves re-enabling it', async () => {
    vi.mocked(loadSettings).mockResolvedValue({ doubleCopyEnabled: false });
    mount();
    await vi.waitFor(() => expect(lifecycle.initialized).toHaveBeenCalledWith(true));
    expect(invoke).toHaveBeenCalledWith('set_double_copy_enabled', { enabled: false });
    const commands = vi.mocked(invoke).mock.calls.map(([command]) => command);
    expect(commands.indexOf('set_double_copy_enabled')).toBeLessThan(commands.indexOf('finish_startup_checks'));
    expect(useAppStore.getState().doubleCopyEnabled).toBe(false);
    expect(saveSettings).not.toHaveBeenCalled();
    await useAppStore.getState().setDoubleCopyEnabled(true);
    expect(saveSettings).toHaveBeenCalledExactlyOnceWith({ doubleCopyEnabled: true });
  });
});
