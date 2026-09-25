//! Cross-platform application logic. Operating-system specifics are reached
//! only through the contracts in [`crate::platform`].

pub mod anki;
pub mod dictionary;
pub mod integrity;
pub mod ocr;
pub mod selection;
pub mod shell;
pub mod shortcut;
pub mod translation;
