import { beforeEach, describe, expect, it, vi } from 'vitest';
import { load } from '@tauri-apps/plugin-store';

const mockInvoke = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
  Channel: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-store', () => ({ load: vi.fn() }));

const mockSettingsFile = (initial: Record<string, unknown> = {}) => {
  const data = new Map(Object.entries(initial));
  const store = {
    get: vi.fn(async (key: string) => data.get(key)),
    set: vi.fn(async (key: string, value: unknown) => {
      data.set(key, value);
    }),
    save: vi.fn(async () => {}),
  };
  vi.mocked(load).mockResolvedValue(store as unknown as Awaited<ReturnType<typeof load>>);
  return { data, store };
};

describe('useAppStore resetToDefaults', () => {
  beforeEach(() => {
    vi.resetModules();
    mockInvoke.mockReset().mockResolvedValue(undefined);
    mockSettingsFile();
  });

  it('resets state to defaults and notifies native core while preserving state integrity', async () => {
    const { useAppStore } = await import('./useAppStore');

    // Mutate state away from defaults
    useAppStore.setState({
      fontSize: 20,
      overlayOpacity: 0.35,
      hotkey: 'Ctrl+Alt+F',
      theme: 'dark',
      provider: 'openai',
    });

    expect(useAppStore.getState().fontSize).toBe(20);
    expect(useAppStore.getState().provider).toBe('openai');

    // Trigger reset
    await useAppStore.getState().resetToDefaults();

    // Verify state was reset to defaults
    const state = useAppStore.getState();
    expect(state.fontSize).toBe(14);
    expect(state.overlayOpacity).toBe(0.72);
    expect(state.theme).toBe('system');
    expect(state.provider).toBe('bing_free');
    expect(state.hotkey).toBe('Alt+Q');

    // Verify invoke was called for shortcuts and native settings
    expect(mockInvoke).toHaveBeenCalledWith('set_shortcut', { action: 'selection', accelerator: 'Alt+Q' });
    expect(mockInvoke).toHaveBeenCalledWith('set_double_copy_enabled', { enabled: true });
  });
});
