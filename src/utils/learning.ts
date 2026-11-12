import type { LookupNode, PosDefinition } from '../types';
import type { DefinitionKind, LearningContext, LearningDraft } from '../types/learning';

export interface LearningSense {
  id: string;
  pos: string;
  meaning: string;
  kind: DefinitionKind;
  source: string;
  lemma: string;
}

export interface LearningContextOption {
  id: string;
  context: LearningContext | null;
}

/** Only ready English word cards can be saved. No extra capture or network I/O. */
export function learningSenses(node: LookupNode): LearningSense[] {
  const word = (node.wordData?.word || node.text).trim();
  if (node.kind !== 'word' || node.status !== 'ready'
    || word.length > 128 || !/[a-z]/i.test(word) || !/^[a-z0-9 .’'-]+$/i.test(word)) return [];

  const senses: LearningSense[] = [];
  const seen = new Set<string>();
  const addDefinitions = (definitions: PosDefinition[], source: string, lemma: string) => {
    for (const definition of definitions) {
      for (const value of definition.meanings) {
        const meaning = value.trim();
        const pos = definition.pos.trim();
        const id = JSON.stringify([pos, meaning]);
        if (!meaning || seen.has(id)) continue;
        seen.add(id);
        senses.push({ id, pos, meaning, kind: 'dictionary', source, lemma });
      }
    }
  };
  addDefinitions(node.wordData?.definitions ?? [], node.wordData?.source ?? '', word);
  const local = node.wordData?.local_supplement;
  if (local) addDefinitions(local.definitions, 'local_eccedict', local.word);
  // A translation fallback must be clearly labeled and explicitly confirmed.
  if (senses.length === 0 && node.translation.trim()) {
    const meaning = node.translation.trim();
    senses.push({ id: JSON.stringify(['translation', meaning]), pos: '', meaning, kind: 'translation', source: '', lemma: word });
  }
  return senses;
}

/** Reuse examples already displayed by the card; never try to read another app. */
export function learningContexts(node: LookupNode): LearningContextOption[] {
  const options: LearningContextOption[] = [];
  const seen = new Set<string>();
  for (const [index, example] of (node.wordData?.examples ?? []).entries()) {
    const text = example.en.trim();
    if (!text || seen.has(text)) continue;
    seen.add(text);
    options.push({
      id: `example:${index}`,
      context: {
        text,
        translation: example.zh.trim() || null,
        source: example.source ?? 'dictionary',
      },
    });
  }
  for (const [index, example] of (node.wordData?.local_supplement?.examples ?? []).entries()) {
    const text = example.en.trim();
    if (!text || seen.has(text)) continue;
    seen.add(text);
    options.push({ id: `local:${index}`, context: { text, translation: example.zh.trim() || null, source: 'dictionary' } });
  }
  const origin = node.originText?.trim();
  // Clicking a word-form pill only passes the headword, not a real sentence.
  if (origin && origin.split(/\s+/).length > 1 && !seen.has(origin)) {
    options.push({ id: 'lookup', context: { text: origin, translation: null, source: 'lookup' } });
  }
  options.push({ id: 'none', context: null });
  return options;
}

export function learningDraft(
  node: LookupNode,
  senseId: string,
  contextId: string,
  translationSource: string,
): LearningDraft | null {
  const sense = learningSenses(node).find((sense) => sense.id === senseId);
  const example = learningContexts(node).find((option) => option.id === contextId);
  if (!sense || !example) return null;
  return {
    language: 'en',
    lemma: sense.lemma,
    surface: (node.originWord || node.text).trim(),
    phonetic: (sense.source === 'local_eccedict' && node.wordData?.local_supplement
      ? node.wordData.local_supplement.phonetic
      : node.wordData?.phonetic)?.trim() || null,
    pos: sense.pos,
    meaning: sense.meaning,
    definition_kind: sense.kind,
    definition_source: sense.kind === 'dictionary'
      ? sense.source
      : node.translationProvider ?? translationSource,
    context: example.context,
  };
}
