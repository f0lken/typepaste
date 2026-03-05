//! Fallback implementation for unsupported platforms.
//!
//! Provides stub implementations that return errors, allowing the project
//! to compile on any platform while clearly indicating lack of support.

use crate::config::Config;
use crate::error::{Result, TypePasteError};

pub fn check_accessibility() -> Result<()> {
    Err(TypePasteError::Platform(
        "TypePaste is not yet supported on this platform. Supported: macOS, Windows.".into(),
    ))
}

pub fn type_string(_text: &str, _config: &Config) -> Result<()> {
    Err(TypePasteError::Platform(
        "Keystroke emulation is not available on this platform.".into(),
    ))
}
