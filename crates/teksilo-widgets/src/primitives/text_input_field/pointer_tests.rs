// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pointer editing on the single-line stack — both devices, in one file
//! because they are one behaviour with two answers.
//!
//! The **mouse** half is a baseline this package needed before it could touch
//! `mouse.rs` at all: nothing anywhere in the stack dispatched a
//! `PointerDown`/`PointerMove` pair or a double / triple click at a
//! `TextInputField`, so "the pre-existing suite still passes" was satisfiable
//! with drag-select broken. Every assertion here describes behaviour that
//! predates touch and must survive it byte for byte.
//!
//! The **touch** half is the new behaviour: a release-time caret, a word
//! selected by a hold, handles in the affordance band, and the secure-field
//! exceptions.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::event::{EventResponse, PointerButton, ScrollDelta, WidgetEvent};
use teksilo_core::overlay::SelectionHandleKind;
use teksilo_core::pointer::{PointerId, PointerPhase};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_tokens::PointerKind;

use super::state::SharedState;
use super::touch::FieldTouch;
use super::*;
use crate::button::press_test_support::{finger, touch};

/// The field sits at (20, 20) so every coordinate assertion has to survive the
/// window→local conversion the router applies. A field at the origin cannot
/// tell a correct conversion from a missing one.
fn origin() -> Point {
    Point::new(20.0, 120.0)
}

fn viewport() -> SizeProposal {
    SizeProposal::exact(400.0, 400.0)
}

struct Harness {
    tree: WidgetTree,
    state: SharedState,
    touch: Rc<FieldTouch>,
    field: WidgetId,
    /// Somewhere else for the focus to go.
    elsewhere: WidgetId,
}

impl Harness {
    fn new(field: TextInputField, text: &str) -> Self {
        let _ = text;
        let state_slot = field.state_slot.clone();
        let touch = field.touch.clone();
        // The focusable sibling is a VStack child rather than a second root:
        // roots stack, and one added after the field would sit on top of it and
        // take every press.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        // 120 dp of headroom, so the toolbar has somewhere to hang above the
        // selection instead of flipping down over the handles.
        let padded = tree.add(crate::primitives::Padding::symmetric(120.0, 20.0).child(field));
        let other = tree.add(
            crate::primitives::FixedSize::new()
                .width(40.0)
                .height(20.0)
                .child(crate::primitives::RectWidget::new().focusable(true)),
        );
        tree.add(
            crate::primitives::VStack::new()
                .add_child(padded)
                .add_child(other),
        );
        tree.layout(viewport());
        // The engine lays out in `paint`, so a test that never renders
        // hit-tests an empty layout and every click lands at the document end.
        let _ = tree.render();
        let state = state_slot.borrow().clone().expect("the field was built");
        let field = tree
            .first_focusable_descendant(padded)
            .expect("the field is focusable");
        let elsewhere = tree
            .first_focusable_descendant(other)
            .expect("somewhere else for the focus to go");
        Self {
            tree,
            state,
            touch,
            field,
            elsewhere,
        }
    }

    fn plain(text: &str) -> Self {
        Self::new(TextInputField::new(Signal::new(text.to_string())), text)
    }

    fn focused(text: &str) -> Self {
        let mut h = Self::plain(text);
        h.tree.focus(h.field);
        h.tree.layout(viewport());
        let _ = h.tree.render();
        h
    }

    /// Window x of the caret boundary before character `offset`.
    fn x_of(&self, offset: usize) -> f32 {
        let st = self.state.borrow();
        origin().x + st.engine.caret_rect(offset, CursorAffinity::Downstream)[0] - st.scroll_x
    }

    fn at(&self, offset: usize) -> Point {
        Point::new(self.x_of(offset), origin().y + 8.0)
    }

    fn selection(&self) -> std::ops::Range<usize> {
        let st = self.state.borrow();
        let a = st.cursor.anchor();
        let p = st.cursor.position();
        a.min(p)..a.max(p)
    }

    fn caret(&self) -> usize {
        self.state.borrow().cursor.position()
    }

    fn render(&mut self) {
        self.tree.layout(viewport());
        let _ = self.tree.render();
    }

    /// Where `content`'s overlay sits in the stack, which is what the band
    /// decides: bands sort before show order.
    fn stack_index(&self, content: WidgetId) -> Option<usize> {
        let id = self.tree.overlay_manager().find_by_content(content)?;
        self.tree
            .overlay_manager()
            .active_ids()
            .into_iter()
            .position(|other| other == id)
    }

    fn mouse_press(&mut self, at: Point) {
        self.tree.pointer_down_button(at, PointerButton::Primary);
    }

    fn mouse_move(&mut self, at: Point) {
        self.tree
            .dispatch_event(WidgetEvent::PointerMove { position: at });
    }

    fn mouse_release(&mut self, at: Point) {
        self.tree.pointer_up_button(at, PointerButton::Primary);
    }

    fn finger_down(&mut self, id: PointerId, at: Point, ms: u64) {
        self.tree
            .dispatch_pointer(touch(id, PointerPhase::Down, at, ms));
    }

    fn finger_move(&mut self, id: PointerId, at: Point, ms: u64) {
        self.tree
            .dispatch_pointer(touch(id, PointerPhase::Move, at, ms));
    }

    fn finger_up(&mut self, id: PointerId, at: Point, ms: u64) {
        self.tree
            .dispatch_pointer(touch(id, PointerPhase::Up, at, ms));
    }

    /// The affordance widgets — the handles and, when one is mounted, the lens —
    /// with whether each is currently active.
    fn affordance_nodes(&self) -> Vec<(bool, Rect)> {
        let host = self
            .touch
            .layer
            .get()
            .expect("the affordance host was built");
        let layer = self.tree.children(host)[0];
        self.tree
            .children(layer)
            .into_iter()
            .map(|id| (self.tree.is_active(id), self.tree.bounds(id)))
            .collect()
    }

    fn handles(&self) -> Vec<teksilo_core::text_touch::SelectionHandleGeometry> {
        self.touch.controller.borrow().handles()
    }

    fn handle(
        &self,
        kind: SelectionHandleKind,
    ) -> Option<teksilo_core::text_touch::SelectionHandleGeometry> {
        self.handles().into_iter().find(|h| h.kind == kind)
    }

    fn toolbar_actions(&self) -> Vec<teksilo_core::text_touch::TextAction> {
        self.touch
            .controller
            .borrow()
            .toolbar()
            .map(|t| t.actions)
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// The mouse baseline
// ---------------------------------------------------------------------------

/// A mouse press places the caret at the press — before any release, and
/// before any gesture is recognised. That is the click convention this package
/// deliberately does not move for a precise pointer.
#[test]
fn a_mouse_press_places_the_caret_where_it_landed() {
    let mut h = Harness::focused("hello world");
    let target = h.at(5);
    h.mouse_press(target);
    assert_eq!(h.caret(), 5, "the caret follows the press, not the release");
    assert!(h.selection().is_empty(), "a bare press selects nothing");
}

/// Press, move, release selects everything the pointer crossed.
#[test]
fn a_mouse_drag_selects_the_text_it_crossed() {
    let mut h = Harness::focused("hello world");
    let from = h.at(2);
    let to = h.at(8);
    h.mouse_press(from);
    h.mouse_move(to);
    h.mouse_release(to);
    assert_eq!(h.selection(), 2..8);
}

/// …and the release ends the session: a later move with no button down must not
/// keep extending it.
#[test]
fn a_mouse_move_after_the_release_extends_nothing() {
    let mut h = Harness::focused("hello world");
    let from = h.at(2);
    let to = h.at(6);
    h.mouse_press(from);
    h.mouse_move(to);
    h.mouse_release(to);
    let after_release = h.selection();
    h.mouse_move(h.at(11));
    assert_eq!(h.selection(), after_release);
}

/// A double click selects the word under it.
#[test]
fn a_mouse_double_click_selects_the_word_under_it() {
    let mut h = Harness::focused("hello world");
    let target = h.at(8);
    h.mouse_press(target);
    h.mouse_release(target);
    h.mouse_press(target);
    h.mouse_release(target);
    assert_eq!(h.selection(), 6..11, "the second word");
}

/// A triple click selects the whole single-line document.
#[test]
fn a_mouse_triple_click_selects_the_whole_field() {
    let mut h = Harness::focused("hello world");
    let target = h.at(8);
    for _ in 0..3 {
        h.mouse_press(target);
        h.mouse_release(target);
    }
    assert_eq!(h.selection(), 0..11);
}

/// A non-primary press is not a caret placement.
#[test]
fn a_middle_click_does_not_move_the_caret() {
    let mut h = Harness::focused("hello world");
    let start = h.caret();
    let target = h.at(5);
    h.tree.pointer_down_button(target, PointerButton::Middle);
    h.tree.pointer_up_button(target, PointerButton::Middle);
    assert_eq!(h.caret(), start);
}

/// A mouse raises no affordance at all — the whole touch layer stays empty for
/// a precise pointer, which is what "the mouse path is untouched" means.
#[test]
fn a_mouse_raises_no_affordances() {
    let mut h = Harness::focused("hello world");
    let from = h.at(2);
    let to = h.at(8);
    h.mouse_press(from);
    h.mouse_move(to);
    h.mouse_release(to);
    assert_eq!(h.selection(), 2..8, "it did select");
    assert!(
        h.handles().is_empty(),
        "…and it raised no handles: {:?}",
        h.handles()
    );
    assert!(h.toolbar_actions().is_empty(), "nor a selection toolbar");
}

/// A mouse hold is not a word selection. The gesture arena installs a
/// long-press recognizer on the *presence* of the handler, with no
/// pointer-kind condition, so the host's own guard is the only thing standing
/// between a resting mouse button and a selected word.
#[test]
fn a_mouse_hold_selects_no_word() {
    let mut h = Harness::focused("hello world");
    let target = h.at(8);
    h.tree.long_press_at(PointerKind::Mouse, target);
    assert!(
        h.selection().is_empty(),
        "a mouse hold selected {:?}",
        h.selection()
    );
    assert!(h.handles().is_empty(), "and raised handles");
}

// ---------------------------------------------------------------------------
// Touch: the caret
// ---------------------------------------------------------------------------

/// A finger's press is not yet a click — the same contact is the opening
/// sample of a scroll — so the caret is placed on the release.
#[test]
fn a_finger_places_the_caret_on_the_release_not_the_press() {
    let mut h = Harness::focused("hello world");
    let start = h.caret();
    let target = h.at(5);
    let id = finger();
    h.finger_down(id, target, 0);
    assert_eq!(
        h.caret(),
        start,
        "the press must not have moved the caret yet"
    );
    h.finger_up(id, target, 30);
    assert_eq!(h.caret(), 5, "the release places it");
}

/// A finger that left the field before lifting placed no caret.
///
/// The predicate is the framework's own tap boundary — the same one that decides
/// whether a button release still counts as a click — so a slide *within* the
/// field is still a tap that lands where it lifted, and a slide off it is not a
/// tap at all. Deliberately the framework's rule rather than a text-specific
/// one: a 360 dp field and a 360 dp button answer the same question the same
/// way.
#[test]
fn a_finger_that_left_the_field_places_no_caret() {
    let mut h = Harness::focused("hello world");
    h.state
        .borrow()
        .cursor
        .set_position(0, teksilo_text::text_document::MoveMode::MoveAnchor);
    let from = h.at(2);
    let id = finger();
    h.finger_down(id, from, 0);
    // Straight down, out of the field's own 20 dp band.
    let away = Point::new(from.x, origin().y + 150.0);
    h.finger_move(id, away, 16);
    h.finger_up(id, away, 32);
    assert!(
        h.selection().is_empty(),
        "a departing finger selected {:?}",
        h.selection()
    );
    assert_eq!(h.caret(), 0, "and it placed no caret");
}

/// The plan's scrolling test, with both halves asserted. Losing the pan
/// arbitration silences a node's *recognizers*, not its `on_pointer_event`, so
/// the scroll alone is not evidence: before this package the field extended its
/// selection **while** the pan reached the container.
#[test]
fn a_finger_over_a_field_in_a_scroller_scrolls_it_and_leaves_the_selection_alone() {
    let field = TextInputField::new(Signal::new("hello world".to_string()));
    let state_slot = field.state_slot.clone();
    let scrolled = Rc::new(Cell::new(0.0_f32));
    let seen = scrolled.clone();

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let inner = tree.add(crate::primitives::Padding::uniform(20.0).child(field));
    tree.add(
        crate::primitives::VStack::new()
            .add_child(inner)
            .scroll_container(teksilo_core::pointer::touch_action::PanAxes::BOTH)
            .pan_claim(teksilo_core::pointer::touch_action::PanClaim::vertical())
            .on_scroll(move |event, _ctx| {
                if let WidgetEvent::Scroll { delta, .. } = event {
                    let dy = match delta {
                        ScrollDelta::Pixels { y, .. } => *y,
                        ScrollDelta::Lines { y, .. } => *y * 20.0,
                    };
                    seen.set(seen.get() + dy);
                }
                EventResponse::Handled
            }),
    );
    tree.layout(viewport());
    let _ = tree.render();
    let state = state_slot.borrow().clone().expect("the field was built");
    let field_id = tree
        .first_focusable_descendant(inner)
        .expect("the field is focusable");
    tree.focus(field_id);
    tree.layout(viewport());
    let _ = tree.render();
    state
        .borrow()
        .cursor
        .set_position(0, teksilo_text::text_document::MoveMode::MoveAnchor);

    let slop = teksilo_core::gesture::default_profile(PointerKind::Touch)
        .pan_slop
        .expect("a touch profile pans");
    let from = Point::new(60.0, 30.0);
    let id = finger();
    tree.dispatch_pointer(touch(id, PointerPhase::Down, from, 0));
    let armed = Point::new(from.x, from.y - slop - 1.0);
    tree.dispatch_pointer(touch(id, PointerPhase::Move, armed, 16));
    let end = Point::new(from.x, armed.y - 40.0);
    tree.dispatch_pointer(touch(id, PointerPhase::Move, end, 32));
    tree.dispatch_pointer(touch(id, PointerPhase::Up, end, 48));

    assert!(
        scrolled.get() != 0.0,
        "the finger's pan never reached the container above the field"
    );
    let st = state.borrow();
    let (a, p) = (st.cursor.anchor(), st.cursor.position());
    drop(st);
    assert_eq!(
        (a, p),
        (0, 0),
        "the scrolling finger moved the field's caret or selection"
    );
}

// ---------------------------------------------------------------------------
// Touch: the hold
// ---------------------------------------------------------------------------

/// A hold selects the word under the finger and raises the two handles that
/// adjust it.
#[test]
fn a_finger_hold_selects_a_word_and_raises_its_handles() {
    let mut h = Harness::focused("hello world");
    let target = h.at(8);
    h.tree.long_press_at(PointerKind::Touch, target);
    assert_eq!(h.selection(), 6..11, "the word under the finger");
    let kinds: Vec<_> = h.handles().iter().map(|g| g.kind).collect();
    assert_eq!(
        kinds,
        vec![SelectionHandleKind::Start, SelectionHandleKind::End],
        "a range gets two handles and no caret handle"
    );
}

/// The hold's own position is converted out of the field's space into the window
/// space the controller works in.
///
/// A `TapEvent`'s position arrives widget-**local**, like every other pointer
/// coordinate the router hands a handler, while `TextHitSource` is stated
/// entirely in window coordinates — and a long press has no sample to read a
/// window position from, because it is recognised by a timer. The point chosen
/// here is a few characters into the *second* word, so a hold that lost the
/// field's origin on the way in selects the first one instead of merely being
/// off by a character.
#[test]
fn a_hold_selects_the_word_under_the_finger_and_not_one_beside_it() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(7));
    assert_eq!(h.selection(), 6..11, "the second word, not the first");
}

/// …and the handles land on the caret they mark, in window coordinates. The
/// router hands a handler widget-**local** positions while the controller's
/// whole contract is in window space, so a host that forwards them unconverted
/// puts every handle one viewport origin away from its text.
#[test]
fn a_raised_handle_sits_at_the_caret_it_marks_in_window_space() {
    let mut h = Harness::focused("hello world");
    let target = h.at(8);
    h.tree.long_press_at(PointerKind::Touch, target);
    let start = h
        .handle(SelectionHandleKind::Start)
        .expect("a start handle is up");
    let expected_x = h.x_of(6);
    assert!(
        (start.caret.x - expected_x).abs() < 0.5,
        "start handle at {:?}, caret 6 is at x={expected_x}",
        start.caret
    );
    assert!(
        (start.caret.y - origin().y).abs() < 1.0,
        "and on the field's own line: {:?} vs y={}",
        start.caret,
        origin().y
    );
}

/// The affordance layer is mounted in the text-affordance band and the toolbar
/// in the standard one, and bands sort before show order — so a handle raised
/// while a menu is open goes under it.
#[test]
fn the_layer_sorts_under_the_toolbar() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    h.render();
    let layer = h.touch.layer.get().expect("the layer was built");
    let toolbar = h.touch.toolbar.get().expect("the toolbar was built");
    let (below, above) = (
        h.stack_index(layer).expect("the layer is raised"),
        h.stack_index(toolbar).expect("the toolbar is raised"),
    );
    assert!(
        below < above,
        "the affordance band must sort under the menu: {below} vs {above}"
    );
}

/// The next tap after a hold places a caret **and** takes the toolbar down, in
/// one gesture.
///
/// This is what decides the toolbar's dismiss behaviour. A direct pointer's
/// outside press is *armed* rather than dismissed — the framework withholds both
/// the down and the up from the tree, on the grounds that a finger covers what
/// it is about to actuate — so a click-outside toolbar would spend this tap
/// closing itself and the caret would not move. The host owns the retirement
/// instead.
#[test]
fn a_tap_after_a_hold_places_a_caret_and_takes_the_toolbar_down() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    h.render();
    let toolbar = h.touch.toolbar.get().expect("the toolbar was built");
    assert!(h.stack_index(toolbar).is_some(), "the hold raised it");

    // Clear of both 44 dp handle targets — see
    // `a_tap_inside_a_handles_target_adjusts_the_selection_instead` for what
    // happens inside one.
    let target = h.at(1);
    let id = finger();
    h.finger_down(id, target, 0);
    h.finger_up(id, target, 30);
    h.render();
    assert_eq!(h.caret(), 1, "the tap placed no caret");
    assert!(
        h.selection().is_empty(),
        "and it collapsed the hold's selection"
    );
    assert!(
        !h.tree.is_active(toolbar),
        "the toolbar is still showing over a selection that is gone"
    );
}

/// A tap that lands **inside** a handle's target adjusts that end of the
/// selection instead of placing a caret.
///
/// Not a defect but a consequence of the geometry, and the rule is the ladder
/// rather than any one measurement. A handle's target is `HANDLE_HIT_SIZE` through
/// `dp(.., TargetRole::Target, ..)`, which only ever *raises* — and 44 dp is
/// already at or above every rung of the shipped ladder, so the target is 44 dp
/// square at all three densities. A single-line field is shorter than that: this
/// harness builds the bare primitive, whose height is its measured text height,
/// and the styled field's is `dp(TEXT_FIELD_HEIGHT, Target, ..)` — the larger of
/// its own recipe constant and the density's `target_size` — which reaches 44 dp
/// only at `TargetDensity::Touch`. So below Touch the two handles of a short
/// selection blanket the text between them and a little either side, which is why
/// a tap meant to dismiss a selection has to land clear of both ends.
#[test]
fn a_tap_inside_a_handles_target_adjusts_the_selection_instead() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    h.render();
    let start = h
        .handle(SelectionHandleKind::Start)
        .expect("a start handle is up");
    assert!(
        start.hit.contains(h.at(3)),
        "the fixture assumes offset 3 is under the start handle's target: {:?}",
        start.hit
    );
    let id = finger();
    h.finger_down(id, h.at(3), 0);
    h.finger_up(id, h.at(3), 30);
    assert_eq!(
        h.selection(),
        3..11,
        "the tap dragged the start of the selection to where it landed"
    );
}

/// The affordance overlay covers the **whole viewport**, and that is now safe.
///
/// This test asserted the opposite until the router learned to fall through, and
/// the reason is worth keeping: an overlay is chosen by its rectangle and then
/// searched alone, and the router returned whatever that search answered — `None`
/// included — so a viewport-sized affordance overlay answered "nothing here" for
/// every press that was not on a handle and the field stopped taking presses.
/// Sizing the overlay to the affordances was the way round it, and the size was
/// therefore a correctness property.
///
/// `WidgetTree::hit_test_with` now falls through to the tree when the chosen
/// overlay's content root declares `event_pass_through` and its subtree claimed
/// nothing, so the placement the affordance band was designed for is usable and
/// the geometry the layer needs — a rectangle containing every position the text
/// can be at — is the plain one. The behaviour that made the size load-bearing is
/// pinned by `a_tap_beyond_the_affordances_reaches_the_field_itself` below, whose
/// **assertions** did not change — its one behaviour assertion is byte-identical —
/// and which reddens if the fall-through is reverted. Its *fixture* had to change:
/// it used to read the overlay's own rectangle, and that was only ever the same
/// rectangle as the affordances' while the overlay was sized to them, so it now
/// reads the affordances' directly, which is what it always meant.
#[test]
fn the_affordance_overlay_covers_the_viewport_and_passes_presses_through() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(2));
    h.render();
    let host = h.touch.layer.get().expect("the affordance host was built");
    let bounds = h
        .tree
        .overlay_manager()
        .bounds_for_content(host)
        .expect("the overlay is up");
    let wanted = super::touch::affordance_bounds(&h.touch.controller.borrow().affordances());
    assert!(
        bounds.width >= 400.0 && bounds.height >= 400.0,
        "the layer must be given the whole viewport: {bounds:?}"
    );
    assert!(
        bounds.contains(wanted.origin())
            && bounds.contains(Point::new(wanted.right(), wanted.bottom())),
        "…and it must contain the affordances it holds, or the hit-test walk \
         never descends to them: {bounds:?} against {wanted:?}"
    );
}

/// A tap **beyond** the affordances' rectangle reaches the field, and the field
/// answers it.
///
/// The host-level proof of the router's fall-through: the overlay above this press
/// is the whole viewport, so a router that returned what the affordance layer
/// answered would drop the press and the field would stop taking presses for as
/// long as a selection was up. Measured that way before the fix, as `hit_test`
/// returning `None` at a point inside the field.
///
/// It is not the only assertion here that depends on the fall-through, and the
/// others are the better evidence because **three of the four pre-date it**. Any
/// test that presses inside the field, clear of the handles, while a selection is
/// up is pressing under the viewport-sized overlay and asserting that the field
/// answered — so reverting the fall-through reddens this one,
/// `a_tap_after_a_hold_places_a_caret_and_takes_the_toolbar_down`,
/// `a_right_click_retires_the_affordances_the_framework_tore_down` and
/// `a_mouse_click_in_the_field_away_from_the_chrome_retires_it`. Only the last was
/// written alongside the fix; the first three were already here, asserting
/// ordinary behaviour, which is what makes them worth more than a bespoke test.
///
/// The fixture reads the **affordances'** rectangle rather than the overlay's,
/// which is what it always meant; the two were the same thing only while the
/// overlay was sized to them.
#[test]
fn a_tap_beyond_the_affordances_reaches_the_field_itself() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(2));
    h.render();
    let over = super::touch::affordance_bounds(&h.touch.controller.borrow().affordances());
    let target = h.at(11);
    assert!(
        target.x > over.right(),
        "the fixture assumes the end of the text is clear of the affordances {over:?}"
    );
    let id = finger();
    h.finger_down(id, target, 0);
    h.finger_up(id, target, 30);
    assert_eq!(h.caret(), 11, "the field never saw the tap");
}

/// A press outside the handles does not retire them: every tap that moves a
/// caret is outside a selection handle, so outside-press dismissal would take
/// them away on the first tap that used them. Their lifetime is the host's.
#[test]
fn the_handles_survive_a_press_outside_them() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    h.render();
    let layer = h.touch.layer.get().expect("the layer was built");
    let away = Point::new(origin().x + 300.0, origin().y + 60.0);
    h.mouse_press(away);
    h.mouse_release(away);
    h.render();
    assert!(
        h.stack_index(layer).is_some(),
        "an outside press retired the affordance band"
    );
}

/// Only the handles the controller wants are active: a caret handle parked
/// while a range is up would be a third target over text it does not mark.
#[test]
fn the_hold_activates_two_handles_and_no_lens() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    h.render();
    let active = h.affordance_nodes().iter().filter(|(a, _)| *a).count();
    assert_eq!(active, 2, "two handles, no caret handle, no lens");
}

/// A hold on an editable field offers the commands it can honour, and only
/// those.
#[test]
fn the_toolbar_offers_what_the_field_will_honour() {
    use teksilo_core::text_touch::TextAction;
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    assert_eq!(
        h.toolbar_actions(),
        vec![TextAction::Cut, TextAction::Copy, TextAction::Paste],
        "a selection in an editable field"
    );
}

/// A read-only field can be read from and not written to, so its toolbar is
/// Copy and nothing else.
#[test]
fn a_read_only_field_offers_copy_alone() {
    use teksilo_core::text_touch::TextAction;
    let field = TextInputField::new(Signal::new("hello world".to_string())).read_only(true);
    let mut h = Harness::new(field, "hello world");
    h.tree.focus(h.field);
    h.render();
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    assert_eq!(h.toolbar_actions(), vec![TextAction::Copy]);
    assert!(
        h.handle(SelectionHandleKind::Caret).is_none(),
        "a caret handle is offered only where the user may place a caret"
    );
}

// ---------------------------------------------------------------------------
// Touch: adjusting the range
// ---------------------------------------------------------------------------

/// Dragging the end handle grows the selection — through the handle's own
/// overlay node, which is what a finger actually meets.
#[test]
fn dragging_the_end_handle_grows_the_selection() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(2));
    assert_eq!(h.selection(), 0..5, "the first word");
    h.render();

    let end = h
        .handle(SelectionHandleKind::End)
        .expect("an end handle is up");
    let target = Point::new(h.x_of(9), end.anchor.y);
    let contact = h.tree.new_contact();
    h.tree.touch_down(contact, end.anchor);
    h.tree.touch_move(contact, target);
    h.tree.touch_up(contact, target);

    assert_eq!(h.selection(), 0..9);
}

/// A **mouse** click over the affordances places a caret and takes them away.
///
/// Three things at once, and each is load-bearing on a hybrid machine. The
/// handle refuses an indirect pointer (its own guard, in `teksilo-core`), so the
/// click is not a drag. The chrome is standing over the text a cursor is trying
/// to reach, and nothing else would ever retire it — the affordance band is
/// exempt from outside-press dismissal by design. And the click was a request
/// for a caret, which it gets.
#[test]
fn a_mouse_click_over_the_affordances_places_a_caret_and_retires_them() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(2));
    h.render();
    let end = h
        .handle(SelectionHandleKind::End)
        .expect("an end handle is up");
    let target = Point::new(h.x_of(9), end.anchor.y);
    h.mouse_press(end.anchor);
    h.mouse_move(target);
    h.mouse_release(target);
    assert_ne!(
        h.selection(),
        0..9,
        "the mouse dragged the handle instead of clicking through it"
    );
    assert_eq!(h.caret(), 5, "the click placed no caret");
    assert!(
        h.selection().is_empty(),
        "…and left a selection behind: {:?}",
        h.selection()
    );
    assert!(
        h.handles().is_empty(),
        "the touch chrome outlived the cursor's arrival"
    );
}

/// A cursor's click **anywhere in the field** retires the touch chrome.
///
/// The companion to the test above, and a deliberate widening. While the
/// affordance overlay was sized to the affordances, this behaviour reached only
/// the presses inside that rectangle — a mouse click on the text a few dp away
/// left the handles and the toolbar standing. The overlay is the viewport now and
/// its content root passes presses through, so those presses arrive at the field
/// itself and the field is where the answer belongs: a cursor has taken over, and
/// the affordance band is exempt from outside-press dismissal, so nothing else on
/// a hybrid machine would ever remove them.
#[test]
fn a_mouse_click_in_the_field_away_from_the_chrome_retires_it() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(2));
    h.render();
    assert!(!h.handles().is_empty(), "the hold raised handles");

    // Clear of every affordance, and inside the field.
    let over = super::touch::affordance_bounds(&h.touch.controller.borrow().affordances());
    let at = h.at(11);
    assert!(
        at.x > over.right(),
        "the fixture assumes {at:?} is clear of the affordances {over:?}"
    );
    h.mouse_press(at);
    h.mouse_release(at);
    h.render();

    assert!(
        h.handles().is_empty(),
        "the touch chrome outlived the cursor's arrival: {:?}",
        h.handles()
    );
    assert_eq!(h.caret(), 11, "…and the click still placed its caret");
}

/// A finger's double tap selects a word and raises its handles, like a hold.
///
/// The multi-tap recognizers are the field's own and predate touch; what is new
/// is that a *direct* pointer's selection has to raise the affordances for it,
/// because the controller does not see a gesture.
#[test]
fn a_finger_double_tap_selects_a_word_and_raises_its_handles() {
    let mut h = Harness::focused("hello world");
    let target = h.at(8);
    let first = finger();
    h.finger_down(first, target, 0);
    h.finger_up(first, target, 20);
    let second = finger();
    h.finger_down(second, target, 60);
    h.finger_up(second, target, 80);
    assert_eq!(h.selection(), 6..11, "the word under the taps");
    assert_eq!(
        h.handles().len(),
        2,
        "…and its two handles: {:?}",
        h.handles()
    );
}

/// …and a triple tap takes the whole line, with the handles at its two ends.
#[test]
fn a_finger_triple_tap_selects_the_whole_field_and_raises_its_handles() {
    let mut h = Harness::focused("hello world");
    let target = h.at(8);
    for tap in 0..3u64 {
        let id = finger();
        h.finger_down(id, target, tap * 40);
        h.finger_up(id, target, tap * 40 + 20);
    }
    assert_eq!(h.selection(), 0..11);
    let offsets: Vec<_> = h.handles().iter().map(|g| g.offset).collect();
    assert_eq!(offsets, vec![0, 11]);
}

/// An assistive client that sets the selection moves the handles with it.
///
/// `SetTextSelection` on the field's own node is the screen reader's route into
/// the selection, and it is not a route the controller can see.
#[test]
fn an_assistive_selection_moves_the_handles() {
    use teksilo_core::accesskit::{Action, ActionData, TextSelection};
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    h.render();
    assert_eq!(h.selection(), 6..11, "raised over the second word");

    let node = teksilo_core::accessibility::widget_id_to_node_id(h.field);
    let position = |index: usize| teksilo_core::accesskit::TextPosition {
        node,
        character_index: index,
    };
    let handled = h.tree.dispatch_access_action(
        node,
        Action::SetTextSelection,
        Some(ActionData::SetTextSelection(TextSelection {
            anchor: position(0),
            focus: position(5),
        })),
        &mut teksilo_core::window::NoopWindowOps,
    );
    assert!(handled, "the field refused SetTextSelection");
    h.render();
    assert_eq!(h.selection(), 0..5);
    let offsets: Vec<_> = h.handles().iter().map(|g| g.offset).collect();
    assert_eq!(
        offsets,
        vec![0, 5],
        "the handles still mark the old range: {:?}",
        h.handles()
    );
}

/// A handle is a slider to a screen reader, and `SetValue` on it is the only
/// route assistive technology has into the selection.
#[test]
fn a_handle_answers_set_value_by_moving_that_end() {
    use teksilo_core::accesskit::{Action, ActionData};
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(2));
    h.render();
    let end = h
        .handle(SelectionHandleKind::End)
        .expect("an end handle is up");
    let host = h.touch.layer.get().expect("the affordance host was built");
    let layer = h.tree.children(host)[0];
    let end_node = h
        .tree
        .children(layer)
        .into_iter()
        .find(|id| {
            let b = h.tree.bounds(*id);
            h.tree.is_active(*id) && (b.x - end.hit.x).abs() < 0.5 && (b.y - end.hit.y).abs() < 0.5
        })
        .expect("the end handle has a node placed on its own hit rectangle");
    let handled = h.tree.dispatch_access_action(
        teksilo_core::accessibility::widget_id_to_node_id(end_node),
        Action::SetValue,
        Some(ActionData::NumericValue(9.0)),
        &mut teksilo_core::window::NoopWindowOps,
    );
    assert!(handled, "the handle refused SetValue");
    assert_eq!(h.selection(), 0..9);
}

// ---------------------------------------------------------------------------
// Touch: lifetime
// ---------------------------------------------------------------------------

/// The affordance band is exempt from outside-press dismissal and is
/// anchor-independent, so nothing but the host retires it.
///
/// Two mechanisms both answer this one: the field's own `on_focus` calls
/// `dismiss`, and the overlay's `on_dismiss` callback fires if the framework
/// takes the overlay down as focus moves. Each is pinned on its own below —
/// `deactivating_the_window_retires_the_affordances` for the first, which no
/// overlay teardown can serve, and
/// `a_right_click_retires_the_affordances_the_framework_tore_down` for the
/// second, where nothing about focus changes.
#[test]
fn losing_focus_retires_the_affordances() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    assert!(!h.handles().is_empty(), "raised first");
    h.tree.focus(h.elsewhere);
    h.render();
    assert!(
        h.handles().is_empty(),
        "focus left and the handles stayed: {:?}",
        h.handles()
    );
    assert!(h.toolbar_actions().is_empty(), "…and so did the toolbar");
}

/// Deactivating the window retires them too, and this is the path only the
/// host's own `dismiss` can serve: the effect that watches window activation has
/// no `EventContext`, so it cannot dismiss an overlay — which is exactly why
/// retirement is the controller publishing empty geometry rather than an overlay
/// teardown.
#[test]
fn deactivating_the_window_retires_the_affordances() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    assert!(!h.handles().is_empty(), "raised first");
    h.tree.set_window_active(false);
    h.render();
    assert!(
        h.handles().is_empty(),
        "handles over an inactive window: {:?}",
        h.handles()
    );
    let host = h.touch.layer.get().expect("the affordance host was built");
    assert!(
        h.stack_index(host).is_some(),
        "…and the overlay is still on the stack, which is the point: nothing \
         with an EventContext ran"
    );
}

/// When the **framework** takes an overlay down, the host has to notice.
///
/// A right-click clears the transient overlays before mounting its menu, and the
/// affordance overlay is one of them. Its content is gated on the controller's
/// published state, so without the `on_dismiss` callback that gate would
/// re-activate a node no overlay hosts any more — a menu panel or a stray handle
/// in the corner of the window.
/// P27's largest behaviour change, and the one that was asserted only in a code
/// comment: attaching `on_long_press` to the field **withdraws the tree-owned
/// long-press route** (`touch_route`'s rule 1 — a widget's own hold wins), for
/// all ten single-line surfaces. So a hold no longer opens the field's context
/// menu; the selection toolbar is the touch route to those same commands.
///
/// Both halves are asserted, because either alone would hold for the wrong
/// reason: the toolbar must be up (or the commands are unreachable by touch at
/// all), and no third overlay may be, since a context menu raised beside the
/// affordances is exactly what a route that had *not* withdrawn would produce.
#[test]
fn a_hold_raises_the_toolbar_and_not_the_fields_context_menu() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    h.render();

    let host = h.touch.layer.get().expect("the affordance host was built");
    let toolbar = h.touch.toolbar.get().expect("the toolbar was raised");
    assert!(
        !h.toolbar_actions().is_empty(),
        "the toolbar is the touch route to Cut/Copy/Paste, so it must offer them"
    );

    let up = h.tree.overlay_manager().active_ids();
    let ours: Vec<_> = [host, toolbar]
        .into_iter()
        .filter_map(|c| h.tree.overlay_manager().find_by_content(c))
        .collect();
    assert_eq!(ours.len(), 2, "both of the field's own overlays are up");
    let extra: Vec<_> = up.iter().filter(|id| !ours.contains(id)).collect();
    assert!(
        extra.is_empty(),
        "a hold raised {} overlay(s) beside the affordances, so the tree-owned \
         long-press route was not withdrawn: {extra:?}",
        extra.len()
    );
}

#[test]
fn a_right_click_retires_the_affordances_the_framework_tore_down() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    h.render();
    let host = h.touch.layer.get().expect("the affordance host was built");
    assert!(h.stack_index(host).is_some(), "raised first");

    // Clear of the affordances, so the right-click reaches the field and its
    // context-menu factory rather than a handle (which has none, and would open
    // no menu at all).
    // The **affordances'** rectangle, not the overlay's: the overlay is the whole
    // viewport now, and what this fixture needs is a point no handle covers.
    let over = super::touch::affordance_bounds(&h.touch.controller.borrow().affordances());
    let at = h.at(0);
    assert!(
        at.x < over.x,
        "the fixture assumes {at:?} is clear of the affordances {over:?}"
    );
    h.tree.pointer_down_button(at, PointerButton::Secondary);
    h.tree.pointer_up_button(at, PointerButton::Secondary);
    h.render();
    assert!(
        h.stack_index(host).is_none(),
        "the right-click did not tear the affordance overlay down, so this test \
         is not exercising the callback it exists for"
    );
    assert!(
        h.handles().is_empty(),
        "the host kept publishing handles for an overlay that is gone: {:?}",
        h.handles()
    );
}

/// A keystroke moves the affordances with the caret, and takes the toolbar down.
///
/// The controller cannot see a keystroke, so without a refresh the handles keep
/// marking where the text used to be — and the toolbar's commands would be aimed
/// at a selection the arrow key has just collapsed.
#[test]
fn a_keystroke_moves_the_handles_with_the_caret_and_retires_the_toolbar() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    h.render();
    let toolbar = h.touch.toolbar.get().expect("the toolbar was built");
    assert_eq!(h.handles().len(), 2, "a range, so two handles");
    assert!(h.tree.is_active(toolbar), "and its commands");

    h.tree.press_key(
        teksilo_core::event::Key::ArrowRight,
        teksilo_core::event::Modifiers::NONE,
    );
    h.render();

    let handles = h.handles();
    assert_eq!(handles.len(), 1, "the range collapsed: {handles:?}");
    assert_eq!(handles[0].kind, SelectionHandleKind::Caret);
    assert_eq!(
        handles[0].offset,
        h.caret(),
        "the caret handle marks the caret's new offset"
    );
    assert!(
        !h.tree.is_active(toolbar),
        "the toolbar outlived the selection it was offering commands for"
    );
}

/// Replacing the field's text retires them: the affordances marked characters
/// that are gone. The second path only the host's own `dismiss` can serve — the
/// effect that watches the bound signal has no `EventContext` either.
#[test]
fn replacing_the_text_retires_the_affordances() {
    let text = Signal::new("hello world".to_string());
    let field = TextInputField::new(text.clone());
    let mut h = Harness::new(field, "hello world");
    h.tree.focus(h.field);
    h.render();
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    assert!(!h.handles().is_empty(), "raised first");
    text.set("something else entirely".to_string());
    h.render();
    assert!(
        h.handles().is_empty(),
        "handles over text that has been replaced: {:?}",
        h.handles()
    );
}

/// Escape retires the toolbar, through the framework rather than the host — and
/// the host's published intent follows, so the menu's content does not linger as
/// a root of its own.
#[test]
fn escape_retires_the_toolbar() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    h.render();
    let toolbar = h.touch.toolbar.get().expect("the toolbar was built");
    assert!(h.stack_index(toolbar).is_some(), "raised first");
    h.tree.press_key(
        teksilo_core::event::Key::Escape,
        teksilo_core::event::Modifiers::NONE,
    );
    h.render();
    assert!(h.stack_index(toolbar).is_none(), "Escape left it up");
    assert!(
        !h.tree.is_active(toolbar),
        "its content is still active, so it is now a stray root"
    );
}

/// A caret placed by a finger reports the OS IME candidate area, so the
/// keyboard's candidate window does not cover the character being typed.
#[test]
fn a_finger_caret_reports_the_ime_area() {
    let mut h = Harness::focused("hello world");
    h.state.borrow_mut().last_ime_area = None;
    let target = h.at(5);
    let id = finger();
    h.finger_down(id, target, 0);
    h.finger_up(id, target, 30);
    let area = h.state.borrow().last_ime_area;
    let expected = h.x_of(5);
    match area {
        Some(r) => assert!(
            (r.x - expected).abs() < 0.5,
            "reported {r:?}, caret 5 is at x={expected}"
        ),
        None => panic!("no IME area was reported for the touch caret"),
    }
}

// ---------------------------------------------------------------------------
// The password exception
// ---------------------------------------------------------------------------

fn secure(reveal: Option<Signal<bool>>, echo: EchoMode) -> TextInputField {
    let mut field = TextInputField::new(Signal::new("secret".to_string())).secure(echo);
    if let Some(r) = reveal {
        field = field.revealed(r);
    }
    field
}

/// A secure field builds **no magnifier node at all**: the lens replays the
/// text layer, and while the field is revealed that layer is the password.
#[test]
fn a_secure_field_mounts_no_magnifier_node() {
    let mut h = Harness::new(secure(None, EchoMode::Masked), "secret");
    h.tree.focus(h.field);
    h.render();
    h.tree.long_press_at(PointerKind::Touch, h.at(3));
    h.render();
    assert_eq!(
        h.affordance_nodes().len(),
        3,
        "three handles and nothing else — a magnifier node would be a fourth"
    );
}

/// …and the controller refuses one too, so a host that *did* mount a lens
/// would still never be handed a request for it. Two independent switches,
/// pinned independently: either one alone would make a single
/// "no magnifier" assertion pass with the other deleted.
#[test]
fn a_secure_fields_controller_never_asks_for_a_magnifier() {
    let mut h = Harness::new(secure(None, EchoMode::Masked), "secret");
    h.tree.focus(h.field);
    h.render();
    h.tree.long_press_at(PointerKind::Touch, h.at(3));
    h.render();
    let end = h
        .handle(SelectionHandleKind::End)
        .expect("a handle is up on a masked field");
    let contact = h.tree.new_contact();
    h.tree.touch_down(contact, end.anchor);
    h.tree
        .touch_move(contact, Point::new(end.anchor.x + 8.0, end.anchor.y));
    assert!(
        h.touch.controller.borrow().magnifier_request().is_none(),
        "a lens was raised mid-drag on a secure field"
    );
}

/// A plain field does get one — otherwise the assertion above would pass on a
/// framework that had no magnifier at all.
#[test]
fn a_plain_field_does_raise_a_magnifier() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    h.render();
    let end = h.handle(SelectionHandleKind::End).expect("a handle is up");
    let contact = h.tree.new_contact();
    h.tree.touch_down(contact, end.anchor);
    h.tree
        .touch_move(contact, Point::new(end.anchor.x - 8.0, end.anchor.y));
    assert!(
        h.touch.controller.borrow().magnifier_request().is_some(),
        "no lens on a plain field mid-drag"
    );
    h.render();
    assert_eq!(h.affordance_nodes().len(), 4, "three handles plus the lens");
}

/// While masked, the toolbar offers neither Cut nor Copy — the same predicate
/// the field's own `Ctrl+C` and context menu use, so the three cannot disagree.
#[test]
fn a_masked_secure_field_offers_neither_cut_nor_copy() {
    use teksilo_core::text_touch::TextAction;
    let mut h = Harness::new(secure(None, EchoMode::Masked), "secret");
    h.tree.focus(h.field);
    h.render();
    h.tree.long_press_at(PointerKind::Touch, h.at(3));
    assert!(!h.selection().is_empty(), "a word was selected");
    assert_eq!(h.toolbar_actions(), vec![TextAction::Paste]);
}

/// Revealed, it offers them — and must, because the field's own keyboard and
/// context menu do. `TextSurface::allows_copy` on this same handle answers the
/// raw opt-in flag instead and so disagrees; see the note at
/// `FieldHitSource::allows_copy`.
#[test]
fn a_revealed_secure_field_offers_copy() {
    use teksilo_core::text_touch::TextAction;
    let reveal = Signal::new(true);
    let mut h = Harness::new(secure(Some(reveal), EchoMode::Masked), "secret");
    h.tree.focus(h.field);
    h.render();
    h.tree.long_press_at(PointerKind::Touch, h.at(3));
    let actions = h.toolbar_actions();
    assert!(
        actions.contains(&TextAction::Copy) && actions.contains(&TextAction::Cut),
        "revealed and still refusing: {actions:?}"
    );
}

/// `NoEcho` while masked lays out an **empty** engine source, so every offset
/// hit-tests to zero and every caret rectangle is the same rectangle. There is
/// no geometry to hang an affordance off, so none is raised.
#[test]
fn a_no_echo_masked_field_raises_no_affordances_at_all() {
    let mut h = Harness::new(secure(None, EchoMode::NoEcho), "secret");
    h.tree.focus(h.field);
    h.render();
    h.tree.long_press_at(PointerKind::Touch, h.at(3));
    assert!(
        h.handles().is_empty(),
        "handles over an empty layout: {:?}",
        h.handles()
    );
    assert!(h.toolbar_actions().is_empty());
    let id = finger();
    let target = Point::new(origin().x + 10.0, origin().y + 8.0);
    h.finger_down(id, target, 0);
    h.finger_up(id, target, 30);
    assert!(h.handles().is_empty(), "nor after a tap");
}

/// A rebuild does not strand the overlay it left behind, and a hold after one
/// still raises.
///
/// The overlay contents are `add_detached`, so a rebuild destroys the previous
/// set and mints a new one. What keeps the stack honest is the framework's own
/// `gc_orphaned_overlays`, which dismisses any overlay whose content is no longer
/// active — so this is a guard on a property the field depends on rather than on
/// code the field owns. It is here because the failure it describes is
/// unpleasant and silent: a stranded entry keeps the bounds its dead content was
/// last laid out with, an overlay is searched alone, and a press inside those
/// bounds is therefore dropped.
#[test]
fn a_rebuild_does_not_strand_the_overlays_the_previous_build_raised() {
    let mut h = Harness::focused("hello world");
    h.tree.long_press_at(PointerKind::Touch, h.at(8));
    h.render();
    let raised = h.tree.overlay_manager().len();
    assert_eq!(raised, 2, "the layer and the toolbar");

    for round in 0..3 {
        h.tree.arena_mark_needs_rebuild_for_testing(h.field);
        h.render();
        h.tree.long_press_at(PointerKind::Touch, h.at(8));
        h.render();
        // The stranded entry is not merely untidy: its bounds are the last ones
        // the destroyed content was laid out with, and an overlay is searched
        // alone — so a press inside them reaches a node that no longer exists and
        // is dropped. The hold that follows a rebuild is what shows it.
        assert!(
            !h.handles().is_empty(),
            "round {round}: the hold after a rebuild raised nothing"
        );
        assert_eq!(
            h.tree.overlay_manager().len(),
            raised,
            "round {round}: the overlay stack has grown"
        );
    }
}

/// The affordances are per surface, so two fields never share one layer.
#[test]
fn each_field_owns_its_own_affordance_layer() {
    let a = TextInputField::new(Signal::new("one".to_string()));
    let b = TextInputField::new(Signal::new("two".to_string()));
    let (ta, tb) = (a.touch.clone(), b.touch.clone());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(crate::primitives::VStack::new().child(a).child(b));
    tree.layout(SizeProposal::exact(400.0, 120.0));
    let _ = tree.render();
    let (la, lb) = (ta.layer.get(), tb.layer.get());
    assert!(la.is_some() && lb.is_some());
    assert_ne!(la, lb, "two fields, two layers");
}

/// A dimension the affordance geometry needs and the field did not have: the
/// viewport's **height**. `sync_viewport` used to keep only the origin and the
/// width, so a handle clamped into a zero-height rectangle never appeared.
#[test]
fn the_field_publishes_a_viewport_with_height() {
    let h = Harness::plain("hello");
    let st = h.state.borrow();
    assert!(
        st.viewport_height > 0.0,
        "the field reported a {} dp tall viewport",
        st.viewport_height
    );
    assert_eq!(
        Rect::new(
            st.viewport_origin.x,
            st.viewport_origin.y,
            st.viewport_width,
            st.viewport_height
        )
        .height,
        st.viewport_height
    );
}
