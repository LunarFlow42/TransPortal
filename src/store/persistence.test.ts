import { beforeEach, describe, expect, it, vi } from 'vitest';
import { load } from '@tauri-apps/plugin-store';

vi.mock('@tauri-apps/plugin-store', () => ({ load: vi.fn() }));

beforeEach(() => {
  vi.resetModules();
  vi.mocked(load).mockReset();
});

const settingsFile = (initial: Record<string, unknown> = {}) => {
  const data = new Map(Object.entries(initial));
  const store = {
    get: vi.fn(async (key: string) => data.get(key)),
    set: vi.fn(async (key: string, value: unknown) => { data.set(key, value); }),
    save: vi.fn(async () => {}),
  };
  vi.mocked(load).mockResolvedValue(store as unknown as Awaited<ReturnType<typeof load>>);
  return { data, store };
};

describe('persistent OCR configuration', () => {
  it('loads the saved engine and language and preserves unrelated settings when switching', async () => {
    const { data, store } = settingsFile({ ocrEngine: 'onnx', ocrLanguage: 'auto', theme: 'dark' });
    const { loadSettings, saveSettings } = await import('./persistence');
    expect(await loadSettings()).toMatchObject({ ocrEngine: 'onnx', ocrLanguage: 'auto' });
    await saveSettings({ ocrEngine: 'windows', ocrLanguage: 'en-US', theme: undefined });
    expect(Object.fromEntries(data)).toEqual({ ocrEngine: 'windows', ocrLanguage: 'en-US', theme: 'dark' });
    expect(store.save).toHaveBeenCalledOnce();
    expect(load).toHaveBeenCalledOnce();
  });

  it('retries opening the settings file after a transient failure instead of caching rejection', async () => {
    settingsFile();
    vi.mocked(load).mockRejectedValueOnce(new Error('temporarily unavailable'));
    const { loadSettings } = await import('./persistence');
    await expect(loadSettings()).rejects.toThrow('temporarily unavailable');
    await expect(loadSettings()).resolves.toMatchObject({ ocrEngine: undefined, ocrLanguage: undefined });
    expect(load).toHaveBeenCalledTimes(2);
  });

  it('loads and saves smoothTransitions correctly', async () => {
    const { data, store } = settingsFile({ smoothTransitions: true });
    const { loadSettings, saveSettings } = await import('./persistence');
    expect(await loadSettings()).toMatchObject({ smoothTransitions: true });
    await saveSettings({ smoothTransitions: false });
    expect(Object.fromEntries(data)).toEqual({ smoothTransitions: false });
    expect(store.save).toHaveBeenCalledOnce();
  });
});
