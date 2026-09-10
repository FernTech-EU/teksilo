// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Touch and pointer behaviour of the `Terminal` view, driven headlessly
//! through a `WidgetTree` against the `MemoryEngine`.
//!
//! Half of these tests are about the **mouse**. They are here because the touch
//! work changed the answer the primary press gives (`Handled` → `Ignored`, so the
//! gesture arena runs at all) and rewrote the scroll handler around it, and
//! "the mouse is unchanged" is a claim that has to be able to fail.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::event::{Modifiers, PointerButton, ScrollDelta, WidgetEvent};
use teksilo_core::pointer::{PointerInfo, ScrollPhase, ScrollSample, ScrollSource};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_core::window::NoopWindowOps;
use teksilo_terminal::{
    MemoryEngineFactory, MemoryShared, RecipeTerminalStyle, SelectionKind, Terminal, TerminalStyle,
    TouchReporting,
};

fn theme() -> teksilo_core::styles::Theme {
    teksilo_core::presets::intui::light()
}

/// The cell metrics the headless tree resolves: no text backend is installed, so
/// `measure_cell` falls back to the mono style's own numbers. Computing them the
/// same way here keeps the quantisation assertions exact instead of approximate.
fn cell_size() -> (f32, f32) {
    let mono = theme().typography.mono.clone();
    (
        (mono.size * 0.6).max(1.0),
        (mono.size * mono.line_height).max(1.0),
    )
}

/// A window point inside the cell at `(col, row)` of an origin-aligned terminal.
/// The `+ 6.0` is the chrome inset between the widget's bounds and the grid's
/// content origin; a fractional `col`/`row` lands inside the cell rather than on
/// its edge.
fn cell_position(col: f32, row: f32) -> Point {
    let (cw, ch) = cell_size();
    Point::new(cw * col + 6.0, ch * row + 6.0)
}

/// A layout-transparent container that places its child at a fixed offset, so a
/// test can put the terminal somewhere other than the window's top-left corner.
/// `teksilo-terminal` cannot depend on `teksilo-widgets`, so there is no `Padding`
/// to reach for.
#[derive(Debug)]
struct Offset {
    child: WidgetId,
    dx: Rc<std::cell::Cell<f32>>,
    dy: f32,
}

impl Offset {
    fn at(child: WidgetId, dx: f32, dy: f32) -> Self {
        Self {
            child,
            dx: Rc::new(std::cell::Cell::new(dx)),
            dy,
        }
    }
}

impl Widget for Offset {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let dx = self.dx.get();
        for placement in children.iter_mut() {
            placement.origin = Point::new(bounds.x + dx, bounds.y + self.dy);
            placement.size = Size::new(
                (bounds.width - dx).max(0.0),
                (bounds.height - self.dy).max(0.0),
            );
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

/// An ancestor pan claimant that records the vertical lines a declined pan
/// chains out to it.
#[derive(Debug)]
struct PanCatcher {
    child: WidgetId,
    taken: Rc<std::cell::Cell<f32>>,
}

impl Widget for PanCatcher {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        let taken = self.taken.clone();
        ctx.apply_self_handlers(
            teksilo_core::widget_builder::HandlerSet::new()
                .scroll_container(teksilo_core::PanAxes::Y)
                .on_scroll(move |event, _ctx| {
                    if let WidgetEvent::Scroll { delta, .. } = event {
                        let y = match delta {
                            ScrollDelta::Lines { y, .. } | ScrollDelta::Pixels { y, .. } => *y,
                        };
                        taken.set(taken.get() + y);
                    }
                    teksilo_core::event::EventResponse::Handled
                }),
        );
        vec![self.child]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for placement in children.iter_mut() {
            placement.origin = bounds.origin();
            placement.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

/// A minimal focusable leaf, so a test can move focus off the terminal. There is
/// no `WidgetTree::clear_focus`, and nothing else in these fixtures takes focus.
#[derive(Debug)]
struct FocusableLeaf;

impl Widget for FocusableLeaf {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        ctx.apply_self_handlers(teksilo_core::widget_builder::HandlerSet::new().focusable(true));
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(1.0, 1.0).into()
    }
}

struct Fixture {
    tree: WidgetTree,
    shared: Rc<RefCell<MemoryShared>>,
    id: WidgetId,
    controller: teksilo_terminal::TerminalController,
}

fn mount(configure: impl FnOnce(Terminal) -> Terminal) -> Fixture {
    mount_with(configure, |_| {})
}

/// `prepare` runs after layout but **before** the post-mount spawn, which is
/// where the widget first copies the engine's snapshot into its own cache. A
/// snapshot installed later would sit in `MemoryShared` unread until something
/// else refreshed the cache.
fn mount_with(
    configure: impl FnOnce(Terminal) -> Terminal,
    prepare: impl FnOnce(&Rc<RefCell<MemoryShared>>),
) -> Fixture {
    let mut tree = WidgetTree::new().with_theme(theme());
    let factory = MemoryEngineFactory::new();
    let shared = factory.shared();
    let terminal = configure(Terminal::with_engine_factory(factory));
    let controller = terminal.controller();
    let id = tree.add(terminal);
    tree.layout(SizeProposal::exact(480.0, 320.0));
    prepare(&shared);
    tree.run_mount_actions(&mut NoopWindowOps);
    tree.focus(id);
    Fixture {
        tree,
        shared,
        id,
        controller,
    }
}

/// A snapshot with `span` highlighted — what `TextHitSource::selection` reads.
fn snapshot_with_selection(
    span: teksilo_terminal::SelectionSpan,
) -> teksilo_terminal::GridSnapshot {
    teksilo_terminal::GridSnapshot {
        columns: 40,
        screen_lines: 20,
        cells: vec![teksilo_terminal::Cell::default(); 40 * 20],
        cursor: teksilo_terminal::CursorInfo {
            line: 0,
            column: 0,
            shape: teksilo_terminal::TermCursorShape::Block,
            visible: true,
        },
        selection: Some(span),
        display_offset: 0,
        history_len: 0,
    }
}

/// A terminal with `history_len` lines of scrollback available to move through.
fn mount_with_history(lines: usize) -> Fixture {
    mount_configured_with_history(|t| t, lines)
}

fn mount_configured_with_history(
    configure: impl FnOnce(Terminal) -> Terminal,
    lines: usize,
) -> Fixture {
    let f = mount(configure);
    f.shared.borrow_mut().history_len = lines;
    f
}

/// Turn on the child's mouse tracking (SGR + click + drag + any-motion).
fn track_mouse(shared: &Rc<RefCell<MemoryShared>>) {
    let mut s = shared.borrow_mut();
    s.mode.mouse_report_click = true;
    s.mode.mouse_drag = true;
    s.mode.mouse_motion = true;
    s.mode.sgr_mouse = true;
}

fn writes(shared: &Rc<RefCell<MemoryShared>>) -> String {
    String::from_utf8_lossy(&shared.borrow().writes).into_owned()
}

// ---------------------------------------------------------------------------
// The mouse, which must not have moved
// ---------------------------------------------------------------------------

/// The press still anchors a selection at the cell under the cursor, and the
/// drag still extends it — the behaviour the `Handled` → `Ignored` change had to
/// preserve.
#[test]
fn a_mouse_drag_still_selects_from_the_cell_it_pressed() {
    let f = mount(|t| t);
    let mut tree = f.tree;
    let (cw, ch) = cell_size();
    let from = Point::new(cw * 3.5 + 6.0, ch * 2.5 + 6.0);
    let to = Point::new(cw * 7.5 + 6.0, ch * 2.5 + 6.0);

    tree.dispatch_event(WidgetEvent::pointer_down(
        from,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(to));
    tree.dispatch_event(WidgetEvent::pointer_up(
        to,
        PointerButton::Primary,
        Modifiers::NONE,
    ));

    let s = f.shared.borrow();
    assert_eq!(
        s.selections,
        vec![(2, 3, SelectionKind::Simple)],
        "the press anchors a Simple selection at the cell under the cursor"
    );
    assert_eq!(
        s.selection_updates.first().map(|u| (u.0, u.1)),
        Some((2, 7)),
        "the drag extends it to the cell the pointer reached"
    );
}

/// A press that leaves the widget's bounds keeps extending the selection.
///
/// This is what "capture during a selection drag" buys, and it arrives with the
/// arena: declining the press lets the gesture arena take its implicit Down..Up
/// capture, which routes the moves back here. Before the change the press was
/// `Handled`, no arena ran, and nothing captured anything.
#[test]
fn a_selection_drag_keeps_its_moves_after_leaving_the_widget() {
    let mut tree = WidgetTree::new().with_theme(theme());
    let factory = MemoryEngineFactory::new();
    let shared = factory.shared();
    let terminal = tree.add(Terminal::with_engine_factory(factory));
    // Half the window, so there is somewhere outside it to drag to.
    let root = tree.add(Offset::at(terminal, 0.0, 0.0));
    let _ = root;
    tree.layout(SizeProposal::exact(480.0, 320.0));
    tree.run_mount_actions(&mut NoopWindowOps);

    let (cw, ch) = cell_size();
    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(cw * 2.5 + 6.0, ch * 1.5 + 6.0),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    // Well outside the 480 × 320 window.
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(2_000.0, 2_000.0)));

    assert!(
        !shared.borrow().selection_updates.is_empty(),
        "a move past the widget's edge must still reach the selection"
    );
}

/// The press resolves against the terminal's **own** origin, not the window's.
///
/// A pre-existing defect: `cell_at_position` subtracted the window-space content
/// origin from a widget-local point, so for a terminal anywhere but (0, 0) every
/// press landed at a negative coordinate and was dropped — no selection and no
/// mouse report, by mouse as much as by finger.
#[test]
fn a_press_on_an_offset_terminal_lands_on_the_cell_it_is_over() {
    let mut tree = WidgetTree::new().with_theme(theme());
    let factory = MemoryEngineFactory::new();
    let shared = factory.shared();
    let terminal = tree.add(Terminal::with_engine_factory(factory));
    tree.add(Offset::at(terminal, 120.0, 60.0));
    tree.layout(SizeProposal::exact(480.0, 320.0));
    tree.run_mount_actions(&mut NoopWindowOps);

    let (cw, ch) = cell_size();
    let bounds = tree.bounds(terminal);
    assert_eq!((bounds.x, bounds.y), (120.0, 60.0), "fixture sanity");

    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(bounds.x + cw * 4.5 + 6.0, bounds.y + ch * 3.5 + 6.0),
        PointerButton::Primary,
        Modifiers::NONE,
    ));

    assert_eq!(
        shared.borrow().selections,
        vec![(3, 4, SelectionKind::Simple)],
        "an offset terminal resolves the same cell an origin-aligned one would"
    );
}

/// Double- and triple-click select a word and a line. Documented since the
/// widget was written; dead until the press stopped answering `Handled`.
#[test]
fn a_double_and_a_triple_click_select_a_word_and_a_line() {
    let f = mount(|t| t);
    let mut tree = f.tree;
    let at = Point::new(40.0, 40.0);

    tree.pointer_down_button(at, PointerButton::Primary);
    tree.pointer_up_button(at, PointerButton::Primary);
    tree.pointer_down_button(at, PointerButton::Primary);
    tree.pointer_up_button(at, PointerButton::Primary);
    assert!(
        f.shared
            .borrow()
            .selections
            .iter()
            .any(|(_, _, kind)| *kind == SelectionKind::Word),
        "the second click of a pair selects a word"
    );

    tree.pointer_down_button(at, PointerButton::Primary);
    tree.pointer_up_button(at, PointerButton::Primary);
    assert!(
        f.shared
            .borrow()
            .selections
            .iter()
            .any(|(_, _, kind)| *kind == SelectionKind::Line),
        "the third selects a line"
    );
}

/// Press, drag and release report exactly the bytes they always did.
#[test]
fn mouse_reporting_bytes_are_unchanged() {
    let f = mount(|t| t);
    let mut tree = f.tree;
    track_mouse(&f.shared);
    let (cw, ch) = cell_size();
    let at = |col: f32, row: f32| Point::new(cw * col + 6.0, ch * row + 6.0);

    tree.dispatch_event(WidgetEvent::pointer_down(
        at(4.5, 2.5),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(at(5.5, 2.5)));
    tree.dispatch_event(WidgetEvent::pointer_up(
        at(5.5, 2.5),
        PointerButton::Primary,
        Modifiers::NONE,
    ));

    assert_eq!(
        writes(&f.shared),
        "\x1b[<0;5;3M\x1b[<32;6;3M\x1b[<0;6;3m",
        "press (button 0), drag (button 0 + motion 32), release (final `m`)"
    );
    assert!(
        f.shared.borrow().selections.is_empty(),
        "a reported press starts no local selection"
    );
}

/// `Shift` still forces local selection while the child is tracking the mouse.
#[test]
fn shift_still_overrides_mouse_reporting() {
    let f = mount(|t| t);
    let mut tree = f.tree;
    track_mouse(&f.shared);
    let (cw, ch) = cell_size();

    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(cw * 4.5 + 6.0, ch * 2.5 + 6.0),
        PointerButton::Primary,
        Modifiers::SHIFT,
    ));

    assert!(
        writes(&f.shared).is_empty(),
        "a Shift-press reports nothing"
    );
    assert_eq!(
        f.shared.borrow().selections,
        vec![(2, 4, SelectionKind::Simple)],
        "…and selects locally instead"
    );
}

/// A double-click while the child tracks the mouse selects nothing locally: the
/// contact belongs to the program, which is being told about both clicks.
#[test]
fn a_double_click_under_mouse_reporting_selects_nothing_locally() {
    let f = mount(|t| t);
    let mut tree = f.tree;
    track_mouse(&f.shared);
    let at = Point::new(40.0, 40.0);

    for _ in 0..2 {
        tree.pointer_down_button(at, PointerButton::Primary);
        tree.pointer_up_button(at, PointerButton::Primary);
    }
    assert!(
        f.shared.borrow().selections.is_empty(),
        "no local word selection under a full-screen program"
    );
    assert!(
        writes(&f.shared).contains("\x1b[<0;"),
        "the clicks were reported instead"
    );
}

// ---------------------------------------------------------------------------
// The wheel
// ---------------------------------------------------------------------------

fn wheel_at(tree: &mut WidgetTree, y: f32, modifiers: Modifiers, position: Option<Point>) {
    tree.dispatch_scroll(ScrollSample {
        delta: ScrollDelta::Lines { x: 0.0, y },
        position,
        phase: ScrollPhase::Discrete,
        source: ScrollSource::Wheel,
        pointer: PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO),
        modifiers,
    });
}

fn wheel(tree: &mut WidgetTree, y: f32, modifiers: Modifiers) {
    wheel_at(tree, y, modifiers, Some(Point::new(200.0, 160.0)));
}

/// Turning the wheel **up** shows older output.
///
/// `ScrollDelta` is positive toward the end of the content — the platform layer
/// negates winit's natural sign to get there — and `Scroll::Delta` is positive
/// toward *older* output, so the two disagree by a sign. The terminal used to
/// pass one straight into the other, which scrolled the scrollback backwards.
#[test]
fn the_wheel_scrolls_the_scrollback_the_way_it_is_turned() {
    let f = mount_with_history(100);
    let mut tree = f.tree;

    wheel(&mut tree, -3.0, Modifiers::NONE);
    assert_eq!(
        f.shared.borrow().scrolls,
        vec![teksilo_terminal::Scroll::Delta(3)],
        "a wheel-up notch moves three lines toward older output"
    );
    assert_eq!(f.shared.borrow().display_offset, 3);

    wheel(&mut tree, 3.0, Modifiers::NONE);
    assert_eq!(
        f.shared.borrow().display_offset,
        0,
        "a wheel-down notch comes back to the live prompt"
    );
}

/// …and reports the direction it was turned, when the child is tracking.
///
/// The coordinates are the cell the notch is over. They used to be `1;1` in this
/// assertion whatever the pointer was on, which was the report being wrong
/// rather than the wheel: see
/// `a_wheel_report_names_the_cell_under_the_pointer`.
#[test]
fn the_wheel_reports_the_direction_it_was_turned() {
    let f = mount(|t| t);
    let mut tree = f.tree;
    track_mouse(&f.shared);
    let over = Some(cell_position(4.5, 2.5));

    wheel_at(&mut tree, -1.0, Modifiers::NONE, over);
    assert_eq!(writes(&f.shared), "\x1b[<64;5;3M", "wheel up is button 64");
    f.shared.borrow_mut().writes.clear();

    wheel_at(&mut tree, 1.0, Modifiers::NONE, over);
    assert_eq!(
        writes(&f.shared),
        "\x1b[<65;5;3M",
        "wheel down is button 65"
    );
}

/// A wheel report names the cell **under the pointer**, the same as a press.
///
/// It named cell `(0, 0)` whatever the pointer was on, so a full-screen program
/// splitting its window — `tmux`, `htop`, an editor with two panes — read every
/// notch as having happened in its top-left corner and scrolled the wrong pane.
///
/// Two cells, because one assertion cannot tell "derived from the pointer" from
/// "happens to coincide with the pointer": the report has to *move* when the
/// pointer does.
#[test]
fn a_wheel_report_names_the_cell_under_the_pointer() {
    let f = mount(|t| t);
    let mut tree = f.tree;
    track_mouse(&f.shared);

    wheel_at(
        &mut tree,
        -1.0,
        Modifiers::NONE,
        Some(cell_position(4.5, 2.5)),
    );
    assert_eq!(
        writes(&f.shared),
        "\x1b[<64;5;3M",
        "column 4, row 2, in SGR's 1-based coordinates"
    );
    f.shared.borrow_mut().writes.clear();

    wheel_at(
        &mut tree,
        -1.0,
        Modifiers::NONE,
        Some(cell_position(11.5, 7.5)),
    );
    assert_eq!(
        writes(&f.shared),
        "\x1b[<64;12;8M",
        "a different cell reports a different pair"
    );
}

/// A real mouse wheel carries **no** position — it is routed by hover, and
/// `WidgetEvent::Scroll::position` is `None` for it — so the cell comes from
/// where the cursor last was. Without that, the shipped wheel would keep
/// reporting the origin however the positioned form behaved.
#[test]
fn a_positionless_wheel_report_names_the_cell_the_cursor_is_over() {
    let f = mount(|t| t);
    let mut tree = f.tree;
    track_mouse(&f.shared);

    tree.dispatch_event(WidgetEvent::pointer_move(cell_position(6.5, 4.5)));
    // The move is itself reported under any-motion tracking; the wheel's bytes
    // are what this test is about.
    f.shared.borrow_mut().writes.clear();

    wheel_at(&mut tree, -1.0, Modifiers::NONE, None);
    assert_eq!(
        writes(&f.shared),
        "\x1b[<64;7;5M",
        "the cell the cursor was left on"
    );
}

/// …and on a terminal that is not at the window's corner, through the same
/// conversion the press was fixed to use.
///
/// `WidgetTree::localize_event` does not rewrite a `Scroll`, so its position is
/// still in **window** space where every other pointer position the handler sees
/// is widget-local. Feeding it in raw would report a cell offset by the
/// terminal's own position — the neighbouring form of the defect the press had.
#[test]
fn a_wheel_report_on_an_offset_terminal_names_the_cell_it_is_over() {
    let mut tree = WidgetTree::new().with_theme(theme());
    let factory = MemoryEngineFactory::new();
    let shared = factory.shared();
    let terminal = tree.add(Terminal::with_engine_factory(factory));
    tree.add(Offset::at(terminal, 120.0, 60.0));
    tree.layout(SizeProposal::exact(480.0, 320.0));
    tree.run_mount_actions(&mut NoopWindowOps);
    track_mouse(&shared);

    let bounds = tree.bounds(terminal);
    assert_eq!((bounds.x, bounds.y), (120.0, 60.0), "fixture sanity");
    let over = cell_position(4.5, 2.5);

    wheel_at(
        &mut tree,
        -1.0,
        Modifiers::NONE,
        Some(Point::new(bounds.x + over.x, bounds.y + over.y)),
    );
    assert_eq!(
        writes(&shared),
        "\x1b[<64;5;3M",
        "an offset terminal reports the same cell an origin-aligned one does"
    );
}

/// A trackpad's sub-line sample is banked, not dropped.
#[test]
fn a_sub_line_scroll_is_banked_until_it_makes_a_line() {
    let f = mount_with_history(100);
    let mut tree = f.tree;
    let (_, ch) = cell_size();

    let pixels = |tree: &mut WidgetTree, y: f32| {
        tree.dispatch_scroll(ScrollSample {
            delta: ScrollDelta::Pixels { x: 0.0, y },
            position: Some(Point::new(200.0, 160.0)),
            phase: ScrollPhase::Changed,
            source: ScrollSource::Trackpad,
            pointer: PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO),
            modifiers: Modifiers::NONE,
        });
    };

    // A third of a line, three times: nothing, nothing, one line.
    pixels(&mut tree, -ch / 3.0);
    assert_eq!(f.shared.borrow().display_offset, 0);
    pixels(&mut tree, -ch / 3.0);
    assert_eq!(f.shared.borrow().display_offset, 0);
    pixels(&mut tree, -ch / 3.0);
    assert_eq!(
        f.shared.borrow().display_offset,
        1,
        "three thirds of a line are one line"
    );
}

// ---------------------------------------------------------------------------
// The finger
// ---------------------------------------------------------------------------

/// One finger dragged down pans the scrollback toward older output.
#[test]
fn a_one_finger_pan_scrolls_the_scrollback() {
    let f = mount_with_history(1_000);
    let mut tree = f.tree;
    let (_, ch) = cell_size();

    tree.touch_drag(Point::new(200.0, 80.0), Point::new(200.0, 240.0), 8);

    let offset = f.shared.borrow().display_offset;
    let expected = (160.0 / ch).floor() as usize;
    assert!(
        offset >= expected - 2 && offset <= expected + 1,
        "160 px of finger travel is about {expected} lines of a {ch} px cell, got {offset}"
    );
}

/// A pan the terminal cannot use chains to the scrollable that encloses it.
///
/// The wheel keeps absorbing unconditionally — that is the behaviour every
/// terminal has and the one this change was not allowed to touch — but a finger
/// takes the documented claimant-chain route, and a terminal sitting at the live
/// prompt with nothing above it in the ring has genuinely nothing to give.
#[test]
fn a_pan_with_no_scrollback_left_chains_outward() {
    let mut tree = WidgetTree::new().with_theme(theme());
    let factory = MemoryEngineFactory::new();
    let shared = factory.shared();
    let terminal = tree.add(Terminal::with_engine_factory(factory));
    let outer_lines = Rc::new(std::cell::Cell::new(0.0f32));
    tree.add(PanCatcher {
        child: terminal,
        taken: outer_lines.clone(),
    });
    tree.layout(SizeProposal::exact(480.0, 320.0));
    tree.run_mount_actions(&mut NoopWindowOps);

    // Nothing in the ring: `history_len` stays 0, `display_offset` stays 0.
    tree.touch_drag(Point::new(200.0, 240.0), Point::new(200.0, 80.0), 8);

    assert_eq!(
        shared.borrow().display_offset,
        0,
        "the terminal had nothing to scroll"
    );
    assert!(
        outer_lines.get() != 0.0,
        "…so the enclosing scrollable got the pan instead"
    );
}

/// A flick keeps the scrollback moving after the finger has left the glass.
#[test]
fn a_flick_coasts_the_scrollback() {
    let f = mount_with_history(1_000);
    let mut tree = f.tree;

    tree.fling(
        Point::new(200.0, 60.0),
        Point::new(200.0, 260.0),
        Duration::from_millis(120),
    );
    let at_release = f.shared.borrow().display_offset;
    assert!(at_release > 0, "the pan itself moved the ring");

    for _ in 0..12 {
        let now = tree.simulated_now() + Duration::from_millis(16);
        tree.advance_time(Duration::from_millis(16));
        tree.tick_flings(now);
    }
    assert!(
        f.shared.borrow().display_offset > at_release,
        "the coast carried it further: {} then {}",
        at_release,
        f.shared.borrow().display_offset
    );
}

/// Two fingers pan at one finger's rate, not two.
///
/// Pan sessions are per contact, so a two-finger drag delivers two synthesised
/// scrolls per frame. One of them is honoured and the other absorbed.
#[test]
fn a_second_contact_does_not_double_the_pan() {
    let f = mount_with_history(1_000);
    let mut tree = f.tree;
    let one = tree.new_contact();
    let two = tree.new_contact();

    tree.touch_down(one, Point::new(180.0, 80.0));
    tree.touch_down(two, Point::new(240.0, 80.0));
    for step in 1..=8 {
        let y = 80.0 + step as f32 * 20.0;
        tree.touch_move(one, Point::new(180.0, y));
        tree.touch_move(two, Point::new(240.0, y));
    }
    let two_fingers = f.shared.borrow().display_offset;
    tree.touch_up(one, Point::new(180.0, 240.0));
    tree.touch_up(two, Point::new(240.0, 240.0));

    let g = mount_with_history(1_000);
    let mut tree2 = g.tree;
    tree2.touch_drag(Point::new(180.0, 80.0), Point::new(180.0, 240.0), 8);
    let one_finger = g.shared.borrow().display_offset;

    assert!(
        two_fingers <= one_finger + 1,
        "two fingers travelling together moved {two_fingers} lines where one moved {one_finger}"
    );
    assert!(two_fingers > 0, "…and they did move it");
}

/// A finger is not reported to the child by default, whatever mode it is in.
#[test]
fn a_finger_is_not_reported_by_default() {
    let f = mount(|t| t);
    let mut tree = f.tree;
    track_mouse(&f.shared);
    let contact = tree.new_contact();

    tree.touch_down(contact, Point::new(60.0, 60.0));
    tree.touch_up(contact, Point::new(60.0, 60.0));

    assert!(
        writes(&f.shared).is_empty(),
        "TouchReporting::Off means no finger reaches the child"
    );
}

/// `AsButton1` reports a single contact as mouse button 1 — and never reports
/// two, so a two-finger pan is still the view's and still reaches the
/// scrollback. That is the way back once one finger belongs to the child.
#[test]
fn as_button1_reports_one_contact_and_never_two() {
    let f = mount_configured_with_history(
        |t| t.touch_mouse_reporting(TouchReporting::AsButton1),
        1_000,
    );
    let mut tree = f.tree;
    track_mouse(&f.shared);
    let (cw, ch) = cell_size();
    let one = tree.new_contact();

    tree.touch_down(one, Point::new(cw * 4.5 + 6.0, ch * 2.5 + 6.0));
    assert_eq!(
        writes(&f.shared),
        "\x1b[<0;5;3M",
        "one finger presses as button 1 at the cell it is over"
    );
    tree.touch_up(one, Point::new(cw * 4.5 + 6.0, ch * 2.5 + 6.0));
    f.shared.borrow_mut().writes.clear();

    // Two at once. The first is alone when it lands, so its press is reported;
    // from the moment the second is down neither contact is, so the drag that
    // follows produces no report at all and scrolls the scrollback instead.
    let a = tree.new_contact();
    let b = tree.new_contact();
    tree.touch_down(a, Point::new(120.0, 60.0));
    let after_first = writes(&f.shared);
    tree.touch_down(b, Point::new(200.0, 60.0));
    for step in 1..=8 {
        let y = 60.0 + step as f32 * 22.0;
        tree.touch_move(a, Point::new(120.0, y));
        tree.touch_move(b, Point::new(200.0, y));
    }

    assert_eq!(
        writes(&f.shared),
        after_first,
        "not one byte more once the second contact is down"
    );
    assert!(
        f.shared.borrow().display_offset > 0,
        "the two-finger pan reached the scrollback"
    );
}

// ---------------------------------------------------------------------------
// Assistive technology
// ---------------------------------------------------------------------------

/// The `ScrollUp` / `ScrollDown` actions the `Role::Terminal` node advertises
/// move the viewport. They were advertised and unimplemented.
#[test]
fn the_advertised_scroll_actions_move_the_viewport() {
    let f = mount_with_history(1_000);
    let mut tree = f.tree;
    let node = teksilo_core::accessibility::widget_id_to_node_id(f.id);

    assert!(tree.dispatch_access_action(
        node,
        accesskit::Action::ScrollUp,
        None,
        &mut NoopWindowOps
    ));
    let up = f.shared.borrow().display_offset;
    assert!(up > 0, "ScrollUp moved into the scrollback");

    assert!(tree.dispatch_access_action(
        node,
        accesskit::Action::ScrollDown,
        None,
        &mut NoopWindowOps
    ));
    assert_eq!(
        f.shared.borrow().display_offset,
        0,
        "ScrollDown came back to the prompt"
    );
}

// ---------------------------------------------------------------------------
// The long-press menu
// ---------------------------------------------------------------------------

/// The content of the topmost overlay, laid out so its rows have bounds.
fn open_menu_content(tree: &mut WidgetTree) -> WidgetId {
    let content = *tree
        .overlay_manager()
        .active_content_ids()
        .last()
        .expect("a context menu should be up");
    tree.layout(SizeProposal::exact(480.0, 320.0));
    content
}

fn row_names(tree: &WidgetTree, content: WidgetId) -> Vec<String> {
    tree.children(content)
        .into_iter()
        .filter_map(|row| tree.accessibility_node(row).name().map(str::to_string))
        .collect()
}

/// A hold opens the menu. The route is the tree's — the terminal installs a
/// `context_menu` factory and no `on_long_press` handler, which is what
/// `LongPressRole::Auto` resolves to a context menu for — so this is touch-only
/// by construction and a mouse hold does nothing.
#[test]
fn a_hold_opens_the_context_menu() {
    let f = mount_with_history(0);
    let mut tree = f.tree;
    let contact = tree.new_contact();

    tree.touch_down(contact, Point::new(120.0, 80.0));
    assert!(
        tree.overlay_manager().active_content_ids().is_empty(),
        "not before the hold ripens"
    );
    tree.advance_time(Duration::from_millis(1_200));

    let content = open_menu_content(&mut tree);
    assert_eq!(
        row_names(&tree, content),
        vec![
            "Paste".to_string(),
            "Select all".to_string(),
            "Clear".to_string()
        ],
        "no selection, so no Copy; a terminal never offers Cut"
    );
}

/// The rows come from the touch contract's `clipboard_actions`, so a selection
/// adds Copy and a read-only terminal drops Paste.
#[test]
fn the_menu_offers_only_what_the_terminal_will_honour() {
    let f = mount_with(
        |t| t,
        |shared| {
            let mut s = shared.borrow_mut();
            s.selection_text = Some("selected".into());
            s.snapshot = Some(snapshot_with_selection(teksilo_terminal::SelectionSpan {
                start: (1, 2),
                end: (1, 6),
                block: false,
            }));
        },
    );
    let mut tree = f.tree;
    let contact = tree.new_contact();
    tree.touch_down(contact, Point::new(120.0, 80.0));
    tree.advance_time(Duration::from_millis(1_200));
    let content = open_menu_content(&mut tree);
    assert_eq!(
        row_names(&tree, content),
        vec![
            "Copy".to_string(),
            "Paste".to_string(),
            "Select all".to_string(),
            "Clear".to_string()
        ],
        "a selection adds Copy"
    );
}

#[test]
fn a_read_only_terminal_offers_no_paste() {
    let f = mount(|t| t.read_only(true));
    let mut tree = f.tree;
    let contact = tree.new_contact();
    tree.touch_down(contact, Point::new(120.0, 80.0));
    tree.advance_time(Duration::from_millis(1_200));
    let content = open_menu_content(&mut tree);
    assert_eq!(
        row_names(&tree, content),
        vec!["Select all".to_string(), "Clear".to_string()],
        "nothing to paste into"
    );
}

/// Choosing a row runs the command and takes the menu down.
#[test]
fn a_menu_row_runs_its_command_and_closes_the_menu() {
    let f = mount_with_history(0);
    let mut tree = f.tree;
    let contact = tree.new_contact();
    tree.touch_down(contact, Point::new(120.0, 80.0));
    tree.advance_time(Duration::from_millis(1_200));
    let content = open_menu_content(&mut tree);
    let rows = tree.children(content);

    // Paste / Select all / Clear — pick "Select all".
    tree.click(rows[1]);
    assert_eq!(
        f.shared.borrow().select_all_calls,
        1,
        "the row ran its command"
    );
    assert!(
        tree.overlay_manager().active_content_ids().is_empty(),
        "…and the menu closed behind it"
    );
}

/// The same two effects from an assistive client's `Click` on the row, which is
/// the only route a screen-reader user has into this menu.
#[test]
fn an_assistive_click_on_a_row_runs_it_too() {
    let f = mount_with_history(0);
    let mut tree = f.tree;
    let contact = tree.new_contact();
    tree.touch_down(contact, Point::new(120.0, 80.0));
    tree.advance_time(Duration::from_millis(1_200));
    let content = open_menu_content(&mut tree);
    let rows = tree.children(content);
    let node = teksilo_core::accessibility::widget_id_to_node_id(rows[2]);

    assert!(tree.dispatch_access_action(node, accesskit::Action::Click, None, &mut NoopWindowOps));
    assert_eq!(f.shared.borrow().clear_screen_calls, 1);
    assert!(tree.overlay_manager().active_content_ids().is_empty());
}

/// Arrow keys move the highlight and Enter runs the row it is on, with the menu
/// itself holding focus and publishing the row as its `active_descendant`.
#[test]
fn the_menu_is_navigable_from_the_keyboard() {
    let f = mount_with_history(0);
    let mut tree = f.tree;
    let contact = tree.new_contact();
    tree.touch_down(contact, Point::new(120.0, 80.0));
    tree.advance_time(Duration::from_millis(1_200));
    let content = open_menu_content(&mut tree);
    let rows = tree.children(content);

    // The AT route into a menu that keeps focus on itself is the
    // `active_descendant` it publishes, so it is asserted off the real
    // `TreeUpdate` rather than off the summary `AccessibilityInfo` (which
    // carries no such field).
    let active_descendant = |tree: &WidgetTree| -> Option<accesskit::NodeId> {
        let update = tree.accessibility_tree_snapshot();
        let content_node = teksilo_core::accessibility::widget_id_to_node_id(content);
        update
            .nodes
            .iter()
            .find(|(id, _)| *id == content_node)
            .and_then(|(_, node)| node.active_descendant())
    };
    assert_eq!(
        active_descendant(&tree),
        Some(teksilo_core::accessibility::widget_id_to_node_id(rows[0])),
        "the first row is active when the menu opens"
    );

    tree.press_key(teksilo_core::event::Key::ArrowDown, Modifiers::NONE);
    tree.press_key(teksilo_core::event::Key::ArrowDown, Modifiers::NONE);
    assert_eq!(
        active_descendant(&tree),
        Some(teksilo_core::accessibility::widget_id_to_node_id(rows[2])),
        "two downs moved the published descendant with the highlight"
    );
    tree.press_key(teksilo_core::event::Key::Enter, Modifiers::NONE);
    assert_eq!(
        f.shared.borrow().clear_screen_calls,
        1,
        "two downs from Paste is Clear"
    );
    assert!(tree.overlay_manager().active_content_ids().is_empty());
}

/// The menu's rows follow the density ladder, the same as this crate's selection
/// handles do.
///
/// `Compact` is the load-bearing half: the density layer must be inert until an
/// app opts in, so a Compact menu keeps exactly the row height the crate shipped
/// before there was a ladder. `Touch` is the conformance half — a menu a *finger*
/// opens must not be the one part of the crate that ignores the density it was
/// opened at.
#[test]
fn the_menu_rows_follow_the_density_ladder() {
    for (density, row_height) in [
        (teksilo_tokens::TargetDensity::Compact, 24.0_f32),
        (teksilo_tokens::TargetDensity::Touch, 44.0),
    ] {
        let f = mount_with_history(0);
        let mut tree = f.tree;
        tree.set_input_density(density);
        tree.layout(SizeProposal::exact(480.0, 320.0));

        let contact = tree.new_contact();
        tree.touch_down(contact, Point::new(120.0, 80.0));
        tree.advance_time(Duration::from_millis(1_200));
        let content = open_menu_content(&mut tree);

        let rows = tree.children(content);
        assert_eq!(rows.len(), 3, "Paste / Select all / Clear at {density:?}");
        for row in &rows {
            assert_eq!(
                tree.bounds(*row).height,
                row_height,
                "a menu row's height at {density:?}"
            );
        }
        assert!(
            tree.bounds(content).height >= row_height * rows.len() as f32,
            "the panel at {density:?} is tall enough to hold the rows it grew"
        );
    }
}

// ---------------------------------------------------------------------------
// The crate boundary
// ---------------------------------------------------------------------------

/// `teksilo-terminal` must not gain a `teksilo-widgets` dependency.
///
/// It is why the touch-text contract lives in `teksilo-core` at all, why this
/// crate's context menu is written by hand, and why the terminal is embeddable in
/// a widget set that is not Teksilo's. A `cargo add` away from being untrue, and
/// nothing else in the suite would notice.
#[test]
fn the_crate_does_not_depend_on_teksilo_widgets() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("the crate's own manifest");
    assert!(
        !manifest.contains("teksilo-widgets"),
        "teksilo-terminal must depend only on core/tokens/canvas/platform"
    );
}

// ---------------------------------------------------------------------------
// Touch selection: the handles
// ---------------------------------------------------------------------------

/// A terminal showing `span` as its selection, with a finger's double-tap
/// already spent on raising the affordances.
fn mount_with_raised_handles(span: teksilo_terminal::SelectionSpan) -> (Fixture, WidgetId) {
    let f = mount_with(
        |t| t,
        move |shared| {
            let mut s = shared.borrow_mut();
            s.selection_text = Some("selected".into());
            s.snapshot = Some(snapshot_with_selection(span));
        },
    );
    let mut tree = f.tree;
    let at = Point::new(120.0, 80.0);
    let one = tree.new_contact();
    tree.touch_down(one, at);
    tree.touch_up(one, at);
    let two = tree.new_contact();
    tree.touch_down(two, at);
    tree.touch_up(two, at);
    tree.layout(SizeProposal::exact(480.0, 320.0));

    let host = *tree
        .overlay_manager()
        .active_content_ids()
        .last()
        .expect("a finger's double-tap raises the affordance overlay");
    let layer = tree.children(host)[0];
    (
        Fixture {
            tree,
            shared: f.shared,
            id: f.id,
            controller: f.controller,
        },
        layer,
    )
}

/// The bounds of every affordance node that has any — the invisible ones are
/// never placed, so they stay at zero.
fn placed_handles(tree: &WidgetTree, layer: WidgetId) -> Vec<Rect> {
    tree.children(layer)
        .into_iter()
        .map(|id| tree.bounds(id))
        .filter(|b| b.width > 0.0 && b.height > 0.0)
        .collect()
}

/// A selection handle sits **on a cell boundary**, not between two columns.
///
/// That is the whole of what "snapping" means for a monospace grid, and it comes
/// from `TextHitSource::caret_rect` returning a rectangle centred on the grid
/// line: the controller puts the handle at `caret.x + caret.width / 2.0`.
#[test]
fn the_handles_sit_on_cell_boundaries() {
    let (f, layer) = mount_with_raised_handles(teksilo_terminal::SelectionSpan {
        start: (3, 5),
        end: (3, 11),
        block: false,
    });
    let (cw, _) = cell_size();
    let handles = placed_handles(&f.tree, layer);
    assert_eq!(
        handles.len(),
        2,
        "a selection gets two handles and — a terminal having no caret of its \
         own — no caret handle"
    );

    // The grid's own origin inside the widget, from the style rather than from a
    // number copied out of it.
    let inset = RecipeTerminalStyle.content_inset();
    let mut columns: Vec<f32> = handles
        .iter()
        .map(|b| (b.center().x - inset) / cw)
        .collect();
    columns.sort_by(f32::total_cmp);
    for (got, want) in columns.iter().zip([5.0, 12.0]) {
        assert!(
            (got - want).abs() < 0.01,
            "handle at column {got}, wanted the boundary at {want}"
        );
    }
}

/// The trailing handle of a one-line selection, and the row it marks.
fn end_handle(tree: &WidgetTree, layer: WidgetId) -> Rect {
    placed_handles(tree, layer)
        .into_iter()
        .max_by(|a, b| a.center().x.total_cmp(&b.center().x))
        .expect("two handles")
}

/// The `(line, column)` the last recorded selection head named.
fn last_head(shared: &Rc<RefCell<MemoryShared>>) -> (usize, usize) {
    let updates = shared.borrow().selection_updates.clone();
    let last = updates
        .last()
        .copied()
        .expect("the drag moved the selection");
    (last.0, last.1)
}

/// Dragging a handle moves the selection to the **nearest** boundary, so a
/// fingertip landing four-fifths of the way across a cell takes the next
/// boundary rather than truncating to the one behind it.
///
/// The destination's *y* is the handle's own, not the target line's centre: a
/// finger sliding sideways along a row keeps holding the disc, which hangs a
/// radius below the row it marks. Stating the destination in text coordinates
/// instead — which this test used to do — cannot see whether the sample was
/// translated back onto the row, because it has already been placed on it by
/// hand. See `a_handle_drag_stays_on_the_row_the_handle_marks`.
#[test]
fn a_handle_drag_snaps_the_selection_to_the_nearest_boundary() {
    let (f, layer) = mount_with_raised_handles(teksilo_terminal::SelectionSpan {
        start: (3, 5),
        end: (3, 11),
        block: false,
    });
    let mut tree = f.tree;
    let (cw, _) = cell_size();
    let inset = RecipeTerminalStyle.content_inset();
    let end = end_handle(&tree, layer);

    f.shared.borrow_mut().selection_updates.clear();
    let contact = tree.new_contact();
    tree.touch_down(contact, end.center());
    // 0.8 of a cell past column 20 — the nearest boundary is 21.
    let target = Point::new(inset + cw * 20.8, end.center().y);
    tree.touch_move(contact, target);
    tree.touch_up(contact, target);

    assert_eq!(
        last_head(&f.shared),
        (3, 20),
        "the head lands on the cell before boundary 21, on line 3"
    );
}

/// A handle drag sideways stays on the row the handle marks.
///
/// The disc hangs a radius **off** the row so the fingertip does not cover the
/// character it points at, so the point a finger actually holds is never on the
/// glyph row. `offset_at` floors the vertical axis and an `End` handle's anchor
/// sits past `caret.bottom()` — already the top edge of the next row — so an
/// untranslated sample resolves one row down however small the radius is. The
/// host owes the translation; this is the assertion that it makes it.
#[test]
fn a_handle_drag_stays_on_the_row_the_handle_marks() {
    let (f, layer) = mount_with_raised_handles(teksilo_terminal::SelectionSpan {
        start: (3, 5),
        end: (3, 11),
        block: false,
    });
    let mut tree = f.tree;
    let (cw, _) = cell_size();
    let end = end_handle(&tree, layer);

    f.shared.borrow_mut().selection_updates.clear();
    let contact = tree.new_contact();
    tree.touch_down(contact, end.center());
    // A horizontal delta only: the y a real finger keeps while sliding.
    let target = Point::new(end.center().x + cw * 8.0, end.center().y);
    tree.touch_move(contact, target);
    tree.touch_up(contact, target);

    assert_eq!(
        last_head(&f.shared).0,
        3,
        "eight cells sideways is still line 3"
    );
}

/// …and a drag **up or down** moves by the rows it travelled — one cell of
/// finger movement is one row, so the compensation is a translation and not a
/// clamp onto the row the drag started from.
///
/// A selection spanning rows 2–5, so the trailing handle has a row to move to in
/// either direction: dragging it above the anchor would empty the range instead
/// of naming a row (`set_handle_offset` clamps a crossed handle), which is the
/// selection's own rule and not something this test is about.
#[test]
fn a_handle_drag_up_or_down_moves_by_the_rows_it_travelled() {
    let (_, ch) = cell_size();
    for (dy, want_row) in [(-ch, 4_usize), (ch, 6)] {
        let (f, layer) = mount_with_raised_handles(teksilo_terminal::SelectionSpan {
            start: (2, 5),
            end: (5, 11),
            block: false,
        });
        let mut tree = f.tree;
        let end = end_handle(&tree, layer);

        f.shared.borrow_mut().selection_updates.clear();
        let contact = tree.new_contact();
        tree.touch_down(contact, end.center());
        let target = Point::new(end.center().x, end.center().y + dy);
        tree.touch_move(contact, target);
        tree.touch_up(contact, target);

        assert_eq!(
            last_head(&f.shared).0,
            want_row,
            "a drag of {dy} px from row 5 lands on row {want_row}"
        );
    }
}

/// The **leading** handle needs the compensation too, with the opposite sign: its
/// disc sits *above* the row it marks rather than below it.
///
/// Read off the recorded selection **anchor** rather than the head, because a
/// leading-handle drag moves the anchor and leaves the head where it was.
#[test]
fn a_leading_handle_drag_stays_on_the_row_it_marks() {
    let (f, layer) = mount_with_raised_handles(teksilo_terminal::SelectionSpan {
        start: (2, 5),
        end: (5, 11),
        block: false,
    });
    let mut tree = f.tree;
    let (cw, _) = cell_size();
    let start = placed_handles(&tree, layer)
        .into_iter()
        .min_by(|a, b| a.center().x.total_cmp(&b.center().x))
        .expect("two handles");

    f.shared.borrow_mut().selections.clear();
    let contact = tree.new_contact();
    tree.touch_down(contact, start.center());
    let target = Point::new(start.center().x + cw * 6.0, start.center().y);
    tree.touch_move(contact, target);
    tree.touch_up(contact, target);

    let anchors = f.shared.borrow().selections.clone();
    let last = anchors.last().copied().expect("the drag moved the anchor");
    assert_eq!(last.0, 2, "six cells sideways is still line 2");
}

/// A **mouse** double-click raises no touch chrome. It has a drag for adjusting
/// a selection, and two discs hanging off the grid would be in its way.
#[test]
fn a_mouse_double_click_raises_no_handles() {
    let f = mount_with(
        |t| t,
        |shared| {
            let mut s = shared.borrow_mut();
            s.selection_text = Some("selected".into());
            s.snapshot = Some(snapshot_with_selection(teksilo_terminal::SelectionSpan {
                start: (3, 5),
                end: (3, 11),
                block: false,
            }));
        },
    );
    let mut tree = f.tree;
    let at = Point::new(120.0, 80.0);
    for _ in 0..2 {
        tree.pointer_down_button(at, PointerButton::Primary);
        tree.pointer_up_button(at, PointerButton::Primary);
    }
    assert!(
        tree.overlay_manager().active_content_ids().is_empty(),
        "no affordance overlay for a cursor"
    );
}

/// Scrolling retires the handles. Their offsets are **viewport** cells, so after
/// the ring moves they would mark different text than the user selected.
#[test]
fn scrolling_retires_the_handles() {
    let (f, layer) = mount_with_raised_handles(teksilo_terminal::SelectionSpan {
        start: (3, 5),
        end: (3, 11),
        block: false,
    });
    let mut tree = f.tree;
    f.shared.borrow_mut().history_len = 100;
    let _ = layer;
    assert_eq!(handle_nodes(&tree), 2, "up to start with");

    wheel(&mut tree, -3.0, Modifiers::NONE);
    tree.layout(SizeProposal::exact(480.0, 320.0));
    assert_eq!(
        handle_nodes(&tree),
        0,
        "the viewport moved, so the affordances came down"
    );
}

/// How many selection handles the accessibility tree carries. A retired handle
/// is dormant, and a dormant node is not walked — which is the observable that
/// distinguishes "taken down" from "still there with stale bounds".
fn handle_nodes(tree: &WidgetTree) -> usize {
    tree.accessibility_tree_snapshot()
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == accesskit::Role::Slider)
        .count()
}

/// Focus leaving the terminal retires the handles. The affordance band is exempt
/// from outside-press dismissal — every cell a press lands on is "outside" a
/// handle — so every retirement path is the host's, and this is one of them.
#[test]
fn losing_focus_retires_the_handles() {
    let (f, _layer) = mount_with_raised_handles(teksilo_terminal::SelectionSpan {
        start: (3, 5),
        end: (3, 11),
        block: false,
    });
    let mut tree = f.tree;
    assert_eq!(handle_nodes(&tree), 2, "up to start with");

    let elsewhere = tree.add(FocusableLeaf);
    tree.layout(SizeProposal::exact(480.0, 320.0));
    tree.focus(elsewhere);
    tree.layout(SizeProposal::exact(480.0, 320.0));
    assert_eq!(handle_nodes(&tree), 0);
}

/// So does the window going inactive: touch chrome belongs to the window the
/// finger is in.
#[test]
fn a_window_going_inactive_retires_the_handles() {
    let (f, _layer) = mount_with_raised_handles(teksilo_terminal::SelectionSpan {
        start: (3, 5),
        end: (3, 11),
        block: false,
    });
    let mut tree = f.tree;
    assert_eq!(handle_nodes(&tree), 2, "up to start with");

    tree.set_window_active(false);
    tree.layout(SizeProposal::exact(480.0, 320.0));
    assert_eq!(handle_nodes(&tree), 0);
}

/// The published handle geometry follows the **grid** across a layout, with no
/// frame of lag.
///
/// This is the rule that replaces per-command affordance bookkeeping: nothing has
/// to decide whether a change should move or retire the chrome, because
/// `place_children` re-derives it from where the grid now is — before the layer's
/// own children are placed in that same pass, which is why one layout is enough.
#[test]
fn the_handles_follow_the_grid_across_a_layout() {
    let mut tree = WidgetTree::new().with_theme(theme());
    let factory = MemoryEngineFactory::new();
    let shared = factory.shared();
    let terminal = tree.add(Terminal::with_engine_factory(factory));
    let offset = Offset::at(terminal, 0.0, 0.0);
    let dx = offset.dx.clone();
    tree.add(offset);
    tree.layout(SizeProposal::exact(480.0, 320.0));
    {
        let mut s = shared.borrow_mut();
        s.selection_text = Some("selected".into());
        s.snapshot = Some(snapshot_with_selection(teksilo_terminal::SelectionSpan {
            start: (3, 5),
            end: (3, 11),
            block: false,
        }));
    }
    tree.run_mount_actions(&mut NoopWindowOps);
    tree.focus(terminal);

    let at = Point::new(120.0, 80.0);
    for _ in 0..2 {
        let c = tree.new_contact();
        tree.touch_down(c, at);
        tree.touch_up(c, at);
    }
    tree.layout(SizeProposal::exact(480.0, 320.0));
    let host = *tree
        .overlay_manager()
        .active_content_ids()
        .last()
        .expect("a finger's double-tap raises the affordance overlay");
    let layer = tree.children(host)[0];
    let before: Vec<f32> = placed_handles(&tree, layer)
        .iter()
        .map(|b| b.center().x)
        .collect();
    assert_eq!(before.len(), 2, "up to start with");

    dx.set(90.0);
    // `layout` is a no-op on a clean tree, and moving a `Cell` the fixture owns
    // dirties nothing. Re-installing the *same* theme is the cheapest honest way
    // to ask for a fresh pass — it marks every node dirty and changes no value.
    tree.set_theme(theme());
    tree.layout(SizeProposal::exact(480.0, 320.0));
    let after: Vec<f32> = placed_handles(&tree, layer)
        .iter()
        .map(|b| b.center().x)
        .collect();
    assert_eq!(after.len(), 2);
    for (a, b) in after.iter().zip(before.iter()) {
        assert!(
            (a - b - 90.0).abs() < 0.01,
            "each handle moved with the grid: {before:?} then {after:?}"
        );
    }
}

/// A keystroke returns the view to the live prompt, which moves the viewport out
/// from under the handles' cell offsets — so it retires them.
#[test]
fn a_keystroke_retires_the_handles() {
    let (f, _layer) = mount_with_raised_handles(teksilo_terminal::SelectionSpan {
        start: (3, 5),
        end: (3, 11),
        block: false,
    });
    let mut tree = f.tree;
    assert_eq!(handle_nodes(&tree), 2, "up to start with");

    tree.press_key(teksilo_core::event::Key::A, Modifiers::NONE);
    tree.layout(SizeProposal::exact(480.0, 320.0));
    assert_eq!(handle_nodes(&tree), 0);
}

/// A right-click opens the same menu a hold does — the framework's Secondary
/// press walks up to the same `context_menu` factory.
#[test]
fn a_right_click_opens_the_context_menu() {
    let f = mount_with_history(0);
    let mut tree = f.tree;
    tree.pointer_down_button(Point::new(120.0, 80.0), PointerButton::Secondary);
    let content = open_menu_content(&mut tree);
    assert_eq!(
        row_names(&tree, content),
        vec![
            "Paste".to_string(),
            "Select all".to_string(),
            "Clear".to_string()
        ]
    );
}

/// …and so does an assistive client's `ShowContextMenu`, which the dispatcher
/// services by falling through to the very same factory.
#[test]
fn the_show_context_menu_action_opens_it_too() {
    let f = mount_with_history(0);
    let mut tree = f.tree;
    let node = teksilo_core::accessibility::widget_id_to_node_id(f.id);
    assert!(tree.dispatch_access_action(
        node,
        accesskit::Action::ShowContextMenu,
        None,
        &mut NoopWindowOps
    ));
    let content = open_menu_content(&mut tree);
    assert_eq!(row_names(&tree, content).len(), 3);
}

/// A wheel notch the terminal cannot use is still **absorbed**, not chained.
///
/// The finger's pan is the one that hands the rest of the gesture outward; the
/// wheel keeps the behaviour every terminal has, and this is the assertion that
/// keeps the two from being confused for each other.
#[test]
fn a_wheel_notch_at_the_prompt_is_absorbed_rather_than_chained() {
    let mut tree = WidgetTree::new().with_theme(theme());
    let factory = MemoryEngineFactory::new();
    let shared = factory.shared();
    let terminal = tree.add(Terminal::with_engine_factory(factory));
    let outer = Rc::new(std::cell::Cell::new(0.0f32));
    tree.add(PanCatcher {
        child: terminal,
        taken: outer.clone(),
    });
    tree.layout(SizeProposal::exact(480.0, 320.0));
    tree.run_mount_actions(&mut NoopWindowOps);

    // Nothing in the ring, so the notch cannot move anything.
    wheel(&mut tree, 3.0, Modifiers::NONE);
    assert_eq!(shared.borrow().display_offset, 0, "fixture sanity");
    assert_eq!(
        outer.get(),
        0.0,
        "the wheel stops at the terminal even when it moves nothing"
    );
}

/// Under `AsButton1` a finger's whole gesture is on the wire as button 1:
/// press, motion-with-button, release.
#[test]
fn as_button1_reports_a_fingers_press_drag_and_release() {
    let f = mount(|t| t.touch_mouse_reporting(TouchReporting::AsButton1));
    let mut tree = f.tree;
    track_mouse(&f.shared);
    let (cw, ch) = cell_size();
    let inset = RecipeTerminalStyle.content_inset();
    let at = |col: f32, row: f32| Point::new(inset + cw * col, inset + ch * row);

    let contact = tree.new_contact();
    tree.touch_down(contact, at(4.5, 2.5));
    tree.touch_move(contact, at(9.5, 2.5));
    tree.touch_up(contact, at(9.5, 2.5));

    assert_eq!(
        writes(&f.shared),
        "\x1b[<0;5;3M\x1b[<32;10;3M\x1b[<0;10;3m",
        "the same three reports a left mouse button would produce"
    );
}

/// An assistive client moves a handle with `SetValue` over the document's cell
/// offsets — the only route a screen-reader user has into a selection handle,
/// which is deliberately outside the Tab ring.
#[test]
fn set_value_on_a_handle_moves_the_selection() {
    let (f, layer) = mount_with_raised_handles(teksilo_terminal::SelectionSpan {
        start: (3, 5),
        end: (3, 11),
        block: false,
    });
    let mut tree = f.tree;
    let columns = f.controller.columns_signal().get();

    // The trailing handle, by position.
    let handles = tree.children(layer);
    let end = handles
        .iter()
        .copied()
        .filter(|id| {
            let b = tree.bounds(*id);
            b.width > 0.0 && b.height > 0.0
        })
        .max_by(|a, b| {
            tree.bounds(*a)
                .center()
                .x
                .total_cmp(&tree.bounds(*b).center().x)
        })
        .expect("two handles");

    f.shared.borrow_mut().selection_updates.clear();
    let offset = 3 * columns + 21;
    assert!(tree.dispatch_access_action(
        teksilo_core::accessibility::widget_id_to_node_id(end),
        accesskit::Action::SetValue,
        Some(accesskit::ActionData::NumericValue(offset as f64)),
        &mut NoopWindowOps
    ));

    let updates = f.shared.borrow().selection_updates.clone();
    let last = updates.last().copied().expect("the handle moved");
    assert_eq!(
        (last.0, last.1),
        (3, 20),
        "offset {offset} is the boundary after cell (3, 20): {updates:?}"
    );
}

/// A **revoked** pan does not lock the surface out of the next one.
///
/// A cancel closes the pan session with no `Ended` phase, so the owner recorded
/// for the two-contact rule has to be cleared when the last contact leaves rather
/// than when its own gesture reports finishing. Otherwise the first cancelled
/// finger keeps the claim for the life of the widget and every later pan is
/// silently absorbed.
#[test]
fn a_cancelled_pan_does_not_block_the_next_one() {
    let f = mount_with_history(1_000);
    let mut tree = f.tree;

    let first = tree.new_contact();
    tree.touch_down(first, Point::new(200.0, 80.0));
    for step in 1..=4 {
        tree.touch_move(first, Point::new(200.0, 80.0 + step as f32 * 20.0));
    }
    tree.touch_cancel(first, Point::new(200.0, 160.0));
    let after_cancel = f.shared.borrow().display_offset;

    tree.touch_drag(Point::new(200.0, 80.0), Point::new(200.0, 240.0), 8);
    assert!(
        f.shared.borrow().display_offset > after_cancel,
        "the second pan moved the ring: {after_cancel} then {}",
        f.shared.borrow().display_offset
    );
}

/// A click still focuses the terminal.
///
/// Worth an assertion of its own because the press no longer answers `Handled`:
/// the gesture arena consumes it in the widget's stead now, and "click to focus"
/// is the one thing every other test in this file takes for granted by calling
/// `WidgetTree::focus` directly.
#[test]
fn a_click_still_focuses_the_terminal() {
    let mut tree = WidgetTree::new().with_theme(theme());
    let factory = MemoryEngineFactory::new();
    let shared = factory.shared();
    let id = tree.add(Terminal::with_engine_factory(factory));
    tree.layout(SizeProposal::exact(480.0, 320.0));
    tree.run_mount_actions(&mut NoopWindowOps);
    assert_ne!(tree.focused(), Some(id), "not focused to begin with");

    let at = Point::new(120.0, 80.0);
    tree.pointer_down_button(at, PointerButton::Primary);
    tree.pointer_up_button(at, PointerButton::Primary);
    assert_eq!(tree.focused(), Some(id));

    // …and the child still receives what is typed after that click.
    tree.press_key(teksilo_core::event::Key::A, Modifiers::NONE);
    assert_eq!(shared.borrow().writes, b"a");
}

/// A **cursor's** press in the grid retires the touch chrome.
///
/// The affordance band is exempt from outside-press dismissal, so a mouse
/// arriving on a hybrid machine has to be answered explicitly or the handles
/// stand over a selection the cursor is already replacing.
#[test]
fn a_mouse_press_in_the_grid_retires_the_handles() {
    let (f, _layer) = mount_with_raised_handles(teksilo_terminal::SelectionSpan {
        start: (3, 5),
        end: (3, 11),
        block: false,
    });
    let mut tree = f.tree;
    assert_eq!(handle_nodes(&tree), 2, "up to start with");

    tree.pointer_down_button(Point::new(60.0, 200.0), PointerButton::Primary);
    tree.layout(SizeProposal::exact(480.0, 320.0));
    assert_eq!(handle_nodes(&tree), 0);
}

/// …and so does a cursor's press **on a handle**, which is the one press the
/// affordance overlay's own content root exists to answer.
///
/// A handle refuses an indirect pointer and answers `Ignored`, and an `Ignored`
/// bubbles up the handle's own path — the overlay's content, not the terminal,
/// which is a different arena root. Without the pass-through wrapper above the
/// handles that click is swallowed and the chrome stands with nothing able to
/// remove it.
#[test]
fn a_mouse_press_on_a_handle_retires_the_handles() {
    let (f, layer) = mount_with_raised_handles(teksilo_terminal::SelectionSpan {
        start: (3, 5),
        end: (3, 11),
        block: false,
    });
    let mut tree = f.tree;
    let on_a_handle = placed_handles(&tree, layer)
        .first()
        .expect("two handles")
        .center();

    tree.pointer_down_button(on_a_handle, PointerButton::Primary);
    tree.layout(SizeProposal::exact(480.0, 320.0));
    assert_eq!(handle_nodes(&tree), 0);
}
