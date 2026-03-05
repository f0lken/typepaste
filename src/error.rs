use thiserror::Error;

#[derive(Error, Debug)]
pub enum TypePasteError {
    #[error("Clipboard error: {0}")]
    Clipboard(String),

    #[error("Keystroke emulation error: {0}")]
    Keystroke(String),

    #[error("Platform error: {0}")]
    Platform(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Accessibility permission not granted")]
    AccessibilityDenied,

    #[error("Hotkey registration failed: {0}")]
    HotkeyRegistration(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, TypePasteError>;
