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
      useAppStore.getState().hydrate({
        ...settings,
        hotkey: undefined,
        ocrCardHotkey: undefined,
        ocrPictureHotkey: undefined,
        ocrTextHotkey: undefined,
        replaceSelectionHotkey: undefined,
        silentOcrHotkey: undefined,
        inputTranslateHotkey: undefined,
        inkTranslateKey: undefined,
        doubleCopyEnabled: undefined,
        clipboardMonitorEnabled: undefined,
        ocrEngine: undefined,
        ocrLanguage: undefined,
      });

      if (settings.dictSources) {
        await applyDictSources(settings.dictSources);
      }
      if (disposed) return;

      if (settings.hotkey) {
        await useAppStore.getState().setShortcut('selection', settings.hotkey);
      }
      if (disposed) return;

      if (settings.ocrCardHotkey) {
        await useAppStore.getState().setShortcut('ocr_card', settings.ocrCardHotkey);
      }
      if (disposed) return;

      if (settings.ocrPictureHotkey) {
        await useAppStore.getState().setShortcut('ocr_picture', settings.ocrPictureHotkey);
      }
      if (disposed) return;

      if (settings.ocrTextHotkey) {
        await useAppStore.getState().setShortcut('ocr_text', settings.ocrTextHotkey);
      }
      if (disposed) return;

      if (settings.replaceSelectionHotkey) {
        await useAppStore.getState().setShortcut('replace_selection', settings.replaceSelectionHotkey);
      }
      if (disposed) return;

      if (settings.silentOcrHotkey) {
        await useAppStore.getState().setShortcut('silent_ocr', settings.silentOcrHotkey);
      }
      if (disposed) return;

      if (settings.inputTranslateHotkey) {
        await useAppStore.getState().setShortcut('input_translate', settings.inputTranslateHotkey);
      }
      if (disposed) return;

      if (settings.inkTranslateKey) {
        await useAppStore.getState().setInkTranslateKey(settings.inkTranslateKey);
      }
      if (disposed) return;

      if (settings.clipboardMonitorEnabled !== undefined) {
        await useAppStore.getState().setClipboardMonitorEnabled(settings.clipboardMonitorEnabled);
      }
      if (disposed) return;

      if (settings.doubleCopyEnabled !== undefined) {
        await useAppStore.getState().setDoubleCopyEnabled(settings.doubleCopyEnabled);
      }
      if (disposed) return;

      const nativeConfig = await invoke<OcrConfig>('get_ocr_config');
      if (disposed) return;
      useAppStore.getState().hydrate({
        ocrEngine: nativeConfig.engine,
        ocrLanguage: nativeConfig.language ?? 'auto',
      });
      if (settings.ocrEngine || settings.ocrLanguage) {
        await useAppStore.getState().setOcrConfig(
          settings.ocrEngine ?? nativeConfig.engine,
          settings.ocrLanguage ?? (nativeConfig.language ?? 'auto'),
        );
      }
      if (disposed) return;

      if (persist) {
        const {
          hotkey,
          ocrCardHotkey,
          ocrPictureHotkey,
          ocrTextHotkey,
          replaceSelectionHotkey,
          silentOcrHotkey,
          inputTranslateHotkey,
          inkTranslateKey,
          doubleCopyEnabled,
          clipboardMonitorEnabled,
          autoPronounce,
          ocrEngine,
          ocrLanguage,
          fontSize,
          showCrosshairs,
          aiPromptTemplate,
          targetLanguage,
          ocrPictureDefaultView,
        } = useAppStore.getState();
        await emit(SETTINGS_CHANGED_EVENT, {
          hotkey,
          ocrCardHotkey,
          ocrPictureHotkey,
          ocrTextHotkey,
          replaceSelectionHotkey,
          silentOcrHotkey,
          inputTranslateHotkey,
          inkTranslateKey,
          doubleCopyEnabled,
          clipboardMonitorEnabled,
          autoPronounce,
          ocrEngine,
          ocrLanguage,
          fontSize,
          showCrosshairs,
          aiPromptTemplate,
          targetLanguage,
          ocrPictureDefaultView,
        });
      }
    })().catch((error) => {
      if (!disposed) useAppStore.setState({ settingsError: `设置恢复失败：${String(error)}` });
    }).finally(() => {
      if (disposed) return;
      ready.current = true;
      setInitialized(true);
      void invoke('finish_startup_checks').catch((error) => {
        console.error('failed to finish startup checks:', error);
      });
    });

    if (persist) {
      unsubscribe = useAppStore.subscribe((state, previous) => {
        if (!ready.current) return;

        const patch: Partial<PersistedSettings> = {};
        if (state.provider !== previous.provider) patch.provider = state.provider;
        if (state.openaiConfig !== previous.openaiConfig) {
          patch.openaiConfig = state.openaiConfig;
        }
        if (state.hotkey !== previous.hotkey) patch.hotkey = state.hotkey;
        if (state.ocrCardHotkey !== previous.ocrCardHotkey) patch.ocrCardHotkey = state.ocrCardHotkey;
        if (state.ocrPictureHotkey !== previous.ocrPictureHotkey) patch.ocrPictureHotkey = state.ocrPictureHotkey;
        if (state.ocrTextHotkey !== previous.ocrTextHotkey) patch.ocrTextHotkey = state.ocrTextHotkey;
        if (state.replaceSelectionHotkey !== previous.replaceSelectionHotkey) patch.replaceSelectionHotkey = state.replaceSelectionHotkey;
        if (state.silentOcrHotkey !== previous.silentOcrHotkey) patch.silentOcrHotkey = state.silentOcrHotkey;
        if (state.inputTranslateHotkey !== previous.inputTranslateHotkey) patch.inputTranslateHotkey = state.inputTranslateHotkey;
        if (state.inkTranslateKey !== previous.inkTranslateKey) patch.inkTranslateKey = state.inkTranslateKey;
        if (state.doubleCopyEnabled !== previous.doubleCopyEnabled) patch.doubleCopyEnabled = state.doubleCopyEnabled;
        if (state.clipboardMonitorEnabled !== previous.clipboardMonitorEnabled) patch.clipboardMonitorEnabled = state.clipboardMonitorEnabled;
        if (state.autoPronounce !== previous.autoPronounce) patch.autoPronounce = state.autoPronounce;
        if (state.ocrEngine !== previous.ocrEngine) patch.ocrEngine = state.ocrEngine;
        if (state.ocrLanguage !== previous.ocrLanguage) patch.ocrLanguage = state.ocrLanguage;
        if (state.theme !== previous.theme) patch.theme = state.theme;
        if (state.overlayOpacity !== previous.overlayOpacity) {
          patch.overlayOpacity = state.overlayOpacity;
        }
        if (state.fontSize !== previous.fontSize) patch.fontSize = state.fontSize;
        if (state.showCrosshairs !== previous.showCrosshairs) patch.showCrosshairs = state.showCrosshairs;
        if (state.aiPromptTemplate !== previous.aiPromptTemplate) patch.aiPromptTemplate = state.aiPromptTemplate;
        if (state.targetLanguage !== previous.targetLanguage) {
          patch.targetLanguage = state.targetLanguage;
        }
        if (state.ocrPictureDefaultView !== previous.ocrPictureDefaultView) {
          patch.ocrPictureDefaultView = state.ocrPictureDefaultView;
        }
        if (state.smoothTransitions !== previous.smoothTransitions) {
          patch.smoothTransitions = state.smoothTransitions;
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
        void unlisten?.then((dispose) => dispose?.());
      };
    }

    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [persist]);

  return initialized;
}
