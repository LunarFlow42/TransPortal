import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { useOcrStatus } from './useOcr';
import type { OcrStatus } from '../types';

const lifecycle = vi.hoisted(() => ({
  cursor: 0, values: [] as unknown[],
  effects: [] as Array<() => void | (() => void)>,
  cleanups: [] as Array<() => void>,
  writes: vi.fn(),
}));
vi.mock('react', async (original) => ({
  ...await original<typeof import('react')>(),
  useState: (initial: unknown) => {
    const index = lifecycle.cursor++;
    if (!(index in lifecycle.values)) lifecycle.values[index] = initial;
    return [lifecycle.values[index], (value: unknown) => {
      lifecycle.writes();
      lifecycle.values[index] = typeof value === 'function' ? value(lifecycle.values[index]) : value;
    }];
  },
  useRef: (current: unknown) => {
    const index = lifecycle.cursor++;
    if (!(index in lifecycle.values)) lifecycle.values[index] = { current };
    return lifecycle.values[index];
  },
  useCallback: (callback: unknown) => {
    const index = lifecycle.cursor++;
    if (!(index in lifecycle.values)) lifecycle.values[index] = callback;
    return lifecycle.values[index];
  },
  useEffect: (effect: () => void | (() => void)) => {
    const index = lifecycle.cursor++;
    if (index in lifecycle.values) return;
    lifecycle.values[index] = true;
    lifecycle.effects.push(effect);
  },
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => vi.fn()) }));
vi.mock('../store/useAppStore', () => ({
  useAppStore: (selector: (state: { ocrEngine: string; ocrLanguage: string }) => unknown) => selector({ ocrEngine: 'windows', ocrLanguage: 'auto' }),
}));
const status = { supported: true, engine: { id: 'windows', available: true } } as OcrStatus;
function render() {
  lifecycle.cursor = 0;
  const result = useOcrStatus();
  lifecycle.effects.splice(0).forEach((effect) => {
    const cleanup = effect();
    if (cleanup) lifecycle.cleanups.push(cleanup);
  });
  return result;
}
function unmount() { lifecycle.cleanups.splice(0).forEach((cleanup) => cleanup()); }
const flush = async () => { for (let i = 0; i < 6; i++) await Promise.resolve(); };
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((accept, fail) => { resolve = accept; reject = fail; });
  return { promise, resolve, reject };
}
beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  lifecycle.values.length = 0;
  lifecycle.effects.length = 0;
  lifecycle.cleanups.length = 0;
  vi.mocked(invoke).mockReset().mockResolvedValue(status);
});
afterEach(() => { unmount(); vi.useRealTimers(); });

describe('OCR status initialization recovery', () => {
  it('recovers automatically when the first request precedes native service initialization', async () => {
    vi.mocked(invoke).mockRejectedValueOnce('OCR 服务正在初始化...');
    render();
    await flush();
    expect(render().error).toContain('正在初始化');
    expect(render().status).toBeNull();
    await vi.advanceTimersByTimeAsync(499);
    expect(invoke).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(render().status).toEqual(status);
    expect(render().error).toBeNull();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it('backs off consecutive failures and caps retry frequency at one request per ten seconds', async () => {
    vi.mocked(invoke).mockRejectedValue('not ready');
    render(); await flush();
    for (const delay of [500, 1000, 2000, 4000, 8000, 10000, 10000]) {
      const before = vi.mocked(invoke).mock.calls.length;
      await vi.advanceTimersByTimeAsync(delay - 1);
      expect(invoke).toHaveBeenCalledTimes(before);
      await vi.advanceTimersByTimeAsync(1);
      expect(invoke).toHaveBeenCalledTimes(before + 1);
    }
  });

  it('successful manual refresh cancels a scheduled retry', async () => {
    vi.mocked(invoke).mockRejectedValueOnce('not ready');
    render(); await flush();
    await render().refresh();
    expect(render().status).toEqual(status);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it('clears retry timers on unmount', async () => {
    vi.mocked(invoke).mockRejectedValue('not ready');
    render(); await flush();
    unmount();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it.each(['resolve', 'reject'] as const)('ignores an in-flight %s after unmount without scheduling retries', async (outcome) => {
    const pending = deferred<OcrStatus>();
    vi.mocked(invoke).mockReturnValueOnce(pending.promise);
    render(); unmount(); lifecycle.writes.mockClear();
    if (outcome === 'resolve') pending.resolve(status);
    else pending.reject('not ready');
    await flush();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(lifecycle.writes).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it.each(['resolve', 'reject'] as const)('does not let an old request %s overwrite a newer successful refresh', async (outcome) => {
    const old = deferred<OcrStatus>();
    vi.mocked(invoke).mockReturnValueOnce(old.promise);
    render();
    await render().refresh();
    if (outcome === 'resolve') old.resolve({ ...status, supported: false });
    else old.reject('stale error');
    await flush();
    expect(render().status).toEqual(status);
    expect(render().error).toBeNull();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(invoke).toHaveBeenCalledTimes(2);
  });
});
