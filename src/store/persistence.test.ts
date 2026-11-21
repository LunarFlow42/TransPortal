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

describe('persistent automatic update preference', () => {
  it('leaves a missing preference undefined for the application default', async () => {
    settingsFile();
    const { loadSettings } = await import('./persistence');
    expect((await loadSettings()).autoCheckUpdates).toBeUndefined();
  });

  it.each([false, true])('saves and restores the preference %s', async (enabled) => {
    const { data, store } = settingsFile({ theme: 'dark' });
    const { loadSettings, saveSettings } = await import('./persistence');
    await saveSettings({ autoCheckUpdates: enabled });
    expect(data.get('autoCheckUpdates')).toBe(enabled);
    expect(data.get('theme')).toBe('dark');
    expect((await loadSettings()).autoCheckUpdates).toBe(enabled);
    expect(store.save).toHaveBeenCalledOnce();
  });
});

describe('persistent OCR configuration', () => {
  it('round-trips cleared shortcuts without replacing them with defaults', async () => {
    const { store } = settingsFile({ ocrTextHotkey: 'Alt+Shift+C' });
    const { loadSettings, saveSettings } = await import('./persistence');
    const cleared = {
      hotkey: '', ocrCardHotkey: '', ocrPictureHotkey: '', ocrTextHotkey: '',
      ocrSearchHotkey: '', replaceSelectionHotkey: '', silentOcrHotkey: '', inputTranslateHotkey: '',
    };
    await saveSettings(cleared);
    expect(await loadSettings()).toMatchObject(cleared);
    expect(store.save).toHaveBeenCalledOnce();
  });

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
