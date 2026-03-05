//! Platform-specific implementations.
//!
//! Each platform module provides:
//! - `check_accessibility()` — verify OS permissions for keystroke injection
//! - `type_string(text, config)` — emit keystrokes for the given text

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

// Fallback for unsupported platforms (Linux could be added later)
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod fallback;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub use fallback::*;
