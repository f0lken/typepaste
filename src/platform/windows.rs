//! Windows implementation using enigo 0.2+ for keystroke emulation.
//!
//! Uses the `enigo` crate (which wraps Windows SendInput API) to synthesize
//! keyboard events at the OS level. This approach works even in applications
//! that block clipboard paste, remote desktop sessions, VMs, and terminal
//! emulators.
//!
//! For layout switch hotkeys, we use the raw Windows SendInput API directly
//! to avoid any issues with enigo's key mapping.

use std::thread;
use std::time::Duration;

use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use log::{debug, info, warn};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_SHIFT, VK_CONTROL,
    VK_MENU, VK_LWIN,
};

use crate::config::Config;
use crate::error::{Result, TypePasteError};
use super::WindowInfo;

// ─── Public free functions (called by engine.rs via platform::*) ────────────────

/// On Windows, accessibility is generally available — just return Ok.
pub fn check_accessibility() -> Result<()> {
    debug!("Windows: no accessibility check required");
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

    info!("Typing {} characters on Windows", text.len());

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| TypePasteError::Platform(format!("Failed to create Enigo: {e}")))?;

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
                        press_layout_switch_hotkey(&ls.switch_hotkey)?;
                        thread::sleep(Duration::from_millis(ls.switch_delay_ms));
                    }
                    current_layout = needed_layout;
                }
            }
        }

        // ── Type the character ──
        match ch {
            '\n' if config.newlines_as_enter => {
                enigo.key(Key::Return, Click)
                    .map_err(|e| TypePasteError::Platform(format!("Key Return: {e}")))?;
            }
            '\t' if config.tabs_as_tab => {
                enigo.key(Key::Tab, Click)
                    .map_err(|e| TypePasteError::Platform(format!("Key Tab: {e}")))?;
            }
            '\r' => {
                // Skip carriage returns
                continue;
            }
            _ => {
                enigo.text(&ch.to_string())
                    .map_err(|e| TypePasteError::Platform(format!("Text '{}': {e}", ch)))?;
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

/// Focus a window whose title contains the given substring.
pub fn focus_window_by_title(title: &str) -> Result<(u32, String)> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
    };

    struct SearchResult {
        target: String,
        found_hwnd: Option<isize>,
        found_pid: u32,
        found_title: String,
    }

    let mut search = SearchResult {
        target: title.to_lowercase(),
        found_hwnd: None,
        found_pid: 0,
        found_title: String::new(),
    };

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam.0 as *mut SearchResult);

        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowTextW, GetWindowTextLengthW,
            GetWindowThreadProcessId, IsWindowVisible,
        };

        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }

        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return BOOL(1);
        }

        let mut buf = vec![0u16; (len + 1) as usize];
        GetWindowTextW(hwnd, &mut buf);
        let win_title = String::from_utf16_lossy(&buf[..len as usize]);

        if win_title.to_lowercase().contains(&search.target) {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            search.found_hwnd = Some(hwnd.0 as isize);
            search.found_pid = pid;
            search.found_title = win_title;
            return BOOL(0); // stop enumeration
        }

        BOOL(1)
    }

    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut search as *mut _ as isize),
        );
    }

    if let Some(hwnd_val) = search.found_hwnd {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
            SetForegroundWindow(HWND(hwnd_val as *mut _));
        }
        thread::sleep(Duration::from_millis(200));
        info!("Focused window: \"{}\" (PID {})", search.found_title, search.found_pid);
        Ok((search.found_pid, search.found_title))
    } else {
        Err(TypePasteError::Platform(format!(
            "No window found matching title: \"{title}\""
        )))
    }
}

/// Focus a window owned by the given PID.
pub fn focus_window_by_pid(pid: u32) -> Result<String> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
    };

    struct SearchResult {
        target_pid: u32,
        found_hwnd: Option<isize>,
        found_title: String,
    }

    let mut search = SearchResult {
        target_pid: pid,
        found_hwnd: None,
        found_title: String::new(),
    };

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam.0 as *mut SearchResult);

        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowTextW, GetWindowTextLengthW,
            GetWindowThreadProcessId, IsWindowVisible,
        };

        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }

        let mut win_pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut win_pid));

        if win_pid == search.target_pid {
            let len = GetWindowTextLengthW(hwnd);
            if len > 0 {
                let mut buf = vec![0u16; (len + 1) as usize];
                GetWindowTextW(hwnd, &mut buf);
                search.found_title = String::from_utf16_lossy(&buf[..len as usize]);
                search.found_hwnd = Some(hwnd.0 as isize);
                return BOOL(0); // stop
            }
        }

        BOOL(1)
    }

    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut search as *mut _ as isize),
        );
    }

    if let Some(hwnd_val) = search.found_hwnd {
        unsafe {
            SetForegroundWindow(HWND(hwnd_val as *mut _));
        }
        thread::sleep(Duration::from_millis(200));
        info!("Focused PID {} window: \"{}\"", pid, search.found_title);
        Ok(search.found_title)
    } else {
        Err(TypePasteError::Platform(format!(
            "No window found with PID {pid}"
        )))
    }
}

/// List visible windows with their title, PID, and app name.
pub fn list_windows() -> Result<Vec<WindowInfo>> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsWindowVisible,
    };

    let mut windows: Vec<WindowInfo> = Vec::new();

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowTextW, GetWindowTextLengthW,
            GetWindowThreadProcessId, IsWindowVisible,
        };

        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }

        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return BOOL(1);
        }

        let mut buf = vec![0u16; (len + 1) as usize];
        GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..len as usize]);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        let app_name = get_process_name(pid);

        let windows = &mut *(lparam.0 as *mut Vec<WindowInfo>);
        windows.push(WindowInfo {
            title,
            pid,
            app_name,
        });

        BOOL(1) // continue
    }

    unsafe {
        EnumWindows(
            Some(callback),
            LPARAM(&mut windows as *mut _ as isize),
        )
        .map_err(|e| TypePasteError::Platform(format!("EnumWindows failed: {e}")))?;
    }

    Ok(windows)
}

// ─── Internal helpers ───────────────────────────────────────────────────────────

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

/// Press the layout switch hotkey using Windows SendInput.
fn press_layout_switch_hotkey(hotkey: &str) -> Result<()> {
    debug!("Pressing layout switch hotkey: {}", hotkey);

    let keys = parse_hotkey_to_vk(hotkey);
    if keys.is_empty() {
        return Err(TypePasteError::Platform(
            format!("Cannot parse layout switch hotkey: {hotkey}")
        ));
    }

    // Press all keys down, then release all in reverse order
    for &vk in &keys {
        send_vk_event(vk, false)?;
    }
    for &vk in keys.iter().rev() {
        send_vk_event(vk, true)?;
    }

    Ok(())
}

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
            let _ = CloseHandle(handle);
            if len > 0 {
                let name = String::from_utf16_lossy(&buf[..len as usize]);
                return name.trim_end_matches(".exe").to_string();
            }
        }
        format!("PID_{pid}")
    }
}
