use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::error::{Result, TypePasteError};

/// Defines a keyboard layout with its name and the Unicode code-point ranges
/// it covers. When a character falls into one of these ranges, TypePaste
/// knows that this layout should be active on the remote system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutDefinition {
    /// Human-readable name (e.g. "English", "Russian").
    pub name: String,

    /// Unicode ranges that belong to this layout.
    /// Each pair is `[start, end]` inclusive.
    /// Example for Basic Latin letters: `[[0x0041, 0x005A], [0x0061, 0x007A]]`
    pub unicode_ranges: Vec<[u32; 2]>,
}

/// Configuration for automatic keyboard layout switching on the remote system.
///
/// When typing mixed-script text (e.g. Russian + English), the remote system
/// requires pressing a hotkey to switch between keyboard layouts. TypePaste
/// detects script boundary crossings and emits the configured hotkey
/// automatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutSwitchConfig {
    /// Master switch: enable or disable layout switching logic.
    pub enabled: bool,

    /// Hotkey to press when switching layouts on the remote system.
    /// Format: "Modifier+Modifier+Key", e.g. "Alt+Shift", "Ctrl+Shift",
    /// "Win+Space".  Uses the same format as the main hotkey config.
    pub switch_hotkey: String,

    /// Delay in milliseconds to wait after pressing the layout switch hotkey.
    /// The remote OS needs a moment to actually switch the layout before
    /// the next character is typed. 50–150 ms is usually enough.
    pub switch_delay_ms: u64,

    /// The ordered list of layouts.  The order matters: TypePaste cycles
    /// through them using the switch hotkey (one press = next layout).
    /// The first layout in the list is the assumed initial layout when
    /// typing starts.
    pub layouts: Vec<LayoutDefinition>,
}

impl Default for LayoutSwitchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            switch_hotkey: "Alt+Shift".to_string(),
            switch_delay_ms: 100,
            layouts: vec![
                LayoutDefinition {
                    name: "English".to_string(),
                    unicode_ranges: vec![
                        [0x0041, 0x005A], // A-Z
                        [0x0061, 0x007A], // a-z
                    ],
                },
                LayoutDefinition {
                    name: "Russian".to_string(),
                    unicode_ranges: vec![
                        [0x0400, 0x04FF], // Cyrillic block
                    ],
                },
            ],
        }
    }
}

impl LayoutSwitchConfig {
    /// Determine which layout index a character belongs to.
    /// Returns `None` for characters not claimed by any layout
    /// (digits, punctuation, whitespace, etc.) — those are "neutral"
    /// and should not trigger a layout switch.
    pub fn layout_for_char(&self, ch: char) -> Option<usize> {
        let cp = ch as u32;
        for (idx, layout) in self.layouts.iter().enumerate() {
            for &[start, end] in &layout.unicode_ranges {
                if cp >= start && cp <= end {
                    return Some(idx);
                }
            }
        }
        None
    }

    /// Calculate how many hotkey presses are needed to go from
    /// `from_idx` to `to_idx` in a cyclic layout list.
    pub fn presses_needed(&self, from_idx: usize, to_idx: usize) -> usize {
        let n = self.layouts.len();
        if n == 0 {
            return 0;
        }
        if to_idx >= from_idx {
            to_idx - from_idx
        } else {
            n - from_idx + to_idx
        }
    }
}

/// Application configuration persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Base delay between individual keystrokes in milliseconds.
    /// Applied to every keystroke as a fixed minimum interval.
    pub keystroke_delay_ms: u64,

    /// Minimum random delay added on top of the base delay (ms).
    /// Set both min and max to 0 to disable random jitter.
    pub random_delay_min_ms: u64,

    /// Maximum random delay added on top of the base delay (ms).
    /// Each keystroke gets: base + rand(min..=max) total delay.
    /// This simulates human-like typing cadence and can help with
    /// systems that detect automated input.
    pub random_delay_max_ms: u64,

    /// Initial delay before starting to type (ms).
    /// Gives user time to focus the target window.
    pub initial_delay_ms: u64,

    /// Whether to show a notification before typing starts.
    pub show_notification: bool,

    /// Whether to start the app on system login.
    pub start_on_login: bool,

    /// Global hotkey to paste clipboard as keystrokes in tray mode.
    /// Format: "Modifier+Modifier+Key", e.g. "Cmd+Shift+V", "Ctrl+Shift+V".
    /// Changes are applied immediately without restart.
    pub hotkey: String,

    /// Optional additional hotkey for paste-as-keystrokes.
    /// Allows registering a second shortcut alongside the primary one.
    /// Leave empty to disable. Same format as `hotkey`.
    #[serde(default)]
    pub paste_hotkey: String,

    /// Maximum text length to process (safety limit).
    pub max_text_length: usize,

    /// Whether to handle newlines as Enter key presses.
    pub newlines_as_enter: bool,

    /// Whether to handle tabs as Tab key presses.
    pub tabs_as_tab: bool,

    /// Keyboard layout switching configuration for remote systems.
    #[serde(default)]
    pub layout_switch: LayoutSwitchConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keystroke_delay_ms: 5,
            random_delay_min_ms: 0,
            random_delay_max_ms: 0,
            initial_delay_ms: 500,
            show_notification: true,
            start_on_login: false,
            #[cfg(target_os = "macos")]
            hotkey: "Cmd+Shift+V".to_string(),
            #[cfg(not(target_os = "macos"))]
            hotkey: "Ctrl+Shift+V".to_string(),
            paste_hotkey: String::new(),
            max_text_length: 100_000,
            newlines_as_enter: true,
            tabs_as_tab: true,
            layout_switch: LayoutSwitchConfig::default(),
        }
    }
}

impl Config {
    /// Returns the path to the config file.
    pub fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| TypePasteError::Config("Cannot determine config directory".into()))?;
        let app_dir = config_dir.join("typepaste");
        Ok(app_dir.join("config.json"))
    }

    /// Load configuration from disk, or create default if not found.
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if path.exists() {
            let contents = fs::read_to_string(&path)?;
            serde_json::from_str(&contents)
                .map_err(|e| TypePasteError::Config(format!("Parse error: {e}")))
        } else {
            let config = Self::default();
            config.save()?;
            Ok(config)
        }
    }

    /// Persist current configuration to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| TypePasteError::Config(format!("Serialize error: {e}")))?;
        fs::write(&path, json)?;
        Ok(())
    }

    /// Whether random delay jitter is enabled.
    pub fn has_random_delay(&self) -> bool {
        self.random_delay_max_ms > 0
    }

    /// Whether a secondary paste hotkey is configured.
    pub fn has_paste_hotkey(&self) -> bool {
        !self.paste_hotkey.trim().is_empty()
    }

    /// Validate configuration values and fix inconsistencies.
    pub fn validate(&mut self) {
        // Ensure min <= max for random delay
        if self.random_delay_min_ms > self.random_delay_max_ms {
            std::mem::swap(&mut self.random_delay_min_ms, &mut self.random_delay_max_ms);
        }
    }
}
