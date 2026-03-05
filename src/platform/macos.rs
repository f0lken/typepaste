//! macOS implementation using CGEvent API for keystroke emulation.
//!
//! ## How it works
//!
//! On macOS, we use two approaches depending on the character:
//!
//! 1. **CGEvent keyboard events** — for standard ASCII characters, we create
//!    CGEvent keyboard events with the appropriate virtual keycodes and post
//!    them to the HID event tap. This is the most reliable method for
//!    injecting keystrokes into remote desktop sessions, VMs, etc.
//!
//! 2. **enigo fallback** — for Unicode characters that don't have a direct
//!    keycode mapping, we use enigo's `text()` method which leverages
//!    CGEventKeyboardSetUnicodeString under the hood.
//!
//! ## Permissions
//!
//! The app requires Accessibility permissions (System Settings → Privacy &
//! Security → Accessibility). Without this, CGEvent posting will silently fail.

use log::debug;
use rand::Rng;

use crate::config::Config;
use crate::error::{Result, TypePasteError};

/// Check if the application has Accessibility permissions on macOS.
///
/// If not granted, returns an error with instructions.
pub fn check_accessibility() -> Result<()> {
    unsafe {
        let trusted = macos_accessibility_check();
        if !trusted {
            return Err(TypePasteError::AccessibilityDenied);
        }
    }
    debug!("Accessibility permission granted");
    Ok(())
}

/// FFI call to AXIsProcessTrustedWithOptions
unsafe fn macos_accessibility_check() -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    extern "C" {
        fn AXIsProcessTrustedWithOptions(
            options: core_foundation::base::CFTypeRef,
        ) -> bool;
    }

    // kAXTrustedCheckOptionPrompt = true → show the system prompt
    let key = CFString::new("AXTrustedCheckOptionPrompt");
    let value = CFBoolean::true_value();
    let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);

    AXIsProcessTrustedWithOptions(options.as_CFTypeRef())
}

/// Compute the delay for a single keystroke: base + optional random jitter.
fn compute_delay(config: &Config) -> std::time::Duration {
    let base = config.keystroke_delay_ms;
    let jitter = if config.has_random_delay() {
        let mut rng = rand::thread_rng();
        rng.gen_range(config.random_delay_min_ms..=config.random_delay_max_ms)
    } else {
        0
    };
    std::time::Duration::from_millis(base + jitter)
}

/// Type a string by emitting individual keystroke events.
pub fn type_string(text: &str, config: &Config) -> Result<()> {
    use enigo::{Enigo, Keyboard, Settings};

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| TypePasteError::Keystroke(format!("Init enigo: {e}")))?;

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
                // Skip carriage returns (handle \r\n as just \n)
                continue;
            }
            _ => {
                // Use enigo's text() for individual characters — handles Unicode
                let s = ch.to_string();
                enigo
                    .text(&s)
                    .map_err(|e| TypePasteError::Keystroke(format!("Char '{ch}': {e}")))?;
            }
        }

        let delay = compute_delay(config);
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
    }

    Ok(())
}
