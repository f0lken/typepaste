//! Windows implementation using SendInput / enigo for keystroke emulation.
//!
//! Uses the Windows SendInput API (via the `enigo` crate) to synthesize
//! keyboard events at the OS level. This approach works even in applications
//! that block clipboard paste, remote desktop sessions, VMs, and terminal
//! emulators.

use std::thread;
use std::time::Duration;

use enigo::{Enigo, Key, KeyboardControllable};
use log::{debug, info, warn};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_SHIFT, VK_CONTROL,
    VK_MENU, VK_LWIN,
};

use crate::config::Config;
use crate::error::{Result, TypePasteError};

/// Implements keystroke emulation on Windows using SendInput.
pub struct WindowsPlatform {
    config: Config,
}

impl WindowsPlatform {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Type a string by emitting individual keystrokes.
    pub fn type_string(&self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }

        info!("Typing {} characters on Windows", text.len());

        // Handle layout switching if enabled
        if self.config.layout_switch.enabled && self.config.layout_switch.layouts.len() > 1 {
            return self.type_string_with_layout_switch(text);
        }

        self.type_string_direct(text)
    }

    /// Type text without layout switching (standard path).
    fn type_string_direct(&self, text: &str) -> Result<()> {
        let mut enigo = Enigo::new();

        // Apply initial delay
        if self.config.initial_delay_ms > 0 {
            thread::sleep(Duration::from_millis(self.config.initial_delay_ms));
        }

        for ch in text.chars() {
            self.type_char_enigo(&mut enigo, ch)?;
            self.apply_delay();
        }

        Ok(())
    }

    /// Type text with automatic keyboard layout switching.
    ///
    /// Detects when the script changes (e.g. Latin → Cyrillic) and emits
    /// the configured layout switch hotkey before typing the next character.
    fn type_string_with_layout_switch(&self, text: &str) -> Result<()> {
        let ls = &self.config.layout_switch;
        let mut enigo = Enigo::new();

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
                        self.press_layout_switch_hotkey()?;
                        thread::sleep(Duration::from_millis(ls.switch_delay_ms));
                    }
                    current_layout = target_layout;
                }
            }
            // Neutral characters (digits, punctuation) — no layout switch needed

            self.type_char_enigo(&mut enigo, ch)?;
            self.apply_delay();
        }

        Ok(())
    }

    /// Press the layout switch hotkey using Windows SendInput.
    fn press_layout_switch_hotkey(&self) -> Result<()> {
        let hotkey = &self.config.layout_switch.switch_hotkey;
        debug!("Pressing layout switch hotkey: {}", hotkey);

        let keys = parse_hotkey_to_vk(hotkey);
        if keys.is_empty() {
            return Err(TypePasteError::Platform(
                format!("Cannot parse layout switch hotkey: {hotkey}")
            ));
        }

        // Press all keys down, then release all
        for &vk in &keys {
            send_vk_event(vk, false)?;
        }
        for &vk in keys.iter().rev() {
            send_vk_event(vk, true)?;
        }

        Ok(())
    }

    /// Type a single character using enigo.
    fn type_char_enigo(&self, enigo: &mut Enigo, ch: char) -> Result<()> {
        match ch {
            '\n' if self.config.newlines_as_enter => {
                enigo.key_click(Key::Return);
            }
            '\t' if self.config.tabs_as_tab => {
                enigo.key_click(Key::Tab);
            }
            _ => {
                enigo.key_sequence(&ch.to_string());
            }
        }
        Ok(())
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

    /// List all visible windows using EnumWindows.
    pub fn list_windows(&self) -> Result<Vec<crate::platform::WindowInfo>> {
        use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindowTextW, GetWindowThreadProcessId,
            IsWindowVisible, GetWindowTextLengthW,
        };

        let mut windows: Vec<crate::platform::WindowInfo> = Vec::new();
        let windows_ptr = &mut windows as *mut _ as isize;

        unsafe {
            EnumWindows(
                Some(enum_windows_callback),
                LPARAM(windows_ptr),
            )
            .map_err(|e| TypePasteError::Platform(format!("EnumWindows failed: {e}")))?
        };

        Ok(windows)
    }

    /// Focus a window by its HWND and then type text.
    pub fn type_to_window(&self, text: &str, pid: u32, title: Option<&str>) -> Result<()> {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            FindWindowW, SetForegroundWindow,
        };

        // Try to find window by title first
        if let Some(title_str) = title {
            let wide: Vec<u16> = title_str.encode_utf16().chain(std::iter::once(0)).collect();
            let hwnd = unsafe { FindWindowW(None, windows::core::PCWSTR(wide.as_ptr())) };
            if hwnd != HWND(0) {
                unsafe { SetForegroundWindow(hwnd) };
                thread::sleep(Duration::from_millis(200));
                debug!("Focused window '{}' by title", title_str);
                return self.type_string(text);
            }
        }

        // Fall back to focusing by PID
        if let Ok(windows) = self.list_windows() {
            if let Some(win) = windows.iter().find(|w| w.pid == pid) {
                let wide: Vec<u16> = win.title.encode_utf16().chain(std::iter::once(0)).collect();
                let hwnd = unsafe {
                    windows::Win32::UI::WindowsAndMessaging::FindWindowW(
                        None, windows::core::PCWSTR(wide.as_ptr())
                    )
                };
                if hwnd != HWND(0) {
                    unsafe { windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd) };
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }

        self.type_string(text)
    }
}

// ─── SendInput helpers ─────────────────────────────────────────────────────────

/// Send a single virtual key event (key down or key up).
fn send_vk_event(vk: u16, key_up: bool) -> Result<()> {
    let flags = if key_up { KEYEVENTF_KEYUP } else { Default::default() };

    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let result = unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32)
    };

    if result == 0 {
        Err(TypePasteError::Platform("SendInput failed".into()))
    } else {
        Ok(())
    }
}

/// Parse a hotkey string into a list of virtual key codes.
/// Example: "Alt+Shift" → [VK_MENU, VK_SHIFT]
fn parse_hotkey_to_vk(s: &str) -> Vec<u16> {
    let mut keys = Vec::new();
    for part in s.split('+').map(|p| p.trim()) {
        let vk = match part.to_lowercase().as_str() {
            "shift" => Some(VK_SHIFT.0),
            "ctrl" | "control" => Some(VK_CONTROL.0),
            "alt" | "option" => Some(VK_MENU.0),
            "win" | "meta" | "super" | "cmd" | "command" => Some(VK_LWIN.0),
            key if key.len() == 1 => {
                let ch = key.chars().next().unwrap().to_ascii_uppercase();
                Some(ch as u16)
            }
            _ => None,
        };
        if let Some(vk) = vk {
            keys.push(vk);
        }
    }
    keys
}

// ─── EnumWindows callback ──────────────────────────────────────────────────────

unsafe extern "system" fn enum_windows_callback(
    hwnd: windows::Win32::Foundation::HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::BOOL {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, GetWindowTextLengthW,
    };

    if IsWindowVisible(hwnd).as_bool() {
        let len = GetWindowTextLengthW(hwnd);
        if len > 0 {
            let mut buf = vec![0u16; (len + 1) as usize];
            GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..len as usize]);

            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));

            // Get app name via process
            let app_name = get_process_name(pid);

            let windows = &mut *(lparam.0 as *mut Vec<crate::platform::WindowInfo>);
            windows.push(crate::platform::WindowInfo {
                pid,
                app_name,
                title,
            });
        }
    }
    windows::Win32::Foundation::BOOL(1) // continue enumeration
}

/// Get the process name (executable name without extension) for a given PID.
fn get_process_name(pid: u32) -> String {
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
    use windows::Win32::Foundation::CloseHandle;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        if let Ok(handle) = handle {
            let mut buf = vec![0u16; 260];
            let len = GetModuleBaseNameW(handle, None, &mut buf);
            CloseHandle(handle).ok();
            if len > 0 {
                let name = String::from_utf16_lossy(&buf[..len as usize]);
                // Strip .exe extension
                return name.trim_end_matches(".exe").to_string();
            }
        }
        format!("PID_{pid}")
    }
}
