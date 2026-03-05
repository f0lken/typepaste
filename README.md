# TypePaste

**Paste text anywhere via keystroke emulation** — works in VMs, RDP, and restricted apps.

TypePaste sits in your system tray and listens for a global hotkey. When triggered, it reads your clipboard and types the text character-by-character using low-level keyboard events. This bypasses clipboard restrictions in remote desktop sessions, virtual machines, KVM switches, locked-down applications, and anywhere `Ctrl+V` / `Cmd+V` doesn't work.

---

## What's New in v0.4.0 — Keyboard Layout Switching

v0.4.0 adds automatic keyboard layout switching for remote systems.
When typing mixed-script text (e.g. English + Russian), the remote OS requires
a hotkey press to switch the keyboard layout. TypePaste now detects script
boundaries automatically and emits the switch hotkey for you.

### How it works

1. You configure the layouts TypePaste should know about (e.g. English/Latin and Russian/Cyrillic).
2. TypePaste iterates through the text character by character.
3. When it detects a script boundary crossing (e.g. `a` → `а`), it presses the configured switch hotkey (default: `Alt+Shift`) before typing the next character.
4. Neutral characters (digits, punctuation, spaces) don't trigger a switch.
5. Multiple layouts cycle: for 3 layouts, pressing the hotkey twice moves from layout 0 to layout 2.

### Configuration

Edit via Settings GUI or `~/.config/typepaste/config.json`:

```json
{
  "layout_switch": {
    "enabled": true,
    "switch_hotkey": "Alt+Shift",
    "switch_delay_ms": 100,
    "layouts": [
      {
        "name": "English",
        "unicode_ranges": [[65, 90], [97, 122]]
      },
      {
        "name": "Russian",
        "unicode_ranges": [[1024, 1279]]
      }
    ]
  }
}
```

### CLI flags

```bash
# Enable layout switching with default config
typepaste type --layout-switch "Hello мир"

# Custom hotkey and delay
typepaste type --layout-switch --layout-switch-hotkey "Ctrl+Shift" --layout-switch-delay 150 "Hello мир"

# Start from layout index 1 (e.g. remote is already in Russian)
typepaste type --layout-switch --initial-layout 1 "Hello мир"
```

---

## Features

- **Keystroke emulation** — types characters one by one using CGEvent (macOS) or SendInput (Windows)
- **System tray** — lives quietly in the menu bar/taskbar, always ready
- **Global hotkey** — trigger paste from any app without switching windows
- **Secondary paste hotkey** — register a second shortcut for convenience
- **Layout switching** — auto-switch keyboard layout for remote/VM sessions (v0.4.0)
- **Configurable delays** — base delay, random jitter, initial delay
- **Window targeting** — type into a specific window by title or PID
- **CLI mode** — headless, scriptable, pipe-friendly
- **MCP-ready** — `list-windows --json` and `type` subcommands designed for AI tool use
- **Cross-platform** — macOS and Windows

---

## Installation

### From source

```bash
git clone https://github.com/f0lken/typepaste
cd typepaste
cargo build --release
# Binary: ./target/release/typepaste
```

### macOS permissions

TypePaste requires **Accessibility** permissions to synthesize keyboard events.
Go to **System Settings → Privacy & Security → Accessibility** and add TypePaste.

---

## Usage

### Tray mode (default)

```bash
typepaste
```

TypePaste starts in the system tray. Copy text to your clipboard, focus the target window, and press your hotkey (`Cmd+Shift+V` on macOS, `Ctrl+Shift+V` on Windows).

### CLI mode

```bash
# Type a string
typepaste type "Hello, World!"

# Type clipboard contents
typepaste type --clipboard

# Read from stdin
echo "Hello" | typepaste type --stdin

# Type into a specific window by title
typepaste type --window "Remote Desktop" "Hello мир"

# Type into a specific window by PID
typepaste type --pid 1234 "Hello мир"

# List visible windows (human-readable)
typepaste list-windows

# List windows as JSON (for MCP/scripting)
typepaste list-windows --json

# Open settings GUI
typepaste settings
```

### Layout switching examples

```bash
# Auto-switch layouts while typing mixed Russian+English
typepaste type --layout-switch "Привет World"

# Custom switch hotkey (e.g. Ctrl+Shift on Windows)
typepaste type --layout-switch --layout-switch-hotkey "Ctrl+Shift" "Hello Мир"

# Start from layout 1 (remote is already in Russian)
typepaste type --layout-switch --initial-layout 1 "мир World"
```

---

## Configuration

Configuration is stored at:
- **macOS**: `~/Library/Application Support/typepaste/config.json`  
- **Windows**: `%APPDATA%\typepaste\config.json`

### Full config reference

```json
{
  "keystroke_delay_ms": 5,
  "random_delay_min_ms": 0,
  "random_delay_max_ms": 0,
  "initial_delay_ms": 500,
  "show_notification": true,
  "start_on_login": false,
  "hotkey": "Cmd+Shift+V",
  "paste_hotkey": "",
  "max_text_length": 100000,
  "newlines_as_enter": true,
  "tabs_as_tab": true,
  "layout_switch": {
    "enabled": false,
    "switch_hotkey": "Alt+Shift",
    "switch_delay_ms": 100,
    "layouts": [
      {
        "name": "English",
        "unicode_ranges": [[65, 90], [97, 122]]
      },
      {
        "name": "Russian",
        "unicode_ranges": [[1024, 1279]]
      }
    ]
  }
}
```

---

## Architecture

```
typepaste/
├── src/
│   ├── main.rs          # Entry point, tray mode, hotkey management
│   ├── cli.rs           # CLI argument definitions (clap)
│   ├── config.rs        # Config struct, LayoutSwitchConfig, persistence
│   ├── engine.rs        # TypeEngine — orchestrates typing
│   ├── error.rs         # Error types
│   ├── platform/
│   │   ├── mod.rs       # Platform trait + WindowInfo
│   │   ├── macos.rs     # CGEvent implementation
│   │   └── windows.rs   # SendInput / enigo implementation
│   └── ui/
│       └── settings.rs  # egui settings window
├── assets/
│   └── icon.png         # Tray icon
└── Cargo.toml
```

---

## License

MIT
