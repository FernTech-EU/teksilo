//! The terminal widget's mutable state, the PTY-reader background thread, and
//! the per-frame drain that feeds the child's output into the engine.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bastyde_canvas::Point;
use bastyde_core::window::BastydeWindowId;
use bastyde_core::{AppEventPoster, RepaintWindowRequest};

use crate::color_scheme::ColorScheme;
use crate::engine::{GridSnapshot, PtyGeom, Scroll, TermEvent, TerminalEngine};
use crate::render::CellMetrics;
use crate::terminal::CursorStyle;

/// Bytes read from the child, shared between the reader thread and the UI
/// thread behind a `Mutex`.
#[derive(Default)]
pub(crate) struct ReaderShared {
    pub(crate) queue: Vec<u8>,
    pub(crate) eof: bool,
}

/// An in-progress selection drag.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DragState {
    /// Whether the pointer has moved since the press (so a bare click clears the
    /// selection instead of leaving a zero-width one).
    pub(crate) moved: bool,
}

/// The widget's mutable state, shared (via `Rc<RefCell<_>>`) between the widget,
/// its event handlers, and the [`crate::TerminalController`].
pub(crate) struct TerminalState {
    /// The engine (PTY + VT model). `None` until the post-mount spawn runs.
    pub(crate) engine: Option<Box<dyn TerminalEngine>>,
    pub(crate) reader: Arc<Mutex<ReaderShared>>,
    /// Set on drop / teardown so the reader thread exits promptly.
    pub(crate) reader_stop: Arc<AtomicBool>,
    /// Coalescing flag for repaint requests: `true` while a request is already
    /// outstanding. Set when posting (reader thread or UI mutation), cleared by
    /// `drain_and_advance` on the UI thread — so a flood of PTY reads posts at
    /// most one pending repaint instead of one per chunk.
    pub(crate) repaint_pending: Arc<AtomicBool>,
    /// Poster for waking the UI loop after a direct engine mutation (clear,
    /// scroll, …) that produces no child echo.
    pub(crate) poster: Option<Arc<dyn AppEventPoster>>,
    /// The window hosting this terminal, so a repaint request can target it.
    pub(crate) window_id: Option<BastydeWindowId>,

    pub(crate) snapshot: GridSnapshot,
    pub(crate) metrics: CellMetrics,
    pub(crate) cols: usize,
    pub(crate) rows: usize,
    pub(crate) origin: Point,
    pub(crate) geom: PtyGeom,

    pub(crate) scheme: ColorScheme,
    pub(crate) focused: bool,
    pub(crate) window_active: bool,

    pub(crate) blink_on: bool,
    pub(crate) blink_last: Option<Instant>,
    pub(crate) cursor_style_pref: CursorStyle,
    pub(crate) cursor_blink: bool,

    pub(crate) alt_sends_escape: bool,
    pub(crate) scroll_on_output: bool,
    pub(crate) read_only: bool,
    pub(crate) mouse_reporting: bool,

    pub(crate) drag: Option<DragState>,
    /// The button currently held for mouse *reporting* (drives drag reports),
    /// distinct from a local selection drag.
    pub(crate) mouse_button_held: Option<crate::mouse::MouseButton>,
    pub(crate) prev_cursor_line: usize,
    pub(crate) exit_reported: bool,

    /// Timestamp of the last bell, for the visual-bell flash.
    pub(crate) bell_flash: Option<Instant>,
    /// The owner's frame-request handle (set at build), so the visual bell and
    /// cursor blink can schedule follow-up frames.
    pub(crate) frame_request: Option<std::rc::Rc<std::cell::Cell<bool>>>,
}

impl TerminalState {
    pub(crate) fn new(scheme: ColorScheme) -> Self {
        Self {
            engine: None,
            reader: Arc::new(Mutex::new(ReaderShared::default())),
            reader_stop: Arc::new(AtomicBool::new(false)),
            repaint_pending: Arc::new(AtomicBool::new(false)),
            poster: None,
            window_id: None,
            snapshot: blank_snapshot(80, 24),
            metrics: CellMetrics {
                width: 8.0,
                height: 16.0,
            },
            cols: 80,
            rows: 24,
            origin: Point::ZERO,
            geom: PtyGeom::new(80, 24, 0, 0),
            scheme,
            focused: false,
            window_active: true,
            blink_on: true,
            blink_last: None,
            cursor_style_pref: CursorStyle::Block,
            cursor_blink: true,
            alt_sends_escape: true,
            scroll_on_output: false,
            read_only: false,
            mouse_reporting: true,
            drag: None,
            mouse_button_held: None,
            prev_cursor_line: 0,
            exit_reported: false,
            bell_flash: None,
            frame_request: None,
        }
    }

    /// Rebuild the cached snapshot from the engine (called after any mutation).
    pub(crate) fn refresh_snapshot(&mut self) {
        if let Some(engine) = self.engine.as_ref() {
            self.snapshot = engine.snapshot();
        }
    }

    /// Wake the UI loop to repaint after a direct engine mutation (coalesced).
    pub(crate) fn wake(&self) {
        if let (Some(poster), Some(window_id)) = (&self.poster, self.window_id) {
            post_repaint(poster, window_id, &self.repaint_pending);
        }
    }
}

/// Post a coalesced [`RepaintWindowRequest`]: fire only when one isn't already
/// outstanding. `pending` is cleared by [`drain_and_advance`] on the UI thread,
/// so a burst of PTY reads collapses to a single in-flight repaint.
pub(crate) fn post_repaint(
    poster: &Arc<dyn AppEventPoster>,
    window_id: BastydeWindowId,
    pending: &AtomicBool,
) {
    if !pending.swap(true, Ordering::AcqRel) {
        poster.post_external(Box::new(RepaintWindowRequest { window_id }));
    }
}

/// The result of draining the child's pending output during a frame.
pub(crate) struct DrainResult {
    pub(crate) events: Vec<TermEvent>,
    pub(crate) eof: bool,
    pub(crate) content_changed: bool,
}

/// Take the child's pending bytes, feed them to the engine, and rebuild the
/// cached snapshot. Returns the engine events + EOF flag for the widget to act
/// on (it owns the user callbacks / reactive signals).
pub(crate) fn drain_and_advance(state: &mut TerminalState) -> DrainResult {
    // Re-arm coalescing BEFORE reading the queue: any read that appends after
    // this point re-posts a fresh repaint (rather than being lost because a
    // request still looked outstanding).
    state.repaint_pending.store(false, Ordering::Release);
    let (bytes, eof) = {
        let mut shared = state
            .reader
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (std::mem::take(&mut shared.queue), shared.eof)
    };

    let mut events = Vec::new();
    let mut content_changed = false;

    if !bytes.is_empty()
        && let Some(engine) = state.engine.as_mut()
    {
        engine.advance(&bytes);
        events = engine.drain_events();
        if state.scroll_on_output {
            engine.scroll(Scroll::Bottom);
        }
        content_changed = true;
    }

    if content_changed {
        state.refresh_snapshot();
    }

    DrainResult {
        events,
        eof,
        content_changed,
    }
}

/// Compute the grid dimensions + PTY geometry + content origin for a given
/// widget bounds and cell metrics. `inset` is the chrome padding.
pub(crate) fn compute_layout(
    bounds: bastyde_canvas::Rect,
    metrics: CellMetrics,
    inset: f32,
) -> (usize, usize, PtyGeom, Point) {
    let content_w = (bounds.width - inset * 2.0).max(0.0);
    let content_h = (bounds.height - inset * 2.0).max(0.0);
    let cols = if metrics.width > 0.0 {
        (content_w / metrics.width).floor() as usize
    } else {
        0
    }
    .max(1);
    let rows = if metrics.height > 0.0 {
        (content_h / metrics.height).floor() as usize
    } else {
        0
    }
    .max(1);
    let geom = PtyGeom::new(
        cols as u16,
        rows as u16,
        (cols as f32 * metrics.width) as u16,
        (rows as f32 * metrics.height) as u16,
    );
    let origin = Point::new(bounds.x + inset, bounds.y + inset);
    (cols, rows, geom, origin)
}

/// A blank snapshot of the given size (shown before the engine has produced
/// anything).
pub(crate) fn blank_snapshot(cols: usize, rows: usize) -> GridSnapshot {
    GridSnapshot {
        columns: cols,
        screen_lines: rows,
        cells: vec![crate::engine::Cell::default(); cols * rows],
        cursor: crate::engine::CursorInfo {
            line: 0,
            column: 0,
            shape: crate::engine::TermCursorShape::Block,
            visible: false,
        },
        selection: None,
        display_offset: 0,
        history_len: 0,
    }
}

/// Spawn the PTY-reader background thread. It blocks on `read`, appends bytes to
/// the shared queue, and wakes the UI loop after each chunk (and on EOF).
pub(crate) fn spawn_reader_thread(
    mut reader: Box<dyn Read + Send>,
    shared: Arc<Mutex<ReaderShared>>,
    stop: Arc<AtomicBool>,
    pending: Arc<AtomicBool>,
    poster: Arc<dyn AppEventPoster>,
    window_id: BastydeWindowId,
) {
    let wake = move || post_repaint(&poster, window_id, &pending);
    let _ = std::thread::Builder::new()
        .name("bastyde-terminal-pty".into())
        .spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                match reader.read(&mut buf) {
                    Ok(0) => {
                        shared
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .eof = true;
                        wake();
                        break;
                    }
                    Ok(n) => {
                        shared
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .queue
                            .extend_from_slice(&buf[..n]);
                        wake();
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        shared
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .eof = true;
                        wake();
                        break;
                    }
                }
            }
        });
}
