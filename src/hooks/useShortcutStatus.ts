import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useAppStore } from '../store/useAppStore';
import type { ShortcutReport } from '../types';

export function useShortcutStatus() {
  const [report, setReport] = useState<ShortcutReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [retrying, setRetrying] = useState(false);
  const revision = useRef(-1);
  const accept = useCallback((next: ShortcutReport) => {
    if (next.revision < revision.current) return;
    revision.current = next.revision;
    setReport(next);
    setError(null);
    const selection = next.shortcuts.find((shortcut) => shortcut.id === 'selection');
    // A failed saved binding may be using a fallback. Keep the saved preference
    // until a retry succeeds; the settings window remains the persistence writer.
    if (next.ready && selection?.active && !selection.error) {
      useAppStore.getState().hydrate({ hotkey: selection.active });
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    const subscription = listen<ShortcutReport>('transportal://shortcut-status', ({ payload }) => {
      if (!disposed) accept(payload);
    });
    void subscription.then(async () => {
      if (disposed) return;
      const next = await invoke<ShortcutReport>('shortcut_status');
      if (!disposed) accept(next);
    }).catch((error) => { if (!disposed) setError(String(error)); });
    return () => {
      disposed = true;
      void subscription.then((unlisten) => unlisten()).catch(() => {});
    };
  }, [accept]);

  const retry = async () => {
    setRetrying(true);
    try { accept(await invoke<ShortcutReport>('retry_shortcuts')); }
    catch (error) { setError(String(error)); }
    finally { setRetrying(false); }
  };

  return { report, error, retrying, retry };
}
