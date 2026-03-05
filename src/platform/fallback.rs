//! Fallback implementation for unsupported platforms.
//!
//! Provides stub implementations that return errors, allowing the project
//! to compile on any platform while clearly indicating lack of support.

use crate::config::Config;
use crate::error::{Result, TypePasteError};
use super::WindowInfo;

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

pub fn focus_window_by_title(_title: &str) -> Result<(u32, String)> {
    Err(TypePasteError::Platform(
        "Window management is not available on this platform.".into(),
    ))
}

pub fn focus_window_by_pid(_pid: u32) -> Result<String> {
    Err(TypePasteError::Platform(
        "Window management is not available on this platform.".into(),
    ))
}

pub fn list_windows() -> Result<Vec<WindowInfo>> {
    Err(TypePasteError::Platform(
        "Window enumeration is not available on this platform.".into(),
    ))
}
