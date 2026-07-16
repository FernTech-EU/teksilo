//! The default [`TerminalEngine`] backend: `alacritty_terminal`'s VT model
//! (parser + grid + scrollback + selection) driven over a `portable-pty` child.
//!
//! This is the *only* place that touches the emulation crate. It reuses
//! Alacritty's `Term` + `vte` parser wholesale — nothing here re-implements
//! escape-code handling — and translates its grid/cursor/colours/events into
//! the engine-agnostic vocabulary in [`crate::engine`] that the view consumes.

use std::cell::RefCell;
use std::rc::Rc;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll as AScroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Config, Term, TermMode as AMode};
use alacritty_terminal::vte::ansi::{
    Color as AColor, CursorShape as ACursorShape, NamedColor, Processor, Rgb,
};

use crate::color_scheme::TermColor;
use crate::engine::{
    Cell, CellAttrs, CellSide, CursorInfo, GridSnapshot, PtyGeom, Scroll, SelectionKind,
    SpawnedEngine, TermCursorShape, TermEvent, TermMode, TerminalCommand, TerminalEngine,
    TerminalEngineFactory, TerminalExit,
};
use crate::pty::{self, Pty};

/// Collects the VT model's out-of-band events (title / bell / OSC …) during a
/// parse so the engine can drain them afterwards. Shared with the owning
/// [`AlacrittyEngine`] through an `Rc` — everything here is single-threaded.
#[derive(Clone)]
struct EventProxy {
    queue: Rc<RefCell<Vec<Event>>>,
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        self.queue.borrow_mut().push(event);
    }
}

/// The default terminal engine: `alacritty_terminal::Term` + its `vte` parser
/// over a `portable-pty` child.
pub struct AlacrittyEngine {
    term: Term<EventProxy>,
    parser: Processor,
    pty: Pty,
    raw_events: Rc<RefCell<Vec<Event>>>,
    pending: Vec<TermEvent>,
    geom: PtyGeom,
}

impl AlacrittyEngine {
    fn window_size(&self) -> WindowSize {
        let cols = self.geom.cols.max(1);
        let rows = self.geom.rows.max(1);
        WindowSize {
            num_lines: rows,
            num_cols: cols,
            cell_width: (self.geom.pixel_width / cols).max(1),
            cell_height: (self.geom.pixel_height / rows).max(1),
        }
    }

    /// Handle the events the parser queued: forward the terminal's own replies
    /// (`PtyWrite`, DA/DSR responses, size/colour queries) to the child, and
    /// surface the rest to the view as [`TermEvent`]s.
    fn pump_events(&mut self) {
        let events: Vec<Event> = std::mem::take(&mut *self.raw_events.borrow_mut());
        for event in events {
            match event {
                Event::PtyWrite(text) => self.pty.write(text.as_bytes()),
                Event::TextAreaSizeRequest(callback) => {
                    let reply = callback(self.window_size());
                    self.pty.write(reply.as_bytes());
                }
                Event::ColorRequest(index, callback) => {
                    let reply = callback(default_rgb(index));
                    self.pty.write(reply.as_bytes());
                }
                Event::ClipboardLoad(_, callback) => {
                    // Deny OSC 52 paste-from-clipboard by returning empty text
                    // (a program should never be able to read the user's
                    // clipboard silently).
                    let reply = callback("");
                    self.pty.write(reply.as_bytes());
                }
                Event::ClipboardStore(_, text) => {
                    self.pending.push(TermEvent::ClipboardStore(text));
                }
                Event::Title(title) => self.pending.push(TermEvent::Title(title)),
                Event::ResetTitle => self.pending.push(TermEvent::ResetTitle),
                Event::Bell => self.pending.push(TermEvent::Bell),
                // The view drives cursor blink from its own focus policy, so the
                // app's DECSCUSR blink preference is not surfaced. `Exit` /
                // `ChildExit` are driven by Alacritty's own IO loop, which we
                // don't use — child death is detected via the PTY reader's EOF
                // + `poll_exit`. The rest are internal.
                Event::CursorBlinkingChange
                | Event::Exit
                | Event::ChildExit(_)
                | Event::MouseCursorDirty
                | Event::Wakeup => {}
            }
        }
    }

    /// Convert a viewport row to an absolute buffer line, accounting for how far
    /// the view is scrolled up into the scrollback.
    fn viewport_to_point(&self, line: usize, column: usize) -> Point {
        let offset = self.term.grid().display_offset() as i32;
        Point::new(Line(line as i32 - offset), Column(column))
    }
}

impl TerminalEngine for AlacrittyEngine {
    fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
        self.pump_events();
    }

    fn write(&mut self, bytes: &[u8]) {
        self.pty.write(bytes);
    }

    fn resize(&mut self, geom: PtyGeom) {
        self.geom = geom;
        self.pty.resize(geom);
        self.term
            .resize(alacritty_terminal::term::test::TermSize::new(
                geom.cols as usize,
                geom.rows as usize,
            ));
    }

    fn snapshot(&self) -> GridSnapshot {
        let columns = self.term.columns();
        let screen_lines = self.term.screen_lines();
        let mut cells = vec![Cell::default(); columns * screen_lines];

        let content = self.term.renderable_content();
        let display_offset = content.display_offset;
        let colors = content.colors;

        for item in content.display_iter {
            let row = item.point.line.0 + display_offset as i32;
            let col = item.point.column.0;
            if row < 0 || row as usize >= screen_lines || col >= columns {
                continue;
            }
            cells[row as usize * columns + col] = convert_cell(item.cell, colors);
        }

        // Selection range → viewport span (clamp scrolled-away endpoints).
        let selection = content.selection.map(|range| {
            let to_view = |p: Point| -> (usize, usize) {
                let line = (p.line.0 + display_offset as i32)
                    .clamp(0, screen_lines.saturating_sub(1) as i32)
                    as usize;
                let col = p.column.0.min(columns.saturating_sub(1));
                (line, col)
            };
            crate::engine::SelectionSpan {
                start: to_view(range.start),
                end: to_view(range.end),
                block: range.is_block,
            }
        });

        // The cursor point is in the same buffer coordinate space as the cells.
        let cur = content.cursor;
        let cur_row = cur.point.line.0 + display_offset as i32;
        let in_view = cur_row >= 0 && (cur_row as usize) < screen_lines;
        let cursor = CursorInfo {
            line: cur_row.max(0) as usize,
            column: cur.point.column.0.min(columns.saturating_sub(1)),
            shape: convert_cursor_shape(cur.shape),
            visible: in_view && cur.shape != ACursorShape::Hidden,
        };

        GridSnapshot {
            columns,
            screen_lines,
            cells,
            cursor,
            selection,
            display_offset,
            history_len: self.term.grid().history_size(),
        }
    }

    fn scroll(&mut self, scroll: Scroll) {
        let a = match scroll {
            Scroll::Delta(n) => AScroll::Delta(n),
            Scroll::PageUp => AScroll::PageUp,
            Scroll::PageDown => AScroll::PageDown,
            Scroll::Top => AScroll::Top,
            Scroll::Bottom => AScroll::Bottom,
        };
        self.term.scroll_display(a);
    }

    fn mode(&self) -> TermMode {
        let m = self.term.mode();
        TermMode {
            app_cursor: m.contains(AMode::APP_CURSOR),
            app_keypad: m.contains(AMode::APP_KEYPAD),
            bracketed_paste: m.contains(AMode::BRACKETED_PASTE),
            alt_screen: m.contains(AMode::ALT_SCREEN),
            mouse_report_click: m.contains(AMode::MOUSE_REPORT_CLICK),
            mouse_drag: m.contains(AMode::MOUSE_DRAG),
            mouse_motion: m.contains(AMode::MOUSE_MOTION),
            sgr_mouse: m.contains(AMode::SGR_MOUSE),
            utf8_mouse: m.contains(AMode::UTF8_MOUSE),
            focus_in_out: m.contains(AMode::FOCUS_IN_OUT),
        }
    }

    fn history_len(&self) -> usize {
        self.term.grid().history_size()
    }

    fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    fn selection_start(&mut self, line: usize, column: usize, side: CellSide, kind: SelectionKind) {
        let ty = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Block => SelectionType::Block,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
        };
        let point = self.viewport_to_point(line, column);
        self.term.selection = Some(Selection::new(ty, point, to_side(side)));
    }

    fn selection_update(&mut self, line: usize, column: usize, side: CellSide) {
        let point = self.viewport_to_point(line, column);
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, to_side(side));
        }
    }

    fn select_all(&mut self) {
        let history = self.term.grid().history_size();
        let screen_lines = self.term.screen_lines();
        let last_col = self.term.columns().saturating_sub(1);
        let start = Point::new(Line(-(history as i32)), Column(0));
        let end = Point::new(Line(screen_lines as i32 - 1), Column(last_col));
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        self.term.selection = Some(selection);
    }

    fn selection_clear(&mut self) {
        self.term.selection = None;
    }

    fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|s| !s.is_empty())
    }

    fn clear_screen(&mut self) {
        // Clear the visible screen and move home, reusing the emulator itself.
        self.advance(b"\x1b[2J\x1b[H");
    }

    fn reset(&mut self) {
        // RIS — full reset (screen + scrollback + modes).
        self.advance(b"\x1bc");
    }

    fn drain_events(&mut self) -> Vec<TermEvent> {
        std::mem::take(&mut self.pending)
    }

    fn poll_exit(&mut self) -> Option<TerminalExit> {
        self.pty.poll_exit()
    }

    fn kill(&mut self) {
        self.pty.kill();
    }
}

/// Converts an Alacritty cell into the engine-agnostic [`Cell`].
fn convert_cell(cell: &alacritty_terminal::term::cell::Cell, colors: &Colors) -> Cell {
    let f = cell.flags;
    let attrs = CellAttrs {
        bold: f.contains(Flags::BOLD),
        dim: f.contains(Flags::DIM),
        italic: f.contains(Flags::ITALIC),
        underline: f.intersects(
            Flags::UNDERLINE | Flags::UNDERCURL | Flags::DOTTED_UNDERLINE | Flags::DASHED_UNDERLINE,
        ),
        double_underline: f.contains(Flags::DOUBLE_UNDERLINE),
        strikeout: f.contains(Flags::STRIKEOUT),
        inverse: f.contains(Flags::INVERSE),
        hidden: f.contains(Flags::HIDDEN),
        wide: f.contains(Flags::WIDE_CHAR),
        wide_spacer: f.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
    };
    Cell {
        ch: cell.c,
        fg: convert_color(cell.fg, colors),
        bg: convert_color(cell.bg, colors),
        attrs,
        zerowidth: cell.zerowidth().map(<[char]>::to_vec),
    }
}

/// Maps an Alacritty colour to a symbolic [`TermColor`]. An in-band OSC colour
/// override (in `colors`) wins and is baked to concrete RGB; otherwise the
/// colour stays symbolic so the view's [`crate::color_scheme::ColorScheme`]
/// decides the pixels (enabling live re-theming).
fn convert_color(color: AColor, colors: &Colors) -> TermColor {
    match color {
        AColor::Spec(rgb) => TermColor::Rgb(rgb.r, rgb.g, rgb.b),
        AColor::Named(named) => match &colors[named] {
            Some(rgb) => TermColor::Rgb(rgb.r, rgb.g, rgb.b),
            None => named_to_symbolic(named),
        },
        AColor::Indexed(index) => match &colors[index as usize] {
            Some(rgb) => TermColor::Rgb(rgb.r, rgb.g, rgb.b),
            None if index < 16 => TermColor::Ansi(index),
            None => TermColor::Indexed(index),
        },
    }
}

fn named_to_symbolic(named: NamedColor) -> TermColor {
    let n = named as usize;
    match n {
        0..=15 => TermColor::Ansi(n as u8),
        256 => TermColor::DefaultFg,                   // Foreground
        257 => TermColor::DefaultBg,                   // Background
        258 => TermColor::Cursor,                      // Cursor
        259..=266 => TermColor::Ansi((n - 259) as u8), // Dim* → base ANSI (DIM flag dims it)
        267 => TermColor::DefaultFg,                   // BrightForeground
        268 => TermColor::DefaultFg,                   // DimForeground
        _ => TermColor::DefaultFg,
    }
}

fn to_side(side: CellSide) -> Side {
    match side {
        CellSide::Left => Side::Left,
        CellSide::Right => Side::Right,
    }
}

fn convert_cursor_shape(shape: ACursorShape) -> TermCursorShape {
    match shape {
        ACursorShape::Block => TermCursorShape::Block,
        ACursorShape::Underline => TermCursorShape::Underline,
        ACursorShape::Beam => TermCursorShape::Beam,
        ACursorShape::HollowBlock => TermCursorShape::HollowBlock,
        ACursorShape::Hidden => TermCursorShape::Hidden,
    }
}

/// A fixed default RGB for OSC colour queries (`ColorRequest`). Only used to
/// answer a program's "what is colour N?" query so it doesn't wait — the actual
/// rendered palette is the view's [`crate::color_scheme::ColorScheme`].
fn default_rgb(index: usize) -> Rgb {
    const BASE16: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xcc, 0x00, 0x00),
        (0x4e, 0x9a, 0x06),
        (0xc4, 0xa0, 0x00),
        (0x34, 0x65, 0xa4),
        (0x75, 0x50, 0x7b),
        (0x06, 0x98, 0x9a),
        (0xd3, 0xd7, 0xcf),
        (0x55, 0x57, 0x53),
        (0xef, 0x29, 0x29),
        (0x8a, 0xe2, 0x34),
        (0xfc, 0xe9, 0x4f),
        (0x72, 0x9f, 0xcf),
        (0xad, 0x7f, 0xa8),
        (0x34, 0xe2, 0xe2),
        (0xee, 0xee, 0xec),
    ];
    let (r, g, b) = match index {
        0..=15 => BASE16[index],
        16..=231 => {
            let i = index - 16;
            let level = |v: usize| -> u8 { if v == 0 { 0 } else { (55 + 40 * v) as u8 } };
            (level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        232..=255 => {
            let gray = (8 + 10 * (index - 232)) as u8;
            (gray, gray, gray)
        }
        257 => (0x1e, 0x22, 0x2a), // background query
        _ => (0xab, 0xb2, 0xbf),   // foreground / cursor / anything else
    };
    Rgb { r, g, b }
}

/// The default engine factory — spawns an [`AlacrittyEngine`].
#[derive(Debug, Default, Clone, Copy)]
pub struct AlacrittyEngineFactory;

impl TerminalEngineFactory for AlacrittyEngineFactory {
    fn spawn(
        &self,
        command: &TerminalCommand,
        geom: PtyGeom,
        scrollback_lines: usize,
    ) -> std::io::Result<SpawnedEngine> {
        let (pty, reader) = pty::spawn(command, geom)?;

        let queue: Rc<RefCell<Vec<Event>>> = Rc::new(RefCell::new(Vec::new()));
        let config = Config {
            scrolling_history: scrollback_lines,
            ..Config::default()
        };
        let size =
            alacritty_terminal::term::test::TermSize::new(geom.cols as usize, geom.rows as usize);
        let term = Term::new(
            config,
            &size,
            EventProxy {
                queue: queue.clone(),
            },
        );

        let engine = AlacrittyEngine {
            term,
            parser: Processor::new(),
            pty,
            raw_events: queue,
            pending: Vec::new(),
            geom,
        };
        Ok(SpawnedEngine {
            engine: Box::new(engine),
            reader,
        })
    }
}
