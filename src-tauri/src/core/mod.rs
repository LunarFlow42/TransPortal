//! Cross-platform application logic. Operating-system specifics are reached
//! only through the contracts in [`crate::platform`].

pub mod anki;
pub mod clipboard_monitor;
pub mod dictionary;
pub mod integrity;
pub mod intent;
pub mod ocr;
pub mod selection;
pub mod shell;
pub mod shortcut;
pub mod translation;
