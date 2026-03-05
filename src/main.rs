//! TypePaste — paste text anywhere via keystroke emulation.
//!
//! Works even in remote desktops, VMs, and applications that don't support
//! direct clipboard paste (Ctrl+V / Cmd+V).
//!
//! ## Modes
//!
//! - **Tray mode** (default, no subcommand): system tray + global hotkey
//! - **CLI mode** (subcommands): headless, scriptable, MCP-ready
//!
//! ```bash
//! # Tray mode
//! typepaste
//!
//! # CLI mode
//! typepaste type "Hello, World!"
//! typepaste type --clipboard --window "Terminal"
//! typepaste list-windows --json
//! typepaste settings
//! ```

mod cli;
mod config;
mod engine;
mod error;
mod platform;
mod ui;

use std::io::Read;
use std::sync::{Arc, Mutex};

use clap::Parser;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use log::{error, info, warn};
use tray_icon::{
    TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
use winit::event_loop::{ControlFlow, EventLoop};

use cli::{Cli, Command};
use config::Config;
use engine::TypeEngine;

fn main() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    info!("TypePaste v{} starting", env!("CARGO_PKG_VERSION"));

    // Parse CLI arguments
    let cli = Cli::parse();

    // Load configuration
    let config = Config::load().unwrap_or_else(|e| {
        error!("Failed to load config: {e}, using defaults");
        Config::default()
    });

    // Dispatch: subcommand → CLI mode, no subcommand → tray mode
    match cli.command {
        Some(command) => run_cli(command, config),
        None => run_tray(config),
    }
}

// ═══════════════════════════════════════════════════════════════════
// CLI MODE
// ═══════════════════════════════════════════════════════════════════

fn run_cli(command: Command, mut config: Config) {
    match command {
        Command::Type(args) => {
            // Resolve text from: argument > --clipboard > --stdin
            let text = if let Some(ref t) = args.text {
                t.clone()
            } else if args.clipboard {
                let engine = TypeEngine::new(config.clone());
                match engine.read_clipboard() {
                    Ok(t) => t,
                    Err(e) => {
                        error!("Failed to read clipboard: {e}");
                        std::process::exit(1);
                    }
                }
            } else if args.stdin {
                let mut buf = String::new();
                if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                    error!("Failed to read stdin: {e}");
                    std::process::exit(1);
                }
                buf
            } else {
                error!("No text source specified. Provide text as argument, or use --clipboard / --stdin");
                std::process::exit(1);
            };

            if text.is_empty() {
                info!("Empty text, nothing to type");
                return;
            }

            // Apply CLI delay overrides
            if let Some(delay) = args.delay {
                config.keystroke_delay_ms = delay;
            }
            if let Some(min) = args.random_min {
                config.random_delay_min_ms = min;
            }
            if let Some(max) = args.random_max {
                config.random_delay_max_ms = max;
            }
            if let Some(initial) = args.initial_delay {
                config.initial_delay_ms = initial;
            }
            let use_initial_delay = !args.no_delay;

            let engine = TypeEngine::new(config);

            // Dispatch based on window targeting
            let result = if let Some(ref window_title) = args.window {
                engine.type_text_to_window_by_title(&text, window_title, use_initial_delay)
            } else if let Some(pid) = args.pid {
                engine.type_text_to_window_by_pid(&text, pid, use_initial_delay)
            } else {
                engine.type_text(&text, use_initial_delay)
            };

            if let Err(e) = result {
                error!("Failed to type text: {e}");
                std::process::exit(1);
            }
        }

        Command::ListWindows(args) => {
            match platform::list_windows() {
                Ok(windows) => {
                    if args.json {
                        // JSON output for programmatic consumption (MCP server)
                        let json = serde_json::to_string_pretty(&windows)
                            .expect("Failed to serialize windows");
                        println!("{json}");
                    } else {
                        // Human-readable table
                        if windows.is_empty() {
                            println!("No visible windows found.");
                            return;
                        }
                        println!(
                            "{:<8} {:<24} {}",
                            "PID", "APP", "TITLE"
                        );
                        println!("{}", "-".repeat(72));
                        for w in &windows {
                            println!(
                                "{:<8} {:<24} {}",
                                w.pid, w.app_name, w.title
                            );
                        }
                        println!("\nTotal: {} window(s)", windows.len());
                    }
                }
                Err(e) => {
                    error!("Failed to list windows: {e}");
                    std::process::exit(1);
                }
            }
        }

        Command::Settings => {
            info!("Opening settings window");
            let engine = Arc::new(Mutex::new(TypeEngine::new(config)));
            ui::settings::open_settings_window(engine);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// TRAY MODE (default)
// ═══════════════════════════════════════════════════════════════════

/// State for tracking registered hotkeys so we can re-register on config change.
struct HotkeyState {
    current_hotkey_str: String,
    current_hotkey: Option<HotKey>,
    current_paste_hotkey_str: String,
    current_paste_hotkey: Option<HotKey>,
}

fn run_tray(config: Config) {
    info!(
        "Config loaded: delay={}ms, random={}..{}ms, initial={}ms, hotkey={}, paste_hotkey={}",
        config.keystroke_delay_ms,
        config.random_delay_min_ms,
        config.random_delay_max_ms,
        config.initial_delay_ms,
        config.hotkey,
        if config.paste_hotkey.is_empty() { "(none)" } else { &config.paste_hotkey }
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

    // Tray icon
    let icon = load_tray_icon();

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("TypePaste — Paste as Keystrokes")
        .with_icon(icon)
        .build()
        .expect("Failed to create tray icon");

    // ── Global Hotkeys ──
    let hotkey_manager = GlobalHotKeyManager::new().expect("Failed to init hotkey manager");

    // Register primary hotkey
    let primary_hk = parse_hotkey(&config.hotkey);
    if let Some(hk) = &primary_hk {
        if let Err(e) = hotkey_manager.register(*hk) {
            warn!("Failed to register primary hotkey '{}': {e}", config.hotkey);
        } else {
            info!("Primary hotkey registered: {}", config.hotkey);
        }
    }

    // Register secondary paste hotkey (if configured)
    let paste_hk = if config.has_paste_hotkey() {
        let hk = parse_hotkey(&config.paste_hotkey);
        if let Some(ref hk) = hk {
            if let Err(e) = hotkey_manager.register(*hk) {
                warn!("Failed to register paste hotkey '{}': {e}", config.paste_hotkey);
            } else {
                info!("Paste hotkey registered: {}", config.paste_hotkey);
            }
        }
        hk
    } else {
        None
    };

    let mut hk_state = HotkeyState {
        current_hotkey_str: config.hotkey.clone(),
        current_hotkey: primary_hk,
        current_paste_hotkey_str: config.paste_hotkey.clone(),
        current_paste_hotkey: paste_hk,
    };

    // ── Main Event Loop ──
    let engine_for_tray = engine.clone();
    let engine_for_hotkey = engine.clone();
    let engine_for_settings = engine.clone();
    let engine_for_hk_check = engine.clone();

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

            // Handle hotkey events — both primary and paste hotkey trigger paste
            if let Ok(event) = hotkey_channel.try_recv() {
                if event.state() == HotKeyState::Pressed {
                    let id = event.id();
                    let is_primary = hk_state.current_hotkey.map_or(false, |hk| hk.id() == id);
                    let is_paste = hk_state.current_paste_hotkey.map_or(false, |hk| hk.id() == id);
                    if is_primary || is_paste {
                        trigger_paste(&engine_for_hotkey);
                    }
                }
            }

            // ── Hot-reload hotkeys on config change ──
            // Check if the engine's config has changed hotkeys (from settings GUI)
            if let Ok(engine) = engine_for_hk_check.lock() {
                let cfg = engine.config();
                reregister_hotkeys_if_changed(&hotkey_manager, &mut hk_state, cfg);
            }

            // Prevent busy-looping: sleep briefly
            std::thread::sleep(std::time::Duration::from_millis(16));
        })
        .expect("Event loop error");
}

/// Check if hotkeys in config differ from currently registered ones, and re-register if needed.
fn reregister_hotkeys_if_changed(
    manager: &GlobalHotKeyManager,
    state: &mut HotkeyState,
    config: &Config,
) {
    // Primary hotkey
    if config.hotkey != state.current_hotkey_str {
        info!(
            "Hotkey changed: '{}' → '{}', re-registering",
            state.current_hotkey_str, config.hotkey
        );
        // Unregister old
        if let Some(old) = state.current_hotkey {
            let _ = manager.unregister(old);
        }
        // Register new
        let new_hk = parse_hotkey(&config.hotkey);
        if let Some(hk) = &new_hk {
            if let Err(e) = manager.register(*hk) {
                warn!("Failed to register new hotkey '{}': {e}", config.hotkey);
            } else {
                info!("New primary hotkey registered: {}", config.hotkey);
            }
        }
        state.current_hotkey_str = config.hotkey.clone();
        state.current_hotkey = new_hk;
    }

    // Secondary paste hotkey
    if config.paste_hotkey != state.current_paste_hotkey_str {
        info!(
            "Paste hotkey changed: '{}' → '{}', re-registering",
            if state.current_paste_hotkey_str.is_empty() { "(none)" } else { &state.current_paste_hotkey_str },
            if config.paste_hotkey.is_empty() { "(none)" } else { &config.paste_hotkey }
        );
        // Unregister old
        if let Some(old) = state.current_paste_hotkey {
            let _ = manager.unregister(old);
        }
        // Register new
        let new_hk = if config.has_paste_hotkey() {
            let hk = parse_hotkey(&config.paste_hotkey);
            if let Some(ref hk) = hk {
                if let Err(e) = manager.register(*hk) {
                    warn!("Failed to register paste hotkey '{}': {e}", config.paste_hotkey);
                } else {
                    info!("New paste hotkey registered: {}", config.paste_hotkey);
                }
            }
            hk
        } else {
            None
        };
        state.current_paste_hotkey_str = config.paste_hotkey.clone();
        state.current_paste_hotkey = new_hk;
    }
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

    let s = s.trim();
    if s.is_empty() {
        return None;
    }

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
                        '0' => Some(Code::Digit0),
                        '1' => Some(Code::Digit1),
                        '2' => Some(Code::Digit2),
                        '3' => Some(Code::Digit3),
                        '4' => Some(Code::Digit4),
                        '5' => Some(Code::Digit5),
                        '6' => Some(Code::Digit6),
                        '7' => Some(Code::Digit7),
                        '8' => Some(Code::Digit8),
                        '9' => Some(Code::Digit9),
                        _ => None,
                    };
                } else {
                    // Named keys: F1-F12, Space, Enter, etc.
                    code = match key {
                        "f1" => Some(Code::F1),
                        "f2" => Some(Code::F2),
                        "f3" => Some(Code::F3),
                        "f4" => Some(Code::F4),
                        "f5" => Some(Code::F5),
                        "f6" => Some(Code::F6),
                        "f7" => Some(Code::F7),
                        "f8" => Some(Code::F8),
                        "f9" => Some(Code::F9),
                        "f10" => Some(Code::F10),
                        "f11" => Some(Code::F11),
                        "f12" => Some(Code::F12),
                        "space" => Some(Code::Space),
                        "enter" | "return" => Some(Code::Enter),
                        "tab" => Some(Code::Tab),
                        "backspace" => Some(Code::Backspace),
                        "delete" | "del" => Some(Code::Delete),
                        "escape" | "esc" => Some(Code::Escape),
                        "home" => Some(Code::Home),
                        "end" => Some(Code::End),
                        "pageup" => Some(Code::PageUp),
                        "pagedown" => Some(Code::PageDown),
                        "up" => Some(Code::ArrowUp),
                        "down" => Some(Code::ArrowDown),
                        "left" => Some(Code::ArrowLeft),
                        "right" => Some(Code::ArrowRight),
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
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let in_border = x < 2 || x >= size - 2 || y < 2 || y >= size - 2;
            let in_clip = x >= 10 && x <= 22 && y <= 6;
            let in_body = x >= 4 && x <= 28 && y >= 4 && y <= 28;

            if in_clip {
                rgba[idx] = 100;
                rgba[idx + 1] = 100;
                rgba[idx + 2] = 100;
                rgba[idx + 3] = 255;
            } else if in_body && in_border {
                rgba[idx] = 60;
                rgba[idx + 1] = 60;
                rgba[idx + 2] = 60;
                rgba[idx + 3] = 255;
            } else if in_body {
                rgba[idx] = 220;
                rgba[idx + 1] = 220;
                rgba[idx + 2] = 240;
                rgba[idx + 3] = 255;
            } else {
                rgba[idx + 3] = 0;
            }
        }
    }

    tray_icon::Icon::from_rgba(rgba, size, size).expect("Failed to create tray icon")
}
