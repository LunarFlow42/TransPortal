import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { useAppStore } from '../store/useAppStore';
import type { MonitorUpdate } from '../types';

export function useAutoOcrEvents() {
  useEffect(() => {
    let disposed = false;
    const apply = (event: MonitorUpdate) => { if (!disposed) useAppStore.getState().applyAutoOcr(event); };
    const unlisten = listen<MonitorUpdate>('transportal://auto-ocr', ({ payload }) => apply(payload));
    void unlisten.then(() => invoke<MonitorUpdate>('auto_ocr_latest')).then(apply)
      .catch((error) => console.error('automatic OCR subscription failed:', error));
    return () => { disposed = true; void unlisten.then((dispose) => dispose()).catch(() => {}); };
  }, []);
}
