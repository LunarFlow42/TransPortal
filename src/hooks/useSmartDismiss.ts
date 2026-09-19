import { useEffect, useRef, useCallback } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import { useAppStore } from '../store/useAppStore';
import { isAutoHideSuppressed } from '../utils/autoHide';
import { isAudioPlaying } from '../utils/audio';

export interface ShellShownEvent {
  no_activate: boolean;
}

/**
 * Manages hybrid smart auto-dismiss for no-activate floating windows:
 * - Starts a 6-second idle timer when shown without stealing focus.
 * - Pauses/freezes the timer when the mouse hovers over the window.
 * - Starts a 1.5-second grace buffer timer when the mouse leaves.
 * - Upgrades to full interactive session when the user clicks inside.
 * - Protects against premature dismissal when pinned, auto-OCR active, or audio playing.
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
    if (store.pinned) return;
    if (store.autoOcr?.active) return;
    if (isAutoHideSuppressed()) return;
    if (isAudioPlaying()) return;
    if (isHoveredRef.current) return;

    clearTimers();
    isNoActivateRef.current = false;
    void getCurrentWindow().hide();
  }, [clearTimers]);

  useEffect(() => {
    if (!active) return;

    const unlistenPromise = listen<ShellShownEvent>('transportal://shell-shown', (event) => {
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

    return () => {
      clearTimers();
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, [active, clearTimers, dismissIfEligible]);

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

  const onMouseDown = useCallback(() => {
    clearTimers();
    isNoActivateRef.current = false;
  }, [clearTimers]);

  return {
    onMouseEnter,
    onMouseLeave,
    onMouseDown,
  };
}
