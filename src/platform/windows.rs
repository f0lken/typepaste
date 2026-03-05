//! Windows implementation using SendInput / enigo for keystroke emulation.
//!
//! ## How it works
//!
//! On Windows, we use the Win32 SendInput API (via the `enigo` crate) to inject
//! keyboard events at the OS level. This works even in:
//! - Remote Desktop (RDP) sessions where clipboard sharing is disabled
//! - VMware/VirtualBox consoles without guest tools
//! - Windows login screens (UAC prompts require running as admin)
//! - Applications that block Ctrl+V paste
//!
//! For Unicode characters, we use KEYEVENTF_UNICODE flag with SendInput,
//! which injects the character directly without needing a keycode mapping.
//!
//! ## Permissions
//!
//! No special permissions are required on Windows for standard applications.
//! However, injecting into elevated (admin) processes requires the TypePaste
//! process to also run elevated.

use log::debug;

use crate::config::Config;
use crate::error::{Result, TypePasteError};

/// On Windows, no special accessibility check is needed.
/// However, we note if the process is not elevated (for informational purposes).
pub fn check_accessibility() -> Result<()> {
    debug!("Windows: no accessibility check required");
    // TODO: optionally check if running as admin and warn if not
    Ok(())
}

/// Type a string by emitting individual keystroke events via SendInput.
pub fn type_string(text: &str, config: &Config) -> Result<()> {
    use enigo::{Enigo, Keyboard, Settings};

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| TypePasteError::Keystroke(format!("Init enigo: {e}")))?;

    let delay = std::time::Duration::from_millis(config.keystroke_delay_ms);

    for ch in text.chars() {
        match ch {
            '\n' if config.newlines_as_enter => {
                enigo
                    .key(enigo::Key::Return, enigo::Direction::Click)
                    .map_err(|e| TypePasteError::Keystroke(format!("Return key: {e}")))?;
            }
            '\t' if config.tabs_as_tab => {
                enigo
                    .key(enigo::Key::Tab, enigo::Direction::Click)
                    .map_err(|e| TypePasteError::Keystroke(format!("Tab key: {e}")))?;
            }
            '\r' => {
                // Skip carriage returns
                continue;
            }
            _ => {
                // enigo on Windows uses SendInput with KEYEVENTF_UNICODE
                // which handles arbitrary Unicode characters correctly
                let s = ch.to_string();
                enigo
                    .text(&s)
                    .map_err(|e| TypePasteError::Keystroke(format!("Char '{ch}': {e}")))?;
            }
        }

        if config.keystroke_delay_ms > 0 {
            std::thread::sleep(delay);
        }
    }

    Ok(())
}
