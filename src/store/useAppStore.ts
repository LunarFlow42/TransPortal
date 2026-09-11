import { create } from 'zustand';
import { invoke, Channel } from '@tauri-apps/api/core';
import type {
  AppMode,
  DictEntry,
  IntentResult,
  OpenAiConfig,
  ProviderType,
  StreamChunk,
  TextIntent,
  AnkiConfig,
  AnkiStatus,
} from '../types';

interface AppState {
  // UI Display & Navigation
  mode: AppMode;
  queryText: string;
  intent: TextIntent;
  wordData: DictEntry | null;
  translation: string;
  streamingText: string;
  isLoading: boolean;
  isStreaming: boolean;
  errorMessage: string | null;

  // History Navigation Stack (Recursive Drill-down)
  history: string[];
  historyIndex: number;

  // Provider & Config
  provider: ProviderType;
  openaiConfig: OpenAiConfig;
  ankiConfig: AnkiConfig;
  ankiStatus: AnkiStatus;
  ankiMessage: string | null;

  // Actions
  setMode: (mode: AppMode) => void;
  setQueryText: (text: string) => void;
  setProvider: (provider: ProviderType) => void;
  setOpenaiConfig: (config: Partial<OpenAiConfig>) => void;
  setAnkiConfig: (config: Partial<AnkiConfig>) => void;
  
  // High-level Workflows
  processText: (text: string, forceMode?: AppMode) => Promise<void>;
  drillDown: (word: string) => Promise<void>;
  goBack: () => Promise<void>;
  goForward: () => Promise<void>;
  startStreamTranslation: (text: string) => Promise<void>;
  syncToAnki: (contextSentence?: string) => Promise<boolean>;
  exportTsv: (contextSentence?: string) => void;
}

export const useAppStore = create<AppState>((set, get) => ({
  mode: 'bubble',
  queryText: '',
  intent: 'sentence',
  wordData: null,
  translation: '',
  streamingText: '',
  isLoading: false,
  isStreaming: false,
  errorMessage: null,

  history: [],
  historyIndex: -1,

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

  setMode: (mode) => set({ mode }),
  setQueryText: (queryText) => set({ queryText }),
  setProvider: (provider) => set({ provider }),
  setOpenaiConfig: (config) =>
    set((state) => ({ openaiConfig: { ...state.openaiConfig, ...config } })),
  setAnkiConfig: (config) =>
    set((state) => ({ ankiConfig: { ...state.ankiConfig, ...config } })),

  processText: async (text: string, forceMode?: AppMode) => {
    const clean = text.trim();
    if (!clean) return;

    set({
      queryText: clean,
      isLoading: true,
      errorMessage: null,
      streamingText: '',
      translation: '',
      ankiStatus: 'idle',
      ankiMessage: null,
    });

    try {
      // 1. Detect text granularity intent
      const intentRes = await invoke<IntentResult>('detect_intent', { text: clean });
      const intent = intentRes.intent;
      const targetMode = forceMode ?? (intent === 'word' ? 'card' : 'bubble');

      set({ intent, mode: targetMode });

      if (targetMode === 'card') {
        // Word Card flow: local-first lookup < 5ms
        const dictRes = await invoke<DictEntry | null>('lookup_dict', { word: clean });
        
        // Update history stack
        const { history, historyIndex } = get();
        const nextHistory = [...history.slice(0, historyIndex + 1), clean];

        set({
          wordData: dictRes,
          history: nextHistory,
          historyIndex: nextHistory.length - 1,
          isLoading: false,
        });

        // Async LLM enhancement in background
        get().startStreamTranslation(clean);
      } else {
        // Sentence Ghost Bubble flow: stream translation immediately
        set({ wordData: null, isLoading: false });
        await get().startStreamTranslation(clean);
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      set({ isLoading: false, errorMessage: msg });
    }
  },

  drillDown: async (rawWord: string) => {
    const word = rawWord.replace(/^[^\w\s]+|[^\w\s]+$/g, '').trim();
    if (!word) return;

    set({
      queryText: word,
      intent: 'word',
      mode: 'card',
      isLoading: true,
      errorMessage: null,
      streamingText: '',
      ankiStatus: 'idle',
      ankiMessage: null,
    });

    try {
      const dictRes = await invoke<DictEntry | null>('lookup_dict', { word });
      const { history, historyIndex } = get();
      const nextHistory = [...history.slice(0, historyIndex + 1), word];

      set({
        wordData: dictRes,
        history: nextHistory,
        historyIndex: nextHistory.length - 1,
        isLoading: false,
      });

      // Background stream enhancement for the new word
      get().startStreamTranslation(word);
    } catch (err: unknown) {
      set({ isLoading: false, errorMessage: String(err) });
    }
  },

  goBack: async () => {
    const { history, historyIndex } = get();
    if (historyIndex <= 0) return;

    const prevIndex = historyIndex - 1;
    const word = history[prevIndex];

    set({
      historyIndex: prevIndex,
      queryText: word,
      isLoading: true,
      streamingText: '',
      ankiStatus: 'idle',
    });

    try {
      const dictRes = await invoke<DictEntry | null>('lookup_dict', { word });
      set({ wordData: dictRes, isLoading: false });
      get().startStreamTranslation(word);
    } catch (err: unknown) {
      set({ isLoading: false, errorMessage: String(err) });
    }
  },

  goForward: async () => {
    const { history, historyIndex } = get();
    if (historyIndex >= history.length - 1) return;

    const nextIndex = historyIndex + 1;
    const word = history[nextIndex];

    set({
      historyIndex: nextIndex,
      queryText: word,
      isLoading: true,
      streamingText: '',
      ankiStatus: 'idle',
    });

    try {
      const dictRes = await invoke<DictEntry | null>('lookup_dict', { word });
      set({ wordData: dictRes, isLoading: false });
      get().startStreamTranslation(word);
    } catch (err: unknown) {
      set({ isLoading: false, errorMessage: String(err) });
    }
  },

  startStreamTranslation: async (text: string) => {
    const { provider, openaiConfig } = get();
    set({ isStreaming: true, streamingText: '', errorMessage: null });

    const channel = new Channel<StreamChunk>();

    channel.onmessage = (chunk) => {
      if (chunk.error) {
        set({ errorMessage: chunk.error, isStreaming: false });
        return;
      }

      if (chunk.text) {
        set((state) => ({ streamingText: state.streamingText + chunk.text }));
      }

      if (chunk.is_done) {
        set((state) => ({
          isStreaming: false,
          translation: state.streamingText,
        }));
      }
    };

    try {
      await invoke('translate_stream', {
        query: { text, target_lang: 'zh-CN' },
        provider,
        openaiConfig: provider === 'openai' ? openaiConfig : null,
        onChunk: channel,
      });
    } catch (err: unknown) {
      set({ isStreaming: false, errorMessage: String(err) });
    }
  },

  syncToAnki: async (contextSentence?: string) => {
    const { wordData, queryText, ankiConfig, translation, streamingText } = get();
    const word = wordData?.word || queryText;
    if (!word) return false;

    set({ ankiStatus: 'syncing', ankiMessage: 'Connecting to Anki...' });

    const phonetic = wordData?.phonetic || '';
    const meaning =
      wordData?.definitions.map((d) => `<b>${d.pos}</b> ${d.meanings.join('; ')}`).join('<br>') ||
      translation ||
      streamingText;
    const sentence = contextSentence || wordData?.examples[0]?.en || '';

    const frontHtml = `
      <div style="font-size: 24px; font-weight: bold; color: #3b82f6;">${word}</div>
      ${phonetic ? `<div style="color: #64748b; font-style: italic;">${phonetic}</div>` : ''}
      ${sentence ? `<div style="margin-top: 10px; font-size: 14px; color: #475569;">"${sentence}"</div>` : ''}
    `.trim();

    const backHtml = `
      <div style="font-size: 16px; line-height: 1.6;">${meaning}</div>
    `.trim();

    const payload = {
      action: 'addNote',
      version: 6,
      params: {
        note: {
          deckName: ankiConfig.deckName,
          modelName: ankiConfig.modelName,
          fields: {
            Front: frontHtml,
            Back: backHtml,
          },
          options: {
            allowDuplicate: false,
          },
          tags: ['TransPortal', 'AutoSynced'],
        },
      },
    };

    try {
      const response = await fetch(ankiConfig.endpoint, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });

      const resJson = await response.json();
      if (resJson.error) {
        set({ ankiStatus: 'error', ankiMessage: `Anki Error: ${resJson.error}` });
        return false;
      }

      set({ ankiStatus: 'success', ankiMessage: `Card added to ${ankiConfig.deckName}!` });
      setTimeout(() => set({ ankiStatus: 'idle', ankiMessage: null }), 3000);
      return true;
    } catch (err: unknown) {
      set({
        ankiStatus: 'error',
        ankiMessage: 'Cannot reach AnkiConnect (ensure Anki is running on port 8765)',
      });
      return false;
    }
  },

  exportTsv: (contextSentence?: string) => {
    const { wordData, queryText, translation, streamingText } = get();
    const word = wordData?.word || queryText;
    if (!word) return;

    const phonetic = wordData?.phonetic || '';
    const meaning =
      wordData?.definitions.map((d) => `${d.pos} ${d.meanings.join('; ')}`).join(' | ') ||
      translation ||
      streamingText;
    const sentence = contextSentence || wordData?.examples[0]?.en || '';

    const row = [word, phonetic, meaning, sentence].map((f) => `"${f.replace(/"/g, '""')}"`).join('\t');
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
