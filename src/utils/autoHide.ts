let suppressedUntil = 0;

/**
 * Temporarily prevent the overlay from auto-hiding on blur.
 *
 * Starting a native window drag makes the webview lose focus, which would
 * otherwise trip the hide-on-blur behaviour mid-drag.
 */
export function suppressAutoHide(ms = 3000): void {
  suppressedUntil = Math.max(suppressedUntil, Date.now() + ms);
}

export function isAutoHideSuppressed(): boolean {
  return Date.now() < suppressedUntil;
}
