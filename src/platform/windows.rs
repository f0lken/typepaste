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
//! ## Window Management
//!
//! Window focus and enumeration use Win32 APIs:
//! - `EnumWindows` + `GetWindowTextW` for listing
//! - `SetForegroundWindow` for focusing
//! - `GetWindowThreadProcessId` for PID matching
//!
//! ## Permissions
//!
//! No special permissions are required on Windows for standard applications.
//! However, injecting into elevated (admin) processes requires the TypePaste
//! process to also run elevated.

use log::{debug, info};
use rand::Rng;

use crate::config::Config;
use crate::error::{Result, TypePasteError};
use super::WindowInfo;

/// On Windows, no special accessibility check is needed.
pub fn check_accessibility() -> Result<()> {
    debug!("Windows: no accessibility check required");
    Ok(())
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

/// Type a string by emitting individual keystroke events via SendInput.
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
                continue;
            }
            _ => {
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
pub fn focus_window_by_title(title: &str) -> Result<(u32, String)> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, IsWindowVisible, SetForegroundWindow,
        GetWindowThreadProcessId,
    };

    let title_lower = title.to_lowercase();

    struct SearchState {
        target: String,
        found_hwnd: Option<HWND>,
        found_title: String,
        found_pid: u32,
    }

    let mut state = SearchState {
        target: title_lower,
        found_hwnd: None,
        found_title: String::new(),
        found_pid: 0,
    };

    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam.0 as *mut SearchState);

        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL::from(true);
        }

        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len == 0 {
            return BOOL::from(true);
        }

        let win_title = String::from_utf16_lossy(&buf[..len as usize]);
        if win_title.to_lowercase().contains(&state.target) {
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            state.found_hwnd = Some(hwnd);
            state.found_title = win_title;
            state.found_pid = pid;
            return BOOL::from(false); // stop enumeration
        }

        BOOL::from(true)
    }

    unsafe {
        let _ = EnumWindows(
            Some(enum_callback),
            LPARAM(&mut state as *mut SearchState as isize),
        );
    }

    match state.found_hwnd {
        Some(hwnd) => {
            unsafe {
                let _ = SetForegroundWindow(hwnd);
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
            info!(
                "Focused window: \"{}\" (PID {})",
                state.found_title, state.found_pid
            );
            Ok((state.found_pid, state.found_title))
        }
        None => Err(TypePasteError::Platform(format!(
            "No window found matching title: \"{title}\""
        ))),
    }
}

/// Focus a window owned by the given PID.
pub fn focus_window_by_pid(pid: u32) -> Result<String> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, IsWindowVisible, SetForegroundWindow,
        GetWindowThreadProcessId,
    };

    struct SearchState {
        target_pid: u32,
        found_hwnd: Option<HWND>,
        found_title: String,
    }

    let mut state = SearchState {
        target_pid: pid,
        found_hwnd: None,
        found_title: String::new(),
    };

    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam.0 as *mut SearchState);

        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL::from(true);
        }

        let mut win_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut win_pid));

        if win_pid == state.target_pid {
            let mut buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..len as usize]);
            if !title.is_empty() {
                state.found_hwnd = Some(hwnd);
                state.found_title = title;
                return BOOL::from(false);
            }
        }

        BOOL::from(true)
    }

    unsafe {
        let _ = EnumWindows(
            Some(enum_callback),
            LPARAM(&mut state as *mut SearchState as isize),
        );
    }

    match state.found_hwnd {
        Some(hwnd) => {
            unsafe {
                let _ = SetForegroundWindow(hwnd);
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
            info!("Focused PID {} window: \"{}\"", pid, state.found_title);
            Ok(state.found_title)
        }
        None => Err(TypePasteError::Platform(format!(
            "No visible window found for PID {pid}"
        ))),
    }
}

/// List visible windows with their title, PID, and app name.
pub fn list_windows() -> Result<Vec<WindowInfo>> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, IsWindowVisible,
        GetWindowThreadProcessId,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let mut windows: Vec<WindowInfo> = Vec::new();

    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let windows = &mut *(lparam.0 as *mut Vec<WindowInfo>);

        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL::from(true);
        }

        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len == 0 {
            return BOOL::from(true);
        }

        let title = String::from_utf16_lossy(&buf[..len as usize]);
        if title.is_empty() {
            return BOOL::from(true);
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        // Try to get process name
        let app_name = if let Ok(handle) = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        ) {
            let mut name_buf = [0u16; 260];
            let mut size = name_buf.len() as u32;
            use windows::Win32::System::Threading::QueryFullProcessImageNameW;
            use windows::Win32::System::Threading::PROCESS_NAME_FORMAT;
            if QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                windows::core::PWSTR(name_buf.as_mut_ptr()),
                &mut size,
            )
            .is_ok()
            {
                let full_path = String::from_utf16_lossy(&name_buf[..size as usize]);
                full_path
                    .rsplit('\\')
                    .next()
                    .unwrap_or(&full_path)
                    .to_string()
            } else {
                format!("PID {}", pid)
            }
        } else {
            format!("PID {}", pid)
        };

        windows.push(WindowInfo {
            title,
            pid,
            app_name,
        });

        BOOL::from(true)
    }

    unsafe {
        let _ = EnumWindows(
            Some(enum_callback),
            LPARAM(&mut windows as *mut Vec<WindowInfo> as isize),
        );
    }

    Ok(windows)
}
