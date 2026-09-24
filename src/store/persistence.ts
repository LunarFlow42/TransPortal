import { load, type Store } from '@tauri-apps/plugin-store';
import type { PersistedSettings } from '../types';

const SETTINGS_FILE = 'settings.json';

let storePromise: Promise<Store> | null = null;

function settingsStore(): Promise<Store> {
  if (!storePromise) {
    storePromise = load(SETTINGS_FILE, { autoSave: false }).catch((error) => {
      storePromise = null;
      throw error;
    });
  }
  return storePromise;
}

/** Read persisted settings. Missing keys come back as `undefined`. */
export async function loadSettings(): Promise<Partial<PersistedSettings>> {
  const store = await settingsStore();
  const [
    provider,
    openaiConfig,
    ankiConfig,
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
    autostart,
    theme,
    overlayOpacity,
    fontSize,
    showCrosshairs,
    aiPromptTemplate,
    autoHideOnBlur,
    targetLanguage,
    ocrPictureDefaultView,
    acrylicEffect,
    smoothTransitions,
    dictSources,
  ] = await Promise.all([
    store.get<PersistedSettings['provider']>('provider'),
    store.get<PersistedSettings['openaiConfig']>('openaiConfig'),
    store.get<PersistedSettings['ankiConfig']>('ankiConfig'),
    store.get<PersistedSettings['hotkey']>('hotkey'),
    store.get<PersistedSettings['ocrCardHotkey']>('ocrCardHotkey'),
    store.get<PersistedSettings['ocrPictureHotkey']>('ocrPictureHotkey'),
    store.get<PersistedSettings['ocrTextHotkey']>('ocrTextHotkey'),
    store.get<PersistedSettings['replaceSelectionHotkey']>('replaceSelectionHotkey'),
    store.get<PersistedSettings['silentOcrHotkey']>('silentOcrHotkey'),
    store.get<PersistedSettings['inputTranslateHotkey']>('inputTranslateHotkey'),
    store.get<PersistedSettings['inkTranslateKey']>('inkTranslateKey'),
    store.get<PersistedSettings['doubleCopyEnabled']>('doubleCopyEnabled'),
    store.get<PersistedSettings['clipboardMonitorEnabled']>('clipboardMonitorEnabled'),
    store.get<PersistedSettings['autoPronounce']>('autoPronounce'),
    store.get<PersistedSettings['ocrEngine']>('ocrEngine'),
    store.get<PersistedSettings['ocrLanguage']>('ocrLanguage'),
    store.get<PersistedSettings['autostart']>('autostart'),
    store.get<PersistedSettings['theme']>('theme'),
    store.get<PersistedSettings['overlayOpacity']>('overlayOpacity'),
    store.get<PersistedSettings['fontSize']>('fontSize'),
    store.get<PersistedSettings['showCrosshairs']>('showCrosshairs'),
    store.get<PersistedSettings['aiPromptTemplate']>('aiPromptTemplate'),
    store.get<PersistedSettings['autoHideOnBlur']>('autoHideOnBlur'),
    store.get<PersistedSettings['targetLanguage']>('targetLanguage'),
    store.get<PersistedSettings['ocrPictureDefaultView']>('ocrPictureDefaultView'),
    store.get<PersistedSettings['acrylicEffect']>('acrylicEffect'),
    store.get<PersistedSettings['smoothTransitions']>('smoothTransitions'),
    store.get<PersistedSettings['dictSources']>('dictSources'),
  ]);

  return {
    provider,
    openaiConfig,
    ankiConfig,
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
    autostart,
    theme,
    overlayOpacity,
    fontSize,
    showCrosshairs,
    aiPromptTemplate,
    autoHideOnBlur,
    targetLanguage,
    ocrPictureDefaultView,
    acrylicEffect,
    smoothTransitions,
    dictSources,
  };
}

/** Merge a partial settings patch into the persisted file. */
export async function saveSettings(
  patch: Partial<PersistedSettings>,
): Promise<void> {
  const store = await settingsStore();
  await Promise.all(
    Object.entries(patch).map(([key, value]) =>
      value === undefined ? Promise.resolve() : store.set(key, value),
    ),
  );
  await store.save();
}
