//! TypePaste — paste clipboard text anywhere via keystroke emulation.
//!
//! Works even in remote desktops, VMs, and applications that don't support
//! direct clipboard paste (Ctrl+V / Cmd+V).
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │                       TypePaste App                          │
//! │                                                              │
//! │  ┌──────────┐   ┌────────────┐   ┌────────────────────────┐ │
//! │  │ System   │   │  Global    │   │   TypeEngine           │ │
//! │  │ Tray     │◄──│  Hotkey    │──►│                        │ │
//! │  │ Icon     │   │  Listener  │   │  1. Read clipboard     │ │
//! │  │          │   │            │   │  2. Wait initial delay  │ │
//! │  │ • Paste  │   │ Cmd+Shift+V│  │  3. Emit keystrokes    │ │
//! │  │ • Settings│  │ (macOS)    │   │     per character      │ │
//! │  │ • Quit   │   │            │   │                        │ │
//! │  └──────────┘   │ Ctrl+Shift+V│  └────────┬───────────────┘ │
//! │                 │ (Windows)  │             │                 │
//! │                 └────────────┘             │                 │
//! │                                           ▼                 │
//! │  ┌─────────────────────────────────────────────────────────┐ │
//! │  │              Platform Layer                              │ │
//! │  │                                                          │ │
//! │  │  macOS: CGEvent + AXIsProcessTrusted + enigo             │ │
//! │  │  Windows: SendInput (KEYEVENTF_UNICODE) + enigo          │ │
//! │  └─────────────────────────────────────────────────────────┘ │
//! └──────────────────────────────────────────────────────────────┘
//! ```

mod config;
mod engine;
mod error;
mod platform;
mod ui;

use std::sync::{Arc, Mutex};

use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use log::{error, info};
use tray_icon::{
    TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
use winit::event_loop::{ControlFlow, EventLoop};

use config::Config;
use engine::TypeEngine;

fn main() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    info!("TypePaste v{} starting", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let config = Config::load().unwrap_or_else(|e| {
        error!("Failed to load config: {e}, using defaults");
        Config::default()
    });
    info!(
        "Config loaded: delay={}ms, random={}..{}ms, initial={}ms, hotkey={}",
        config.keystroke_delay_ms,
        config.random_delay_min_ms,
        config.random_delay_max_ms,
        config.initial_delay_ms,
        config.hotkey
    );

    // Create the engine
    let engine = Arc::new(Mutex::new(TypeEngine::new(config.clone())));

    // ── Event loop (must be created before tray on macOS) ──
    let event_loop = EventLoop::new().expect("Failed to create event loop");

    // ── System Tray ──
    let menu = Menu::new();
    let paste_item = MenuItem::new("Paste as Keystrokes", true, None);
    let settings_item = MenuItem::new("Settings...", true, None);
    let separator = PredefinedMenuItem::separator();
    let quit_item = MenuItem::new("Quit TypePaste", true, None);

    menu.append(&paste_item).unwrap();
    menu.append(&settings_item).unwrap();
    menu.append(&separator).unwrap();
    menu.append(&quit_item).unwrap();

    let paste_item_id = paste_item.id().clone();
    let settings_item_id = settings_item.id().clone();
    let quit_item_id = quit_item.id().clone();

    // Tray icon — use a simple embedded icon or load from file
    let icon = load_tray_icon();

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("TypePaste — Paste as Keystrokes")
        .with_icon(icon)
        .build()
        .expect("Failed to create tray icon");

    // ── Global Hotkey ──
    let hotkey_manager = GlobalHotKeyManager::new().expect("Failed to init hotkey manager");

    // Parse hotkey from config (default: Cmd+Shift+V on macOS, Ctrl+Shift+V on Windows)
    let hotkey = parse_hotkey(&config.hotkey);
    if let Some(hk) = &hotkey {
        hotkey_manager
            .register(*hk)
            .expect("Failed to register global hotkey");
        info!("Global hotkey registered: {}", config.hotkey);
    }

    // ── Main Event Loop ──
    let engine_for_tray = engine.clone();
    let engine_for_hotkey = engine.clone();
    let engine_for_settings = engine.clone();

    let menu_channel = MenuEvent::receiver();
    let hotkey_channel = GlobalHotKeyEvent::receiver();

    info!(
        "TypePaste is running. Use {} or tray menu to paste.",
        config.hotkey
    );

    event_loop
        .run(move |_event, event_loop_window_target| {
            event_loop_window_target.set_control_flow(ControlFlow::Poll);

            // Handle menu events
            if let Ok(event) = menu_channel.try_recv() {
                if event.id() == &paste_item_id {
                    trigger_paste(&engine_for_tray);
                } else if event.id() == &settings_item_id {
                    info!("Opening settings window");
                    ui::settings::open_settings_window(engine_for_settings.clone());
                } else if event.id() == &quit_item_id {
                    info!("Quit requested via tray menu");
                    std::process::exit(0);
                }
            }

            // Handle hotkey events
            if let Ok(event) = hotkey_channel.try_recv() {
                if event.state() == HotKeyState::Pressed {
                    trigger_paste(&engine_for_hotkey);
                }
            }

            // Prevent busy-looping: sleep briefly
            std::thread::sleep(std::time::Duration::from_millis(16));
        })
        .expect("Event loop error");
}

/// Trigger the paste-as-keystrokes action.
fn trigger_paste(engine: &Arc<Mutex<TypeEngine>>) {
    info!("Paste as keystrokes triggered");
    let engine = engine.lock().unwrap();
    if let Err(e) = engine.paste_as_keystrokes() {
        error!("Failed to paste: {e}");
    }
}

/// Parse a hotkey string like "Cmd+Shift+V" or "Ctrl+Shift+V" into a HotKey.
fn parse_hotkey(s: &str) -> Option<HotKey> {
    use global_hotkey::hotkey::{Code, Modifiers};

    let parts: Vec<&str> = s.split('+').map(|p| p.trim()).collect();
    let mut modifiers = Modifiers::empty();
    let mut code = None;

    for part in &parts {
        match part.to_lowercase().as_str() {
            "cmd" | "command" | "meta" | "super" => modifiers |= Modifiers::META,
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "shift" => modifiers |= Modifiers::SHIFT,
            "alt" | "option" => modifiers |= Modifiers::ALT,
            key => {
                // Single character key
                if key.len() == 1 {
                    let ch = key.chars().next().unwrap().to_ascii_uppercase();
                    code = match ch {
                        'A' => Some(Code::KeyA),
                        'B' => Some(Code::KeyB),
                        'C' => Some(Code::KeyC),
                        'D' => Some(Code::KeyD),
                        'E' => Some(Code::KeyE),
                        'F' => Some(Code::KeyF),
                        'G' => Some(Code::KeyG),
                        'H' => Some(Code::KeyH),
                        'I' => Some(Code::KeyI),
                        'J' => Some(Code::KeyJ),
                        'K' => Some(Code::KeyK),
                        'L' => Some(Code::KeyL),
                        'M' => Some(Code::KeyM),
                        'N' => Some(Code::KeyN),
                        'O' => Some(Code::KeyO),
                        'P' => Some(Code::KeyP),
                        'Q' => Some(Code::KeyQ),
                        'R' => Some(Code::KeyR),
                        'S' => Some(Code::KeyS),
                        'T' => Some(Code::KeyT),
                        'U' => Some(Code::KeyU),
                        'V' => Some(Code::KeyV),
                        'W' => Some(Code::KeyW),
                        'X' => Some(Code::KeyX),
                        'Y' => Some(Code::KeyY),
                        'Z' => Some(Code::KeyZ),
                        _ => None,
                    };
                }
            }
        }
    }

    code.map(|c| HotKey::new(Some(modifiers), c))
}

/// Load or generate the tray icon.
fn load_tray_icon() -> tray_icon::Icon {
    // Generate a simple 32x32 RGBA icon (clipboard with keyboard overlay)
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    // Simple clipboard icon: white rectangle with dark border
    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let in_border = x < 2 || x >= size - 2 || y < 2 || y >= size - 2;
            let in_clip = x >= 10 && x <= 22 && y >= 0 && y <= 6;
            let in_body = x >= 4 && x <= 28 && y >= 4 && y <= 28;

            if in_clip {
                // Clip part at top — dark
                rgba[idx] = 100;     // R
                rgba[idx + 1] = 100; // G
                rgba[idx + 2] = 100; // B
                rgba[idx + 3] = 255; // A
            } else if in_body && in_border {
                // Border
                rgba[idx] = 60;
                rgba[idx + 1] = 60;
                rgba[idx + 2] = 60;
                rgba[idx + 3] = 255;
            } else if in_body {
                // Body — light
                rgba[idx] = 220;
                rgba[idx + 1] = 220;
                rgba[idx + 2] = 240;
                rgba[idx + 3] = 255;
            } else {
                // Transparent
                rgba[idx + 3] = 0;
            }
        }
    }

    tray_icon::Icon::from_rgba(rgba, size, size).expect("Failed to create tray icon")
}
