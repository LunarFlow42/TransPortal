import { describe, expect, it } from 'vitest';
import { detectLanguage, languageTagOf, resolveTargetLanguage } from './language';

describe('detectLanguage', () => {
  it('detects English and Latin text', () => {
    expect(detectLanguage('Hello world')).toBe('en');
    expect(detectLanguage('Bonjour le monde')).toBe('en');
    expect(detectLanguage('12345!@#$%')).toBe('en');
  });

  it('detects Chinese text', () => {
    expect(detectLanguage('你好世界')).toBe('zh');
    expect(detectLanguage('今天天气真不错')).toBe('zh');
    expect(detectLanguage('繁體中文測試')).toBe('zh');
  });

  it('detects Japanese text with kana', () => {
    expect(detectLanguage('こんにちは')).toBe('ja');
    expect(detectLanguage('これは日本語のテストです')).toBe('ja');
    expect(detectLanguage('カタカナ')).toBe('ja');
  });

  it('detects Korean text with hangul', () => {
    expect(detectLanguage('안녕하세요')).toBe('ko');
    expect(detectLanguage('한국어 테스트')).toBe('ko');
  });
});

describe('resolveTargetLanguage', () => {
  it('defaults to English when source is Chinese and preferred is zh-CN', () => {
    expect(resolveTargetLanguage('你好世界', 'zh-CN')).toBe('en');
    expect(resolveTargetLanguage('这道题怎么解？', 'zh-CN')).toBe('en');
  });

  it('translates to zh-CN when source is English/Japanese/Korean and preferred is zh-CN', () => {
    expect(resolveTargetLanguage('Hello world', 'zh-CN')).toBe('zh-CN');
    expect(resolveTargetLanguage('こんにちは', 'zh-CN')).toBe('zh-CN');
    expect(resolveTargetLanguage('안녕하세요', 'zh-CN')).toBe('zh-CN');
  });

  it('inverts to zh-CN when source is English and preferred is en', () => {
    expect(resolveTargetLanguage('Hello world', 'en')).toBe('zh-CN');
    expect(resolveTargetLanguage('你好世界', 'en')).toBe('en');
    expect(resolveTargetLanguage('こんにちは', 'en')).toBe('en');
  });

  it('inverts to zh-CN when source is Japanese and preferred is ja', () => {
    expect(resolveTargetLanguage('こんにちは', 'ja')).toBe('zh-CN');
    expect(resolveTargetLanguage('你好世界', 'ja')).toBe('ja');
    expect(resolveTargetLanguage('Hello world', 'ja')).toBe('ja');
  });

  it('inverts to zh-CN when source is Korean and preferred is ko', () => {
    expect(resolveTargetLanguage('안녕하세요', 'ko')).toBe('zh-CN');
    expect(resolveTargetLanguage('你好世界', 'ko')).toBe('ko');
  });

  it('defaults preferred target to zh-CN if empty', () => {
    expect(resolveTargetLanguage('你好')).toBe('en');
    expect(resolveTargetLanguage('hello')).toBe('zh-CN');
  });
});

describe('languageTagOf', () => {
  it('returns the tag of the text language for back-translation', () => {
    expect(languageTagOf('今天天气真不错')).toBe('zh-CN');
    expect(languageTagOf('Hello world')).toBe('en');
    expect(languageTagOf('こんにちは')).toBe('ja');
    expect(languageTagOf('안녕하세요')).toBe('ko');
  });
});
