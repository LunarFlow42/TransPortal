import { describe, expect, it } from 'vitest';
import { learningContexts, learningDraft, learningSenses } from './learning';
import type { LookupNode } from '../types';

function wordNode(patch: Partial<LookupNode> = {}): LookupNode {
  return {
    id: 1,
    kind: 'word',
    text: 'banks',
    density: 'dense',
    intent: 'word',
    translation: '',
    status: 'ready',
    wordData: {
      word: 'bank',
      phonetic: '/bæŋk/',
      definitions: [{ pos: 'n.', meanings: ['银行', '河岸', ' 银行 ', ''] }],
      tags: [],
      examples: [
        { en: 'We sat on the bank of the river.', zh: '我们坐在河岸上。' },
        { en: 'She went to the bank.', zh: '她去了银行。', source: 'generated' },
      ],
      source: 'youdao',
    },
    ...patch,
  };
}

describe('explicit usage snapshots from existing word cards', () => {
  it('offers distinct senses, without saving every meaning at once', () => {
    const senses = learningSenses(wordNode());
    expect(senses.map((sense) => sense.meaning)).toEqual(['银行', '河岸']);
    const node = wordNode();
    const draft = learningDraft(node, senses[1].id, 'example:0', 'bing_free');
    expect(draft).toMatchObject({
      language: 'en', lemma: 'bank', surface: 'banks', meaning: '河岸',
      pos: 'n.', definition_kind: 'dictionary', definition_source: 'youdao',
      phonetic: '/bæŋk/',
      context: { text: 'We sat on the bank of the river.', translation: '我们坐在河岸上。', source: 'dictionary' },
    });
  });

  it('keeps dictionary and generated examples distinguishable', () => {
    const contexts = learningContexts(wordNode());
    expect(contexts[0].context?.source).toBe('dictionary');
    expect(contexts[1].context?.source).toBe('generated');
    expect(contexts[contexts.length - 1]).toEqual({ id: 'none', context: null });
  });

  it('reuses the existing lookup sentence without attempting native capture', () => {
    const node = wordNode({ originText: 'The bank approved the loan.', originWord: 'bank' });
    expect(learningContexts(node).find((option) => option.id === 'lookup')?.context).toEqual({
      text: 'The bank approved the loan.', translation: null, source: 'lookup',
    });
    const draft = learningDraft(node, learningSenses(node)[0].id, 'lookup', 'google_free');
    expect(draft?.surface).toBe('bank');
  });

  it('does not treat a word-form pill as an original sentence', () => {
    expect(learningContexts(wordNode({ originText: 'bank' })).some((option) => option.id === 'lookup')).toBe(false);
  });

  it('allows saving without examples even when examples already exist', () => {
    const node = wordNode();
    expect(learningDraft(node, learningSenses(node)[0].id, 'none', 'bing_free')?.context).toBeNull();
    node.wordData!.examples = [];
    expect(learningContexts(node)).toEqual([{ id: 'none', context: null }]);
  });

  it('deduplicates identical examples and does not expose empty examples', () => {
    const node = wordNode();
    node.wordData!.examples.push({ ...node.wordData!.examples[0] }, { en: ' ', zh: '' });
    expect(learningContexts(node)).toHaveLength(3);
  });

  it('uses a labeled translation fallback with the original provider snapshot', () => {
    const node = wordNode({ wordData: null, text: 'ephemeral', translation: '短暂的', translationProvider: 'openai' });
    const senses = learningSenses(node);
    expect(senses[0].kind).toBe('translation');
    expect(learningDraft(node, senses[0].id, 'none', 'google_free')).toMatchObject({
      meaning: '短暂的', definition_kind: 'translation', definition_source: 'openai',
    });
  });

  it('does not offer collection for unfinished, sentence, or non-English cards', () => {
    for (const status of ['loading', 'streaming', 'notfound', 'error'] as const) {
      expect(learningSenses(wordNode({ status }))).toEqual([]);
    }
    expect(learningSenses(wordNode({ kind: 'sentence' }))).toEqual([]);
    expect(learningSenses(wordNode({ text: '你好', wordData: null, translation: 'Hello' }))).toEqual([]);
  });

  it('refuses stale or invalid selections rather than silently choosing another sense', () => {
    const node = wordNode();
    expect(learningDraft(node, 'missing', 'none', 'bing_free')).toBeNull();
    expect(learningDraft(node, learningSenses(node)[0].id, 'example:99', 'bing_free')).toBeNull();
  });

  it('saves a local supplemental sense with its own source, without labeling it online', () => {
    const node = wordNode();
    node.wordData!.local_supplement = {
      word: 'bank', fields: ['definitions', 'examples'],
      definitions: [{ pos: 'v.', meanings: ['堆积'] }], english_definitions: [],
      examples: [{ en: 'They banked the earth.', zh: '他们堆起泥土。' }],
    };
    const sense = learningSenses(node).find((sense) => sense.meaning === '堆积')!;
    expect(learningDraft(node, sense.id, 'local:0', 'bing_free')).toMatchObject({
      lemma: 'bank', pos: 'v.', meaning: '堆积', definition_source: 'local_eccedict',
      context: { text: 'They banked the earth.', translation: '他们堆起泥土。', source: 'dictionary' },
    });
  });
});
