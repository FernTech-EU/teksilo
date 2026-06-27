// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Widget-level integration tests for `Splitter` (headless `WidgetTree`).
//! The pure sizing engine is unit-tested in `distribute.rs`; the model in
//! `model.rs`. Here we cover placement, drag, keyboard, collapse triggers,
//! RTL, clipping, and accessibility through a real arena.

use super::*;
use bastyde_canvas::{Point, Size, SizeProposal};
use bastyde_core::event::{Key, Modifiers, PointerButton};
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget};
use bastyde_core::widget_tree::WidgetTree;

#[derive(Debug)]
struct FixedLeaf(f32, f32);

impl Widget for FixedLeaf {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(self.0, self.1).into()
    }
}

fn theme_tree() -> WidgetTree {
    WidgetTree::new().with_theme(bastyde_core::presets::intui::light())
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
    // Equal thirds (each gets min 0 + equal stretch slack).
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
fn rtl_horizontal_mirrors_pane_order() {
    let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
    let model = h_model(&[avail * 0.3, avail * 0.7]);
    let mut tree = theme_tree();
    tree.set_layout_direction(bastyde_core::environment::LayoutDirection::RightToLeft);
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
    tree.set_layout_direction(bastyde_core::environment::LayoutDirection::RightToLeft);
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
        .find_by_role(bastyde_core::accesskit::Role::Splitter)
        .unwrap();
    let info = tree.accessibility_node(handle);
    assert_eq!(info.role(), bastyde_core::accesskit::Role::Splitter);
    assert!(
        info.actions()
            .contains(&bastyde_core::accesskit::Action::Increment)
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
        tree.find_by_role(bastyde_core::accesskit::Role::Splitter)
            .is_some()
    );

    model.set_pane_visible(0, false);
    tree.tick_animations(std::time::Duration::from_millis(400));
    tree.layout(SizeProposal::exact(400.0, 200.0));
    assert!(!tree.is_active(inner0), "hidden pane content dormant");
    assert!(
        tree.find_by_role(bastyde_core::accesskit::Role::Splitter)
            .is_none(),
        "the now-inactive handle is removed from the AT tree"
    );

    model.set_pane_visible(0, true);
    tree.tick_animations(std::time::Duration::from_millis(400));
    tree.layout(SizeProposal::exact(400.0, 200.0));
    assert!(tree.is_active(inner0), "shown pane content reactivates");
    assert!(
        tree.find_by_role(bastyde_core::accesskit::Role::Splitter)
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
