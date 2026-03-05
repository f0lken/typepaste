use arboard::Clipboard;
use log::{debug, info, warn};

use crate::config::Config;
use crate::error::{Result, TypePasteError};
use crate::platform;

/// Core engine: reads clipboard and types it out via keystroke emulation.
pub struct TypeEngine {
    config: Config,
}

impl TypeEngine {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Update the configuration at runtime.
    pub fn update_config(&mut self, config: Config) {
        self.config = config;
    }

    /// Read current clipboard text content.
    pub fn read_clipboard(&self) -> Result<String> {
        let mut clipboard = Clipboard::new()
            .map_err(|e| TypePasteError::Clipboard(format!("Init clipboard: {e}")))?;
        clipboard
            .get_text()
            .map_err(|e| TypePasteError::Clipboard(format!("Read clipboard: {e}")))
    }

    /// Main action: read clipboard and type it out as keystrokes.
    pub fn paste_as_keystrokes(&self) -> Result<()> {
        // 1. Check accessibility permissions (macOS-specific, noop on Windows)
        platform::check_accessibility()?;

        // 2. Read clipboard
        let text = self.read_clipboard()?;
        if text.is_empty() {
            info!("Clipboard is empty, nothing to type");
            return Ok(());
        }

        // 3. Safety: limit text length
        if text.len() > self.config.max_text_length {
            warn!(
                "Text length {} exceeds maximum {}, truncating",
                text.len(),
                self.config.max_text_length
            );
        }
        let text = if text.len() > self.config.max_text_length {
            &text[..self.config.max_text_length]
        } else {
            &text
        };

        info!("Typing {} characters as keystrokes", text.len());

        // 4. Initial delay — let user focus the target window
        if self.config.initial_delay_ms > 0 {
            debug!(
                "Waiting {}ms before starting...",
                self.config.initial_delay_ms
            );
            std::thread::sleep(std::time::Duration::from_millis(self.config.initial_delay_ms));
        }

        // 5. Type each character
        platform::type_string(text, &self.config)?;

        info!("Done typing");
        Ok(())
    }
}
