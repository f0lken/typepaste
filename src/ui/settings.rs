//! Settings window implemented with eframe/egui.
//!
//! Opens a native window where the user can edit all TypePaste configuration
//! values. Changes are saved to disk and hot-reloaded into the running engine.
//! Hotkey changes are applied immediately (no restart required).

use std::sync::{Arc, Mutex};

use eframe::egui;
use log::{error, info};

use crate::config::Config;
use crate::engine::TypeEngine;

/// State for the settings window.
struct SettingsApp {
    /// Working copy of the config (editable in the UI).
    config: Config,
    /// Reference to the running engine for hot-reload.
    engine: Arc<Mutex<TypeEngine>>,
    /// Status message shown at the bottom.
    status: String,
    /// Mutable string buffers for numeric fields (egui needs &mut String).
    buf_keystroke_delay: String,
    buf_random_min: String,
    buf_random_max: String,
    buf_initial_delay: String,
    buf_max_text_length: String,
    buf_hotkey: String,
    buf_paste_hotkey: String,
}

impl SettingsApp {
    fn new(engine: Arc<Mutex<TypeEngine>>) -> Self {
        let config = Config::load().unwrap_or_default();
        let buf_keystroke_delay = config.keystroke_delay_ms.to_string();
        let buf_random_min = config.random_delay_min_ms.to_string();
        let buf_random_max = config.random_delay_max_ms.to_string();
        let buf_initial_delay = config.initial_delay_ms.to_string();
        let buf_max_text_length = config.max_text_length.to_string();
        let buf_hotkey = config.hotkey.clone();
        let buf_paste_hotkey = config.paste_hotkey.clone();
        Self {
            config,
            engine,
            status: String::new(),
            buf_keystroke_delay,
            buf_random_min,
            buf_random_max,
            buf_initial_delay,
            buf_max_text_length,
            buf_hotkey,
            buf_paste_hotkey,
        }
    }

    /// Sync string buffers back into the config struct.
    fn sync_buffers_to_config(&mut self) {
        if let Ok(v) = self.buf_keystroke_delay.trim().parse::<u64>() {
            self.config.keystroke_delay_ms = v;
        }
        if let Ok(v) = self.buf_random_min.trim().parse::<u64>() {
            self.config.random_delay_min_ms = v;
        }
        if let Ok(v) = self.buf_random_max.trim().parse::<u64>() {
            self.config.random_delay_max_ms = v;
        }
        if let Ok(v) = self.buf_initial_delay.trim().parse::<u64>() {
            self.config.initial_delay_ms = v;
        }
        if let Ok(v) = self.buf_max_text_length.trim().parse::<usize>() {
            self.config.max_text_length = v;
        }
        self.config.hotkey = self.buf_hotkey.trim().to_string();
        self.config.paste_hotkey = self.buf_paste_hotkey.trim().to_string();
        self.config.validate();
    }

    /// Reload string buffers from the config struct.
    fn sync_config_to_buffers(&mut self) {
        self.buf_keystroke_delay = self.config.keystroke_delay_ms.to_string();
        self.buf_random_min = self.config.random_delay_min_ms.to_string();
        self.buf_random_max = self.config.random_delay_max_ms.to_string();
        self.buf_initial_delay = self.config.initial_delay_ms.to_string();
        self.buf_max_text_length = self.config.max_text_length.to_string();
        self.buf_hotkey = self.config.hotkey.clone();
        self.buf_paste_hotkey = self.config.paste_hotkey.clone();
    }

    /// Save config to disk and update the running engine.
    fn save_and_apply(&mut self) {
        self.sync_buffers_to_config();
        match self.config.save() {
            Ok(()) => {
                // Hot-reload into the running engine
                // (the tray event loop detects config changes and re-registers hotkeys)
                if let Ok(mut engine) = self.engine.lock() {
                    engine.update_config(self.config.clone());
                }
                self.status = "✓ Saved and applied.".to_string();
                info!("Settings saved and applied");
            }
            Err(e) => {
                self.status = format!("✗ Save failed: {e}");
                error!("Failed to save settings: {e}");
            }
        }
    }

    /// Reset to defaults.
    fn reset_defaults(&mut self) {
        self.config = Config::default();
        self.sync_config_to_buffers();
        self.status = "Reset to defaults (not yet saved).".to_string();
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("TypePaste Settings");
            ui.add_space(8.0);

            // ── Delay Settings ──
            ui.group(|ui| {
                ui.strong("Delay Settings");
                ui.add_space(4.0);

                egui::Grid::new("delay_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Base keystroke delay (ms):");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.buf_keystroke_delay)
                                .desired_width(80.0),
                        );
                        ui.end_row();

                        ui.label("Initial delay before typing (ms):");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.buf_initial_delay)
                                .desired_width(80.0),
                        );
                        ui.end_row();
                    });
            });

            ui.add_space(8.0);

            // ── Random Jitter ──
            ui.group(|ui| {
                ui.strong("Random Jitter (human-like typing)");
                ui.add_space(4.0);
                ui.label("Each keystroke: base delay + random(min .. max)");
                ui.label("Set both to 0 to disable.");
                ui.add_space(4.0);

                egui::Grid::new("jitter_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Random delay min (ms):");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.buf_random_min)
                                .desired_width(80.0),
                        );
                        ui.end_row();

                        ui.label("Random delay max (ms):");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.buf_random_max)
                                .desired_width(80.0),
                        );
                        ui.end_row();
                    });

                // Show effective delay preview
                if let (Ok(base), Ok(min), Ok(max)) = (
                    self.buf_keystroke_delay.trim().parse::<u64>(),
                    self.buf_random_min.trim().parse::<u64>(),
                    self.buf_random_max.trim().parse::<u64>(),
                ) {
                    ui.add_space(4.0);
                    if max > 0 {
                        ui.label(format!(
                            "Effective delay: {}..{} ms per keystroke",
                            base + min,
                            base + max
                        ));
                    } else {
                        ui.label(format!("Effective delay: {} ms (fixed)", base));
                    }
                }
            });

            ui.add_space(8.0);

            // ── Hotkeys ──
            ui.group(|ui| {
                ui.strong("Hotkeys");
                ui.add_space(4.0);
                ui.label("Format: Modifier+Modifier+Key  (e.g. Cmd+Shift+V, Ctrl+Alt+P)");
                ui.label(
                    egui::RichText::new("Changes are applied immediately after Save.")
                        .small()
                        .weak(),
                );
                ui.add_space(4.0);

                egui::Grid::new("hotkey_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Primary hotkey:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.buf_hotkey)
                                .desired_width(160.0)
                                .hint_text("Cmd+Shift+V"),
                        );
                        ui.end_row();

                        ui.label("Additional hotkey:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.buf_paste_hotkey)
                                .desired_width(160.0)
                                .hint_text("(optional)"),
                        );
                        ui.end_row();
                    });

                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(
                        "Supported modifiers: Cmd/Ctrl/Shift/Alt/Option\n\
                         Supported keys: A-Z, 0-9, F1-F12, Space, Enter, Tab, Esc, \
                         Home, End, PageUp, PageDown, Up/Down/Left/Right, Delete, Backspace",
                    )
                    .small()
                    .weak(),
                );
            });

            ui.add_space(8.0);

            // ── Toggles ──
            ui.group(|ui| {
                ui.strong("Behavior");
                ui.add_space(4.0);

                ui.checkbox(&mut self.config.newlines_as_enter, "Convert newlines to Enter key");
                ui.checkbox(&mut self.config.tabs_as_tab, "Convert tabs to Tab key");
                ui.checkbox(&mut self.config.show_notification, "Show notification before typing");
                ui.checkbox(&mut self.config.start_on_login, "Start on system login");
            });

            ui.add_space(8.0);

            // ── Safety ──
            ui.group(|ui| {
                ui.strong("Safety");
                ui.add_space(4.0);

                egui::Grid::new("safety_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Max text length (chars):");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.buf_max_text_length)
                                .desired_width(100.0),
                        );
                        ui.end_row();
                    });
            });

            ui.add_space(12.0);

            // ── Buttons ──
            ui.horizontal(|ui| {
                if ui.button("   Save & Apply   ").clicked() {
                    self.save_and_apply();
                }
                if ui.button("Reset to Defaults").clicked() {
                    self.reset_defaults();
                }
            });

            // ── Status ──
            if !self.status.is_empty() {
                ui.add_space(8.0);
                ui.label(&self.status);
            }

            // ── Config path ──
            ui.add_space(12.0);
            ui.separator();
            if let Ok(path) = Config::config_path() {
                ui.label(
                    egui::RichText::new(format!("Config: {}", path.display()))
                        .small()
                        .weak(),
                );
            }
        });
    }
}

/// Open the settings window. Spawns a new thread so it doesn't block the tray event loop.
///
/// The `engine` reference is used for hot-reloading config changes.
pub fn open_settings_window(engine: Arc<Mutex<TypeEngine>>) {
    std::thread::spawn(move || {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("TypePaste Settings")
                .with_inner_size([440.0, 640.0])
                .with_min_inner_size([380.0, 500.0])
                .with_resizable(true),
            ..Default::default()
        };

        if let Err(e) = eframe::run_native(
            "TypePaste Settings",
            options,
            Box::new(move |_cc| Ok(Box::new(SettingsApp::new(engine)))),
        ) {
            error!("Settings window error: {e}");
        }
    });
}
