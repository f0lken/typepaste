# TypePaste

**Paste clipboard text anywhere via keystroke emulation** — even into remote desktops, VMs, and applications that don't support direct paste.

## Problem

Many remote desktop tools (RDP, VNC, vSphere console, ILO, IPMI), virtual machines without guest tools, and security-restricted applications block standard clipboard paste (`Ctrl+V` / `Cmd+V`). This forces users to manually type long passwords, commands, and text — a slow and error-prone process.

## Solution

TypePaste reads text from your clipboard and "types" it out character by character by emitting real OS-level keystroke events. The target application sees these as genuine keyboard input, bypassing any clipboard restrictions.

## Features

- **Cross-platform** — macOS (first) + Windows
- **Global hotkey** — `Cmd+Shift+V` (macOS) / `Ctrl+Shift+V` (Windows)
- **System tray** — minimal UI, always accessible
- **Unicode support** — handles special characters, not just ASCII
- **Configurable delays** — adjust typing speed for slow remote connections
- **Newlines & tabs** — properly handles multi-line text
- **Safety limits** — max text length to prevent accidental mega-pastes

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                         TypePaste App                            │
│                                                                  │
│  ┌─────────────┐  ┌──────────────┐  ┌─────────────────────────┐ │
│  │ System Tray │  │ Global       │  │    TypeEngine           │ │
│  │ (tray-icon) │◄─│ Hotkey       │─►│                         │ │
│  │             │  │ (global-     │  │ 1. Read clipboard       │ │
│  │ • Paste     │  │  hotkey)     │  │    (arboard)            │ │
│  │ • Settings  │  │              │  │ 2. Initial delay        │ │
│  │ • Quit      │  │ Cmd+Shift+V │  │    (configurable)       │ │
│  └─────────────┘  │ Ctrl+Shift+V│  │ 3. Emit keystrokes      │ │
│                   └──────────────┘  │    per character         │ │
│                                     └──────────┬──────────────┘ │
│                                                │                │
│  ┌─────────────────────────────────────────────▼──────────────┐ │
│  │                    Platform Layer                           │ │
│  │                                                             │ │
│  │  ┌─────────────────────┐  ┌───────────────────────────┐    │ │
│  │  │       macOS         │  │        Windows             │    │ │
│  │  │                     │  │                            │    │ │
│  │  │ • CGEvent API       │  │ • SendInput API            │    │ │
│  │  │ • AXIsProcess-      │  │ • KEYEVENTF_UNICODE       │    │ │
│  │  │   Trusted check     │  │ • No special permissions   │    │ │
│  │  │ • Accessibility     │  │   (admin for UAC only)     │    │ │
│  │  │   permission prompt │  │                            │    │ │
│  │  │ • enigo for Unicode │  │ • enigo for event dispatch │    │ │
│  │  └─────────────────────┘  └───────────────────────────┘    │ │
│  └─────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
```

### Module Structure

```
src/
├── main.rs              # App entry: event loop, tray, hotkeys
├── engine.rs            # Core logic: clipboard → keystrokes
├── config.rs            # Persistent JSON config
├── error.rs             # Error types
└── platform/
    ├── mod.rs           # Platform dispatch
    ├── macos.rs         # macOS: CGEvent + Accessibility
    ├── windows.rs       # Windows: SendInput
    └── fallback.rs      # Stub for unsupported platforms
```

### Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| **Rust** | Single binary, no runtime deps, safe FFI, cross-platform |
| **enigo** crate | Battle-tested keyboard/mouse simulation on macOS/Windows |
| **arboard** crate | Cross-platform clipboard access |
| **tray-icon + global-hotkey** | Lightweight system tray + OS-level hotkeys without a heavy GUI framework |
| **Per-character emission** | Maximum compatibility — works even where `SendInput` batch fails |
| **Configurable delay** | Slow remote connections (satellite, VPN) may drop fast keystrokes |

### Data Flow

```
User presses Cmd+Shift+V
        │
        ▼
Global Hotkey captured (global-hotkey crate)
        │
        ▼
TypeEngine::paste_as_keystrokes()
        │
        ├─► check_accessibility() — macOS: AXIsProcessTrusted
        │                           Windows: noop
        │
        ├─► read_clipboard() — arboard::Clipboard::get_text()
        │
        ├─► sleep(initial_delay_ms) — user switches to target window
        │
        └─► type_string(text, config)
                │
                ├─► for each char in text:
                │       match char:
                │         '\n' → Key::Return
                │         '\t' → Key::Tab
                │         '\r' → skip
                │         _   → enigo.text(char)
                │
                │       sleep(keystroke_delay_ms)
                │
                └─► Done
```

## Installation

### From source

```bash
# Clone
git clone https://github.com/f0lken/typepaste.git
cd typepaste

# Build
cargo build --release

# Run
./target/release/typepaste
```

### macOS specific

On first run, macOS will ask for Accessibility permissions:
**System Settings → Privacy & Security → Accessibility → Enable TypePaste**

### Windows specific

No special permissions needed. For injecting into admin/elevated windows, run TypePaste as Administrator.

## Configuration

Config is stored at:
- macOS: `~/Library/Application Support/typepaste/config.json`
- Windows: `%APPDATA%\typepaste\config.json`

```json
{
  "keystroke_delay_ms": 5,
  "initial_delay_ms": 500,
  "show_notification": true,
  "start_on_login": false,
  "hotkey": "Cmd+Shift+V",
  "max_text_length": 100000,
  "newlines_as_enter": true,
  "tabs_as_tab": true
}
```

| Setting | Default | Description |
|---------|---------|-------------|
| `keystroke_delay_ms` | 5 | Delay between keystrokes (increase for slow connections) |
| `initial_delay_ms` | 500 | Delay before typing starts (time to focus target window) |
| `hotkey` | Cmd+Shift+V / Ctrl+Shift+V | Global hotkey trigger |
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

## Roadmap

- [x] Core architecture (macOS + Windows)
- [x] System tray + global hotkey
- [x] Persistent configuration
- [ ] macOS `.app` bundle with proper Info.plist
- [ ] Windows installer (MSI/NSIS)
- [ ] Settings GUI window
- [ ] Typing progress indicator / cancel button
- [ ] Linux support (X11/Wayland via xdotool/ydotool)
- [ ] Auto-update mechanism
- [ ] Homebrew formula / Winget package

## License

MIT — see [LICENSE](LICENSE)
