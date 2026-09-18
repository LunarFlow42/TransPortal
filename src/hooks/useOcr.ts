import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useAppStore } from '../store/useAppStore';
import type { OcrEvent, OcrStatus, ShortcutReport } from '../types';

function withShortcuts(status: OcrStatus, report: ShortcutReport | null): OcrStatus {
  if (!report) return status;
  const capture = report.shortcuts.find((shortcut) => shortcut.id === 'ocr_text');
  const translate = report.shortcuts.find((shortcut) => shortcut.id === 'ocr_translate');
  return { ...status,
    observing_shortcut: capture ? Boolean(capture.active) : status.observing_shortcut,
    shortcut_error: capture ? capture.error : status.shortcut_error,
    translate_shortcut_error: translate ? translate.error : status.translate_shortcut_error,
  };
}

export function useOcrEvents() {
  useEffect(() => {
    let disposed = false;
    const unlisten = listen<OcrEvent>('transportal://ocr', ({ payload }) => {
      if (!disposed) useAppStore.getState().applyOcrEvent(payload);
    });
    // Subscribe first, then replay: a screenshot completed during webview
    // startup must not disappear, or translate twice due to a duplicate event.
    void unlisten.then(async () => {
      if (disposed) return;
      const latest = await invoke<OcrEvent | null>('ocr_latest');
      if (!disposed && latest) useAppStore.getState().applyOcrEvent(latest);
    }).catch((error) => console.error('OCR event subscription failed:', error));
    return () => {
      disposed = true;
      void unlisten.then((dispose) => dispose());
    };
  }, []);
}

export function useOcrStatus() {
  const engine = useAppStore((state) => state.ocrEngine);
  const language = useAppStore((state) => state.ocrLanguage);
  const [status, setStatus] = useState<OcrStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const sequence = useRef(0);
  const shortcuts = useRef<ShortcutReport | null>(null);
  const refresh = useCallback(async () => {
    const request = ++sequence.current;
    setLoading(true);
    try {
      const next = await invoke<OcrStatus>('ocr_status');
      if (request === sequence.current) {
        setStatus(withShortcuts(next, shortcuts.current));
        setError(null);
      }
    } catch (error) {
      if (request === sequence.current) setError(String(error));
    } finally {
      if (request === sequence.current) setLoading(false);
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh, engine, language]);
  useEffect(() => {
    const unlisten = listen('transportal://ocr-assets-changed', () => { void refresh(); });
    return () => { void unlisten.then((dispose) => dispose()); };
  }, [refresh]);
  useEffect(() => {
    let disposed = false;
    const unlisten = listen<ShortcutReport>('transportal://shortcut-status', ({ payload }) => {
      if (disposed || payload.revision < (shortcuts.current?.revision ?? -1)) return;
      shortcuts.current = payload;
      setStatus((current) => current ? withShortcuts(current, payload) : current);
    });
    return () => {
      disposed = true;
      void unlisten.then((dispose) => dispose()).catch(() => {});
    };
  }, []);
  return { status, error, loading, refresh };
}
