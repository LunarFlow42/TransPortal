import { useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isAutoHideSuppressed } from '../utils/autoHide';
import { useAppStore } from '../store/useAppStore';

/**
 * Hide the current window immediately when it loses focus.
 *
 * Guarded against premature hiding when autoHideOnBlur is disabled,
 * pinned, auto-OCR is actively observing, or when auto-hide is temporarily
 * suppressed (e.g. during native window dragging or file dialogs).
 */
export function useHideOnBlur(active = true) {
  useEffect(() => {
    if (!active) return;
    const window = getCurrentWindow();

    const unlisten = window.onFocusChanged(({ payload: focused }) => {
      if (focused) return;

      const store = useAppStore.getState();
      if (!store.autoHideOnBlur) return;
      if (isAutoHideSuppressed()) return;
      if (store.pinned) return;
      if (store.autoOcr?.active) return;

      void window.hide();
    });

    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, [active]);
}

