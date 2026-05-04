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
    let mut ids = state.shell_root_ids.get();
    ids.push(shell_id);
    state.shell_root_ids.set(ids);

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
fn click_at_window_center_reaches_button_bounds() {
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());

    let clicked = Rc::new(Cell::new(false));
    let c = clicked.clone();
    let button = tree.add(
        Button::new_literal("Click Me").on_activate_fn(move |_| c.set(true)),
    );

    let state = InspectorState::new(false);
    let shell_id = tree.add(InspectorShell::new(button, state.clone()));
    let mut ids = state.shell_root_ids.get();
    ids.push(shell_id);
    state.shell_root_ids.set(ids);

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
