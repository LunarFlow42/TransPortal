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
  english_definitions?: PosDefinition[];
  exchange?: string;
  tags: string[];
  examples: ExampleSentence[];
  source?: string;
}

export type DictSourceKind = 'online' | 'local';

export interface DictSourceInfo {
  id: string;
  label: string;
  description: string;
  kind: DictSourceKind;
  enabled: boolean;
  available: boolean;
  last_ok?: boolean | null;
  last_latency_ms?: number | null;
}

export interface DictStatus {
  installed: boolean;
  entry_count: number;
  local_path: string;
}

export interface DictProgress {
  stage: 'downloading' | 'importing' | 'done';
  downloaded: number;
  total?: number | null;
  imported?: number | null;
}

/** Visual density of a lookup card. */
export type Density = 'compact' | 'dense';

/** Lifecycle of a single lookup stack frame. */
export type LookupStatus =
  | 'loading'
  | 'streaming'
  | 'ready'
  | 'notfound'
  | 'error';

/**
 * One frame in the drill-down lookup stack.
 *
 * The stack is a pure LIFO navigation history: clicking a word pushes a frame,
 * back pops it, and every frame keeps enough of a snapshot to be restored
 * instantly (content, translation, scroll offset, and origin context).
 */
export interface LookupNode {
  id: number;
  kind: 'sentence' | 'word';
  text: string;
  density: Density;
  intent: TextIntent;
  wordData: DictEntry | null;
  translation: string;
  status: LookupStatus;
  /** Sentence/example this frame was opened from (also feeds Anki context). */
  originText?: string;
  /** Word clicked in `originText`; the parent frame highlights it. */
  originWord?: string;
  /** Word that was clicked *from* this frame (kept after the child is popped). */
  highlightWord?: string;
}

export type CaptureMethod = 'ui_automation' | 'clipboard' | 'ocr';

export interface SelectionBounds {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface CaptureOutcome {
  text: string;
  method: CaptureMethod;
  error?: string | null;
  replace_token?: number | null;
  bounds?: SelectionBounds | null;
}

export type OcrMode = 'text' | 'picture' | 'card' | 'silent' | 'translate';

export type ShortcutActionId =
  | 'selection'
  | 'ocr_card'
  | 'ocr_picture'
  | 'ocr_text'
  | 'double_copy'
  | 'replace_selection'
  | 'silent_ocr'
  | 'input_translate'
  | 'ocr_translate';

export interface ShortcutStatus {
  id: ShortcutActionId;
  label: string;
  requested: string;
  active: string | null;
  error: string | null;
  system: boolean;
}

export interface ShortcutReport {
  revision: number;
  ready: boolean;
  shortcuts: ShortcutStatus[];
}

export interface IntegrityReport {
  status: 'verified' | 'development' | 'unsupported' | 'missing_manifest' | 'invalid_manifest' | 'mismatch' | 'io_error';
  version: string;
  expected_sha256: string | null;
  actual_sha256: string | null;
  error: string | null;
}

export type OcrEngineId = 'windows' | 'onnx';
export interface OcrConfig {
  engine: OcrEngineId;
  language: string | null;
}
export type OcrStage = 'capturing' | 'recognizing' | 'ready' | 'cancelled' | 'error';

export interface OcrEvent {
  id: number;
  mode: OcrMode;
  stage: OcrStage;
  text: string;
  language: string | null;
  engine: string;
  image_path?: string | null;
  picture?: OcrPicture | null;
  elapsed_ms: number | null;
  copied: boolean;
  copy_error: string | null;
  error: string | null;
}

export interface OcrBlock {
  id: number;
  text: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface OcrPicture {
  width: number;
  height: number;
  data_url: string;
  blocks: OcrBlock[];
}

export interface PictureTranslation {
  blocks: { id: number; text: string; error: string | null }[];
  target_lang: string;
}

export interface ScreenRect { x: number; y: number; width: number; height: number }
export type RegionPurpose = 'text' | 'translate' | 'picture' | 'card' | 'silent' | 'monitor';
export interface RegionSelection { id: number; purpose: RegionPurpose; bounds: ScreenRect; image_data_url?: string | null }
export interface MonitorUpdate {
  id: number;
  revision: number;
  active: boolean;
  selecting: boolean;
  paused: boolean;
  stage: 'selecting' | 'watching' | 'recognizing' | 'translating' | 'paused' | 'error' | 'stopped';
  interval_ms: number;
  checks: number;
  target_lang: string;
  region: ScreenRect | null;
  error: string | null;
  frame: { number: number; picture: OcrPicture; translation: PictureTranslation | null; engine: string; elapsed_ms: number } | null;
}

export interface OcrEngineInfo {
    id: OcrEngineId;
    label: string;
    languages: { tag: string; label: string }[];
    available: boolean;
    error: string | null;
}

export interface OcrInstallProgress {
  stage: 'downloading' | 'verifying' | 'ready';
  file: string;
  downloaded: number;
  total: number;
}

export interface OcrStatus {
  supported: boolean;
  engine: OcrEngineInfo;
  engines: OcrEngineInfo[];
  config: OcrConfig;
  onnx: {
    installed: boolean;
    installing: boolean;
    path: string | null;
    download_bytes: number;
    installed_bytes: number;
    error: string | null;
  };
  observing_shortcut: boolean;
  shortcut_error: string | null;
  translate_shortcut_error: string | null;
}

export type ProviderType =
  | 'google_free'
  | 'youdao_free'
  | 'tencent_free'
  | 'openai';

export type ThemeMode = 'system' | 'light' | 'dark';

export interface OpenAiConfig {
  api_key: string;
  base_url?: string;
  model?: string;
  temperature?: number;
  system_prompt?: string;
  prompt_template?: 'direct' | 'academic' | 'code' | 'casual' | string;
}

export interface AnkiConfig {
  endpoint: string;
  deckName: string;
  modelName: string;
}

export type AnkiStatus = 'idle' | 'syncing' | 'success' | 'error';

export interface PersistedSettings {
  provider: ProviderType;
  openaiConfig: OpenAiConfig;
  ankiConfig: AnkiConfig;
  hotkey: string;
  ocrCardHotkey?: string;
  ocrPictureHotkey?: string;
  ocrTextHotkey?: string;
  replaceSelectionHotkey?: string;
  silentOcrHotkey?: string;
  inputTranslateHotkey?: string;
  inkTranslateKey?: string;
  doubleCopyEnabled: boolean;
  clipboardMonitorEnabled?: boolean;
  autoPronounce: boolean;
  ocrEngine: OcrEngineId;
  /** `auto` uses Windows profile languages, otherwise an installed BCP-47 tag. */
  ocrLanguage: string;
  autostart: boolean;
  theme: ThemeMode;
  /** Overlay glass opacity, 0.25–1.0. */
  overlayOpacity: number;
  fontSize?: number;
  showCrosshairs?: boolean;
  aiPromptTemplate?: 'direct' | 'academic' | 'code' | 'casual';
  autoHideOnBlur: boolean;
  targetLanguage: string;
  ocrPictureDefaultView?: 'translated' | 'original';
  acrylicEffect?: boolean;
  smoothTransitions?: boolean;
  dictSources: { id: string; enabled: boolean }[];
}

export interface MemoryReport {
  total_working_set_bytes: number;
  total_private_bytes: number;
  main_working_set_bytes: number;
  process_count: number;
}

export interface TrimResult {
  before_bytes: number;
  after_bytes: number;
  trimmed_bytes: number;
  process_count: number;
}
