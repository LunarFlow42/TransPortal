//! Keyboard gesture contract: passive observation of double Ctrl+C, the ink
//! translation key, the OS screenshot chord and Esc. Observers never consume
//! or re-inject the keys they watch.

use std::sync::mpsc::SyncSender;

#[derive(Debug)]
pub enum GestureEvent {
    /// Observe the OS-owned Win+Shift+S chord without consuming it.
    Snip {
        clipboard_revision: u32,
    },
    DoubleCopy {
        clipboard_revision: u32,
        window: usize,
    },
    InkTranslate {
        window: usize,
    },
    Cancel,
}

/// Owns the observer's lifetime. Dropping it stops the observation.
pub trait GestureObserver: Send + Sync {}

/// A single key that, pressed and released alone, translates the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InkKey {
    /// The `` ` `` / `~` key.
    Backquote,
    CapsLock,
    Control,
    Alt,
    Shift,
    /// F1–F12.
    Function(u8),
}

impl InkKey {
    /// Parse the settings value; unknown or empty values disable the key.
    pub fn parse(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().as_str() {
            "~" | "`" => Some(Self::Backquote),
            "capslock" | "caps_lock" => Some(Self::CapsLock),
            "control" | "ctrl" => Some(Self::Control),
            "alt" => Some(Self::Alt),
            "shift" => Some(Self::Shift),
            other => other
                .strip_prefix('f')
                .and_then(|number| number.parse::<u8>().ok())
                .filter(|number| (1..=12).contains(number))
                .map(Self::Function),
        }
    }
}

pub trait GestureSource: Send + Sync {
    /// Select the ink translation key (`None` disables it). Applies to the
    /// running observer as well as later ones.
    fn set_ink_key(key: Option<InkKey>);

    /// Start observing; events are delivered until the observer is dropped.
    fn observe(&self, events: SyncSender<GestureEvent>)
    -> Result<Box<dyn GestureObserver>, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ink_keys_parse_like_the_settings_values() {
        assert_eq!(InkKey::parse("~"), Some(InkKey::Backquote));
        assert_eq!(InkKey::parse("`"), Some(InkKey::Backquote));
        assert_eq!(InkKey::parse(" CapsLock "), Some(InkKey::CapsLock));
        assert_eq!(InkKey::parse("caps_lock"), Some(InkKey::CapsLock));
        assert_eq!(InkKey::parse("Control"), Some(InkKey::Control));
        assert_eq!(InkKey::parse("ctrl"), Some(InkKey::Control));
        assert_eq!(InkKey::parse("Alt"), Some(InkKey::Alt));
        assert_eq!(InkKey::parse("Shift"), Some(InkKey::Shift));
        assert_eq!(InkKey::parse("F1"), Some(InkKey::Function(1)));
        assert_eq!(InkKey::parse("f12"), Some(InkKey::Function(12)));
        for disabled in ["", "none", "f0", "f13", "f", "fx", "a"] {
            assert_eq!(InkKey::parse(disabled), None, "{disabled}");
        }
    }
}
