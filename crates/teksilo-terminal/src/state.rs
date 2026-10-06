// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The terminal widget's mutable state, and the drain that feeds the child's
//! output into the engine.

use std::time::{Duration, Instant};

use teksilo_canvas::sync::Arc;
use teksilo_canvas::{Point, Rect};
use teksilo_core::RepaintTrigger;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::pointer::PointerId;

use crate::color_scheme::ColorScheme;
use crate::engine::{GridSnapshot, PtyGeom, Scroll, TermEvent, TerminalEngine};
use crate::reader::ReaderQueue;
use crate::render::CellMetrics;
use crate::terminal::CursorStyle;

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
    /// The child's output on its way from the reader thread. Shut down when
    /// the widget goes, which ends the reader thread.
    pub(crate) reader: Arc<ReaderQueue>,
    /// Attached to the widget in every build: the reader thread asks it for
    /// a pull when output arrives, and a direct engine mutation (clear,
    /// scroll, …) asks it for a repaint.
    pub(crate) trigger: RepaintTrigger,
    /// The layout direction of the last layout, for the touch affordances the
    /// pull hook re-projects when output moves the text.
    pub(crate) layout_direction: LayoutDirection,

    pub(crate) snapshot: GridSnapshot,
    pub(crate) metrics: CellMetrics,
    pub(crate) cols: usize,
    pub(crate) rows: usize,
    pub(crate) origin: Point,
    /// The widget's own bounds, in window coordinates. Wider than the grid by
    /// the chrome inset, which is why the touch affordances clamp into it
    /// rather than into the grid: a handle hanging under the last row needs
    /// that inset to sit in.
    pub(crate) bounds: Rect,
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
    /// What a direct pointer is reported to the child as. See
    /// [`crate::mouse::TouchReporting`].
    pub(crate) touch_reporting: crate::mouse::TouchReporting,
    /// How many direct contacts are on the surface right now. Counted here
    /// because nothing on `EventContext` answers it, and because the
    /// two-contact rules — never report a finger while a second one is down,
    /// honour one pan session and not two — both need the number.
    pub(crate) contacts: usize,
    /// The contact whose pan is moving the scrollback. A second finger opens a
    /// pan session of its own (sessions are per pointer), and honouring both
    /// would scroll at twice the rate the fingers moved.
    pub(crate) pan_owner: Option<PointerId>,
    /// Sub-line scroll carried between samples.
    ///
    /// The scrollback is a ring position quantised to whole lines, so a pan
    /// sample worth 7 px of a 16 px line is worth **no** lines. Dropping it
    /// makes a slow finger — and a precise trackpad — move nothing at all,
    /// which is what the hand-rolled `/ 16.0` did. Banking it here is what
    /// turns a pixel stream into line steps without losing the remainder.
    pub(crate) scroll_residue: f32,

    /// Where the pointer last was, in **widget-local** space — the space
    /// `WidgetTree::localize_event` hands every pointer event over in.
    ///
    /// Recorded for the wheel. A wheel notch is routed by hover and carries no
    /// position of its own (`WidgetEvent::Scroll::window_position` is `None` for a
    /// mouse), so the cell its VT report names can only come from where the
    /// cursor last was. `None` until the pointer has been over the terminal at
    /// all, which is the only case a report has no cell to name.
    pub(crate) last_local_pointer: Option<Point>,

    pub(crate) drag: Option<DragState>,
    /// The button currently held for mouse *reporting* (drives drag reports),
    /// distinct from a local selection drag.
    pub(crate) mouse_button_held: Option<crate::mouse::MouseButton>,
    pub(crate) prev_cursor_line: usize,
    pub(crate) exit_reported: bool,

    /// Timestamp of the last bell, for the visual-bell flash.
    pub(crate) bell_flash: Option<Instant>,
    /// When the engine shows the synchronized update it holds back, if it
    /// holds one: the frame step asks for a pull then.
    pub(crate) sync_deadline: Option<Instant>,
    /// The owner's frame-request handle (set at build), so the visual bell and
    /// cursor blink can schedule follow-up frames.
    pub(crate) frame_request: Option<std::rc::Rc<std::cell::Cell<bool>>>,
    /// The tree's one-shot wake slot (set at build), so the caret blink wakes
    /// the loop for its next toggle instead of pumping frames.
    pub(crate) wake_at: Option<std::rc::Rc<std::cell::Cell<Option<Instant>>>>,
}

impl TerminalState {
    pub(crate) fn new(scheme: ColorScheme) -> Self {
        Self {
            engine: None,
            reader: Arc::new(ReaderQueue::new()),
            trigger: RepaintTrigger::new(),
            layout_direction: LayoutDirection::default(),
            snapshot: blank_snapshot(80, 24),
            metrics: CellMetrics {
                width: 8.0,
                height: 16.0,
            },
            cols: 80,
            rows: 24,
            origin: Point::ZERO,
            bounds: Rect::ZERO,
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
            touch_reporting: crate::mouse::TouchReporting::Off,
            contacts: 0,
            pan_owner: None,
            scroll_residue: 0.0,
            last_local_pointer: None,
            drag: None,
            mouse_button_held: None,
            prev_cursor_line: 0,
            exit_reported: false,
            bell_flash: None,
            sync_deadline: None,
            frame_request: None,
            wake_at: None,
        }
    }

    /// Rebuild the cached snapshot from the engine (called after any mutation).
    pub(crate) fn refresh_snapshot(&mut self) {
        if let Some(engine) = self.engine.as_ref() {
            let snapshot = engine.snapshot();
            self.set_snapshot(snapshot);
        }
    }

    /// Replace the cached snapshot. A caret that comes back into view (output
    /// showing it again, the view scrolled back to it) restarts its blink,
    /// which stops while no caret is visible: see
    /// [`frame_step`](crate::terminal::frame_step).
    pub(crate) fn set_snapshot(&mut self, snapshot: GridSnapshot) {
        let reappeared = snapshot.cursor.visible && !self.snapshot.cursor.visible;
        self.snapshot = snapshot;
        if reappeared && self.blinks() {
            self.restart_blink(Instant::now());
        }
    }

    /// Whether the caret blinks: focused in an active window, blinking
    /// enabled, and a caret in view to blink.
    pub(crate) fn blinks(&self) -> bool {
        self.focused && self.window_active && self.cursor_blink && self.snapshot.cursor.visible
    }

    /// Start the blink at `now` with the caret on, and wake for its first
    /// toggle.
    pub(crate) fn restart_blink(&mut self, now: Instant) {
        self.blink_on = true;
        self.blink_last = Some(now);
        self.schedule_wake(now + crate::terminal::BLINK_INTERVAL);
    }

    /// Wake the event loop at `at`, unless something already wants it awake
    /// sooner: never push an earlier pending wake later.
    pub(crate) fn schedule_wake(&self, at: Instant) {
        if let Some(slot) = &self.wake_at {
            let merged = match slot.get() {
                Some(existing) if existing <= at => existing,
                _ => at,
            };
            slot.set(Some(merged));
        }
    }

    /// Repaint the terminal after a direct engine mutation that produces no
    /// child echo (coalesced; only this widget repaints).
    pub(crate) fn wake(&self) {
        self.trigger.request_repaint();
    }
}

/// How long one pull may spend parsing the child's output: what it may
/// hold the UI thread for. What is left waits for the next pull.
pub(crate) const PULL_BUDGET: Duration = Duration::from_millis(8);

/// The output parsed between two looks at [`PULL_BUDGET`].
pub(crate) const PULL_CHUNK: usize = 64 << 10;

/// The result of draining the child's pending output during a frame.
pub(crate) struct DrainResult {
    pub(crate) events: Vec<TermEvent>,
    pub(crate) eof: bool,
    pub(crate) content_changed: bool,
    /// Output is left for the next pull: the budget ran out first.
    pub(crate) more: bool,
}

/// Take the child's pending bytes, feed them to the engine, and rebuild the
/// cached snapshot. Returns the engine events + EOF flag for the widget to act
/// on (it owns the user callbacks / reactive signals).
///
/// Parses [`PULL_CHUNK`] at a time until the queue is empty or
/// [`PULL_BUDGET`] is spent, and always at least one chunk, so a flood
/// neither stalls the UI thread nor stops making progress.
pub(crate) fn drain_and_advance(state: &mut TerminalState) -> DrainResult {
    // The trigger's flag was taken before the pull hook ran, so a read that
    // lands after this take asks for a fresh pull.
    let started = Instant::now();
    let mut content_changed = false;
    // A synchronized update the child never ended is shown once its deadline
    // has passed (the frame step asks for this pull then).
    if let Some(engine) = state.engine.as_mut()
        && engine
            .synchronized_update_deadline()
            .is_some_and(|deadline| deadline <= started)
    {
        engine.end_synchronized_update();
        content_changed = true;
    }
    let (eof, more) = loop {
        let taken = state.reader.take_up_to(PULL_CHUNK);
        if !taken.bytes.is_empty()
            && let Some(engine) = state.engine.as_mut()
        {
            engine.advance(&taken.bytes);
            content_changed = true;
        }
        if !taken.more || started.elapsed() >= PULL_BUDGET {
            break (taken.eof, taken.more);
        }
    };

    let mut events = Vec::new();
    if content_changed && let Some(engine) = state.engine.as_mut() {
        events = engine.drain_events();
        if state.scroll_on_output {
            engine.scroll(Scroll::Bottom);
        }
    }
    if content_changed {
        state.refresh_snapshot();
    }
    state.sync_deadline = state
        .engine
        .as_ref()
        .and_then(|engine| engine.synchronized_update_deadline());
    if let Some(deadline) = state.sync_deadline {
        state.schedule_wake(deadline);
    }

    DrainResult {
        events,
        eof,
        content_changed,
        more,
    }
}

/// Compute the grid dimensions + PTY geometry + content origin for a given
/// widget bounds and cell metrics. `inset` is the chrome padding.
pub(crate) fn compute_layout(
    bounds: teksilo_canvas::Rect,
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
