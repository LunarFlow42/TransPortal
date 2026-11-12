import { describe, expect, it } from 'vitest';
import { exchangeItems, phoneticParts } from './dictionaryDisplay';

describe('enriched dictionary display', () => {
  it('splits UK and US phonetics without dropping either accent', () => {
    expect(phoneticParts('英 /həˈləʊ/ 美 /həˈloʊ/')).toEqual([
      { accent: 'uk', value: '/həˈləʊ/' }, { accent: 'us', value: '/həˈloʊ/' },
    ]);
    expect(phoneticParts('美 /us/ 英 /uk/')).toEqual([
      { accent: 'uk', value: '/uk/' }, { accent: 'us', value: '/us/' },
    ]);
    expect(phoneticParts('/old-style/')).toBeNull();
    expect(phoneticParts('英 /uk/')).toEqual([{ accent: 'uk', value: '/uk/' }]);
  });

  it('shows canonical word forms and lemma but not transformation codes as words', () => {
    expect(exchangeItems('p:went/d:gone/i:going/3:goes/s:words/0:go/1:s')).toEqual([
      { code: 'p', value: 'went' }, { code: 'd', value: 'gone' }, { code: 'i', value: 'going' },
      { code: '3', value: 'goes' }, { code: 's', value: 'words' }, { code: '0', value: 'go' },
    ]);
  });

  it('does not create empty or duplicate controls from malformed exchanges', () => {
    expect(exchangeItems('s:words/s:words/p:/unknown:x/malformed')).toEqual([{ code: 's', value: 'words' }]);
  });
});
