# Plexus

A modern, minimal terminal multiplexer written in Rust.

## Features

- Multiple panes with live shell sessions
- Horizontal and vertical splits
- Keyboard-driven pane navigation
- Minimal VT100 emulation for shell rendering
- Built-in status bar

## Building

```bash
cargo build --release
```

The binary is produced at `target/release/plexus`.

## Usage

```bash
./target/release/plexus
```

### Key Bindings

Prefix key: `Ctrl+a`

| Command | Action |
|---------|--------|
| `Ctrl+a` `\|` | Split pane vertically |
| `Ctrl+a` `-` | Split pane horizontally |
| `Ctrl+a` `←` | Navigate to pane on the left |
| `Ctrl+a` `→` | Navigate to pane on the right |
| `Ctrl+a` `↑` | Navigate to pane above |
| `Ctrl+a` `↓` | Navigate to pane below |
| `Ctrl+a` `c` | Close active pane |
| `Ctrl+a` `q` | Quit plexus |

All other keystrokes are forwarded to the active pane's shell.

## Architecture

- `vt.rs` — Minimal VT100 screen buffer with ANSI escape sequence parsing
- `app.rs` — Pane and window management, PTY spawning, layout engine
- `main.rs` — Event loop, input handling, rendering via crossterm

## License

MIT
