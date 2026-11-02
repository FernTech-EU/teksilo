// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Widget-level integration tests for `Splitter` (headless `WidgetTree`).
//! The pure sizing engine is unit-tested in `distribute.rs`; the model in
//! `model.rs`. Here we cover placement, drag, keyboard, collapse triggers,
//! RTL, clipping, and accessibility through a real arena.

use super::*;
use teksilo_canvas::{Point, Size, SizeProposal};
use teksilo_core::event::{Key, Modifiers, PointerButton};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_tree::WidgetTree;

#[derive(Debug)]
struct FixedLeaf(f32, f32);

impl Widget for FixedLeaf {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(self.0, self.1).into()
    }
}

fn theme_tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

/// Two-pane horizontal model with exact sizes, `min = 0`, `stretch = 0`
/// (so `distribute` returns the sizes unchanged when they fill `available`).
fn h_model(sizes: &[f32]) -> SplitterModel {
    SplitterModel::from_panes(
        sizes
            .iter()
            .map(|&s| PaneDescriptor::new().size(s).min_size(0.0).stretch(0.0))
            .collect(),
        Orientation::Horizontal,
    )
}

#[test]
fn collapsed_pane_with_collapsed_size_keeps_a_sliver_live() {
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new()
                .size(200.0)
                .min_size(0.0)
                .stretch(0.0)
                .collapsed_size(32.0),
            PaneDescriptor::new().size(200.0).min_size(0.0).stretch(1.0),
        ],
        Orientation::Vertical,
    );
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(80.0, 300.0))
            .pane(FixedLeaf(80.0, 100.0)),
    );
    tree.layout(SizeProposal::exact(200.0, 400.0));

    model.set_collapsed_immediate(0, true);
    tree.layout(SizeProposal::exact(200.0, 400.0));

    let pane0 = tree.child_widget(root, 0);
    let pb = tree.bounds(pane0);
    assert!(
        pb.height > 20.0 && pb.height < 45.0,
        "collapsed pane folds to ~32 (collapsed_size), got {}",
        pb.height
    );
    // The pane's content must stay LIVE (clipped sliver), not dormant.
    let content = tree.child_widget(pane0, 0);
    assert!(
        tree.bounds(content).height > 1.0,
        "collapsed-pane content (the sliver) must stay live, got {}",
        tree.bounds(content).height
    );
}

#[test]
fn horizontal_places_panes_and_dividers() {
    let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
    let model = h_model(&[avail * 0.25, avail * 0.75]);
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model)
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));

    let first = tree.child_widget(root, 0);
    let handle = tree.child_widget(root, 1);
    let second = tree.child_widget(root, 2);
    assert!((tree.bounds(first).width - avail * 0.25).abs() < 0.5);
    assert!((tree.bounds(handle).width - SPLITTER_GUTTER_THICKNESS).abs() < 0.01);
    assert!((tree.bounds(second).width - avail * 0.75).abs() < 0.5);
}

#[test]
fn vertical_places_panes_and_dividers() {
    let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new()
                .size(avail * 0.25)
                .min_size(0.0)
                .stretch(0.0),
            PaneDescriptor::new()
                .size(avail * 0.75)
                .min_size(0.0)
                .stretch(0.0),
        ],
        Orientation::Vertical,
    );
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model)
            .pane(FixedLeaf(80.0, 100.0))
            .pane(FixedLeaf(80.0, 100.0)),
    );
    tree.layout(SizeProposal::exact(200.0, 400.0));

    let first = tree.child_widget(root, 0);
    let second = tree.child_widget(root, 2);
    assert!((tree.bounds(first).height - avail * 0.25).abs() < 0.5);
    assert!((tree.bounds(second).height - avail * 0.75).abs() < 0.5);
}

#[test]
fn three_panes_have_two_handles() {
    let model = SplitterModel::new(3, Orientation::Horizontal);
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model)
            .pane(FixedLeaf(50.0, 40.0))
            .pane(FixedLeaf(50.0, 40.0))
            .pane(FixedLeaf(50.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(600.0, 200.0));
    // children: pane,handle,pane,handle,pane = 5
    assert_eq!(tree.children(root).len(), 5);
    // Equal thirds (equal min floors + equal stretch slack).
    let avail = 600.0 - 2.0 * SPLITTER_GUTTER_THICKNESS;
    for idx in [0usize, 2, 4] {
        let w = tree.bounds(tree.child_widget(root, idx)).width;
        assert!((w - avail / 3.0).abs() < 1.0, "pane {idx} width {w}");
    }
}

#[test]
fn drag_moves_the_boundary() {
    let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
    let model = h_model(&[avail * 0.5, avail * 0.5]);
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));

    let handle = tree.child_widget(root, 1);
    let start = tree.bounds(handle).center();
    tree.drag(start, Point::new(start.x + 80.0, start.y));

    assert!(
        model.stored_size(0) > avail * 0.5 + 50.0,
        "pane0 should grow, got {}",
        model.stored_size(0)
    );
}

#[test]
fn drag_tracks_cursor_across_relayouts() {
    // Regression: pointer events arrive localized to the *handle*, which
    // slides as the divider is dragged. The drag must recover
    // window-absolute coords, or the size chases a moving target (lag +
    // jitter). Move in steps, relaying out between each (as the real app
    // does on every bound `version` change), and assert the divider lands
    // at the cursor — not behind it.
    let model = h_model(&[300.0, 300.0]);
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model)
            .pane(FixedLeaf(50.0, 40.0))
            .pane(FixedLeaf(50.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(600.0, 200.0));

    let handle = tree.child_widget(root, 1);
    let start = tree.bounds(handle).center();
    tree.pointer_down_button(start, PointerButton::Primary);
    let target_x = 440.0;
    for step_x in [start.x + 40.0, start.x + 90.0, target_x] {
        tree.pointer_move(Point::new(step_x, start.y));
        tree.layout(SizeProposal::exact(600.0, 200.0));
    }
    tree.pointer_up_button(Point::new(target_x, start.y), PointerButton::Primary);

    let container = tree.bounds(root);
    let expected_pane0 = target_x - container.x - SPLITTER_GUTTER_THICKNESS / 2.0;
    let pane0 = tree.bounds(tree.child_widget(root, 0)).width;
    assert!(
        (pane0 - expected_pane0).abs() < 4.0,
        "divider should track the cursor across relayouts: pane0={pane0}, expected~{expected_pane0}"
    );
}

#[test]
fn minimum_sizes_clamp() {
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new()
                .size(10.0)
                .min_size(120.0)
                .stretch(0.0),
            PaneDescriptor::new()
                .size(10.0)
                .min_size(120.0)
                .stretch(0.0),
        ],
        Orientation::Horizontal,
    );
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model)
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(300.0, 160.0));
    assert!(tree.bounds(tree.child_widget(root, 0)).width >= 119.5);
    assert!(tree.bounds(tree.child_widget(root, 2)).width >= 119.5);
}

#[test]
fn keyboard_resizes_focused_handle() {
    let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
    let model = h_model(&[avail * 0.5, avail * 0.5]);
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));

    let handle = tree.child_widget(root, 1);
    tree.focus(handle);
    let before = model.stored_size(0);
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    assert!(model.stored_size(0) > before, "arrow-right grows pane0");
    assert_eq!(tree.focused(), Some(handle));
}

#[test]
fn rtl_mirrors_the_resize_arrows() {
    // The pane order and the drag math have been mirrored since this widget
    // shipped — `Splitter::place_children` writes the direction into the cell
    // the drag path reads — but the keyboard was not, so `ArrowRight` pulled
    // the divider the wrong way in a right-to-left window. Qt flips on
    // `isRightToLeft()`, and only the horizontal pair.
    let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
    let model = h_model(&[avail * 0.5, avail * 0.5]);
    let mut tree = theme_tree();
    tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));

    let handle = tree.child_widget(root, 1);
    tree.focus(handle);
    let before = model.stored_size(0);
    tree.press_key(Key::ArrowLeft, Modifiers::NONE);
    assert!(
        model.stored_size(0) > before,
        "under RTL the leading pane grows towards the left edge"
    );

    let mid = model.stored_size(0);
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    assert!(model.stored_size(0) < mid, "and shrinks the other way");
}

#[test]
fn home_and_end_drive_the_boundary_to_its_limits() {
    let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
    let model = h_model(&[avail * 0.5, avail * 0.5]);
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));

    let handle = tree.child_widget(root, 1);
    tree.focus(handle);
    tree.press_key(Key::Home, Modifiers::NONE);
    let at_min = model.stored_size(0);
    tree.press_key(Key::End, Modifiers::NONE);
    assert!(
        model.stored_size(0) > at_min,
        "End gives pane 0 everything Home took away"
    );
}

#[test]
fn page_keys_are_not_a_splitter_chord() {
    // The ARIA window-splitter pattern asks only for `Home` and `End`, and
    // `QSplitterHandle` binds no page keys: a divider has no unit a page could
    // be a multiple of, and the panes either side own their own paging.
    let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
    let model = h_model(&[avail * 0.5, avail * 0.5]);
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));

    let handle = tree.child_widget(root, 1);
    tree.focus(handle);
    let before = model.stored_size(0);
    tree.press_key(Key::PageUp, Modifiers::NONE);
    tree.press_key(Key::PageDown, Modifiers::NONE);
    assert_eq!(model.stored_size(0), before);
}

#[test]
fn an_accelerator_chord_does_not_resize() {
    // Behaviour change: modifiers used to be ignored, so `Ctrl+ArrowRight`
    // resized the split and swallowed the chord.
    let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
    let model = h_model(&[avail * 0.5, avail * 0.5]);
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));

    let handle = tree.child_widget(root, 1);
    tree.focus(handle);
    let before = model.stored_size(0);
    for (key, mods) in [
        (Key::ArrowRight, Modifiers::CTRL),
        (Key::Home, Modifiers::ALT),
        (Key::End, Modifiers::SUPER),
    ] {
        tree.press_key(key, mods);
        assert_eq!(
            model.stored_size(0),
            before,
            "{key:?} with {mods:?} must fall through"
        );
    }
}

#[test]
fn rtl_horizontal_mirrors_pane_order() {
    let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
    let model = h_model(&[avail * 0.3, avail * 0.7]);
    let mut tree = theme_tree();
    tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
    let root = tree.add(
        Splitter::new(model)
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));
    // Model index 0 is the leading pane → on the RIGHT under RTL.
    let first = tree.bounds(tree.child_widget(root, 0));
    let second = tree.bounds(tree.child_widget(root, 2));
    assert!(first.x > second.x, "leading pane should sit at the right");
}

#[test]
fn rtl_vertical_still_stacks_top_to_bottom() {
    let model = SplitterModel::new(2, Orientation::Vertical);
    let mut tree = theme_tree();
    tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
    let root = tree.add(
        Splitter::new(model)
            .pane(FixedLeaf(80.0, 100.0))
            .pane(FixedLeaf(80.0, 100.0)),
    );
    tree.layout(SizeProposal::exact(200.0, 400.0));
    let first = tree.bounds(tree.child_widget(root, 0));
    let second = tree.bounds(tree.child_widget(root, 2));
    assert!(
        first.y < second.y,
        "first pane stays above under RTL+vertical"
    );
}

#[test]
fn panes_are_wrapped_in_clip_containers() {
    let model = SplitterModel::new(2, Orientation::Horizontal);
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model)
            .pane(FixedLeaf(500.0, 40.0))
            .pane(FixedLeaf(500.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));
    assert_eq!(tree.children(tree.child_widget(root, 0)).len(), 1);
    assert_eq!(tree.children(tree.child_widget(root, 2)).len(), 1);
}

#[test]
fn handle_exposes_splitter_accessibility() {
    let model = SplitterModel::new(2, Orientation::Horizontal);
    let mut tree = theme_tree();
    tree.add(
        Splitter::new(model)
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));
    let handle = tree
        .find_by_role(teksilo_core::accesskit::Role::Splitter)
        .unwrap();
    let info = tree.accessibility_node(handle);
    assert_eq!(info.role(), teksilo_core::accesskit::Role::Splitter);
    assert!(
        info.actions()
            .contains(&teksilo_core::accesskit::Action::Increment)
    );
}

#[test]
fn programmatic_collapse_shrinks_pane_to_zero() {
    let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new()
                .size(avail * 0.5)
                .collapsible(true)
                .stretch(0.0),
            PaneDescriptor::new()
                .size(avail * 0.5)
                .min_size(0.0)
                .stretch(0.0),
        ],
        Orientation::Horizontal,
    );
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));

    model.set_collapsed(0, true);
    tree.tick_animations(std::time::Duration::from_millis(400));
    tree.layout(SizeProposal::exact(400.0, 200.0));

    assert!(
        tree.bounds(tree.child_widget(root, 0)).width < 5.0,
        "collapsed pane should be ~0, got {}",
        tree.bounds(tree.child_widget(root, 0)).width
    );
    // The freed space flows to the surviving pane.
    assert!(tree.bounds(tree.child_widget(root, 2)).width > avail - 5.0);
}

#[test]
fn enter_on_focused_handle_toggles_adjacent_collapse() {
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new().collapsible(true),
            PaneDescriptor::new(),
        ],
        Orientation::Horizontal,
    );
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));
    let handle = tree.child_widget(root, 1);
    tree.focus(handle);
    assert!(!model.is_collapsed(0));
    tree.press_key(Key::Enter, Modifiers::NONE);
    assert!(
        model.is_collapsed(0),
        "Enter collapses the collapsible pane"
    );
}

#[test]
fn a_modified_enter_on_a_handle_belongs_to_the_application() {
    // `Enter` was matched before any modifier test, so `Ctrl+Enter` collapsed
    // the pane and reported the key handled — the one row of
    // `docs/range-keyboard.md` ("`Ctrl` / `Alt` / `Super` + any → falls
    // through") the divider did not honour, while every other key on it did.
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new().collapsible(true),
            PaneDescriptor::new(),
        ],
        Orientation::Horizontal,
    );
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));
    let handle = tree.child_widget(root, 1);
    tree.focus(handle);

    for mods in [Modifiers::CTRL, Modifiers::ALT, Modifiers::SUPER] {
        tree.press_key(Key::Enter, mods);
        assert!(
            !model.is_collapsed(0),
            "Enter with {mods:?} must fall through to the application"
        );
    }
}

#[test]
fn double_click_handle_toggles_adjacent_collapse() {
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new().collapsible(true),
            PaneDescriptor::new(),
        ],
        Orientation::Horizontal,
    );
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));
    let center = tree.bounds(tree.child_widget(root, 1)).center();
    // Two taps at the same spot → on_double_tap.
    tree.pointer_down_button(center, PointerButton::Primary);
    tree.pointer_up_button(center, PointerButton::Primary);
    tree.pointer_down_button(center, PointerButton::Primary);
    tree.pointer_up_button(center, PointerButton::Primary);
    assert!(model.is_collapsed(0), "double-click collapses the pane");
}

#[test]
fn drag_past_min_snaps_to_collapsed() {
    let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new().size(avail * 0.5).collapsible(true),
            PaneDescriptor::new().size(avail * 0.5),
        ],
        Orientation::Horizontal,
    );
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));
    let handle = tree.child_widget(root, 1);
    let start = tree.bounds(handle).center();
    // Drag the divider hard to the leading edge → pane0 below min−snap.
    tree.drag(start, Point::new(8.0, start.y));
    assert!(
        model.is_collapsed(0),
        "dragging past min snaps to collapsed"
    );
}

#[test]
fn drag_restores_collapsed_pane_and_keeps_it_open() {
    // Regression: pulling a collapsed pane's divider out must restore it and
    // let it track the cursor — not pop open and immediately snap shut
    // because the cursor is still in the collapse zone (inverted hysteresis).
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new()
                .collapsible(true)
                .collapsed(true)
                .min_size(96.0),
            PaneDescriptor::new().min_size(0.0),
        ],
        Orientation::Horizontal,
    );
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(50.0, 40.0))
            .pane(FixedLeaf(50.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));
    assert!(model.is_collapsed(0));

    // Grab the (leading-edge) handle and pull it out in steps, relaying out
    // between each so the handle slides under the cursor.
    let handle = tree.child_widget(root, 1);
    let start = tree.bounds(handle).center();
    tree.pointer_down_button(start, PointerButton::Primary);
    for x in [40.0, 120.0, 210.0] {
        tree.pointer_move(Point::new(x, start.y));
        tree.layout(SizeProposal::exact(400.0, 200.0));
    }
    tree.pointer_up_button(Point::new(210.0, start.y), PointerButton::Primary);

    assert!(
        !model.is_collapsed(0),
        "pane should be restored, not re-collapsed"
    );
    assert!(
        tree.bounds(tree.child_widget(root, 0)).width > 150.0,
        "restored pane should track the cursor, got {}",
        tree.bounds(tree.child_widget(root, 0)).width
    );
}

#[test]
fn collapsed_pane_content_is_dormant() {
    // A collapsed pane's content must be parked dormant — out of paint, the
    // focus order, hit-test, and the a11y tree — not merely sized to zero.
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new().collapsible(true),
            PaneDescriptor::new(),
        ],
        Orientation::Horizontal,
    );
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(80.0, 40.0))
            .pane(FixedLeaf(80.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));

    let clip0 = tree.child_widget(root, 0);
    let inner0 = tree.child_widget(clip0, 0);
    assert!(tree.is_active(inner0), "expanded pane content is active");

    model.set_collapsed(0, true);
    tree.tick_animations(std::time::Duration::from_millis(400));
    tree.layout(SizeProposal::exact(400.0, 200.0));
    assert!(
        !tree.is_active(inner0),
        "collapsed pane content must be dormant (excluded from AT / focus)"
    );

    // Re-expanding reactivates it.
    model.set_collapsed(0, false);
    tree.tick_animations(std::time::Duration::from_millis(400));
    tree.layout(SizeProposal::exact(400.0, 200.0));
    assert!(
        tree.is_active(inner0),
        "re-expanded content must reactivate"
    );
}

#[test]
fn collapsing_content_keeps_full_layout_size() {
    // Mid-collapse, the content must stay laid out at the pane's full size
    // (and be clipped) rather than reflow into the shrinking pane.
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new()
                .size(200.0)
                .min_size(0.0)
                .stretch(0.0)
                .collapsible(true),
            PaneDescriptor::new().size(194.0).min_size(0.0).stretch(0.0),
        ],
        Orientation::Horizontal,
    );
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(50.0, 40.0))
            .pane(FixedLeaf(50.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));
    let clip0 = tree.child_widget(root, 0);
    let inner0 = tree.child_widget(clip0, 0);

    model.set_collapsed(0, true);
    // Tick to roughly the middle of the collapse tween.
    tree.tick_animations(std::time::Duration::from_millis(80));
    tree.layout(SizeProposal::exact(400.0, 200.0));

    let clip_w = tree.bounds(clip0).width;
    let inner_w = tree.bounds(inner0).width;
    assert!(clip_w < 190.0, "clip should be shrinking, got {clip_w}");
    assert!(
        inner_w > clip_w + 20.0,
        "content should keep its full width ({inner_w}) while the clip shrinks ({clip_w})"
    );
}

#[test]
fn set_pane_visible_immediate_needs_no_tick() {
    let model = SplitterModel::new(2, Orientation::Horizontal);
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(50.0, 40.0))
            .pane(FixedLeaf(50.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));

    // No `tick_animations`: a single layout must already show the change.
    model.set_pane_visible_immediate(0, false);
    tree.layout(SizeProposal::exact(400.0, 200.0));
    assert!(
        tree.bounds(tree.child_widget(root, 0)).width < 2.0,
        "hidden at once, got {}",
        tree.bounds(tree.child_widget(root, 0)).width
    );
    assert!(
        tree.bounds(tree.child_widget(root, 2)).width > 396.0,
        "the remaining pane fills the container at once"
    );

    model.set_pane_visible_immediate(0, true);
    tree.layout(SizeProposal::exact(400.0, 200.0));
    assert!(
        tree.bounds(tree.child_widget(root, 0)).width > 100.0,
        "shown again at once, got {}",
        tree.bounds(tree.child_widget(root, 0)).width
    );
}

#[test]
fn hiding_a_pane_removes_pane_and_its_gutter() {
    let model = SplitterModel::new(2, Orientation::Horizontal);
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(50.0, 40.0))
            .pane(FixedLeaf(50.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));

    model.set_pane_visible(0, false);
    tree.tick_animations(std::time::Duration::from_millis(400));
    tree.layout(SizeProposal::exact(400.0, 200.0));

    assert!(
        tree.bounds(tree.child_widget(root, 0)).width < 2.0,
        "hidden pane → 0"
    );
    assert!(
        tree.bounds(tree.child_widget(root, 1)).width < 2.0,
        "its gutter → 0 (handle removed)"
    );
    assert!(
        tree.bounds(tree.child_widget(root, 2)).width > 396.0,
        "remaining pane fills the whole container (no gutter)"
    );
}

#[test]
fn hiding_a_pane_with_a_collapsed_size_floor_still_folds_to_zero() {
    // A pane can carry a `collapsed_size` floor (so *collapse* folds it to a
    // header sliver) AND be *hidden* via `set_pane_visible`. Hiding must remove
    // it entirely — the collapse floor applies to collapse, not to visibility.
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new()
                .size(200.0)
                .min_size(0.0)
                .stretch(0.0)
                .collapsed_size(32.0),
            PaneDescriptor::new().size(200.0).min_size(0.0).stretch(1.0),
        ],
        Orientation::Vertical,
    );
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(80.0, 300.0))
            .pane(FixedLeaf(80.0, 100.0)),
    );
    tree.layout(SizeProposal::exact(200.0, 400.0));

    model.set_pane_visible(0, false);
    tree.tick_animations(std::time::Duration::from_millis(400));
    tree.layout(SizeProposal::exact(200.0, 400.0));

    assert!(
        tree.bounds(tree.child_widget(root, 0)).height < 2.0,
        "hidden pane folds fully to 0 despite its collapsed_size floor, got {}",
        tree.bounds(tree.child_widget(root, 0)).height
    );
}

#[test]
fn hidden_pane_content_is_dormant_and_handle_at_hidden() {
    let model = SplitterModel::new(2, Orientation::Horizontal);
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(50.0, 40.0))
            .pane(FixedLeaf(50.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 200.0));
    let inner0 = tree.child_widget(tree.child_widget(root, 0), 0);
    assert!(
        tree.find_by_role(teksilo_core::accesskit::Role::Splitter)
            .is_some()
    );

    model.set_pane_visible(0, false);
    tree.tick_animations(std::time::Duration::from_millis(400));
    tree.layout(SizeProposal::exact(400.0, 200.0));
    assert!(!tree.is_active(inner0), "hidden pane content dormant");
    assert!(
        tree.find_by_role(teksilo_core::accesskit::Role::Splitter)
            .is_none(),
        "the now-inactive handle is removed from the AT tree"
    );

    model.set_pane_visible(0, true);
    tree.tick_animations(std::time::Duration::from_millis(400));
    tree.layout(SizeProposal::exact(400.0, 200.0));
    assert!(tree.is_active(inner0), "shown pane content reactivates");
    assert!(
        tree.find_by_role(teksilo_core::accesskit::Role::Splitter)
            .is_some()
    );
}

#[test]
fn hiding_a_middle_pane_drops_both_its_gutters() {
    let model = SplitterModel::new(3, Orientation::Horizontal);
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(50.0, 40.0))
            .pane(FixedLeaf(50.0, 40.0))
            .pane(FixedLeaf(50.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(600.0, 200.0));

    model.set_pane_visible(1, false);
    tree.tick_animations(std::time::Duration::from_millis(400));
    tree.layout(SizeProposal::exact(600.0, 200.0));

    // children: clip0, handle0, clip1, handle1, clip2
    assert!(
        tree.bounds(tree.child_widget(root, 2)).width < 2.0,
        "middle pane gone"
    );
    assert!(
        tree.bounds(tree.child_widget(root, 1)).width < 2.0,
        "gutter 0 gone"
    );
    assert!(
        tree.bounds(tree.child_widget(root, 3)).width < 2.0,
        "gutter 1 gone"
    );
    let w0 = tree.bounds(tree.child_widget(root, 0)).width;
    let w2 = tree.bounds(tree.child_widget(root, 4)).width;
    assert!(
        (w0 + w2 - 600.0).abs() < 2.0,
        "the two visible panes fill the container"
    );
}

#[test]
fn export_import_round_trips_through_widget() {
    let model = SplitterModel::new(3, Orientation::Horizontal);
    let mut tree = theme_tree();
    tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(50.0, 40.0))
            .pane(FixedLeaf(50.0, 40.0))
            .pane(FixedLeaf(50.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(600.0, 200.0));

    model.set_stored_size(0, 111.0);
    model.set_collapsed(2, true);
    let state = model.export_state();

    let restored = SplitterModel::new(3, Orientation::Horizontal);
    assert!(restored.import_state(&state));
    assert_eq!(restored.stored_size(0), 111.0);
    assert!(restored.is_collapsed(2));
}

#[test]
fn vertical_arrows_move_the_divider_the_way_they_point() {
    // Regression: routing the handle through `range_nav` inverted this pair.
    // `increase` is axis-relative — `Up` increases a value, `Down` decreases
    // it — so reading it as a direction made `Down` shrink the pane above the
    // divider, where it had always grown it. Only the horizontal pair was
    // covered, so the whole vertical axis went the wrong way unnoticed.
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new().size(100.0).min_size(0.0).stretch(0.0),
            PaneDescriptor::new().size(100.0).min_size(0.0).stretch(0.0),
        ],
        Orientation::Vertical,
    );
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(80.0, 100.0))
            .pane(FixedLeaf(80.0, 100.0)),
    );
    tree.layout(SizeProposal::exact(
        200.0,
        200.0 + SPLITTER_GUTTER_THICKNESS,
    ));

    let handle = tree.child_widget(root, 1);
    tree.focus(handle);
    let before = model.stored_size(0);

    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert!(
        model.stored_size(0) > before,
        "ArrowDown moves the divider down, which grows the pane above it"
    );

    let mid = model.stored_size(0);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    assert!(model.stored_size(0) < mid, "and ArrowUp moves it back up");
}

#[test]
fn a_vertical_splitter_ignores_the_layout_direction() {
    // `Home`/`End` and the vertical arrows name points in value space, so a
    // right-to-left window must not touch them — only the horizontal pair
    // mirrors.
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new().size(100.0).min_size(0.0).stretch(0.0),
            PaneDescriptor::new().size(100.0).min_size(0.0).stretch(0.0),
        ],
        Orientation::Vertical,
    );
    let mut tree = theme_tree();
    tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(80.0, 100.0))
            .pane(FixedLeaf(80.0, 100.0)),
    );
    tree.layout(SizeProposal::exact(
        200.0,
        200.0 + SPLITTER_GUTTER_THICKNESS,
    ));

    let handle = tree.child_widget(root, 1);
    tree.focus(handle);
    let before = model.stored_size(0);

    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert!(
        model.stored_size(0) > before,
        "ArrowDown still grows the pane above, whatever the layout direction"
    );
}

// ---------------------------------------------------------------------------
// N22: a splitter smaller than its panes' minimums
// ---------------------------------------------------------------------------

/// Two expanded horizontal panes with minimum 320 each: they need
/// 646 px with the gutter, and these tests lay them out in 330.
fn too_small_model(orientation: Orientation, collapsible: bool) -> SplitterModel {
    SplitterModel::from_panes(
        vec![
            PaneDescriptor::new()
                .size(500.0)
                .min_size(320.0)
                .stretch(0.0)
                .collapsible(collapsible),
            PaneDescriptor::new()
                .size(400.0)
                .min_size(320.0)
                .stretch(0.0)
                .collapsible(collapsible),
        ],
        orientation,
    )
}

fn contains(outer: Rect, inner: Rect) -> bool {
    const TOL: f32 = 0.5;
    inner.x >= outer.x - TOL
        && inner.y >= outer.y - TOL
        && inner.x + inner.width <= outer.x + outer.width + TOL
        && inner.y + inner.height <= outer.y + outer.height + TOL
}

/// Every descendant of `root` that is laid out lies inside `root`'s bounds.
fn assert_descendants_inside(tree: &WidgetTree, root: WidgetId) {
    let outer = tree.bounds(root);
    let mut stack = tree.children(root);
    while let Some(id) = stack.pop() {
        let b = tree.bounds(id);
        assert!(
            contains(outer, b),
            "{id:?} at {b:?} lies outside the splitter at {outer:?}"
        );
        stack.extend(tree.children(id));
    }
}

fn too_small_tree(model: &SplitterModel, proposal: SizeProposal) -> (WidgetTree, WidgetId) {
    let mut tree = theme_tree();
    let root = tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(proposal);
    (tree, root)
}

#[test]
fn a_splitter_too_small_for_its_minimums_keeps_every_child_inside() {
    let (tree, root) = too_small_tree(
        &too_small_model(Orientation::Horizontal, false),
        SizeProposal::exact(330.0, 200.0),
    );
    let avail = 330.0 - SPLITTER_GUTTER_THICKNESS;
    let a = tree.bounds(tree.child_widget(root, 0)).width;
    let b = tree.bounds(tree.child_widget(root, 2)).width;
    assert!(
        (a - avail / 2.0).abs() < 0.5 && (b - avail / 2.0).abs() < 0.5,
        "equal minimums share the room equally: {a} and {b}"
    );
    assert_descendants_inside(&tree, root);
}

#[test]
fn a_too_small_splitter_keeps_every_child_inside_in_rtl() {
    let model = too_small_model(Orientation::Horizontal, false);
    let mut tree = theme_tree();
    tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
    let root = tree.add(
        Splitter::new(model)
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(100.0, 40.0)),
    );
    tree.layout(SizeProposal::exact(330.0, 200.0));
    assert_descendants_inside(&tree, root);
}

#[test]
fn a_too_small_vertical_splitter_keeps_every_child_inside() {
    let (tree, root) = too_small_tree(
        &too_small_model(Orientation::Vertical, false),
        SizeProposal::exact(200.0, 330.0),
    );
    assert_descendants_inside(&tree, root);
}

#[test]
fn a_too_small_splitter_hiding_a_pane_keeps_every_child_inside() {
    let model = too_small_model(Orientation::Horizontal, false);
    let (mut tree, root) = too_small_tree(&model, SizeProposal::exact(330.0, 200.0));
    model.set_pane_visible(1, false);
    let outer = tree.bounds(root);
    for _ in 0..12 {
        tree.tick_animations(std::time::Duration::from_millis(16));
        tree.layout(SizeProposal::exact(330.0, 200.0));
        // The panes and the divider; a folding pane's content keeps its
        // full layout and is clipped by its pane, as in any fold.
        for id in tree.children(root) {
            let b = tree.bounds(id);
            assert!(contains(outer, b), "{id:?} at {b:?} outside {outer:?}");
        }
    }
    // Once hidden, the remaining pane fits on its own: it gets the whole
    // splitter, at its minimum or above.
    tree.tick_animations(std::time::Duration::from_millis(600));
    tree.layout(SizeProposal::exact(330.0, 200.0));
    let first = tree.bounds(tree.child_widget(root, 0));
    assert!((first.width - 330.0).abs() < 0.5, "{first:?}");
}

#[test]
fn gutters_wider_than_the_splitter_narrow_to_fit() {
    let model = too_small_model(Orientation::Horizontal, false);
    let (tree, root) = too_small_tree(&model, SizeProposal::exact(5.0, 200.0));
    const { assert!(SPLITTER_GUTTER_THICKNESS > 5.0) };
    assert_descendants_inside(&tree, root);
    assert!(tree.bounds(tree.child_widget(root, 0)).width.abs() < 0.01);
}

#[test]
fn a_too_small_splitter_lays_pane_content_at_the_scaled_size() {
    let (tree, root) = too_small_tree(
        &too_small_model(Orientation::Horizontal, false),
        SizeProposal::exact(330.0, 200.0),
    );
    for k in [0, 2] {
        let pane = tree.child_widget(root, k);
        let content = tree.child_widget(pane, 0);
        assert!(
            (tree.bounds(content).width - tree.bounds(pane).width).abs() < 0.5,
            "pane {k}'s content is laid at its pane's scaled width"
        );
    }
    assert_descendants_inside(&tree, root);
}

#[test]
fn a_bounded_proposal_is_answered_with_itself_and_an_unbounded_one_with_the_floors() {
    let model = too_small_model(Orientation::Horizontal, false);
    let (mut tree, _root) = too_small_tree(&model, SizeProposal::exact(330.0, 200.0));
    let bounded = tree
        .measure_root_intrinsic(SizeProposal {
            width: Some(330.0),
            height: Some(200.0),
        })
        .unwrap()
        .width;
    assert!(
        (bounded - 330.0).abs() < 0.01,
        "bounded 330 reports {bounded}"
    );
    let unbounded = tree
        .measure_root_intrinsic(SizeProposal {
            width: None,
            height: Some(200.0),
        })
        .unwrap()
        .width;
    let floors = 640.0 + SPLITTER_GUTTER_THICKNESS;
    assert!(
        (unbounded - floors).abs() < 0.01,
        "unbounded reports the floors {floors}, got {unbounded}"
    );
    // A hidden pane asks for nothing, nor does its gutter.
    model.set_pane_visible_immediate(1, false);
    tree.layout(SizeProposal::exact(330.0, 200.0));
    let one = tree
        .measure_root_intrinsic(SizeProposal {
            width: None,
            height: Some(200.0),
        })
        .unwrap()
        .width;
    assert!(
        (one - 320.0).abs() < 0.01,
        "one visible pane reports 320, got {one}"
    );
}

#[test]
fn the_splitter_clips_its_children() {
    let (mut tree, root) = too_small_tree(
        &too_small_model(Orientation::Horizontal, false),
        SizeProposal::exact(330.0, 200.0),
    );
    assert!(tree.widget_clips_children(root));
    let bounds = tree.bounds(root);
    let frame = tree.render();
    assert!(
        frame.draw_order.iter().any(|cmd| matches!(
            cmd,
            teksilo_canvas::DrawCommand::SetClip(r) if *r == bounds
        )),
        "the render walk opens a clip at the splitter's bounds {bounds:?}"
    );
}

#[test]
fn a_small_drag_in_a_too_small_splitter_collapses_nothing_and_keeps_the_sizes() {
    let model = too_small_model(Orientation::Horizontal, true);
    let (mut tree, root) = too_small_tree(&model, SizeProposal::exact(330.0, 200.0));
    let before = (
        tree.bounds(tree.child_widget(root, 0)).width,
        tree.bounds(tree.child_widget(root, 2)).width,
    );
    let handle = tree.child_widget(root, 1);
    let start = tree.bounds(handle).center();
    tree.pointer_down_button(start, PointerButton::Primary);
    let to = Point::new(start.x + 2.0, start.y);
    tree.pointer_move(to);
    tree.layout(SizeProposal::exact(330.0, 200.0));
    tree.pointer_up_button(to, PointerButton::Primary);
    tree.layout(SizeProposal::exact(330.0, 200.0));

    assert!(
        !model.is_collapsed(0) && !model.is_collapsed(1),
        "nothing collapses"
    );
    let after = (
        tree.bounds(tree.child_widget(root, 0)).width,
        tree.bounds(tree.child_widget(root, 2)).width,
    );
    assert!(
        (after.0 - before.0).abs() < 0.5 && (after.1 - before.1).abs() < 0.5,
        "the divider cannot move while the panes are below their minimums: {before:?} -> {after:?}"
    );
    assert_eq!((model.stored_size(0), model.stored_size(1)), (500.0, 400.0));
}

#[test]
fn resizing_a_too_small_splitter_keeps_the_users_split() {
    let model = too_small_model(Orientation::Horizontal, true);
    let wide = SizeProposal::exact(900.0 + SPLITTER_GUTTER_THICKNESS, 200.0);
    let narrow = SizeProposal::exact(330.0, 200.0);
    let (mut tree, root) = too_small_tree(&model, wide);
    let handle = tree.child_widget(root, 1);
    tree.layout(narrow);

    // An arrow key, an AccessKit Increment and a drag on the divider.
    tree.focus(handle);
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    tree.layout(narrow);
    let mut ops = teksilo_core::window::NoopWindowOps;
    tree.dispatch_access_action(
        teksilo_core::accessibility::widget_id_to_node_id(handle),
        teksilo_core::accesskit::Action::Increment,
        None,
        &mut ops,
    );
    tree.layout(narrow);
    let start = tree.bounds(handle).center();
    let to = Point::new(start.x + 10.0, start.y);
    tree.pointer_down_button(start, PointerButton::Primary);
    tree.pointer_move(to);
    tree.layout(narrow);
    tree.pointer_up_button(to, PointerButton::Primary);
    assert!(!model.is_collapsed(0) && !model.is_collapsed(1));

    tree.layout(wide);
    assert_eq!(
        (model.stored_size(0), model.stored_size(1)),
        (500.0, 400.0),
        "the stored split survives a gesture at a width that could not show it"
    );
    assert!((tree.bounds(tree.child_widget(root, 0)).width - 500.0).abs() < 0.5);
    assert!((tree.bounds(tree.child_widget(root, 2)).width - 400.0).abs() < 0.5);
}

/// A pin, not a test of the fix: proportional scaling keeps the ratio of
/// the minimums, which is what the overflowing layout reported too.
#[test]
fn a_too_small_splitter_divider_value_keeps_the_ratio_of_the_minimums() {
    let (tree, _root) = too_small_tree(
        &too_small_model(Orientation::Horizontal, false),
        SizeProposal::exact(330.0, 200.0),
    );
    let handle = tree
        .find_by_role(teksilo_core::accesskit::Role::Splitter)
        .unwrap();
    let node = tree.accessibility_node(handle);
    let value = node.numeric_value().expect("a numeric value");
    assert!(
        (value - 50.0).abs() < 0.5,
        "equal minimums read 50 %, got {value}"
    );
}

#[test]
fn expanding_a_pane_in_a_too_small_splitter_does_not_jump_at_the_end() {
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new()
                .size(400.0)
                .min_size(320.0)
                .stretch(0.0)
                .collapsible(true)
                .collapsed(true),
            PaneDescriptor::new()
                .size(400.0)
                .min_size(320.0)
                .stretch(1.0),
        ],
        Orientation::Horizontal,
    );
    let proposal = SizeProposal::exact(330.0, 200.0);
    let (mut tree, root) = too_small_tree(&model, proposal);
    let sibling = |tree: &WidgetTree| tree.bounds(tree.child_widget(root, 2)).width;
    assert!((sibling(&tree) - 324.0).abs() < 0.5);

    model.set_collapsed(0, false);
    let mut last = sibling(&tree);
    for _ in 0..120 {
        tree.tick_animations(std::time::Duration::from_millis(8));
        tree.layout(proposal);
        let now = sibling(&tree);
        assert!(
            (now - last).abs() < 60.0,
            "the sibling jumps from {last} to {now} during the expansion"
        );
        last = now;
    }
    assert!(
        (last - 162.0).abs() < 0.5,
        "the expansion ends with equal shares, got {last}"
    );
    assert_descendants_inside(&tree, root);
}

#[test]
fn collapsing_a_pane_in_a_too_small_splitter_keeps_the_users_split() {
    let model = too_small_model(Orientation::Horizontal, true);
    let wide = SizeProposal::exact(900.0 + SPLITTER_GUTTER_THICKNESS, 200.0);
    let narrow = SizeProposal::exact(330.0, 200.0);
    let (mut tree, root) = too_small_tree(&model, wide);
    let handle = tree.child_widget(root, 1);
    tree.layout(narrow);

    // Enter on the divider collapses pane 0, animated, at the scaled size.
    tree.focus(handle);
    tree.press_key(Key::Enter, Modifiers::NONE);
    tree.tick_animations(std::time::Duration::from_millis(600));
    tree.layout(narrow);
    assert!(model.is_collapsed(0));

    tree.layout(wide);
    model.set_collapsed(0, false);
    tree.tick_animations(std::time::Duration::from_millis(600));
    tree.layout(wide);
    assert_eq!(
        (model.stored_size(0), model.stored_size(1)),
        (500.0, 400.0),
        "collapsing at a scaled size does not store the scaled size"
    );
    assert!((tree.bounds(tree.child_widget(root, 0)).width - 500.0).abs() < 0.5);
}

#[test]
fn a_drag_that_collapses_a_pane_in_a_too_small_splitter_keeps_the_stored_sizes() {
    let model = too_small_model(Orientation::Horizontal, true);
    let (mut tree, root) = too_small_tree(&model, SizeProposal::exact(330.0, 200.0));
    let handle = tree.child_widget(root, 1);
    let start = tree.bounds(handle).center();
    let to = Point::new(5.0, start.y);
    tree.pointer_down_button(start, PointerButton::Primary);
    tree.pointer_move(to);
    tree.layout(SizeProposal::exact(330.0, 200.0));
    tree.pointer_up_button(to, PointerButton::Primary);
    assert!(
        model.is_collapsed(0),
        "dragging past the scaled floor collapses"
    );
    assert_eq!((model.stored_size(0), model.stored_size(1)), (500.0, 400.0));
}

#[test]
fn a_frozen_divider_offers_no_increment_or_decrement() {
    use teksilo_core::accesskit::Action;
    let model = too_small_model(Orientation::Horizontal, false);
    let (mut tree, root) = too_small_tree(&model, SizeProposal::exact(330.0, 200.0));
    let handle = tree.child_widget(root, 1);
    let actions = |tree: &WidgetTree| tree.accessibility_node(handle).actions().to_vec();
    let frozen = actions(&tree);
    assert!(frozen.contains(&Action::Focus));
    assert!(
        !frozen.contains(&Action::Increment) && !frozen.contains(&Action::Decrement),
        "a divider that cannot move advertises no resize: {frozen:?}"
    );
    tree.layout(SizeProposal::exact(
        900.0 + SPLITTER_GUTTER_THICKNESS,
        200.0,
    ));
    let free = actions(&tree);
    assert!(free.contains(&Action::Increment) && free.contains(&Action::Decrement));
}

/// The tween runs to the settled layout in a splitter of any size: before,
/// an expanding pane tweened to its stored size and its sibling then jumped
/// by the slack the pane's stretch takes, and a hiding pane started from its
/// stored size rather than from where it was.
#[test]
fn expanding_and_hiding_a_stretch_pane_do_not_jump() {
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new()
                .size(100.0)
                .min_size(0.0)
                .stretch(1.0)
                .collapsible(true)
                .collapsed(true),
            PaneDescriptor::new().size(100.0).min_size(0.0).stretch(1.0),
        ],
        Orientation::Horizontal,
    );
    let proposal = SizeProposal::exact(600.0 + SPLITTER_GUTTER_THICKNESS, 200.0);
    let (mut tree, root) = too_small_tree(&model, proposal);
    let width = |tree: &WidgetTree, k: usize| tree.bounds(tree.child_widget(root, k)).width;
    assert!((width(&tree, 2) - 600.0).abs() < 0.5);

    let run = |tree: &mut WidgetTree, what: &str| {
        let mut last = (width(tree, 0), width(tree, 2));
        for _ in 0..250 {
            tree.tick_animations(std::time::Duration::from_millis(4));
            tree.layout(proposal);
            let now = (width(tree, 0), width(tree, 2));
            assert!(
                (now.0 - last.0).abs() < 100.0 && (now.1 - last.1).abs() < 100.0,
                "{what}: the panes jump from {last:?} to {now:?}"
            );
            last = now;
        }
        last
    };
    model.set_collapsed(0, false);
    let expanded = run(&mut tree, "expanding");
    assert!(
        (expanded.0 - 300.0).abs() < 0.5 && (expanded.1 - 300.0).abs() < 0.5,
        "the expansion ends with the stretch shares, got {expanded:?}"
    );
    model.set_pane_visible(0, false);
    let hidden = run(&mut tree, "hiding");
    assert!(hidden.0 < 0.5, "the pane is hidden, got {hidden:?}");
}

#[test]
fn an_unbounded_proposal_ignores_hidden_and_collapsed_content() {
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new().size(100.0).min_size(0.0).stretch(0.0),
            PaneDescriptor::new().size(100.0).min_size(0.0).stretch(0.0),
            PaneDescriptor::new()
                .size(100.0)
                .min_size(0.0)
                .stretch(0.0)
                .collapsed(true)
                .collapsed_size(20.0),
        ],
        Orientation::Horizontal,
    );
    let mut tree = theme_tree();
    tree.add(
        Splitter::new(model.clone())
            .pane(FixedLeaf(100.0, 40.0))
            .pane(FixedLeaf(1000.0, 40.0))
            .pane(FixedLeaf(1000.0, 40.0)),
    );
    model.set_pane_visible_immediate(1, false);
    tree.layout(SizeProposal::exact(400.0, 200.0));
    let width = tree
        .measure_root_intrinsic(SizeProposal {
            width: None,
            height: Some(200.0),
        })
        .unwrap()
        .width;
    let expected = 100.0 + 20.0 + SPLITTER_GUTTER_THICKNESS;
    assert!(
        (width - expected).abs() < 0.01,
        "one visible pane, one collapsed header and one gutter: {expected}, got {width}"
    );
}
