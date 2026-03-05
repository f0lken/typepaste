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

    /// Get a reference to the current config.
    pub fn config(&self) -> &Config {
        &self.config
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

        // 3. Type the text
        self.type_text_internal(&text, true)?;

        info!("Done typing");
        Ok(())
    }

    /// Type arbitrary text via keystroke emulation (CLI mode \u2014 text from argument/stdin).
    ///
    /// If `use_initial_delay` is true, waits `initial_delay_ms` before typing.
    pub fn type_text(&self, text: &str, use_initial_delay: bool) -> Result<()> {
        // Check accessibility
        platform::check_accessibility()?;

        if text.is_empty() {
            info!("Empty text, nothing to type");
            return Ok(());
        }

        self.type_text_internal(text, use_initial_delay)?;

        info!("Done typing");
        Ok(())
    }

    /// Focus a window by title, then type text into it.
    pub fn type_text_to_window_by_title(
        &self,
        text: &str,
        window_title: &str,
        use_initial_delay: bool,
    ) -> Result<()> {
        platform::check_accessibility()?;

        // Focus the target window
        let (pid, actual_title) = platform::focus_window_by_title(window_title)?;
        info!(
            "Focused window \"{}\" (PID {}) \u2014 typing {} chars",
            actual_title,
            pid,
            text.len()
        );

        self.type_text_internal(text, use_initial_delay)
    }

    /// Focus a window by PID, then type text into it.
    pub fn type_text_to_window_by_pid(
        &self,
        text: &str,
        pid: u32,
        use_initial_delay: bool,
    ) -> Result<()> {
        platform::check_accessibility()?;

        // Focus the target window
        let win_title = platform::focus_window_by_pid(pid)?;
        info!(
            "Focused PID {} window \"{}\" \u2014 typing {} chars",
            pid, win_title, text.len()
        );

        self.type_text_internal(text, use_initial_delay)
    }

    /// Internal: validate, log, delay, and type.
    fn type_text_internal(&self, text: &str, use_initial_delay: bool) -> Result<()> {
        // Safety: limit text length
        let text = if text.len() > self.config.max_text_length {
            warn!(
                "Text length {} exceeds maximum {}, truncating",
                text.len(),
                self.config.max_text_length
            );
            &text[..self.config.max_text_length]
        } else {
            text
        };

        // Log delay info
        if self.config.has_random_delay() {
            info!(
                "Typing {} chars | base delay: {}ms | random jitter: {}..{}ms",
                text.len(),
                self.config.keystroke_delay_ms,
                self.config.random_delay_min_ms,
                self.config.random_delay_max_ms
            );
        } else {
            info!(
                "Typing {} chars | fixed delay: {}ms",
                text.len(),
                self.config.keystroke_delay_ms
            );
        }

        // Initial delay \u2014 let user focus the target window (or window settle after focus switch)
        if use_initial_delay && self.config.initial_delay_ms > 0 {
            debug!(
                "Waiting {}ms before starting...",
                self.config.initial_delay_ms
            );
            std::thread::sleep(std::time::Duration::from_millis(self.config.initial_delay_ms));
        }

        // Type each character
        platform::type_string(text, &self.config)?;

        Ok(())
    }
}
