// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a finger does to the scrollable widgets, and what it must not do.
//!
//! Every scrollable in this crate now installs
//! `common::scrollable::ScrollableBehavior`, which attaches two things: the
//! shared scroll arithmetic and the `PanClaim` that puts the node on the chain
//! a synthesised pan walks. This suite exercises them through the **real**
//! widgets rather than a fixture, because the failure this package exists to
//! prevent is a surface that installs one half and not the other — which a
//! fixture cannot show.
//!
//! The five **data views** — `ListView`, `TreeView`, `GridView`, `TableView`,
//! `TreeTableView` — are the owners this file exercises. `ScrollArea` has its
//! own pan suite in `scroll_area.rs`, and `RichTextEditor` has
//! `a_finger_pans_the_rich_text_editor` in `rich_text/tests.rs`;
//! `a_claim_on_the_node_that_owns_the_press_wins` at the foot of this file
//! covers the router rule the text surfaces depend on. `CodeEditor` and
//! `LogView` have **no** finger-pan test of their own — all three text
//! surfaces install one `common::text_scroll::text_surface_behavior`, so the
//! rich-text test covers the helper but not those two build sites. Recorded in
//! `docs/kinetic-scrolling.md` §9.
//!
//! Five questions, and which of the five views each is asked of:
//!
//! 1. **a wheel notch still scrolls it, by the same amount** — all five;
//! 2. **a finger's pan scrolls it** — all five. The fling that keeps it going
//!    after the lift is asked of `ListView` only: the coast is the tree's
//!    `FlingDriver` re-dispatching along the same chain, so it is one shared
//!    mechanism rather than per-widget wiring;
//! 3. **a pan that starts on a row — a tile, in `GridView` — activates
//!    nothing** — all five. Every one of them wires its activation hook to a
//!    *gesture* (`on_tap` under `ActivateOn::SingleClick`, `on_double_tap`
//!    under the `DoubleClick` default), so a drag genuinely can reach it and
//!    the arbitration is what stops it;
//! 4. **a pan that runs out of range hands the whole event to the container
//!    outside it, never a fraction of one** — all five;
//! 5. **a pan commits nothing a release on that row would have committed** —
//!    the chevron toggle (`TreeView`, the only view that toggles from the row
//!    body) and the deferred collapse of a multi-selection (`TreeView`,
//!    `ListView`, `TableView`, `TreeTableView`). Unlike question 3 these are
//!    **not** gestures: they are raw `PointerUp` arms, so the arbitration
//!    cannot stop them and each has to ask
//!    `data_views::release_completes_the_press` for itself.
//!
//! What a pan must not commit on the **press** is a separate file:
//! `data_view_selection.rs`, because no release-time predicate can reach a
//! selection already written on `PointerDown` and the answer there is to move
//! the write to the release for a direct pointer. What a finger's *drag* does
//! instead of scrolling — the deferred marquee and the reorder behind a hold —
//! is `data_view_drag.rs`.
//!
//! Plus two questions asked once each, for the same "shared machinery" reason: the
//! realized row window at an offset a finger reached is the same window the wheel
//! reaches there (`ListView`), and a trackpad stream takes the wheel path rather
//! than the kinetic one (`ListView`).

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{EventResponse, Modifiers, ScrollDelta, WidgetEvent};
use teksilo_core::pointer::{
    BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
    PointerSample,
};
use teksilo_core::signal::Signal;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{ListModel, TreeModel};
use teksilo_i18n::lit;
use teksilo_widgets::primitives::{TextWidget, VStack};
use teksilo_widgets::table_view::{Column, ColumnWidth, TableView};
use teksilo_widgets::{GridSizing, GridView, ListView, TreeTableView, TreeView};

// ---------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------

const VIEWPORT: f32 = 400.0;
const ROW: f32 = 40.0;
const ROWS: usize = 200;

fn tree_with(root: impl teksilo_core::widget::Widget + 'static) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    // A `ManualClock`, because `touch(..)` below stamps each sample with an
    // explicit `EventTime` and the fling is started at the *sample's* time
    // (`end_pan` reads `sequence_now()`) while it is ticked at the tree's input
    // axis. On the default monotonic clock those two have different origins: the
    // samples run 0..48 ms while the axis is anchored to however much real time
    // the test has taken, so the coast's first tick can compute an elapsed of
    // zero — or of whatever the machine's load made it. That is not a
    // hypothetical: `a_finger_pans_a_list_view_and_the_fling_keeps_it_going`
    // failed 2 runs in 6 of the full parallel suite, in two different ways (the
    // offset going backwards, and the coast producing nothing), while passing
    // 27/27 in isolation and under CPU load. A manual clock has no wall-clock
    // anchor, so `input_now()` reads it directly and `advance_time` moves it:
    // sample times and tick times then share one origin by construction.
    tree.set_input_clock(std::rc::Rc::new(
        teksilo_core::pointer::clock::ManualClock::new(teksilo_core::pointer::EventTime::ZERO),
    ));
    let id = tree.add(root);
    tree.layout(SizeProposal::exact(VIEWPORT, VIEWPORT));
    (tree, id)
}

fn finger() -> PointerId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    PointerIdAllocator::global().begin(
        BackendDeviceKey::new(0x5CB0),
        NEXT.fetch_add(1, Ordering::Relaxed),
    )
}

fn touch(id: PointerId, phase: PointerPhase, at: Point, ms: u64) -> PointerSample {
    PointerSample {
        pointer: PointerInfo::touch(id, EventTime::from_millis(ms)),
        phase,
        position: at,
        button: None,
        modifiers: Modifiers::NONE,
        coalesced: Vec::new(),
    }
}

fn pan_slop() -> f32 {
    teksilo_core::gesture::default_profile(teksilo_tokens::PointerKind::Touch)
        .pan_slop
        .expect("a touch profile pans")
}

/// Press at `from` and drag the finger `up` logical pixels, crossing the pan
/// slop first so the claim is taken. Leaves the contact **down**.
fn pan_up(tree: &mut WidgetTree, from: Point, up: f32) -> (PointerId, Point) {
    let id = finger();
    tree.dispatch_pointer(touch(id, PointerPhase::Down, from, 0));
    let armed = Point::new(from.x, from.y - pan_slop() - 1.0);
    tree.dispatch_pointer(touch(id, PointerPhase::Move, armed, 16));
    let end = Point::new(from.x, armed.y - up);
    tree.dispatch_pointer(touch(id, PointerPhase::Move, end, 32));
    (id, end)
}

fn lift(tree: &mut WidgetTree, id: PointerId, at: Point, ms: u64) {
    tree.dispatch_pointer(touch(id, PointerPhase::Up, at, ms));
}

fn wheel(tree: &mut WidgetTree, at: Point, dy: f32) {
    tree.dispatch_event(WidgetEvent::scroll_at(
        ScrollDelta::Pixels { x: 0.0, y: dy },
        Modifiers::NONE,
        at,
    ));
}

fn centre() -> Point {
    Point::new(VIEWPORT / 2.0, VIEWPORT / 2.0)
}

/// Question 4, asked of any owner: a pan the surface can absorb stays with it,
/// and a pan it cannot absorb reaches the container outside it **whole**.
///
/// `park_at` is any offset past the surface's own end — the next placement
/// pass clamps it into range, so the exact number does not matter as long as
/// it is finite (an infinity reaches the row-index arithmetic before the clamp
/// does).
fn assert_the_whole_pan_chains_outward(
    view: impl teksilo_core::widget::Widget + 'static,
    offset: Signal<f32>,
    park_at: f32,
    what: &str,
) {
    let outer_seen = Rc::new(Cell::new(0_u32));
    let outer_total = Rc::new(Cell::new(0.0_f32));

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let inner = tree.add(view);
    let seen = outer_seen.clone();
    let total = outer_total.clone();
    let _outer = tree.add(
        VStack::new()
            .child(inner)
            .scroll_container(teksilo_core::pointer::touch_action::PanAxes::BOTH)
            .pan_claim(teksilo_core::pointer::touch_action::PanClaim::vertical())
            .on_scroll(move |event, _ctx| {
                if let WidgetEvent::Scroll { delta, .. } = event {
                    let dy = match delta {
                        ScrollDelta::Pixels { y, .. } => *y,
                        ScrollDelta::Lines { y, .. } => *y * 20.0,
                    };
                    if dy != 0.0 {
                        seen.set(seen.get() + 1);
                        total.set(total.get() + dy);
                    }
                }
                EventResponse::Handled
            }),
    );
    tree.layout(SizeProposal::exact(VIEWPORT, VIEWPORT));

    // A pan it CAN absorb. Without this half the test would pass just as well
    // with the surface's claim deleted, since the container would then be the
    // only claimant either way.
    let (first_id, first_at) = pan_up(&mut tree, centre(), 80.0);
    lift(&mut tree, first_id, first_at, 64);
    assert!(
        offset.get() > 0.0,
        "{what} absorbed the pan it had room for",
    );
    assert_eq!(
        outer_seen.get(),
        0,
        "so the container outside {what} was offered nothing",
    );

    offset.set(park_at);
    tree.layout(SizeProposal::exact(VIEWPORT, VIEWPORT));
    let end = offset.get();
    assert!(end > 0.0, "{what} has somewhere to be at the end of");

    let (id, at) = pan_up(&mut tree, centre(), 80.0);
    lift(&mut tree, id, at, 64);

    assert_eq!(
        offset.get(),
        end,
        "{what} was already at its end and must not have moved",
    );
    assert!(
        outer_seen.get() > 0,
        "a pan {what} could not absorb must reach the container outside it",
    );
    assert!(
        outer_total.get() > 0.0,
        "and it must arrive as the whole delta, not a residual of zero",
    );
}

/// Question 3, asked of any owner with a pointer-driven activation hook:
/// dragging off a row scrolls the view and activates nothing.
///
/// The caller wires its own `on_activate` twin into `view` and hands back the
/// counter, because each owner spells that builder differently.
fn assert_a_pan_off_a_row_activates_nothing(
    view: impl teksilo_core::widget::Widget + 'static,
    offset: Signal<f32>,
    activations: Rc<Cell<u32>>,
    what: &str,
) {
    let (mut tree, _id) = tree_with(view);
    let (id, at) = pan_up(&mut tree, centre(), 120.0);
    lift(&mut tree, id, at, 48);
    assert!(offset.get() > 0.0, "the pan scrolled {what}");
    assert_eq!(
        activations.get(),
        0,
        "dragging off a row of {what} must not activate it",
    );
}

fn items() -> ListModel<usize> {
    ListModel::from_vec((0..ROWS).collect())
}

/// The set of rows a view currently has realized.
///
/// Each row's widget adds its index on construction and removes it when the
/// arena destroys it, so this is the *live* window rather than a log of every
/// row ever built — which is what makes it comparable between two views that
/// reached the same offset by different routes.
#[derive(Clone, Default)]
struct Realized(Rc<RefCell<BTreeSet<usize>>>);

impl Realized {
    fn live(&self) -> BTreeSet<usize> {
        self.0.borrow().clone()
    }
}

#[derive(Debug)]
struct RowProbe {
    index: usize,
    live: Rc<RefCell<BTreeSet<usize>>>,
}

impl RowProbe {
    fn new(index: usize, realized: &Realized) -> Self {
        realized.0.borrow_mut().insert(index);
        Self {
            index,
            live: realized.0.clone(),
        }
    }
}

impl Drop for RowProbe {
    fn drop(&mut self) {
        self.live.borrow_mut().remove(&self.index);
    }
}

impl teksilo_core::widget::Widget for RowProbe {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        teksilo_canvas::Size::new(100.0, ROW).into()
    }
}

// ---------------------------------------------------------------------------
// ListView
// ---------------------------------------------------------------------------

fn list_view(realized: Realized) -> ListView<usize> {
    let r = realized.clone();
    ListView::new(items(), move |i, _item, _sel| {
        Box::new(RowProbe::new(i, &r))
    })
    .item_height(ROW)
}

#[test]
fn a_wheel_notch_still_scrolls_a_list_view() {
    let realized = Realized::default();
    let view = list_view(realized);
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);
    wheel(&mut tree, centre(), 120.0);
    // Smooth scrolling is on by default, so the notch aims a tween rather than
    // jumping; the target is what the wheel decided.
    assert_eq!(
        offset.animation_target(),
        Some(120.0),
        "a 120 dp notch moves the list 120 dp, exactly as before the migration",
    );
}

/// A **trackpad** two-finger scroll takes the wheel path, not the kinetic one.
///
/// `handle_scroll_event` branches on the scroll *source* and never on the
/// phase, and that is the whole reason the migration was safe: a trackpad
/// stream carries `Began` / `Changed` / `Ended` phases exactly as a synthesised
/// touch pan does, but it is not a finger dragging the content — it is a
/// pointing device reporting deltas, and it must clamp against the animation
/// target and tween like every wheel notch always has.
///
/// The distinction had no test. Rewriting the branch as
/// `!matches!(phase, ScrollPhase::Discrete)` — which reads as an equivalent
/// spelling and is the natural "simplification" — routes this stream into
/// `pan_step` instead, where nothing tweens and a `KineticScroller` starts
/// tracking velocity for a device that has no contact to lift. Under that
/// rewrite the tween below is absent and `animation_target()` answers `None`.
#[test]
fn a_trackpad_stream_takes_the_wheel_path_and_not_the_kinetic_one() {
    use teksilo_core::pointer::{PointerInfo, ScrollPhase, ScrollSample, ScrollSource};

    let realized = Realized::default();
    let view = list_view(realized);
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    for (i, phase) in [ScrollPhase::Began, ScrollPhase::Changed, ScrollPhase::Ended]
        .into_iter()
        .enumerate()
    {
        tree.dispatch_scroll(ScrollSample {
            delta: ScrollDelta::Pixels { x: 0.0, y: 30.0 },
            position: Some(centre()),
            phase,
            source: ScrollSource::Trackpad,
            pointer: PointerInfo::mouse(EventTime::from_millis(i as u64 * 16)),
            modifiers: Modifiers::NONE,
        });
    }

    assert_eq!(
        offset.animation_target(),
        Some(90.0),
        "three 30 dp trackpad samples must accumulate onto ONE wheel tween — a          pan path would have set the offset outright and aimed no tween at all",
    );
}

#[test]
fn a_finger_pans_a_list_view_and_the_fling_keeps_it_going() {
    let realized = Realized::default();
    let view = list_view(realized);
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    let (id, at) = pan_up(&mut tree, centre(), 150.0);
    let panned = offset.get();
    assert!(
        panned > 100.0,
        "a finger dragged 150 dp up should have scrolled the list down by about \
         that much, not {panned}",
    );
    // A pan never tweens — the finger is the animation.
    assert_eq!(
        offset.animation_target(),
        None,
        "a pan must not aim a tween at the offset",
    );

    lift(&mut tree, id, at, 48);
    for _ in 0..30 {
        tree.advance_time(std::time::Duration::from_millis(16));
    }
    assert!(
        offset.get() > panned,
        "the coast after the lift should have carried the list further than the \
         finger did ({} vs {panned})",
        offset.get(),
    );
}

#[test]
fn a_pan_that_starts_on_a_list_row_activates_nothing() {
    let realized = Realized::default();
    let activations = Rc::new(Cell::new(0_u32));
    let count = activations.clone();
    let r = realized.clone();
    let view = ListView::new(items(), move |i, _item, _sel| {
        Box::new(RowProbe::new(i, &r))
    })
    .item_height(ROW)
    .on_activate(move |_i, _ctx| count.set(count.get() + 1));
    let offset = view.scroll_y_signal().clone();
    assert_a_pan_off_a_row_activates_nothing(view, offset, activations, "the list");
}

#[test]
fn a_list_view_at_its_end_hands_the_whole_pan_outward() {
    let realized = Realized::default();
    let view = list_view(realized);
    let offset = view.scroll_y_signal().clone();
    assert_the_whole_pan_chains_outward(view, offset, ROWS as f32 * ROW, "the list");
}

/// The row window a virtualized list realizes is a function of its offset. A
/// finger that reached an offset must therefore see exactly the rows a wheel
/// sees there — an off-by-a-buffer here is how a fling ends on blank space.
#[test]
fn a_finger_and_a_wheel_realize_the_same_rows_at_the_same_offset() {
    // Far enough that the realization buffer is genuinely exited: the pane
    // rebuilds only when the visible range leaves the window it built, which
    // is why a short scroll is not a test of this at all.
    const TARGET: f32 = 1200.0;

    fn realized_at(reach: impl FnOnce(&mut WidgetTree, &Signal<f32>)) -> BTreeSet<usize> {
        let realized = Realized::default();
        let view = list_view(realized.clone());
        let offset = view.scroll_y_signal().clone();
        let (mut tree, _id) = tree_with(view);
        reach(&mut tree, &offset);
        // Land on exactly the same offset either way, so what is compared is
        // the realization rule and not where each input happened to stop.
        offset.set(TARGET);
        // A Rebuild-level binding raised during a pass is drained by the next
        // one — the deferral that keeps a scrollbar thumb drag from tearing
        // its own list down — so settle before reading.
        for _ in 0..3 {
            tree.layout(SizeProposal::exact(VIEWPORT, VIEWPORT));
        }
        realized.live()
    }

    let by_wheel = realized_at(|tree, _offset| wheel(tree, centre(), TARGET));
    let by_finger = realized_at(|tree, _offset| {
        let (id, at) = pan_up(tree, centre(), TARGET);
        lift(tree, id, at, 48);
    });

    // The rows the viewport actually shows at TARGET: 1200 / 40 = row 30, and
    // a 400 dp viewport holds ten of them.
    let must_show: BTreeSet<usize> = (30..40).collect();
    assert!(
        by_wheel.is_superset(&must_show),
        "the wheel left the visible rows unrealized: {by_wheel:?}",
    );
    assert_eq!(
        by_wheel, by_finger,
        "the realized row window must depend on the offset, not on what moved it",
    );
}

// ---------------------------------------------------------------------------
// TreeView
// ---------------------------------------------------------------------------

fn flat_tree() -> TreeModel<usize> {
    let model: TreeModel<usize> = TreeModel::new();
    for i in 0..ROWS {
        model.insert_root(i, i);
    }
    model
}

#[test]
fn a_finger_pans_a_tree_view_and_a_wheel_still_does() {
    // `TreeView` exposes its offset only as a setter, so the test supplies the
    // signal it will scroll. It must be animated: the view smooth-scrolls by
    // default, and a wheel notch aims a tween at it.
    let offset = Signal::new_animated(0.0);
    let view = TreeView::new(flat_tree(), |item, _entry, _sel| {
        Box::new(TextWidget::new(lit!(item.to_string())))
    })
    .item_height(ROW)
    .scroll_signal(offset.clone());
    let (mut tree, _id) = tree_with(view);

    wheel(&mut tree, centre(), 90.0);
    assert_eq!(
        offset.animation_target(),
        Some(90.0),
        "the wheel path is untouched by the migration",
    );
    offset.set(0.0);

    let (id, at) = pan_up(&mut tree, centre(), 130.0);
    assert!(
        offset.get() > 100.0,
        "a finger dragged 130 dp up should have scrolled the tree, not left it at {}",
        offset.get(),
    );
    lift(&mut tree, id, at, 48);
}

#[test]
fn a_pan_that_starts_on_a_tree_row_activates_nothing() {
    let activations = Rc::new(Cell::new(0_u32));
    let count = activations.clone();
    let offset = Signal::new_animated(0.0);
    let view = TreeView::new(flat_tree(), |item, _entry, _sel| {
        Box::new(TextWidget::new(lit!(item.to_string())))
    })
    .item_height(ROW)
    .scroll_signal(offset.clone())
    .on_activate(move |_i, _ctx| count.set(count.get() + 1));
    assert_a_pan_off_a_row_activates_nothing(view, offset, activations, "the tree");
}

#[test]
fn a_tree_view_at_its_end_hands_the_whole_pan_outward() {
    let offset = Signal::new_animated(0.0);
    let view = TreeView::new(flat_tree(), |item, _entry, _sel| {
        Box::new(TextWidget::new(lit!(item.to_string())))
    })
    .item_height(ROW)
    .scroll_signal(offset.clone());
    assert_the_whole_pan_chains_outward(view, offset, ROWS as f32 * ROW, "the tree");
}

// ---------------------------------------------------------------------------
// GridView
// ---------------------------------------------------------------------------

#[test]
fn a_finger_pans_a_grid_view_and_a_wheel_still_does() {
    let view = GridView::new(items(), |tc| {
        Box::new(TextWidget::new(lit!(tc.item.to_string())))
    })
    .sizing(GridSizing::Fixed {
        width: 100.0,
        height: ROW,
    });
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    wheel(&mut tree, centre(), 80.0);
    assert_eq!(
        offset.animation_target(),
        Some(80.0),
        "the wheel path is untouched by the migration",
    );
    offset.set(0.0);

    let (id, at) = pan_up(&mut tree, centre(), 120.0);
    assert!(
        offset.get() > 90.0,
        "a finger dragged 120 dp up should have scrolled the grid, not left it at {}",
        offset.get(),
    );
    lift(&mut tree, id, at, 48);
}

fn grid_view() -> GridView<usize> {
    GridView::new(items(), |tc| {
        Box::new(TextWidget::new(lit!(tc.item.to_string())))
    })
    .sizing(GridSizing::Fixed {
        width: 100.0,
        height: ROW,
    })
}

#[test]
fn a_pan_that_starts_on_a_grid_tile_activates_nothing() {
    let activations = Rc::new(Cell::new(0_u32));
    let count = activations.clone();
    let view = grid_view().on_tile_activate(move |_i, _ctx| count.set(count.get() + 1));
    let offset = view.scroll_y_signal().clone();
    assert_a_pan_off_a_row_activates_nothing(view, offset, activations, "the grid");
}

#[test]
fn a_grid_view_at_its_end_hands_the_whole_pan_outward() {
    let view = grid_view();
    let offset = view.scroll_y_signal().clone();
    assert_the_whole_pan_chains_outward(view, offset, ROWS as f32 * ROW, "the grid");
}

// ---------------------------------------------------------------------------
// TableView and TreeTableView — two axes, and a Shift+wheel remap
// ---------------------------------------------------------------------------

fn table() -> TableView<usize> {
    TableView::new(items())
        .add_column(
            Column::new("a", lit!("A"), |v: &usize, _cx| {
                Box::new(TextWidget::new(lit!(v.to_string())))
            })
            .width(ColumnWidth::Fixed(300.0)),
        )
        .add_column(
            Column::new("b", lit!("B"), |v: &usize, _cx| {
                Box::new(TextWidget::new(lit!(v.to_string())))
            })
            .width(ColumnWidth::Fixed(300.0)),
        )
        .row_height(ROW)
}

#[test]
fn a_finger_pans_a_table_view_and_a_wheel_still_does() {
    let view = table();
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    wheel(&mut tree, centre(), 70.0);
    assert_eq!(
        offset.animation_target(),
        Some(70.0),
        "the wheel path is untouched by the migration",
    );
    offset.set(0.0);

    let (id, at) = pan_up(&mut tree, centre(), 110.0);
    assert!(
        offset.get() > 80.0,
        "a finger dragged 110 dp up should have scrolled the table, not left it at {}",
        offset.get(),
    );
    lift(&mut tree, id, at, 48);
}

/// The Shift+wheel horizontal remap the table has always had. It is the one
/// piece the shared handler cannot express — it rewrites the delta rather than
/// consuming it — so it lives in the surface's `before` arm and hands the
/// rewritten event back to the same shared handler.
#[test]
fn shift_and_the_wheel_still_scroll_a_table_sideways() {
    let view = table();
    let x = view.scroll_x_signal().clone();
    let y = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    tree.dispatch_event(WidgetEvent::scroll_at(
        ScrollDelta::Pixels { x: 0.0, y: 60.0 },
        Modifiers::SHIFT,
        centre(),
    ));
    assert_eq!(
        x.animation_target(),
        Some(60.0),
        "a vertical notch held with Shift moves the columns",
    );
    assert_eq!(
        y.animation_target(),
        None,
        "and leaves the rows exactly where they were",
    );
}

/// A Shift+wheel notch a table cannot absorb **sideways** must not be spent
/// **downwards** instead.
///
/// The remap runs in the shared behaviour's `before` arm, which rewrites the
/// event and hands the rewrite to `handle_scroll_event` itself. When the
/// rewritten horizontal delta cannot move — no column overflow at all, as
/// here, or the columns already at their end — that call answers `Ignored`,
/// which is the correct boundary answer: the notch belongs to whatever
/// encloses the table. What must NOT happen is the shared handler then running
/// a second time on the *original*, un-remapped event and scrolling the rows.
/// That is what a `Handled`-means-short-circuit rule in `install` produced, and
/// what `before`'s `Option` answer now makes unrepresentable.
#[test]
fn a_shift_notch_a_table_cannot_take_sideways_does_not_scroll_its_rows() {
    // One column, narrower than the viewport: `max_scroll_x` is zero, so the
    // remapped horizontal delta has nowhere to go.
    let view = TableView::new(items())
        .add_column(
            Column::new("a", lit!("A"), |v: &usize, _cx| {
                Box::new(TextWidget::new(lit!(v.to_string())))
            })
            .width(ColumnWidth::Fixed(80.0)),
        )
        .row_height(ROW);
    let x = view.scroll_x_signal().clone();
    let y = view.scroll_y_signal().clone();

    let outer_seen = Rc::new(Cell::new(0_u32));
    let seen = outer_seen.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let inner = tree.add(view);
    let _outer = tree.add(VStack::new().child(inner).on_scroll(move |_event, _ctx| {
        seen.set(seen.get() + 1);
        EventResponse::Handled
    }));
    tree.layout(SizeProposal::exact(VIEWPORT, VIEWPORT));
    assert_eq!(
        y.animation_target(),
        None,
        "the table starts with no vertical tween in flight",
    );

    tree.dispatch_event(WidgetEvent::scroll_at(
        ScrollDelta::Pixels { x: 0.0, y: 60.0 },
        Modifiers::SHIFT,
        centre(),
    ));

    assert_eq!(
        x.animation_target(),
        None,
        "there was nowhere sideways to go, so the columns did not move",
    );
    assert_eq!(
        y.animation_target(),
        None,
        "and the notch must not have been spent on the rows instead",
    );
    assert_eq!(y.get(), 0.0, "nor set outright on them");
    assert_eq!(
        outer_seen.get(),
        1,
        "the whole notch reached the container outside the table",
    );
}

/// The remap is a **wheel** convention. A finger holding no keyboard cannot
/// reach it, but a keyboard held during a pan could — and turning that pan
/// sideways is not what the hand asked for.
#[test]
fn a_finger_pan_is_never_remapped_by_a_held_shift() {
    let view = table();
    let x = view.scroll_x_signal().clone();
    let y = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    let id = finger();
    let from = centre();
    let with_shift = |ms: u64, phase: PointerPhase, at: Point| PointerSample {
        pointer: PointerInfo::touch(id, EventTime::from_millis(ms)),
        phase,
        position: at,
        button: None,
        modifiers: Modifiers::SHIFT,
        coalesced: Vec::new(),
    };
    tree.dispatch_pointer(with_shift(0, PointerPhase::Down, from));
    for (i, dy) in [pan_slop() + 1.0, pan_slop() + 90.0]
        .into_iter()
        .enumerate()
    {
        tree.dispatch_pointer(with_shift(
            16 + i as u64 * 16,
            PointerPhase::Move,
            Point::new(from.x, from.y - dy),
        ));
    }
    assert!(
        y.get() > 0.0,
        "the vertical pan landed on the vertical axis",
    );
    assert_eq!(x.get(), 0.0, "and not on the horizontal one");
}

#[test]
fn a_finger_pans_a_tree_table_view_and_a_wheel_still_does() {
    let view = TreeTableView::new(flat_tree()).row_height(ROW);
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    wheel(&mut tree, centre(), 65.0);
    assert_eq!(
        offset.animation_target(),
        Some(65.0),
        "the wheel path is untouched by the migration",
    );
    offset.set(0.0);

    let (id, at) = pan_up(&mut tree, centre(), 115.0);
    assert!(
        offset.get() > 80.0,
        "a finger dragged 115 dp up should have scrolled the tree table, not \
         left it at {}",
        offset.get(),
    );
    lift(&mut tree, id, at, 48);
}

// ---------------------------------------------------------------------------
// The rule that decides which surfaces a claim can actually serve
// ---------------------------------------------------------------------------

#[test]
fn a_table_view_at_its_end_hands_the_whole_pan_outward() {
    let view = table();
    let offset = view.scroll_y_signal().clone();
    assert_the_whole_pan_chains_outward(view, offset, ROWS as f32 * ROW, "the table");
}

#[test]
fn a_pan_that_starts_on_a_table_row_activates_nothing() {
    let activations = Rc::new(Cell::new(0_u32));
    let count = activations.clone();
    let view = table().on_row_activate(move |_i, _ctx| count.set(count.get() + 1));
    let offset = view.scroll_y_signal().clone();
    assert_a_pan_off_a_row_activates_nothing(view, offset, activations, "the table");
}

#[test]
fn a_pan_that_starts_on_a_tree_table_row_activates_nothing() {
    let activations = Rc::new(Cell::new(0_u32));
    let count = activations.clone();
    let view = TreeTableView::new(flat_tree())
        .row_height(ROW)
        .on_row_activate(move |_i, _ctx| count.set(count.get() + 1));
    let offset = view.scroll_y_signal().clone();
    assert_a_pan_off_a_row_activates_nothing(view, offset, activations, "the tree table");
}

#[test]
fn a_tree_table_view_at_its_end_hands_the_whole_pan_outward() {
    let view = TreeTableView::new(flat_tree()).row_height(ROW);
    let offset = view.scroll_y_signal().clone();
    assert_the_whole_pan_chains_outward(view, offset, ROWS as f32 * ROW, "the tree table");
}

/// A node that both owns the press arena and claims the pan **wins its own
/// pan**.
///
/// This is the router's rule, not a widget's: `advance_sequence`
/// (`teksilo-core`, `widget_tree/pointer_state.rs`) breaks its candidate walk
/// at the node whose arena took the press only for a `Gesture` member, whose
/// recognizer the capture dispatch is already driving. A `Pan` member is
/// evaluated in that walk and nowhere else — its product is a synthesised
/// `Scroll`, not a `GestureEvent` fed to an arena — so breaking there too
/// would leave the claim permanently inert. It is why the three text surfaces
/// pan under a finger even though their tap recognizers put the arena on the
/// very node that carries the claim.
///
/// **Triage note.** This test previously asserted the opposite — that such a
/// claim is inert — and its own doc named the day the rule changed as the day
/// it should say so. That day is this one: with the claim inert *and the walk
/// stopped at it*, a finger inside a text surface scrolled neither the surface
/// nor the page behind it, which is a regression against the behaviour those
/// surfaces had before they were given a claim at all.
#[test]
fn a_claim_on_the_node_that_owns_the_press_wins() {
    use teksilo_core::pointer::touch_action::{PanAxes, PanClaim};
    use teksilo_widgets::primitives::FixedSize;

    let scrolls = Rc::new(Cell::new(0_u32));
    let count = scrolls.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let _node = tree.add(
        FixedSize::new()
            .width(VIEWPORT)
            .height(VIEWPORT)
            .child(TextWidget::new(lit!("x")))
            // A tap handler is what gives the node its gesture arena, and so
            // makes it the press owner.
            .on_tap(|_tap, _ctx| {})
            .scroll_container(PanAxes::BOTH)
            .pan_claim(PanClaim::vertical())
            .on_scroll(move |_event, _ctx| {
                count.set(count.get() + 1);
                EventResponse::Handled
            }),
    );
    tree.layout(SizeProposal::exact(VIEWPORT, VIEWPORT));

    let (id, at) = pan_up(&mut tree, centre(), 100.0);
    lift(&mut tree, id, at, 64);

    assert!(
        scrolls.get() > 0,
        "the claim serves the node that owns the press; it received {} scrolls",
        scrolls.get(),
    );
}

// ---------------------------------------------------------------------------
// Question 5: a pan commits nothing a release would have committed
// ---------------------------------------------------------------------------

/// A branch tree built as 200 collapsed roots, each with one child, so every
/// visible row is a branch the row body would toggle.
fn branch_tree() -> TreeModel<usize> {
    let model: TreeModel<usize> = TreeModel::new();
    for i in 0..ROWS {
        let root = model.insert_root(i, i);
        model.insert_child(root, 0, i * 1000);
    }
    model
}

#[test]
fn a_pan_that_starts_on_a_tree_branch_row_toggles_nothing() {
    let slot: Rc<RefCell<Option<teksilo_data::tree_slice::TreeSliceHandle<usize>>>> =
        Rc::new(RefCell::new(None));
    let sink = slot.clone();
    let offset = Signal::new_animated(0.0);
    let view = TreeView::new_with_context(branch_tree(), move |item, _entry, _sel, ctx| {
        if sink.borrow().is_none() {
            *sink.borrow_mut() = Some(ctx.slice_handle());
        }
        Box::new(TextWidget::new(lit!(item.to_string())))
    })
    .item_height(ROW)
    .scroll_signal(offset.clone());
    let (mut tree, _id) = tree_with(view);
    let slice = slot.borrow().clone().expect("a row was realized");
    assert_eq!(slice.visible_count(), ROWS, "precondition: all collapsed");

    let (id, at) = pan_up(&mut tree, centre(), 120.0);
    lift(&mut tree, id, at, 48);

    assert!(offset.get() > 0.0, "the pan scrolled the tree");
    assert_eq!(
        slice.visible_count(),
        ROWS,
        "the pan must not have expanded the branch row it started on",
    );
}

/// Question 5's second half: a pan must not collapse a multi-selection to the
/// row the finger happened to start on.
///
/// Two things about the setup are load-bearing, and each one hides the bug if
/// dropped:
///
/// * the pan is **short** — one third of a row past the slop. A long pan leaves
///   the realized window and rebuilds the body pane, and the fresh row handler
///   has no deferred flag left to fire.
/// * the view wires an **activation** hook. That is what gives the row a tap
///   recognizer, and so the pointer capture whose presence makes
///   `process_pending_rebuilds` defer the pane rebuild until the gesture is
///   over — which is what keeps the pressed row's own handler alive to receive
///   the release. Without it the flag dies with the handler that set it.
///
/// Both are the ordinary case in an application (a data view with a
/// double-click-to-open hook, a finger nudging the list a few dp), and neither
/// is a fixture convenience: with either one absent the commit is *missed*
/// rather than *prevented*, and this test would be green over the defect.
fn assert_a_pan_off_a_selected_row_keeps_the_selection(
    view: impl teksilo_core::widget::Widget + 'static,
    offset: Signal<f32>,
    sel: teksilo_data::SelectionModel,
    what: &str,
) {
    sel.select_all(ROWS);
    let (mut tree, _id) = tree_with(view);
    let (id, at) = pan_up(&mut tree, centre(), ROW / 3.0);
    lift(&mut tree, id, at, 48);
    assert!(offset.get() > 0.0, "the pan scrolled {what}");
    assert_eq!(
        sel.count(),
        ROWS,
        "the pan collapsed {what}'s selection to the row the finger happened to start on",
    );
}

#[test]
fn a_pan_off_a_selected_tree_row_keeps_the_selection() {
    let sel = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Multi);
    let offset = Signal::new_animated(0.0);
    let view = TreeView::new(flat_tree(), |item, _entry, _sel| {
        Box::new(TextWidget::new(lit!(item.to_string())))
    })
    .item_height(ROW)
    .scroll_signal(offset.clone())
    .on_activate(|_i, _ctx| {})
    .selection(sel.clone());
    assert_a_pan_off_a_selected_row_keeps_the_selection(view, offset, sel, "the tree");
}

#[test]
fn a_pan_off_a_selected_list_row_keeps_the_selection() {
    let sel = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Multi);
    let view = ListView::new(items(), |i, _item, _s| {
        Box::new(TextWidget::new(lit!(i.to_string())))
    })
    .item_height(ROW)
    .on_activate(|_i, _ctx| {})
    .selection(sel.clone());
    let offset = view.scroll_y_signal().clone();
    assert_a_pan_off_a_selected_row_keeps_the_selection(view, offset, sel, "the list");
}

#[test]
fn a_pan_off_a_selected_table_row_keeps_the_selection() {
    use teksilo_widgets::table_view::TableSelectionMode;
    let sel = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Multi);
    let view = table()
        .on_row_activate(|_i, _ctx| {})
        .selection(sel.clone())
        .selection_mode(TableSelectionMode::MultiRow);
    let offset = view.scroll_y_signal().clone();
    assert_a_pan_off_a_selected_row_keeps_the_selection(view, offset, sel, "the table");
}

#[test]
fn a_pan_off_a_selected_tree_table_row_keeps_the_selection() {
    use teksilo_widgets::table_view::TableSelectionMode;
    let sel = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Multi);
    let view = TreeTableView::new(flat_tree())
        .row_height(ROW)
        .on_row_activate(|_i, _ctx| {})
        .selection(sel.clone())
        .selection_mode(TableSelectionMode::MultiRow);
    let offset = view.scroll_y_signal().clone();
    assert_a_pan_off_a_selected_row_keeps_the_selection(view, offset, sel, "the tree table");
}

// `GridView`'s release-time commit is the shared `deferred_select::on_up`, so it
// is guarded by the same line the other four are. It also used not to *reach* a
// pan at all in `Multi` mode, which left the guard nothing to be asked by; the
// test below is what that measurement became.

/// A finger scrolls a `GridView` whose selection is in `SelectionMode::Multi`.
///
/// This was a shipped defect, and the cause turned out to be neither the claim
/// losing the arbitration nor the marquee running instead. In `Multi` mode the
/// rubber-band marquee's `on_drag` sat on the `GridView`'s **own** node — the
/// node that also carries the `PanClaim`. That gave it a gesture arena, hence
/// the implicit press capture, and the capture dispatch runs before the
/// arbitration advances: its `DragRecognizer` latched at `drag_slop` (18 dp) on
/// the first move and *decided the sequence*, so the claim was never evaluated
/// at 36 dp and no scroll was ever synthesised. The marquee then declined the
/// press because it had landed on a tile. The drag won and did nothing — which
/// is why the trace showed the grid deciding while the selection stayed
/// untouched and the offset stayed 0.
///
/// The fix is structural and has two halves, each with its own test in
/// `data_view_drag.rs`: the body pane gets a gesture arena of its own
/// (`data_views::press_absorber`) so the press is captured *inside* the
/// claimant rather than by it, and the marquee's drag moves onto a
/// `data_views::DragSurface` that strictly encloses that pane — the one shape
/// the tree arms `DragActivation` for, so a finger's marquee now waits for a
/// long press while a mouse still latches at 5 dp.
#[test]
fn a_finger_on_a_multi_select_grid_pans_it() {
    let sel = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Multi);
    let view = grid_view().selection(sel.clone());
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);
    let (id, at) = pan_up(&mut tree, centre(), 120.0);
    lift(&mut tree, id, at, 48);
    assert!(
        offset.get() > 0.0,
        "a finger must scroll a multi-select grid; it left the offset at {}",
        offset.get(),
    );
}
