import { afterEach, describe, expect, it, vi } from 'vitest';
import { isAutoHideSuppressed, suppressAutoHide } from './autoHide';

describe('autoHide', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('suppresses auto-hide until the duration elapses', () => {
    vi.useFakeTimers();
    expect(isAutoHideSuppressed()).toBe(false);

    suppressAutoHide(1000);
    expect(isAutoHideSuppressed()).toBe(true);

    vi.advanceTimersByTime(1001);
    expect(isAutoHideSuppressed()).toBe(false);
  });
});
