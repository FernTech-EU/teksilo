//! End-to-end tests covering the inspector → user-root event path.
//!
//! These tests reproduce in headless mode the regression noted by the
//! user: "simple-button doesn't react to mouse clicks" once the
//! inspector wraps the user root. We hand-roll the post_root
//! wrapping (the same shape `state::install` performs at runtime),
//! lay out the tree, and assert that clicking the button still fires
//! its `on_activate` handler.

#![cfg(test)]

use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Point, SizeProposal};
use fern_core::widget_id::WidgetId;
use fern_core::widget_tree::WidgetTree;
use fern_tokens::Theme;
use fern_widgets::Button;

use crate::shell::InspectorShell;
use crate::state::InspectorState;

#[test]
fn click_passes_through_inspector_shell_to_user_button() {
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());

    let clicked = Rc::new(Cell::new(false));
    let c = clicked.clone();
    let button = tree.add(
        Button::new_literal("Click Me").on_activate_fn(move |_| c.set(true)),
    );

    let state = InspectorState::new(false);
    let shell_id = tree.add(InspectorShell::new(button, state.clone()));
    let mut ids = state.user_root_ids.get();
    ids.push(button);
    state.user_root_ids.set(ids);
    let _ = shell_id;

    tree.layout(SizeProposal::exact(400.0, 300.0));

    let bounds = tree.bounds(button);
    assert!(
        bounds.width > 0.0 && bounds.height > 0.0,
        "button should have non-zero bounds inside the inspector wrapper, got {:?}",
        bounds
    );

    tree.click(button);
    assert!(
        clicked.get(),
        "click on the user-root button should fire its on_activate even when wrapped by InspectorShell"
    );
}

#[test]
fn picker_resolves_click_to_user_widget() {
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    let button = tree.add(Button::new_literal("Click Me").on_activate_fn(|_| {}));
    let state = InspectorState::new(false);
    let shell_id = tree.add(InspectorShell::new(button, state.clone()));
    let mut ids = state.user_root_ids.get();
    ids.push(button);
    state.user_root_ids.set(ids);
    let _ = shell_id;

    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Activate picker mode + relayout so the picker overlay activates.
    state.picker_mode.set(true);
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Click at the button center; the picker should record the point
    // and the resolver should populate `selected_id` on the next layout.
    let center = Point::new(200.0, 150.0);
    tree.pointer_move(center);
    tree.pointer_down_button(center, fern_core::event::PointerButton::Primary);
    tree.pointer_up_button(center, fern_core::event::PointerButton::Primary);

    // Run another layout so PickResolver gets to hit-test.
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let selected = state.selected_id.get().expect("picker should select a widget");
    // Walk parent chain — the resolved id should be the button itself
    // OR one of its descendants (Button is a composing widget; the
    // deepest-hit at click position is typically its inner label).
    let mut cur = Some(selected);
    let mut found_button = false;
    while let Some(id) = cur {
        if id == button {
            found_button = true;
            break;
        }
        cur = tree.parent(id);
    }
    assert!(
        found_button,
        "picker resolved to {:?}, expected the button (id={:?}) or one of its descendants",
        selected, button
    );
    assert!(
        !state.picker_mode.get(),
        "picker should disengage automatically after one pick"
    );
}

#[test]
fn panel_fills_window_width_when_open() {
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    let button = tree.add(Button::new_literal("X").on_activate_fn(|_| {}));
    let state = InspectorState::new(true); // start with panel open
    let shell_id = tree.add(InspectorShell::new(button, state.clone()));
    let mut ids = state.user_root_ids.get();
    ids.push(button);
    state.user_root_ids.set(ids);
    let _ = shell_id;

    tree.layout(SizeProposal::exact(800.0, 600.0));

    // The shell takes the full window. With the panel open, the
    // bottom slot must also span the full window width — not the
    // panel's natural content width — otherwise the right side of
    // the inspector chrome shows the user app instead of being
    // covered by the panel.
    // Walk shell → root child (VStack) → its last child (the panel
    // slot, which is the FillWidthFixedHeight wrapping the panel
    // switcher). Verify that slot fills the full window width.
    let shell_kids = tree.children(shell_id);
    assert_eq!(shell_kids.len(), 1, "InspectorShell wraps one child");
    let stack_id = shell_kids[0];
    let stack_kids = tree.children(stack_id);
    assert!(stack_kids.len() >= 4, "outer VStack has ≥4 children");
    let panel_slot_id = *stack_kids.last().unwrap();
    let panel_bounds = tree.bounds(panel_slot_id);
    assert_eq!(
        panel_bounds.width, 800.0,
        "panel slot must span the full window width when open: {:?}",
        panel_bounds
    );
    assert!(
        panel_bounds.height > 0.0,
        "panel slot should have non-zero height when open: {:?}",
        panel_bounds
    );
}

#[test]
fn tab_content_fills_panel_width() {
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    let button = tree.add(Button::new_literal("X").on_activate_fn(|_| {}));
    let state = InspectorState::new(true);
    let shell_id = tree.add(InspectorShell::new(button, state.clone()));
    let mut ids = state.user_root_ids.get();
    ids.push(button);
    state.user_root_ids.set(ids);
    let _ = shell_id;

    tree.layout(SizeProposal::exact(800.0, 600.0));

    // Walk down to find a TabWidget content area. We don't have IDs
    // for them, so just walk the deepest descendant under the panel
    // slot and assert any leaf widget there spans the full window
    // width.
    fn deepest_full_height_descendant(
        tree: &WidgetTree,
        id: WidgetId,
        depth: u32,
    ) -> (WidgetId, fern_canvas::Rect, u32) {
        let kids = tree.children(id);
        let mut best = (id, tree.bounds(id), depth);
        for k in kids {
            let nested = deepest_full_height_descendant(tree, k, depth + 1);
            if nested.2 > best.2 {
                best = nested;
            }
        }
        best
    }

    let shell_kids = tree.children(shell_id);
    let panel_slot_id = *tree.children(shell_kids[0]).last().unwrap();
    let (deepest, deepest_bounds, _) = deepest_full_height_descendant(&tree, panel_slot_id, 0);
    eprintln!("deepest panel descendant {:?} bounds {:?}", deepest, deepest_bounds);

    // Inspect the panel-content area: walk all panel descendants and
    // verify *some* widget reaches the full window width. If everything
    // is narrow, the panel body is being sized to natural content
    // width — the bug we're guarding against.
    fn any_full_width(tree: &WidgetTree, id: WidgetId, target_w: f32) -> bool {
        if (tree.bounds(id).width - target_w).abs() < 0.5 {
            return true;
        }
        for k in tree.children(id) {
            if any_full_width(tree, k, target_w) {
                return true;
            }
        }
        false
    }

    assert!(
        any_full_width(&tree, panel_slot_id, 800.0),
        "no panel descendant fills the full 800px window width"
    );

    // Find the deepest "tall" descendant under the panel slot
    // (height > 100 px, i.e. a content region, not a control or row).
    // With the previous `Expand::vertical(switcher)` bug in
    // TabWidget, the active tab's content shrank to its natural width
    // (~200–300 px even inside an 800 px panel). The deepest tall
    // widget should approach the panel's inner width (~776 px after
    // the Panel's 12 px padding on each side).
    fn deepest_tall(
        tree: &WidgetTree,
        id: WidgetId,
        depth: u32,
    ) -> Option<(WidgetId, fern_canvas::Rect, u32)> {
        let b = tree.bounds(id);
        let mut best = (b.height > 100.0).then(|| (id, b, depth));
        for k in tree.children(id) {
            if let Some(nested) = deepest_tall(tree, k, depth + 1) {
                if best.as_ref().is_none_or(|(_, _, d)| nested.2 > *d) {
                    best = Some(nested);
                }
            }
        }
        best
    }
    let (_, deepest_bounds, _) =
        deepest_tall(&tree, panel_slot_id, 0).expect("panel must contain a tall widget");
    assert!(
        deepest_bounds.width >= 700.0,
        "deepest content region inside the 800 px panel only reaches {:.0} px wide; \
         TabWidget's content slot is not claiming full width",
        deepest_bounds.width
    );
}

#[test]
fn panel_resize_handle_tracks_cursor_one_to_one() {
    use fern_core::event::PointerButton;

    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    let button = tree.add(Button::new_literal("X").on_activate_fn(|_| {}));
    let state = InspectorState::new(true);
    let shell_id = tree.add(InspectorShell::new(button, state.clone()));
    let mut ids = state.user_root_ids.get();
    ids.push(button);
    state.user_root_ids.set(ids);
    let _ = shell_id;

    tree.layout(SizeProposal::exact(800.0, 600.0));
    let start_height = state.panel_height.get();

    // Click on the resize handle. Find it by walking down to the
    // last child of the outer VStack's panel-slot and following the
    // first descendant chain (handle is the first child of
    // panel_block VStack).
    let shell_kids = tree.children(shell_id);
    let outer_vstack = shell_kids[0];
    let panel_slot = *tree.children(outer_vstack).last().unwrap();
    // panel_slot → Expand → FixedSize → panel_switcher → ZStack →
    // panel_block → VStack → first child = handle wrapper. Just walk
    // first-children until we find a leaf with a small height.
    fn find_handle(tree: &WidgetTree, id: WidgetId) -> Option<WidgetId> {
        let bounds = tree.bounds(id);
        // Resize handle is exactly HANDLE_HEIGHT tall and full width.
        if (bounds.height - 6.0).abs() < 0.5 && bounds.width > 100.0 {
            return Some(id);
        }
        for c in tree.children(id) {
            if let Some(id) = find_handle(tree, c) {
                return Some(id);
            }
        }
        None
    }
    let handle = find_handle(&tree, panel_slot).expect("resize handle");
    let handle_bounds = tree.bounds(handle);
    let click_y = handle_bounds.y + handle_bounds.height / 2.0;
    let click = Point::new(handle_bounds.x + 50.0, click_y);

    tree.pointer_move(click);
    tree.pointer_down_button(click, PointerButton::Primary);

    // Drag up by 40 px in window coords. Panel should grow by 40.
    let drag_to = Point::new(click.x, click.y - 40.0);
    tree.pointer_move(drag_to);
    let after_one_move = state.panel_height.get();
    assert!(
        (after_one_move - (start_height + 40.0)).abs() < 0.5,
        "after dragging up 40px panel should grow by 40, got start={} now={}",
        start_height,
        after_one_move
    );

    // Drag up another 30 px (cursor at click.y - 70). Panel should
    // be start + 70, NOT after_one_move + 70 (the bug we're guarding).
    let drag_to_2 = Point::new(click.x, click.y - 70.0);
    tree.pointer_move(drag_to_2);
    let after_two_moves = state.panel_height.get();
    assert!(
        (after_two_moves - (start_height + 70.0)).abs() < 0.5,
        "after dragging to total -70px panel should be start+70, got start={} now={}",
        start_height,
        after_two_moves
    );

    tree.pointer_up_button(drag_to_2, PointerButton::Primary);
}

#[test]
fn click_at_window_center_reaches_button_bounds() {
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());

    let clicked = Rc::new(Cell::new(false));
    let c = clicked.clone();
    let button = tree.add(
        Button::new_literal("Click Me").on_activate_fn(move |_| c.set(true)),
    );

    let state = InspectorState::new(false);
    let shell_id = tree.add(InspectorShell::new(button, state.clone()));
    let mut ids = state.user_root_ids.get();
    ids.push(button);
    state.user_root_ids.set(ids);
    let _ = shell_id;

    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Aim a click at the center of the window — this is what a real
    // user would do. With the panel closed the user-root area takes
    // the full window, so a center-window click must hit the button
    // (assuming default Button alignment centers it within Expand).
    let center = Point::new(200.0, 150.0);
    tree.pointer_move(center);
    tree.pointer_down_button(center, fern_core::event::PointerButton::Primary);
    tree.pointer_up_button(center, fern_core::event::PointerButton::Primary);

    assert!(
        clicked.get(),
        "click at window center should hit the button; button bounds = {:?}",
        tree.bounds(button)
    );
}
