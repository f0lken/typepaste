//! CLI argument definitions using clap.
//!
//! This module defines all CLI subcommands and their arguments.
//! TypePaste supports both interactive (tray) mode and headless CLI mode.

use clap::{Args, Parser, Subcommand};

/// TypePaste — paste text anywhere as keystrokes.
///
/// Without a subcommand, TypePaste runs in system tray mode.
/// With a subcommand, it runs headlessly (useful for scripting and MCP).
#[derive(Debug, Parser)]
#[command(
    name = "typepaste",
    version,
    about = "Paste text anywhere as keystrokes — works in VMs, RDP, and restricted apps",
    long_about = None,
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Available CLI subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Type text as keystrokes into the focused window or a specific window.
    #[command(name = "type")]
    Type(TypeArgs),

    /// List all visible windows with their PID, app name, and title.
    #[command(name = "list-windows")]
    ListWindows(ListWindowsArgs),

    /// Open the settings window.
    #[command(name = "settings")]
    Settings,
}

/// Arguments for the `type` subcommand.
#[derive(Debug, Args)]
pub struct TypeArgs {
    /// Text to type. If omitted, reads from --clipboard or --stdin.
    pub text: Option<String>,

    /// Read text from the system clipboard.
    #[arg(long, conflicts_with_all = ["stdin"])]
    pub clipboard: bool,

    /// Read text from stdin.
    #[arg(long, conflicts_with_all = ["clipboard"])]
    pub stdin: bool,

    /// Target window by title (substring match, case-insensitive).
    #[arg(long, conflicts_with = "pid")]
    pub window: Option<String>,

    /// Target window by PID.
    #[arg(long, conflicts_with = "window")]
    pub pid: Option<u32>,

    /// Keystroke delay in milliseconds (overrides config).
    #[arg(long)]
    pub delay: Option<u64>,

    /// Minimum random delay in milliseconds (overrides config).
    #[arg(long)]
    pub random_min: Option<u64>,

    /// Maximum random delay in milliseconds (overrides config).
    #[arg(long)]
    pub random_max: Option<u64>,

    /// Initial delay before typing starts in milliseconds (overrides config).
    #[arg(long)]
    pub initial_delay: Option<u64>,

    /// Skip the initial delay entirely.
    #[arg(long)]
    pub no_delay: bool,

    /// Enable automatic keyboard layout switching for remote systems.
    /// TypePaste will emit the configured layout switch hotkey when it detects
    /// a script boundary (e.g. Latin → Cyrillic).
    #[arg(long)]
    pub layout_switch: bool,

    /// Hotkey used to switch keyboard layouts on the remote system.
    /// Example: "Alt+Shift", "Ctrl+Shift", "Win+Space".
    #[arg(long)]
    pub layout_switch_hotkey: Option<String>,

    /// Delay in milliseconds to wait after pressing the layout switch hotkey.
    /// The remote OS needs time to actually switch the layout. Default: 100.
    #[arg(long)]
    pub layout_switch_delay: Option<u64>,

    /// Initial layout index (0-based) to assume when typing starts.
    /// 0 = first layout in config (default), 1 = second, etc.
    /// Useful when the remote system is already in a non-default layout.
    #[arg(long)]
    pub initial_layout: Option<usize>,
}

/// Arguments for the `list-windows` subcommand.
#[derive(Debug, Args)]
pub struct ListWindowsArgs {
    /// Output in JSON format (for programmatic use / MCP).
    #[arg(long)]
    pub json: bool,
}
