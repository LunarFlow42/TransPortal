import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useWindowAnimation, dismissShell, getAnimationStage } from './useWindowAnimation';
import { useAppStore } from '../store/useAppStore';
import * as audioModule from '../utils/audio';

const mockHide = vi.fn();
let shellShownCallback: (() => void) | null = null;
const mockUnlisten = vi.fn();

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    hide: mockHide,
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: (eventName: string, cb: () => void) => {
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
    shellShownCallback!();

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

    shellShownCallback!();
    expect(getAnimationStage()).toBe('entered');
  });

  it('performs 100ms graceful dismiss when smoothTransitions is enabled', async () => {
    useWindowAnimation();
    for (const effect of lifecycle.effects) {
      effect();
    }
    shellShownCallback!();
    vi.advanceTimersByTime(120);
    expect(getAnimationStage()).toBe('entered');

    const promise = dismissShell();
    expect(getAnimationStage()).toBe('exiting');
    expect(mockHide).not.toHaveBeenCalled();

    // Advance 100ms exit animation
    vi.advanceTimersByTime(100);
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
    vi.advanceTimersByTime(100);
    await forced;
    expect(mockHide).toHaveBeenCalledTimes(1);
  });

  it('cancels pending exit timer if shell is shown again during exit animation', async () => {
    useWindowAnimation();
    for (const effect of lifecycle.effects) {
      effect();
    }
    shellShownCallback!();
    vi.advanceTimersByTime(120);

    // Start dismissing
    void dismissShell();
    expect(getAnimationStage()).toBe('exiting');

    // 50ms in, user presses hotkey again!
    vi.advanceTimersByTime(50);
    shellShownCallback!();

    // Should immediately abort exit and switch to entering
    expect(getAnimationStage()).toBe('entering');

    // After 100ms (original exit timer duration), hide should NOT have been called!
    vi.advanceTimersByTime(100);
    expect(mockHide).not.toHaveBeenCalled();

    // After remaining 20ms (total 120ms entrance duration), should become entered
    vi.advanceTimersByTime(20);
    expect(getAnimationStage()).toBe('entered');
  });
});
