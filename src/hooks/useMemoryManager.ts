import { useEffect, useRef } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import type { MemoryReport, TrimResult } from '../types';

/** Idle delay before triggering deep physical working set trimming (3 seconds). */
const IDLE_TRIM_DELAY_MS = 3000;

export async function fetchMemoryUsage(): Promise<MemoryReport | null> {
  try {
    return await invoke<MemoryReport>('get_memory_usage');
  } catch (error) {
    console.error('Failed to fetch memory usage:', error);
    return null;
  }
}

export async function trimMemoryNow(): Promise<TrimResult | null> {
  try {
    return await invoke<TrimResult>('trim_memory');
  } catch (error) {
    console.error('Failed to trim memory:', error);
    return null;
  }
}

/**
 * Two-tier memory manager for the TransPortal window:
 * - Tier 1: On blur, immediate preparation.
 * - Tier 2: If the window remains idle/hidden for >= 3 seconds, triggers deep
 *   physical working set compression across all processes (Rust + WebView2 child processes).
 */
export function useMemoryManager(enabled = true) {
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!enabled) return;
    const window = getCurrentWindow();

    const cancelIdleTimer = () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };

    const scheduleIdleTrim = () => {
      cancelIdleTimer();
      timerRef.current = setTimeout(() => {
        void trimMemoryNow();
      }, IDLE_TRIM_DELAY_MS);
    };

    const unlisten = window.onFocusChanged(({ payload: focused }) => {
      if (focused) {
        cancelIdleTimer();
      } else {
        scheduleIdleTrim();
      }
    });

    return () => {
      cancelIdleTimer();
      void unlisten.then((dispose) => dispose());
    };
  }, [enabled]);
}
