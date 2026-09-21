// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The inspector under a finger: the coarse-pointer door, the grabbable resize
//! strip, density-driven row heights, the picker, and the Pointers tab.
//!
//! Same harness as `integration_tests.rs` — the post-root wrapping hand-rolled
//! around a user root, laid out headlessly.

#![cfg(test)]

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_tokens::{PointerKind, TargetDensity};
use teksilo_widgets::Button;

use crate::shell::InspectorShell;
use crate::state::InspectorState;

const W: f32 = 800.0;
const H: f32 = 600.0;

/// A user root wrapped in the inspector shell, exactly as `state::install`
/// wraps it at runtime.
fn shell_around(tree: &mut WidgetTree, user_root: WidgetId, state: &InspectorState) -> WidgetId {
    let shell = tree.add(InspectorShell::new(user_root, state.clone()));
    let mut ids = state.user_root_ids.get();
    ids.push(user_root);
    state.user_root_ids.set(ids);
    shell
}

fn layout(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(W, H));
}

// ---------------------------------------------------------------------------
// the public door
// ---------------------------------------------------------------------------

#[test]
fn toggle_is_the_public_door_to_the_panel() {
    let state = InspectorState::new(false);
    state.toggle();
    assert!(state.open.get(), "toggle must open a closed panel");
    state.toggle();
    assert!(!state.open.get(), "and close an open one");
}

// ---------------------------------------------------------------------------
// the corner grip
// ---------------------------------------------------------------------------

/// The bottom-trailing corner of the window, well inside the grip's square.
fn corner() -> Point {
    Point::new(W - 8.0, H - 8.0)
}

#[test]
fn a_hold_in_the_corner_opens_the_inspector_for_a_finger() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let button = tree.add(Button::new(lit!("App")).on_activate_fn(|_| {}));
    let state = InspectorState::new(false);
    shell_around(&mut tree, button, &state);

    // The grip is mounted only once the session has seen a finger — which is
    // what `state::install` bridges from the tree's last-pointer-kind signal.
    state.coarse_pointer.set(true);
    layout(&mut tree);

    tree.long_press_at(PointerKind::Touch, corner());
    assert!(
        state.open.get(),
        "a hold in the corner is the inspector's coarse-pointer door"
    );
}

#[test]
fn the_grip_is_absent_until_a_finger_is_seen() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let button = tree.add(Button::new(lit!("App")).on_activate_fn(|_| {}));
    let state = InspectorState::new(false);
    shell_around(&mut tree, button, &state);
    layout(&mut tree);

    tree.long_press_at(PointerKind::Touch, corner());
    assert!(
        !state.open.get(),
        "a mouse-driven session must not grow a grip, so the hold reaches nothing"
    );
}

/// The grip's cost, measured: it fills the window but is hittable only inside
/// its corner square, so everything else still belongs to the application.
#[test]
fn the_mounted_grip_takes_its_corner_and_nothing_else() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let tapped = Rc::new(Cell::new(false));
    let t = tapped.clone();
    let button = tree.add(Button::new(lit!("App")).on_activate_fn(move |_| t.set(true)));
    let state = InspectorState::new(false);
    shell_around(&mut tree, button, &state);
    state.coarse_pointer.set(true);
    layout(&mut tree);

    // A tap on the application's own button (a `Button` sizes to its content,
    // so the point has to be its centre rather than the window's).
    let target = tree.bounds(button).center();
    tree.tap_with(PointerKind::Touch, target);
    assert!(
        tapped.get(),
        "the grip must not take a press outside its own square"
    );
    assert!(!state.open.get(), "and must not have opened the panel");

    // A tap *in* the square reaches the grip, which does nothing with it — the
    // corner's taps are the price, and the price is exactly this.
    tapped.set(false);
    tree.tap_with(PointerKind::Touch, corner());
    let _ = target;
    assert!(
        !tapped.get(),
        "inside the square the press belongs to the grip"
    );
}

// ---------------------------------------------------------------------------
// the resize strip
// ---------------------------------------------------------------------------

/// The panel's resize strip, found by its type name.
fn find_resize_handle(tree: &WidgetTree, id: WidgetId) -> Option<WidgetId> {
    if tree
        .widget_type_name(id)
        .is_some_and(|n| n.ends_with("ResizeHandle"))
    {
        return Some(id);
    }
    tree.children(id)
        .into_iter()
        .find_map(|child| find_resize_handle(tree, child))
}

/// A 6 dp strip is not a 24 dp target, and what gives it reach at Touch density
/// is the **slot**: `TouchTarget` reserves a target-sized row with the 6 dp
/// paint centred in it, and inside that slot the framework's miss-only slop pass
/// tops the strip up to it.
///
/// The fixture is discriminating twice over. The user root is a **filling,
/// tappable** widget, so without the slot the exact hit above the strip has an
/// eligible handler containing it at distance zero and the slop pass cannot
/// serve the press at all (A10's redundancy note) — which is what makes the slot
/// the mechanism under test rather than a decoration. And the mouse is asserted
/// to keep the app's press, so the reach is a coarse pointer's alone.
#[test]
fn a_finger_grabs_the_resize_strip_from_outside_its_paint() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let app = tree.add(teksilo_core::widget_builder::WidgetBuilder::on_tap(
        teksilo_widgets::primitives::RectWidget::new(),
        |_e: &teksilo_core::gesture::TapEvent, _c: &mut teksilo_core::widget::EventContext| {},
    ));
    let state = InspectorState::new(true);
    let shell = shell_around(&mut tree, app, &state);
    tree.set_density(TargetDensity::Touch);
    layout(&mut tree);

    let handle = find_resize_handle(&tree, shell).expect("the panel has a resize strip");
    let bounds = tree.bounds(handle);
    assert!(
        (bounds.height - crate::resize_handle::HANDLE_HEIGHT).abs() < 0.01,
        "the strip keeps its painted height at every density, got {bounds:?}"
    );

    // 4 dp above the paint: inside the target-sized slot, outside the strip.
    let above = Point::new(bounds.x + bounds.width * 0.5, bounds.y - 4.0);
    let finger = teksilo_core::pointer::PointerInfo::touch(
        tree.new_contact(),
        teksilo_core::pointer::EventTime::ZERO,
    );
    assert_eq!(
        tree.hit_test_for(above, &finger),
        Some(handle),
        "a finger's press just above the paint must resolve to the strip"
    );

    // The same point, by mouse: a mouse hot-spot is exact, the outset is zero
    // for it, and the press belongs to the application.
    let mouse = teksilo_core::pointer::PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO);
    assert_ne!(
        tree.hit_test_for(above, &mouse),
        Some(handle),
        "a mouse must not gain reach it never had"
    );
}

/// The slot has to be *reserved*, not just requested: the shell adds the strip's
/// slot height to the block it gives the panel, and reserving the 6 dp paint
/// instead would squeeze the panel by the difference at Touch.
#[test]
fn the_panel_keeps_its_full_height_beside_a_target_sized_strip() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let app = tree.add(Button::new(lit!("App")).on_activate_fn(|_| {}));
    let state = InspectorState::new(true);
    let shell = shell_around(&mut tree, app, &state);
    tree.set_density(TargetDensity::Touch);
    layout(&mut tree);

    // The panel keeps its own fixed height whatever happens, so what an
    // under-reservation actually produces is a block too tall for the space it
    // was given: the panel's bottom edge slides off the window.
    let panel = find_by_type(&tree, shell, "Panel").expect("the open panel is in the tree");
    let bounds = tree.bounds(panel);
    let want = state.panel_height.get();
    assert!(
        (bounds.height - want).abs() < 0.51,
        "the panel must still get its {want} dp, got {bounds:?}"
    );
    assert!(
        bounds.bottom() <= H + 0.51,
        "…and must still end at the window's bottom edge, not {} past it: {bounds:?}",
        bounds.bottom() - H
    );
}

/// Compact is untouched: the slot is the paint, so the panel a mouse sees is the
/// panel it always saw.
#[test]
fn at_compact_the_strip_reserves_nothing_beyond_its_paint() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let app = tree.add(Button::new(lit!("App")).on_activate_fn(|_| {}));
    let state = InspectorState::new(true);
    let shell = shell_around(&mut tree, app, &state);
    layout(&mut tree);

    let handle = find_resize_handle(&tree, shell).expect("the panel has a resize strip");
    let slot = tree
        .parent(handle)
        .expect("the strip sits in a slot wrapper");
    assert!(
        (tree.bounds(slot).height - crate::resize_handle::HANDLE_HEIGHT).abs() < 0.01,
        "at Compact the slot must be the paint: {:?}",
        tree.bounds(slot)
    );
}

#[test]
fn the_resize_strip_answers_the_accessibility_actions_it_advertises() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let button = tree.add(Button::new(lit!("App")).on_activate_fn(|_| {}));
    let state = InspectorState::new(true);
    let shell = shell_around(&mut tree, button, &state);
    layout(&mut tree);

    let handle = find_resize_handle(&tree, shell).expect("the panel has a resize strip");
    let before = state.panel_height.get();
    dispatch_action(
        &mut tree,
        handle,
        teksilo_core::accesskit::Action::Increment,
    );
    assert!(
        state.panel_height.get() > before,
        "Increment must grow the panel: {} → {}",
        before,
        state.panel_height.get()
    );
    let grown = state.panel_height.get();
    dispatch_action(
        &mut tree,
        handle,
        teksilo_core::accesskit::Action::Decrement,
    );
    assert!(
        state.panel_height.get() < grown,
        "Decrement must shrink it again"
    );
}

// ---------------------------------------------------------------------------
// density-driven rows
// ---------------------------------------------------------------------------

/// The first widget of a given type in a subtree.
fn find_by_type(tree: &WidgetTree, id: WidgetId, suffix: &str) -> Option<WidgetId> {
    if tree
        .widget_type_name(id)
        .is_some_and(|n| n.ends_with(suffix))
    {
        return Some(id);
    }
    tree.children(id)
        .into_iter()
        .find_map(|child| find_by_type(tree, child, suffix))
}

/// The Tree tab's row band, found by its type name.
fn find_tree_rows(tree: &WidgetTree, id: WidgetId) -> Option<WidgetId> {
    if tree
        .widget_type_name(id)
        .is_some_and(|n| n.ends_with("TreeRows"))
    {
        return Some(id);
    }
    tree.children(id)
        .into_iter()
        .find_map(|child| find_tree_rows(tree, child))
}

#[test]
fn compact_inspector_rows_are_unchanged_and_touch_rows_reach_the_target() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let button = tree.add(Button::new(lit!("App")).on_activate_fn(|_| {}));
    let state = InspectorState::new(true);
    let shell = shell_around(&mut tree, button, &state);
    layout(&mut tree);

    let rows = find_tree_rows(&tree, shell).expect("the Tree tab has a row band");
    let compact_height = tree.bounds(rows).height;
    let count = (compact_height / 18.0).round();
    assert!(
        count >= 1.0 && (compact_height - count * 18.0).abs() < 0.01,
        "Compact rows stay at 18 dp: {compact_height} is not a whole number of them"
    );

    tree.set_density(TargetDensity::Touch);
    layout(&mut tree);
    // A density switch REBUILDS, so every id from before it is stale — which is
    // the mechanism working, not a wrinkle in the test. Find the band again.
    let rows = find_tree_rows(&tree, shell).expect("the Tree tab still has a row band");
    let touch_height = tree.bounds(rows).height;
    let target = tree.theme().input.target_size;
    assert!(
        (touch_height - count * target).abs() < 0.01,
        "at Touch the same {count} rows must be {target} dp each, got {touch_height}"
    );
}

/// The half that would break silently: the press-to-row division has to use the
/// height the paint used, or a press at Touch selects the wrong row.
///
/// Which row is which is knowable, so the test names them rather than merely
/// asking that two presses differ: the Tree tab lists the user subtree
/// depth-first from the user root, so row 0 **is** the user root and row 1 is
/// its first child. A division that used 18 dp against a 44 dp paint maps the
/// first row's centre onto row 1.
#[test]
fn a_finger_selects_the_row_it_landed_on_at_touch_density() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let button = tree.add(Button::new(lit!("App")).on_activate_fn(|_| {}));
    let state = InspectorState::new(true);
    let shell = shell_around(&mut tree, button, &state);
    tree.set_density(TargetDensity::Touch);
    layout(&mut tree);

    let rows = find_tree_rows(&tree, shell).expect("the Tree tab has a row band");
    let band = tree.bounds(rows);
    let row_height = tree.theme().input.target_size;
    assert!(
        band.height >= row_height * 2.0,
        "the fixture needs at least two rows, got {band:?}"
    );

    let first_child = *tree
        .children(button)
        .first()
        .expect("the Button composite has a child, which is row 1");

    tree.tap_with(
        PointerKind::Touch,
        Point::new(band.x + 10.0, band.y + row_height * 0.5),
    );
    layout(&mut tree);
    assert_eq!(
        state.selected_id.get(),
        Some(button),
        "the first row is the user root itself"
    );

    tree.tap_with(
        PointerKind::Touch,
        Point::new(band.x + 10.0, band.y + row_height * 1.5),
    );
    layout(&mut tree);
    assert_eq!(
        state.selected_id.get(),
        Some(first_child),
        "and the second row is its first child"
    );
}

// ---------------------------------------------------------------------------
// the picker
// ---------------------------------------------------------------------------

/// The picker resolves what the developer aimed at, and a finger aims the same
/// way a mouse does: the pick is an exact hit test at the press position for
/// both, deliberately — a coarse-pointer widening would report the node the
/// *framework* would have activated instead of the one under the finger.
#[test]
fn the_picker_resolves_the_same_node_by_touch_and_by_mouse() {
    /// The picked node, as `(id, type label)` — two runs build two trees, and
    /// the ids happen to match only because the trees are built identically;
    /// the label is what makes the comparison meaningful.
    fn pick_with(kind: PointerKind) -> (Option<WidgetId>, Option<String>) {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let button = tree.add(Button::new(lit!("App")).on_activate_fn(|_| {}));
        let state = InspectorState::new(false);
        shell_around(&mut tree, button, &state);
        layout(&mut tree);

        state.picker_mode.set(true);
        layout(&mut tree);
        // The user root is a `Button`, which sizes to its content: the pick has
        // to land on it, not on the middle of the window.
        let at = tree.bounds(button).center();

        match kind {
            PointerKind::Touch => {
                let finger = tree.new_contact();
                tree.touch_down(finger, at);
                // `PickResolver` resolves the stashed point during layout.
                layout(&mut tree);
                tree.touch_up(finger, at);
            }
            _ => {
                tree.pointer_move(at);
                tree.pointer_down_button(at, teksilo_core::event::PointerButton::Primary);
                layout(&mut tree);
                tree.pointer_up_button(at, teksilo_core::event::PointerButton::Primary);
            }
        }
        layout(&mut tree);
        let picked = state.pending_pick_chain.get().first().cloned();
        (picked.as_ref().map(|e| e.id), picked.map(|e| e.label))
    }

    let by_touch = pick_with(PointerKind::Touch);
    let by_mouse = pick_with(PointerKind::Mouse);
    assert!(
        by_touch.0.is_some(),
        "a finger must be able to pick at all: {by_touch:?}"
    );
    assert_eq!(
        by_touch, by_mouse,
        "the picker must resolve the same node whichever device pressed"
    );
}

// ---------------------------------------------------------------------------
// the Pointers tab
// ---------------------------------------------------------------------------

#[test]
fn the_pointers_tab_lists_a_declared_pan_claim_and_the_active_density() {
    use crate::tabs::pointers::PointersTab;

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    // A `ScrollArea` is the reference pan claimant: it declares both axes.
    let user = tree.add(
        teksilo_widgets::ScrollArea::new()
            .child(teksilo_widgets::primitives::MinSize::new(0.0, 2000.0)),
    );
    let state = InspectorState::new(true);
    state.user_root_ids.set(vec![user]);
    let tab = tree.add(PointersTab::new(state.clone()));
    layout(&mut tree);

    let lines = tab_lines(&tree, tab);
    let joined = lines.join("\n");
    assert!(
        joined.contains("density=Compact"),
        "the tab reports the density in force:\n{joined}"
    );
    assert!(
        lines.iter().any(|l| l.contains("pan=")),
        "a declared pan claim must be listed:\n{joined}"
    );
    assert!(
        joined.contains("watch: off"),
        "and the watch must say it is off:\n{joined}"
    );
}

#[test]
fn the_pointers_tab_renders_two_live_contacts() {
    use crate::tabs::pointers::{PointerRow, PointersTab};

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let user = tree.add(Button::new(lit!("App")).on_activate_fn(|_| {}));
    let state = InspectorState::new(true);
    state.user_root_ids.set(vec![user]);
    state.pointer_watch.set(true);
    state.pointer_rows.set(vec![
        PointerRow {
            id: 7,
            kind: PointerKind::Touch,
            position: Point::new(100.0, 200.0),
            down: true,
            touch_action: "AUTO".to_string(),
            pressed: true,
        },
        PointerRow {
            id: 8,
            kind: PointerKind::Touch,
            position: Point::new(300.0, 220.0),
            down: true,
            touch_action: "AUTO".to_string(),
            pressed: false,
        },
    ]);
    let tab = tree.add(PointersTab::new(state.clone()));
    layout(&mut tree);

    let lines = tab_lines(&tree, tab);
    let joined = lines.join("\n");
    assert!(
        joined.contains("#7 Touch at (100,200)"),
        "the first contact must be rendered:\n{joined}"
    );
    assert!(
        joined.contains("#8 Touch at (300,220)"),
        "…and so must the second:\n{joined}"
    );
    assert!(
        joined.contains("2 live"),
        "the header counts them:\n{joined}"
    );
}

/// The other half: the watch is what puts live contacts on that list, and two
/// fingers must both appear — a surface that served only the first contact
/// would report one.
#[test]
fn the_armed_watch_reports_both_fingers() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let button = tree.add(Button::new(lit!("App")).on_activate_fn(|_| {}));
    let state = InspectorState::new(true);
    shell_around(&mut tree, button, &state);
    state.pointer_watch.set(true);
    layout(&mut tree);

    let a = tree.new_contact();
    let b = tree.new_contact();
    tree.touch_down(a, Point::new(200.0, 200.0));
    tree.touch_down(b, Point::new(400.0, 240.0));

    let rows = state.pointer_rows.get();
    assert_eq!(
        rows.len(),
        2,
        "both contacts must be reported, got {rows:?}"
    );
    assert!(
        rows.iter().all(|r| r.kind == PointerKind::Touch && r.down),
        "and reported as fingers that are down: {rows:?}"
    );

    tree.touch_up(a, Point::new(200.0, 200.0));
    assert_eq!(
        state.pointer_rows.get().len(),
        1,
        "a lifted contact ceases to exist"
    );
    tree.touch_up(b, Point::new(400.0, 240.0));
    assert!(
        state.pointer_rows.get().is_empty(),
        "and so does the second"
    );
}

/// While the watch is armed the application receives nothing — which is the cost
/// the toolbar's label and the docs both state, and the reason the probe answers
/// every pointer event instead of merely observing it.
#[test]
fn an_armed_watch_takes_the_applications_input() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let tapped = Rc::new(Cell::new(false));
    let t = tapped.clone();
    let button = tree.add(Button::new(lit!("App")).on_activate_fn(move |_| t.set(true)));
    let state = InspectorState::new(true);
    shell_around(&mut tree, button, &state);
    state.pointer_watch.set(true);
    layout(&mut tree);

    let target = tree.bounds(button).center();
    tree.tap_with(PointerKind::Touch, target);
    assert!(
        !tapped.get(),
        "an armed watch must take the press, not share it"
    );
    assert_eq!(
        state.pointer_rows.get().len(),
        0,
        "and the lifted contact leaves no row behind"
    );
}

#[test]
fn the_watch_is_off_by_default_so_the_app_keeps_its_input() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let tapped = Rc::new(Cell::new(false));
    let t = tapped.clone();
    let button = tree.add(Button::new(lit!("App")).on_activate_fn(move |_| t.set(true)));
    let state = InspectorState::new(true);
    shell_around(&mut tree, button, &state);
    layout(&mut tree);

    assert!(!state.pointer_watch.get(), "the watch starts disarmed");
    tree.tap_with(PointerKind::Touch, tree.bounds(button).center());
    assert!(
        tapped.get(),
        "with the watch off the application receives its own presses"
    );
}

/// Invoke an accessibility action on a widget, the way a screen reader would.
fn dispatch_action(tree: &mut WidgetTree, id: WidgetId, action: teksilo_core::accesskit::Action) {
    let node = teksilo_core::accessibility::widget_id_to_node_id(id);
    tree.dispatch_access_action(node, action, None, &mut teksilo_core::NoopWindowOps);
}

/// Read a `PointersTab`'s rendered lines through the `as_any` hook — the
/// sanctioned way for a test to reach a mounted widget's own state.
fn tab_lines(tree: &WidgetTree, id: WidgetId) -> Vec<String> {
    tree.widget_as_any(id)
        .and_then(|any| any.downcast_ref::<crate::tabs::pointers::PointersTab>())
        .map(|tab| tab.lines_for_test())
        .expect("the Pointers tab exposes its lines through as_any")
}

/// A density switch rebuilds every root, and the inspector's shell **is** the
/// only root once it has wrapped the application. Measured before the fix: the
/// wrapped root came back destroyed (`widget_type_name() == None`) and the
/// window was left showing inspector chrome over nothing.
#[test]
fn a_density_switch_does_not_destroy_the_application_the_shell_wrapped() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let button = tree.add(Button::new(lit!("App")).on_activate_fn(|_| {}));
    let state = InspectorState::new(true);
    shell_around(&mut tree, button, &state);
    layout(&mut tree);
    let before = tree.bounds(button);
    assert!(before.width > 0.0, "the application starts laid out");

    tree.set_density(TargetDensity::Touch);
    layout(&mut tree);

    assert!(
        tree.widget_type_name(button).is_some(),
        "the application's own root must survive a rebuild of the shell around it"
    );
    assert_eq!(
        tree.bounds(button).size(),
        before.size(),
        "and must still be laid out — it moves, because the panel's own chrome \
         grew at the new density, but it is the same widget at the same size"
    );
}
