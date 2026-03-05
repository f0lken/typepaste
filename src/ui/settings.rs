//! Settings window implemented with eframe/egui.
//!
//! Provides a cross-platform GUI for editing TypePaste configuration.
//! Changes are applied immediately and persisted to disk.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};
use log::{error, info};

use crate::config::{Config, LayoutDefinition};
use crate::engine::TypeEngine;

/// Open the settings window. Blocks until the window is closed.
pub fn open_settings_window(engine: Arc<Mutex<TypeEngine>>) {
    let config = {
        let engine = engine.lock().unwrap();
        engine.config().clone()
    };

    let app = SettingsApp::new(config, engine);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TypePaste Settings")
            .with_inner_size([520.0, 700.0])
            .with_resizable(true),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "TypePaste Settings",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    ) {
        error!("Settings window error: {e}");
    }
}

// ─── App state ─────────────────────────────────────────────────────────────────

struct SettingsApp {
    config: Config,
    engine: Arc<Mutex<TypeEngine>>,
    status_message: Option<String>,
    status_is_error: bool,
    // Layout switching UI state
    new_layout_name: String,
    new_layout_ranges: String,
}

impl SettingsApp {
    fn new(config: Config, engine: Arc<Mutex<TypeEngine>>) -> Self {
        Self {
            config,
            engine,
            status_message: None,
            status_is_error: false,
            new_layout_name: String::new(),
            new_layout_ranges: String::new(),
        }
    }

    fn save_config(&mut self) {
        self.config.validate();
        match self.config.save() {
            Ok(_) => {
                info!("Settings saved");
                // Update engine config
                if let Ok(mut engine) = self.engine.lock() {
                    engine.update_config(self.config.clone());
                }
                self.status_message = Some("Settings saved.".into());
                self.status_is_error = false;
            }
            Err(e) => {
                error!("Failed to save settings: {e}");
                self.status_message = Some(format!("Error saving settings: {e}"));
                self.status_is_error = true;
            }
        }
    }
}

// ─── eframe App implementation ─────────────────────────────────────────────────

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("TypePaste Settings");
                ui.add_space(8.0);

                // ── Timing ──────────────────────────────────────────────────────
                ui.group(|ui| {
                    ui.label(RichText::new("Timing").strong());
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label("Keystroke delay (ms):");
                        ui.add(egui::DragValue::new(&mut self.config.keystroke_delay_ms)
                            .clamp_range(0u64..=1000u64));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Random delay min (ms):");
                        ui.add(egui::DragValue::new(&mut self.config.random_delay_min_ms)
                            .clamp_range(0u64..=1000u64));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Random delay max (ms):");
                        ui.add(egui::DragValue::new(&mut self.config.random_delay_max_ms)
                            .clamp_range(0u64..=1000u64));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Initial delay (ms):");
                        ui.add(egui::DragValue::new(&mut self.config.initial_delay_ms)
                            .clamp_range(0u64..=10000u64));
                    });
                });

                ui.add_space(8.0);

                // ── Hotkeys ──────────────────────────────────────────────────────
                ui.group(|ui| {
                    ui.label(RichText::new("Hotkeys").strong());
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label("Primary hotkey:");
                        ui.text_edit_singleline(&mut self.config.hotkey);
                    });
                    ui.label(RichText::new("  Format: Modifier+Key, e.g. Cmd+Shift+V")
                        .small().weak());

                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label("Secondary paste hotkey:");
                        ui.text_edit_singleline(&mut self.config.paste_hotkey);
                    });
                    ui.label(RichText::new("  Leave empty to disable.")
                        .small().weak());
                });

                ui.add_space(8.0);

                // ── Behavior ──────────────────────────────────────────────────────
                ui.group(|ui| {
                    ui.label(RichText::new("Behavior").strong());
                    ui.add_space(4.0);

                    ui.checkbox(&mut self.config.newlines_as_enter, "Newlines as Enter key");
                    ui.checkbox(&mut self.config.tabs_as_tab, "Tabs as Tab key");
                    ui.checkbox(&mut self.config.show_notification, "Show notification before typing");
                    ui.checkbox(&mut self.config.start_on_login, "Start on system login");

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Max text length:");
                        ui.add(egui::DragValue::new(&mut self.config.max_text_length)
                            .clamp_range(1000usize..=10_000_000usize));
                    });
                });

                ui.add_space(8.0);

                // ── Layout Switching ──────────────────────────────────────────────
                ui.group(|ui| {
                    ui.label(RichText::new("Keyboard Layout Switching (Remote Systems)").strong());
                    ui.add_space(4.0);
                    ui.label(RichText::new(
                        "Automatically press the switch hotkey when typing crosses a script boundary.\n\
                         E.g. typing mixed English+Russian text into a remote desktop."
                    ).small().weak());
                    ui.add_space(4.0);

                    ui.checkbox(
                        &mut self.config.layout_switch.enabled,
                        "Enable layout switching",
                    );

                    if self.config.layout_switch.enabled {
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("Switch hotkey:");
                            ui.text_edit_singleline(&mut self.config.layout_switch.switch_hotkey);
                        });
                        ui.label(RichText::new("  e.g. Alt+Shift, Ctrl+Shift, Win+Space")
                            .small().weak());

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("Switch delay (ms):");
                            ui.add(egui::DragValue::new(&mut self.config.layout_switch.switch_delay_ms)
                                .clamp_range(0u64..=2000u64));
                        });
                        ui.label(RichText::new("  Wait after pressing hotkey for remote OS to switch.")
                            .small().weak());

                        ui.add_space(8.0);
                        ui.label(RichText::new("Configured Layouts").strong());
                        ui.label(RichText::new("  Order matters: layouts cycle on each hotkey press.\n  First layout is assumed to be active when typing starts.")
                            .small().weak());
                        ui.add_space(4.0);

                        // Display current layouts
                        let mut remove_idx: Option<usize> = None;
                        let layouts_count = self.config.layout_switch.layouts.len();
                        for (i, layout) in self.config.layout_switch.layouts.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}. {}", i + 1, layout.name));
                                let ranges_str: Vec<String> = layout.unicode_ranges.iter()
                                    .map(|&[s, e]| format!("U+{:04X}–U+{:04X}", s, e))
                                    .collect();
                                ui.label(RichText::new(ranges_str.join(", ")).weak().small());
                                if layouts_count > 1 {
                                    if ui.small_button("✕").clicked() {
                                        remove_idx = Some(i);
                                    }
                                }
                            });
                        }
                        if let Some(idx) = remove_idx {
                            self.config.layout_switch.layouts.remove(idx);
                        }

                        ui.add_space(4.0);
                        ui.label(RichText::new("Add Layout:").strong());
                        ui.horizontal(|ui| {
                            ui.label("Name:");
                            ui.text_edit_singleline(&mut self.new_layout_name);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Unicode ranges (e.g. 0041-005A,0061-007A):");
                            ui.text_edit_singleline(&mut self.new_layout_ranges);
                        });
                        ui.label(RichText::new("  Format: START-END pairs, hex, comma-separated.")
                            .small().weak());

                        if ui.button("Add Layout").clicked() {
                            match parse_unicode_ranges(&self.new_layout_ranges) {
                                Ok(ranges) if !self.new_layout_name.is_empty() => {
                                    self.config.layout_switch.layouts.push(LayoutDefinition {
                                        name: self.new_layout_name.clone(),
                                        unicode_ranges: ranges,
                                    });
                                    self.new_layout_name.clear();
                                    self.new_layout_ranges.clear();
                                    self.status_message = Some("Layout added.".into());
                                    self.status_is_error = false;
                                }
                                Ok(_) => {
                                    self.status_message = Some("Layout name cannot be empty.".into());
                                    self.status_is_error = true;
                                }
                                Err(e) => {
                                    self.status_message = Some(format!("Invalid ranges: {e}"));
                                    self.status_is_error = true;
                                }
                            }
                        }
                    }
                });

                ui.add_space(12.0);

                // ── Status + Save ──────────────────────────────────────────────
                if let Some(ref msg) = self.status_message {
                    let color = if self.status_is_error {
                        egui::Color32::RED
                    } else {
                        egui::Color32::GREEN
                    };
                    ui.colored_label(color, msg);
                    ui.add_space(4.0);
                }

                if ui.button(RichText::new("Save Settings").strong()).clicked() {
                    self.save_config();
                }
            });
        });
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────────

/// Parse a comma-separated list of hex range strings like "0041-005A,0061-007A"
/// into a Vec<[u32; 2]>.
fn parse_unicode_ranges(input: &str) -> Result<Vec<[u32; 2]>, String> {
    let mut ranges = Vec::new();
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let parts: Vec<&str> = part.split('-').collect();
        if parts.len() != 2 {
            return Err(format!("Expected START-END, got '{part}'"));
        }
        let start = u32::from_str_radix(parts[0].trim(), 16)
            .map_err(|_| format!("Invalid hex: '{}'", parts[0]))?;
        let end = u32::from_str_radix(parts[1].trim(), 16)
            .map_err(|_| format!("Invalid hex: '{}'", parts[1]))?;
        if start > end {
            return Err(format!("Start {start:04X} > End {end:04X}"));
        }
        ranges.push([start, end]);
    }
    if ranges.is_empty() {
        return Err("No ranges specified".into());
    }
    Ok(ranges)
}
