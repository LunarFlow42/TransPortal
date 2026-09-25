import { useEffect, useRef, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useAppStore } from '../store/useAppStore';
import { isAutoHideSuppressed } from '../utils/autoHide';
import { isAudioPlaying } from '../utils/audio';
import { dismissShell, SHELL_SHOWN_EVENT, type ShellShownEvent } from './useWindowAnimation';
import { SIZE_MOVE_EVENT } from './useHideOnBlur';

export type { ShellShownEvent };

/** How soon to re-check when a transient hold (audio, suppression) blocked a dismiss. */
const RETRY_MS = 1000;

/**
 * Manages hybrid smart auto-dismiss for no-activate floating windows:
 * - Starts a 6-second idle timer when shown without stealing focus.
 * - Pauses/freezes the timer when the mouse hovers over the window.
 * - Starts a 1.5-second grace buffer timer when the mouse leaves.
 * - Upgrades to full interactive session when the user clicks inside, or
 *   grabs the window to move or resize it.
 * - Never dismisses when auto-hide is disabled, pinned, or auto-OCR is active;
 *   playing audio or a temporary suppression only postpones the dismissal.
 */
export function useSmartDismiss(active = true) {
  const isNoActivateRef = useRef(false);
  const isHoveredRef = useRef(false);
  const idleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const leaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const clearTimers = useCallback(() => {
    if (idleTimerRef.current) {
      clearTimeout(idleTimerRef.current);
      idleTimerRef.current = null;
    }
    if (leaveTimerRef.current) {
      clearTimeout(leaveTimerRef.current);
      leaveTimerRef.current = null;
    }
  }, []);

  const dismissIfEligible = useCallback(() => {
    const store = useAppStore.getState();
    if (!store.autoHideOnBlur || store.pinned || store.autoOcr?.active) return;
    if (isHoveredRef.current) return;
    if (isAutoHideSuppressed() || isAudioPlaying()) {
      clearTimers();
      idleTimerRef.current = setTimeout(dismissIfEligible, RETRY_MS);
      return;
    }

    clearTimers();
    isNoActivateRef.current = false;
    void dismissShell();
  }, [clearTimers]);

  /** Any deliberate interaction turns the lightweight popup into a normal session. */
  const interact = useCallback(() => {
    clearTimers();
    isNoActivateRef.current = false;
  }, [clearTimers]);

  useEffect(() => {
    if (!active) return;

    const unlistenPromise = listen<ShellShownEvent>(SHELL_SHOWN_EVENT, (event) => {
      clearTimers();
      isHoveredRef.current = false;
      const noActivate = event.payload?.no_activate ?? false;
      isNoActivateRef.current = noActivate;

      if (noActivate) {
        // Start 6-second idle timer for lightweight reading
        idleTimerRef.current = setTimeout(() => {
          dismissIfEligible();
        }, 6000);
      }
    });

    // Edge and grip presses never reach the webview's mousedown handler.
    const unlistenSizeMove = listen<boolean>(SIZE_MOVE_EVENT, ({ payload: moving }) => {
      if (moving) interact();
    });

    return () => {
      clearTimers();
      void unlistenPromise.then((unlisten) => unlisten());
      void unlistenSizeMove.then((unlisten) => unlisten());
    };
  }, [active, clearTimers, dismissIfEligible, interact]);

  const onMouseEnter = useCallback(() => {
    isHoveredRef.current = true;
    clearTimers();
  }, [clearTimers]);

  const onMouseLeave = useCallback(() => {
    isHoveredRef.current = false;
    if (isNoActivateRef.current) {
      if (leaveTimerRef.current) {
        clearTimeout(leaveTimerRef.current);
      }
      leaveTimerRef.current = setTimeout(() => {
        dismissIfEligible();
      }, 1500);
    }
  }, [dismissIfEligible]);


  return {
    onMouseEnter,
    onMouseLeave,
    onMouseDown: interact,
  };
}
