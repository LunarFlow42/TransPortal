import { useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isAutoHideSuppressed } from '../utils/autoHide';
import { useAppStore } from '../store/useAppStore';

const HIDE_DELAY_MS = 200;

/**
 * Hide the current window shortly after it loses focus.
 *
 * A short delay + focus re-check prevents the window from disappearing while a
 * native drag (which briefly blurs the webview) is in progress.
 */
export function useHideOnBlur() {
  useEffect(() => {
    const window = getCurrentWindow();
    let timer: ReturnType<typeof setTimeout> | undefined;

    const unlisten = window.onFocusChanged(({ payload: focused }) => {
      if (focused) {
        if (timer) {
          clearTimeout(timer);
          timer = undefined;
        }
        return;
      }

      if (timer) clearTimeout(timer);
      timer = setTimeout(async () => {
        timer = undefined;
        if (isAutoHideSuppressed()) return;
        if (useAppStore.getState().pinned) return;
        if (useAppStore.getState().autoOcr?.active) return;
        try {
          if (await window.isFocused()) return;
        } catch {
          // Fall through and hide if focus state cannot be read.
        }
        void window.hide();
      }, HIDE_DELAY_MS);
    });

    return () => {
      if (timer) clearTimeout(timer);
      void unlisten.then((dispose) => dispose());
    };
  }, []);
}
