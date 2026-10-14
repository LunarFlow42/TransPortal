import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke, Channel } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { TtsInstallProgress, TtsInstallStatus } from '../types';
import { playPronunciation } from '../utils/audio';

export function useTtsStatus() {
  const [status, setStatus] = useState<TtsInstallStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [progress, setProgress] = useState<TtsInstallProgress | null>(null);
  const sequence = useRef(0);

  const refresh = useCallback(async () => {
    const request = ++sequence.current;
    setLoading(true);
    try {
      const next = await invoke<TtsInstallStatus>('tts_status');
      if (request === sequence.current) {
        setStatus(next);
        setError(null);
      }
    } catch (err) {
      if (request === sequence.current) setError(String(err));
    } finally {
      if (request === sequence.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const unlisten = listen('transportal://tts-assets-changed', () => {
      void refresh();
    });
    return () => {
      void unlisten.then((dispose) => dispose()).catch(() => {});
    };
  }, [refresh]);

  const install = useCallback(async () => {
    setError(null);
    const channel = new Channel<TtsInstallProgress>();
    channel.onmessage = (p) => {
      setProgress(p);
    };

    try {
      await invoke('install_onnx_tts', { onProgress: channel });
      setProgress(null);
      await refresh();
    } catch (err) {
      setError(String(err));
      setProgress(null);
      throw err;
    }
  }, [refresh]);

  const deleteTts = useCallback(async () => {
    setError(null);
    try {
      await invoke('delete_onnx_tts');
      await refresh();
    } catch (err) {
      setError(String(err));
      throw err;
    }
  }, [refresh]);

  const previewVoice = useCallback((voiceId: string, speed = 1.0) => {
    const sampleText = 'Welcome to TransPortal, your local translation assistant.';
    playPronunciation(sampleText, null, () => {}, { voice: voiceId, speed });
  }, []);

  return {
    status,
    error,
    loading,
    progress,
    refresh,
    install,
    deleteTts,
    previewVoice,
  };
}
