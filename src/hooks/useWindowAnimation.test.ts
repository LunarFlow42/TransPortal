import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useWindowAnimation, dismissShell, getAnimationStage } from './useWindowAnimation';
import { useAppStore } from '../store/useAppStore';
import * as audioModule from '../utils/audio';

const mockHide = vi.fn();
type ShownCallback = (event: { payload: { no_activate: boolean; generation?: number } }) => void;
let shellShownCallback: ShownCallback | null = null;
let focusCallback: ((event: { payload: boolean }) => void) | null = null;
const mockUnlisten = vi.fn();
const hideShell = vi.hoisted(() => ({ result: true }));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (command: string, args: { generation: number | null }) => {
    if (command !== 'hide_shell') throw new Error(`unexpected command ${command}`);
    if (!hideShell.result) return false;
    mockHide(args);
    return true;
  }),
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    hide: mockHide,
    onFocusChanged: (cb: (event: { payload: boolean }) => void) => {
      focusCallback = cb;
      return Promise.resolve(mockUnlisten);
    },
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: (eventName: string, cb: ShownCallback) => {
    if (eventName === 'transportal://shell-shown') {
      shellShownCallback = cb;
    }
    return Promise.resolve(mockUnlisten);
  },
}));

const lifecycle = vi.hoisted(() => ({
  effects: [] as Array<() => (() => void) | undefined>,
}));

vi.mock('react', async (importOriginal) => ({
  ...await importOriginal<typeof import('react')>(),
  useEffect: (effect: () => (() => void) | undefined) => lifecycle.effects.push(effect),
  useRef: (current: unknown) => ({ current }),
  useState: (initial: unknown) => [initial, vi.fn()],
  useCallback: (fn: Function) => fn,
}));

describe('useWindowAnimation & dismissShell', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    shellShownCallback = null;
    focusCallback = null;
    hideShell.result = true;
    lifecycle.effects.length = 0;
    useAppStore.setState({
      smoothTransitions: true,
      pinned: false,
      autoOcr: null,
    });
    vi.spyOn(audioModule, 'isAudioPlaying').mockReturnValue(false);
    vi.spyOn(audioModule, 'stopPronunciation').mockImplementation(() => {});
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('initializes listeners on mount and handles shell-shown entrance transition', () => {
    useWindowAnimation();
    // Run effects
    for (const effect of lifecycle.effects) {
      effect();
    }

    expect(shellShownCallback).toBeTypeOf('function');
    shellShownCallback!({ payload: { no_activate: false } });

    expect(getAnimationStage()).toBe('entering');

    // After 120ms entrance duration, should transition to entered
    vi.advanceTimersByTime(120);
    expect(getAnimationStage()).toBe('entered');
  });

  it('enters immediately when smoothTransitions is disabled', () => {
    useAppStore.setState({ smoothTransitions: false });
    useWindowAnimation();
    for (const effect of lifecycle.effects) {
      effect();
    }

    shellShownCallback!({ payload: { no_activate: false } });
    expect(getAnimationStage()).toBe('entered');
  });

  it('performs 100ms graceful dismiss when smoothTransitions is enabled', async () => {
    useWindowAnimation();
    for (const effect of lifecycle.effects) {
      effect();
    }
    shellShownCallback!({ payload: { no_activate: false } });
    vi.advanceTimersByTime(120);
    expect(getAnimationStage()).toBe('entered');

    const promise = dismissShell();
    expect(getAnimationStage()).toBe('exiting');
    expect(mockHide).not.toHaveBeenCalled();

    // Advance 100ms exit animation
    await vi.advanceTimersByTimeAsync(100);
    await promise;

    expect(mockHide).toHaveBeenCalledTimes(1);
    expect(getAnimationStage()).toBe('idle');
  });

  it('hides immediately when immediate: true or smoothTransitions is false', async () => {
    useAppStore.setState({ smoothTransitions: false });
    const dismissed = await dismissShell();
    expect(dismissed).toBe(true);
    expect(mockHide).toHaveBeenCalledTimes(1);
    expect(getAnimationStage()).toBe('idle');
  });

  it('protects against dismiss when pinned, autoOcr, or audio playing unless forced', async () => {
    useAppStore.setState({ pinned: true });
    const blockedPinned = await dismissShell();
    expect(blockedPinned).toBe(false);
    expect(mockHide).not.toHaveBeenCalled();

    useAppStore.setState({ pinned: false, autoOcr: { active: true } as any });
    const blockedOcr = await dismissShell();
    expect(blockedOcr).toBe(false);
    expect(mockHide).not.toHaveBeenCalled();

    useAppStore.setState({ autoOcr: null });
    vi.spyOn(audioModule, 'isAudioPlaying').mockReturnValue(true);
    const blockedAudio = await dismissShell();
    expect(blockedAudio).toBe(false);
    expect(mockHide).not.toHaveBeenCalled();

    // Force dismiss bypasses all exemptions
    const forced = dismissShell({ force: true });
    await vi.advanceTimersByTimeAsync(100);
    await forced;
    expect(mockHide).toHaveBeenCalledTimes(1);
  });

  it('cancels pending exit timer if shell is shown again during exit animation', async () => {
    useWindowAnimation();
    for (const effect of lifecycle.effects) {
      effect();
    }
    shellShownCallback!({ payload: { no_activate: false } });
    vi.advanceTimersByTime(120);

    // Start dismissing
    void dismissShell();
    expect(getAnimationStage()).toBe('exiting');

    // 50ms in, user presses hotkey again!
    vi.advanceTimersByTime(50);
    shellShownCallback!({ payload: { no_activate: false } });

    // Should immediately abort exit and switch to entering
    expect(getAnimationStage()).toBe('entering');

    // After 100ms (original exit timer duration), hide should NOT have been called!
    vi.advanceTimersByTime(100);
    expect(mockHide).not.toHaveBeenCalled();

    // After remaining 20ms (total 120ms entrance duration), should become entered
    vi.advanceTimersByTime(20);
    expect(getAnimationStage()).toBe('entered');
  });

  function mount() {
    useWindowAnimation();
    for (const effect of lifecycle.effects) {
      effect();
    }
  }

  it('passes the last shown generation so Rust can reject stale hides', async () => {
    useAppStore.setState({ smoothTransitions: false });
    mount();
    shellShownCallback!({ payload: { no_activate: true, generation: 42 } });

    await dismissShell();
    expect(mockHide).toHaveBeenCalledWith({ generation: 42 });
  });

  it('keeps the content visible when Rust rejects a hide superseded by a newer show', async () => {
    useAppStore.setState({ smoothTransitions: false });
    hideShell.result = false;

    const hidden = await dismissShell();
    expect(hidden).toBe(false);
    expect(getAnimationStage()).toBe('entered');
  });

  it('does not overwrite an entrance that arrived while the hide was in flight', async () => {
    mount();
    const promise = dismissShell();
    await vi.advanceTimersByTimeAsync(99);
    // The exit timer fires and the hide IPC starts; a show lands before it settles.
    vi.advanceTimersByTime(1);
    shellShownCallback!({ payload: { no_activate: false, generation: 2 } });
    await promise;

    expect(getAnimationStage()).toBe('entering');
  });

  it('reveals the content when the window gains focus after an unannounced show', async () => {
    useAppStore.setState({ smoothTransitions: false });
    mount();
    await dismissShell();
    expect(getAnimationStage()).toBe('idle');

    focusCallback!({ payload: true });
    expect(getAnimationStage()).toBe('entered');
  });

  it('only blocks on auto-OCR while it is actively observing', async () => {
    useAppStore.setState({ smoothTransitions: false, autoOcr: { active: false } as any });
    expect(await dismissShell()).toBe(true);
  });
});
