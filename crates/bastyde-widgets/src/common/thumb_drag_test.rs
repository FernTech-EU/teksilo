// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The shared contract test for every virtualized, self-scrolling view.
//!
//! ## The invariant
//!
//! > A widget owning a live gesture must never be a descendant of a widget
//! > that rebuilds on data or scroll changes. Persistent chrome and
//! > virtualized content are **siblings**.
//!
//! Each of `ListView`, `TreeView`, `TableView`, `TreeTableView` and `GridView`
//! mounts its own `ScrollBar`. While the user drags that thumb the framework
//! holds an implicit pointer capture on it for the whole Down→Up sequence
//! (`GestureEvent::DragStarted` → `ctx.capture_pointer()`, released on
//! `DragEnded`), and `WidgetTree::process_pending_rebuilds` deliberately skips
//! any rebuild targeting an *ancestor* of the captured widget — it must, since
//! rebuilding an ancestor destroys the scrollbar's arena node together with
//! its gesture recognizer, and the fresh `ScrollBar` built in its place would
//! carry fresh drag-origin signals.
//!
//! So a view whose row realization is rooted on the view itself cannot
//! re-realize while its own thumb is held: drag past the buffer and the body
//! goes blank until release. Every one of the five therefore hoists its rows
//! into a body pane that is a *sibling* of the scrollbar.
//!
//! This is easy to regress and invisible in every other test — the body only
//! empties while a pointer is down — so each view asserts it here, through one
//! shared driver, rather than trusting the topology to stay correct.

use bastyde_canvas::{Point, SizeProposal};
use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
use bastyde_core::widget_id::WidgetId;
use bastyde_core::widget_tree::WidgetTree;

/// Drive a real scrollbar thumb drag on `view` and assert its body keeps
/// materializing rows *while the pointer is still captured*.
///
/// `rows_in_viewport` counts the view's realized rows that currently land
/// inside the viewport band — each view supplies its own, since "a row" is a
/// `ListItem` wrapper here, an a11y `Role::Row` there and a tile in
/// `GridView`. `label` names the view in the failure message.
///
/// `thumb_top` is where the scrollbar's band starts, in view-local y — the
/// header height for the table views, `0.0` for the header-less ones. Pressing
/// above it would land on the header and start no drag at all, which would
/// make this test pass vacuously.
///
/// The drag is the real gesture, not a synthetic `scroll_y.set`: press on the
/// thumb, cross the drag threshold so the recognizer fires `DragStarted` (and
/// the framework captures), then walk the thumb down the track a step at a
/// time, laying out between steps exactly as a held drag does.
pub(crate) fn assert_body_survives_thumb_drag(
    tree: &mut WidgetTree,
    view: WidgetId,
    width: f32,
    height: f32,
    thumb_top: f32,
    label: &str,
    rows_in_viewport: impl Fn(&WidgetTree) -> usize,
) {
    // Lay out, let pending rebuilds run, then lay out again. A render can
    // rebuild a body pane, and its fresh children carry no bounds until the
    // next layout pass — ending on `render` would read every row as unplaced
    // and make this test fail for the wrong reason.
    let pump = |t: &mut WidgetTree| {
        t.layout(SizeProposal::exact(width, height));
        t.render();
        t.layout(SizeProposal::exact(width, height));
    };
    pump(tree);
    let before = rows_in_viewport(tree);
    assert!(
        before > 0,
        "{label}: precondition — the body must have rows before the drag"
    );
    let realized_before = descendant_ids(tree, view);

    // Press the thumb (right edge, near the top) and cross the threshold so
    // the gesture arena fires DragStarted and auto-captures the pointer.
    let x = width - 5.0;
    let start = Point::new(x, thumb_top + 4.0);
    tree.pointer_move(start);
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: start,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });

    // Walk the thumb to the bottom of the track, still held.
    let travel = height - thumb_top - 8.0;
    for step in 1..=10 {
        let y = thumb_top + 4.0 + travel * (step as f32 / 10.0);
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(x, y),
        });
        pump(tree);
    }

    let during = rows_in_viewport(tree);

    // Vacuity guard. If the press had missed the thumb — a mis-aimed x, a
    // collapsed bar, a header eating the event — nothing would scroll, no
    // buffer would be crossed, no pane would rebuild, and `during > 0` below
    // would hold for the wrong reason. Dragging 500 rows' worth of track has
    // to realize a different set of row widgets than the one we started with.
    assert_ne!(
        realized_before,
        descendant_ids(tree, view),
        "{label}: the thumb drag realized no new rows — the drag never took \
         effect, so this test is not exercising the deferral at all"
    );

    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(x, height - 4.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    pump(tree);
    let after = rows_in_viewport(tree);

    assert!(
        during > 0,
        "{label}: the body went blank while the thumb was held \
         (rows before={before}, during={during}, after release={after}). \
         Row realization has moved back onto an ancestor of the scrollbar — \
         see `common::thumb_drag_test`'s module docs."
    );
    assert!(
        after > 0,
        "{label}: the body is empty after releasing the thumb \
         (before={before}, during={during}, after={after})"
    );
}

/// The second half of the same contract: a body-pane rebuild must not cancel
/// an in-flight smooth scroll.
///
/// `register_animated_signal` records the widget that registered a signal
/// *last* as its owner, and `rebuild_single_widget` calls
/// `animation_scheduler.cancel_by_widget`, which clears the animation target.
/// So a view that re-registers `scroll_y` from its body pane, or that rebuilds
/// its own root on scroll-buffer exit, aborts the very fling that crossed the
/// buffer — the scroll stops dead partway through. Registration therefore
/// belongs to the root, and the root must not rebuild on buffer exit.
///
/// Deterministic on purpose: rather than sleeping through real frames, this
/// starts an animation and then triggers a pane rebuild directly.
pub(crate) fn assert_fling_survives_pane_rebuild(
    tree: &mut WidgetTree,
    width: f32,
    height: f32,
    scroll: &bastyde_core::signal::Signal<f32>,
    label: &str,
    trigger_pane_rebuild: impl FnOnce(),
) {
    use bastyde_tokens::Easing;
    use std::time::Duration;

    tree.layout(SizeProposal::exact(width, height));
    tree.render();

    scroll.animate_to(4000.0, Duration::from_millis(400), Easing::EaseOut);
    assert_eq!(
        scroll.animation_target(),
        Some(4000.0),
        "{label}: precondition — the fling must be registered"
    );

    trigger_pane_rebuild();
    tree.layout(SizeProposal::exact(width, height));
    tree.render();
    tree.layout(SizeProposal::exact(width, height));

    assert_eq!(
        scroll.animation_target(),
        Some(4000.0),
        "{label}: a body-pane rebuild cancelled the in-flight smooth scroll. \
         `scroll_y` must be registered on the ROOT only, and the root must not \
         rebuild on scroll-buffer exit — see this module's docs."
    );
}

/// Every arena id under `view`, in a deterministic walk order. Used only to
/// tell "the view re-realized its rows" from "nothing happened at all".
fn descendant_ids(tree: &WidgetTree, view: WidgetId) -> Vec<WidgetId> {
    let mut out = Vec::new();
    let mut walker = vec![view];
    while let Some(id) = walker.pop() {
        out.push(id);
        for c in tree.children(id) {
            walker.push(c);
        }
    }
    out
}

/// The generic "is this a placed row inside the viewport" walk, for views
/// whose rows aren't distinguished by an accessibility role. Counts every
/// descendant of `view` with a row-shaped, non-collapsed rect landing in the
/// vertical band — which excludes the scrollbar (too narrow) and any row
/// scrolled fully out of sight.
pub(crate) fn placed_rows_in_band(
    tree: &WidgetTree,
    view: WidgetId,
    width: f32,
    height: f32,
) -> usize {
    let mut n = 0;
    let mut walker = vec![view];
    while let Some(id) = walker.pop() {
        if id != view {
            let b = tree.bounds(id);
            if b.height > 1.0 && b.width > width * 0.5 && b.y > -b.height && b.y < height {
                n += 1;
            }
        }
        for c in tree.children(id) {
            walker.push(c);
        }
    }
    n
}
