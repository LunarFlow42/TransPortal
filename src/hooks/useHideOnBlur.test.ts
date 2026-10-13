import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useHideOnBlur } from './useHideOnBlur';
import { useAppStore } from '../store/useAppStore';
import { resumeAutoHide, suppressAutoHide } from '../utils/autoHide';
import * as audioModule from '../utils/audio';

const mockHide = vi.fn();
let focusCallback: ((event: { payload: boolean }) => void) | null = null;
const mockUnlisten = vi.fn();
let outsideClickCallback: (() => void) | null = null;
let sizeMoveCallback: ((event: { payload: boolean }) => void) | null = null;

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (command: string) => {
    if (command !== 'hide_shell') throw new Error(`unexpected command ${command}`);
    mockHide();
    return true;
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: (eventName: string, cb: (event: { payload: boolean }) => void) => {
    if (eventName === 'transportal://outside-click') outsideClickCallback = cb as () => void;
    if (eventName === 'transportal://shell-size-move') sizeMoveCallback = cb;
    return Promise.resolve(mockUnlisten);
  },
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    hide: mockHide,
    setIgnoreCursorEvents: vi.fn().mockResolvedValue(undefined),
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
}));

describe('useHideOnBlur', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    focusCallback = null;
    outsideClickCallback = null;
    sizeMoveCallback = null;
    resumeAutoHide(0);
    lifecycle.effects.length = 0;
    useAppStore.setState({
      pinned: false,
      autoOcr: null,
      smoothTransitions: true,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('hides after 100ms dismiss animation when window loses focus', async () => {
    useHideOnBlur(true);
    const effect = lifecycle.effects[0];
    effect();

    expect(focusCallback).toBeTypeOf('function');
    focusCallback!({ payload: false });

    expect(mockHide).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(100);
    expect(mockHide).toHaveBeenCalledTimes(1);
  });

  it('hides immediately when smoothTransitions is disabled', async () => {
    useAppStore.setState({ smoothTransitions: false });
    useHideOnBlur(true);
    const effect = lifecycle.effects[0];
    effect();

    focusCallback!({ payload: false });
    await vi.advanceTimersByTimeAsync(0);
    expect(mockHide).toHaveBeenCalledTimes(1);
  });

  it('does not hide when focus is gained', () => {
    useHideOnBlur(true);
    const effect = lifecycle.effects[0];
    effect();

    focusCallback!({ payload: true });

    expect(mockHide).not.toHaveBeenCalled();
  });

  it('does not hide when pinned', () => {
    useAppStore.setState({ pinned: true });

    useHideOnBlur(true);
    const effect = lifecycle.effects[0];
    effect();

    focusCallback!({ payload: false });

    expect(mockHide).not.toHaveBeenCalled();
  });

  it('does not hide when autoOcr is actively observing', () => {
    useAppStore.setState({
      autoOcr: {
        id: 1,
        revision: 1,
        active: true,
        selecting: false,
        paused: false,
        stage: 'watching',
        interval_ms: 1000,
        checks: 1,
        target_lang: 'zh-CN',
        region: { x: 0, y: 0, width: 100, height: 100 },
        error: null,
        frame: null,
      },
    });

    useHideOnBlur(true);
    const effect = lifecycle.effects[0];
    effect();

    focusCallback!({ payload: false });

    expect(mockHide).not.toHaveBeenCalled();
  });

  it('does not hide when auto-hide is suppressed', () => {
    suppressAutoHide(5000);

    useHideOnBlur(true);
    const effect = lifecycle.effects[0];
    effect();

    focusCallback!({ payload: false });

    expect(mockHide).not.toHaveBeenCalled();
  });

  it('does not register listener when active is false', () => {
    useHideOnBlur(false);
    const effect = lifecycle.effects[0];
    effect();

    expect(focusCallback).toBeNull();
  });

  it('hides on a native outside click even though the overlay never had focus', async () => {
    useHideOnBlur(true);
    lifecycle.effects[0]();

    expect(outsideClickCallback).toBeTypeOf('function');
    outsideClickCallback!();
    await vi.advanceTimersByTimeAsync(100);
    expect(mockHide).toHaveBeenCalledTimes(1);
  });

  it('applies the same guards to outside clicks', async () => {
    useAppStore.setState({ pinned: true });
    useHideOnBlur(true);
    lifecycle.effects[0]();

    outsideClickCallback!();
    await vi.advanceTimersByTimeAsync(200);
    expect(mockHide).not.toHaveBeenCalled();
  });

  it('does not let playing pronunciation keep the overlay open', async () => {
    vi.spyOn(audioModule, 'isAudioPlaying').mockReturnValue(true);
    const stop = vi.spyOn(audioModule, 'stopPronunciation').mockImplementation(() => {});
    useHideOnBlur(true);
    lifecycle.effects[0]();

    focusCallback!({ payload: false });
    await vi.advanceTimersByTimeAsync(100);
    expect(mockHide).toHaveBeenCalledTimes(1);
    expect(stop).toHaveBeenCalled();
  });

  it('keeps the overlay while the user moves or resizes it, even if the blur came first', async () => {
    useHideOnBlur(true);
    lifecycle.effects[0]();

    // Grabbing an edge blurs the webview; the size/move report may lag behind.
    focusCallback!({ payload: false });
    await vi.advanceTimersByTimeAsync(40);
    sizeMoveCallback!({ payload: true });
    await vi.advanceTimersByTimeAsync(200);
    expect(mockHide).not.toHaveBeenCalled();

    // Blur during the interaction is ignored too.
    focusCallback!({ payload: false });
    await vi.advanceTimersByTimeAsync(200);
    expect(mockHide).not.toHaveBeenCalled();
  });

  it('resumes auto-hide shortly after the move or resize ends', async () => {
    useHideOnBlur(true);
    lifecycle.effects[0]();

    sizeMoveCallback!({ payload: true });
    sizeMoveCallback!({ payload: false });
    await vi.advanceTimersByTimeAsync(400);

    outsideClickCallback!();
    await vi.advanceTimersByTimeAsync(100);
    expect(mockHide).toHaveBeenCalledTimes(1);
  });
});
