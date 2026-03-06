//! macOS implementation using CGEvent API for keystroke emulation.
//!
//! ## How it works
//!
//! On macOS, we use CGEvent API to synthesize keyboard events at the OS level.
//! For arbitrary Unicode characters, CGEventKeyboardSetUnicodeString injects
//! the character directly. This works in remote desktop sessions, VMs, etc.
//!
//! ## Layout Switching
//!
//! When `layout_switch.enabled` is true, we track the current keyboard layout
//! index and emit the configured switch hotkey (e.g. Alt+Shift) when the text
//! crosses a Unicode script boundary (e.g. Latin → Cyrillic).
//!
//! ## Permissions
//!
//! Requires Accessibility permissions (System Settings → Privacy & Security →
//! Accessibility). Without this, CGEvent posting will silently fail.

use std::thread;
use std::time::Duration;

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use log::{debug, info, warn};

use crate::config::Config;
use crate::error::{Result, TypePasteError};
use super::WindowInfo;

// ─── Virtual key codes ─────────────────────────────────────────────────────────

const KEY_RETURN: CGKeyCode = 0x24;
const KEY_TAB: CGKeyCode = 0x30;
const KEY_SHIFT: CGKeyCode = 0x38;
const KEY_CONTROL: CGKeyCode = 0x3B;
const KEY_ALT: CGKeyCode = 0x3A; // Option key
const KEY_CMD: CGKeyCode = 0x37; // Command key

// ─── Public free functions (called by engine.rs via platform::*) ────────────────

/// Check if the application has Accessibility permissions on macOS.
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

/// Type a string by emitting individual keystroke events.
///
/// When `config.layout_switch.enabled` is true, detects Unicode script
/// boundary crossings and emits the layout switch hotkey as needed.
pub fn type_string(text: &str, config: &Config) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| TypePasteError::Platform("Failed to create CGEventSource".into()))?;

    let ls = &config.layout_switch;
    let layout_enabled = ls.enabled && ls.layouts.len() > 1 && !ls.switch_hotkey.is_empty();

    let mut current_layout: usize = 0;

    for ch in text.chars() {
        // ── Layout switching ──
        if layout_enabled {
            if let Some(needed_layout) = ls.layout_for_char(ch) {
                if needed_layout != current_layout {
                    let presses = ls.presses_needed(current_layout, needed_layout);
                    debug!(
                        "Layout switch: {} → {} ({} press(es) of '{}')",
                        ls.layouts[current_layout].name,
                        ls.layouts[needed_layout].name,
                        presses,
                        ls.switch_hotkey
                    );
                    for _ in 0..presses {
                        press_layout_switch_hotkey(&source, &ls.switch_hotkey)?;
                        thread::sleep(Duration::from_millis(ls.switch_delay_ms));
                    }
                    current_layout = needed_layout;
                }
            }
        }

        // ── Type the character ──
        match ch {
            '\n' if config.newlines_as_enter => {
                emit_key(&source, KEY_RETURN)?;
            }
            '\t' if config.tabs_as_tab => {
                emit_key(&source, KEY_TAB)?;
            }
            '\r' => {
                // Skip carriage returns
                continue;
            }
            _ => {
                type_unicode_char(&source, ch)?;
            }
        }

        // ── Per-keystroke delay ──
        let delay = compute_delay(config);
        if !delay.is_zero() {
            thread::sleep(delay);
        }
    }

    Ok(())
}

/// Focus a window whose title contains the given substring (case-insensitive).
/// Returns the PID and title of the focused window.
pub fn focus_window_by_title(title: &str) -> Result<(u32, String)> {
    use std::process::Command;

    let script = format!(
        r#"
        tell application "System Events"
            repeat with proc in (every process whose background only is false)
                try
                    repeat with win in (every window of proc)
                        set winTitle to name of win
                        if winTitle contains "{title}" then
                            set matchedPID to unix id of proc
                            tell proc to set frontmost to true
                            delay 0.3
                            return (matchedPID as text) & "|" & winTitle
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

    let parts: Vec<&str> = stdout.splitn(2, '|').collect();
    if parts.len() == 2 {
        let pid = parts[0].trim().parse::<u32>().unwrap_or(0);
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
            try
                set winName to name of first window of targetProc
                return winName
            on error
                return "(no window)"
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

    let win_title = String::from_utf8_lossy(&output.stdout).trim().to_string();
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

// ─── Internal helpers ───────────────────────────────────────────────────────────

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

    let key = CFString::new("AXTrustedCheckOptionPrompt");
    let value = CFBoolean::true_value();
    let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
    AXIsProcessTrustedWithOptions(options.as_CFTypeRef())
}

/// Compute the delay for a single keystroke: base + optional random jitter.
fn compute_delay(config: &Config) -> Duration {
    let base = config.keystroke_delay_ms;
    let jitter = if config.has_random_delay() {
        use rand::Rng;
        rand::thread_rng().gen_range(config.random_delay_min_ms..=config.random_delay_max_ms)
    } else {
        0
    };
    Duration::from_millis(base + jitter)
}

/// Type a single Unicode character via CGEvent.
fn type_unicode_char(source: &CGEventSource, ch: char) -> Result<()> {
    // Create key-down event with virtual keycode 0, then set the Unicode string
    let key_down = CGEvent::new_keyboard_event(source.clone(), 0, true)
        .map_err(|_| TypePasteError::Platform("Failed to create key down event".into()))?;
    key_down.set_string(&ch.to_string());
    key_down.post(CGEventTapLocation::HID);

    let key_up = CGEvent::new_keyboard_event(source.clone(), 0, false)
        .map_err(|_| TypePasteError::Platform("Failed to create key up event".into()))?;
    key_up.set_string("");
    key_up.post(CGEventTapLocation::HID);

    Ok(())
}

/// Emit a key press+release for a given virtual keycode (no modifiers).
fn emit_key(source: &CGEventSource, code: CGKeyCode) -> Result<()> {
    let down = CGEvent::new_keyboard_event(source.clone(), code, true)
        .map_err(|_| TypePasteError::Platform("Failed to create key event".into()))?;
    down.post(CGEventTapLocation::HID);

    let up = CGEvent::new_keyboard_event(source.clone(), code, false)
        .map_err(|_| TypePasteError::Platform("Failed to create key event".into()))?;
    up.post(CGEventTapLocation::HID);
    Ok(())
}

/// Emit a key press with the given modifier flags.
fn emit_key_with_modifiers(
    source: &CGEventSource,
    code: CGKeyCode,
    flags: CGEventFlags,
) -> Result<()> {
    let down = CGEvent::new_keyboard_event(source.clone(), code, true)
        .map_err(|_| TypePasteError::Platform("Failed to create key event".into()))?;
    down.set_flags(flags);
    down.post(CGEventTapLocation::HID);

    let up = CGEvent::new_keyboard_event(source.clone(), code, false)
        .map_err(|_| TypePasteError::Platform("Failed to create key event".into()))?;
    up.set_flags(CGEventFlags::empty());
    up.post(CGEventTapLocation::HID);
    Ok(())
}

/// Press the layout switch hotkey (e.g. "Alt+Shift") via CGEvent.
fn press_layout_switch_hotkey(source: &CGEventSource, hotkey: &str) -> Result<()> {
    debug!("Pressing layout switch hotkey: {}", hotkey);

    let (modifiers, key_code) = parse_hotkey_to_cg(hotkey)
        .ok_or_else(|| TypePasteError::Platform(
            format!("Cannot parse layout switch hotkey: {hotkey}")
        ))?;

    emit_key_with_modifiers(source, key_code, modifiers)?;
    Ok(())
}

/// Parse a hotkey string (e.g. "Alt+Shift", "Ctrl+Shift") into
/// (CGEventFlags, CGKeyCode). For modifier-only combos like "Alt+Shift",
/// the last modifier in the string becomes the "key" that gets press/release.
fn parse_hotkey_to_cg(s: &str) -> Option<(CGEventFlags, CGKeyCode)> {
    let mut flags = CGEventFlags::empty();
    let mut last_key_code: Option<CGKeyCode> = None;
    let mut last_modifier_code: Option<CGKeyCode> = None;

    for part in s.split('+').map(|p| p.trim()) {
        match part.to_lowercase().as_str() {
            "shift" => {
                flags |= CGEventFlags::CGEventFlagShift;
                last_modifier_code = Some(KEY_SHIFT);
            }
            "ctrl" | "control" => {
                flags |= CGEventFlags::CGEventFlagControl;
                last_modifier_code = Some(KEY_CONTROL);
            }
            "alt" | "option" => {
                flags |= CGEventFlags::CGEventFlagAlternate;
                last_modifier_code = Some(KEY_ALT);
            }
            "cmd" | "command" | "meta" | "super" => {
                flags |= CGEventFlags::CGEventFlagCommand;
                last_modifier_code = Some(KEY_CMD);
            }
            key => {
                // Non-modifier key
                last_key_code = match key {
                    "space" => Some(0x31),
                    "a" => Some(0x00), "b" => Some(0x0B), "c" => Some(0x08),
                    "d" => Some(0x02), "e" => Some(0x0E), "f" => Some(0x03),
                    "g" => Some(0x05), "h" => Some(0x04), "i" => Some(0x22),
                    "j" => Some(0x26), "k" => Some(0x28), "l" => Some(0x25),
                    "m" => Some(0x2E), "n" => Some(0x2D), "o" => Some(0x1F),
                    "p" => Some(0x23), "q" => Some(0x0C), "r" => Some(0x0F),
                    "s" => Some(0x01), "t" => Some(0x11), "u" => Some(0x20),
                    "v" => Some(0x09), "w" => Some(0x0D), "x" => Some(0x07),
                    "y" => Some(0x10), "z" => Some(0x06),
                    _ => None,
                };
            }
        }
    }

    // If there's an explicit non-modifier key, use that.
    // Otherwise, use the last modifier key as the "trigger" key
    // (for combos like "Alt+Shift" where Shift is the trigger).
    let key_code = last_key_code.or(last_modifier_code)?;
    Some((flags, key_code))
}
