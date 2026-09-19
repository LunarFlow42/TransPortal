import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { fetchMemoryUsage, trimMemoryNow, useMemoryManager } from './useMemoryManager';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

let focusCallback: ((event: { payload: boolean }) => void) | null = null;
const mockUnlisten = vi.fn();

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    onFocusChanged: (cb: (event: { payload: boolean }) => void) => {
      focusCallback = cb;
      return Promise.resolve(mockUnlisten);
    },
  }),
}));

const lifecycle = vi.hoisted(() => ({
  effects: [] as Array<() => (() => void) | undefined>,
}));

vi.mock('react', async (importOriginal) => ({
  ...await importOriginal<typeof import('react')>(),
  useEffect: (effect: () => (() => void) | undefined) => lifecycle.effects.push(effect),
  useRef: (current: unknown) => ({ current }),
}));

describe('useMemoryManager', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    focusCallback = null;
    lifecycle.effects.length = 0;
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('fetchMemoryUsage calls get_memory_usage command', async () => {
    const mockReport = {
      total_working_set_bytes: 50 * 1024 * 1024,
      total_private_bytes: 40 * 1024 * 1024,
      main_working_set_bytes: 20 * 1024 * 1024,
      process_count: 3,
    };
    vi.mocked(invoke).mockResolvedValueOnce(mockReport);
    const result = await fetchMemoryUsage();
    expect(invoke).toHaveBeenCalledWith('get_memory_usage');
    expect(result).toEqual(mockReport);
  });

  it('trimMemoryNow calls trim_memory command', async () => {
    const mockTrim = {
      before_bytes: 100 * 1024 * 1024,
      after_bytes: 35 * 1024 * 1024,
      trimmed_bytes: 65 * 1024 * 1024,
      process_count: 3,
    };
    vi.mocked(invoke).mockResolvedValueOnce(mockTrim);
    const result = await trimMemoryNow();
    expect(invoke).toHaveBeenCalledWith('trim_memory');
    expect(result).toEqual(mockTrim);
  });

  it('schedules idle trim after 3 seconds on blur', async () => {
    useMemoryManager(true);
    const effect = lifecycle.effects[lifecycle.effects.length - 1];
    effect();

    expect(focusCallback).toBeTypeOf('function');

    // Window loses focus
    focusCallback!({ payload: false });

    // After 2 seconds, not trimmed yet
    vi.advanceTimersByTime(2000);
    expect(invoke).not.toHaveBeenCalledWith('trim_memory');

    // After 3 seconds, trimmed!
    vi.advanceTimersByTime(1100);
    expect(invoke).toHaveBeenCalledWith('trim_memory');
  });

  it('cancels scheduled trim if window regains focus before 3 seconds', async () => {
    useMemoryManager(true);
    const effect = lifecycle.effects[lifecycle.effects.length - 1];
    effect();

    // Window loses focus
    focusCallback!({ payload: false });

    // After 1.5 seconds, window regains focus
    vi.advanceTimersByTime(1500);
    focusCallback!({ payload: true });

    // Advance past 3 seconds
    vi.advanceTimersByTime(3000);
    expect(invoke).not.toHaveBeenCalledWith('trim_memory');
  });
});
