<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Terminal (Console)

`teksilo-terminal` is an embeddable, fully-accessible terminal-emulator widget —
a real shell running over a pseudo-terminal on Windows (ConPTY / "modern
terminal"), Linux and macOS. It is the framework's answer to VS Code's
integrated terminal, Qt `QTermWidget`, GTK `VteTerminal`, and SwiftTerm.

```rust
use teksilo::prelude::*;   // with the `terminal` feature enabled

let terminal = Terminal::new()                 // runs the user's default shell
    .scrollback_lines(10_000)
    .cursor_shape(CursorStyle::Beam)
    .on_title_changed(|t| println!("title: {t}"));
```

Run the showcase: `cargo run -p terminal-demo`.

## Design: Teksilo owns the *view*, not the emulator

A VT emulator (escape-sequence parsing, the cell grid, scrollback, reflow) is a
correctness-critical *domain library*, not a GUI concern — reinventing it would
be a mistake. So the split mirrors `teksilo-data` ("a peer of the GUI, not part
of it") and `teksilo-webview` (a heavy engine behind a feature flag):

- **The view (this crate)** — grid rendering, keyboard→byte encoding, mouse
  reporting, selection, `Role::Terminal` accessibility, theming, lifecycle.
- **The engine** — the PTY + VT model, behind the [`TerminalEngine`] trait. The
  default backend (feature `alacritty`, on via the umbrella's `terminal`
  feature) pairs [`portable-pty`](https://docs.rs/portable-pty) (ConPTY /
  openpty) with [`alacritty_terminal`](https://docs.rs/alacritty_terminal) (the
  VT parser + grid + scrollback). Nothing in this crate parses escape codes.

Unlike `teksilo-webview`, the terminal renders **into the wgpu surface**, so it
keeps full accessibility, theming, and the opacity/blur/transform paint scopes,
and needs no native-subview compositing.

The crate sits at the `teksilo-widgets` tier but depends only on `teksilo-core`,
`teksilo-tokens`, `teksilo-canvas`, and `teksilo-platform` (for the clipboard).
It is **off by default** — apps that don't embed a terminal pull neither a PTY
layer nor a VT parser.

## Public API

### `Terminal` — the widget builder

| Group | Methods |
| --- | --- |
| Construction | `Terminal::new()` (default shell), `Terminal::with_command(cmd)`, `Terminal::with_engine_factory(f)` (custom engine) |
| Process | `.command(TerminalCommand)`, `.shell(program, args)`, `.working_directory(p)`, `.env(k, v)`, `.on_close(TerminalClosePolicy)` |
| Appearance | `.font(TextStyle)` (defaults to `theme.typography.mono`; **must be monospace** — a proportional font misaligns the grid and logs a one-time warning), `.color_scheme(ColorScheme)`, `.cursor_shape(CursorStyle)`, `.cursor_blink(bool)`, `.follow_text_scale(bool)`, `.style(impl TerminalStyle)` |
| Behaviour | `.scrollback_lines(n)`, `.scroll_on_output(bool)`, `.bell(BellStyle)`, `.read_only(bool)`, `.mouse_reporting(bool)`, `.touch_mouse_reporting(TouchReporting)`, `.alt_sends_escape(bool)`, `.label(name)` |
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
  clipboard is the app-installed `ClipboardHandle` (teksilo-app's `clipboard`
  feature).
- **Selection** — drag to select (word on double-click, line on triple-click,
  rectangular with Alt). A drag that leaves the widget keeps extending: the
  gesture arena takes the pointer for the whole press.
- **Scrollback** — the wheel, `Shift+PageUp` / `Shift+PageDown`, and a finger
  (below); a keystroke snaps back to the prompt. A wheel notch the terminal
  cannot use is still absorbed, the way every terminal absorbs it — the finger's
  pan is the one that chains outward at the end of the ring. A trackpad's
  sub-line samples are **banked** until they add up to a line, so a slow
  two-finger scroll moves the ring instead of being rounded away.
- **Mouse reporting** — when a full-screen app (vim, tmux) enables it, presses /
  releases / drags / wheel are reported (SGR + legacy X10), each naming the cell
  the pointer is over — the wheel included, so a program that splits its window
  can tell which pane the notch happened in. A wheel notch is routed by hover and
  carries no position of its own, so its cell is the one the cursor was last seen
  on. `Shift` forces local selection instead. A **direct** pointer is governed
  separately by `.touch_mouse_reporting(..)` and is not reported by default.
- **Leaving the terminal** — `Ctrl+Tab` / `Ctrl+Shift+Tab` move focus to the
  next / previous widget. Plain `Tab` and `Shift+Tab` belong to the child (they
  are encoded as `\t` and CSI Z), so this chord is the way out; it is reserved
  by the dispatcher for **every** `keyboard_capture` surface and never reaches
  the widget. Ctrl, not ⌘, on macOS as well — ⌘⇥ is the application switcher.
  The terminal advertises it to assistive technology as its keyboard shortcut.

## Touch

A finger needs three things from a terminal — a way to scroll it, a way to select
text, and a way to reach the clipboard — and there is one thing it must not be
given, which is the child program's mouse stream. The shipped policy is one
sentence per gesture:

| Gesture | What it does |
| --- | --- |
| One finger, dragged | Pans the **scrollback**, with a kinetic hand-off on release. |
| Two fingers | The same pan, at one finger's rate, and never reported to the child — see `TouchReporting` below. |
| Double / triple tap | Selects a word / a line, then raises two **selection handles**. |
| Handle dragged | Adjusts that end of the selection, snapping to the cell grid. |
| Hold | Opens the context menu: Copy / Paste / Select all / Clear. |

### The pan, and why it is not `ScrollableBehavior`

The workspace's scrollables adopt
[`common::scrollable`](../crates/teksilo-widgets/src/common/scrollable.rs) and get
velocity, fling and rubber band from `teksilo-core::kinetic`
([kinetic-scrolling.md](kinetic-scrolling.md) §9 lists which). The terminal
cannot, and not only because that helper lives in `teksilo-widgets`: its offset
is a **scrollback ring position quantised to whole lines**, not a pixel offset
with a maximum. Three consequences, and they are the whole design:

- a sample worth less than a line has to be **banked**, not applied and not
  dropped. `TerminalState::scroll_residue` is that bank, shared by the finger and
  the trackpad;
- there is no fractional position to rubber-band and nothing to settle onto, so
  a **kinetic pan here means exactly**: line steps arriving at frame rate from
  the tree's fling pump, stopping when the simulation stops or the ring ends.
  There is no overscroll and no spring, and adding one would need a continuum the
  model does not have;
- a pan that runs out of scrollback answers `Ignored`, so the rest of the gesture
  chains to whatever encloses the terminal — the ordinary claimant-chain rule.
  The **wheel** deliberately does not: it keeps absorbing unconditionally, which
  is both what every terminal does and the byte-for-byte mouse behaviour this
  work was not allowed to change.

The claim itself is one builder call —
`.pan_claim(PanClaim { axes: PanAxes::Y, kinetic: true, .. })`, direct pointers
only by `PanClaim`'s own default — so the wheel keeps the bubble route it has
always had and only a finger walks the claimant chain.

### Selection by finger

The terminal implements
[`TextHitSource`](../crates/teksilo-core/src/text_touch.rs) over its cell grid
and mounts the framework's affordance layer, so touch selection is the same
machinery every text surface uses. Two things are specific to a grid:

- the **document is the visible screen** and an offset is a cell:
  `offset = line * columns + column`, `document_len() == columns * rows`. So an
  offset is a **cell boundary**, and `offset_at` *rounds* to the nearest one
  rather than truncating into the cell the point is inside — which is what makes a
  dragged handle snap to the grid rather than hover between two columns;
- those offsets are **viewport** offsets, so anything that scrolls the view
  retires the affordances rather than trying to follow them. New *output* does
  not: the engine holds its selection in buffer coordinates and the snapshot
  re-projects it, so the handles move with the text;
- a handle's disc hangs a radius **off** the row it marks, so the host translates
  every drag sample back onto that row — the grab offset is captured when the drag
  begins and added to each sample after. Because `offset_at` floors the vertical
  axis and a trailing handle's anchor sits past the row's bottom edge, an
  untranslated sample resolves a whole row away however small the radius is. Same
  correction, same shape, as the rich-text host's.

`is_editable()` is `false` — the cursor belongs to the child program, so there is
no caret to place, no caret handle and nothing a Cut could remove. A **cursor**
raises no handles at all: it has a drag for adjusting a selection, and two discs
hanging off the grid would be in its way.

No selection **toolbar** is raised. The terminal has one menu and it is the
context menu; a second, nearly-identical bar at the end of a handle drag would be
two mechanisms for one job.

### The context menu

Reached three ways — a hold (the tree-owned long-press route, which resolves here
because the widget installs a `context_menu` factory and no `on_long_press`
handler), a right-click, and an assistive client's `ShowContextMenu`. Its rows are
not a list held in the widget: Copy / Paste / Select all come from
`TextHitSource::clipboard_actions`, the same answer a selection toolbar would
read, so the menu cannot offer a Copy with nothing selected or a Paste into a
read-only terminal. Clear is the terminal's own. There is no Cut.

The menu widget is **written in this crate**, by hand, because `MenuList` lives
in `teksilo-widgets` and this crate does not depend on it. What that costs is
plain: a flat list of rows, no submenus, no separators, no icons, no shortcut
column, no mnemonics, and English labels (this crate has no message bundle). It is
a menu for four commands, and an application that wants a richer or translated
one installs its own `context_menu` factory on an ancestor — the terminal's is
consulted first only because the walk starts at the target.

Its rows do follow the **density ladder**: the row-height floor is a base passed
through `dp(.., TargetRole::Target, ..)`, the same projection the selection
handles' diameters take, so a menu opened by a finger is sized for one. A row
still grows past that floor when its label's line is taller.

### `TouchReporting`

`.touch_mouse_reporting(..)` decides what a **direct** pointer is reported to the
child as while that child has mouse tracking on. The default is `Off`, and it is
a decision rather than a simplification: a finger has no hover, no buttons and no
sub-cell resolution, so reporting it gives a full-screen program a stream it
cannot use well while spending the only gesture vocabulary the *view* has — with
the contact routed to the child there is no tap to select with, no hold to open a
menu with and no drag to scroll with.

`AsButton1` hands a single contact over as **mouse button 1**: the same bytes a
left button produces (`ESC [ < 0 ; col ; row M` on press, `32` — button 1 plus
the motion bit — on each move, `… m` on release under SGR; the legacy X10 triple
otherwise). Nothing on the wire says it was a finger; there is no VT encoding
that could. Two or more simultaneous contacts are **never** reported, so a
two-finger pan still reaches the scrollback — that is the way back once one
finger belongs to the child.

## Accessibility

The terminal exposes a native **`Role::Terminal`** node (all three OS backends).
Its children are one `Role::TextRun` per visible row — several linked runs on a
row past 255 columns — so a screen reader reviews the screen with its normal
text-navigation commands; the VT cursor maps to the AT caret. Each run carries
its own box and per-character extents, so a magnifier follows the review cursor
and a braille cell routes to the cell under it. **New output** is announced through a separate,
small `Role::Status` + `Live::Polite` region (the last completed line) rather
than by re-announcing the whole screen — the way screen readers actually consume
ARIA live regions. Verify with the automation MCP (`snapshot_tree`,
`pull_announcements`) or Orca / VoiceOver / Narrator.

The node advertises `Action::ScrollUp` / `Action::ScrollDown`, and both move the
viewport by a **page** — the meaning those two actions carry everywhere else in
the framework, and the only workable one here, since a screen reader driving a
10 000-line buffer a line at a time would need one action per line. Each selection
handle is a `Role::Slider` over the document's cell offsets, with `SetValue` as
its whole contract; handles are deliberately outside the Tab ring.

## Two framework primitives this widget introduced

Both are generally useful and live in `teksilo-core`, not just here:

- **`WidgetBuilder::keyboard_capture(bool)`** — while focused, the node receives
  every `KeyDown` raw, bypassing shortcut → intent → action resolution. Any
  "capture all keys" surface (a terminal, a game viewport, a modal editor) wants
  it. One chord is reserved and never delivered: **`Ctrl+Tab` /
  `Ctrl+Shift+Tab` always cycle focus**, so a capture surface cannot become a
  keyboard trap (WCAG 2.1.2) however greedily its `on_key` behaves. Escape is
  *not* reserved — overlay back-navigation runs ahead of the capture check only
  while an overlay is actually open.
- **`RepaintWindowRequest { window_id }`** — a thread-safe "repaint this window"
  request posted via `AppEventPoster::post_external` from a background thread. A
  bare redraw re-presents cached paint, so content changed **off the UI thread**
  (the PTY-reader thread) needs its window marked paint-dirty; teksilo-app routes
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
- The context menu's labels are **English**; see above for why and for the way
  out.
- A handle drag over a **block** (Alt-drag) selection continues it as a flowing
  one: `set_selection` takes an offset range, which is what a flowing selection
  is, and there is no range shape that means "rectangle".
- `TextHitSource::word_range_at` is a run of non-blank cells, which is *simpler*
  than the word the mouse's double-click gets (that goes through the engine's own
  separator set, which is not exposed as a range). The two cannot be seen to
  differ in the shipped terminal — a hold opens the menu rather than selecting a
  word, so nothing calls it — but a host driving `TouchSelection` itself should
  know.
- Nothing paints the ring's ends: there is no overscroll to show, per the pan
  section above.

## Files

- Widget: [crates/teksilo-terminal/src/terminal.rs](../crates/teksilo-terminal/src/terminal.rs)
  (+ `state.rs`, `render.rs`, `input.rs`, `mouse.rs`, `a11y.rs`, `style.rs`,
  `color_scheme.rs`).
- Touch: [touch.rs](../crates/teksilo-terminal/src/touch.rs) (the
  `TextHitSource` over the grid, the controller mount, the affordance overlay's
  content root, and the fallback selection style) and
  [menu.rs](../crates/teksilo-terminal/src/menu.rs) (the context menu).
  `TouchReporting` is in [mouse.rs](../crates/teksilo-terminal/src/mouse.rs)
  beside the report encoder it governs. Tests:
  [tests/touch.rs](../crates/teksilo-terminal/tests/touch.rs).
  Contract: [text-touch-editing.md](text-touch-editing.md); the pan's place in
  the workspace's scroll physics: [kinetic-scrolling.md](kinetic-scrolling.md).
- Engine trait + types: [engine.rs](../crates/teksilo-terminal/src/engine.rs);
  default backend: [alacritty_engine.rs](../crates/teksilo-terminal/src/alacritty_engine.rs)
  + [pty.rs](../crates/teksilo-terminal/src/pty.rs). Test double:
  [memory.rs](../crates/teksilo-terminal/src/memory.rs).
- Framework primitives: `keyboard_capture` in
  [widget_builder.rs](../crates/teksilo-core/src/widget_builder.rs);
  `RepaintWindowRequest` in [app_event.rs](../crates/teksilo-core/src/app_event.rs),
  routed in [teksilo-app/src/app.rs](../crates/teksilo-app/src/app.rs).
- Demo: [examples/terminal_demo/src/main.rs](../examples/terminal_demo/src/main.rs).
