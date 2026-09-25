export type DetectedLang = 'zh' | 'ja' | 'ko' | 'en';

/**
 * Ultra-lightweight Unicode range language detector (< 0.1ms).
 *
 * Distinguishes Japanese (Kana), Korean (Hangul), Chinese (CJK Ideographs without Kana/Hangul),
 * and Latin/European languages (ASCII / Latin / other scripts).
 */
export function detectLanguage(text: string): DetectedLang {
  let hasCjk = false;
  for (const char of text) {
    const code = char.codePointAt(0) ?? 0;
    // Japanese Hiragana & Katakana
    if ((code >= 0x3040 && code <= 0x309f) || (code >= 0x30a0 && code <= 0x30ff)) {
      return 'ja';
    }
    // Korean Hangul Syllables & Jamo
    if (
      (code >= 0xac00 && code <= 0xd7af) ||
      (code >= 0x1100 && code <= 0x11ff) ||
      (code >= 0x3130 && code <= 0x318f)
    ) {
      return 'ko';
    }
    // CJK Unified Ideographs & Extension A
    if ((code >= 0x4e00 && code <= 0x9fff) || (code >= 0x3400 && code <= 0x4dbf)) {
      hasCjk = true;
    }
  }
  return hasCjk ? 'zh' : 'en';
}

/**
 * Resolve the optimal target language based on source text language and user's preferred target.
 *
 * Implements intelligent mutual translation (双向自适应互译):
 * - If preferredTarget is Chinese (zh-CN / zh-TW):
 *   - English / Japanese / Korean / other foreign -> translates to Chinese.
 *   - Chinese -> automatically translates to English ('en')!
 * - If preferredTarget is English (en):
 *   - Chinese / Japanese / Korean / other foreign -> translates to English.
 *   - English -> automatically translates to Chinese ('zh-CN')!
 * - If preferredTarget is another language (e.g. ja / ko / fr / de):
 *   - Source matches target -> flips to Chinese ('zh-CN')!
 *   - Source does not match target -> translates to preferredTarget!
 */
export function resolveTargetLanguage(text: string, preferredTarget = 'zh-CN'): string {
  const target = (preferredTarget || 'zh-CN').trim();
  const detected = detectLanguage(text);

  const targetIsZh = /^zh([-_].*)?$/i.test(target);
  const targetIsEn = /^en([-_].*)?$/i.test(target);
  const targetIsJa = /^(ja|jp)([-_].*)?$/i.test(target);
  const targetIsKo = /^(ko|kr)([-_].*)?$/i.test(target);

  switch (detected) {
    case 'zh':
      return targetIsZh ? 'en' : target;
    case 'en':
      return targetIsEn ? 'zh-CN' : target;
    case 'ja':
      return targetIsJa ? 'zh-CN' : target;
    case 'ko':
      return targetIsKo ? 'zh-CN' : target;
    default:
      return target;
  }
}

const LANGUAGE_TAGS: Record<DetectedLang, string> = { zh: 'zh-CN', ja: 'ja', ko: 'ko', en: 'en' };

/** Language tag of the text's own language, e.g. the target of a back-translation. */
export function languageTagOf(text: string): string {
  return LANGUAGE_TAGS[detectLanguage(text)];
}
