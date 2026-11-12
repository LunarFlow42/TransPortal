import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import { DictionaryMetadata, LocalDictionaryDetails } from './DictionaryMetadata';
import type { DictEntry } from '../types';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock('./InteractiveSentence', () => ({
  InteractiveSentence: ({ sentence }: { sentence: string }) => React.createElement('p', null, sentence),
}));

const entry: DictEntry = {
  word: 'hello', source: 'youdao', definitions: [{ pos: 'n.', meanings: ['在线问候'] }],
  tags: ['CET4'], examples: [{ en: 'Hello, friend.', zh: '你好，朋友。' }],
  collins: 3, oxford: true, bnc: 100, frq: 200,
  local_supplement: {
    word: 'hello', fields: ['phonetic', 'tags', 'definitions', 'examples'],
    definitions: [{ pos: 'n.', meanings: ['本地问候'] }],
    english_definitions: [{ pos: 'n.', meanings: ['a greeting'] }],
    examples: [{ en: 'A local example.', zh: '本地例句。' }],
  },
};

describe('online/local dictionary attribution', () => {
  it('shows the online primary source and separately identifies local additions', () => {
    const html = renderToStaticMarkup(React.createElement(DictionaryMetadata, { entry }));
    expect(html).toContain('card.sourceYoudao');
    expect(html).toContain('card.localSupplementBadge');
    expect(html).toContain('card.collinsStars');
    expect(html).toContain('card.oxfordCore');
    expect(html).toContain('card.bncRank');
    expect(html).toContain('card.frqRank');
  });

  it('keeps local definitions collapsed and distinct from the online answer', () => {
    const html = renderToStaticMarkup(React.createElement(LocalDictionaryDetails, { entry }));
    expect(html).toContain('<details');
    expect(html).not.toContain('<details open');
    expect(html).toContain('本地问候');
    expect(html).toContain('a greeting');
    expect(html).toContain('A local example.');
    expect(html).not.toContain('在线问候');
    expect(html).not.toContain('Hello, friend.');
  });

  it('does not claim enrichment or show missing metadata for old dictionaries', () => {
    const plain: DictEntry = { word: 'hello', source: 'local_eccedict', definitions: [], tags: [], examples: [] };
    const html = renderToStaticMarkup(React.createElement(DictionaryMetadata, { entry: plain }));
    expect(html).toContain('card.sourceLocal');
    expect(html).not.toContain('card.localSupplementBadge');
    expect(html).not.toContain('card.collinsStars');
    expect(renderToStaticMarkup(React.createElement(LocalDictionaryDetails, { entry: plain }))).toBe('');
  });
});
