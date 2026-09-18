import { describe, expect, it } from 'vitest';
import { wrapPictureText } from './picture';

describe('translation text fitting', () => {
  const measure = (text: string) => Array.from(text).length;
  it('wraps long words, Chinese and supplementary characters without losing content', () => {
    const text = 'A longwordwithoutspaces 中文翻译测试😀𠮷';
    const lines = wrapPictureText(text, 6, measure);
    expect(lines.every((line) => measure(line) <= 6)).toBe(true);
    expect(lines.join('').replace(/\s/g, '')).toBe(text.replace(/\s/g, ''));
  });
  it('retains intentional paragraph breaks and keeps words together when they fit', () => {
    expect(wrapPictureText('Hello world\r\n\r\nNext line', 8, measure)).toEqual(['Hello', 'world', '', 'Next', 'line']);
  });
});
