import { useEffect, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useAppStore } from '../store/useAppStore';
import { isAudioPlaying, stopPronunciation } from '../utils/audio';

export type WindowAnimationStage = 'idle' | 'entering' | 'entered' | 'exiting';

export interface DismissOptions {
  immediate?: boolean;
  force?: boolean;
}

export interface ShellShownEvent {
  no_activate: boolean;
  generation: number;
}

export const SHELL_SHOWN_EVENT = 'transportal://shell-shown';

// Module-level state & listener dispatch so any hook or event can orchestrate dismiss.
// The overlay starts visible (the Rust setup shows it before our listener exists),
// so the initial stage must render the content rather than wait for an event.
let currentStage: WindowAnimationStage = 'entered';
const listeners = new Set<(stage: WindowAnimationStage) => void>();
let exitTimer: ReturnType<typeof setTimeout> | null = null;
let enterTimer: ReturnType<typeof setTimeout> | null = null;
/** Generation of the last `shell-shown` event; lets Rust reject stale hides. */
let shownGeneration: number | null = null;

function setStage(next: WindowAnimationStage) {
  currentStage = next;
  listeners.forEach((listener) => listener(next));
}

/**
 * Returns current animation stage of the shell window.
 */
export function getAnimationStage(): WindowAnimationStage {
  return currentStage;
}

/**
 * Hide the overlay unless Rust showed it again since our last `shell-shown`.
 * Resolves whether the window was actually hidden.
 */
async function hideWindow(): Promise<boolean> {
  try {
    return await invoke<boolean>('hide_shell', { generation: shownGeneration });
  } catch {
    try {
      await getCurrentWindow().hide();
    } catch {}
    return true;
  }
}

/** Settle the stage after a hide attempt, unless a newer show already took over. */
async function finishHide(): Promise<boolean> {
  const hidden = await hideWindow();
  if (currentStage === 'exiting' || currentStage === 'idle') {
    setStage(hidden ? 'idle' : 'entered');
  }
  return hidden;
}

/**
 * Gracefully dismisses the shell window with a 100ms smooth fade-out and scale-down.
 * Respects pinned state, active auto-ocr, and audio playing unless forced.
 * If smoothTransitions is disabled or immediate is true, hides instantly.
 *
 * @returns Promise<boolean> true if the window was hidden, false if blocked or superseded.
 */
export async function dismissShell(options: DismissOptions = {}): Promise<boolean> {
  const { immediate = false, force = false } = options;
  const state = useAppStore.getState();

  if (!force) {
    if (state.pinned || state.autoOcr?.active || isAudioPlaying()) {
      return false;
    }
  }

  // Always stop pronunciation immediately when starting dismissal
  stopPronunciation();

  if (exitTimer !== null) {
    clearTimeout(exitTimer);
    exitTimer = null;
  }
  if (enterTimer !== null) {
    clearTimeout(enterTimer);
    enterTimer = null;
  }

  const smooth = state.smoothTransitions && !immediate;

  if (!smooth) {
    setStage('idle');
    return finishHide();
  }

  setStage('exiting');

  return new Promise<boolean>((resolve) => {
    exitTimer = setTimeout(() => {
      exitTimer = null;
      void finishHide().then(resolve);
    }, 100);
  });
}

/**
 * Abort a dismiss that is still fading out, e.g. because the user just
 * grabbed the window to move or resize it.
 */
export function cancelDismiss(): void {
  if (exitTimer === null) return;
  clearTimeout(exitTimer);
  exitTimer = null;
  setStage('entered');
}

/**
 * React hook that coordinates window entrance and exit animations.
 */
export function useWindowAnimation(): {
  stage: WindowAnimationStage;
  isExiting: boolean;
  isEntering: boolean;
  dismiss: (options?: DismissOptions) => Promise<boolean>;
} {
  const [stage, setLocalStage] = useState<WindowAnimationStage>(currentStage);

  useEffect(() => {
    listeners.add(setLocalStage);
    return () => {
      listeners.delete(setLocalStage);
    };
  }, []);

  useEffect(() => {
    const unlistenPromise = listen<ShellShownEvent>(SHELL_SHOWN_EVENT, (event) => {
      if (typeof event.payload?.generation === 'number') {
        shownGeneration = event.payload.generation;
      }
      // If an exit timer was scheduled, abort it immediately!
      if (exitTimer !== null) {
        clearTimeout(exitTimer);
        exitTimer = null;
      }
      if (enterTimer !== null) {
        clearTimeout(enterTimer);
        enterTimer = null;
      }

      const smooth = useAppStore.getState().smoothTransitions;
      if (!smooth) {
        setStage('entered');
        return;
      }

      setStage('entering');
      enterTimer = setTimeout(() => {
        enterTimer = null;
        setStage('entered');
      }, 120);
    });

    // Safety net: a window that gains focus is on screen, so never leave its
    // content transparent even if a show path failed to announce itself.
    const unlistenFocus = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused && currentStage === 'idle') setStage('entered');
    });

    return () => {
      void unlistenPromise.then((unlisten) => unlisten()).catch(() => {});
      void unlistenFocus.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);

  const dismiss = useCallback(
    (options?: DismissOptions) => dismissShell(options),
    [],
  );

  return {
    stage,
    isExiting: stage === 'exiting',
    isEntering: stage === 'entering',
    dismiss,
  };
}
