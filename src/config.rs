use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::error::{Result, TypePasteError};

/// Application configuration persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Delay between individual keystrokes in milliseconds.
    pub keystroke_delay_ms: u64,

    /// Initial delay before starting to type (ms).
    /// Gives user time to focus the target window.
    pub initial_delay_ms: u64,

    /// Whether to show a notification before typing starts.
    pub show_notification: bool,

    /// Whether to start the app on system login.
    pub start_on_login: bool,

    /// Hotkey string representation (e.g., "Ctrl+Shift+V" / "Cmd+Shift+V").
    pub hotkey: String,

    /// Maximum text length to process (safety limit).
    pub max_text_length: usize,

    /// Whether to handle newlines as Enter key presses.
    pub newlines_as_enter: bool,

    /// Whether to handle tabs as Tab key presses.
    pub tabs_as_tab: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keystroke_delay_ms: 5,
            initial_delay_ms: 500,
            show_notification: true,
            start_on_login: false,
            #[cfg(target_os = "macos")]
            hotkey: "Cmd+Shift+V".to_string(),
            #[cfg(not(target_os = "macos"))]
            hotkey: "Ctrl+Shift+V".to_string(),
            max_text_length: 100_000,
            newlines_as_enter: true,
            tabs_as_tab: true,
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
}
