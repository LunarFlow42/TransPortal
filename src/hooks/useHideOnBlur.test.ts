import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useHideOnBlur } from './useHideOnBlur';
import { useAppStore } from '../store/useAppStore';
import { suppressAutoHide } from '../utils/autoHide';

const mockHide = vi.fn();
let focusCallback: ((event: { payload: boolean }) => void) | null = null;
const mockUnlisten = vi.fn();

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    hide: mockHide,
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
    lifecycle.effects.length = 0;
    useAppStore.setState({
      autoHideOnBlur: true,
      pinned: false,
      autoOcr: null,
      smoothTransitions: true,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('hides after 100ms dismiss animation when window loses focus and autoHideOnBlur is true', () => {
    useHideOnBlur(true);
    const effect = lifecycle.effects[0];
    effect();

    expect(focusCallback).toBeTypeOf('function');
    focusCallback!({ payload: false });

    expect(mockHide).not.toHaveBeenCalled();
    vi.advanceTimersByTime(100);
    expect(mockHide).toHaveBeenCalledTimes(1);
  });

  it('hides immediately when smoothTransitions is disabled', () => {
    useAppStore.setState({ smoothTransitions: false });
    useHideOnBlur(true);
    const effect = lifecycle.effects[0];
    effect();

    focusCallback!({ payload: false });
    expect(mockHide).toHaveBeenCalledTimes(1);
  });

  it('does not hide when focus is gained', () => {
    useHideOnBlur(true);
    const effect = lifecycle.effects[0];
    effect();

    focusCallback!({ payload: true });

    expect(mockHide).not.toHaveBeenCalled();
  });

  it('does not hide when autoHideOnBlur is disabled', () => {
    useAppStore.setState({ autoHideOnBlur: false });

    useHideOnBlur(true);
    const effect = lifecycle.effects[0];
    effect();

    focusCallback!({ payload: false });

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
});
