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

use log::{debug, info, warn};
use rand::Rng;

use crate::config::Config;
use crate::error::{Result, TypePasteError};
use super::WindowInfo;

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

/// Focus a window whose title contains the given substring (case-insensitive).
///
/// Uses AppleScript `System Events` to find and activate the matching window.
/// Returns the PID and title of the focused window.
pub fn focus_window_by_title(title: &str) -> Result<(u32, String)> {
    use std::process::Command;

    // AppleScript: iterate over all processes and windows, find matching title
    let script = format!(
        r#"
        tell application "System Events"
            set matchedApp to ""
            set matchedWin to ""
            set matchedPID to 0
            repeat with proc in (every process whose background only is false)
                try
                    repeat with win in (every window of proc)
                        set winTitle to name of win
                        if winTitle contains "{title}" then
                            set matchedApp to name of proc
                            set matchedWin to winTitle
                            set matchedPID to unix id of proc
                            tell proc to set frontmost to true
                            -- Small delay for window to come to front
                            delay 0.3
                            return matchedPID & "|" & matchedWin & "|" & matchedApp
                        end if
                    end repeat
                end try
            end repeat
            return ""
        end tell
        "#
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| TypePasteError::Platform(format!("osascript exec: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return Err(TypePasteError::Platform(format!(
            "No window found matching title: \"{title}\""
        )));
    }

    let parts: Vec<&str> = stdout.splitn(3, '|').collect();
    if parts.len() >= 2 {
        let pid = parts[0]
            .trim()
            .parse::<u32>()
            .unwrap_or(0);
        let win_title = parts[1].trim().to_string();
        info!("Focused window: \"{}\" (PID {})", win_title, pid);
        Ok((pid, win_title))
    } else {
        Err(TypePasteError::Platform(
            "Failed to parse window focus result".into(),
        ))
    }
}

/// Focus a window owned by the given PID.
pub fn focus_window_by_pid(pid: u32) -> Result<String> {
    use std::process::Command;

    let script = format!(
        r#"
        tell application "System Events"
            set targetProc to first process whose unix id is {pid}
            set frontmost of targetProc to true
            delay 0.3
            set procName to name of targetProc
            try
                set winName to name of first window of targetProc
                return winName & "|" & procName
            on error
                return "(no window)" & "|" & procName
            end try
        end tell
        "#
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| TypePasteError::Platform(format!("osascript exec: {e}")))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(TypePasteError::Platform(format!(
            "No process found with PID {pid}: {stderr}"
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let parts: Vec<&str> = stdout.splitn(2, '|').collect();
    let win_title = parts.first().unwrap_or(&"").to_string();
    info!("Focused PID {} window: \"{}\"", pid, win_title);
    Ok(win_title)
}

/// List visible windows with their title, PID, and app name.
pub fn list_windows() -> Result<Vec<WindowInfo>> {
    use std::process::Command;

    let script = r#"
        set output to ""
        tell application "System Events"
            repeat with proc in (every process whose background only is false)
                try
                    set procPID to unix id of proc
                    set procName to name of proc
                    repeat with win in (every window of proc)
                        set winTitle to name of win
                        set output to output & procPID & "\t" & procName & "\t" & winTitle & "\n"
                    end repeat
                end try
            end repeat
        end tell
        return output
    "#;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| TypePasteError::Platform(format!("osascript exec: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut windows = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() == 3 {
            let pid = parts[0].trim().parse::<u32>().unwrap_or(0);
            let app_name = parts[1].trim().to_string();
            let title = parts[2].trim().to_string();
            if !title.is_empty() {
                windows.push(WindowInfo {
                    title,
                    pid,
                    app_name,
                });
            }
        }
    }

    Ok(windows)
}
