import { useEffect, useState, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useAppStore } from '../store/useAppStore';
import { isAudioPlaying, stopPronunciation } from '../utils/audio';

export type WindowAnimationStage = 'idle' | 'entering' | 'entered' | 'exiting';

export interface DismissOptions {
  immediate?: boolean;
  force?: boolean;
}

export const SHELL_SHOWN_EVENT = 'transportal://shell-shown';

// Module-level state & listener dispatch so any hook or event can orchestrate dismiss
let currentStage: WindowAnimationStage = 'idle';
const listeners = new Set<(stage: WindowAnimationStage) => void>();
let exitTimer: ReturnType<typeof setTimeout> | null = null;
let enterTimer: ReturnType<typeof setTimeout> | null = null;

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
 * Gracefully dismisses the shell window with a 100ms smooth fade-out and scale-down.
 * Respects pinned state, active auto-ocr, and audio playing unless forced.
 * If smoothTransitions is disabled or immediate is true, hides instantly.
 *
 * @returns Promise<boolean> true if dismissal was executed or initiated, false if blocked.
 */
export async function dismissShell(options: DismissOptions = {}): Promise<boolean> {
  const { immediate = false, force = false } = options;
  const state = useAppStore.getState();

  if (!force) {
    if (state.pinned || state.autoOcr || isAudioPlaying()) {
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
    try {
      await getCurrentWindow().hide();
    } catch {}
    return true;
  }

  setStage('exiting');

  return new Promise<boolean>((resolve) => {
    exitTimer = setTimeout(async () => {
      exitTimer = null;
      try {
        await getCurrentWindow().hide();
      } catch {}
      setStage('idle');
      resolve(true);
    }, 100);
  });
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
    const unlistenPromise = listen(SHELL_SHOWN_EVENT, () => {
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

    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
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
