// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The arbitration matrix: pointer kind × frozen `TouchAction` × member set ×
//! movement vector → the named winner and each loser's exact cancel.
//!
//! # What this file is
//!
//! One table, [`rows`], of press-and-move scenarios with their whole
//! observable arbitration outcome written out: which competitors were enrolled
//! at the press and in what state, what the press froze, who had won after each
//! step of the movement, and which of the losers received a
//! [`CancelReason`] — spelled by *name*, so a row that changes reads as a
//! sentence rather than as a diff of widget ids.
//!
//! The **mouse rows record today's behaviour**. They are the no-regression
//! proof for the touch programme: an indirect precise pointer must come out of
//! it latching where it always latched, deciding when it always decided, and
//! cancelling nobody it did not cancel before.
//!
//! # Why the fixtures are not the real widgets
//!
//! All seven scenarios the design names live in `teksilo-widgets` or
//! `teksilo-scene`, and `teksilo-core` cannot depend on either. Each row is
//! therefore the **minimal core-only fixture** that reproduces the real
//! widget's *arbitration shape* — the handlers it installs, the claims it
//! declares, and the capture it takes — and the row names say which widget the
//! shape is taken from, with the source line it was read off. Two consequences,
//! both deliberate:
//!
//! * a row proves what the **framework** does with that shape, not what the
//!   named widget does today. The production declarers of a [`PanClaim`] are
//!   the nine surfaces that adopt `ScrollableBehavior` — `ScrollArea`, the five
//!   data views and the three text surfaces — plus `Terminal` and `SceneView`,
//!   which declare their own directly because neither scrolls a pixel offset in
//!   `[0, max]`; `TabBar` inherits one from the scroll area it wraps its header
//!   row in. Every *other* scenario's claimant is the enclosing scroller the
//!   real shape would sit in, written out. So the generated table in
//!   `docs/events-and-gestures.md` is labelled as fixture behaviour;
//! * when a widget's shape changes, this file does not notice. That is what
//!   the source line in each scenario's doc comment is for.
//!
//! # Which mechanism each row is known to cover
//!
//! Every row here was checked by deleting the mechanism it names and
//! confirming it goes red — the only way to tell a row apart from a row that
//! merely happens to pass — with one exception, recorded below: the three
//! gates the four mouse rows with a pan claimant name have never been removed
//! all at once, only one at a time. [`every_matrix_row_holds`] evaluates **every** row
//! whatever the earlier ones did and names the whole set, so the blast radius
//! below is what one run reports rather than what several runs pieced
//! together, and a mapping that says "and no other" is checkable by deleting
//! the mechanism once. The mapping, so the next reader does not have to
//! re-derive it: `PointerSequence::latch_slop`'s precise branch → *column grip
//! declaring NONE · touch*, alone — the *pen* row declaring `NONE` is
//! unmoved, because `PEN.slop_precise` and `PEN.drag_slop` are both 2.0; `note_explicit_capture`'s press-time decision →
//! *column grip · mouse* and `mouse_latch_is_five_in_every_configuration`;
//! `PointerSequence::resolve_activation`'s `AfterLongPress` branch → *scene
//! marquee in a scroller · touch*; `recognized_owning_gesture` → *list row ·
//! mouse* and every other `Gesture`-member row; `note_preview_claim` →
//! *nested previewers*; `PointerSequence::decide`'s live-only filter and the
//! tap-boundary self-reject sweep → *scene marquee in a scroller, press at the
//! edge*; `WidgetTree::pan_candidates`' axis filter → *two-axis scroller under
//! PAN_X · touch* (with *under PAN* as its control);
//! `PointerSequence::latch_slop`'s `is_none()` **condition**, as against any
//! action narrower than `AUTO` → *column grip declaring MANIPULATION · touch*;
//! `begin_sequence`'s `is_direct` guard → **nothing at all**: removing it
//! reddens no row of this table and no other test in `teksilo-core`, which is
//! what one gate of a redundant set looks like and is why the mouse rows that
//! name it are labelled as covering the rule;
//! [`PanClaim::devices`] and `GestureProfile::PEN`'s `pan_slop` → *text
//! selection · pen* **and** *column grip · pen*, plus
//! `a_pen_loses_an_explicit_capture_to_a_drag_capable_ancestor`. Either gate
//! alone produces that set, and the two sets are identical failure for
//! failure: between them these rows say a device gate moved, never which.
//! `PointerSequence::defer_own_drag` → the three *scene view with its own
//! claim* rows on a **direct** pointer, and no other row in the table, but each
//! call site is singly observable only on its own rows and the ancestor call has
//! a companion it cannot be separated from. Measured, deletion by deletion:
//! removing the call in `enrol_sequence_members`' **captured** arm reddens
//! *scene view with its own claim · touch* and *· pen* (plus ten rows of
//! `tests/dual_role_arbitration.rs`); removing its **ancestor** arm's call
//! reddens *no row here at all*, because that arm also feeds the ancestor's
//! `Down` and without the feed its recognizer cannot latch either way — the
//! single witness is
//! `an_ancestor_dual_role_nodes_own_drag_runs_after_a_hold`. Keep the feed and
//! drop only the deferral and *scene view with its own claim, over a widget item
//! · touch* goes red, which is what that row is for. The two **mouse** rows of
//! the same two scenarios stay green through every one of those deletions, and
//! that is the structural statement the pair is here to make: the arm refuses
//! anything but a live `Pan` member, and a mouse enrols none.
//! The arena gate (`sequence_blocks_member`'s `own_drag_armed_at` clause) and
//! `note_gesture_recognized`'s `own_drag_blocked` guard **mask each other** for
//! the *winner* these rows assert: with only the first removed the drag fires
//! but never claims the sequence, so every row here stays green while six rows
//! of `dual_role_arbitration.rs` — which count the handler calls — go red.
//! Remove both and the same three direct-pointer rows redden. The guard alone
//! is not singly observable anywhere, which is what a deliberately redundant
//! read looks like; its comment says so.
//!
//! The rows that cover a **rule** rather than one implementation of it are
//! *slider thumb · touch* and the four mouse rows whose fixture does carry a
//! pan claimant — *list row*, *text selection*, *tab strip vs tab drag*,
//! *column grip*. Both claims are enforced by mutually-masking gates, so no
//! single deletion is expected to redden those rows. For the four mouse rows
//! that is established by run for two of their three gates and left to
//! inspection for the third: `begin_sequence`'s `is_direct` guard reddens
//! nothing anywhere in `teksilo-core`, `PanClaim::devices`' DIRECT default
//! reddens only the pen rows named above, and `GestureProfile::MOUSE`'s absent
//! `pan_slop` has **not** been individually mutated. Their notes
//! say so, and name the row on which each gate is singly observable where one
//! exists (*text selection · pen*, for the two device gates — singly
//! observable, not distinguishable: either removal reddens it the same way).
//! The remaining mouse
//! rows — *scene marquee*, *drag region*, *nested previewers* — have no
//! claimant in the fixture at all, and the marquee's note says so rather than
//! letting an empty member list read as the mouse's doing.
//!
//! # Placement
//!
//! `tests/`, as the plan names it. `teksilo_core::test_widgets` is
//! `pub(crate)`, so the two shapes these fixtures need — a leaf and a stack —
//! are restated in `tests/common/mod.rs` against the public `Widget` trait
//! rather than widening that module: a seam reachable only by widening
//! visibility is the wrong seam. Everything else the matrix reads
//! (`sequence_winner`, `sequence_members`, `sequence_touch_action`, the A21
//! touch/pen helpers, `MemberRole`, `MemberState`, `CancelReason`) is already
//! public.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use common::{Leaf, Stack};
use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::gesture::{MemberRole, MemberState};
use teksilo_core::pointer::touch_action::{PanClaim, TouchAction};
use teksilo_core::pointer::{CancelReason, PointerId};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_tokens::{PenKind, PointerKind, TargetDensity};

// ---------------------------------------------------------------------------
// The vocabulary a row is written in
// ---------------------------------------------------------------------------

/// A [`MemberRole`] with its payload dropped, so a row can name one as a
/// constant.
///
/// `MemberRole::Pan` carries the claim it was enrolled for; a row cares that
/// the claimant competes, not which axes it asked for — the axes are already
/// pinned by the frozen `TouchAction` the same row asserts.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Role {
    Gesture,
    RawDrag,
    RawPreview,
    Pan,
}

impl Role {
    fn of(role: MemberRole) -> Self {
        match role {
            MemberRole::Gesture => Role::Gesture,
            MemberRole::RawDrag => Role::RawDrag,
            MemberRole::RawPreview => Role::RawPreview,
            MemberRole::Pan(_) => Role::Pan,
            // `MemberRole` is `#[non_exhaustive]`: a role added later has no
            // tag here yet, and a row asserting an exact member list must fail
            // loudly rather than silently reading it as something else.
            other => panic!("the arbitration matrix has no tag for {other:?}"),
        }
    }
}

/// One step of a row's movement: an offset from the press point, and who must
/// own the sequence once that sample has been dispatched.
///
/// Written as a *sequence* of steps rather than one end position because the
/// threshold is the point: a row that only samples past the slop passes with
/// the slop set to anything smaller.
type Step = (f32, f32, Option<&'static str>);

/// One row of the matrix.
struct Row {
    /// The row's name in the generated documentation table.
    name: &'static str,
    /// Which fixture to build.
    scenario: Scenario,
    /// Which device presses.
    kind: PointerKind,
    /// The `TouchAction` the press must freeze.
    frozen: TouchAction,
    /// Every competitor at the press, innermost first — exactly, in order.
    members: &'static [(&'static str, Role, MemberState)],
    /// Where to press, when the fixture's centre is not the point. `None` is
    /// the centre.
    press_at: Option<(f32, f32)>,
    /// The movement, and the winner after each step.
    movement: &'static [Step],
    /// Each cancel any node received over the whole press, in delivery order.
    cancels: &'static [(&'static str, CancelReason)],
    /// What the row is evidence *of*. One sentence; it becomes the table's
    /// last column.
    note: &'static str,
}

// ---------------------------------------------------------------------------
// The fixtures
// ---------------------------------------------------------------------------

/// Which core-only fixture a row is run against.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Scenario {
    /// **A5 scenario 1 — a reorderable row in a vertical list.**
    /// `teksilo-widgets/src/list_view/body_pane.rs:471` installs the row's
    /// drag; the enclosing scroll area declares the claim
    /// (`common/scrollable.rs:610`).
    ListRow,
    /// **A5 scenario 2 — a slider thumb**, which declares
    /// `TouchAction::NONE` (`teksilo-widgets/src/slider.rs:414`) inside a
    /// scroller.
    Slider,
    /// **A5 scenario 3 — text selection inside a `PAN_Y` scroller.** The
    /// editor answers `Ignored` on the press
    /// (`teksilo-widgets/src/rich_text/mouse.rs:12`) and owns an arena only
    /// through its multi-tap handlers (`rich_text.rs:3870`), so it is the
    /// implicit captor and **not** an explicit `RawDrag`.
    ///
    /// The scroller declares `TouchAction::PAN_Y` as well as a vertical claim,
    /// which is what the scenario is named for: a `ScrollArea` that scrolls on
    /// one axis is the shape in which a frozen single-axis action and a claim
    /// agreeing with it meet.
    TextSelection,
    /// **A5 scenario 4 — a `SceneView` marquee**: a draggable container above
    /// tappable cards, with nothing claiming a pan.
    ///
    /// The *shipped* view is [`Scenario::SceneViewDualRole`] — this shape is the
    /// one it has when `interactive` is off, so nothing declares a claim. (This
    /// doc comment used to cite `teksilo-scene/src/view/build_impl.rs:451` for
    /// the drag; the declaration is at `:479`, inside the `if self.interactive`
    /// block that opens at `:447`, and the same block declares the claim at
    /// `:464` — which is the shape this scenario deliberately does *not* have.)
    SceneMarquee,
    /// [`Scenario::SceneMarquee`] inside a scroller — a shape in which
    /// [`teksilo_tokens::DragActivation`] reaches production behaviour, its drag
    /// owner being a strict *ancestor* of the captor.
    SceneMarqueeInScroller,
    /// **The shipped `SceneView` with selection or magnetism on**: one node
    /// declaring a [`PanClaim`] *and* carrying `on_drag`, plus the `on_tap` its
    /// empty-space click needs.
    ///
    /// `teksilo-scene/src/view/build_impl.rs:464` declares the claim and `:479`
    /// the drag handlers, both onto the same `HandlerSet` inside the
    /// `if self.interactive` block that opens at `:447` — so this is one node
    /// wanting two roles, and no other row in this table has that shape. Its
    /// absence is why the defect it pins shipped: the claim is enrolled by
    /// `begin_sequence` before any handler runs, so the drag's `enrol` was
    /// refused and its `DragActivation` never consulted.
    SceneViewDualRole,
    /// [`Scenario::SceneViewDualRole`] with an interactive child between the
    /// press and the dual-role node — a heavyweight (`add_widget_item`) scene
    /// item, which takes the capture itself.
    ///
    /// The second door to the same refusal: the ancestor walk reaches it
    /// through `enrol_drag`, which opens with the same `enrol`.
    SceneViewDualRoleOverItem,
    /// **A5 scenario 5 — a `DragRegion` window move**
    /// (`teksilo-widgets/src/title_bar/drag_region.rs:168`): drag plus
    /// double-tap on the node the pointer lands on, no capture, no claim.
    DragRegion,
    /// **A5 scenario 6 — a `TabBar` strip against a tab drag**
    /// (`teksilo-widgets/src/tab_widget/header.rs:920`), the horizontal twin of
    /// [`Scenario::ListRow`]; the strip's claim comes from the `ScrollArea`
    /// `TabBar` wraps its header row in (`tab_widget/bar.rs:1603` for the
    /// horizontal strip, `:1609` for the vertical one).
    TabStrip,
    /// **A5 scenario 7 — a `TableView` column grip**
    /// (`teksilo-widgets/src/table_view/header.rs:751`): `capture_pointer()`
    /// and `Handled` on the press, under the header strip's claim and a
    /// draggable ancestor.
    ColumnGrip,
    /// [`Scenario::ColumnGrip`] with the grip declaring `TouchAction::NONE` —
    /// the splitter / dock-handle shape, and the only configuration that
    /// reaches `PointerSequence::latch_slop`'s precise branch.
    ColumnGripNone,
    /// [`Scenario::ColumnGrip`] with the grip declaring
    /// `TouchAction::MANIPULATION`. The control for
    /// [`Scenario::ColumnGripNone`]: `latch_slop`'s precise branch keys on
    /// `NONE` **exactly**, not on "anything narrower than `AUTO`".
    ColumnGripManipulation,
    /// A scroller claiming **both** axes, under a frozen action the row
    /// chooses. The shape in which `pan_candidates`' "excluded entirely —
    /// never narrowed" rule is observable: a two-axis claim under a
    /// single-axis action is dropped whole, and no other fixture can show
    /// that because a claim naming one axis is filtered identically by
    /// `pan_candidates` and by `PointerSequence::pan_is_eligible`.
    TwoAxisScroller(TouchAction),
    /// A nested pair of raw previewers, both claiming the press.
    NestedPreviewers,
    /// One tappable control, for the two-fingers-on-one-button rows.
    Button,
    /// That control inside a scroller.
    ButtonInScroller,
}

/// A built fixture: the tree, the names its nodes answer to, and the cancel log
/// every one of them writes into.
struct Fixture {
    tree: WidgetTree,
    names: Vec<(&'static str, WidgetId)>,
    cancels: Rc<RefCell<Vec<(&'static str, CancelReason)>>>,
    /// Where a row presses. The centre of the fixture, so every node on the
    /// path is hit and the movement has room in all four directions.
    press: Point,
    /// Taps that reached the innermost control, if the fixture has one.
    taps: Rc<Cell<usize>>,
}

impl Fixture {
    fn id(&self, name: &str) -> WidgetId {
        self.names
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, id)| *id)
            .unwrap_or_else(|| panic!("no node named {name:?} in this fixture"))
    }

    fn name_of(&self, id: WidgetId) -> &'static str {
        self.names
            .iter()
            .find(|(_, other)| *other == id)
            .map(|(n, _)| *n)
            .unwrap_or("<unnamed>")
    }

    fn winner(&self, pointer: PointerId) -> Option<&'static str> {
        self.tree
            .sequence_winner(pointer)
            .map(|id| self.name_of(id))
    }

    fn members(&self, pointer: PointerId) -> Vec<(&'static str, Role, MemberState)> {
        self.tree
            .sequence_members(pointer)
            .into_iter()
            .map(|(id, role, state)| (self.name_of(id), Role::of(role), state))
            .collect()
    }

    fn cancel_log(&self) -> Vec<(&'static str, CancelReason)> {
        self.cancels.borrow().clone()
    }
}

/// A cancel-recording tap of the shared log, for one named node.
fn recorder(
    log: Rc<RefCell<Vec<(&'static str, CancelReason)>>>,
    name: &'static str,
) -> impl FnMut(
    &teksilo_core::pointer::PointerInfo,
    CancelReason,
    &mut teksilo_core::widget::EventContext,
) + use<> {
    move |_pointer, reason, _ctx| log.borrow_mut().push((name, reason))
}

fn build(scenario: Scenario) -> Fixture {
    let cancels: Rc<RefCell<Vec<(&'static str, CancelReason)>>> = Rc::new(RefCell::new(Vec::new()));
    let taps = Rc::new(Cell::new(0usize));
    let mut tree = WidgetTree::new();
    // Compact is the density the whole programme's no-regression criterion is
    // stated at, and slops are density-independent — `mouse_latch_is_five_in_
    // every_configuration` below asserts that rather than assuming it.
    tree.set_density(TargetDensity::Compact);
    let mut names: Vec<(&'static str, WidgetId)> = Vec::new();

    macro_rules! node {
        ($name:literal, $widget:expr) => {{
            let id = tree.add($widget.on_pointer_cancel(recorder(cancels.clone(), $name)));
            names.push(($name, id));
            id
        }};
    }

    match scenario {
        Scenario::ListRow => {
            let leaf = node!("leaf", Leaf::new());
            let t = taps.clone();
            let row = node!(
                "row",
                Stack::new()
                    .child(leaf)
                    .on_tap(move |_e, _c| t.set(t.get() + 1))
                    .on_drag(|_p, _c| {})
            );
            node!(
                "scroller",
                Stack::new().child(row).pan_claim(PanClaim::vertical())
            );
        }
        Scenario::Slider => {
            let t = taps.clone();
            let slider = node!(
                "slider",
                Leaf::new()
                    .on_tap(move |_e, _c| t.set(t.get() + 1))
                    .on_drag(|_p, _c| {})
                    .touch_action(TouchAction::NONE)
            );
            node!(
                "scroller",
                Stack::new().child(slider).pan_claim(PanClaim::vertical())
            );
        }
        Scenario::TextSelection => {
            let editor = node!(
                "editor",
                Leaf::new()
                    .on_double_tap(|_e, _c| {})
                    .on_pointer_event(|_e, _c| EventResponse::Ignored)
            );
            node!(
                "scroller",
                Stack::new()
                    .child(editor)
                    .pan_claim(PanClaim::vertical())
                    .touch_action(TouchAction::PAN_Y)
            );
        }
        Scenario::SceneMarquee => {
            let leaf = node!("leaf", Leaf::new());
            let t = taps.clone();
            let card = node!(
                "card",
                Stack::new()
                    .child(leaf)
                    .on_tap(move |_e, _c| t.set(t.get() + 1))
            );
            node!("container", Stack::new().child(card).on_drag(|_p, _c| {}));
        }
        Scenario::SceneMarqueeInScroller => {
            let leaf = node!("leaf", Leaf::new());
            let t = taps.clone();
            let card = node!(
                "card",
                Stack::new()
                    .child(leaf)
                    .on_tap(move |_e, _c| t.set(t.get() + 1))
            );
            let container = node!("container", Stack::new().child(card).on_drag(|_p, _c| {}));
            node!(
                "scroller",
                Stack::new()
                    .child(container)
                    .pan_claim(PanClaim::vertical())
            );
        }
        Scenario::SceneViewDualRole => {
            let leaf = node!("leaf", Leaf::new());
            let t = taps.clone();
            node!(
                "view",
                Stack::new()
                    .child(leaf)
                    .pan_claim(PanClaim::both())
                    .on_drag(|_p, _c| {})
                    .on_tap(move |_e, _c| t.set(t.get() + 1))
            );
        }
        Scenario::SceneViewDualRoleOverItem => {
            let leaf = node!("leaf", Leaf::new());
            let t = taps.clone();
            let item = node!(
                "item",
                Stack::new()
                    .child(leaf)
                    .on_tap(move |_e, _c| t.set(t.get() + 1))
            );
            node!(
                "view",
                Stack::new()
                    .child(item)
                    .pan_claim(PanClaim::both())
                    .on_drag(|_p, _c| {})
            );
        }
        Scenario::DragRegion => {
            node!(
                "region",
                Leaf::new().on_drag(|_p, _c| {}).on_double_tap(|_e, _c| {})
            );
        }
        Scenario::TabStrip => {
            let leaf = node!("leaf", Leaf::new());
            let t = taps.clone();
            let tab = node!(
                "tab",
                Stack::new()
                    .child(leaf)
                    .on_tap(move |_e, _c| t.set(t.get() + 1))
                    .on_drag(|_p, _c| {})
            );
            node!(
                "strip",
                Stack::new().child(tab).pan_claim(PanClaim::horizontal())
            );
        }
        Scenario::ColumnGrip | Scenario::ColumnGripNone | Scenario::ColumnGripManipulation => {
            let grip = Leaf::new()
                .on_double_tap(|_e, _c| {})
                .on_pointer_event(|event, ctx| {
                    if matches!(event, WidgetEvent::PointerDown { .. }) {
                        ctx.capture_pointer();
                        return EventResponse::Handled;
                    }
                    EventResponse::Ignored
                });
            let grip = match scenario {
                Scenario::ColumnGripNone => node!("grip", grip.touch_action(TouchAction::NONE)),
                Scenario::ColumnGripManipulation => {
                    node!("grip", grip.touch_action(TouchAction::MANIPULATION))
                }
                _ => node!("grip", grip),
            };
            let strip = node!(
                "strip",
                Stack::new().child(grip).pan_claim(PanClaim::horizontal())
            );
            node!("ancestor", Stack::new().child(strip).on_drag(|_p, _c| {}));
        }
        Scenario::TwoAxisScroller(action) => {
            let leaf = node!("leaf", Leaf::new());
            node!(
                "scroller",
                Stack::new()
                    .child(leaf)
                    .pan_claim(PanClaim::both())
                    .touch_action(action)
            );
        }
        Scenario::NestedPreviewers => {
            let leaf = node!("leaf", Leaf::new());
            let inner = node!(
                "inner",
                Stack::new().child(leaf).on_pointer_event(|event, _ctx| {
                    if matches!(event, WidgetEvent::PointerDown { .. }) {
                        return EventResponse::Handled;
                    }
                    EventResponse::Ignored
                })
            );
            node!(
                "outer",
                Stack::new().child(inner).on_pointer_event(|event, _ctx| {
                    if matches!(event, WidgetEvent::PointerDown { .. }) {
                        return EventResponse::Handled;
                    }
                    EventResponse::Ignored
                })
            );
        }
        Scenario::Button => {
            let t = taps.clone();
            node!(
                "button",
                Leaf::new().on_tap(move |_e, _c| t.set(t.get() + 1))
            );
        }
        Scenario::ButtonInScroller => {
            let t = taps.clone();
            let button = node!(
                "button",
                Leaf::new().on_tap(move |_e, _c| t.set(t.get() + 1))
            );
            node!(
                "scroller",
                Stack::new().child(button).pan_claim(PanClaim::vertical())
            );
        }
    }

    tree.layout(SizeProposal::exact(400.0, 400.0));
    Fixture {
        tree,
        names,
        cancels,
        press: Point::new(200.0, 200.0),
        taps,
    }
}

// ---------------------------------------------------------------------------
// Driving a fixture with one device
// ---------------------------------------------------------------------------

impl Fixture {
    fn press_with(&mut self, kind: PointerKind) -> PointerId {
        let at = self.press;
        match kind {
            PointerKind::Touch => {
                let id = self.tree.new_contact();
                self.tree.touch_down(id, at);
                id
            }
            PointerKind::Pen(_) => {
                self.tree.pen_down(at, 0.5, (0.0, 0.0));
                self.tree
                    .live_pointers()
                    .find(|p| matches!(p.kind, PointerKind::Pen(_)))
                    .map(|p| p.id)
                    .expect("the pen was admitted")
            }
            _ => {
                self.tree.pointer_down_button(at, PointerButton::Primary);
                PointerId::MOUSE
            }
        }
    }

    fn move_with(&mut self, kind: PointerKind, pointer: PointerId, to: Point) {
        match kind {
            PointerKind::Touch => self.tree.touch_move(pointer, to),
            PointerKind::Pen(_) => self.tree.pen_move(to, 0.5, (0.0, 0.0)),
            _ => self.tree.pointer_move(to),
        }
    }

    fn release_with(&mut self, kind: PointerKind, pointer: PointerId, at: Point) {
        match kind {
            PointerKind::Touch => self.tree.touch_up(pointer, at),
            PointerKind::Pen(_) => self.tree.pen_up(at, 0.0, (0.0, 0.0)),
            _ => self.tree.pointer_up_button(at, PointerButton::Primary),
        }
    }
}

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

const PEN: PointerKind = PointerKind::Pen(PenKind::Pen);

fn rows() -> Vec<Row> {
    vec![
        // -------------------------------------------------------------
        // A5 scenario 1 — a reorderable row in a vertical list
        // -------------------------------------------------------------
        Row {
            name: "list row · mouse",
            scenario: Scenario::ListRow,
            kind: PointerKind::Mouse,
            frozen: TouchAction::AUTO,
            members: &[("row", Role::Gesture, MemberState::Possible)],
            press_at: None,
            movement: &[(0.0, 4.0, None), (0.0, 5.0, Some("row"))],
            cancels: &[],
            note: "a mouse never enrols a pan claimant, so the row's own drag \
                   is the only competitor and it latches at 5 dp. Three \
                   redundant gates enforce the no-enrolment half — \
                   `begin_sequence`'s is_direct guard, PanClaim::devices' \
                   DIRECT default and the mouse profile's absent pan_slop — \
                   and no single deletion reddens it: removing the is_direct \
                   guard moves nothing in teksilo-core at all, and removing \
                   the DIRECT default moves only the pen rows. It covers the \
                   rule, not any one implementation of it; see the header for \
                   which of the three have been mutated",
        },
        Row {
            name: "list row · touch",
            scenario: Scenario::ListRow,
            kind: PointerKind::Touch,
            frozen: TouchAction::AUTO,
            members: &[
                ("row", Role::Gesture, MemberState::Possible),
                ("scroller", Role::Pan, MemberState::Possible),
            ],
            press_at: None,
            movement: &[(0.0, 17.0, None), (0.0, 19.0, Some("row"))],
            cancels: &[("scroller", CancelReason::PeerClaimed)],
            note: "the reorder wins at drag_slop (18) before the scroller's \
                   pan_slop (36) is reached, and the claimant is told",
        },
        // -------------------------------------------------------------
        // A5 scenario 2 — a slider thumb
        // -------------------------------------------------------------
        Row {
            name: "slider thumb · touch",
            scenario: Scenario::Slider,
            kind: PointerKind::Touch,
            frozen: TouchAction::NONE,
            members: &[("slider", Role::Gesture, MemberState::Possible)],
            press_at: None,
            movement: &[(0.0, 17.0, None), (0.0, 19.0, Some("slider"))],
            cancels: &[],
            note: "TouchAction::NONE keeps the enclosing claimant out of the \
                   member list entirely — there is no loser to cancel. Two \
                   independent gates enforce it (`pan_candidates`' axis filter \
                   and `PointerSequence::pan_is_eligible`'s), so this row goes \
                   red only when *both* are removed: it covers the rule, not \
                   either implementation of it",
        },
        // -------------------------------------------------------------
        // A5 scenario 3 — text selection inside a PAN_Y scroller
        // -------------------------------------------------------------
        Row {
            name: "text selection · mouse",
            scenario: Scenario::TextSelection,
            kind: PointerKind::Mouse,
            frozen: TouchAction::PAN_Y,
            members: &[],
            press_at: None,
            movement: &[(0.0, 20.0, None), (0.0, 60.0, None)],
            cancels: &[],
            note: "a mouse selection is owned by the capture, not by the \
                   arbitration: no competitor is ever enrolled. Redundant \
                   gates enforce that — `begin_sequence`'s is_direct guard, \
                   PanClaim::devices' DIRECT default and the mouse profile's \
                   absent pan_slop — so the row covers the *rule*. The row on \
                   which a device set and a profile's pan_slop are each singly \
                   observable is `text selection · pen`, on a pointer those \
                   two gates admit",
        },
        Row {
            name: "text selection · touch",
            scenario: Scenario::TextSelection,
            kind: PointerKind::Touch,
            frozen: TouchAction::PAN_Y,
            members: &[("scroller", Role::Pan, MemberState::Possible)],
            press_at: None,
            movement: &[(0.0, 20.0, None), (0.0, 40.0, Some("scroller"))],
            cancels: &[],
            note: "a frozen PAN_Y admits the claim it agrees with: the pan \
                   wins at 36 dp and the editor — the captor, but not a member \
                   — is never cancelled",
        },
        Row {
            name: "text selection · pen",
            scenario: Scenario::TextSelection,
            kind: PEN,
            frozen: TouchAction::PAN_Y,
            members: &[("scroller", Role::Pan, MemberState::Possible)],
            press_at: None,
            movement: &[(0.0, 6.0, None), (0.0, 9.0, Some("scroller"))],
            cancels: &[],
            note: "what this row alone asserts is the **enrolment** — that a \
                   pen puts the claimant in the member list at all, which \
                   PanClaim::devices' DIRECT default decides — and the \
                   threshold it then pans at, bracketed by the two steps: past \
                   6 dp and by 9, i.e. GestureProfile::PEN's own pan_slop and \
                   not a finger's 36. Either gate reddens it, identically — it \
                   says a gate moved, not which — and it is not the sole \
                   witness to either: the `column grip · pen` row and \
                   `a_pen_loses_an_explicit_capture_to_a_drag_capable_ancestor` \
                   read the same two through the DragActivation they resolve \
                   on the capture-versus-ancestor path, and neither asserts \
                   this row's threshold",
        },
        // -------------------------------------------------------------
        // A5 scenario 4 — a SceneView marquee
        // -------------------------------------------------------------
        Row {
            name: "scene marquee · mouse",
            scenario: Scenario::SceneMarquee,
            kind: PointerKind::Mouse,
            frozen: TouchAction::AUTO,
            members: &[("container", Role::Gesture, MemberState::Possible)],
            press_at: None,
            movement: &[(4.0, 0.0, None), (5.0, 0.0, Some("container"))],
            cancels: &[],
            note: "the marquee reaches through the tapped card's capture and \
                   latches at 5 dp; the card is not a member, so it is not \
                   cancelled. Nothing above it claims a pan, so — unlike the \
                   other mouse rows — the empty pan half is the fixture's \
                   doing, not the mouse's",
        },
        Row {
            name: "scene marquee · touch",
            scenario: Scenario::SceneMarquee,
            kind: PointerKind::Touch,
            frozen: TouchAction::AUTO,
            members: &[("container", Role::Gesture, MemberState::Possible)],
            press_at: None,
            movement: &[(17.0, 0.0, None), (19.0, 0.0, Some("container"))],
            cancels: &[],
            note: "with no pan claimant above it, DragActivation::Auto resolves \
                   to Immediate and the marquee latches at drag_slop",
        },
        Row {
            name: "scene marquee in a scroller · touch",
            scenario: Scenario::SceneMarqueeInScroller,
            kind: PointerKind::Touch,
            frozen: TouchAction::AUTO,
            members: &[
                ("container", Role::Gesture, MemberState::Possible),
                ("scroller", Role::Pan, MemberState::Possible),
            ],
            press_at: None,
            movement: &[(0.0, 20.0, None), (0.0, 37.0, Some("scroller"))],
            cancels: &[("container", CancelReason::PeerClaimed)],
            note: "an eligible pan defers the marquee to AfterLongPress, so it \
                   cannot win at 19 dp the way the unscrolled marquee does; the \
                   pan takes the press at 36 and cancels it",
        },
        Row {
            name: "scene marquee in a scroller, press at the edge · touch",
            scenario: Scenario::SceneMarqueeInScroller,
            kind: PointerKind::Touch,
            frozen: TouchAction::AUTO,
            members: &[
                ("container", Role::Gesture, MemberState::Possible),
                ("scroller", Role::Pan, MemberState::Possible),
            ],
            press_at: Some((200.0, 390.0)),
            movement: &[(0.0, 20.0, None), (0.0, 37.0, Some("scroller"))],
            cancels: &[],
            note: "a coarse pointer's tap boundary is the member's own bounds, \
                   not a slop radius: 20 dp off a press near the edge leaves \
                   them, the deferred marquee withdraws itself, and a member \
                   that withdrew is never cancelled",
        },
        // -------------------------------------------------------------
        // The shipped SceneView: one node, a claim AND its own drag
        // -------------------------------------------------------------
        Row {
            name: "scene view with its own claim · mouse",
            scenario: Scenario::SceneViewDualRole,
            kind: PointerKind::Mouse,
            frozen: TouchAction::AUTO,
            members: &[("view", Role::Gesture, MemberState::Possible)],
            press_at: None,
            movement: &[(0.0, 4.0, None), (0.0, 6.0, Some("view"))],
            cancels: &[],
            note: "the mouse cannot reach the dual-role arm at all: it enrols \
                   no pan member, so the node's own drag takes the slot by the \
                   ordinary `enrol` and latches at 5 dp. `defer_own_drag` \
                   refuses anything but a live Pan member, which is what makes \
                   that a structural guarantee rather than an observation",
        },
        Row {
            name: "scene view with its own claim · touch",
            scenario: Scenario::SceneViewDualRole,
            kind: PointerKind::Touch,
            frozen: TouchAction::AUTO,
            members: &[("view", Role::Pan, MemberState::Possible)],
            press_at: None,
            movement: &[(0.0, 20.0, None), (0.0, 37.0, Some("view"))],
            cancels: &[],
            note: "one node, one member — enrolled as the Pan claim \
                   `begin_sequence` put there before any handler ran. Its own \
                   drag gets a say through `defer_own_drag`, which resolves \
                   Auto against that claim to AfterLongPress: so 20 dp latches \
                   nothing and the pan takes the press at 36. Before that arm \
                   existed the drag latched at 18 and this surface could not \
                   pan under a finger at all",
        },
        Row {
            name: "scene view with its own claim · pen",
            scenario: Scenario::SceneViewDualRole,
            kind: PEN,
            frozen: TouchAction::AUTO,
            members: &[("view", Role::Pan, MemberState::Possible)],
            press_at: None,
            movement: &[(0.0, 4.0, None), (0.0, 9.0, Some("view"))],
            cancels: &[],
            note: "a pen is precise but *direct*, so `resolve_activation` defers \
                   its drag too and it pans at PEN's own pan_slop (8) rather \
                   than marqueeing at its drag_slop (2). Consistent with `column \
                   grip · pen`, and the lever for a surface that wants \
                   otherwise is `EventContext::set_drag_activation` per press, \
                   not a pointer-kind clause in the resolution",
        },
        Row {
            name: "scene view with its own claim, over a widget item · touch",
            scenario: Scenario::SceneViewDualRoleOverItem,
            kind: PointerKind::Touch,
            frozen: TouchAction::AUTO,
            members: &[("view", Role::Pan, MemberState::Possible)],
            press_at: None,
            movement: &[(0.0, 20.0, None), (0.0, 37.0, Some("view"))],
            cancels: &[],
            note: "the second door to the same refusal: the captor is the item, \
                   so the view is reached by the ancestor walk, whose \
                   `enrol_drag` opens with the same `enrol`. Same answer — the \
                   pan at 36 — but reached through the branch a heavyweight \
                   scene item's press goes through",
        },
        Row {
            name: "scene view with its own claim, over a widget item · mouse",
            scenario: Scenario::SceneViewDualRoleOverItem,
            kind: PointerKind::Mouse,
            frozen: TouchAction::AUTO,
            members: &[("view", Role::Gesture, MemberState::Possible)],
            press_at: None,
            movement: &[(0.0, 4.0, None), (0.0, 6.0, Some("view"))],
            cancels: &[],
            note: "and the mouse is unreachable at that door too: with no pan \
                   member the ancestor walk's `enrol_drag` succeeds, so the \
                   view is an ordinary Gesture ancestor latching at 5 dp — the \
                   `scene marquee` rows' behaviour, on a node that also \
                   declares a claim",
        },
        // -------------------------------------------------------------
        // A5 scenario 5 — a DragRegion window move
        // -------------------------------------------------------------
        Row {
            name: "drag region · mouse",
            scenario: Scenario::DragRegion,
            kind: PointerKind::Mouse,
            frozen: TouchAction::AUTO,
            members: &[("region", Role::Gesture, MemberState::Possible)],
            press_at: None,
            movement: &[(4.0, 0.0, None), (5.0, 0.0, Some("region"))],
            cancels: &[],
            note: "the window move starts from DragPhase::Started, never from \
                   the press: an implicit arena capture decides nothing",
        },
        Row {
            name: "drag region · touch",
            scenario: Scenario::DragRegion,
            kind: PointerKind::Touch,
            frozen: TouchAction::AUTO,
            members: &[("region", Role::Gesture, MemberState::Possible)],
            press_at: None,
            movement: &[(17.0, 0.0, None), (19.0, 0.0, Some("region"))],
            cancels: &[],
            note: "same rule for a finger, at the finger's slop",
        },
        // -------------------------------------------------------------
        // A5 scenario 6 — a TabBar strip against a tab drag
        // -------------------------------------------------------------
        Row {
            name: "tab strip vs tab drag · mouse",
            scenario: Scenario::TabStrip,
            kind: PointerKind::Mouse,
            frozen: TouchAction::AUTO,
            members: &[("tab", Role::Gesture, MemberState::Possible)],
            press_at: None,
            movement: &[(4.0, 0.0, None), (5.0, 0.0, Some("tab"))],
            cancels: &[],
            note: "the horizontal twin of the list row: the tab's own drag is \
                   the only mouse competitor, and the same three redundant \
                   gates keep the strip's claim out — the rule, not one \
                   implementation of it",
        },
        Row {
            name: "tab strip vs tab drag · touch",
            scenario: Scenario::TabStrip,
            kind: PointerKind::Touch,
            frozen: TouchAction::AUTO,
            members: &[
                ("tab", Role::Gesture, MemberState::Possible),
                ("strip", Role::Pan, MemberState::Possible),
            ],
            press_at: None,
            movement: &[(17.0, 0.0, None), (19.0, 0.0, Some("tab"))],
            cancels: &[("strip", CancelReason::PeerClaimed)],
            note: "dragging a tab beats scrolling the strip on the same axis, \
                   for the same reason a reorder beats a list scroll",
        },
        // -------------------------------------------------------------
        // A5 scenario 7 — a TableView column grip
        // -------------------------------------------------------------
        Row {
            name: "column grip · mouse",
            scenario: Scenario::ColumnGrip,
            kind: PointerKind::Mouse,
            frozen: TouchAction::AUTO,
            members: &[
                ("grip", Role::RawDrag, MemberState::Won),
                ("ancestor", Role::Gesture, MemberState::Rejected),
            ],
            press_at: None,
            movement: &[(4.0, 0.0, Some("grip")), (40.0, 0.0, Some("grip"))],
            cancels: &[],
            note: "an explicit capture by an *indirect* pointer decides at the \
                   press; the ancestor is enrolled already-rejected and so is \
                   never cancelled. The strip's claim is absent for the same \
                   three redundant gates the other mouse rows name",
        },
        Row {
            name: "column grip · touch",
            scenario: Scenario::ColumnGrip,
            kind: PointerKind::Touch,
            frozen: TouchAction::AUTO,
            members: &[
                ("grip", Role::RawDrag, MemberState::Possible),
                ("strip", Role::Pan, MemberState::Possible),
                ("ancestor", Role::Gesture, MemberState::Possible),
            ],
            press_at: None,
            movement: &[(17.0, 0.0, None), (19.0, 0.0, Some("grip"))],
            cancels: &[
                ("strip", CancelReason::PeerClaimed),
                ("ancestor", CancelReason::PeerClaimed),
            ],
            note: "a contact's explicit capture does not decide at the press: \
                   the grip latches at latch_slop, which under AUTO is 18, and \
                   both live competitors are told",
        },
        Row {
            name: "column grip declaring NONE · touch",
            scenario: Scenario::ColumnGripNone,
            kind: PointerKind::Touch,
            frozen: TouchAction::NONE,
            members: &[
                ("grip", Role::RawDrag, MemberState::Possible),
                ("ancestor", Role::Gesture, MemberState::Possible),
            ],
            press_at: None,
            movement: &[(1.0, 0.0, None), (3.0, 0.0, Some("grip"))],
            cancels: &[("ancestor", CancelReason::PeerClaimed)],
            note: "under a frozen NONE a direct pointer's RawDrag drops to the \
                   jitter floor (2 dp) — the only production reader of \
                   PointerSequence::latch_slop's precise branch",
        },
        Row {
            name: "column grip · pen",
            scenario: Scenario::ColumnGrip,
            kind: PEN,
            frozen: TouchAction::AUTO,
            members: &[
                ("grip", Role::RawDrag, MemberState::Possible),
                ("strip", Role::Pan, MemberState::Possible),
                ("ancestor", Role::Gesture, MemberState::Possible),
            ],
            press_at: None,
            movement: &[(1.0, 0.0, None), (3.0, 0.0, Some("grip"))],
            cancels: &[("strip", CancelReason::PeerClaimed)],
            note: "a pen is precise but *direct*, so the press-time decision \
                   does not fire and the grip latches on travel instead; the \
                   eligible pan defers the ancestor, which both leaves the grip \
                   room to latch and withdraws the ancestor — unannounced — as \
                   the press leaves its 2 dp tap boundary",
        },
        Row {
            name: "column grip declaring NONE · pen",
            scenario: Scenario::ColumnGripNone,
            kind: PEN,
            frozen: TouchAction::NONE,
            members: &[
                ("grip", Role::RawDrag, MemberState::Possible),
                ("ancestor", Role::Gesture, MemberState::Possible),
            ],
            press_at: None,
            movement: &[(1.0, 0.0, None), (3.0, 0.0, Some("ancestor"))],
            cancels: &[("grip", CancelReason::PeerClaimed)],
            note: "**recorded defect, not a rule.** NONE removes the pan \
                   competitor and so re-resolves the ancestor from \
                   AfterLongPress to Immediate; the two slops that then \
                   compete are the grip's latch_slop — slop_precise, 2 dp, \
                   under a frozen NONE — and the ancestor's DragRecognizer \
                   threshold, PEN.drag_slop, also 2 dp, so the tie goes to \
                   dispatch order and the ordinary bubble reaches the ancestor \
                   first. See \
                   `a_pen_loses_an_explicit_capture_to_a_drag_capable_ancestor`",
        },
        // -------------------------------------------------------------
        // The frozen action's own axis — the matrix's second axis, at the
        // values no A5 scenario reaches on its own
        // -------------------------------------------------------------
        Row {
            name: "column grip declaring MANIPULATION · touch",
            scenario: Scenario::ColumnGripManipulation,
            kind: PointerKind::Touch,
            frozen: TouchAction::MANIPULATION,
            members: &[
                ("grip", Role::RawDrag, MemberState::Possible),
                ("strip", Role::Pan, MemberState::Possible),
                ("ancestor", Role::Gesture, MemberState::Possible),
            ],
            press_at: None,
            movement: &[
                (1.0, 0.0, None),
                (3.0, 0.0, None),
                (19.0, 0.0, Some("grip")),
            ],
            cancels: &[
                ("strip", CancelReason::PeerClaimed),
                ("ancestor", CancelReason::PeerClaimed),
            ],
            note: "latch_slop's precise branch keys on NONE **exactly**, not \
                   on any action narrower than AUTO: MANIPULATION forbids \
                   nothing the pan arbitration reads, so the grip stays at 18 \
                   dp and 3 dp of travel decides nothing. Widen that condition \
                   to `!= AUTO` and this row — alone — goes red at step 1",
        },
        Row {
            name: "two-axis scroller under PAN_X · touch",
            scenario: Scenario::TwoAxisScroller(TouchAction::PAN_X),
            kind: PointerKind::Touch,
            frozen: TouchAction::PAN_X,
            members: &[],
            press_at: None,
            movement: &[(30.0, 0.0, None), (40.0, 0.0, None)],
            cancels: &[],
            note: "a claim is **excluded entirely, never narrowed**: a \
                   two-axis scroller inside a PAN_X region pans on neither \
                   axis, not on the permitted one. The only row that separates \
                   `pan_candidates`' axis filter from \
                   `PointerSequence::pan_is_eligible`'s — the latter asks \
                   whether *any* claimed axis survives and would admit this \
                   claim, so deleting the former alone reddens this row and no \
                   other",
        },
        Row {
            name: "two-axis scroller under PAN · touch",
            scenario: Scenario::TwoAxisScroller(TouchAction::PAN),
            kind: PointerKind::Touch,
            frozen: TouchAction::PAN,
            members: &[("scroller", Role::Pan, MemberState::Possible)],
            press_at: None,
            // The PAN_X row's travel exactly: a control that varied the axis as
            // well as the frozen action would not be a control.
            movement: &[(30.0, 0.0, None), (40.0, 0.0, Some("scroller"))],
            cancels: &[],
            note: "the control for the PAN_X row: the same claim, the same \
                   fixture, the same travel along the same axis — admitted, \
                   because PAN carries both pan bits, and the one thing that \
                   differs is the frozen action. Without it the row above \
                   would pass just as well if `pan_candidates` returned \
                   nothing at all",
        },
        // -------------------------------------------------------------
        // Nested previewers
        // -------------------------------------------------------------
        Row {
            name: "nested previewers · mouse",
            scenario: Scenario::NestedPreviewers,
            kind: PointerKind::Mouse,
            frozen: TouchAction::AUTO,
            members: &[("outer", Role::RawPreview, MemberState::Won)],
            press_at: None,
            movement: &[(40.0, 0.0, Some("outer"))],
            cancels: &[],
            note: "the raw-preview pass is root-first and the first Handled \
                   claims the press, so the inner previewer never runs",
        },
        Row {
            name: "nested previewers · touch",
            scenario: Scenario::NestedPreviewers,
            kind: PointerKind::Touch,
            frozen: TouchAction::AUTO,
            members: &[("outer", Role::RawPreview, MemberState::Won)],
            press_at: None,
            movement: &[(40.0, 0.0, Some("outer"))],
            cancels: &[],
            note: "a preview claim is unconditional: unlike an explicit capture \
                   it decides for a coarse pointer too",
        },
    ]
}

// ---------------------------------------------------------------------------
// Running a row
// ---------------------------------------------------------------------------

/// Run one row and return every way it failed to hold, rather than stopping at
/// the first.
///
/// The matrix exists to say **which** rows a change moves, and a row that
/// asserts eagerly can only ever answer that for itself: the run ends there and
/// every row after it goes unevaluated, so a note claiming "this reddens that
/// row and no other" cannot be checked by running the test. Each check
/// therefore records a sentence and the row carries on;
/// [`every_matrix_row_holds`] reports the whole blast radius in one run.
///
/// The movement loop keeps going past a mismatched step for the same reason: a
/// row that latches one step early and one step late is two different defects,
/// and reporting only the first hides which.
fn run(row: &Row) -> Vec<String> {
    let mut failures: Vec<String> = Vec::new();

    /// Compare, and on a mismatch record what was expected against what
    /// happened — the same pair `assert_eq!` would have printed.
    macro_rules! check {
        ($actual:expr, $expected:expr, $($what:tt)+) => {{
            let actual = $actual;
            let expected = $expected;
            if actual != expected {
                failures.push(format!(
                    "{}\n      expected: {expected:?}\n      actual:   {actual:?}",
                    format_args!($($what)+),
                ));
            }
        }};
    }

    let mut fx = build(row.scenario);
    if let Some((x, y)) = row.press_at {
        fx.press = Point::new(x, y);
    }
    let pointer = fx.press_with(row.kind);

    check!(
        fx.tree.sequence_touch_action(pointer),
        row.frozen,
        "frozen TouchAction"
    );
    check!(
        fx.members(pointer),
        row.members.to_vec(),
        "members at the press, innermost first"
    );

    let origin = fx.press;
    let mut last = origin;
    for (step, &(dx, dy, expected)) in row.movement.iter().enumerate() {
        last = Point::new(origin.x + dx, origin.y + dy);
        fx.move_with(row.kind, pointer, last);
        check!(
            fx.winner(pointer),
            expected,
            "winner after step {step} ({dx:+}, {dy:+})"
        );
    }

    check!(
        fx.cancel_log(),
        row.cancels.to_vec(),
        "cancels delivered over the press"
    );

    fx.release_with(row.kind, pointer, last);
    fx.tree.assert_no_leaked_pointer_state();

    failures
}

/// What a caught panic said, so a row that panics reads like a row that failed
/// a check rather than like a lost run.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else {
        "<non-string panic payload>".to_string()
    }
}

#[test]
fn every_matrix_row_holds() {
    let mut broken: Vec<(&'static str, Vec<String>)> = Vec::new();

    for row in rows() {
        let name = row.name;
        // A row can still panic rather than fail a check: `press_with` expects
        // a pen it admitted, `Role::of` refuses a `MemberRole` it has no tag
        // for, and `assert_no_leaked_pointer_state` is the framework's own
        // assertion. Catching it attributes the panic to its row and leaves
        // the rows after it to run, which is the whole point of collecting —
        // the alternative reports one row and hides the rest.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&row))) {
            Ok(failures) if failures.is_empty() => {}
            Ok(failures) => broken.push((name, failures)),
            Err(payload) => broken.push((
                name,
                vec![format!("panicked: {}", panic_message(&*payload))],
            )),
        }
    }

    if broken.is_empty() {
        return;
    }

    // The roster first, on one line per row: a reader checking a note that says
    // "this row and no other" needs the list of names before the detail.
    let mut report = format!(
        "{} of {} matrix rows did not hold:\n",
        broken.len(),
        rows().len()
    );
    for (name, _) in &broken {
        report.push_str(&format!("  · {name}\n"));
    }
    report.push('\n');
    for (name, failures) in &broken {
        report.push_str(&format!("[{name}]\n"));
        for failure in failures {
            report.push_str(&format!("    {failure}\n"));
        }
    }
    panic!("{report}");
}

// ---------------------------------------------------------------------------
// The mouse column's anchor: the latch is 5.0 in every configuration
// ---------------------------------------------------------------------------

/// A mouse drag latches at exactly 5.0 dp whatever the density and whatever the
/// frozen `TouchAction`, and an explicit capture by a mouse decides at the
/// press in the same grid.
///
/// The `Gesture` half restates the flagship
/// `a_mouse_drag_latches_at_five_in_every_configuration`
/// (`widget_tree/gesture_dispatch_impl.rs`) deliberately: it is the matrix's
/// mouse anchor, and this file is where a reader looks for the whole mouse
/// column. The `RawDrag` half is **not** covered there and is the reason the
/// grid is re-run rather than merely cited: `PointerSequence::latch_slop` is
/// unreachable for a mouse, because `note_explicit_capture` decides an indirect
/// pointer's sequence at the press before any travel is measured, and nothing
/// asserted that across the grid.
#[test]
fn mouse_latch_is_five_in_every_configuration() {
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        for action in [
            TouchAction::AUTO,
            TouchAction::NONE,
            TouchAction::PAN,
            TouchAction::PAN_X,
            TouchAction::PAN_Y,
            TouchAction::MANIPULATION,
        ] {
            // A `Gesture` member: the drag lives on an ancestor of the node the
            // press captured, so the latch is `DragRecognizer`'s.
            let mut tree = WidgetTree::new();
            tree.set_density(density);
            let leaf = tree.add(Leaf::new().on_tap(|_e, _c| {}));
            let container = tree.add(
                Stack::new()
                    .child(leaf)
                    .touch_action(action)
                    .on_drag(|_p, _c| {}),
            );
            tree.layout(SizeProposal::exact(200.0, 200.0));

            tree.pointer_down_button(Point::new(100.0, 100.0), PointerButton::Primary);
            let mut latched_at = None;
            for step in 1..=10 {
                tree.pointer_move(Point::new(100.0 + step as f32, 100.0));
                if tree.sequence_winner(PointerId::MOUSE).is_some() {
                    latched_at = Some(step as f32);
                    break;
                }
            }
            assert_eq!(
                latched_at,
                Some(5.0),
                "a mouse Gesture drag under {action:?} at {density:?}"
            );
            assert_eq!(tree.sequence_winner(PointerId::MOUSE), Some(container));
            tree.pointer_up_button(Point::new(110.0, 100.0), PointerButton::Primary);
            tree.assert_no_leaked_pointer_state();

            // A `RawDrag` member: an explicit capture. A mouse has no eligible
            // pan competitor in any configuration, so this decides at the press
            // — travel 0.0 — and `latch_slop` is never consulted.
            let mut tree = WidgetTree::new();
            tree.set_density(density);
            let grip = tree.add(
                Leaf::new()
                    .on_double_tap(|_e, _c| {})
                    .touch_action(action)
                    .on_pointer_event(|event, ctx| {
                        if matches!(event, WidgetEvent::PointerDown { .. }) {
                            ctx.capture_pointer();
                            return EventResponse::Handled;
                        }
                        EventResponse::Ignored
                    }),
            );
            let ancestor = tree.add(Stack::new().child(grip).on_drag(|_p, _c| {}));
            tree.layout(SizeProposal::exact(200.0, 200.0));

            tree.pointer_down_button(Point::new(100.0, 100.0), PointerButton::Primary);
            assert_eq!(
                tree.sequence_winner(PointerId::MOUSE),
                Some(grip),
                "a mouse RawDrag under {action:?} at {density:?} decides at the press"
            );
            for step in 1..=10 {
                tree.pointer_move(Point::new(100.0 + step as f32 * 4.0, 100.0));
            }
            assert_eq!(
                tree.sequence_winner(PointerId::MOUSE),
                Some(grip),
                "and no travel hands it to {ancestor:?}"
            );
            tree.pointer_up_button(Point::new(140.0, 100.0), PointerButton::Primary);
            tree.assert_no_leaked_pointer_state();
        }
    }
}

// ---------------------------------------------------------------------------
// Two fingers on one Button
// ---------------------------------------------------------------------------

/// Two contacts land on one tappable control. `MultiContact::First` — the
/// default — serves the first and refuses the second, so exactly one tap
/// fires however the two are interleaved, and the first contact keeps the
/// press for the whole time both are down.
///
/// Each contact still gets its *own* sequence: capture, arbitration and press
/// ownership are per pointer, and a second finger arriving must not be able to
/// rewrite what the first one decided.
#[test]
fn two_fingers_on_one_button_fire_one_tap() {
    let mut fx = build(Scenario::Button);
    let button = fx.id("button");
    let at = fx.press;

    let a = fx.tree.new_contact();
    let b = fx.tree.new_contact();
    fx.tree.touch_down(a, at);
    assert_eq!(
        fx.tree.pressed_by(button),
        Some(a),
        "the first finger holds it"
    );

    fx.tree.touch_down(b, Point::new(at.x + 4.0, at.y));
    assert_eq!(
        fx.tree.pressed_by(button),
        Some(a),
        "the second contact does not take the press over"
    );
    assert!(
        fx.tree.sequence_members(b).is_empty(),
        "and enrols no competitor of its own: {:?}",
        fx.tree.sequence_members(b)
    );

    // The refused contact lifts first: it must not complete a tap, and it must
    // not clear the press the first finger still holds.
    fx.tree.touch_up(b, Point::new(at.x + 4.0, at.y));
    assert_eq!(fx.taps.get(), 0, "the refused contact taps nothing");
    assert_eq!(
        fx.tree.pressed_by(button),
        Some(a),
        "and lifting it leaves the first finger's press alone"
    );

    fx.tree.touch_up(a, at);
    assert_eq!(fx.taps.get(), 1, "exactly one tap for two fingers");
    assert!(fx.cancel_log().is_empty(), "and nobody was cancelled");
    fx.tree.assert_no_leaked_pointer_state();
}

/// A second finger on a button **inside** a scroller drags the scroller.
///
/// Recorded, not designed: `MultiContact::First` refuses the second contact at
/// the button's *arena* (`gesture/arena_set.rs`), which is all it governs. The
/// pan session, the pointer sequence and the capture are per pointer, so the
/// second contact enrols the enclosing claimant in a sequence of its own and
/// wins it at `pan_slop` — while the first finger goes on holding the button.
///
/// That is what the platforms do (a second finger dragging inside a scroll view
/// scrolls it), and it is *not* what a reading of `MultiContact::First` as
/// "the second contact is ignored" would predict. The refusal is arena-scoped;
/// see `docs/events-and-gestures.md` §4.1.5.
#[test]
fn a_second_finger_on_a_button_in_a_scroller_pans_the_scroller() {
    let mut fx = build(Scenario::ButtonInScroller);
    let button = fx.id("button");
    let scroller = fx.id("scroller");
    let at = fx.press;

    let a = fx.tree.new_contact();
    fx.tree.touch_down(a, at);
    assert_eq!(fx.tree.pressed_by(button), Some(a));

    let b = fx.tree.new_contact();
    let b_at = Point::new(at.x + 4.0, at.y);
    fx.tree.touch_down(b, b_at);
    assert_eq!(
        fx.members(b),
        vec![("scroller", Role::Pan, MemberState::Possible)],
        "the second contact's own sequence carries the claimant"
    );

    fx.tree.touch_move(b, Point::new(b_at.x, b_at.y + 20.0));
    assert_eq!(fx.tree.sequence_winner(b), None, "20 dp is below pan_slop");
    fx.tree.touch_move(b, Point::new(b_at.x, b_at.y + 40.0));
    assert_eq!(
        fx.tree.sequence_winner(b),
        Some(scroller),
        "and 40 dp hands the second contact to the scroller"
    );
    assert_eq!(
        fx.tree.pressed_by(button),
        Some(a),
        "which leaves the first finger's press untouched"
    );
    assert_eq!(fx.taps.get(), 0);

    fx.tree.touch_up(b, Point::new(b_at.x, b_at.y + 40.0));
    fx.tree.touch_up(a, at);
    fx.tree.assert_no_leaked_pointer_state();
}

/// `AfterLongPress` is a **timer**, not a permanent withdrawal.
///
/// The matrix rows can only show the deferred marquee losing: they move the
/// pointer, and a deferral is lifted by time. This holds the press still,
/// advances the one virtual clock past the profile's `long_press`, and then
/// travels — at which point the marquee wins where it could not have won a
/// moment earlier, and the pan claimant is cancelled instead of winning.
///
/// It is also the arbitration's end of the P14 clock work: the hold ripens
/// because `advance_input_time` moved the input timeline, never because the
/// test took long enough.
#[test]
fn a_deferred_marquee_wins_once_its_long_press_has_ripened() {
    let mut fx = build(Scenario::SceneMarqueeInScroller);
    let container = fx.id("container");
    let at = fx.press;

    let finger = fx.tree.new_contact();
    fx.tree.touch_down(finger, at);

    // Before the hold: travel that would have latched an Immediate drag does
    // not latch a deferred one.
    fx.tree.touch_move(finger, Point::new(at.x, at.y + 19.0));
    assert_eq!(
        fx.tree.sequence_winner(finger),
        None,
        "19 dp is past drag_slop and the deferral still holds it out"
    );

    let hold = teksilo_tokens::GestureProfile::TOUCH.long_press;
    fx.tree.advance_input_time(hold);

    fx.tree.touch_move(finger, Point::new(at.x, at.y + 21.0));
    assert_eq!(
        fx.tree.sequence_winner(finger),
        Some(container),
        "past the hold the same member latches on the very next sample"
    );
    assert_eq!(
        fx.cancel_log(),
        vec![("scroller", CancelReason::PeerClaimed)],
        "and the claimant it beat is told"
    );

    fx.tree.touch_up(finger, Point::new(at.x, at.y + 21.0));
    fx.tree.assert_no_leaked_pointer_state();
}

// ---------------------------------------------------------------------------
// A hold silences its peers — and the silence is not a queue
// ---------------------------------------------------------------------------

/// A holder under a peer that both competes for the press (`on_drag` is what
/// enrols it) and carries a long-press recognizer.
///
/// `max_hold` is raised past the profile's `long_press` so the hold is still
/// standing when the peer's timer comes due: with the shipped 250 ms hold
/// against a 500 ms long press the hold always expires first, the two windows
/// never overlap, and there is nothing to observe. 700 ms then leaves room to
/// watch the release as well.
fn a_holder_under_a_long_pressing_peer(
    hold_on_press: bool,
) -> (WidgetTree, Rc<Cell<bool>>, WidgetId) {
    let fired = Rc::new(Cell::new(false));
    let flag = fired.clone();

    let mut tree = WidgetTree::new();
    let mut theme = tree.theme().clone();
    theme.input.gestures.mouse.max_hold = Duration::from_millis(700);
    tree.set_theme(theme);

    let child = tree.add(Leaf::new().on_pointer_event(move |event, ctx| {
        if hold_on_press && matches!(event, WidgetEvent::PointerDown { .. }) {
            ctx.hold_gesture();
        }
        EventResponse::Ignored
    }));
    let peer = tree.add(
        Stack::new()
            .child(child)
            .on_drag(|_p, _c| {})
            .on_long_press(move |_e, _c| flag.set(true)),
    );
    tree.layout(SizeProposal::exact(200.0, 200.0));
    (tree, fired, peer)
}

/// `ctx.hold_gesture()` is a **silence, not a queue**: a peer whose long press
/// ripens inside the hold loses that gesture, and does not get it back when the
/// hold releases.
///
/// The handler-side table in `docs/events-and-gestures.md` states the promise —
/// no peer may win while a member holds — for both doors the arbitration has,
/// and the timer door honours it one step later than the sample door: a tick is
/// addressed to a node, not to a member, so the node's recognizers all advance
/// and it is the *dispatch* that is gated. The gesture has been produced by
/// then — the tick advances the node's arena before it consults the
/// arbitration — and nothing re-delivers it, so the release finds nothing left
/// to hand over. That is
/// the consequence the promise does not state on its own, which is why it is
/// asserted here rather than described there.
///
/// Which assertion carries what: the middle one — nothing fired *during* the
/// hold — is the mechanism, and goes red when the tick path stops consulting
/// the arbitration. The last one is a **characterization**: nothing implements
/// deferral, so there is no mechanism to delete under it, and it stands as the
/// assertion that would catch queueing being added. It is not vacuous — the
/// control fires at the same instant on the same fixture, so there really was
/// a gesture to lose.
#[test]
fn a_long_press_a_hold_silenced_is_lost_rather_than_deferred() {
    let long_press = teksilo_tokens::GestureProfile::MOUSE.long_press;
    let at = Point::new(100.0, 100.0);
    let state_of = |tree: &WidgetTree, who: WidgetId| {
        tree.sequence_members(PointerId::MOUSE)
            .into_iter()
            .find(|(id, ..)| *id == who)
            .map(|(_, _, state)| state)
    };

    // The control: with nothing holding, this fixture's peer does long-press on
    // the tick. Without it the assertions below would pass on a fixture that
    // could never have fired at all.
    let (mut tree, fired, _peer) = a_holder_under_a_long_pressing_peer(false);
    tree.pointer_down_button(at, PointerButton::Primary);
    tree.advance_time(long_press + Duration::from_millis(100));
    assert!(
        fired.get(),
        "the peer is an enrolled member with a long-press recognizer and its \
         timer came due"
    );
    tree.pointer_up_button(at, PointerButton::Primary);
    tree.assert_no_leaked_pointer_state();

    // The same press, held.
    let (mut tree, fired, peer) = a_holder_under_a_long_pressing_peer(true);
    tree.pointer_down_button(at, PointerButton::Primary);
    assert!(
        tree.sequence_members(PointerId::MOUSE)
            .iter()
            .any(|(_, _, state)| *state == MemberState::Held),
        "the fixture must actually be holding"
    );

    tree.advance_time(long_press + Duration::from_millis(100));
    assert!(
        !fired.get(),
        "no peer may win while a member holds — the timer path is not a way \
         around the arbitration"
    );

    // Past `max_hold`: the hold is gone, and so is the gesture it silenced.
    tree.advance_time(Duration::from_millis(300));
    assert!(
        tree.sequence_members(PointerId::MOUSE)
            .iter()
            .all(|(_, _, state)| *state != MemberState::Held),
        "the hold must have auto-released, or the assertion below would hold \
         for the wrong reason: {:?}",
        tree.sequence_members(PointerId::MOUSE)
    );
    assert_eq!(
        state_of(&tree, peer),
        Some(MemberState::Possible),
        "and the peer is live again — it was silenced, not rejected"
    );
    assert!(
        !fired.get(),
        "the silenced long press is not delivered late: a hold drops the \
         gesture its peer produced, it does not queue it"
    );

    tree.pointer_up_button(at, PointerButton::Primary);
    tree.assert_no_leaked_pointer_state();
}

/// A pen holding an explicit capture loses it to a drag-capable ancestor —
/// but **only** once the handle declares `TouchAction::NONE`.
///
/// **A recorded defect**, written as its own test so it is reachable by name
/// and so the day it is fixed exactly one assertion has to be re-stated rather
/// than a table row quietly flipping.
///
/// An explicit `capture_pointer()` on the press enrols the captor as a
/// `RawDrag` member, and three things then decide who owns the press:
///
/// * for an **indirect** pointer `note_explicit_capture` decides on the spot,
///   so a mouse's grip is safe before any travel is measured;
/// * for a **direct** pointer the grip must instead out-latch its peers at
///   `PointerSequence::latch_slop`;
/// * a `Gesture` peer that is neither rejected nor deferred is still fed by the
///   **ordinary bubble**, which runs one step before the arbitration's
///   innermost-first walk — so the captor is protected only while its latch is
///   strictly *below* the peer's, or while the peer is deferred.
///
/// Under `AUTO` the strip's claim is eligible for a pen (`PEN.pan_slop` is
/// 8 dp), so the ancestor's `DragActivation::Auto` resolves to `AfterLongPress`
/// and it is held out of the running: the grip wins, correctly, at 2 dp.
/// Declaring `NONE` removes the pan competitor — which is the whole point of
/// declaring it — and *that* re-resolves the ancestor to `Immediate`. It is now
/// tied with the grip, because the two slops that meet here — the grip's
/// `latch_slop` (`PEN.slop_precise`, taken under a frozen `NONE`) and the
/// ancestor's `DragRecognizer` threshold (`PEN.drag_slop`) — are both 2.0, and
/// the tie goes to dispatch order. So the declaration a handle makes to protect itself from the
/// scroller is what hands it to the ancestor. A finger never sees this: its
/// `latch_slop` under `NONE` is 2 dp against the peer's 18.
///
/// The real shape is a splitter or dock resize handle inside a draggable panel
/// header. Fixing it is an arbitration-spine change (either the bubble must
/// respect the member order, or the pen token table must separate its two
/// slops), deliberately not done in the package whose job is to pin today's
/// behaviour.
#[test]
fn a_pen_loses_an_explicit_capture_to_a_drag_capable_ancestor() {
    // The control: under AUTO the ancestor is deferred and the grip keeps its
    // capture. Without this half the row below reads as "a pen can never hold a
    // handle", which is not what the framework does.
    let mut fx = build(Scenario::ColumnGrip);
    let grip = fx.id("grip");
    let at = fx.press;
    fx.tree.pen_down(at, 0.5, (0.0, 0.0));
    let pen = fx
        .tree
        .live_pointers()
        .find(|p| matches!(p.kind, PointerKind::Pen(_)))
        .map(|p| p.id)
        .expect("the pen was admitted");
    fx.tree
        .pen_move(Point::new(at.x + 3.0, at.y), 0.5, (0.0, 0.0));
    assert_eq!(
        fx.tree.sequence_winner(pen),
        Some(grip),
        "under AUTO an eligible pan defers the ancestor and the grip latches first"
    );
    fx.tree
        .pen_up(Point::new(at.x + 3.0, at.y), 0.0, (0.0, 0.0));
    fx.tree.assert_no_leaked_pointer_state();

    // The defect: the same handle, declaring the NONE that is supposed to
    // protect it, loses.
    let mut fx = build(Scenario::ColumnGripNone);
    let grip = fx.id("grip");
    let ancestor = fx.id("ancestor");
    let at = fx.press;
    fx.tree.pen_down(at, 0.5, (0.0, 0.0));
    let pen = fx
        .tree
        .live_pointers()
        .find(|p| matches!(p.kind, PointerKind::Pen(_)))
        .map(|p| p.id)
        .expect("the pen was admitted");
    assert_eq!(
        fx.tree.sequence_winner(pen),
        None,
        "a pen's explicit capture does not decide at the press"
    );
    fx.tree
        .pen_move(Point::new(at.x + 3.0, at.y), 0.5, (0.0, 0.0));
    assert_eq!(
        fx.tree.sequence_winner(pen),
        Some(ancestor),
        "NONE re-resolves the ancestor to Immediate, it ties the grip's 2 dp \
         latch, and the bubble reaches it first"
    );
    assert_ne!(fx.tree.sequence_winner(pen), Some(grip));
    assert_eq!(
        fx.cancel_log(),
        vec![("grip", CancelReason::PeerClaimed)],
        "and the captor is the one cancelled"
    );
    fx.tree
        .pen_up(Point::new(at.x + 3.0, at.y), 0.0, (0.0, 0.0));
    fx.tree.assert_no_leaked_pointer_state();
}

// ---------------------------------------------------------------------------
// The generated documentation table
// ---------------------------------------------------------------------------

const DOC_BEGIN: &str = "<!-- BEGIN GENERATED ARBITRATION MATRIX -->";
const DOC_END: &str = "<!-- END GENERATED ARBITRATION MATRIX -->";

fn render_members(members: &[(&str, Role, MemberState)]) -> String {
    if members.is_empty() {
        return "—".to_string();
    }
    members
        .iter()
        .map(|(name, role, state)| format!("`{name}` {role:?}/{state:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_movement(movement: &[Step]) -> String {
    movement
        .iter()
        .map(|(dx, dy, winner)| {
            let who = winner.map(|w| format!("`{w}`")).unwrap_or("—".to_string());
            format!("({dx:+.0}, {dy:+.0}) → {who}")
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn render_cancels(cancels: &[(&str, CancelReason)]) -> String {
    if cancels.is_empty() {
        return "—".to_string();
    }
    cancels
        .iter()
        .map(|(name, reason)| format!("`{name}` {reason:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A `TouchAction` under the name it is declared by. `Debug` prints the raw bit
/// field, which no reader of the documentation can decode.
fn render_action(action: TouchAction) -> &'static str {
    match action {
        TouchAction::AUTO => "AUTO",
        TouchAction::NONE => "NONE",
        TouchAction::PAN => "PAN",
        TouchAction::PAN_X => "PAN_X",
        TouchAction::PAN_Y => "PAN_Y",
        TouchAction::PINCH_ZOOM => "PINCH_ZOOM",
        TouchAction::MANIPULATION => "MANIPULATION",
        _ => "(composite)",
    }
}

fn render_kind(kind: PointerKind) -> &'static str {
    match kind {
        PointerKind::Touch => "touch",
        PointerKind::Pen(_) => "pen",
        _ => "mouse",
    }
}

/// The table as it must appear in `docs/events-and-gestures.md`.
fn render_table() -> String {
    let mut out = String::new();
    out.push_str(
        "<!-- Generated by crates/teksilo-core/tests/arbitration_matrix.rs. \
         Do not edit by hand: `the_documented_table_matches_the_fixtures` \
         fails when the two drift, and `TEKSILO_BLESS=1 cargo test -p \
         teksilo-core --test arbitration_matrix` rewrites this region. -->\n\n",
    );
    out.push_str(
        "Each row is a **core-only fixture** reproducing the named widget's \
         arbitration shape — the handlers it installs, the claims it declares \
         and the capture it takes — not the widget itself, which lives in a \
         crate `teksilo-core` cannot depend on. A row's *claimant* is the \
         enclosing scroller the real shape would sit in, written out, except \
         where the named widget declares one itself. So read a row as *what \
         the framework does with this shape*.\n\n",
    );
    out.push_str(
        "| Scenario | Pointer | Frozen | Members at press (innermost first) | \
         Movement → winner | Losers cancelled | What it pins |\n",
    );
    out.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for row in rows() {
        out.push_str(&format!(
            "| {} | {} | `{}` | {} | {} | {} | {} |\n",
            row.name
                .rsplit_once(" · ")
                .map(|(head, _)| head)
                .unwrap_or(row.name),
            render_kind(row.kind),
            render_action(row.frozen),
            render_members(row.members),
            render_movement(row.movement),
            render_cancels(row.cancels),
            row.note,
        ));
    }
    out
}

/// The table checked into `docs/events-and-gestures.md` is the one these
/// fixtures produce.
///
/// A scenario table that is retyped goes stale the first time a threshold moves
/// and then documents a framework that no longer exists. This asserts the
/// checked-in text character for character and prints the replacement on
/// failure, so regenerating it is a copy rather than a re-derivation.
#[test]
fn the_documented_table_matches_the_fixtures() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/events-and-gestures.md");
    let doc = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    // A Windows checkout under `core.autocrlf` hands the doc back with CRLF
    // line ends; the table is compared, and rewritten, on the LF it is
    // committed with.
    let doc = doc.replace("\r\n", "\n");

    let begin = doc
        .find(DOC_BEGIN)
        .unwrap_or_else(|| panic!("{} carries no {DOC_BEGIN}", path.display()))
        + DOC_BEGIN.len();
    let end = doc
        .find(DOC_END)
        .unwrap_or_else(|| panic!("{} carries no {DOC_END}", path.display()));

    let checked_in = doc[begin..end].trim_matches('\n');
    let generated = render_table();
    let generated = generated.trim_matches('\n');

    // Regenerating by hand is a transcription step, and a transcription step is
    // where a generated table picks up its first hand edit. `TEKSILO_BLESS=1`
    // rewrites the region and re-asserts, so the checked-in text is only ever a
    // copy of what these fixtures produced.
    if std::env::var_os("TEKSILO_BLESS").is_some() && checked_in != generated {
        let updated = format!("{}\n{generated}\n{}", &doc[..begin], &doc[end..]);
        std::fs::write(&path, updated).expect("rewriting the documentation table");
        panic!(
            "the arbitration table in {} was regenerated; re-run without \
             TEKSILO_BLESS to confirm",
            path.display()
        );
    }

    assert_eq!(
        checked_in,
        generated,
        "the arbitration table in {} is out of date. Re-run with TEKSILO_BLESS=1 \
         to regenerate it, or replace everything between the two markers \
         with:\n\n{generated}\n",
        path.display()
    );
}
