// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The engine boundary: a swappable [`TerminalEngine`] that owns the
//! pseudo-terminal (PTY) and the VT emulation (parsing + grid + scrollback),
//! plus the engine-agnostic value types the *view* consumes.
//!
//! Teksilo owns only the view (rendering, input encoding, accessibility). The
//! actual emulation is delegated to a proven crate behind this trait — the
//! default [`crate::alacritty_engine`] backs it with `portable-pty` (the PTY)
//! and `alacritty_terminal` (the VT model). Nothing here reinvents escape-code
//! handling; the trait exists so the view never depends on the engine's own
//! types and so a different engine can be dropped in later.

use crate::color_scheme::TermColor;

/// PTY / grid geometry: the terminal's size in cells plus the pixel size of the
/// text area (some full-screen apps use the pixel size for sixel / cell-pixel
/// queries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyGeom {
    pub cols: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl PtyGeom {
    pub fn new(cols: u16, rows: u16, pixel_width: u16, pixel_height: u16) -> Self {
        // A PTY must never be 0×0 or the child's `ioctl(TIOCSWINSZ)` and many
        // curses apps misbehave; clamp to at least 1×1.
        Self {
            cols: cols.max(1),
            rows: rows.max(1),
            pixel_width,
            pixel_height,
        }
    }
}

/// The child process to spawn. An empty [`Self::program`] means "the user's
/// default shell" (`$SHELL` / `%COMSPEC%`), resolved by the engine.
#[derive(Debug, Clone, Default)]
pub struct TerminalCommand {
    /// The program to run. `None` → the platform default shell.
    pub program: Option<String>,
    /// Arguments passed to the program.
    pub args: Vec<String>,
    /// Extra environment variables (`(key, value)`).
    pub env: Vec<(String, String)>,
    /// Working directory. `None` → inherit the parent's.
    pub cwd: Option<std::path::PathBuf>,
}

impl TerminalCommand {
    /// A command running the user's default login shell.
    pub fn shell() -> Self {
        Self::default()
    }

    /// A command running `program` with `args`.
    pub fn program(program: impl Into<String>, args: impl IntoIterator<Item = String>) -> Self {
        Self {
            program: Some(program.into()),
            args: args.into_iter().collect(),
            ..Default::default()
        }
    }
}

/// Per-cell rendering attributes (SGR state), as reported by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellAttrs {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub double_underline: bool,
    pub strikeout: bool,
    /// Reverse video: the renderer swaps the resolved fg/bg for this cell.
    pub inverse: bool,
    /// Concealed text: the renderer paints the glyph in the background colour.
    pub hidden: bool,
    /// Leading cell of a double-width (e.g. CJK) glyph — it spans this column
    /// and the next.
    pub wide: bool,
    /// The empty trailing half of a wide glyph — the renderer skips its glyph
    /// (the wide leading cell already covers both columns).
    pub wide_spacer: bool,
}

/// One terminal grid cell: its base character, its symbolic colours, and its
/// attributes. Combining marks (accents) that render on top of the base
/// character are carried in [`Self::zerowidth`].
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: TermColor,
    pub bg: TermColor,
    pub attrs: CellAttrs,
    /// Zero-width combining characters stacked on the base character, or `None`
    /// (the overwhelmingly common case).
    pub zerowidth: Option<Vec<char>>,
}

impl Cell {
    /// The full grapheme text of this cell (base + any combining marks) as a
    /// short owned string, for shaping / accessibility.
    pub fn text(&self) -> String {
        match &self.zerowidth {
            None => self.ch.to_string(),
            Some(extra) => {
                let mut s = String::with_capacity(1 + extra.len());
                s.push(self.ch);
                s.extend(extra.iter().copied());
                s
            }
        }
    }

    /// Whether this cell is visually empty (a space with default background and
    /// no decoration) — the renderer can skip its background fill.
    pub fn is_blank(&self) -> bool {
        self.ch == ' '
            && !self.attrs.inverse
            && matches!(self.bg, TermColor::DefaultBg)
            && !self.attrs.underline
            && !self.attrs.double_underline
            && !self.attrs.strikeout
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: TermColor::DefaultFg,
            bg: TermColor::DefaultBg,
            attrs: CellAttrs::default(),
            zerowidth: None,
        }
    }
}

/// The visual shape of the terminal cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermCursorShape {
    Block,
    Underline,
    Beam,
    /// An unfocused block (drawn as a hollow rectangle).
    HollowBlock,
    Hidden,
}

/// The cursor position (in viewport cell coordinates) and shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorInfo {
    /// Row within the visible viewport (`0..rows`).
    pub line: usize,
    /// Column within the visible viewport (`0..cols`).
    pub column: usize,
    pub shape: TermCursorShape,
    /// Whether the cursor is currently visible (DECTCEM + not scrolled away).
    pub visible: bool,
}

/// The currently-selected region, in **viewport** cell coordinates (already
/// clamped to the visible area). Endpoints are inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionSpan {
    /// `(line, column)` of the first selected cell.
    pub start: (usize, usize),
    /// `(line, column)` of the last selected cell.
    pub end: (usize, usize),
    /// A rectangular (block) selection rather than a flowing line selection.
    pub block: bool,
}

/// An owned, engine-agnostic snapshot of the visible grid plus cursor and
/// scrollback state. Produced by [`TerminalEngine::snapshot`] whenever content
/// changes and cached by the view, so painting never touches the engine.
#[derive(Debug, Clone)]
pub struct GridSnapshot {
    pub columns: usize,
    pub screen_lines: usize,
    /// Row-major cells, `screen_lines * columns` entries.
    pub cells: Vec<Cell>,
    pub cursor: CursorInfo,
    /// The selection highlight, if any (viewport coordinates).
    pub selection: Option<SelectionSpan>,
    /// How many lines the viewport is scrolled up into scrollback (0 = bottom).
    pub display_offset: usize,
    /// Number of lines currently held in scrollback above the viewport.
    pub history_len: usize,
}

impl GridSnapshot {
    /// The cell at viewport `(line, column)`, or `None` if out of bounds.
    pub fn cell(&self, line: usize, column: usize) -> Option<&Cell> {
        if line >= self.screen_lines || column >= self.columns {
            return None;
        }
        self.cells.get(line * self.columns + column)
    }

    /// The visible rows as slices, top to bottom.
    pub fn rows(&self) -> impl Iterator<Item = &[Cell]> {
        self.cells.chunks(self.columns.max(1))
    }

    /// Whether viewport cell `(line, column)` falls within the selection.
    pub fn is_selected(&self, line: usize, column: usize) -> bool {
        let Some(sel) = self.selection else {
            return false;
        };
        let (sl, sc) = sel.start;
        let (el, ec) = sel.end;
        // Normalise so start precedes end in reading order.
        let (sl, sc, el, ec) = if (sl, sc) <= (el, ec) {
            (sl, sc, el, ec)
        } else {
            (el, ec, sl, sc)
        };
        if line < sl || line > el {
            return false;
        }
        if sel.block {
            let (lo, hi) = (sc.min(ec), sc.max(ec));
            return column >= lo && column <= hi;
        }
        // Flowing selection: whole intermediate lines are selected; the first
        // and last lines are bounded by their columns.
        let after_start = line > sl || column >= sc;
        let before_end = line < el || column <= ec;
        after_start && before_end
    }
}

/// The DEC private modes the input/mouse encoders need to consult. A subset of
/// the full VT mode set — only what changes how the view encodes input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TermMode {
    /// DECCKM: arrow / Home / End keys send SS3 (`ESC O`) rather than CSI.
    pub app_cursor: bool,
    /// DECKPAM: the numeric keypad sends application-mode sequences.
    pub app_keypad: bool,
    /// Bracketed paste (`ESC [ 200 ~` … `ESC [ 201 ~`) is enabled.
    pub bracketed_paste: bool,
    /// The alternate screen buffer is active (full-screen apps).
    pub alt_screen: bool,
    /// Report mouse button press/release (mode 1000).
    pub mouse_report_click: bool,
    /// Report mouse drag while a button is held (mode 1002).
    pub mouse_drag: bool,
    /// Report all mouse motion (mode 1003).
    pub mouse_motion: bool,
    /// Encode mouse reports in SGR form (mode 1006).
    pub sgr_mouse: bool,
    /// Encode mouse reports in UTF-8 form (mode 1005).
    pub utf8_mouse: bool,
    /// Report focus in/out (`ESC [ I` / `ESC [ O`, mode 1004).
    pub focus_in_out: bool,
}

impl TermMode {
    /// Whether *any* mouse-reporting mode is active.
    pub fn mouse_reporting(&self) -> bool {
        self.mouse_report_click || self.mouse_drag || self.mouse_motion
    }
}

/// A scrollback movement request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    /// Move by a signed number of lines (positive = toward older output).
    Delta(i32),
    PageUp,
    PageDown,
    /// Jump to the oldest scrollback line.
    Top,
    /// Jump back to the live prompt (bottom).
    Bottom,
}

/// The kind of text selection to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    /// Free-form character range (default drag).
    Simple,
    /// Rectangular block (Alt-drag).
    Block,
    /// Whole word (double-click).
    Word,
    /// Whole line (triple-click).
    Line,
}

/// Which half of a cell a selection endpoint falls on, derived from the
/// sub-cell pointer position. It decides whether the boundary cell is included,
/// so a right-to-left drag selects the same cells as a left-to-right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellSide {
    Left,
    Right,
}

/// A completed child process's exit result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalExit {
    pub success: bool,
    pub code: Option<u32>,
}

/// An event surfaced by the engine during [`TerminalEngine::advance`], drained
/// by the view after each parse and turned into the widget's callbacks /
/// reactive signals.
#[derive(Debug, Clone, PartialEq)]
pub enum TermEvent {
    /// The window/tab title changed (OSC 0 / OSC 2).
    Title(String),
    /// The title was reset to the default.
    ResetTitle,
    /// The bell rang (BEL / `^G`).
    Bell,
    /// The child asked to place text on the clipboard (OSC 52). The view gates
    /// this behind an opt-in policy before honouring it.
    ClipboardStore(String),
    /// The current working directory changed (OSC 7), as a URI/path string.
    CwdChanged(String),
    /// The cursor blink preference changed.
    CursorBlinkChanged(bool),
    /// The child process exited.
    Exited(TerminalExit),
}

/// The terminal engine: owns the PTY child and the VT emulation. Lives on the
/// UI thread (it is not required to be `Send`); the view feeds it PTY bytes via
/// [`Self::advance`] and reads it back via [`Self::snapshot`].
pub trait TerminalEngine {
    /// Feed a chunk of the child's PTY output through the VT parser, updating
    /// the grid. Called on the UI thread from the reader-thread delivery path.
    fn advance(&mut self, bytes: &[u8]);

    /// Write bytes to the child's PTY input (the encoded keystrokes / paste).
    fn write(&mut self, bytes: &[u8]);

    /// Resize the PTY and the emulation grid.
    fn resize(&mut self, geom: PtyGeom);

    /// Produce an owned snapshot of the current visible grid + cursor.
    fn snapshot(&self) -> GridSnapshot;

    /// Move the scrollback viewport.
    fn scroll(&mut self, scroll: Scroll);

    /// The current DEC private mode state relevant to input encoding.
    fn mode(&self) -> TermMode;

    /// Number of lines currently in scrollback above the viewport.
    fn history_len(&self) -> usize;

    /// How many lines the viewport is scrolled up (0 = at the live prompt).
    fn display_offset(&self) -> usize;

    /// Begin a selection anchored at viewport cell `(line, column)`, on the
    /// given half of the cell.
    fn selection_start(&mut self, line: usize, column: usize, side: CellSide, kind: SelectionKind);

    /// Extend the in-progress selection to viewport cell `(line, column)`, on
    /// the given half of the cell.
    fn selection_update(&mut self, line: usize, column: usize, side: CellSide);

    /// Select the entire buffer (scrollback + screen).
    fn select_all(&mut self);

    /// Clear any active selection.
    fn selection_clear(&mut self);

    /// The selected text, if any.
    fn selection_text(&self) -> Option<String>;

    /// Clear the visible screen (scrollback is retained).
    fn clear_screen(&mut self);

    /// Full reset (`RIS`): clear screen + scrollback + all modes.
    fn reset(&mut self);

    /// Drain the events accumulated during the last [`Self::advance`].
    fn drain_events(&mut self) -> Vec<TermEvent>;

    /// Poll the child's exit status without blocking, `None` while it runs.
    fn poll_exit(&mut self) -> Option<TerminalExit>;

    /// Terminate the child process (SIGKILL / TerminateProcess).
    fn kill(&mut self);
}

/// The reader half of a spawned engine — a blocking byte source the view drives
/// on a background thread. `Send` so it can cross the thread boundary.
pub type PtyReader = Box<dyn std::io::Read + Send>;

/// A freshly-spawned engine plus the PTY reader for its child's output.
pub struct SpawnedEngine {
    pub engine: Box<dyn TerminalEngine>,
    pub reader: PtyReader,
}

/// Spawns an engine (PTY child + VT model). The default implementation is
/// [`crate::alacritty_engine::AlacrittyEngineFactory`]; apps can install a
/// different one to swap the backend.
pub trait TerminalEngineFactory: 'static {
    fn spawn(
        &self,
        command: &TerminalCommand,
        geom: PtyGeom,
        scrollback_lines: usize,
    ) -> std::io::Result<SpawnedEngine>;
}
