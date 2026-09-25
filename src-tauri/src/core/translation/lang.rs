//! Language detection and intelligent mutual translation target resolution.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedLang {
    Chinese,
    Japanese,
    Korean,
    Other,
}

/// Fast Unicode range language detector (< 0.1ms, zero dependencies).
pub fn detect_language(text: &str) -> DetectedLang {
    let mut has_cjk = false;
    for c in text.chars() {
        match c as u32 {
            // Japanese Hiragana & Katakana
            0x3040..=0x30FF => return DetectedLang::Japanese,
            // Korean Hangul Syllables & Jamo
            0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F => return DetectedLang::Korean,
            // CJK Unified Ideographs & Extension A
            0x4E00..=0x9FFF | 0x3400..=0x4DBF => has_cjk = true,
            _ => {}
        }
    }
    if has_cjk {
        DetectedLang::Chinese
    } else {
        DetectedLang::Other
    }
}

/// Resolve the optimal target language based on source text language and user's preferred target.
///
/// Implements intelligent mutual translation (双向自适应互译):
/// - When preferred target is Chinese (zh-CN): Chinese -> translates to English ("en"); foreign -> translates to Chinese.
/// - When preferred target is English (en): English -> translates to Chinese ("zh-CN"); foreign -> translates to English.
/// - When preferred target is another language (e.g. ja/ko): Source matches target -> flips to Chinese; else translates to target.
pub fn resolve_target_lang(text: &str, preferred_target: Option<&str>) -> String {
    let target = preferred_target.unwrap_or("zh-CN").trim();
    let target_is_zh = target.eq_ignore_ascii_case("zh")
        || target.starts_with("zh-")
        || target.starts_with("zh_")
        || target.eq_ignore_ascii_case("zh-chs")
        || target.eq_ignore_ascii_case("zh-cht");
    let target_is_en =
        target.eq_ignore_ascii_case("en") || target.starts_with("en-") || target.starts_with("en_");
    let target_is_ja = target.eq_ignore_ascii_case("ja")
        || target.eq_ignore_ascii_case("jp")
        || target.starts_with("ja-");
    let target_is_ko = target.eq_ignore_ascii_case("ko")
        || target.eq_ignore_ascii_case("kr")
        || target.starts_with("ko-");

    match detect_language(text) {
        DetectedLang::Chinese => {
            if target_is_zh {
                "en".to_string()
            } else {
                target.to_string()
            }
        }
        DetectedLang::Other => {
            if target_is_en {
                "zh-CN".to_string()
            } else {
                target.to_string()
            }
        }
        DetectedLang::Japanese => {
            if target_is_ja {
                "zh-CN".to_string()
            } else {
                target.to_string()
            }
        }
        DetectedLang::Korean => {
            if target_is_ko {
                "zh-CN".to_string()
            } else {
                target.to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language("Hello world"), DetectedLang::Other);
        assert_eq!(detect_language("你好世界"), DetectedLang::Chinese);
        assert_eq!(detect_language("こんにちは世界"), DetectedLang::Japanese);
        assert_eq!(detect_language("안녕하세요"), DetectedLang::Korean);
    }

    #[test]
    fn test_resolve_target_lang() {
        assert_eq!(resolve_target_lang("你好世界", Some("zh-CN")), "en");
        assert_eq!(resolve_target_lang("Hello world", Some("zh-CN")), "zh-CN");
        assert_eq!(resolve_target_lang("Hello world", Some("en")), "zh-CN");
        assert_eq!(resolve_target_lang("你好世界", Some("en")), "en");
        assert_eq!(resolve_target_lang("こんにちは", Some("ja")), "zh-CN");
    }
}
