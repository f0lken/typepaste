//! macOS implementation using CGEvent API for keystroke emulation.
//!
//! Uses Core Graphics (CGEvent) to synthesize keyboard events at the OS level.
//! This approach works even in applications that block clipboard paste,
//! remote desktop sessions, VMs, and terminal emulators.

use std::collections::HashMap;
use std::thread;
use std::time::Duration;

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use log::{debug, info, warn};

use crate::config::Config;
use crate::error::{Result, TypePasteError};

/// Implements keystroke emulation on macOS using CGEvent.
pub struct MacOSPlatform {
    config: Config,
}

impl MacOSPlatform {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Type a string by emitting individual keystrokes.
    pub fn type_string(&self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }

        info!("Typing {} characters on macOS", text.len());

        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| TypePasteError::Platform("Failed to create CGEventSource".into()))?;

        // Handle layout switching if enabled
        if self.config.layout_switch.enabled && self.config.layout_switch.layouts.len() > 1 {
            return self.type_string_with_layout_switch(text, &source);
        }

        self.type_string_direct(text, &source)
    }

    /// Type text without layout switching (standard path).
    fn type_string_direct(&self, text: &str, source: &CGEventSource) -> Result<()> {
        // Apply initial delay
        if self.config.initial_delay_ms > 0 {
            thread::sleep(Duration::from_millis(self.config.initial_delay_ms));
        }

        for ch in text.chars() {
            self.type_char(ch, source)?;
            self.apply_delay();
        }

        Ok(())
    }

    /// Type text with automatic keyboard layout switching.
    ///
    /// Detects when the script changes (e.g. Latin → Cyrillic) and emits
    /// the configured layout switch hotkey before typing the next character.
    fn type_string_with_layout_switch(&self, text: &str, source: &CGEventSource) -> Result<()> {
        let ls = &self.config.layout_switch;

        // Apply initial delay
        if self.config.initial_delay_ms > 0 {
            thread::sleep(Duration::from_millis(self.config.initial_delay_ms));
        }

        let mut current_layout: usize = 0; // assume first layout is active

        for ch in text.chars() {
            // Determine the required layout for this character
            if let Some(target_layout) = ls.layout_for_char(ch) {
                if target_layout != current_layout {
                    let presses = ls.presses_needed(current_layout, target_layout);
                    info!(
                        "Layout switch: {} → {} ({} press(es)) before '{}'",
                        ls.layouts[current_layout].name,
                        ls.layouts[target_layout].name,
                        presses,
                        ch
                    );
                    for _ in 0..presses {
                        self.press_layout_switch_hotkey(source)?;
                        thread::sleep(Duration::from_millis(ls.switch_delay_ms));
                    }
                    current_layout = target_layout;
                }
            }
            // Neutral characters (digits, punctuation) — no layout switch needed

            self.type_char(ch, source)?;
            self.apply_delay();
        }

        Ok(())
    }

    /// Press the layout switch hotkey (e.g. Alt+Shift) on macOS.
    fn press_layout_switch_hotkey(&self, source: &CGEventSource) -> Result<()> {
        let hotkey = &self.config.layout_switch.switch_hotkey;
        debug!("Pressing layout switch hotkey: {}", hotkey);

        // Parse the hotkey string
        let (modifiers, key_code) = parse_hotkey_to_cg(hotkey)
            .ok_or_else(|| TypePasteError::Platform(
                format!("Cannot parse layout switch hotkey: {hotkey}")
            ))?;

        emit_key_with_modifiers(source, key_code, modifiers)?;
        Ok(())
    }

    /// Emit a single character as a key event.
    fn type_char(&self, ch: char, source: &CGEventSource) -> Result<()> {
        // For Unicode characters, use CGEventKeyboardSetUnicodeString
        // This is the most reliable method for arbitrary Unicode input
        let key_event = CGEvent::new_keyboard_event(source.clone(), 0, true)
            .map_err(|_| TypePasteError::Platform("Failed to create key down event".into()))?;

        key_event.set_string(&ch.to_string());
        key_event.post(CGEventTapLocation::HID);

        let key_up = CGEvent::new_keyboard_event(source.clone(), 0, false)
            .map_err(|_| TypePasteError::Platform("Failed to create key up event".into()))?;
        key_up.set_string("");
        key_up.post(CGEventTapLocation::HID);

        Ok(())
    }

    /// Handle special characters (newlines, tabs) if configured.
    fn handle_special_char(&self, ch: char, source: &CGEventSource) -> Option<Result<()>> {
        match ch {
            '\n' if self.config.newlines_as_enter => {
                Some(emit_key(source, KEY_RETURN, false))
            }
            '\t' if self.config.tabs_as_tab => {
                Some(emit_key(source, KEY_TAB, false))
            }
            _ => None,
        }
    }

    /// Apply per-keystroke delay (base + optional random jitter).
    fn apply_delay(&self) {
        let base = self.config.keystroke_delay_ms;
        let delay = if self.config.has_random_delay() {
            use rand::Rng;
            let jitter = rand::thread_rng().gen_range(
                self.config.random_delay_min_ms..=self.config.random_delay_max_ms
            );
            base + jitter
        } else {
            base
        };
        if delay > 0 {
            thread::sleep(Duration::from_millis(delay));
        }
    }

    /// List all visible windows using CGWindowListCopyWindowInfo.
    pub fn list_windows(&self) -> Result<Vec<crate::platform::WindowInfo>> {
        use core_foundation::array::CFArray;
        use core_foundation::base::TCFType;
        use core_foundation::dictionary::CFDictionary;
        use core_foundation::number::CFNumber;
        use core_foundation::string::CFString;
        use core_graphics::window::{{
            CGWindowListCopyWindowInfo, CGWindowListOption, kCGWindowListOptionOnScreenOnly,
            kCGWindowListExcludeDesktopElements, kCGNullWindowID,
        }};

        let options = kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements;
        let window_list = unsafe {
            CGWindowListCopyWindowInfo(options, kCGNullWindowID)
        };

        if window_list.is_null() {
            return Ok(vec![]);
        }

        let array: CFArray<CFDictionary> = unsafe { CFArray::wrap_under_get_rule(window_list) };
        let mut windows = Vec::new();

        for dict in array.iter() {
            let pid = get_dict_number(&dict, "kCGWindowOwnerPID").unwrap_or(0) as u32;
            let app_name = get_dict_string(&dict, "kCGWindowOwnerName")
                .unwrap_or_default();
            let title = get_dict_string(&dict, "kCGWindowName")
                .unwrap_or_default();
            let layer = get_dict_number(&dict, "kCGWindowLayer").unwrap_or(999);

            // Only include normal windows (layer 0)
            if layer == 0 && pid > 0 {
                windows.push(crate::platform::WindowInfo { pid, app_name, title });
            }
        }

        Ok(windows)
    }

    /// Focus a window by PID and optionally title, then type into it.
    pub fn type_to_window(&self, text: &str, pid: u32, _title: Option<&str>) -> Result<()> {
        use std::process::Command;

        // Use AppleScript to bring the app to front
        let script = format!(
            "tell application \"System Events\" to set frontmost of (first process whose unix id is {pid}) to true"
        );
        let status = Command::new("osascript")
            .args(["-e", &script])
            .status()
            .map_err(|e| TypePasteError::Platform(format!("osascript failed: {e}")));

        match status {
            Ok(s) if s.success() => {
                debug!("Focused window with PID {pid}");
                // Brief pause to allow window to come to front
                thread::sleep(Duration::from_millis(200));
            }
            Ok(s) => {
                warn!("osascript exited with status {s}, proceeding anyway");
            }
            Err(e) => {
                warn!("Failed to focus window: {e}, proceeding anyway");
            }
        }

        self.type_string(text)
    }
}

// ─── CGEvent helpers ───────────────────────────────────────────────────────────

/// macOS virtual key codes for special keys.
const KEY_RETURN: CGKeyCode = 0x24;
const KEY_TAB: CGKeyCode = 0x30;
const KEY_SHIFT: CGKeyCode = 0x38;
const KEY_CONTROL: CGKeyCode = 0x3B;
const KEY_ALT: CGKeyCode = 0x3A;    // Option key
const KEY_CMD: CGKeyCode = 0x37;    // Command key

/// Emit a key press+release without modifiers.
fn emit_key(source: &CGEventSource, code: CGKeyCode, _is_unicode: bool) -> Result<()> {
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

/// Parse a hotkey string (e.g. "Alt+Shift", "Ctrl+Shift") into
/// (CGEventFlags, CGKeyCode). The last non-modifier token is the key.
fn parse_hotkey_to_cg(s: &str) -> Option<(CGEventFlags, CGKeyCode)> {
    let mut flags = CGEventFlags::empty();
    let mut key_code: Option<CGKeyCode> = None;

    for part in s.split('+').map(|p| p.trim()) {
        match part.to_lowercase().as_str() {
            "shift" => flags |= CGEventFlags::CGEventFlagShift,
            "ctrl" | "control" => flags |= CGEventFlags::CGEventFlagControl,
            "alt" | "option" => flags |= CGEventFlags::CGEventFlagAlternate,
            "cmd" | "command" | "meta" | "super" => flags |= CGEventFlags::CGEventFlagCommand,
            key => {
                // Map common key names to CGKeyCode
                key_code = match key {
                    "space" => Some(0x31),
                    "shift" => Some(KEY_SHIFT),    // shouldn't happen but guard it
                    "alt" | "option" => Some(KEY_ALT),
                    "ctrl" | "control" => Some(KEY_CONTROL),
                    "cmd" | "command" => Some(KEY_CMD),
                    // Letters a-z → CGKeyCode mapping (US layout)
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

    key_code.map(|kc| (flags, kc))
}

// ─── CoreFoundation helpers ────────────────────────────────────────────────────

fn get_dict_number(dict: &core_foundation::dictionary::CFDictionary, key: &str) -> Option<i64> {
    use core_foundation::base::TCFType;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;

    let key = CFString::new(key);
    dict.find(&key).and_then(|val| {
        let num: core_foundation::number::CFNumber = unsafe {
            core_foundation::base::TCFType::wrap_under_get_rule(*val as _)
        };
        num.to_i64()
    })
}

fn get_dict_string(
    dict: &core_foundation::dictionary::CFDictionary,
    key: &str,
) -> Option<String> {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    let key = CFString::new(key);
    dict.find(&key).map(|val| {
        let s: CFString = unsafe {
            core_foundation::base::TCFType::wrap_under_get_rule(*val as _)
        };
        s.to_string()
    })
}
