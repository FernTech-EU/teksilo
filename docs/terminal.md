<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Terminal (Console)

`bastyde-terminal` is an embeddable, fully-accessible terminal-emulator widget —
a real shell running over a pseudo-terminal on Windows (ConPTY / "modern
terminal"), Linux and macOS. It is the framework's answer to VS Code's
integrated terminal, Qt `QTermWidget`, GTK `VteTerminal`, and SwiftTerm.

```rust
use bastyde::prelude::*;   // with the `terminal` feature enabled

let terminal = Terminal::new()                 // runs the user's default shell
    .scrollback_lines(10_000)
    .cursor_shape(CursorStyle::Beam)
    .on_title_changed(|t| println!("title: {t}"));
```

Run the showcase: `cargo run -p terminal-demo`.

## Design: Bastyde owns the *view*, not the emulator

A VT emulator (escape-sequence parsing, the cell grid, scrollback, reflow) is a
correctness-critical *domain library*, not a GUI concern — reinventing it would
be a mistake. So the split mirrors `bastyde-data` ("a peer of the GUI, not part
of it") and `bastyde-webview` (a heavy engine behind a feature flag):

- **The view (this crate)** — grid rendering, keyboard→byte encoding, mouse
  reporting, selection, `Role::Terminal` accessibility, theming, lifecycle.
- **The engine** — the PTY + VT model, behind the [`TerminalEngine`] trait. The
  default backend (feature `alacritty`, on via the umbrella's `terminal`
  feature) pairs [`portable-pty`](https://docs.rs/portable-pty) (ConPTY /
  openpty) with [`alacritty_terminal`](https://docs.rs/alacritty_terminal) (the
  VT parser + grid + scrollback). Nothing in this crate parses escape codes.

Unlike `bastyde-webview`, the terminal renders **into the wgpu surface**, so it
keeps full accessibility, theming, and the opacity/blur/transform paint scopes,
and needs no native-subview compositing.

The crate sits at the `bastyde-widgets` tier but depends only on `bastyde-core`,
`bastyde-tokens`, `bastyde-canvas`, and `bastyde-platform` (for the clipboard).
It is **off by default** — apps that don't embed a terminal pull neither a PTY
layer nor a VT parser.

## Public API

### `Terminal` — the widget builder

| Group | Methods |
| --- | --- |
| Construction | `Terminal::new()` (default shell), `Terminal::with_command(cmd)`, `Terminal::with_engine_factory(f)` (custom engine) |
| Process | `.command(TerminalCommand)`, `.shell(program, args)`, `.working_directory(p)`, `.env(k, v)`, `.on_close(TerminalClosePolicy)` |
| Appearance | `.font(TextStyle)` (defaults to `theme.typography.mono`; **must be monospace** — a proportional font misaligns the grid and logs a one-time warning), `.color_scheme(ColorScheme)`, `.cursor_shape(CursorStyle)`, `.cursor_blink(bool)`, `.follow_text_scale(bool)`, `.style(impl TerminalStyle)` |
| Behaviour | `.scrollback_lines(n)`, `.scroll_on_output(bool)`, `.bell(BellStyle)`, `.read_only(bool)`, `.mouse_reporting(bool)`, `.alt_sends_escape(bool)`, `.label(name)` |
| Events | `.on_title_changed(Fn(&str))` (OSC 0/2), `.on_child_exited(Fn(TerminalExit))`, `.on_bell(Fn())`, `.on_cwd_changed(Fn(&str))` (OSC 7) |

### `TerminalController` — drive it from anywhere

`terminal.controller()` returns a cloneable handle (the `ListModel`/`SceneModel`
pattern). It holds a `Weak` reference, so keeping a controller never keeps the
child process alive after the widget is gone.

```rust
let ctrl = terminal.controller();
ctrl.feed_text("cargo build\n");   // write to the child
ctrl.clear();                       // clear the screen
ctrl.scroll_to_bottom();
ctrl.select_all();
```

- **Write:** `write(bytes)`, `feed_text(&str)`, `paste(&str)` (bracketed-paste
  aware).
- **Control:** `clear()`, `reset()`, `scroll_to_bottom()`, `scroll_lines(i32)`,
  `select_all()`, `clear_selection()`, `selection_text()`.
- **Reactive read** (each returns a `Signal`): `title_signal()`, `cwd_signal()`,
  `child_running_signal()`, `has_selection_signal()`, `is_alt_screen_signal()`,
  `columns_signal()`, `rows_signal()`, `exit_signal()`.

### Colour scheme

`ColorScheme` carries the 16 themeable ANSI slots (`0..=7` normal, `8..=15`
bright) plus default foreground/background/cursor/selection. `ColorScheme::dark()`
(the default) and `ColorScheme::light()` ship; the xterm 256-colour cube and
24-bit truecolour are resolved automatically. Because colour resolution is a
*view* concern (the engine reports colours symbolically), a running shell
re-themes live when you swap the scheme.

## Keyboard, mouse, clipboard

- **Every key reaches the child** — the widget is a keyboard-capture surface
  (see below), so `Ctrl+C` interrupts, `Ctrl+W`/`Ctrl+T`/`Alt+<key>` reach the
  shell, not the host app's shortcuts. Application-cursor and function-key modes
  are honoured.
- **Copy / paste** — `Ctrl+Shift+C` / `Ctrl+Shift+V` (⌘C / ⌘V on macOS). Paste
  is wrapped in bracketed-paste markers when the child enables the mode. The
  clipboard is the app-installed `ClipboardHandle` (bastyde-app's `clipboard`
  feature).
- **Selection** — drag to select (word on double-click, line on triple-click,
  rectangular with Alt).
- **Scrollback** — the wheel and `Shift+PageUp` / `Shift+PageDown`; a keystroke
  snaps back to the prompt.
- **Mouse reporting** — when a full-screen app (vim, tmux) enables it, presses /
  releases / drags / wheel are reported (SGR + legacy X10). `Shift` forces local
  selection instead.

## Accessibility

The terminal exposes a native **`Role::Terminal`** node (all three OS backends).
Its children are one `Role::Paragraph` → `Role::TextRun` per visible row, so a
screen reader reviews the screen with its normal text-navigation commands; the
VT cursor maps to the AT caret. **New output** is announced through a separate,
small `Role::Status` + `Live::Polite` region (the last completed line) rather
than by re-announcing the whole screen — the way screen readers actually consume
ARIA live regions. Verify with the automation MCP (`snapshot_tree`,
`pull_announcements`) or Orca / VoiceOver / Narrator.

## Two framework primitives this widget introduced

Both are generally useful and live in `bastyde-core`, not just here:

- **`WidgetBuilder::keyboard_capture(bool)`** — while focused, the node receives
  every `KeyDown` raw, bypassing shortcut → intent → action resolution. Any
  "capture all keys" surface (a terminal, a game viewport, a modal editor) wants
  it. (Escape / overlay back-navigation still run first.)
- **`RepaintWindowRequest { window_id }`** — a thread-safe "repaint this window"
  request posted via `AppEventPoster::post_external` from a background thread. A
  bare redraw re-presents cached paint, so content changed **off the UI thread**
  (the PTY-reader thread) needs its window marked paint-dirty; bastyde-app routes
  this request to do exactly that. It is the off-thread analogue of
  `ctx.request_frame()`. Canonical treatment (and the zero-frame-rule contract):
  [idle-and-animation.md](idle-and-animation.md) "Off-thread repaint".

## Limitations (v1)

- **Application-keypad mode** (numpad-specific sequences) is not yet delivered —
  it needs a physical-key/location bit threaded through `WidgetEvent::KeyDown`,
  a separate framework change (numpad keys currently arrive as their
  NumLock-dependent logical key). Everything else — arrows, F1–F24, Insert,
  Home/End/PageUp/Down — works.
- **Search over scrollback**, split-panes *inside* one terminal, sixel/kitty
  image graphics, and ligatures are out of scope for v1.
- **Move-out / cross-window** child transfer is not supported.
- Rendering is per-cell `draw_text`; GPU cell batching is a future optimisation.

## Files

- Widget: [crates/bastyde-terminal/src/terminal.rs](../crates/bastyde-terminal/src/terminal.rs)
  (+ `state.rs`, `render.rs`, `input.rs`, `mouse.rs`, `a11y.rs`, `style.rs`,
  `color_scheme.rs`).
- Engine trait + types: [engine.rs](../crates/bastyde-terminal/src/engine.rs);
  default backend: [alacritty_engine.rs](../crates/bastyde-terminal/src/alacritty_engine.rs)
  + [pty.rs](../crates/bastyde-terminal/src/pty.rs). Test double:
  [memory.rs](../crates/bastyde-terminal/src/memory.rs).
- Framework primitives: `keyboard_capture` in
  [widget_builder.rs](../crates/bastyde-core/src/widget_builder.rs);
  `RepaintWindowRequest` in [app_event.rs](../crates/bastyde-core/src/app_event.rs),
  routed in [bastyde-app/src/app.rs](../crates/bastyde-app/src/app.rs).
- Demo: [examples/terminal_demo/src/main.rs](../examples/terminal_demo/src/main.rs).
