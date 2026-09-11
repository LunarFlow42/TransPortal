export type TextIntent = 'word' | 'sentence';

export interface IntentResult {
  intent: TextIntent;
  word_count: number;
  char_count: number;
  clean_text: string;
  is_pure_ascii: boolean;
}

export interface TranslateQuery {
  text: string;
  source_lang?: string;
  target_lang?: string;
}

export interface TranslationDetail {
  pos: string;
  terms: string[];
}

export interface TranslateResult {
  original_text: string;
  translated_text: string;
  detected_source_lang?: string;
  target_lang: string;
  details?: TranslationDetail[];
  provider_name: string;
}

export interface StreamChunk {
  text: string;
  is_done: boolean;
  error?: string;
}

export interface PosDefinition {
  pos: string;
  meanings: string[];
}

export interface ExampleSentence {
  en: string;
  zh: string;
}

export interface DictEntry {
  word: string;
  phonetic?: string;
  audio_url?: string;
  definitions: PosDefinition[];
  exchange?: string;
  tags: string[];
  examples: ExampleSentence[];
}

export type AppMode = 'bubble' | 'card';

export type ProviderType = 'google_free' | 'openai';

export interface OpenAiConfig {
  api_key: string;
  base_url?: string;
  model?: string;
  temperature?: number;
  system_prompt?: string;
}

export interface AnkiConfig {
  endpoint: string;
  deckName: string;
  modelName: string;
}

export type AnkiStatus = 'idle' | 'syncing' | 'success' | 'error';
