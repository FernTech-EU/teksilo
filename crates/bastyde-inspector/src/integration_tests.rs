// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! End-to-end tests covering the inspector → user-root event path.
//!
//! These tests reproduce in headless mode the regression noted by the
//! user: "simple-button doesn't react to mouse clicks" once the
//! inspector wraps the user root. We hand-roll the post_root
//! wrapping (the same shape `state::install` performs at runtime),
//! lay out the tree, and assert that clicking the button still fires
//! its `on_activate` handler.

#![cfg(test)]

use bastyde_i18n::lit;
use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Point, SizeProposal};
use bastyde_core::widget_id::WidgetId;
use bastyde_core::widget_tree::WidgetTree;
use bastyde_widgets::Button;

use crate::shell::InspectorShell;
use crate::state::InspectorState;

#[test]
fn click_passes_through_inspector_shell_to_user_button() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());

    let clicked = Rc::new(Cell::new(false));
    let c = clicked.clone();
    let button = tree.add(Button::new(lit!("Click Me")).on_activate_fn(move |_| c.set(true)));

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
fn picker_click_populates_chain_with_button_ancestor() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let button = tree.add(Button::new(lit!("Click Me")).on_activate_fn(|_| {}));
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

    // Click at the button center.
    let center = Point::new(200.0, 150.0);
    tree.pointer_move(center);
    tree.pointer_down_button(center, bastyde_core::event::PointerButton::Primary);

    // PickResolver runs in layout — populates `pending_pick_chain`
    // with the deepest hit + ancestors up to the user-root.
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let chain = state.pending_pick_chain.get();
    assert!(
        !chain.is_empty(),
        "PickResolver should populate the chain after a pick click"
    );
    // The chain ends at (or includes) the user-root id (which IS the
    // button — `Button::new_literal` registers Button as the root
    // of this test tree). The deepest-hit is somewhere inside the
    // Button composite. Both endpoints belong to the button's
    // subtree.
    let chain_ids: Vec<_> = chain.iter().map(|e| e.id).collect();
    assert!(
        chain_ids.contains(&button),
        "chain {chain_ids:?} should include the user-root button id ({button:?})"
    );

    // No selection yet — the user must pick a row from the menu.
    assert!(
        state.selected_id.get().is_none(),
        "selected_id should remain None until the user activates a chain row"
    );
    assert!(
        state.picker_mode.get(),
        "picker_mode stays on while the chain menu is being shown / awaited"
    );

    // PointerUp shows the chain menu via `ctx.show_overlay`. Once
    // the user activates a row, that row's `on_activate` commits the
    // selection, clears the chain, and exits picker mode. We
    // simulate the deepest row's activation by reading the menu id
    // and synthetically clicking the first child Button.
    tree.pointer_up_button(center, bastyde_core::event::PointerButton::Primary);
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let menu_id = state
        .pick_menu_id
        .get()
        .expect("InspectorShell should have registered a chain-menu id");
    let menu_kids = tree.children(menu_id);
    assert_eq!(
        menu_kids.len(),
        1,
        "menu Panel wraps a single child (the inner VStack)"
    );
    let vstack_id = menu_kids[0];
    let row_ids = tree.children(vstack_id);
    assert!(!row_ids.is_empty(), "menu VStack should have row children");
    // Activate row 0 (the deepest entry) — that's what a click on the
    // top menu row would do. `synthetic_click` dispatches a click
    // event so the row's `on_activate_fn` fires.
    tree.click(row_ids[0]);
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let selected = state
        .selected_id
        .get()
        .expect("activating a chain row should set selected_id");
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
        "selected {selected:?} should be the button or one of its descendants"
    );
    assert!(
        !state.picker_mode.get(),
        "picker_mode should turn off after a chain row is activated"
    );
    assert!(
        state.pending_pick_chain.get().is_empty(),
        "chain should be cleared after activation"
    );
}

#[test]
fn panel_fills_window_width_when_open() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let button = tree.add(Button::new(lit!("X")).on_activate_fn(|_| {}));
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
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let button = tree.add(Button::new(lit!("X")).on_activate_fn(|_| {}));
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
    ) -> (WidgetId, bastyde_canvas::Rect, u32) {
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
    eprintln!(
        "deepest panel descendant {:?} bounds {:?}",
        deepest, deepest_bounds
    );

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
    ) -> Option<(WidgetId, bastyde_canvas::Rect, u32)> {
        let b = tree.bounds(id);
        let mut best = (b.height > 100.0).then_some((id, b, depth));
        for k in tree.children(id) {
            if let Some(nested) = deepest_tall(tree, k, depth + 1)
                && best.as_ref().is_none_or(|(_, _, d)| nested.2 > *d)
            {
                best = Some(nested);
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
    use bastyde_core::event::PointerButton;

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let button = tree.add(Button::new(lit!("X")).on_activate_fn(|_| {}));
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
fn tree_tab_renders_rows_for_user_root() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let button = tree.add(Button::new(lit!("X")).on_activate_fn(|_| {}));
    let state = InspectorState::new(true); // panel open
    let shell_id = tree.add(InspectorShell::new(button, state.clone()));
    let mut ids = state.user_root_ids.get();
    ids.push(button);
    state.user_root_ids.set(ids);
    let _ = shell_id;

    // Match simple-button's window dimensions (the example the user
    // is running) — at 400×300 the 280 px default panel dominates the
    // window so any TreeRows sizing or hit-test bug shows up here.
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Walk down to find a tall (>= 30 px) leaf widget INSIDE the
    // Tree tab area whose bounds height equals N * 18 (ROW_HEIGHT).
    // TreeRows reports its bounds height as `rows.len() * ROW_HEIGHT`.
    // A height of 0 means push_subtree found nothing in the user-root
    // subtree — the symptom the user reported.
    fn find_tree_rows_height(tree: &WidgetTree, id: WidgetId) -> Option<(WidgetId, f32)> {
        let kids = tree.children(id);
        if kids.is_empty() {
            let h = tree.bounds(id).height;
            if h >= 54.0 && (h % 18.0).abs() < 0.01 {
                return Some((id, h));
            }
            return None;
        }
        for k in kids {
            if let Some(p) = find_tree_rows_height(tree, k) {
                return Some(p);
            }
        }
        None
    }

    let h = find_tree_rows_height(&tree, shell_id)
        .map(|(_, h)| h)
        .unwrap_or(0.0);
    assert!(
        h >= 54.0,
        "TreeRows reports {h} px height — push_subtree found no widgets in the user-root \
         subtree (button + descendants), so the Tree tab is empty"
    );
}

#[test]
fn overflow_stripes_collected_for_overflowing_user_root() {
    use bastyde_core::signal::Signal;
    use bastyde_widgets::primitives::{FixedSize, HStack};

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());

    // A horizontal stack of rigid 200px children inside a 400px window:
    // the children's union (600px) spills past the parent's right edge,
    // so `collect_overflow` must record an overhang strip.
    let row = tree.add(
        FixedSize::new()
            .bind_width(Signal::new(300.0))
            .bind_height(Signal::new(60.0))
            .child(
                HStack::new()
                    .child(
                        FixedSize::new()
                            .bind_width(Signal::new(200.0))
                            .bind_height(Signal::new(40.0)),
                    )
                    .child(
                        FixedSize::new()
                            .bind_width(Signal::new(200.0))
                            .bind_height(Signal::new(40.0)),
                    )
                    .child(
                        FixedSize::new()
                            .bind_width(Signal::new(200.0))
                            .bind_height(Signal::new(40.0)),
                    ),
            ),
    );

    let state = InspectorState::new(false);
    let shell_id = tree.add(InspectorShell::new(row, state.clone()));
    let mut ids = state.user_root_ids.get();
    ids.push(row);
    state.user_root_ids.set(ids);
    let _ = shell_id;

    // First pass: `BoundsTracker.layout_response` reads bounds during the
    // measure phase, before the sibling user-subtree is placed this pass —
    // so it sees the previous pass's geometry (zero on the very first pass).
    // The snapshot only catches up once a later pass runs against placed
    // bounds, which a live app always provides. Force a second real pass
    // (a changed proposal) to assert the catch-up.
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.layout(SizeProposal::exact(401.0, 300.0));

    assert!(
        !state.overflow_snapshot.get_ref().is_empty(),
        "overflow stripes must be collected for an overflowing HStack; snapshot = {:?}",
        state.overflow_snapshot.get_ref()
    );
}

#[test]
fn f12_global_shortcut_toggles_panel_open() {
    use bastyde_core::event::{Key, Modifiers};
    use bastyde_core::intent::Intent;
    use bastyde_core::shortcut::{KeyStroke, Shortcut};

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());

    let button = tree.add(Button::new(lit!("Click Me")));
    let state = InspectorState::new(false);
    let shell_id = tree.add(InspectorShell::new(button, state.clone()));
    let mut ids = state.user_root_ids.get();
    ids.push(button);
    state.user_root_ids.set(ids);

    // Mirror the runtime registration in `state::install`: a global F12
    // shortcut owned by the (wrapped) root that flips `state.open`.
    let toggle = state.open.clone();
    let shortcut = Shortcut::new("__bastyde_inspector.toggle")
        .name("Toggle Inspector")
        .primary(KeyStroke::new(Key::F12, Modifiers::empty()))
        .on_activate(move |_ks, _ctx| {
            let next = !toggle.get();
            toggle.set(next);
            Intent::new("__bastyde_inspector.toggle")
        })
        .build();
    tree.shortcut_registry_mut()
        .register_owned(shortcut, shell_id);

    tree.layout(SizeProposal::exact(400.0, 300.0));

    assert!(!state.open.get(), "panel starts closed");
    // No widget focused — the global F12 must still fire.
    assert_eq!(tree.focused(), None, "precondition: nothing focused");
    tree.press_key(Key::F12, Modifiers::empty());
    assert!(
        state.open.get(),
        "F12 (global shortcut) must toggle the inspector panel open with no focus"
    );
}

#[test]
fn click_at_window_center_reaches_button_bounds() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());

    let clicked = Rc::new(Cell::new(false));
    let c = clicked.clone();
    let button = tree.add(Button::new(lit!("Click Me")).on_activate_fn(move |_| c.set(true)));

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
    tree.pointer_down_button(center, bastyde_core::event::PointerButton::Primary);
    tree.pointer_up_button(center, bastyde_core::event::PointerButton::Primary);

    assert!(
        clicked.get(),
        "click at window center should hit the button; button bounds = {:?}",
        tree.bounds(button)
    );
}
