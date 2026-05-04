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
