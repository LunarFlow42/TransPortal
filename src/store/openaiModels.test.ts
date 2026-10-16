import { beforeEach, describe, expect, it, vi } from 'vitest';

const mockInvoke = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
  Channel: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-store', () => ({ load: vi.fn() }));

describe('useAppStore openaiModels', () => {
  beforeEach(() => {
    vi.resetModules();
    mockInvoke.mockReset().mockResolvedValue(undefined);
  });

  it('updates openaiModels list and preserves model selection', async () => {
    const { useAppStore } = await import('./useAppStore');

    expect(useAppStore.getState().openaiModels).toEqual([]);

    const testModels = ['deepseek-chat', 'gpt-4o', 'gpt-4o-mini', 'qwen2.5:7b'];
    useAppStore.getState().setOpenaiModels(testModels);

    expect(useAppStore.getState().openaiModels).toEqual(testModels);

    // Update config model
    useAppStore.getState().setOpenaiConfig({ model: 'deepseek-chat' });
    expect(useAppStore.getState().openaiConfig.model).toBe('deepseek-chat');
  });
});
