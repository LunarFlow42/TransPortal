import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useSmartDismiss } from './useSmartDismiss';
import { useAppStore } from '../store/useAppStore';
import * as audioModule from '../utils/audio';

const mockHide = vi.fn();
let shellShownCallback: ((event: { payload: { no_activate: boolean } }) => void) | null = null;
const mockUnlisten = vi.fn();

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    hide: mockHide,
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: (eventName: string, cb: (event: { payload: { no_activate: boolean } }) => void) => {
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
  useCallback: (fn: Function) => fn,
}));

describe('useSmartDismiss', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    shellShownCallback = null;
    lifecycle.effects.length = 0;
    useAppStore.setState({
      autoHideOnBlur: true,
      pinned: false,
      autoOcr: null,
    });
    vi.spyOn(audioModule, 'isAudioPlaying').mockReturnValue(false);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('starts 6s idle timer when shown with no_activate: true and hides when untouched', () => {
    useSmartDismiss(true);
    const effect = lifecycle.effects[0];
    effect();

    expect(shellShownCallback).toBeTypeOf('function');
    shellShownCallback!({ payload: { no_activate: true } });

    vi.advanceTimersByTime(5900);
    expect(mockHide).not.toHaveBeenCalled();

    vi.advanceTimersByTime(200);
    expect(mockHide).toHaveBeenCalledTimes(1);
  });

  it('does not start idle timer when shown with no_activate: false', () => {
    useSmartDismiss(true);
    const effect = lifecycle.effects[0];
    effect();

    shellShownCallback!({ payload: { no_activate: false } });

    vi.advanceTimersByTime(10000);
    expect(mockHide).not.toHaveBeenCalled();
  });

  it('freezes 6s timer when mouse enters, and restarts 1.5s grace timer when mouse leaves', () => {
    const { onMouseEnter, onMouseLeave } = useSmartDismiss(true);
    const effect = lifecycle.effects[0];
    effect();

    shellShownCallback!({ payload: { no_activate: true } });

    vi.advanceTimersByTime(3000);
    // User moves mouse into floating card
    onMouseEnter();

    // Past original 6s mark
    vi.advanceTimersByTime(5000);
    expect(mockHide).not.toHaveBeenCalled();

    // User moves mouse away
    onMouseLeave();

    vi.advanceTimersByTime(1400);
    expect(mockHide).not.toHaveBeenCalled();

    vi.advanceTimersByTime(200);
    expect(mockHide).toHaveBeenCalledTimes(1);
  });

  it('does not hide when pinned even if idle timer expires', () => {
    useAppStore.setState({ pinned: true });
    useSmartDismiss(true);
    const effect = lifecycle.effects[0];
    effect();

    shellShownCallback!({ payload: { no_activate: true } });

    vi.advanceTimersByTime(10000);
    expect(mockHide).not.toHaveBeenCalled();
  });

  it('does not hide when audio pronunciation is actively playing', () => {
    vi.spyOn(audioModule, 'isAudioPlaying').mockReturnValue(true);
    useSmartDismiss(true);
    const effect = lifecycle.effects[0];
    effect();

    shellShownCallback!({ payload: { no_activate: true } });

    vi.advanceTimersByTime(10000);
    expect(mockHide).not.toHaveBeenCalled();
  });

  it('cancels timers when user clicks inside (onMouseDown)', () => {
    const { onMouseDown } = useSmartDismiss(true);
    const effect = lifecycle.effects[0];
    effect();

    shellShownCallback!({ payload: { no_activate: true } });

    onMouseDown();

    vi.advanceTimersByTime(10000);
    expect(mockHide).not.toHaveBeenCalled();
  });
});
