// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The shared driver for "a double click on a row the first click selects".
//!
//! The first click of a double click selects the row, and the selection
//! rebuilds part of the view. That rebuild is deferred while the press holds
//! its implicit capture, so it runs at the first layout after the release,
//! which is exactly where a real frame falls between the two clicks. A test
//! that clicks twice with no `layout()` between them never sees the rebuild
//! and passes whatever the view does, so every driver here lays out between
//! the clicks.

use std::time::Duration;

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::PointerButton;
use teksilo_core::widget_tree::WidgetTree;

/// The gap between the two clicks: well inside every profile's double-click
/// interval.
pub(crate) const CLICK_GAP: Duration = Duration::from_millis(80);

/// One primary click at `at`, press and release.
pub(crate) fn click(tree: &mut WidgetTree, at: Point) {
    tree.tap_with(teksilo_tokens::PointerKind::Mouse, at);
}

/// Click, lay out (the frame between the clicks), wait [`CLICK_GAP`], click
/// again at the same point.
pub(crate) fn double_click_across_a_frame(tree: &mut WidgetTree, at: Point, size: SizeProposal) {
    click(tree, at);
    tree.layout(size);
    tree.advance_input_time(CLICK_GAP);
    click(tree, at);
    tree.layout(size);
}

/// Press, lay out while the button is held (the user holds it over a frame),
/// release, lay out again.
pub(crate) fn click_held_over_a_frame(tree: &mut WidgetTree, at: Point, size: SizeProposal) {
    // On the simulated clock, as `tap_with` is.
    tree.advance_input_time(Duration::ZERO);
    tree.pointer_down_button(at, PointerButton::Primary);
    tree.layout(size);
    tree.pointer_up_button(at, PointerButton::Primary);
    tree.layout(size);
}
