import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { useAppStore } from '../store/useAppStore';
import { loadSettings, saveSettings } from '../store/persistence';
import type { OcrConfig, PersistedSettings } from '../types';

/** Event used to push settings from the settings window to the overlay. */
export const SETTINGS_CHANGED_EVENT = 'transportal://settings-changed';

/** Restore persisted dictionary source order/enabled flags into the Rust core. */
async function applyDictSources(
  list: PersistedSettings['dictSources'] | undefined,
): Promise<void> {
  if (!list?.length) return;
  try {
    await invoke('set_dict_source_order', { ids: list.map((source) => source.id) });
    await Promise.all(
      list.map((source) =>
        invoke('set_dict_source_enabled', { id: source.id, enabled: source.enabled }),
      ),
    );
  } catch (error) {
    console.error('failed to restore dictionary sources:', error);
  }
}

/**
 * Load persisted settings on startup and keep them in sync.
 *
 * The settings window is the writer (`persist = true`): it saves changes and
 * broadcasts them. The overlay is a reader (`persist = false`): it applies
 * pushed updates so translation config stays current without a reload.
 */
export function useBootstrap(persist: boolean) {
  const ready = useRef(false);
  const [initialized, setInitialized] = useState(false);

  useEffect(() => {
    ready.current = false;
    setInitialized(false);
    let disposed = false;
    let unsubscribe: (() => void) | undefined;

    void (async () => {
      const settings = await loadSettings();
      if (disposed) return;

      // Only publish settings backed by a successful native registration.
      useAppStore.getState().hydrate(persist
        ? { ...settings, hotkey: undefined, doubleCopyEnabled: undefined, ocrEngine: undefined, ocrLanguage: undefined }
        : settings);

      if (persist) {
        await applyDictSources(settings.dictSources);
        if (disposed) return;
        if (settings.hotkey) {
          await useAppStore.getState().setHotkey(settings.hotkey);
        }
        if (disposed) return;
        if (settings.doubleCopyEnabled !== undefined) {
          await useAppStore.getState().setDoubleCopyEnabled(settings.doubleCopyEnabled);
        }
        if (disposed) return;
        const nativeConfig = await invoke<OcrConfig>('get_ocr_config');
        if (disposed) return;
        useAppStore.getState().hydrate({ ocrEngine: nativeConfig.engine, ocrLanguage: nativeConfig.language ?? 'auto' });
        if (settings.ocrEngine || settings.ocrLanguage) {
          await useAppStore.getState().setOcrConfig(settings.ocrEngine ?? nativeConfig.engine, settings.ocrLanguage ?? 'auto');
        }
        if (disposed) return;
        const { hotkey, doubleCopyEnabled, ocrEngine, ocrLanguage } = useAppStore.getState();
        await emit(SETTINGS_CHANGED_EVENT, { hotkey, doubleCopyEnabled, ocrEngine, ocrLanguage });
      }
    })().catch((error) => {
      if (!disposed) useAppStore.setState({ settingsError: `设置恢复失败：${String(error)}` });
    }).finally(() => {
      if (disposed) return;
      ready.current = true;
      setInitialized(true);
      if (persist) {
        void invoke('finish_startup_checks').catch((error) => {
          console.error('failed to finish startup checks:', error);
        });
      }
    });

    if (persist) {
      unsubscribe = useAppStore.subscribe((state, previous) => {
        if (!ready.current) return;

        const patch: Partial<PersistedSettings> = {};
        if (state.provider !== previous.provider) patch.provider = state.provider;
        if (state.openaiConfig !== previous.openaiConfig) {
          patch.openaiConfig = state.openaiConfig;
        }
        if (state.ankiConfig !== previous.ankiConfig) patch.ankiConfig = state.ankiConfig;
        if (state.hotkey !== previous.hotkey) patch.hotkey = state.hotkey;
        if (state.doubleCopyEnabled !== previous.doubleCopyEnabled) patch.doubleCopyEnabled = state.doubleCopyEnabled;
        if (state.ocrEngine !== previous.ocrEngine) patch.ocrEngine = state.ocrEngine;
        if (state.ocrLanguage !== previous.ocrLanguage) patch.ocrLanguage = state.ocrLanguage;
        if (state.theme !== previous.theme) patch.theme = state.theme;
        if (state.overlayOpacity !== previous.overlayOpacity) {
          patch.overlayOpacity = state.overlayOpacity;
        }

        if (Object.keys(patch).length === 0) return;

        void saveSettings(patch).catch((error) => {
          useAppStore.setState({ settingsError: `设置保存失败：${String(error)}` });
        });
        void emit(SETTINGS_CHANGED_EVENT, patch).catch((error) => {
          console.error('failed to synchronize settings:', error);
        });
      });
    } else {
      const unlisten = listen<Partial<PersistedSettings>>(
        SETTINGS_CHANGED_EVENT,
        (event) => {
          useAppStore.getState().hydrate(event.payload);
        },
      );

      return () => {
        disposed = true;
        void unlisten.then((dispose) => dispose());
      };
    }

    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [persist]);

  return initialized;
}
