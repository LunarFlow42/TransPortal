import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isAutoHideSuppressed, resumeAutoHide, suppressAutoHide } from '../utils/autoHide';
import { useAppStore } from '../store/useAppStore';
import { cancelDismiss, dismissShell } from './useWindowAnimation';

/** Emitted by Rust when the mouse is pressed outside every TransPortal window. */
export const OUTSIDE_CLICK_EVENT = 'transportal://outside-click';
/** Emitted by Rust with `true`/`false` when the user starts/stops moving or resizing the overlay. */
export const SIZE_MOVE_EVENT = 'transportal://shell-size-move';

/**
 * Hide the current window when the user leaves it: on blur, or on a click
 * elsewhere. The click is reported natively because an overlay shown without
 * activation never has focus to lose.
 *
 * Guarded against premature hiding when the window is pinned, auto-OCR is actively observing,
 * or when auto-hide is temporarily suppressed (e.g. during native window dragging or file dialogs). Moving or
 * resizing the overlay steals focus from the webview, so auto-hide is held
 * for the whole interaction. Playing pronunciation does not keep the overlay
 * open: leaving it is an explicit dismissal, which also stops the audio.
 */
export function useHideOnBlur(active = true) {
  useEffect(() => {
    if (!active) return;

    const dismissOnLeave = () => {
      const store = useAppStore.getState();
      if (isAutoHideSuppressed()) return;
      if (store.pinned) return;
      if (store.autoOcr?.active) return;

      void dismissShell({ force: true });
    };

    const unlistenFocus = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (!focused) dismissOnLeave();
    });
    const unlistenOutside = listen(OUTSIDE_CLICK_EVENT, dismissOnLeave);
    const unlistenSizeMove = listen<boolean>(SIZE_MOVE_EVENT, ({ payload: moving }) => {
      if (moving) {
        // The blur may have arrived first and already started fading out.
        suppressAutoHide(60_000);
        cancelDismiss();
      } else {
        resumeAutoHide();
      }
    });

    return () => {
      void unlistenFocus.then((dispose) => dispose());
      void unlistenOutside.then((dispose) => dispose());
      void unlistenSizeMove.then((dispose) => dispose());
    };
  }, [active]);
}
