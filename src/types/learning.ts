export type DefinitionKind = 'dictionary' | 'translation';
export type ContextSource = 'lookup' | 'dictionary' | 'generated';

export interface LearningContext {
  text: string;
  translation: string | null;
  source: ContextSource;
}

/** An explicit snapshot of one selected usage, not a successful review. */
export interface LearningDraft {
  language: 'en';
  lemma: string;
  surface: string;
  phonetic: string | null;
  pos: string;
  meaning: string;
  definition_kind: DefinitionKind;
  definition_source: string;
  context: LearningContext | null;
}

export interface LearningEntry {
  id: string;
  usage: LearningDraft;
  created_at: number;
  updated_at: number;
}

export interface EnrollResult {
  entry: LearningEntry;
  created: boolean;
}

export interface LearningPage {
  entries: LearningEntry[];
  total: number;
}

export type LearningExportFormat = 'json' | 'tsv';

export interface LearningExportResult {
  entry_count: number;
}
