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

/**
 * End a suppression early, keeping at most `ms` of grace.
 *
 * `suppressAutoHide` only ever extends the window, so a long hold taken for
 * an interaction of unknown length must be released explicitly when it ends.
 */
export function resumeAutoHide(ms = 300): void {
  suppressedUntil = Math.min(suppressedUntil, Date.now() + ms);
}
