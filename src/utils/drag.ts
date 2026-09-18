import type React from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { suppressAutoHide } from './autoHide';

/**
 * Start a native window drag from a mouse-down on a drag handle.
 *
 * Interactive descendants (buttons, inputs, links) are ignored so their own
 * click handlers still fire. Auto-hide is suppressed because a caption drag
 * makes the webview lose focus.
 */
export function startWindowDrag(event: React.MouseEvent): void {
  if (event.button !== 0) return;
  const target = event.target as HTMLElement | null;
  if (target?.closest('button, a, input, select, textarea, [data-no-drag]')) {
    return;
  }
  suppressAutoHide();
  void getCurrentWindow()
    .startDragging()
    .catch((error) => console.error('startDragging failed:', error));
}
