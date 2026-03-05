//! CLI argument definitions using clap.
//!
//! TypePaste supports two modes:
//! - **Tray mode** (default, no subcommand): system tray + global hotkey
//! - **CLI mode** (subcommands): headless, scriptable, MCP-ready
//!
//! ## Examples
//!
//! ```bash
//! # Tray mode (default)
//! typepaste
//!
//! # Type text from argument
//! typepaste type "Hello, World!"
//!
//! # Type text from clipboard
//! typepaste type --clipboard
//!
//! # Type text from stdin (pipe)
//! echo "Hello" | typepaste type --stdin
//!
//! # Type into a specific window by title substring
//! typepaste type "ls -la" --window "Terminal"
//!
//! # Type into a specific window by PID
//! typepaste type "ls -la" --pid 12345
//!
//! # Override delays from CLI
//! typepaste type "slow text" --delay 50 --random-min 10 --random-max 80
//!
//! # List available windows (for --window / --pid discovery)
//! typepaste list-windows
//!
//! # JSON output for programmatic consumption (MCP server)
//! typepaste list-windows --json
//!
//! # Open settings GUI
//! typepaste settings
//! ```

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "typepaste",
    version,
    about = "Paste text anywhere via keystroke emulation",
    long_about = "TypePaste emits OS-level keystroke events to type text into any \
                  window \u2014 even remote desktops, VMs, and applications that block \
                  clipboard paste. Runs as a system tray app (default) or headless \
                  via CLI subcommands."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Type text via keystroke emulation (headless CLI mode).
    Type(TypeArgs),

    /// List visible windows with their titles and PIDs.
    ListWindows(ListWindowsArgs),

    /// Open the settings GUI window.
    Settings,
}

#[derive(Parser, Debug)]
pub struct TypeArgs {
    /// Text to type. If omitted, reads from --clipboard or --stdin.
    pub text: Option<String>,

    /// Read text from the system clipboard instead of argument.
    #[arg(long, short = 'c')]
    pub clipboard: bool,

    /// Read text from stdin (useful for piping).
    #[arg(long, short = 's')]
    pub stdin: bool,

    /// Focus a window by title substring before typing.
    /// Case-insensitive partial match.
    #[arg(long, short = 'w')]
    pub window: Option<String>,

    /// Focus a window by process ID before typing.
    #[arg(long, short = 'p')]
    pub pid: Option<u32>,

    /// Override base keystroke delay (ms).
    #[arg(long, short = 'd')]
    pub delay: Option<u64>,

    /// Override random jitter minimum (ms).
    #[arg(long)]
    pub random_min: Option<u64>,

    /// Override random jitter maximum (ms).
    #[arg(long)]
    pub random_max: Option<u64>,

    /// Override initial delay before typing starts (ms).
    #[arg(long)]
    pub initial_delay: Option<u64>,

    /// Skip initial delay entirely (useful in scripts).
    #[arg(long)]
    pub no_delay: bool,
}

#[derive(Parser, Debug)]
pub struct ListWindowsArgs {
    /// Output as JSON (for programmatic use / MCP server).
    #[arg(long, short = 'j')]
    pub json: bool,
}
