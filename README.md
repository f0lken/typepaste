# TypePaste

**Paste text anywhere via keystroke emulation** — even into remote desktops, VMs, and applications that don't support direct paste.

## Problem

Many remote desktop tools (RDP, VNC, vSphere console, ILO, IPMI), virtual machines without guest tools, and security-restricted applications block standard clipboard paste (`Ctrl+V` / `Cmd+V`). This forces users to manually type long passwords, commands, and text — a slow and error-prone process.

## Solution

TypePaste reads text from your clipboard (or CLI argument / stdin) and "types" it out character by character by emitting real OS-level keystroke events. The target application sees these as genuine keyboard input, bypassing any clipboard restrictions.

## Features

- **Cross-platform** — macOS (first) + Windows
- **Two modes** — system tray GUI + headless CLI
- **Customizable hotkeys** — primary + optional additional, editable in GUI, applied instantly
- **System tray** — minimal UI, always accessible
- **CLI mode** — scriptable, pipe-friendly, MCP-server-ready
- **Window targeting** — focus a window by title substring or PID before typing
- **Unicode support** — handles special characters, not just ASCII
- **Configurable delays** — fixed base delay + optional random jitter
- **Human-like typing** — random delay range simulates natural input cadence
- **Newlines & tabs** — properly handles multi-line text
- **Safety limits** — max text length to prevent accidental mega-pastes

## Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                         TypePaste App                                │
│                                                                      │
│  ┌────────────┐  ┌────────────────┐  ┌────────────────────────────┐ │
│  │  CLI Mode  │  │  Tray Mode     │  │      TypeEngine             │ │
│  │  (clap)    │  │  (tray-icon +  │  │                             │ │
│  │            │  │   global-      │  │  • type_text()              │ │
│  │ • type     │  │   hotkey)      │  │  • type_text_to_window()   │ │
│  │ • list-    │  │                │  │  • paste_as_keystrokes()   │ │
│  │   windows  │  │  • Paste       │  │  • read_clipboard()        │ │
│  │ • settings │  │  • Settings    │  │                             │ │
│  │            │  │  • Quit        │  └──────────┬─────────────────┘ │
│  └──────┬─────┘  └──────┬─────────┘             │                   │
│         │               │                       ▼                   │
│  ┌──────┴───────────────┴───────────────────────────────────────┐   │
│  │                    Platform Layer                              │   │
│  │                                                                │   │
│  │  ┌────────────────────────┐  ┌──────────────────────────────┐ │   │
│  │  │        macOS           │  │         Windows               │ │   │
│  │  │                        │  │                               │ │   │
│  │  │ • CGEvent + enigo      │  │ • SendInput + enigo           │ │   │
│  │  │ • AXIsProcessTrusted   │  │ • KEYEVENTF_UNICODE          │ │   │
│  │  │ • osascript for        │  │ • EnumWindows /               │ │   │
│  │  │   window management    │  │   SetForegroundWindow         │ │   │
│  │  └────────────────────────┘  └──────────────────────────────┘ │   │
│  └────────────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────────┘
```

### Module Structure

```
src/
├── main.rs              # Entry point: CLI dispatch or tray mode
├── cli.rs               # Clap argument definitions
├── engine.rs            # Core logic: text → keystrokes
├── config.rs            # Persistent JSON config
├── error.rs             # Error types
├── ui/
│   ├── mod.rs           # UI module
│   └── settings.rs      # Settings GUI (eframe/egui)
└── platform/
    ├── mod.rs           # Platform dispatch + WindowInfo
    ├── macos.rs         # macOS: CGEvent + AppleScript
    ├── windows.rs       # Windows: SendInput + Win32
    └── fallback.rs      # Stub for unsupported platforms
```

## Installation

### From source

```bash
# Clone
git clone https://github.com/f0lken/typepaste.git
cd typepaste

# Build
cargo build --release

# Run (tray mode)
./target/release/typepaste
```

### macOS specific

On first run, macOS will ask for Accessibility permissions:
**System Settings → Privacy & Security → Accessibility → Enable TypePaste**

### Windows specific

No special permissions needed. For injecting into admin/elevated windows, run TypePaste as Administrator.

## Usage

### Tray Mode (default)

```bash
# Launch as system tray app with global hotkey
typepaste
```

Press `Cmd+Shift+V` (macOS) / `Ctrl+Shift+V` (Windows) to type clipboard contents into the focused window.

### CLI Mode

#### Type text from argument

```bash
typepaste type "Hello, World!"
```

#### Type text from clipboard

```bash
typepaste type --clipboard
# or short form:
typepaste type -c
```

#### Type text from stdin (pipe)

```bash
echo "ls -la" | typepaste type --stdin
cat script.sh | typepaste type --stdin
```

#### Target a specific window by title

```bash
# Focus a window whose title contains "Terminal", then type
typepaste type "ls -la" --window "Terminal"

# Short form:
typepaste type "ls -la" -w "Terminal"
```

#### Target a specific window by PID

```bash
typepaste type "ls -la" --pid 12345
# Short form:
typepaste type "ls -la" -p 12345
```

#### Override delays from CLI

```bash
# Slow typing with human-like jitter
typepaste type "slow text" --delay 50 --random-min 10 --random-max 80

# Skip initial delay entirely (useful in scripts)
typepaste type "fast" --no-delay
```

#### List available windows

```bash
# Human-readable table
typepaste list-windows

# JSON output (for scripts / MCP server)
typepaste list-windows --json
```

#### Open settings GUI

```bash
typepaste settings
```

### MCP Server Integration (planned)

The CLI mode is designed to be the foundation for an MCP server that enables agent-based work through RDP terminals:

```bash
# Example future workflow:
# 1. Agent finds the target window
typepaste list-windows --json

# 2. Agent sends commands via keystroke emulation
typepaste type "cd /var/log && tail -f syslog" --window "Remote Desktop" --no-delay

# 3. Agent captures screen (future feature) + extracts text via LLM
```

## Configuration

Config is stored at:
- macOS: `~/Library/Application Support/typepaste/config.json`
- Windows: `%APPDATA%\typepaste\config.json`

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
  "tabs_as_tab": true
}
```

### Hotkey configuration

TypePaste supports two hotkeys for triggering paste-as-keystrokes:

| Setting | Default | Description |
|---------|---------|-------------|
| `hotkey` | Cmd+Shift+V / Ctrl+Shift+V | Primary global hotkey |
| `paste_hotkey` | (empty — disabled) | Optional additional hotkey |

Both hotkeys trigger the same action — paste clipboard text via keystroke emulation. The additional hotkey is useful if you want different shortcuts for different workflows or keyboard layouts.

**Hotkey format:** `Modifier+Modifier+Key`

Supported modifiers: `Cmd` / `Ctrl` / `Shift` / `Alt` / `Option` / `Meta` / `Super`

Supported keys:
- Letters: `A`–`Z`
- Digits: `0`–`9`
- Function keys: `F1`–`F12`
- Special: `Space`, `Enter`, `Tab`, `Esc`, `Backspace`, `Delete`, `Home`, `End`, `PageUp`, `PageDown`, `Up`, `Down`, `Left`, `Right`

**Examples:**

```
Cmd+Shift+V          ← macOS default
Ctrl+Shift+V         ← Windows default
Ctrl+Alt+P           ← custom alternative
Cmd+F6               ← function key variant
Ctrl+Shift+Space     ← space-based shortcut
```

Hotkey changes made in the Settings GUI are applied immediately — no restart required.

### Delay settings explained

Each keystroke waits: **`keystroke_delay_ms` + random(`random_delay_min_ms` .. `random_delay_max_ms`)**

| Setting | Default | Description |
|---------|---------|-------------|
| `keystroke_delay_ms` | 5 | Fixed base delay between keystrokes (ms) |
| `random_delay_min_ms` | 0 | Minimum random jitter added per keystroke (ms) |
| `random_delay_max_ms` | 0 | Maximum random jitter added per keystroke (ms). Set to 0 to disable jitter |
| `initial_delay_ms` | 500 | Delay before typing starts (time to focus target window) |

**Examples:**

| Scenario | `keystroke_delay_ms` | `random_delay_min_ms` | `random_delay_max_ms` | Effective delay per key |
|----------|---------------------|-----------------------|-----------------------|------------------------|
| Fast (LAN) | 5 | 0 | 0 | 5ms fixed |
| Slow connection | 50 | 0 | 0 | 50ms fixed |
| Human-like | 20 | 10 | 80 | 30..100ms random |
| Very slow (satellite) | 100 | 20 | 150 | 120..250ms random |

### Other settings

| Setting | Default | Description |
|---------|---------|-------------|
| `max_text_length` | 100000 | Safety limit to prevent accidental huge pastes |
| `newlines_as_enter` | true | Convert `\n` to Enter key presses |
| `tabs_as_tab` | true | Convert `\t` to Tab key presses |

## Use Cases

- **Remote Desktop (RDP/VNC)** without clipboard sharing
- **vSphere / ESXi console** — paste commands into VM consoles
- **HP ILO / Dell iDRAC / IPMI** web consoles
- **Windows login screen** where Ctrl+V is disabled
- **Security-restricted apps** that block paste
- **Password managers** → copy password → TypePaste into locked field
- **Automation / scripting** — pipe text into any application via CLI
- **MCP server** — agent-based work through remote terminals (planned)

## Roadmap

- [x] Core architecture (macOS + Windows)
- [x] System tray + global hotkey
- [x] Persistent configuration
- [x] Random delay jitter for human-like typing
- [x] Settings GUI window (eframe/egui)
- [x] Customizable hotkeys (primary + additional, hot-reload)
- [x] CLI mode with clap (type, list-windows, settings)
- [x] Window targeting by title/PID
- [x] stdin/pipe support
- [ ] MCP server integration
- [ ] macOS `.app` bundle with proper Info.plist
- [ ] Windows installer (MSI/NSIS)
- [ ] Typing progress indicator / cancel button
- [ ] Linux support (X11/Wayland via xdotool/ydotool)
- [ ] Auto-update mechanism
- [ ] Homebrew formula / Winget package

## License

MIT — see [LICENSE](LICENSE)
