//! Cross-platform application logic. Operating-system specifics are reached
//! only through the contracts in [`crate::platform`].

pub mod clipboard_monitor;
pub mod dictionary;
pub mod integrity;
pub mod intent;
pub mod learning;
pub mod ocr;
pub(crate) mod onnx_runtime;
pub mod selection;
pub mod shell;
pub mod shortcut;
pub mod translation;
pub mod tts;
pub mod updater;
