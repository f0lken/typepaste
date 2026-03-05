//! Platform-specific implementations.
//!
//! Each platform module provides:
//! - `check_accessibility()` \u2014 verify OS permissions for keystroke injection
//! - `type_string(text, config)` \u2014 emit keystrokes for the given text
//! - `focus_window_by_title(title)` \u2014 focus a window matching the title substring
//! - `focus_window_by_pid(pid)` \u2014 focus a window owned by the given PID
//! - `list_windows()` \u2014 enumerate visible windows with title + PID

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

/// Describes a visible window on the system.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WindowInfo {
    pub title: String,
    pub pid: u32,
    pub app_name: String,
}
