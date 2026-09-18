import { describe, expect, it } from 'vitest';
import { tokenizeSentence } from './tokenize';

describe('tokenizeSentence', () => {
  it('marks ASCII words as look-up-able', () => {
    const tokens = tokenizeSentence('The quick brown fox.');
    expect(tokens.filter((t) => t.isWord).map((t) => t.text)).toEqual([
      'The',
      'quick',
      'brown',
      'fox',
    ]);
  });

  it('keeps punctuation and whitespace as non-word runs', () => {
    const tokens = tokenizeSentence('Hello, world!');
    expect(tokens.map((t) => t.text).join('')).toBe('Hello, world!');
    expect(tokens.find((t) => t.text === ',')?.isWord).toBe(false);
    expect(tokens.find((t) => t.text === ' ')?.isWord).toBe(false);
  });

  it('never marks CJK runs as words', () => {
    const tokens = tokenizeSentence('认知负荷是工作记忆中的心理努力。');
    expect(tokens.every((t) => !t.isWord)).toBe(true);
    expect(tokens.map((t) => t.text).join('')).toBe('认知负荷是工作记忆中的心理努力。');
  });

  it('reassembles the original sentence exactly', () => {
    const sentence = "It's a well-known fact: 2+2=4.";
    expect(tokenizeSentence(sentence).map((t) => t.text).join('')).toBe(sentence);
  });
});
