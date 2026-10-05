// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The terminal on `RepaintTrigger`: output reaches it through the reader
//! thread and the pull hook, repaints only it, is taken in when it is not
//! shown, and ends with the tree. Headless, against `MemoryEngine`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use teksilo_canvas::wake::{CountingWaker, RedrawWaker, WakeKind};
use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::event_source::{AppEventPoster, SubscriptionId, TreeAppContext};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_core::window::NoopWindowOps;
use teksilo_tokens::Color;

use super::{
    BELL_FLASH, BLINK_INTERVAL, FrameStep, Terminal, TerminalClosePolicy, TerminalController,
    frame_step,
};
use crate::color_scheme::ColorScheme;
use crate::memory::{MemoryEngineFactory, MemoryOutput, MemoryShared};
use crate::reader::READER_QUEUE_CAP;
use crate::state::{PULL_CHUNK, TerminalState};

const WAIT: Duration = Duration::from_secs(10);

/// Counts its paints, beside the terminal.
#[derive(Debug)]
struct PaintProbe {
    paints: Rc<Cell<u32>>,
}

impl Widget for PaintProbe {
    fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        p.resolve(20.0, 20.0).into()
    }
    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _c: &PaintContext) {
        self.paints.set(self.paints.get() + 1);
        canvas.fill_rect(bounds, Color::from_rgb(0.5, 0.5, 0.5));
    }
}

/// The terminal and the probe side by side.
#[derive(Debug)]
struct Row(Vec<WidgetId>);

impl Widget for Row {
    fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        p.resolve(600.0, 300.0).into()
    }
    fn place_children(
        &self,
        bounds: Rect,
        _p: SizeProposal,
        children: &mut [WidgetPlacement],
        _c: &LayoutContext,
    ) {
        let mut x = bounds.x;
        for child in children.iter_mut() {
            let width = if child.id == self.0[0] { 400.0 } else { 20.0 };
            child.origin = Point::new(x, bounds.y);
            child.size = Size::new(width, 300.0);
            x += width;
        }
    }
    fn children(&self) -> Vec<WidgetId> {
        self.0.clone()
    }
}

/// Counts what is posted to the event loop: a terminal posts nothing.
#[derive(Default)]
struct RecordingPoster(Mutex<usize>);

impl AppEventPoster for RecordingPoster {
    fn post_subscription_event(&self, _: SubscriptionId, _: Box<dyn std::any::Any + Send>) {
        *self.0.lock().unwrap() += 1;
    }
    fn post_external(&self, _: Box<dyn std::any::Any + Send>) {
        *self.0.lock().unwrap() += 1;
    }
}

struct Fixture {
    tree: WidgetTree,
    term: WidgetId,
    state: Rc<RefCell<TerminalState>>,
    shared: Rc<RefCell<MemoryShared>>,
    output: MemoryOutput,
    waker: Arc<CountingWaker>,
    poster: Arc<RecordingPoster>,
    probe_paints: Rc<Cell<u32>>,
    controller: TerminalController,
}

impl Fixture {
    fn with(configure: impl FnOnce(Terminal) -> Terminal) -> Self {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let waker = Arc::new(CountingWaker::new());
        tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
        let poster = Arc::new(RecordingPoster::default());
        tree.set_app_context(Rc::new(
            TreeAppContext::empty().with_poster(poster.clone() as Arc<dyn AppEventPoster>),
        ));
        let factory = MemoryEngineFactory::new();
        let shared = factory.shared();
        let output = factory.output();
        let terminal = configure(Terminal::with_engine_factory(factory));
        let state = terminal.state.clone();
        let controller = terminal.controller();
        let term = tree.add(terminal);
        let probe_paints = Rc::new(Cell::new(0));
        let probe = tree.add(PaintProbe {
            paints: probe_paints.clone(),
        });
        tree.add(Row(vec![term, probe]));
        let mut f = Self {
            tree,
            term,
            state,
            shared,
            output,
            waker,
            poster,
            probe_paints,
            controller,
        };
        f.tree.layout(SizeProposal::exact(600.0, 300.0));
        f.tree.run_mount_actions(&mut NoopWindowOps);
        f.frame();
        f
    }

    fn new() -> Self {
        Self::with(|t| t)
    }

    fn frame(&mut self) {
        self.tree.layout(SizeProposal::exact(600.0, 300.0));
        let _ = self.tree.render();
    }

    fn stats(&self) -> teksilo_core::RepaintTriggerStats {
        self.state.borrow().trigger.stats()
    }

    /// Write `bytes` and wait until the reader thread has taken them.
    fn produce(&self, bytes: &[u8]) {
        self.output.write(bytes);
        let deadline = Instant::now() + WAIT;
        while self.output.pending_len() > 0 {
            assert!(Instant::now() < deadline, "the reader never read");
            std::thread::yield_now();
        }
    }
}

/// J.1: output wakes the terminal's window, which draws a frame for it since
/// the terminal is shown; that frame takes it in and repaints the terminal
/// and nothing else, and nothing is posted through the app's poster.
#[test]
fn j1_output_repaints_only_the_terminal_and_posts_nothing() {
    for _ in 0..100 {
        let mut f = Fixture::new();
        let probe = f.probe_paints.get();
        f.output.write(b"hi");
        assert!(f.waker.wait_for(1, WAIT), "the output woke the window");
        assert_eq!(
            f.waker.count_of(WakeKind::Layout),
            f.waker.count(),
            "state wakes: they reach a window that draws nothing"
        );
        assert!(
            f.tree.off_thread_needs_frame(),
            "shown: the window draws a frame for it"
        );
        f.tree.layout(SizeProposal::exact(600.0, 300.0));
        assert_eq!(f.shared.borrow().advanced, b"hi", "taken in by the layout");
        assert!(f.tree.needs_render(), "and the terminal marked for paint");
        let _ = f.tree.render();
        assert!(!f.tree.needs_render());
        assert_eq!(f.probe_paints.get(), probe, "nothing else repaints");
        assert_eq!(
            *f.poster.0.lock().unwrap(),
            0,
            "nothing posted through the app's poster"
        );
        assert!(f.stats().pulls >= 1);
    }
}

/// J.2: a burst of reads before the next frame wakes once, and that frame
/// takes the whole burst in.
#[test]
fn j2_a_burst_of_reads_wakes_once() {
    for _ in 0..100 {
        let mut f = Fixture::new();
        for byte in 0..100u8 {
            f.produce(&[byte]);
        }
        // The last read's request may still be on its way: wait for it.
        let deadline = Instant::now() + WAIT;
        while f.stats().wakes + f.stats().wakes_coalesced < 100 {
            assert!(Instant::now() < deadline, "{:?}", f.stats());
            std::thread::yield_now();
        }
        let s = f.stats();
        assert_eq!(s.requests, 100, "one request per read");
        assert_eq!(s.wakes, 1, "one wake for the burst");
        assert_eq!(s.wakes_coalesced, 99);
        f.frame();
        assert_eq!(f.shared.borrow().advanced, (0..100u8).collect::<Vec<_>>());
    }
}

/// J.3a: the child's output ending reports its exit.
#[test]
fn j3a_the_end_of_the_output_reports_the_exit() {
    for _ in 0..100 {
        let mut f = Fixture::new();
        let controller = f.controller.clone();
        assert!(controller.child_running_signal().get());
        f.output.close();
        assert!(f.output.wait_reader_released(WAIT));
        assert!(f.waker.wait_for(1, WAIT));
        f.tree.layout(SizeProposal::exact(600.0, 300.0));
        assert!(
            !f.tree.needs_render(),
            "a pull that changes nothing on screen draws nothing"
        );
        let exit = controller.exit_signal().get().expect("an exit");
        assert!(exit.success && exit.code.is_none());
        assert!(!controller.child_running_signal().get());
    }
}

/// J.3b: the reader thread ends with the tree. One blocked in a read ends at
/// the read's return: a killed child closes its output, a dropped engine too,
/// and nothing else does.
#[test]
fn j3b_the_reader_thread_ends_with_the_tree() {
    for _ in 0..100 {
        let f = Fixture::new();
        let (output, shared) = (f.output.clone(), f.shared.clone());
        drop(f);
        assert!(output.wait_reader_released(WAIT), "killed: the reader ends");
        assert!(shared.borrow().killed);

        let f = Fixture::new();
        let (output, shared, held) = (f.output.clone(), f.shared.clone(), f.state.clone());
        drop(f);
        assert!(
            output.wait_reader_released(WAIT),
            "killed while something still holds the engine: its output ends"
        );
        assert!(shared.borrow().killed);
        drop(held);

        let f = Fixture::with(|t| t.on_close(TerminalClosePolicy::LeaveRunning));
        let output = f.output.clone();
        drop(f);
        assert!(
            output.wait_reader_released(WAIT),
            "the engine dropped: it ends"
        );

        let f = Fixture::with(|t| t.on_close(TerminalClosePolicy::LeaveRunning));
        let (output, shared, held) = (f.output.clone(), f.shared.clone(), f.state.clone());
        assert!(output.wait_reader_blocked(WAIT), "the reader reads");
        drop(f);
        assert!(!shared.borrow().killed, "left running: not killed");
        assert!(
            !output.wait_reader_released(Duration::from_millis(20)),
            "the engine is still held: the read goes on"
        );
        drop(held);
        assert!(
            output.wait_reader_released(WAIT),
            "the engine dropped with the last holder: its child's output ends"
        );
    }
}

/// J.4: direct mutations repaint the terminal once, through its trigger.
#[test]
fn j4_two_clears_before_a_frame_repaint_once() {
    for _ in 0..100 {
        let mut f = Fixture::new();
        let probe = f.probe_paints.get();
        let controller = f.controller.clone();
        let before = f.stats();
        controller.clear();
        controller.clear();
        let s = f.stats();
        assert_eq!(s.wakes, before.wakes + 1);
        assert_eq!(s.wakes_coalesced, before.wakes_coalesced + 1);
        f.frame();
        assert_eq!(f.shared.borrow().clear_screen_calls, 2);
        assert_eq!(f.probe_paints.get(), probe);
        assert_eq!(f.stats().repaints, before.repaints + 1);
    }
}

/// J.5: a focused terminal's caret blinks on deadlines: two frames a second,
/// no frame pump.
#[test]
fn j5_the_caret_blinks_on_deadlines() {
    for _ in 0..100 {
        let mut f = Fixture::new();
        f.tree.focus(f.term);
        f.frame();
        f.frame();
        let last = f.state.borrow().blink_last.expect("the blink started");
        assert_eq!(f.tree.wake_at_handle().get(), Some(last + BLINK_INTERVAL));
        assert!(!f.tree.frame_requested(), "no frame pump");
        let mut on = f.state.borrow().blink_on;
        for _ in 0..4 {
            {
                let mut st = f.state.borrow_mut();
                st.blink_last = st.blink_last.map(|t| t - Duration::from_millis(600));
            }
            f.tree
                .wake_at_handle()
                .set(Some(Instant::now() - Duration::from_millis(1)));
            f.frame();
            let now_on = f.state.borrow().blink_on;
            assert_ne!(now_on, on, "the deadline toggled the caret");
            on = now_on;
            assert!(!f.tree.frame_requested());
        }

        // The window going inactive ends the chain: the deadline already
        // pending fires once, and nothing is scheduled after it.
        f.tree.set_window_active(false);
        f.tree
            .wake_at_handle()
            .set(Some(Instant::now() - Duration::from_millis(1)));
        f.frame();
        assert!(f.state.borrow().blink_on, "the caret stays on");
        assert_eq!(f.tree.wake_at_handle().get(), None, "nothing scheduled");
        assert!(!f.tree.frame_requested());
    }
}

/// A terminal that is not shown still takes its output in, without its
/// window drawing a frame, and shows it when shown again.
#[test]
fn a_dormant_terminal_takes_its_output_in_without_drawing() {
    let mut f = Fixture::new();
    f.tree.set_dormant(f.term);
    f.frame();
    f.output.write(b"busy");
    assert!(f.waker.wait_for(1, WAIT));
    assert_eq!(
        f.waker.count_of(WakeKind::Layout),
        1,
        "not shown: a state wake, which draws nothing"
    );
    f.tree.layout(SizeProposal::exact(600.0, 300.0));
    assert_eq!(
        f.shared.borrow().advanced,
        b"busy",
        "taken in while dormant"
    );
    assert!(
        !f.tree.needs_render(),
        "no frame for a terminal nobody sees"
    );
    assert_eq!(*f.poster.0.lock().unwrap(), 0);
    f.tree.activate(f.term);
    f.tree.layout(SizeProposal::exact(600.0, 300.0));
    assert!(f.tree.needs_render(), "shown again, it repaints");
}

/// The queue stops the reader at its cap until the terminal takes output in,
/// and every byte arrives, in order.
#[test]
fn the_reader_queue_stops_at_its_cap_and_resumes() {
    let mut f = Fixture::new();
    let total = READER_QUEUE_CAP + (1 << 20);
    let bytes: Vec<u8> = (0..total).map(|i| (i % 251) as u8).collect();
    f.output.write(&bytes);
    let deadline = Instant::now() + WAIT;
    loop {
        let queued = f.state.borrow().reader.len();
        if queued >= READER_QUEUE_CAP {
            assert_eq!(queued, READER_QUEUE_CAP, "held at the cap, not past it");
            break;
        }
        assert!(Instant::now() < deadline, "the queue never filled");
        std::thread::yield_now();
    }
    assert!(f.output.pending_len() > 0, "the child waits on its write");
    let deadline = Instant::now() + WAIT;
    while f.shared.borrow().advanced.len() < total {
        assert!(Instant::now() < deadline, "the output never all arrived");
        f.frame();
        std::thread::yield_now();
    }
    assert!(f.shared.borrow().advanced == bytes, "every byte, in order");
}

/// A pull parses no longer than its budget, at least one chunk, and asks
/// for another pull for the rest; every byte arrives, in order.
#[test]
fn a_pull_parses_within_its_budget_and_asks_for_the_rest() {
    let mut f = Fixture::new();
    f.shared.borrow_mut().advance_delay = Some(Duration::from_millis(3));
    let total = PULL_CHUNK * 10;
    let bytes: Vec<u8> = (0..total).map(|i| (i % 251) as u8).collect();
    f.output.write(&bytes);
    let deadline = Instant::now() + WAIT;
    while f.state.borrow().reader.len() < total {
        assert!(Instant::now() < deadline, "the reader never queued it all");
        std::thread::yield_now();
    }
    let wakes = f.waker.count();
    f.tree.layout(SizeProposal::exact(600.0, 300.0));
    let first = f.shared.borrow().advanced.len();
    assert!(
        (PULL_CHUNK..total).contains(&first),
        "one pull parsed part of it, at least a chunk: {first} of {total}"
    );
    assert!(
        f.waker.count() > wakes,
        "and asked for another for the rest"
    );
    let deadline = Instant::now() + WAIT;
    while f.shared.borrow().advanced.len() < total {
        assert!(Instant::now() < deadline, "the rest never arrived");
        f.tree.layout(SizeProposal::exact(600.0, 300.0));
    }
    assert!(f.shared.borrow().advanced == bytes, "every byte, in order");
}

/// A reader held at the cap is let go when the terminal goes, even while
/// the child runs on.
#[test]
fn a_reader_held_at_the_cap_ends_with_the_terminal() {
    let f = Fixture::with(|t| t.on_close(TerminalClosePolicy::LeaveRunning));
    f.output.write(&vec![b'x'; READER_QUEUE_CAP + (1 << 20)]);
    let deadline = Instant::now() + WAIT;
    while f.state.borrow().reader.len() < READER_QUEUE_CAP {
        assert!(Instant::now() < deadline, "the queue never filled");
        std::thread::yield_now();
    }
    let (output, held) = (f.output.clone(), f.state.clone());
    drop(f);
    assert!(
        output.wait_reader_released(WAIT),
        "the reader waiting for space stopped with the terminal"
    );
    drop(held);
}

fn state() -> TerminalState {
    let mut st = TerminalState::new(ColorScheme::default());
    st.focused = true;
    st.window_active = true;
    st.cursor_blink = true;
    st.snapshot.cursor.visible = true;
    st
}

fn with_cursor(st: &TerminalState, visible: bool) -> crate::engine::GridSnapshot {
    let mut snapshot = st.snapshot.clone();
    snapshot.cursor.visible = visible;
    snapshot
}

/// With no caret in view (hidden by the child, or scrolled out), the blink
/// stops: the caret's state resets and nothing is scheduled.
#[test]
fn no_caret_in_view_stops_the_blink() {
    let t0 = Instant::now();
    let mut st = state();
    frame_step(&mut st, t0);
    frame_step(&mut st, t0 + BLINK_INTERVAL);
    assert!(!st.blink_on);
    let hidden = with_cursor(&st, false);
    st.set_snapshot(hidden);
    let step = frame_step(&mut st, t0 + BLINK_INTERVAL * 2);
    assert_eq!(step.wake_at, None, "nothing to wake for");
    assert!(st.blink_on && st.blink_last.is_none());
}

/// A caret coming back into view restarts the blink, on, with a wake for its
/// first toggle; one coming back to a terminal that does not blink, not.
#[test]
fn a_caret_coming_back_restarts_the_blink() {
    let mut st = state();
    let slot = Rc::new(Cell::new(None));
    st.wake_at = Some(slot.clone());
    let hidden = with_cursor(&st, false);
    st.set_snapshot(hidden);
    assert_eq!(slot.get(), None);
    let before = Instant::now();
    let shown = with_cursor(&st, true);
    st.set_snapshot(shown);
    let last = st.blink_last.expect("restarted");
    assert!(last >= before && st.blink_on);
    assert_eq!(slot.get(), Some(last + BLINK_INTERVAL));

    let mut st = state();
    st.focused = false;
    let slot = Rc::new(Cell::new(None));
    st.wake_at = Some(slot.clone());
    let hidden = with_cursor(&st, false);
    st.set_snapshot(hidden);
    let shown = with_cursor(&st, true);
    st.set_snapshot(shown);
    assert_eq!(slot.get(), None, "unfocused: no blink to restart");
}

/// A terminal built while its window is inactive knows it, so focusing it
/// starts no blink until the window is active.
#[test]
fn a_terminal_built_in_an_inactive_window_does_not_blink() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.set_window_active(false);
    let terminal = Terminal::with_engine_factory(MemoryEngineFactory::new());
    let state = terminal.state.clone();
    let id = tree.add(terminal);
    tree.layout(SizeProposal::exact(600.0, 300.0));
    tree.run_mount_actions(&mut NoopWindowOps);
    tree.focus(id);
    for _ in 0..2 {
        tree.layout(SizeProposal::exact(600.0, 300.0));
        let _ = tree.render();
    }
    assert!(!state.borrow().window_active);
    assert!(state.borrow().blink_last.is_none(), "no blink started");
    assert_eq!(tree.wake_at_handle().get(), None, "nothing scheduled");
}

#[test]
fn the_blink_starts_on_and_toggles_at_its_interval() {
    let t0 = Instant::now();
    let mut st = state();
    let step = frame_step(&mut st, t0);
    assert!(st.blink_on && !step.repaint, "seeded, not toggled");
    assert_eq!(step.wake_at, Some(t0 + BLINK_INTERVAL));
    let step = frame_step(&mut st, t0 + Duration::from_millis(499));
    assert!(st.blink_on && !step.repaint);
    let step = frame_step(&mut st, t0 + BLINK_INTERVAL);
    assert!(!st.blink_on && step.repaint);
    assert_eq!(step.wake_at, Some(t0 + BLINK_INTERVAL * 2));
    assert!(!step.another_frame, "a blink pumps no frame");
}

#[test]
fn an_inactive_terminal_shows_its_caret_and_stops_blinking() {
    let t0 = Instant::now();
    let mut st = state();
    frame_step(&mut st, t0);
    frame_step(&mut st, t0 + BLINK_INTERVAL);
    assert!(!st.blink_on);
    st.window_active = false;
    let step = frame_step(&mut st, t0 + BLINK_INTERVAL * 2);
    assert!(st.blink_on && step.repaint);
    assert_eq!(step.wake_at, None, "nothing to wake for");
    assert!(st.blink_last.is_none());
}

#[test]
fn the_visual_bell_fades_then_clears() {
    let t0 = Instant::now();
    let mut st = state();
    st.focused = false;
    st.bell_flash = Some(t0);
    let step = frame_step(&mut st, t0 + Duration::from_millis(50));
    assert_eq!(
        step,
        FrameStep {
            repaint: true,
            wake_at: None,
            another_frame: true,
        }
    );
    let step = frame_step(&mut st, t0 + BELL_FLASH);
    assert!(st.bell_flash.is_none(), "the bell ends");
    assert!(step.repaint && !step.another_frame, "with one last repaint");
}
