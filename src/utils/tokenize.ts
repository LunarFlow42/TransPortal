export interface SentenceToken {
  text: string;
  /** ASCII word that can be looked up in the dictionary. */
  isWord: boolean;
}

const WORD_RE = /^[a-zA-Z0-9'-]+$/;
const SPLIT_RE = /([a-zA-Z0-9'-]+|[^\s\w]+|\s+)/g;

/**
 * Split a sentence into look-up-able ASCII words, punctuation and whitespace.
 *
 * CJK text is not word-segmented and ends up in the non-word runs, so
 * translated Chinese is never rendered as clickable tokens.
 */
export function tokenizeSentence(sentence: string): SentenceToken[] {
  const parts = sentence.match(SPLIT_RE) ?? [sentence];
  return parts.map((text) => ({ text, isWord: WORD_RE.test(text) }));
}
