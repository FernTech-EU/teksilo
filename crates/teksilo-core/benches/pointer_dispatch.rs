// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The pointer-dispatch performance budget.
//!
//! Five measurements, over trees built here rather than borrowed from the
//! widget catalog — a bench of `teksilo-core` cannot reach `teksilo-widgets`,
//! which sits above it in the graph:
//!
//! * `pointer_move` — a `PointerMove` at one contact and at ten, on a
//!   twelve-deep tree. Ten is the contact cap, so it is the worst case a frame
//!   can be asked to dispatch, and it is what the hard budget is stated at.
//! * `hit_test` — the miss-only slop pass. The *miss* is the expensive case:
//!   an exact hit landing on something that takes a press short-circuits,
//!   while a miss walks every candidate in the subtree and sorts them.
//! * `touch_action` — a tap on a twenty-deep declared path, which is where the
//!   two path folds (`effective_touch_action`, `pan_candidates`) run.
//! * `fling` — one tick of a coast, chained scroll and all.
//! * `workspace` — the composite: an IDE-shaped tree with a rail, two docked
//!   panels and a virtualized table body, all under live pan claimants.
//!
//! # Running it
//!
//! ```text
//! cargo bench -p teksilo-core --bench pointer_dispatch                  # measure, then gate
//! cargo bench -p teksilo-core --bench pointer_dispatch -- --gate-only   # gate alone, ~1 s
//! cargo bench -p teksilo-core --bench pointer_dispatch -- --save-baseline p01
//! cargo bench -p teksilo-core --bench pointer_dispatch -- --baseline p01
//! ```
//!
//! The last two are criterion's own baseline machinery, and they are how the
//! mouse path is compared against a recorded SHA: a stored baseline is a
//! directory of measurements taken on one machine, so it belongs in that
//! machine's `target/`, never in the repository.
//!
//! # The gate, and what is only reported
//!
//! The binary's **exit status** is the hard budget: non-zero when a sample at
//! ten contacts costs more than 25 µs. Everything else it has to say — every
//! relative delta against a mouse baseline — is **printed** and cannot reach
//! the exit status; see `budget.rs` for why, and for where the separation is
//! enforced. A CI job runs the command and asserts on the status; the advisory
//! lines are for the human reading the log afterwards.
//!
//! The gate does its own timing rather than reading criterion's estimates.
//! criterion is the instrument you reach for when a number moved and you want
//! to know why — distributions, outliers, baselines, its own file layout. The
//! gate answers one question in about a second, from a median of short rounds,
//! and stays correct wherever `CRITERION_HOME` happens to point.

// Shared with the `budget_gate` bench target, which tests the arithmetic under
// a harness that runs `#[test]`. Each target uses a different part of it.
#[allow(dead_code)]
mod budget;

use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput};
use teksilo_canvas::{Point, Rect, Size, SizeProposal, Vec2};
use teksilo_core::event::{ButtonMask, EventResponse, Modifiers, PointerButton};
use teksilo_core::pointer::touch_action::{PanAxes, PanClaim, TouchAction};
use teksilo_core::pointer::{
    BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
    PointerSample,
};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;

use budget::{GATE_CONTACTS, Gate, Measured, Report};

// ---------------------------------------------------------------------------
// Fixture widgets
// ---------------------------------------------------------------------------

/// A leaf that fills whatever it is offered.
#[derive(Debug, Default)]
struct Slab;

impl Widget for Slab {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

/// A container that places each child at an absolute rectangle in its own
/// coordinates, so every fixture states its geometry outright instead of
/// deriving it from a stack. Children are placed in declaration order, which
/// makes the *last* one topmost — the order the reverse-sibling hit walk sees.
#[derive(Debug)]
struct Board {
    placed: Vec<(WidgetId, Rect)>,
}

impl Board {
    fn new() -> Self {
        Self { placed: Vec::new() }
    }

    fn at(mut self, id: WidgetId, rect: Rect) -> Self {
        self.placed.push((id, rect));
        self
    }
}

impl Widget for Board {
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
        for (child, (_, rect)) in children.iter_mut().zip(&self.placed) {
            child.origin = Point::new(bounds.x + rect.x, bounds.y + rect.y);
            child.size = rect.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.placed.iter().map(|(id, _)| *id).collect()
    }
}

// ---------------------------------------------------------------------------
// Sample construction
// ---------------------------------------------------------------------------

/// One touch sample, built the way `teksilo-platform`'s translator builds one.
///
/// The button mask is the part that matters and the part that is easy to get
/// wrong: a finger holds `PRIMARY` for as long as it is down, because every
/// `accept_buttons()` recognizer gates on it. A contact reporting an empty mask
/// would be invisible to tap, drag and long-press alike, and this bench would
/// then be measuring a cheaper path than the real one.
fn touch_sample(id: PointerId, phase: PointerPhase, at: Point, time: EventTime) -> PointerSample {
    let mut sample = PointerSample::mouse(phase, at, time);
    let mut pointer = PointerInfo::touch(id, time);
    pointer.buttons = match phase {
        PointerPhase::Down | PointerPhase::Move => ButtonMask::PRIMARY,
        _ => ButtonMask::NONE,
    };
    sample.pointer = pointer;
    sample.button = match phase {
        PointerPhase::Down | PointerPhase::Up => Some(PointerButton::Primary),
        _ => None,
    };
    sample.modifiers = Modifiers::NONE;
    sample
}

/// One mouse sample — the baseline every advisory is read against.
fn mouse_sample(phase: PointerPhase, at: Point, time: EventTime) -> PointerSample {
    let mut sample = PointerSample::mouse(phase, at, time);
    if phase == PointerPhase::Down {
        sample.pointer.buttons = ButtonMask::PRIMARY;
        sample.button = Some(PointerButton::Primary);
    }
    sample
}

/// A fresh contact id, minted the way a backend mints one: per press, keyed on
/// the OS-level contact slot it reuses.
fn fresh_contact(slot: u64) -> PointerId {
    let device = BackendDeviceKey::DEFAULT;
    PointerIdAllocator::global().end(device, slot);
    PointerIdAllocator::global().begin(device, slot)
}

/// The slot the one-off taps mint from, kept clear of the slots
/// [`press_contacts`] holds down.
const TAP_SLOT: u64 = 100;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// The root proposal every fixture is laid out under. A 1600 × 1000 logical
/// window: a desktop app on a laptop display, which is the shape the budget is
/// meant to hold for.
const WINDOW: Size = Size {
    width: 1600.0,
    height: 1000.0,
};

/// What every fixture root declares as its `TouchAction`.
///
/// Anything other than [`TouchAction::AUTO`], because
/// [`WidgetTree::sequence_touch_action`] answers `AUTO` both for a path that
/// declared nothing *and* for a pointer the table never admitted — so with an
/// all-`AUTO` fixture there is no way to tell the two apart, and
/// [`Contacts::on`] needs to.
const FIXTURE_ACTION: TouchAction = TouchAction::PAN;

/// How far apart the ten contacts sit in the nested fixture. Wider than
/// `pan_slop` for touch (36 dp) so a contact's drift can never wander into its
/// neighbour's cell.
const CONTACT_PITCH: f32 = 120.0;

/// How far each nested level insets its child.
const NEST_INSET: f32 = 2.0;

/// A tree exactly `depth` nodes deep from root to target, whose innermost
/// container holds `targets` side-by-side tappable cells, one per contact.
///
/// Every level carries three inert decorations declared *after* the spine, so
/// the reverse-sibling walk rejects something on the way down rather than
/// descending a bare chain — a panel's chrome painted over its content, which
/// is what a real tree looks like. They sit in the top-left corner, well clear
/// of the cell centres the contacts land on.
///
/// The depth is asserted rather than promised: the claim in the bench's name is
/// checked.
fn nested_tree(depth: usize, targets: usize) -> (WidgetTree, Vec<WidgetId>) {
    assert!(depth >= 3, "a spine needs a wrapper, a row and a cell");
    let mut tree = WidgetTree::new();

    // The innermost board is inset `NEST_INSET` once per wrapper above it.
    let wrappers = depth - 2;
    let inner = Size::new(
        WINDOW.width - 2.0 * NEST_INSET * wrappers as f32,
        WINDOW.height - 2.0 * NEST_INSET * wrappers as f32,
    );
    assert!(
        CONTACT_PITCH * targets as f32 <= inner.width,
        "the cell row must fit inside the innermost board"
    );

    let mut cells = Vec::with_capacity(targets);
    let mut row = Board::new();
    for i in 0..targets {
        let cell = tree.add(Slab.on_tap(|_, _| {}));
        cells.push(cell);
        row = row.at(
            cell,
            Rect::new(CONTACT_PITCH * i as f32, 0.0, CONTACT_PITCH, inner.height),
        );
    }
    let mut current = tree.add(row);

    for level in 0..wrappers {
        let mut board = Board::new().at(
            current,
            Rect::new(NEST_INSET, NEST_INSET, inner.width, inner.height),
        );
        for corner in 0..3 {
            let deco = tree.add(Slab);
            board = board.at(deco, Rect::new(4.0 + 12.0 * corner as f32, 4.0, 8.0, 8.0));
        }
        current = if level + 1 == wrappers {
            // The outermost wrapper declares [`FIXTURE_ACTION`] — see
            // `Contacts::on` for what the declaration is load-bearing for.
            tree.add(board.touch_action(FIXTURE_ACTION))
        } else {
            tree.add(board)
        };
    }

    tree.layout(SizeProposal::exact(WINDOW.width, WINDOW.height));
    assert_eq!(
        path_depth(&tree, cells[0]),
        depth,
        "the fixture must be exactly as deep as the bench says it is"
    );
    (tree, cells)
}

/// How many nodes lie on the root-to-`id` path, `id` included.
fn path_depth(tree: &WidgetTree, id: WidgetId) -> usize {
    let mut depth = 1;
    let mut current = id;
    while let Some(parent) = tree.parent(current) {
        depth += 1;
        current = parent;
    }
    depth
}

/// The centre of a widget, in window coordinates.
fn centre(tree: &WidgetTree, id: WidgetId) -> Point {
    tree.bounds(id).center()
}

/// Put one finger down per cell and hand back their ids.
fn press_contacts(
    tree: &mut WidgetTree,
    cells: &[WidgetId],
    clock: &mut EventTime,
) -> Vec<PointerId> {
    let mut ids = Vec::with_capacity(cells.len());
    for (slot, &cell) in cells.iter().enumerate() {
        let id = fresh_contact(slot as u64);
        let at = centre(tree, cell);
        *clock = *clock + Duration::from_millis(8);
        tree.dispatch_pointer(touch_sample(id, PointerPhase::Down, at, *clock));
        ids.push(id);
    }
    ids
}

/// A tree `depth` deep whose every node declares a `TouchAction` and a
/// `PanClaim`, so both path folds have `depth` declarations to visit.
///
/// The declarations alternate on purpose: a chain of identical ones would fold
/// to the same answer after one step, and an implementation that quietly
/// short-circuited on "nothing changed" would measure as free.
fn declared_path(depth: usize) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let leaf = tree.add(
        Slab.on_tap(|_, _| {})
            .touch_action(TouchAction::PAN)
            .pan_claim(PanClaim::vertical()),
    );
    let mut current = leaf;
    for level in 1..depth {
        let action = if level % 2 == 0 {
            TouchAction::MANIPULATION
        } else {
            TouchAction::AUTO
        };
        let size = Size::new(
            WINDOW.width - 2.0 * level as f32,
            WINDOW.height - 2.0 * level as f32,
        );
        current = tree.add(
            Board::new()
                .at(
                    current,
                    Rect::new(1.0, 1.0, size.width - 2.0, size.height - 2.0),
                )
                .touch_action(action)
                .pan_claim(PanClaim::vertical()),
        );
    }
    tree.layout(SizeProposal::exact(WINDOW.width, WINDOW.height));
    assert_eq!(path_depth(&tree, leaf), depth);
    (tree, leaf)
}

/// A grid of `rows × cols` targets smaller than the density's target size, so
/// each one earns a slop outset, plus a probe point in the gutter crossing
/// between four of them that hits nothing willing to take a press.
///
/// That is the expensive shape: with no eligible bubble owner the pass has
/// nothing to beat, so it collects and sorts every candidate in the subtree
/// instead of short-circuiting on a distance of zero.
fn slop_grid(rows: usize, cols: usize) -> (WidgetTree, Point) {
    const CELL: f32 = 16.0;
    const GUTTER: f32 = 24.0;
    let mut tree = WidgetTree::new();
    let mut board = Board::new();
    for row in 0..rows {
        for col in 0..cols {
            let cell = tree.add(Slab.on_tap(|_, _| {}));
            board = board.at(
                cell,
                Rect::new(
                    GUTTER + (CELL + GUTTER) * col as f32,
                    GUTTER + (CELL + GUTTER) * row as f32,
                    CELL,
                    CELL,
                ),
            );
        }
    }
    tree.add(board);
    tree.layout(SizeProposal::exact(WINDOW.width, WINDOW.height));
    let probe = Point::new(GUTTER + CELL + GUTTER / 2.0, GUTTER + CELL + GUTTER / 2.0);
    (tree, probe)
}

/// A coasting list: a vertical pan claimant that consumes what the coast
/// delivers, so every tick does the full round trip — simulation step, chained
/// dispatch, handler.
fn coasting_list() -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let list = tree.add(
        Slab.scroll_container(PanAxes::Y)
            .pan_claim(PanClaim::vertical())
            .on_scroll(|_, _| EventResponse::Handled),
    );
    tree.layout(SizeProposal::exact(WINDOW.width, WINDOW.height));
    (tree, list)
}

/// The composite: an IDE-shaped workspace, and its table body's cells in row
/// order.
///
/// A title bar, a ten-button activity rail, a docked side panel of sixty rows,
/// a centre table body of forty rows by eight columns, a docked bottom panel of
/// forty console lines, and a status bar. Each of the three docked bodies is a
/// live pan claimant with a real `on_scroll` and clips its content, so a press
/// enrols a chain rather than finding an empty one and the hit walk meets a
/// clip boundary on the way down.
///
/// **This is not what A21 asked for**, and it cannot be. A21 names "a docking
/// workspace with a virtualized `TableView`"; `DockingLayout` and `TableView`
/// live in `teksilo-widgets`, which depends on `teksilo-core`. A dev-dependency
/// back the other way would invert the graph for the sake of a benchmark. So
/// the shape is reproduced here from core's own parts: forty rows is what a
/// virtualized table body actually mounts, and the work the dispatcher does
/// over them is the same work either way.
fn workspace() -> (WidgetTree, Vec<WidgetId>) {
    const RAIL: f32 = 48.0;
    const TITLE_BAR: f32 = 36.0;
    const STATUS_BAR: f32 = 24.0;
    const SIDE: f32 = 280.0;
    const BOTTOM: f32 = 200.0;
    const ROW: f32 = 22.0;
    const TABLE_ROWS: usize = 40;
    const TABLE_COLS: usize = 8;

    let mut tree = WidgetTree::new();

    // Title bar: three window controls at the trailing edge.
    let mut title = Board::new();
    for i in 0..3 {
        let btn = tree.add(Slab.on_tap(|_, _| {}));
        title = title.at(
            btn,
            Rect::new(WINDOW.width - 40.0 * (3 - i) as f32, 4.0, 32.0, 28.0),
        );
    }
    let title = tree.add(title);

    // Activity rail: ten icon buttons down the leading edge.
    let mut rail = Board::new();
    for i in 0..10 {
        let btn = tree.add(Slab.on_tap(|_, _| {}));
        rail = rail.at(btn, Rect::new(4.0, 4.0 + 44.0 * i as f32, 40.0, 40.0));
    }
    let rail = tree.add(rail);

    let body_height = WINDOW.height - TITLE_BAR - STATUS_BAR;

    // Side panel: sixty tree rows, each an icon plus a label.
    let mut side_rows = Board::new();
    for i in 0..60 {
        let icon = tree.add(Slab);
        let label = tree.add(Slab.on_tap(|_, _| {}));
        let row = tree.add(
            Board::new()
                .at(icon, Rect::new(4.0, 4.0, 14.0, 14.0))
                .at(label, Rect::new(24.0, 0.0, SIDE - 24.0, ROW)),
        );
        side_rows = side_rows.at(row, Rect::new(0.0, ROW * i as f32, SIDE, ROW));
    }
    let side_rows = tree.add(side_rows);
    let side = tree.add(
        Board::new()
            .at(side_rows, Rect::new(0.0, 0.0, SIDE, ROW * 60.0))
            .scroll_container(PanAxes::Y)
            .pan_claim(PanClaim::vertical())
            .on_scroll(|_, _| EventResponse::Handled)
            .clips_children(true),
    );

    // Centre: a virtualized table body — forty realized rows, eight columns.
    let centre_width = WINDOW.width - RAIL - SIDE;
    let col_width = centre_width / TABLE_COLS as f32;
    let mut cells = Vec::with_capacity(TABLE_ROWS * TABLE_COLS);
    let mut grid = Board::new();
    for r in 0..TABLE_ROWS {
        let mut row = Board::new();
        for c in 0..TABLE_COLS {
            let cell = tree.add(Slab.on_tap(|_, _| {}));
            cells.push(cell);
            row = row.at(cell, Rect::new(col_width * c as f32, 0.0, col_width, ROW));
        }
        let row = tree.add(row);
        grid = grid.at(row, Rect::new(0.0, ROW * r as f32, centre_width, ROW));
    }
    let grid = tree.add(grid);
    let centre_height = body_height - BOTTOM;
    let centre = tree.add(
        Board::new()
            .at(
                grid,
                Rect::new(0.0, 0.0, centre_width, ROW * TABLE_ROWS as f32),
            )
            .scroll_container(PanAxes::BOTH)
            .pan_claim(PanClaim::both())
            .on_scroll(|_, _| EventResponse::Handled)
            .clips_children(true),
    );

    // Bottom panel: forty console lines.
    let mut console = Board::new();
    for i in 0..40 {
        let line = tree.add(Slab.on_tap(|_, _| {}));
        console = console.at(line, Rect::new(0.0, ROW * i as f32, centre_width, ROW));
    }
    let console = tree.add(console);
    let bottom = tree.add(
        Board::new()
            .at(console, Rect::new(0.0, 0.0, centre_width, ROW * 40.0))
            .scroll_container(PanAxes::Y)
            .pan_claim(PanClaim::vertical())
            .on_scroll(|_, _| EventResponse::Handled)
            .clips_children(true),
    );

    // Status bar: four segments.
    let mut status = Board::new();
    for i in 0..4 {
        let seg = tree.add(Slab.on_tap(|_, _| {}));
        status = status.at(seg, Rect::new(8.0 + 140.0 * i as f32, 2.0, 132.0, 20.0));
    }
    let status = tree.add(status);

    let body = tree.add(
        Board::new()
            .at(rail, Rect::new(0.0, 0.0, RAIL, body_height))
            .at(side, Rect::new(RAIL, 0.0, SIDE, body_height))
            .at(
                centre,
                Rect::new(RAIL + SIDE, 0.0, centre_width, centre_height),
            )
            .at(
                bottom,
                Rect::new(RAIL + SIDE, centre_height, centre_width, BOTTOM),
            ),
    );

    tree.add(
        Board::new()
            .at(title, Rect::new(0.0, 0.0, WINDOW.width, TITLE_BAR))
            .at(body, Rect::new(0.0, TITLE_BAR, WINDOW.width, body_height))
            .at(
                status,
                Rect::new(0.0, WINDOW.height - STATUS_BAR, WINDOW.width, STATUS_BAR),
            )
            .touch_action(FIXTURE_ACTION),
    );
    tree.layout(SizeProposal::exact(WINDOW.width, WINDOW.height));

    // The realized cells the contacts land on: one per row for the first ten
    // rows, striding the columns so the contacts spread across the body rather
    // than stacking in one (ten contacts over eight columns must reuse two of
    // them; it is the distinct rows that make every cell a distinct node).
    // All ten are inside the centre viewport, which clips below its own
    // height — a contact landing on a clipped row would be re-attributed to
    // the viewport, and ten contacts on one node is nine refusals under
    // `MultiContact::First`.
    let contacts: Vec<WidgetId> = (0..GATE_CONTACTS)
        .map(|i| cells[i * TABLE_COLS + (i * 3) % TABLE_COLS])
        .collect();
    for &cell in &contacts {
        let b = tree.bounds(cell);
        assert!(
            b.bottom() <= TITLE_BAR + centre_height,
            "a contact cell must be inside the clipped viewport"
        );
    }
    (tree, contacts)
}

// ---------------------------------------------------------------------------
// The moving part every bench and the gate share
// ---------------------------------------------------------------------------

/// A contact drifting inside its own cell, the way a finger holding a control
/// actually behaves at 120 Hz: a few dp of tremor, never far enough to arm a
/// pan.
///
/// Deliberately *not* a sweep across the tree. A move that changed target on
/// every sample would spend its time in hover enter/leave rather than in
/// dispatch, and the budget is about dispatch.
struct Drift {
    origin: Point,
    step: u32,
}

impl Drift {
    fn new(origin: Point) -> Self {
        Self { origin, step: 0 }
    }

    /// The next position: a three-dp square traced one corner at a time.
    fn next(&mut self) -> Point {
        const R: f32 = 3.0;
        let p = match self.step % 4 {
            0 => Point::new(self.origin.x + R, self.origin.y),
            1 => Point::new(self.origin.x, self.origin.y + R),
            2 => Point::new(self.origin.x - R, self.origin.y),
            _ => Point::new(self.origin.x, self.origin.y - R),
        };
        self.step = self.step.wrapping_add(1);
        p
    }
}

/// A live multi-contact session: `n` fingers down on `n` cells, each drifting.
struct Contacts {
    tree: WidgetTree,
    ids: Vec<PointerId>,
    drifts: Vec<Drift>,
    clock: EventTime,
}

impl Contacts {
    fn on(mut tree: WidgetTree, cells: &[WidgetId]) -> Self {
        let mut clock = EventTime::ZERO;
        let ids = press_contacts(&mut tree, cells, &mut clock);
        // Load-bearing for the gate, not decoration. The per-sample figure is
        // a round divided by `ids.len()`, and a sample the pointer table
        // refuses returns from `dispatch_pointer` before any work happens at
        // all — so one contact over the cap would have the gate dividing by
        // more work than it did, and under-reporting by that ratio. An
        // admitted press has an entry with a sequence whose frozen action is
        // the one the fixture declared; a refused one has no entry, and reads
        // back as `AUTO`.
        for &id in &ids {
            assert_eq!(
                tree.sequence_touch_action(id),
                FIXTURE_ACTION,
                "contact {id:?} never reached the tree: the fixture must keep every \
                 finger admissible, or the per-sample divisor is a lie"
            );
        }
        let drifts = cells
            .iter()
            .map(|&c| Drift::new(centre(&tree, c)))
            .collect();
        Self {
            tree,
            ids,
            drifts,
            clock,
        }
    }

    /// `n` contacts down on the deepest cells of a `depth`-deep tree.
    fn nested(depth: usize, n: usize) -> Self {
        let (tree, cells) = nested_tree(depth, n);
        Self::on(tree, &cells)
    }

    /// Ten contacts down on the workspace's table body.
    fn workspace() -> Self {
        let (tree, cells) = workspace();
        Self::on(tree, &cells)
    }

    /// One sample per live contact — what a frame's worth of coalesced input
    /// looks like when every finger moved.
    fn move_all(&mut self) {
        self.clock = self.clock + Duration::from_millis(8);
        for (i, &id) in self.ids.iter().enumerate() {
            let at = self.drifts[i].next();
            self.tree
                .dispatch_pointer(touch_sample(id, PointerPhase::Move, at, self.clock));
        }
    }

    /// How many samples one [`Self::move_all`] delivers.
    fn samples_per_move(&self) -> usize {
        self.ids.len()
    }
}

/// A mouse drifting over the same tree: the baseline every advisory is read
/// against.
struct MousePath {
    tree: WidgetTree,
    drift: Drift,
    clock: EventTime,
}

impl MousePath {
    fn on(tree: WidgetTree, cell: WidgetId) -> Self {
        let origin = centre(&tree, cell);
        Self {
            tree,
            drift: Drift::new(origin),
            clock: EventTime::ZERO,
        }
    }

    fn nested(depth: usize) -> Self {
        let (tree, cells) = nested_tree(depth, 1);
        Self::on(tree, cells[0])
    }

    fn workspace() -> Self {
        let (tree, cells) = workspace();
        Self::on(tree, cells[0])
    }

    fn move_once(&mut self) {
        self.clock = self.clock + Duration::from_millis(8);
        let at = self.drift.next();
        self.tree
            .dispatch_pointer(mouse_sample(PointerPhase::Move, at, self.clock));
    }
}

/// A tap — down then up — on a fresh contact id, which is what a real backend
/// mints per press. The press is where the two path folds run.
struct Tapper {
    tree: WidgetTree,
    at: Point,
    clock: EventTime,
}

impl Tapper {
    fn on_declared_path(depth: usize) -> Self {
        let (tree, leaf) = declared_path(depth);
        let at = centre(&tree, leaf);
        let mut tapper = Self {
            tree,
            at,
            clock: EventTime::ZERO,
        };
        // A tap is meant to be self-contained. Prove it before measuring
        // millions of them on one tree: if a press left state behind, the
        // bench would be measuring an ever-growing table rather than a tap.
        for _ in 0..3 {
            tapper.tap();
        }
        tapper.tree.assert_no_leaked_pointer_state();
        tapper
    }

    fn tap(&mut self) {
        let id = fresh_contact(TAP_SLOT);
        self.clock = self.clock + Duration::from_millis(8);
        self.tree
            .dispatch_pointer(touch_sample(id, PointerPhase::Down, self.at, self.clock));
        self.clock = self.clock + Duration::from_millis(40);
        self.tree
            .dispatch_pointer(touch_sample(id, PointerPhase::Up, self.at, self.clock));
    }
}

/// A coast kept alive: restarted whenever it has decayed to a stop, so the
/// timed tick is always a real one rather than the early return an empty
/// driver takes.
struct Coast {
    tree: WidgetTree,
    list: WidgetId,
    now: Instant,
}

impl Coast {
    fn new() -> Self {
        let (tree, list) = coasting_list();
        let now = tree.simulated_now();
        let mut coast = Self { tree, list, now };
        coast.restart();
        coast
    }

    fn restart(&mut self) {
        // The clamped maximum — the fastest flick the physics admits, and so
        // the longest coast, which keeps the restart down to once per couple of
        // thousand ticks.
        self.tree.start_fling(
            self.list,
            Vec2::new(0.0, -8000.0),
            vec![(self.list, PanClaim::vertical())],
        );
    }

    /// Advance the coast by one 8 ms frame.
    fn tick(&mut self) {
        if !self.tree.is_flinging(self.list) {
            self.restart();
        }
        self.now += Duration::from_millis(8);
        self.tree.tick_flings(self.now);
    }
}

// ---------------------------------------------------------------------------
// The criterion benches
// ---------------------------------------------------------------------------

fn bench_pointer_move(c: &mut Criterion) {
    let mut group = c.benchmark_group("pointer_move");

    group.throughput(Throughput::Elements(1));
    group.bench_function("mouse_depth12", |b| {
        let mut path = MousePath::nested(12);
        b.iter(|| path.move_once());
    });
    group.bench_function("touch_1_contact_depth12", |b| {
        let mut contacts = Contacts::nested(12, 1);
        b.iter(|| contacts.move_all());
    });

    group.throughput(Throughput::Elements(GATE_CONTACTS as u64));
    group.bench_function("touch_10_contacts_depth12", |b| {
        let mut contacts = Contacts::nested(12, GATE_CONTACTS);
        b.iter(|| contacts.move_all());
    });

    group.finish();
}

fn bench_hit_test(c: &mut Criterion) {
    let mut group = c.benchmark_group("hit_test");
    let (tree, probe) = slop_grid(20, 20);
    let touch = PointerInfo::touch(PointerId::MOUSE, EventTime::ZERO);
    let mouse = PointerInfo::mouse(EventTime::ZERO);

    // The miss: nothing under the probe takes a press, so the pass has no
    // bubble owner to beat and walks all four hundred candidates.
    group.bench_function("slop_miss_touch_400_candidates", |b| {
        b.iter(|| std::hint::black_box(tree.hit_test_for(probe, &touch)))
    });
    // The same miss for a mouse, whose radius is zero: the pass is not entered
    // at all. This is the "no new work for a precise pointer" claim, measured.
    group.bench_function("slop_miss_mouse_400_candidates", |b| {
        b.iter(|| std::hint::black_box(tree.hit_test_for(probe, &mouse)))
    });

    group.finish();
}

fn bench_touch_action(c: &mut Criterion) {
    let mut group = c.benchmark_group("touch_action");

    // A press is where `effective_touch_action` and `pan_candidates` fold the
    // whole root-to-target chain. Both are `pub(crate)`, so the fold is
    // measured through the door that runs it rather than on its own, and the
    // pair of depths is what makes its contribution visible — imperfectly, since
    // the shallower tree also has a shorter preview/bubble path.
    for depth in [2_usize, 20] {
        group.bench_with_input(
            BenchmarkId::new("tap_declared_path", depth),
            &depth,
            |b, &depth| {
                let mut tapper = Tapper::on_declared_path(depth);
                b.iter(|| tapper.tap());
            },
        );
    }

    group.finish();
}

fn bench_fling(c: &mut Criterion) {
    let mut group = c.benchmark_group("fling");
    group.bench_function("tick", |b| {
        let mut coast = Coast::new();
        b.iter(|| coast.tick());
    });
    group.finish();
}

fn bench_workspace(c: &mut Criterion) {
    let mut group = c.benchmark_group("workspace");

    group.throughput(Throughput::Elements(1));
    group.bench_function("mouse_move", |b| {
        let mut path = MousePath::workspace();
        b.iter(|| path.move_once());
    });

    group.throughput(Throughput::Elements(GATE_CONTACTS as u64));
    group.bench_function("touch_10_contacts_move", |b| {
        let mut contacts = Contacts::workspace();
        b.iter(|| contacts.move_all());
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------

/// How many rounds the median is taken over. Enough that a couple of
/// descheduled rounds cannot reach the middle.
const ROUNDS: usize = 25;

/// How many iterations one round times as a block. Sized so a round lands
/// around a millisecond — long enough to swamp the clock's own resolution,
/// short enough that the whole gate costs about a second.
const BATCH: usize = 400;

/// Median time **per sample** over [`ROUNDS`] rounds of [`BATCH`] iterations,
/// where one iteration delivers `samples_per_iter` samples.
fn measure(samples_per_iter: usize, mut run: impl FnMut()) -> Duration {
    assert!(samples_per_iter > 0);
    // Warm up: first touch of a page, first branch prediction, first growth of
    // every Vec the dispatch path allocates.
    for _ in 0..BATCH {
        run();
    }
    let mut rounds = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let started = Instant::now();
        for _ in 0..BATCH {
            run();
        }
        rounds.push(started.elapsed() / (BATCH * samples_per_iter) as u32);
    }
    budget::median(&mut rounds)
}

/// Time the gate's subject and everything read beside it, and build the report.
///
/// The subject is fixed: a `PointerMove` at ten contacts on the twelve-deep
/// tree. Everything else on the report is an advisory and cannot reach the
/// verdict.
fn audit() -> Report {
    let mut gated = Contacts::nested(12, GATE_CONTACTS);
    let per_move = gated.samples_per_move();
    let subject = Measured::new(
        "pointer_move touch x10 depth12",
        measure(per_move, || gated.move_all()),
    );

    let mut report = Report::new(Gate {
        subject: subject.clone(),
        budget: budget::budget(),
    });

    let mut mouse = MousePath::nested(12);
    let mouse_baseline = Measured::new(
        "pointer_move mouse depth12",
        measure(1, || mouse.move_once()),
    );

    let mut one = Contacts::nested(12, 1);
    let one_per_move = one.samples_per_move();
    report.advise(
        Measured::new(
            "pointer_move touch x1 depth12",
            measure(one_per_move, || one.move_all()),
        ),
        mouse_baseline.clone(),
    );
    report.advise(subject, mouse_baseline);

    let mut ws_mouse = MousePath::workspace();
    let ws_baseline = Measured::new("workspace mouse", measure(1, || ws_mouse.move_once()));
    let mut ws_touch = Contacts::workspace();
    let ws_per_move = ws_touch.samples_per_move();
    report.advise(
        Measured::new(
            "workspace touch x10",
            measure(ws_per_move, || ws_touch.move_all()),
        ),
        ws_baseline,
    );

    let (tree, probe) = slop_grid(20, 20);
    let touch = PointerInfo::touch(PointerId::MOUSE, EventTime::ZERO);
    let mouse_info = PointerInfo::mouse(EventTime::ZERO);
    let slop_baseline = Measured::new(
        "hit_test mouse, slop pass not entered",
        measure(1, || {
            std::hint::black_box(tree.hit_test_for(probe, &mouse_info));
        }),
    );
    report.advise(
        Measured::new(
            "hit_test touch miss, 400 candidates",
            measure(1, || {
                std::hint::black_box(tree.hit_test_for(probe, &touch));
            }),
        ),
        slop_baseline,
    );

    report
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);

    // `--gate-only` skips the instrument entirely: a CI job that only wants the
    // verdict pays a second rather than several minutes. criterion never sees
    // the flag, which is why it is read before `configure_from_args`.
    let gate_only = flag("--gate-only");

    // Whether this run is a measurement at all, decided by criterion's own
    // convention rather than by guessing: `cargo bench` passes `--bench`, and
    // `cargo test --benches` passes nothing at all — which criterion reads as
    // "run each benchmark once to prove it executes". The absence of `--bench`
    // is therefore the signal, not the presence of `--test` (`--test` appears
    // only when a human writes `cargo bench -- --test`). A build that ran each
    // benchmark once, unoptimized, has measured nothing for the gate to judge.
    let measuring = flag("--bench");

    if !gate_only {
        let mut criterion = Criterion::default().configure_from_args();
        bench_pointer_move(&mut criterion);
        bench_hit_test(&mut criterion);
        bench_touch_action(&mut criterion);
        bench_fling(&mut criterion);
        bench_workspace(&mut criterion);
        criterion.final_summary();
    }

    if !gate_only && !measuring {
        return;
    }

    let report = audit();
    print!("{}", report.render());
    std::process::exit(report.exit_code());
}
