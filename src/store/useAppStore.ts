import { create } from 'zustand';
import { invoke, Channel } from '@tauri-apps/api/core';
import { forgetScrollTops } from '../utils/scrollMemory';
import { isNewOcrEvent } from '../utils/ocr';
import { languageTagOf, resolveTargetLanguage } from '../utils/language';
import type {
  Density,
  DictEntry,
  ExampleSentence,
  IntentResult,
  LookupNode,
  OpenAiConfig,
  ProviderType,
  StreamChunk,
  AnkiConfig,
  AnkiStatus,
  CaptureOutcome,
  PersistedSettings,
  ThemeMode,
  OcrEvent,
  OcrMode,
  ShortcutActionId,
  OcrEngineId,
  OcrConfig,
  TextIntent,
  PictureTranslation,
  MonitorUpdate,
} from '../types';

/** Monotonic ids for stack frames. */
let nodeIdCounter = 0;
let lookupSession = 0;
let pictureRequest = 0;
const nextNodeId = () => (nodeIdCounter += 1);

/** Token per frame guarding against stale streams for the *same* frame. */
const streamTokens = new Map<number, number>();

/** Cap on concurrent LLM example generations so deep stacks can't stampede. */
let examplesInFlight = 0;
const MAX_EXAMPLE_REQUESTS = 3;

interface AppState {
  // Drill-down lookup stack (top of the array = current frame)
  lookupStack: LookupNode[];
  errorMessage: string | null;
  capture: CaptureOutcome | null;
  ocr: OcrEvent | null;
  lastOcrEvent: OcrEvent | null;
  ocrImageView: boolean;
  ocrPictureTranslation: PictureTranslation | null;
  ocrPictureBusy: boolean;
  ocrPictureError: string | null;
  ocrPictureOriginal: boolean;
  ocrPictureDefaultView: 'translated' | 'original';
  ocrTarget: string;
  autoOcr: MonitorUpdate | null;
  autoOcrRevision: number;

  setOcrPictureOriginal: (original: boolean) => void;
  toggleOcrPictureOriginal: () => void;
  setOcrPictureDefaultView: (view: 'translated' | 'original') => void;

  // Provider & Config
  provider: ProviderType;
  openaiConfig: OpenAiConfig;
  ankiConfig: AnkiConfig;
  ankiStatus: AnkiStatus;
  ankiMessage: string | null;
  hotkey: string;
  ocrCardHotkey: string;
  ocrPictureHotkey: string;
  ocrTextHotkey: string;
  replaceSelectionHotkey: string;
  silentOcrHotkey: string;
  inputTranslateHotkey: string;
  inkTranslateKey: string;
  doubleCopyEnabled: boolean;
  clipboardMonitorEnabled: boolean;
  autoPronounce: boolean;
  ocrEngine: OcrEngineId;
  ocrLanguage: string;
  lastOcrBenchmarkMs: number | null;
  settingsError: string | null;
  pinned: boolean;
  theme: ThemeMode;
  overlayOpacity: number;
  fontSize: number;
  showCrosshairs: boolean;
  aiPromptTemplate: 'direct' | 'academic' | 'code' | 'casual';
  backTranslation: { text: string; loading: boolean; error: string | null } | null;
  history: string[];
  historyIndex: number;
  autoHideOnBlur: boolean;
  targetLanguage: string;
  acrylicEffect: boolean;
  smoothTransitions: boolean;

  // Config actions
  setProvider: (provider: ProviderType) => void;
  setOpenaiConfig: (config: Partial<OpenAiConfig>) => void;
  setAnkiConfig: (config: Partial<AnkiConfig>) => void;
  setHotkey: (hotkey: string) => Promise<boolean>;
  setShortcut: (action: ShortcutActionId, accelerator: string) => Promise<boolean>;
  setDoubleCopyEnabled: (enabled: boolean) => Promise<boolean>;
  setClipboardMonitorEnabled: (enabled: boolean) => Promise<void>;
  setInkTranslateKey: (key: string) => Promise<void>;
  setFontSize: (size: number) => void;
  setShowCrosshairs: (show: boolean) => void;
  setAiPromptTemplate: (template: 'direct' | 'academic' | 'code' | 'casual') => void;
  runBackTranslation: () => Promise<void>;
  navigateHistory: (direction: 'prev' | 'next') => void;
  setAutoPronounce: (enabled: boolean) => void;
  setAutoHideOnBlur: (enabled: boolean) => void;
  setTargetLanguage: (targetLanguage: string) => void;
  setOcrLanguage: (language: string) => Promise<boolean>;
  setOcrEngine: (engine: OcrEngineId) => Promise<boolean>;
  setOcrConfig: (engine: OcrEngineId, language: string) => Promise<boolean>;
  benchmarkOcr: () => Promise<number>;
  setTheme: (theme: ThemeMode) => void;
  setOverlayOpacity: (opacity: number) => void;
  setAcrylicEffect: (enabled: boolean) => void;
  setSmoothTransitions: (enabled: boolean) => void;
  togglePinned: () => void;
  hydrate: (settings: Partial<PersistedSettings>) => void;

  // Native selection capture
  applyCapture: (outcome: CaptureOutcome) => void;
  applyOcrEvent: (event: OcrEvent) => void;
  startOcr: (mode: OcrMode) => Promise<void>;
  startImageOcr: (path: string, mode?: OcrMode) => Promise<void>;
  cancelOcr: () => void;
  translateOcrText: (text: string) => void;
  translateOcrPicture: (texts?: string[]) => Promise<void>;
  applyAutoOcr: (event: MonitorUpdate) => void;
  startAutoOcr: (intervalMs?: number) => Promise<void>;

  // Stack navigation
  processText: (text: string, forceDensity?: Density, forceIntent?: TextIntent) => Promise<void>;
  pushLookup: (
    word: string,
    origin?: { text: string; word: string },
  ) => Promise<void>;
  popLookup: () => void;
  popTo: (index: number) => void;
  clearLookup: () => void;
  setDensity: (density: Density) => void;
  patchNode: (id: number, patch: Partial<LookupNode>) => void;

  // Workflows
  startStreamTranslation: (text: string, nodeId: number) => Promise<void>;
  fetchWordExamples: (word: string, nodeId: number) => Promise<void>;
  syncToAnki: (contextSentence?: string) => Promise<boolean>;
  exportTsv: (contextSentence?: string) => void;
}

/** Strip surrounding punctuation from a clicked token. */
function cleanWord(raw: string): string {
  return raw.replace(/^[^\w\s]+|[^\w\s]+$/g, '').trim();
}

export const useAppStore = create<AppState>((set, get) => ({
  lookupStack: [],
  errorMessage: null,
  capture: null,
  ocr: null,
  lastOcrEvent: null,
  ocrImageView: false,
  ocrPictureTranslation: null,
  ocrPictureBusy: false,
  ocrPictureError: null,
  ocrPictureOriginal: false,
  ocrPictureDefaultView: 'translated',
  ocrTarget: 'zh-CN',
  autoOcr: null,
  autoOcrRevision: -1,

  provider: 'google_free',
  openaiConfig: {
    api_key: '',
    base_url: 'https://api.openai.com/v1',
    model: 'gpt-4o-mini',
    temperature: 0.3,
  },
  ankiConfig: {
    endpoint: 'http://127.0.0.1:8765',
    deckName: 'Default',
    modelName: 'Basic',
  },
  ankiStatus: 'idle',
  ankiMessage: null,
  hotkey: 'Alt+Q',
  ocrCardHotkey: 'Alt+Shift+X',
  ocrPictureHotkey: 'Alt+S',
  ocrTextHotkey: 'Alt+Shift+S',
  replaceSelectionHotkey: 'Alt+Z',
  silentOcrHotkey: 'Alt+Shift+C',
  inputTranslateHotkey: 'Alt+Space',
  inkTranslateKey: 'None',
  doubleCopyEnabled: true,
  clipboardMonitorEnabled: false,
  autoPronounce: true,
  ocrEngine: 'windows',
  ocrLanguage: 'auto',
  lastOcrBenchmarkMs: null,
  settingsError: null,
  pinned: false,
  theme: 'system',
  overlayOpacity: 0.72,
  fontSize: 14,
  showCrosshairs: true,
  aiPromptTemplate: 'direct',
  backTranslation: null,
  history: [],
  historyIndex: -1,
  autoHideOnBlur: true,
  targetLanguage: 'zh-CN',
  acrylicEffect: false,
  smoothTransitions: true,

  setOcrPictureOriginal: (ocrPictureOriginal) => set({ ocrPictureOriginal }),
  toggleOcrPictureOriginal: () => set((state) => ({ ocrPictureOriginal: !state.ocrPictureOriginal })),
  setOcrPictureDefaultView: (ocrPictureDefaultView) => set({ ocrPictureDefaultView }),

  setProvider: (provider) => set({ provider }),
  setOpenaiConfig: (config) =>
    set((state) => ({ openaiConfig: { ...state.openaiConfig, ...config } })),
  setAnkiConfig: (config) =>
    set((state) => ({ ankiConfig: { ...state.ankiConfig, ...config } })),
  setShortcut: async (action, accelerator) => {
    try {
      await invoke('set_shortcut', { action, accelerator });
      const patch: Partial<AppState> = { settingsError: null };
      if (action === 'selection') patch.hotkey = accelerator;
      else if (action === 'ocr_card') patch.ocrCardHotkey = accelerator;
      else if (action === 'ocr_picture' || action === 'ocr_translate') patch.ocrPictureHotkey = accelerator;
      else if (action === 'ocr_text') patch.ocrTextHotkey = accelerator;
      else if (action === 'replace_selection') patch.replaceSelectionHotkey = accelerator;
      else if (action === 'silent_ocr') patch.silentOcrHotkey = accelerator;
      else if (action === 'input_translate') patch.inputTranslateHotkey = accelerator;
      set(patch);
      return true;
    } catch (error) {
      set({ settingsError: String(error) });
      return false;
    }
  },
  setHotkey: async (hotkey) => {
    return get().setShortcut('selection', hotkey);
  },
  setDoubleCopyEnabled: async (doubleCopyEnabled) => {
    try {
      await invoke('set_double_copy_enabled', { enabled: doubleCopyEnabled });
      set({ doubleCopyEnabled, settingsError: null });
      return true;
    } catch (error) { set({ settingsError: String(error) }); return false; }
  },
  setClipboardMonitorEnabled: async (enabled) => {
    try {
      await invoke('set_clipboard_monitor_enabled', { enabled });
      set({ clipboardMonitorEnabled: enabled, settingsError: null });
    } catch (error) {
      set({ settingsError: String(error) });
    }
  },
  setInkTranslateKey: async (key) => {
    try {
      await invoke('set_ink_translate_key', { key });
      set({ inkTranslateKey: key, settingsError: null });
    } catch (error) {
      set({ settingsError: String(error) });
    }
  },
  setFontSize: (fontSize) => set({ fontSize }),
  setShowCrosshairs: (showCrosshairs) => set({ showCrosshairs }),
  setAiPromptTemplate: (aiPromptTemplate) => {
    set({ aiPromptTemplate });
    get().setOpenaiConfig({ prompt_template: aiPromptTemplate });
  },
  runBackTranslation: async () => {
    const { lookupStack, provider, openaiConfig } = get();
    const node = lookupStack[lookupStack.length - 1];
    if (!node || !node.translation) return;

    set({ backTranslation: { text: '', loading: true, error: null } });

    // Back-translation returns to the source text's language: the forward
    // target depends on the source, so the preference alone cannot tell.
    const backTarget = languageTagOf(node.text);
    let accumulated = '';
    const channel = new Channel<StreamChunk>();
    channel.onmessage = (chunk) => {
      if (chunk.error) {
        set({ backTranslation: { text: accumulated, loading: false, error: chunk.error } });
        return;
      }
      if (chunk.text) {
        accumulated += chunk.text;
        set({ backTranslation: { text: accumulated, loading: true, error: null } });
      }
      if (chunk.is_done) {
        set({ backTranslation: { text: accumulated, loading: false, error: null } });
      }
    };

    try {
      await invoke('translate_stream', {
        query: { text: node.translation, target_lang: backTarget },
        provider,
        openaiConfig: provider === 'openai' ? openaiConfig : null,
        onChunk: channel,
      });
    } catch (err: unknown) {
      set({ backTranslation: { text: accumulated, loading: false, error: String(err) } });
    }
  },
  navigateHistory: (direction) => {
    const { history, historyIndex } = get();
    if (history.length === 0) return;
    let newIndex = historyIndex;
    if (direction === 'prev') {
      if (newIndex === -1) {
        newIndex = history.length - 2;
      } else if (newIndex > 0) {
        newIndex -= 1;
      }
    } else if (direction === 'next') {
      if (newIndex !== -1 && newIndex < history.length - 1) {
        newIndex += 1;
      }
    }
    if (newIndex >= 0 && newIndex < history.length && newIndex !== historyIndex) {
      set({ historyIndex: newIndex });
      const item = history[newIndex];
      if (item) {
        void get().processText(item);
      }
    }
  },
  setAutoPronounce: (autoPronounce) => set({ autoPronounce }),
  setAutoHideOnBlur: (autoHideOnBlur) => set({ autoHideOnBlur }),
  setTargetLanguage: (targetLanguage) => set({ targetLanguage, ocrTarget: targetLanguage }),
  setOcrLanguage: (language) => get().setOcrConfig(get().ocrEngine, language),
  setOcrEngine: (engine) => get().setOcrConfig(engine, 'auto'),
  setOcrConfig: async (ocrEngine, ocrLanguage) => {
    try {
      const applied = await invoke<OcrConfig>('set_ocr_config', {
        config: { engine: ocrEngine, language: ocrLanguage === 'auto' ? null : ocrLanguage },
      });
      set({ ocrEngine: applied.engine, ocrLanguage: applied.language ?? 'auto', settingsError: null });
      return true;
    } catch (error) {
      set({ settingsError: String(error) });
      return false;
    }
  },
  benchmarkOcr: async () => {
    try {
      const ms = await invoke<number>('benchmark_ocr');
      set({ lastOcrBenchmarkMs: ms, settingsError: null });
      return ms;
    } catch (error) {
      set({ settingsError: String(error) });
      throw error;
    }
  },
  setTheme: (theme) => set({ theme }),
  setOverlayOpacity: (overlayOpacity) => set({ overlayOpacity }),
  setAcrylicEffect: (acrylicEffect) => set({ acrylicEffect }),
  setSmoothTransitions: (smoothTransitions) => set({ smoothTransitions }),
  togglePinned: () =>
    set((state) => {
      const nextPinned = !state.pinned;
      invoke('set_window_pinned', { pinned: nextPinned }).catch(() => {});
      return { pinned: nextPinned };
    }),

  hydrate: (settings) =>
    set((state) => ({
      provider: settings.provider ?? state.provider,
      openaiConfig: settings.openaiConfig ?? state.openaiConfig,
      ankiConfig: settings.ankiConfig ?? state.ankiConfig,
      hotkey: settings.hotkey ?? state.hotkey,
      ocrCardHotkey: settings.ocrCardHotkey ?? state.ocrCardHotkey,
      ocrPictureHotkey: settings.ocrPictureHotkey ?? state.ocrPictureHotkey,
      ocrTextHotkey: settings.ocrTextHotkey ?? state.ocrTextHotkey,
      replaceSelectionHotkey: settings.replaceSelectionHotkey ?? state.replaceSelectionHotkey,
      silentOcrHotkey: settings.silentOcrHotkey ?? state.silentOcrHotkey,
      inputTranslateHotkey: settings.inputTranslateHotkey ?? state.inputTranslateHotkey,
      inkTranslateKey: settings.inkTranslateKey ?? state.inkTranslateKey,
      doubleCopyEnabled: settings.doubleCopyEnabled ?? state.doubleCopyEnabled,
      clipboardMonitorEnabled: settings.clipboardMonitorEnabled ?? state.clipboardMonitorEnabled,
      autoPronounce: settings.autoPronounce ?? state.autoPronounce,
      ocrEngine: settings.ocrEngine ?? state.ocrEngine,
      ocrLanguage: settings.ocrLanguage ?? state.ocrLanguage,
      theme: settings.theme ?? state.theme,
      overlayOpacity: settings.overlayOpacity ?? state.overlayOpacity,
      fontSize: settings.fontSize ?? state.fontSize,
      showCrosshairs: settings.showCrosshairs ?? state.showCrosshairs,
      aiPromptTemplate: settings.aiPromptTemplate ?? state.aiPromptTemplate,
      autoHideOnBlur: settings.autoHideOnBlur ?? state.autoHideOnBlur,
      targetLanguage: settings.targetLanguage ?? state.targetLanguage,
      ocrTarget: settings.targetLanguage ?? state.ocrTarget,
      ocrPictureDefaultView: settings.ocrPictureDefaultView ?? state.ocrPictureDefaultView,
      acrylicEffect: settings.acrylicEffect ?? state.acrylicEffect,
      smoothTransitions: settings.smoothTransitions ?? state.smoothTransitions,
    })),

  applyCapture: (outcome) => {
    get().clearLookup();
    set({ capture: outcome, autoOcr: null });
    const text = outcome.text.trim();
    if (text) {
      void get().processText(text);
    }
  },

  applyOcrEvent: (event) => {
    if (!isNewOcrEvent(event, get().lastOcrEvent)) return;
    set({ lastOcrEvent: event });
    if (event.elapsed_ms != null) {
      set({ lastOcrBenchmarkMs: event.elapsed_ms });
    }
    if (event.stage === 'cancelled') {
      pictureRequest++;
      set({ ocr: null, ocrPictureBusy: false, ocrPictureTranslation: null });
      return;
    }
    if (event.stage === 'capturing') get().clearLookup();
    if (event.stage === 'ready' && (event.mode === 'picture' || (event.mode as string) === 'translate')) {
      if (event.picture) {
        const isOriginal = get().ocrPictureDefaultView === 'original';
        set({ ocr: event, capture: null, errorMessage: null, ocrImageView: true, ocrPictureOriginal: isOriginal });
        void get().translateOcrPicture();
        return;
      }
      get().translateOcrText(event.text);
      return;
    }
    if (event.stage === 'ready' && event.mode === 'card') {
      const isOriginal = get().ocrPictureDefaultView === 'original';
      set({ ocr: event, capture: null, errorMessage: null, ocrImageView: false, ocrPictureOriginal: isOriginal });
      return;
    }
    // Text-only OCR has no call into detect_intent, dictionary or providers.
    set({ ocr: event, capture: null, errorMessage: null });
  },

  startOcr: async (mode) => {
    try {
      set({ errorMessage: null });
      await invoke('start_ocr', { mode });
    } catch (error) {
      set({ errorMessage: String(error) });
    }
  },

  startImageOcr: async (path, mode = 'text') => {
    try {
      set({ errorMessage: null });
      await invoke('ocr_image', { path, mode });
    } catch (error) {
      set({ errorMessage: String(error) });
    }
  },

  cancelOcr: () => {
    pictureRequest++;
    const isOriginal = get().ocrPictureDefaultView === 'original';
    set({ ocrPictureBusy: false, ocrPictureOriginal: isOriginal });
    const current = get().ocr;
    if (current) {
      set({ ocr: null, lastOcrEvent: { ...current, stage: 'cancelled' } });
    }
    void invoke('cancel_ocr').catch((error) => console.error('OCR cancellation failed:', error));
  },

  translateOcrText: (text) => {
    if (!text.trim()) return;
    set({ ocr: null, capture: { text, method: 'ocr' } });
    void get().processText(text, 'compact', 'sentence');
  },

  translateOcrPicture: async (texts) => {
    const { ocr, provider, openaiConfig, ocrTarget } = get();
    if (!ocr?.picture || ocr.stage !== 'ready') return;
    const request = ++pictureRequest;
    set({
      ocrImageView: ocr.mode === 'card' ? get().ocrImageView : true,
      ocrPictureBusy: true,
      ocrPictureError: null,
      ocrPictureTranslation: null,
    });
    const current = () => request === pictureRequest && get().ocr?.id === ocr.id;
    try {
      const result = await invoke<PictureTranslation>('translate_ocr_picture', {
        id: ocr.id, texts: texts ?? null, targetLang: ocrTarget, provider,
        openaiConfig: provider === 'openai' ? openaiConfig : null,
      });
      if (current()) set({ ocrPictureTranslation: result });
    } catch (error) {
      if (current()) set({ ocrPictureError: String(error) });
    } finally {
      if (current()) set({ ocrPictureBusy: false });
    }
  },

  applyAutoOcr: (event) => {
    if (event.revision <= get().autoOcrRevision) return;
    if (event.active && event.id !== get().autoOcr?.id) get().clearLookup();
    set({ autoOcrRevision: event.revision, autoOcr: event.active || event.frame || event.error ? event : null });
  },

  startAutoOcr: async (intervalMs = 2000) => {
    const { provider, openaiConfig, ocrTarget } = get();
    set({ errorMessage: null });
    try {
      await invoke('begin_auto_ocr', { options: { provider, openai_config: provider === 'openai' ? openaiConfig : null,
        target_lang: ocrTarget, interval_ms: intervalMs } });
    } catch (error) { set({ errorMessage: String(error) }); }
  },

  processText: async (text, forceDensity, forceIntent) => {
    const clean = text.trim();
    if (!clean) return;

    const session = ++lookupSession;
    streamTokens.clear();
    forgetScrollTops();
    set((state) => {
      const history = [...state.history];
      if (history[history.length - 1] !== clean) {
        history.push(clean);
        if (history.length > 50) history.shift();
      }
      return {
        lookupStack: [],
        ocr: null,
        errorMessage: null,
        ankiStatus: 'idle',
        ankiMessage: null,
        history,
        historyIndex: -1,
        backTranslation: null,
      };
    });

    try {
      const intentRes = forceIntent
        ? { intent: forceIntent }
        : await invoke<IntentResult>('detect_intent', { text: clean });
      if (session !== lookupSession) return;
      const kind: LookupNode['kind'] = intentRes.intent === 'word' ? 'word' : 'sentence';
      const node: LookupNode = {
        id: nextNodeId(),
        kind,
        text: clean,
        density: forceDensity ?? (kind === 'word' ? 'dense' : 'compact'),
        intent: intentRes.intent,
        wordData: null,
        translation: '',
        status: 'loading',
      };

      // A fresh capture starts a new session.
      set({ lookupStack: [node] });

      if (kind === 'word') {
        const dictRes = await invoke<DictEntry | null>('lookup_dict', { word: clean });
        if (session !== lookupSession) return;
        get().patchNode(node.id, {
          wordData: dictRes,
          status: dictRes ? 'ready' : 'streaming',
        });
        if (dictRes) {
          void get().fetchWordExamples(clean, node.id);
        } else {
          void get().startStreamTranslation(clean, node.id);
        }
      } else {
        void get().startStreamTranslation(clean, node.id);
      }
    } catch (err: unknown) {
      if (session !== lookupSession) return;
      const msg = err instanceof Error ? err.message : String(err);
      set({ errorMessage: msg });
    }
  },

  pushLookup: async (rawWord, origin) => {
    const word = cleanWord(rawWord);
    if (!word) return;

    const node: LookupNode = {
      id: nextNodeId(),
      kind: 'word',
      text: word,
      density: 'dense',
      intent: 'word',
      wordData: null,
      translation: '',
      status: 'loading',
      originText: origin?.text,
      originWord: origin?.word,
    };

    set((state) => {
      const stack = state.lookupStack.slice();
      if (stack.length > 0) {
        // Remember which word was clicked so the parent stays highlighted.
        const parentIndex = stack.length - 1;
        stack[parentIndex] = { ...stack[parentIndex], highlightWord: word };
      }
      stack.push(node);
      return {
        lookupStack: stack,
        errorMessage: null,
        ankiStatus: 'idle',
        ankiMessage: null,
      };
    });

    try {
      const dictRes = await invoke<DictEntry | null>('lookup_dict', { word });
      if (!get().lookupStack.some((frame) => frame.id === node.id)) return;
      get().patchNode(node.id, {
        wordData: dictRes,
        status: dictRes ? 'ready' : 'streaming',
      });
      if (dictRes) {
        void get().fetchWordExamples(word, node.id);
      } else {
        void get().startStreamTranslation(word, node.id);
      }
    } catch (err: unknown) {
      if (!get().lookupStack.some((frame) => frame.id === node.id)) return;
      const msg = err instanceof Error ? err.message : String(err);
      get().patchNode(node.id, { status: 'error' });
      set({ errorMessage: msg });
    }
  },

  popLookup: () =>
    set((state) =>
      state.lookupStack.length > 1
        ? { lookupStack: state.lookupStack.slice(0, -1) }
        : {},
    ),

  popTo: (index) =>
    set((state) =>
      index >= 0 && index < state.lookupStack.length - 1
        ? { lookupStack: state.lookupStack.slice(0, index + 1) }
        : {},
    ),

  clearLookup: () => {
    lookupSession += 1;
    pictureRequest++;
    streamTokens.clear();
    forgetScrollTops();
    const isOriginal = get().ocrPictureDefaultView === 'original';
    set({
      lookupStack: [],
      ocr: null,
      ocrImageView: false,
      ocrPictureTranslation: null,
      ocrPictureBusy: false,
      ocrPictureError: null,
      ocrPictureOriginal: isOriginal,
      errorMessage: null,
      ankiStatus: 'idle',
      ankiMessage: null,
    });
  },

  setDensity: (density) =>
    set((state) => {
      if (state.lookupStack.length === 0) return {};
      const stack = state.lookupStack.slice();
      const top = stack[stack.length - 1];
      stack[stack.length - 1] = { ...top, density };
      return { lookupStack: stack };
    }),

  patchNode: (id, patch) =>
    set((state) => ({
      lookupStack: state.lookupStack.map((node) =>
        node.id === id ? { ...node, ...patch } : node,
      ),
    })),

  startStreamTranslation: async (text, nodeId) => {
    const { provider, openaiConfig } = get();
    const token = (streamTokens.get(nodeId) ?? 0) + 1;
    streamTokens.set(nodeId, token);
    const isCurrent = () => streamTokens.get(nodeId) === token
      && get().lookupStack.some((node) => node.id === nodeId);

    let accumulated = '';
    get().patchNode(nodeId, { status: 'streaming', translation: '' });

    const channel = new Channel<StreamChunk>();
    channel.onmessage = (chunk) => {
      if (!isCurrent()) return; // superseded by a newer stream for this frame

      if (chunk.error) {
        get().patchNode(nodeId, { status: 'error', translation: accumulated });
        set({ errorMessage: chunk.error });
        return;
      }

      if (chunk.text) {
        accumulated += chunk.text;
        get().patchNode(nodeId, { translation: accumulated });
      }

      if (chunk.is_done) {
        get().patchNode(nodeId, {
          status: accumulated.trim() ? 'ready' : 'notfound',
          translation: accumulated,
        });
      }
    };

    try {
      const target_lang = resolveTargetLanguage(text, get().targetLanguage);
      await invoke('translate_stream', {
        query: { text, target_lang },
        provider,
        openaiConfig: provider === 'openai' ? openaiConfig : null,
        onChunk: channel,
      });
    } catch (err: unknown) {
      if (isCurrent()) {
        get().patchNode(nodeId, { status: 'error', translation: accumulated });
        set({ errorMessage: String(err) });
      }
    }
  },

  fetchWordExamples: async (word, nodeId) => {
    const { provider, openaiConfig } = get();
    if (provider !== 'openai' || !openaiConfig.api_key) return;
    if (examplesInFlight >= MAX_EXAMPLE_REQUESTS) return;

    examplesInFlight += 1;
    try {
      const examples = await invoke<ExampleSentence[]>('generate_examples', {
        word,
        openaiConfig,
      });
      if (examples.length === 0) return;

      set((state) => ({
        lookupStack: state.lookupStack.map((node) => {
          if (node.id !== nodeId || !node.wordData) return node;
          return {
            ...node,
            wordData: {
              ...node.wordData,
              examples: [...(node.wordData.examples ?? []), ...examples],
            },
          };
        }),
      }));
    } catch (err) {
      console.error('generate_examples failed:', err);
    } finally {
      examplesInFlight -= 1;
    }
  },

  syncToAnki: async (contextSentence?: string) => {
    const { ankiConfig, lookupStack } = get();
    const node = lookupStack[lookupStack.length - 1];
    if (!node) return false;

    const word = node.wordData?.word || node.text;

    set({ ankiStatus: 'syncing', ankiMessage: 'Connecting to Anki...' });

    const phonetic = node.wordData?.phonetic || '';
    const meaning =
      node.wordData?.definitions
        .map((d) => `<b>${d.pos}</b> ${d.meanings.join('; ')}`)
        .join('<br>') ||
      node.translation ||
      '';
    const sentence = contextSentence || node.originText || node.wordData?.examples[0]?.en || '';

    const frontHtml = `
      <div style="font-size: 24px; font-weight: bold; color: #3b82f6;">${word}</div>
      ${phonetic ? `<div style="color: #64748b; font-style: italic;">${phonetic}</div>` : ''}
      ${sentence ? `<div style="margin-top: 10px; font-size: 14px; color: #475569;">"${sentence}"</div>` : ''}
    `.trim();

    const backHtml = `
      <div style="font-size: 16px; line-height: 1.6;">${meaning}</div>
    `.trim();

    try {
      await invoke<number>('anki_add_note', {
        note: {
          endpoint: ankiConfig.endpoint,
          deckName: ankiConfig.deckName,
          modelName: ankiConfig.modelName,
          front: frontHtml,
          back: backHtml,
          tags: ['TransPortal', 'AutoSynced'],
          allowDuplicate: false,
        },
      });

      set({ ankiStatus: 'success', ankiMessage: `Card added to ${ankiConfig.deckName}!` });
      setTimeout(() => set({ ankiStatus: 'idle', ankiMessage: null }), 3000);
      return true;
    } catch (err: unknown) {
      set({
        ankiStatus: 'error',
        ankiMessage: `Anki error: ${err instanceof Error ? err.message : String(err)}`,
      });
      return false;
    }
  },

  exportTsv: (contextSentence?: string) => {
    const { lookupStack } = get();
    const node = lookupStack[lookupStack.length - 1];
    if (!node) return;

    const word = node.wordData?.word || node.text;
    const phonetic = node.wordData?.phonetic || '';
    const meaning =
      node.wordData?.definitions
        .map((d) => `${d.pos} ${d.meanings.join('; ')}`)
        .join(' | ') ||
      node.translation ||
      '';
    const sentence = contextSentence || node.originText || node.wordData?.examples[0]?.en || '';

    const row = [word, phonetic, meaning, sentence]
      .map((field) => `"${field.replace(/"/g, '""')}"`)
      .join('\t');
    const blob = new Blob([`Word\tPhonetic\tDefinition\tContext\n${row}`], {
      type: 'text/tab-separated-values;charset=utf-8;',
    });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.setAttribute('download', `${word}_anki.tsv`);
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(url);
  },
}));
