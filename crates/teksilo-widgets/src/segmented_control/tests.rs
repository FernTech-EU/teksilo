// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Headless widget-level tests for [`SegmentedControl`].
//!
//! **Settling.** A `Signal::set` performed inside `place_children` dirties
//! the binding registry, but `process_state_changes` only translates that
//! into dormancy transitions at the top of the *next* `layout()` call. A
//! real app never notices (the window manager re-lays out whenever
//! `needs_reconcile()`), but a bare `WidgetTree` does — so every assertion
//! about *structural* state must go through [`settle`], not a single
//! `layout()`. `Toolbar`'s own suite has the same requirement.

use super::*;
use std::collections::HashMap;
use teksilo_core::accesskit;
use teksilo_core::event::Modifiers;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

const A: SegmentId = SegmentId::from_u64(101);
const B: SegmentId = SegmentId::from_u64(102);
const C: SegmentId = SegmentId::from_u64(103);

fn tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

/// A tree with a real text backend, so `single_line` labels report a
/// shrink weight and intrinsic widths reflect the label.
fn measured_tree() -> WidgetTree {
    tree().with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )))
}

/// Lay out twice: once to publish the plan, once to reconcile the
/// dormancy it implies. See the module docs.
fn settle(tree: &mut WidgetTree, width: f32, height: f32) {
    tree.layout(SizeProposal::exact(width, height));
    tree.layout(SizeProposal::exact(width, height));
}

fn abc(selected: Signal<Option<SegmentId>>) -> SegmentedControl {
    SegmentedControl::new(selected).segments([
        Segment::new(lit!("A")).id(A),
        Segment::new(lit!("B")).id(B),
        Segment::new(lit!("C")).id(C),
    ])
}

/// Seven segments with distinct labels, for the overflow tests.
fn seven(selected: Signal<Option<SegmentId>>) -> (SegmentedControl, Vec<SegmentId>) {
    let ids: Vec<SegmentId> = (1..=7).map(|i| SegmentId::from_u64(200 + i)).collect();
    let labels = ["One", "Two", "Three", "Four", "Five", "Six", "Seven"];
    let mut control = SegmentedControl::new(selected);
    for (i, label) in labels.iter().enumerate() {
        control = control.segment(Segment::new(lit!(*label)).id(ids[i]));
    }
    (control, ids)
}

/// The cells currently mounted and active — i.e. actually on the strip.
fn active_cells(tree: &WidgetTree, control: WidgetId) -> Vec<WidgetId> {
    tree.children(control)
        .into_iter()
        .filter(|&id| tree.is_active(id))
        .filter(|&id| {
            tree.accessibility_node(id).role() == teksilo_core::accesskit::Role::RadioButton
        })
        .collect()
}

// ───────────────────────── measurement (step 0) ─────────────────────────

/// Width of a hugging, content-sized control over `labels`.
///
/// The control is wrapped in a `VStack`: the tree's root is handed the
/// layout proposal verbatim, so a root-level control always reports the
/// proposal's width and would prove nothing about intrinsic sizing.
fn hugged_width(labels: &[&'static str]) -> f32 {
    let ids: Vec<SegmentId> = (0..labels.len())
        .map(|i| SegmentId::from_u64(500 + i as u64))
        .collect();
    let selected = Signal::new(Some(ids[0]));
    let mut control = SegmentedControl::new(selected)
        .fill_width(false)
        .sizing(SegmentSizing::Fit);
    for (i, label) in labels.iter().enumerate() {
        control = control.segment(Segment::new(lit!(*label)).id(ids[i]));
    }
    let mut t = measured_tree();
    let id = t.add(control);
    t.add(crate::primitives::VStack::new().child(id));
    settle(&mut t, 2000.0, 60.0);
    t.bounds(id).width
}

#[test]
fn intrinsic_width_tracks_the_measured_labels() {
    // The load-bearing regression: cells used to echo the proposal, so an
    // unbounded measure returned 0x0. Every natural width would then be
    // zero, "everything fits" would be trivially true, and the whole
    // overflow feature would be a silent no-op — while a hugging control
    // collapsed to its chrome. Both label sets would measure identically.
    let short = hugged_width(&["A", "B"]);
    let long = hugged_width(&["A rather long label", "Another long one"]);
    assert!(short > 0.0, "a hugging control must have a width");
    assert!(
        long > short * 1.5,
        "intrinsic width must follow the measured labels ({long} vs {short})"
    );
}

#[test]
fn the_control_hugs_its_measured_content_when_not_filling() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(abc(selected).fill_width(false));
    t.add(crate::primitives::VStack::new().child(control));
    settle(&mut t, 800.0, 60.0);
    let width = t.bounds(control).width;
    assert!(
        width < 800.0,
        "fill_width(false) must hug its segments, got {width}"
    );
    assert!(width > 0.0);
}

#[test]
fn filling_is_the_default() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(abc(selected));
    t.add(crate::primitives::VStack::new().child(control));
    settle(&mut t, 800.0, 60.0);
    assert!(
        (t.bounds(control).width - 800.0).abs() < 0.5,
        "the default claims the width it is offered, got {}",
        t.bounds(control).width
    );
}

// ───────────────────────────── identity ─────────────────────────────

#[test]
fn click_selects_by_id_not_position() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(abc(selected.clone()));
    settle(&mut t, 300.0, 60.0);

    let cells = active_cells(&t, control);
    let centre = t.bounds(cells[2]).center();
    t.pointer_down_button(centre, teksilo_core::event::PointerButton::Primary);
    t.pointer_up_button(centre, teksilo_core::event::PointerButton::Primary);
    assert_eq!(selected.get(), Some(C));
}

#[test]
fn inserting_a_segment_ahead_of_the_selection_does_not_move_it() {
    // The whole point of keying: with a bare index this reads as "the
    // selection silently jumped to the new segment".
    let selected = Signal::new(Some(C));
    let mut t = measured_tree();
    t.add(SegmentedControl::new(selected.clone()).segments([
        Segment::new(lit!("new")).id(SegmentId::from_u64(999)),
        Segment::new(lit!("A")).id(A),
        Segment::new(lit!("B")).id(B),
        Segment::new(lit!("C")).id(C),
    ]));
    settle(&mut t, 400.0, 60.0);
    assert_eq!(selected.get(), Some(C), "selection must follow the id");
}

#[test]
fn an_unknown_id_clamps_to_a_real_segment_and_restamps() {
    let selected = Signal::new(Some(SegmentId::from_u64(4242)));
    let mut t = measured_tree();
    t.add(abc(selected.clone()));
    settle(&mut t, 300.0, 60.0);
    assert_eq!(
        selected.get(),
        Some(A),
        "a stale id must resolve to a segment that exists, not stay dangling"
    );
}

#[test]
fn an_empty_control_clears_the_selection() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    t.add(SegmentedControl::new(selected.clone()));
    settle(&mut t, 300.0, 60.0);
    assert_eq!(selected.get(), None);
}

#[test]
fn index_signal_maps_ids_onto_switcher_positions() {
    let selected = Signal::new(Some(B));
    let index = index_signal(&selected, &[A, B, C]);
    assert_eq!(index.get(), 1);
    selected.set(Some(C));
    assert_eq!(index.get(), 2);
    selected.set(None);
    assert_eq!(index.get(), 0, "an absent id resolves to the first pane");
    selected.set(Some(SegmentId::from_u64(7777)));
    assert_eq!(index.get(), 0, "an unknown id resolves to the first pane");
}

// ───────────────────────────── keyboard ─────────────────────────────

#[test]
fn arrow_keys_cycle_and_wrap() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(abc(selected.clone()));
    settle(&mut t, 300.0, 60.0);
    t.focus(control);

    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selected.get(), Some(B));
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selected.get(), Some(C));
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selected.get(), Some(A), "wraps past the end");
    t.press_key(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(selected.get(), Some(C), "wraps past the start");
}

#[test]
fn home_and_end_jump_to_the_ends() {
    let selected = Signal::new(Some(B));
    let mut t = measured_tree();
    let control = t.add(abc(selected.clone()));
    settle(&mut t, 300.0, 60.0);
    t.focus(control);

    t.press_key(Key::End, Modifiers::NONE);
    assert_eq!(selected.get(), Some(C));
    t.press_key(Key::Home, Modifiers::NONE);
    assert_eq!(selected.get(), Some(A));
}

#[test]
fn arrow_keys_skip_disabled_segments() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(SegmentedControl::new(selected.clone()).segments([
        Segment::new(lit!("A")).id(A),
        Segment::new(lit!("B")).id(B).disabled(true),
        Segment::new(lit!("C")).id(C),
    ]));
    settle(&mut t, 300.0, 60.0);
    t.focus(control);

    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selected.get(), Some(C), "the disabled middle is skipped");
}

#[test]
fn a_disabled_flag_flipped_after_build_changes_stepping_with_no_rebuild() {
    // The flags are `Prop<bool>` read at *event* time. A `Vec<bool>`
    // snapshotted during `build()` — which is what this widget used to do
    // — would silently keep routing the arrow key into a segment the app
    // has since disabled.
    let selected = Signal::new(Some(A));
    let b_disabled = Signal::new(false);
    let mut t = measured_tree();
    let control = t.add(SegmentedControl::new(selected.clone()).segments([
        Segment::new(lit!("A")).id(A),
        Segment::new(lit!("B")).id(B).disabled(b_disabled.clone()),
        Segment::new(lit!("C")).id(C),
    ]));
    settle(&mut t, 300.0, 60.0);
    t.focus(control);

    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selected.get(), Some(B), "enabled: B is the next segment");

    selected.set(Some(A));
    b_disabled.set(true);
    // Deliberately no rebuild, and not even a relayout.
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(
        selected.get(),
        Some(C),
        "a live disabled flag must be honoured without a rebuild"
    );
}

#[test]
fn rtl_swaps_the_arrow_directions() {
    let selected = Signal::new(Some(B));
    let mut t = measured_tree();
    t.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
    let control = t.add(abc(selected.clone()));
    settle(&mut t, 300.0, 60.0);
    t.focus(control);

    t.press_key(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(
        selected.get(),
        Some(C),
        "in RTL, ArrowLeft advances in reading order"
    );
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selected.get(), Some(B));
}

#[test]
fn all_disabled_segments_make_the_arrows_no_ops() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(SegmentedControl::new(selected.clone()).segments([
        Segment::new(lit!("A")).id(A).disabled(true),
        Segment::new(lit!("B")).id(B).disabled(true),
    ]));
    settle(&mut t, 300.0, 60.0);
    t.focus(control);
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selected.get(), Some(A));
}

// ───────────────────────────── on_change ─────────────────────────────

#[test]
fn on_change_fires_once_per_user_driven_change_with_an_event_context() {
    let selected = Signal::new(Some(A));
    let seen: Rc<RefCell<Vec<SegmentId>>> = Rc::new(RefCell::new(Vec::new()));
    let mut t = measured_tree();
    let control = t.add(abc(selected.clone()).on_change({
        let seen = seen.clone();
        move |id, _ctx| seen.borrow_mut().push(id)
    }));
    settle(&mut t, 300.0, 60.0);
    t.focus(control);

    t.press_key(Key::ArrowRight, Modifiers::NONE);
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(*seen.borrow(), vec![B, C]);

    // A programmatic write carries no event, so it must not invoke the
    // callback — the documented contract.
    selected.set(Some(A));
    assert_eq!(*seen.borrow(), vec![B, C]);
}

// ───────────────────────────── overflow ─────────────────────────────

#[test]
fn a_narrow_control_overflows_and_a_wide_one_does_not() {
    let selected = Signal::new(Some(SegmentId::from_u64(201)));
    let (control, _ids) = seven(selected);
    let mut t = measured_tree();
    let id = t.add(control);

    settle(&mut t, 900.0, 60.0);
    assert_eq!(
        active_cells(&t, id).len(),
        7,
        "a wide control shows every segment"
    );

    settle(&mut t, 200.0, 60.0);
    let narrow = active_cells(&t, id).len();
    assert!(
        (1..7).contains(&narrow),
        "a narrow control must overflow but keep at least one segment, got {narrow}"
    );

    settle(&mut t, 900.0, 60.0);
    assert_eq!(
        active_cells(&t, id).len(),
        7,
        "re-widening must restore every segment"
    );
}

#[test]
fn a_resize_sweep_never_loses_the_selection_and_always_settles() {
    let selected = Signal::new(Some(SegmentId::from_u64(205)));
    let (control, ids) = seven(selected.clone());
    let mut t = measured_tree();
    let id = t.add(control);

    for width in [900.0, 500.0, 300.0, 160.0, 240.0, 900.0] {
        settle(&mut t, width, 60.0);

        // The invariant: whatever the width, the selected segment is on
        // the strip.
        let cells = active_cells(&t, id);
        assert!(!cells.is_empty(), "at {width} dp the strip went empty");
        let selected_id = selected.get().expect("a selection always exists");
        let position = ids
            .iter()
            .position(|&candidate| candidate == selected_id)
            .expect("the selection is one of the declared segments");
        let selected_cell = t
            .children(id)
            .into_iter()
            .filter(|&child| {
                t.accessibility_node(child).role() == teksilo_core::accesskit::Role::RadioButton
            })
            .nth(position)
            .expect("every segment has a cell");
        assert!(
            cells.contains(&selected_cell),
            "at {width} dp the selected segment was pushed off the strip"
        );

        // And the tree is quiet: a third layout must change nothing.
        let before = active_cells(&t, id);
        t.layout(SizeProposal::exact(width, 60.0));
        assert_eq!(
            before,
            active_cells(&t, id),
            "at {width} dp the plan never settled"
        );
    }
}

#[test]
fn the_overflow_menu_rows_stay_dormant_while_the_chevron_is_closed() {
    let selected = Signal::new(Some(SegmentId::from_u64(201)));
    let (control, _ids) = seven(selected);
    let mut t = measured_tree();
    let id = t.add(control);

    for width in [900.0, 400.0, 200.0, 900.0] {
        settle(&mut t, width, 60.0);
        let menu_items = count_role(&t, id, teksilo_core::accesskit::Role::MenuItemRadio);
        assert_eq!(
            menu_items, 0,
            "at {width} dp a closed overflow menu still published {menu_items} rows to AT"
        );
    }
}

fn count_role(tree: &WidgetTree, root: WidgetId, role: teksilo_core::accesskit::Role) -> usize {
    let mut total = 0;
    if tree.is_active(root) {
        if tree.accessibility_node(root).role() == role {
            total += 1;
        }
        for child in tree.children(root) {
            total += count_role(tree, child, role);
        }
    }
    total
}

#[test]
fn a_caption_bound_to_is_overflowing_still_settles() {
    // The widget-catalog / `collapsible_menu_bar` demo shape: a control in
    // a slider-driven fixed-width box, with a sibling label narrating
    // `is_overflowing()`. That signal is written from `place_children` and
    // consumed at `Relayout` by the label, so this guards the one thing
    // that could go wrong — the caption's own relayout feeding back into
    // another publish and the tree never going quiet.
    let selected = Signal::new(Some(SegmentId::from_u64(201)));
    let (control, _ids) = seven(selected);
    let overflowing = control.is_overflowing();
    let mut t = measured_tree();

    let control_id = t.add(control);
    let boxed = t.add(
        crate::primitives::FixedSize::new()
            .width(300.0_f32)
            .child(control_id),
    );
    let caption = t.add(
        crate::primitives::TextWidget::new(lit!("")).text(overflowing.map(|over| {
            if *over {
                "overflowing".to_string()
            } else {
                "fits".to_string()
            }
        })),
    );
    t.add(crate::primitives::VStack::new().child(boxed).child(caption));

    settle(&mut t, 800.0, 200.0);
    assert!(
        overflowing.get(),
        "300 dp cannot hold seven segments — this test needs the overflow case"
    );

    // Quiescence: further passes must change nothing at all.
    let before = active_cells(&t, control_id);
    for _ in 0..3 {
        t.layout(SizeProposal::exact(800.0, 200.0));
        assert_eq!(
            before,
            active_cells(&t, control_id),
            "the caption binding kept the tree churning"
        );
    }
}

#[test]
fn compress_mode_keeps_every_segment_on_the_strip() {
    let selected = Signal::new(Some(SegmentId::from_u64(201)));
    let (control, _ids) = seven(selected);
    let mut t = measured_tree();
    let id = t.add(control.overflow(SegmentOverflow::Compress));
    settle(&mut t, 180.0, 60.0);
    assert_eq!(
        active_cells(&t, id).len(),
        7,
        "Compress must never move a segment into a menu"
    );
}

#[test]
fn a_single_layout_pass_on_a_narrow_control_does_not_panic() {
    // The visibility props are polled on the *first* pass, before any plan
    // has been published. A bare `flags[i]` there panics; the seeded,
    // fail-open form does not.
    let selected = Signal::new(Some(SegmentId::from_u64(201)));
    let (control, _ids) = seven(selected);
    let mut t = measured_tree();
    t.add(control);
    t.layout(SizeProposal::exact(120.0, 60.0));
}

// ─────────────────────────── sticky promotion ───────────────────────────

#[test]
fn a_promoted_segment_keeps_its_slot_until_another_is_promoted() {
    let selected = Signal::new(Some(SegmentId::from_u64(201)));
    let (control, ids) = seven(selected.clone());
    let mut t = measured_tree();
    let id = t.add(control);
    settle(&mut t, 300.0, 60.0);

    let visible_ids = |t: &WidgetTree| -> Vec<SegmentId> {
        let all: Vec<WidgetId> = t
            .children(id)
            .into_iter()
            .filter(|&c| {
                t.accessibility_node(c).role() == teksilo_core::accesskit::Role::RadioButton
            })
            .collect();
        all.iter()
            .enumerate()
            .filter(|(_, c)| t.is_active(**c))
            .map(|(i, _)| ids[i])
            .collect()
    };

    let initial = visible_ids(&t);
    assert!(
        !initial.contains(&ids[6]),
        "the last segment should start in the menu"
    );

    // Select a hidden segment — as picking it from the menu would.
    selected.set(Some(ids[6]));
    settle(&mut t, 300.0, 60.0);
    let promoted = visible_ids(&t);
    assert!(
        promoted.contains(&ids[6]),
        "selecting a hidden segment must bring it onto the strip"
    );

    // Selecting a segment that was already visible must not evict it.
    selected.set(Some(ids[0]));
    settle(&mut t, 300.0, 60.0);
    assert!(
        visible_ids(&t).contains(&ids[6]),
        "the promoted segment must stay put when a visible one is chosen"
    );

    // Promoting another replaces it.
    selected.set(Some(ids[5]));
    settle(&mut t, 300.0, 60.0);
    let second = visible_ids(&t);
    assert!(second.contains(&ids[5]));
    assert!(
        !second.contains(&ids[6]),
        "only one segment is ever promoted"
    );
}

#[test]
fn widening_to_a_full_fit_forgets_the_promotion() {
    let selected = Signal::new(Some(SegmentId::from_u64(201)));
    let (control, ids) = seven(selected.clone());
    let mut t = measured_tree();
    let id = t.add(control);
    settle(&mut t, 300.0, 60.0);

    selected.set(Some(ids[6]));
    settle(&mut t, 300.0, 60.0);
    // Everything fits: promotion is irrelevant and must be cleared.
    settle(&mut t, 1200.0, 60.0);
    // Narrow again with an early segment selected: if the promotion had
    // stuck, ids[6] would be pinned to the last slot.
    selected.set(Some(ids[0]));
    settle(&mut t, 300.0, 60.0);

    let cells: Vec<WidgetId> = t
        .children(id)
        .into_iter()
        .filter(|&c| t.accessibility_node(c).role() == teksilo_core::accesskit::Role::RadioButton)
        .collect();
    assert!(
        !t.is_active(cells[6]),
        "a stale promotion must not survive a full-fit interlude"
    );
}

// ────────────────────────── accessibility ──────────────────────────

#[test]
fn the_group_carries_its_role_name_and_actions() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(abc(selected).label(lit!("View mode")));
    settle(&mut t, 300.0, 60.0);

    let node = t.accessibility_node(control);
    assert_eq!(node.role(), teksilo_core::accesskit::Role::RadioGroup);
    assert_eq!(node.name(), Some("View mode"));
    assert!(
        node.actions()
            .contains(&teksilo_core::accesskit::Action::Increment)
    );
}

#[test]
fn access_label_still_names_the_control() {
    // The control is the semantic node, so the framework-wide naming
    // idiom keeps working — the reason the chevron lives inside the
    // RadioGroup rather than beside a pruned wrapper.
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(abc(selected).access_label(lit!("Mode")));
    settle(&mut t, 300.0, 60.0);
    assert_eq!(t.accessibility_node(control).name(), Some("Mode"));
}

/// The published AccessKit nodes, keyed by id. `AccessibilityInfo` is a
/// coarse test view that carries neither set positions nor
/// `active_descendant`, so these assertions go to the real tree update.
fn a11y_nodes(t: &mut WidgetTree) -> HashMap<accesskit::NodeId, accesskit::Node> {
    t.sync_accessibility().nodes.into_iter().collect()
}

fn node_of(nodes: &HashMap<accesskit::NodeId, accesskit::Node>, id: WidgetId) -> accesskit::Node {
    nodes
        .get(&teksilo_core::accessibility::widget_id_to_node_id(id))
        .cloned()
        .unwrap_or_else(|| panic!("{id:?} published no AccessKit node"))
}

#[test]
fn segments_announce_their_position_over_the_whole_list() {
    // Overflowed segments are still part of the set — they are reachable
    // from the menu — so the count is the full segment count even while
    // fewer are rendered.
    let selected = Signal::new(Some(SegmentId::from_u64(201)));
    let (control, _ids) = seven(selected);
    let mut t = measured_tree();
    let id = t.add(control);
    settle(&mut t, 300.0, 60.0);

    let cells = active_cells(&t, id);
    assert!(cells.len() < 7, "this test needs an overflowing control");
    // Asked the way an adapter asks it: the position off the cell, the size by
    // walking up to the `Role::RadioGroup` container. An overflowed segment is
    // still one of the choices, so the total is 7 even though fewer cells are
    // rendered.
    let update = t.sync_accessibility();
    crate::a11y_set_semantics::assert_announces(
        &update,
        teksilo_core::accessibility::widget_id_to_node_id(cells[0]),
        1,
        7,
        "the first segment of an overflowing control",
    );
}

#[test]
fn active_descendant_tracks_the_selection() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(abc(selected.clone()));
    settle(&mut t, 300.0, 60.0);
    let cells = active_cells(&t, control);

    let nodes = a11y_nodes(&mut t);
    assert_eq!(
        node_of(&nodes, control).active_descendant(),
        Some(teksilo_core::accessibility::widget_id_to_node_id(cells[0]))
    );

    selected.set(Some(C));
    settle(&mut t, 300.0, 60.0);
    let nodes = a11y_nodes(&mut t);
    assert_eq!(
        node_of(&nodes, control).active_descendant(),
        Some(teksilo_core::accessibility::widget_id_to_node_id(cells[2]))
    );
}

#[test]
fn radio_group_relations_never_reference_an_overflowed_segment() {
    // A dormant cell publishes no AccessKit node, so a `push_to_radio_group`
    // pointing at one would dangle — and `accesskit_macos` unwraps those.
    let selected = Signal::new(Some(SegmentId::from_u64(201)));
    let (control, _ids) = seven(selected);
    let mut t = measured_tree();
    t.add(control);

    for width in [900.0, 400.0, 220.0, 900.0] {
        settle(&mut t, width, 60.0);
        let update = t.sync_accessibility();
        let published: std::collections::HashSet<accesskit::NodeId> =
            update.nodes.iter().map(|(id, _)| *id).collect();
        for (id, node) in &update.nodes {
            for target in node.radio_group() {
                assert!(
                    published.contains(target),
                    "at {width} dp node {id:?} points at unpublished radio-group member {target:?}"
                );
            }
        }
    }
}

#[test]
fn overflowed_segments_leave_the_accessibility_tree() {
    let selected = Signal::new(Some(SegmentId::from_u64(201)));
    let (control, _ids) = seven(selected);
    let mut t = measured_tree();
    let id = t.add(control);
    settle(&mut t, 250.0, 60.0);

    let rendered = count_role(&t, id, teksilo_core::accesskit::Role::RadioButton);
    assert!(
        rendered < 7,
        "overflowed segments must be pruned from AT, not merely hidden"
    );
    assert!(rendered >= 1);
}

#[test]
fn access_click_on_a_disabled_segment_is_ignored() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(SegmentedControl::new(selected.clone()).segments([
        Segment::new(lit!("A")).id(A),
        Segment::new(lit!("B")).id(B).disabled(true),
        Segment::new(lit!("C")).id(C),
    ]));
    settle(&mut t, 300.0, 60.0);

    let cells = active_cells(&t, control);
    t.dispatch_event(WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::Click,
        target: Some(cells[1]),
        target_node: teksilo_core::accessibility::root_node_id(),
        data: None,
    });
    assert_eq!(selected.get(), Some(A));
}

#[test]
fn access_click_selects_a_segment() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(abc(selected.clone()));
    settle(&mut t, 300.0, 60.0);

    let cells = active_cells(&t, control);
    t.dispatch_event(WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::Click,
        target: Some(cells[2]),
        target_node: teksilo_core::accessibility::root_node_id(),
        data: None,
    });
    assert_eq!(selected.get(), Some(C));
}

/// Whether a reader is told a radio button is checked, read off the node as
/// the three adapters read it. All of them take it from `toggled` and none
/// from `selected`. AT-SPI sets `CHECKED` from `Toggled::True`
/// (`accesskit_atspi_common` `node.rs:366-372`), the one state Orca 46.1's
/// radio phrase reads (`orca/generator.py:661-676`): without it Orca says "not
/// selected radio button". UIA gives a radio button its `SelectionItem`
/// pattern only when `toggled` is set, and takes `IsSelected` from it
/// (`accesskit_windows` `node.rs:648-675`). macOS takes `AXValue` from it
/// (`accesskit_macos` `node.rs:344-352`). `None` is a radio button that tells
/// a reader nothing either way.
fn told_checked(node: &accesskit_consumer::NodeRef<'_>) -> Option<bool> {
    node.toggled().map(|t| t == accesskit::Toggled::True)
}

/// The radio buttons in the tree as a reader walks it (`common_filter`), each
/// with its name and [`told_checked`], and the name of the node the reader is
/// on. The consumer follows the group's
/// `active_descendant` to find it (`tree.rs:538-543`), as every adapter does.
fn segments_as_heard(
    platform: &accesskit_consumer::Tree,
) -> (Vec<(String, Option<bool>)>, Option<String>) {
    fn walk(node: accesskit_consumer::NodeRef<'_>, out: &mut Vec<(String, Option<bool>)>) {
        if node.role() == accesskit::Role::RadioButton {
            out.push((node.label().unwrap_or_default(), told_checked(&node)));
        }
        for child in node.filtered_children(&accesskit_consumer::common_filter) {
            walk(child, out);
        }
    }
    let state = platform.state();
    let mut segments = Vec::new();
    walk(state.root(), &mut segments);
    (segments, state.focus().and_then(|node| node.label()))
}

/// Hears nothing: for a consumer tree kept only to be read.
struct Unheard;

impl accesskit_consumer::TreeChangeHandler for Unheard {
    fn node_added(&mut self, _: &accesskit_consumer::NodeRef) {}
    fn node_updated(&mut self, _: &accesskit_consumer::NodeRef, _: &accesskit_consumer::NodeRef) {}
    fn focus_moved(
        &mut self,
        _: Option<&accesskit_consumer::NodeRef>,
        _: Option<&accesskit_consumer::NodeRef>,
    ) {
    }
    fn node_removed(&mut self, _: &accesskit_consumer::NodeRef) {}
}

#[test]
fn the_reader_hears_the_selected_segment_checked() {
    // A segment is a radio button, so a reader hears "First, selected radio
    // button" only if it is checked: a segment that is merely `selected` was
    // read as "not selected radio button" by Orca, on the segment chosen.
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(abc(selected.clone()).label(lit!("View mode")));
    settle(&mut t, 300.0, 60.0);
    t.focus(control);
    let mut platform = accesskit_consumer::Tree::new(t.sync_accessibility(), true);

    let yes = |name: &str| (name.to_owned(), Some(true));
    let no = |name: &str| (name.to_owned(), Some(false));
    assert_eq!(
        segments_as_heard(&platform),
        (vec![yes("A"), no("B"), no("C")], Some("A".to_owned())),
        "the reader is on the selected segment, and it alone is told checked"
    );

    t.press_key(Key::ArrowRight, Modifiers::NONE);
    settle(&mut t, 300.0, 60.0);
    platform.update_and_process_changes(t.sync_accessibility(), &mut Unheard);
    assert_eq!(
        segments_as_heard(&platform),
        (vec![no("A"), yes("B"), no("C")], Some("B".to_owned())),
        "after Right the reader is on B, and B is now the one told checked"
    );
}

/// Every `selection-changed` AT-SPI raises for one update, by the group it is
/// raised on. The adapter raises one on an item's selection container, once
/// an update, when an item carrying `selected == true` enters the tree or an
/// item's `selected` changes (`accesskit_atspi_common` `adapter.rs:79-81`,
/// `318-320`, `249-267`). Orca 46.1 answers each by moving its locus of focus
/// to the group's selected child, and so speaking it, wherever focus was
/// (`orca/scripts/default.py:1540-1602`, `onSelectionChanged`).
#[derive(Default)]
struct SelectionChanged(Vec<String>);

impl SelectionChanged {
    fn raise(&mut self, item: &accesskit_consumer::NodeRef) {
        if item.is_item_like()
            && let Some(group) = item.selection_container(&accesskit_consumer::common_filter)
        {
            let name = group.label().unwrap_or_default();
            if !self.0.contains(&name) {
                self.0.push(name);
            }
        }
    }
}

fn included(node: &accesskit_consumer::NodeRef) -> bool {
    accesskit_consumer::common_filter(node) == accesskit_consumer::FilterResult::Include
}

impl accesskit_consumer::TreeChangeHandler for SelectionChanged {
    fn node_added(&mut self, node: &accesskit_consumer::NodeRef) {
        if included(node) && node.is_selected() == Some(true) {
            self.raise(node);
        }
    }
    fn node_updated(
        &mut self,
        old: &accesskit_consumer::NodeRef,
        new: &accesskit_consumer::NodeRef,
    ) {
        if included(old) && included(new) && old.is_selected() != new.is_selected() {
            self.raise(new);
        }
    }
    fn focus_moved(
        &mut self,
        _: Option<&accesskit_consumer::NodeRef>,
        _: Option<&accesskit_consumer::NodeRef>,
    ) {
    }
    fn node_removed(&mut self, _: &accesskit_consumer::NodeRef) {}
}

#[test]
fn a_segmented_control_raises_no_selection_changed() {
    // A radio group has no selection in the AT-SPI sense: its state is each
    // button's `checked`. A segment that also carried `selected` made the
    // adapter raise `selection-changed` on the group as the control entered
    // the tree and on every choice, and Orca spoke each one, with focus
    // elsewhere: chart-demo's launch read five cut "not selected radio button".
    let selected = Signal::new(Some(A));
    let shown = Signal::new(false);
    let mut t = measured_tree();
    let control = t.add(abc(selected.clone()).label(lit!("View mode")));
    t.add(
        crate::primitives::VStack::new().child(
            crate::primitives::VStack::new()
                .child(control)
                .visible_when(shown.clone()),
        ),
    );
    settle(&mut t, 300.0, 60.0);
    let mut platform = accesskit_consumer::Tree::new(t.sync_accessibility(), true);
    assert!(
        segments_as_heard(&platform).0.is_empty(),
        "the control starts out of the tree"
    );

    let mut arrival = SelectionChanged::default();
    shown.set(true);
    settle(&mut t, 300.0, 60.0);
    platform.update_and_process_changes(t.sync_accessibility(), &mut arrival);
    assert_eq!(
        segments_as_heard(&platform).0.len(),
        3,
        "the control entered the tree"
    );
    t.focus(control);
    let mut choice = SelectionChanged::default();
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    settle(&mut t, 300.0, 60.0);
    platform.update_and_process_changes(t.sync_accessibility(), &mut choice);
    assert_eq!(selected.get(), Some(B));
    assert_eq!(
        (arrival.0, choice.0),
        (Vec::new(), Vec::new()),
        "neither the control's arrival nor a choice may raise selection-changed"
    );
}

// ────────────────────────── visibility / sizing ──────────────────────────

#[test]
fn a_hidden_segment_leaves_the_strip_the_menu_and_the_keyboard_order() {
    let selected = Signal::new(Some(A));
    let show_b = Signal::new(true);
    let mut t = measured_tree();
    let control = t.add(SegmentedControl::new(selected.clone()).segments([
        Segment::new(lit!("A")).id(A),
        Segment::new(lit!("B")).id(B).visible(show_b.clone()),
        Segment::new(lit!("C")).id(C),
    ]));
    settle(&mut t, 400.0, 60.0);
    assert_eq!(active_cells(&t, control).len(), 3);

    show_b.set(false);
    settle(&mut t, 400.0, 60.0);
    assert_eq!(active_cells(&t, control).len(), 2);

    t.focus(control);
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(
        selected.get(),
        Some(C),
        "a hidden segment is skipped entirely, not merely disabled"
    );
}

#[test]
fn hiding_every_segment_is_survivable() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(SegmentedControl::new(selected.clone()).segments([
        Segment::new(lit!("A")).id(A).visible(false),
        Segment::new(lit!("B")).id(B).visible(false),
    ]));
    settle(&mut t, 300.0, 60.0);
    assert!(active_cells(&t, control).is_empty());
    assert_eq!(selected.get(), None);

    t.focus(control);
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selected.get(), None);
    t.render();
}

#[test]
fn the_height_follows_the_measured_content_instead_of_a_fixed_constant() {
    // The height used to be the hardcoded `SEGMENTED_CONTROL_HEIGHT`, so
    // any content taller than 24 dp — a large icon, or an ordinary label
    // under a raised global text scale — was clipped. It now follows the
    // measured content, with the constant as a floor.
    //
    // Exercised with an icon rather than with `set_user_text_scale`,
    // because `MockTextBackend` ignores the `TextStyle` it is handed
    // (fixed 8 px per char, 16 px line height) — headless text never
    // changes size, so a text-scale assertion here would pass or fail for
    // reasons unrelated to this widget. The scale itself rides the
    // framework-wide effective-theme mechanism shared by every widget.
    let plain_selected = Signal::new(Some(A));
    let mut plain_tree = measured_tree();
    let plain = plain_tree.add(abc(plain_selected));
    plain_tree.add(crate::primitives::VStack::new().child(plain));
    settle(&mut plain_tree, 400.0, 400.0);
    let plain_height = plain_tree.bounds(plain).height;

    let tall_selected = Signal::new(Some(A));
    let mut tall_tree = measured_tree();
    let tall = tall_tree.add(
        SegmentedControl::new(tall_selected).segments([
            Segment::new(lit!("A"))
                .id(A)
                .icon(|| crate::primitives::IconWidget::chevron_down(40.0)),
            Segment::new(lit!("B")).id(B),
        ]),
    );
    tall_tree.add(crate::primitives::VStack::new().child(tall));
    settle(&mut tall_tree, 400.0, 400.0);
    let tall_height = tall_tree.bounds(tall).height;

    assert!(
        tall_height > plain_height,
        "taller content must grow the control ({tall_height} vs {plain_height})"
    );
    assert!(
        tall_height > SEGMENTED_CONTROL_HEIGHT,
        "the design constant is a floor, not a ceiling ({tall_height})"
    );
}

#[test]
fn narrow_compressed_labels_stay_inside_the_control() {
    // Regression from before overflow existed: a single-line label must
    // ellipsize to fit its cell rather than spill past the frame.
    fn max_right(tree: &WidgetTree, id: WidgetId) -> f32 {
        let mut right = tree.bounds(id).right();
        for child in tree.children(id) {
            if tree.is_active(child) {
                right = right.max(max_right(tree, child));
            }
        }
        right
    }

    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(
        SegmentedControl::new(selected)
            .overflow(SegmentOverflow::Compress)
            .segments([
                Segment::new(lit!("Full Synopsis")).id(A),
                Segment::new(lit!("Full Chapter")).id(B),
                Segment::new(lit!("Overview")).id(C),
            ]),
    );
    settle(&mut t, 120.0, 40.0);

    let right = t.bounds(control).right();
    assert!(
        right <= 120.5,
        "the control must bound itself (right={right})"
    );
    let deepest = max_right(&t, control);
    assert!(
        deepest <= right + 0.5,
        "a label spilled to x={deepest}, past the control's right edge {right}"
    );
}

// ────────────────────────────── chrome ──────────────────────────────

#[test]
fn the_focused_selection_paints_with_the_accent() {
    let selected = Signal::new(Some(B));
    let mut t = measured_tree();
    let control = t.add(abc(selected));
    settle(&mut t, 300.0, 60.0);
    t.focus(control);
    let frame = t.render();
    let accent = teksilo_core::presets::intui::light()
        .colors
        .accent
        .to_array();
    assert!(frame.shapes.iter().any(|s| s.color == accent));
}

#[test]
fn an_unfocused_selection_uses_the_inactive_surface() {
    let selected = Signal::new(Some(B));
    let mut t = measured_tree();
    t.add(abc(selected));
    settle(&mut t, 300.0, 60.0);
    let frame = t.render();
    let theme = teksilo_core::presets::intui::light();
    assert!(
        !frame
            .shapes
            .iter()
            .any(|s| s.color == theme.colors.accent.to_array())
    );
    assert!(
        frame
            .shapes
            .iter()
            .any(|s| s.color == theme.colors.surface_selected_inactive.to_array())
    );
}

#[test]
fn rebuilding_reproduces_every_child() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(abc(selected));
    settle(&mut t, 300.0, 60.0);
    let before = t.children(control).len();
    t.arena_mark_needs_rebuild_for_testing(control);
    settle(&mut t, 300.0, 60.0);
    assert_eq!(t.children(control).len(), before);
}

// ────────────────────────── tooltips / hover ──────────────────────────

#[test]
fn a_tooltip_appears_on_hover() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(SegmentedControl::new(selected).segments([
        Segment::new(lit!("A")).id(A),
        Segment::new(lit!("B")).id(B).tooltip(lit!("Tip")),
        Segment::new(lit!("C")).id(C),
    ]));
    settle(&mut t, 300.0, 60.0);

    let cells = active_cells(&t, control);
    t.pointer_move(t.bounds(cells[1]).center());
    t.advance_time(std::time::Duration::from_secs(1));
    assert_eq!(t.active_overlays().len(), 1);
}

#[test]
fn icon_only_display_promotes_the_label_to_a_tooltip() {
    let selected = Signal::new(Some(A));
    let mut t = measured_tree();
    let control = t.add(
        SegmentedControl::new(selected)
            .display(SegmentDisplay::Icon)
            .segments([
                Segment::new(lit!("Alpha"))
                    .id(A)
                    .icon(|| crate::primitives::IconWidget::chevron_down(12.0)),
                Segment::new(lit!("Beta"))
                    .id(B)
                    .icon(|| crate::primitives::IconWidget::chevron_down(12.0)),
            ]),
    );
    settle(&mut t, 300.0, 60.0);

    let cells = active_cells(&t, control);
    t.pointer_move(t.bounds(cells[0]).center());
    t.advance_time(std::time::Duration::from_secs(1));
    assert_eq!(
        t.active_overlays().len(),
        1,
        "an icon-only segment must still say what it is"
    );
}

#[test]
fn a_segment_that_overflows_while_hovered_clears_the_hover() {
    let selected = Signal::new(Some(SegmentId::from_u64(201)));
    let (control, _ids) = seven(selected);
    let mut t = measured_tree();
    let id = t.add(control);
    settle(&mut t, 900.0, 60.0);

    let cells = active_cells(&t, id);
    let last = *cells.last().expect("seven cells");
    t.pointer_move(t.bounds(last).center());

    // Narrow until that segment overflows; dormancy fires no PointerLeave.
    settle(&mut t, 200.0, 60.0);
    assert!(
        !t.is_active(last),
        "this test needs the hovered segment to have overflowed"
    );
    // Rendering must not paint a hover tint for a segment that is gone —
    // exercised here as "the frame builds without a stale slot lookup".
    t.render();
}

// ───────────────────────────────── touch ─────────────────────────────────

/// A segment's activation is `on_tap`, so it already lands on the release —
/// which is the whole of what the controls sweep owes it. Pinned here so a
/// later change to the cell's handler set is caught.
#[test]
fn a_touch_tap_selects_a_segment_on_release() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_core::pointer::PointerPhase;

    let selected = Signal::new(Some(A));
    let mut tree = measured_tree();
    let control = tree.add(abc(selected.clone()));
    settle(&mut tree, 300.0, 40.0);
    let cells = active_cells(&tree, control);
    let third = tree.bounds(cells[2]);

    let id = finger();
    tree.dispatch_pointer(touch(id, PointerPhase::Down, third.center(), 0));
    assert_eq!(selected.get(), Some(A), "the press selects nothing");
    tree.dispatch_pointer(touch(id, PointerPhase::Up, third.center(), 30));
    assert_eq!(selected.get(), Some(C), "the release selects");
}

/// A finger that lands on a segment and then slides off it selects nothing —
/// the abort gesture WCAG 2.2 SC 2.5.2 asks for, applied to a control whose
/// segments sit edge to edge.
#[test]
fn a_finger_that_slides_off_a_segment_selects_nothing() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_core::pointer::PointerPhase;

    let selected = Signal::new(Some(A));
    let mut tree = measured_tree();
    let control = tree.add(abc(selected.clone()));
    settle(&mut tree, 300.0, 40.0);
    let cells = active_cells(&tree, control);
    let third = tree.bounds(cells[2]);
    let away = teksilo_canvas::Point::new(third.center().x, third.y + third.height + 90.0);

    let id = finger();
    tree.dispatch_pointer(touch(id, PointerPhase::Down, third.center(), 0));
    tree.dispatch_pointer(touch(id, PointerPhase::Move, away, 20));
    tree.dispatch_pointer(touch(id, PointerPhase::Up, away, 40));
    assert_eq!(selected.get(), Some(A));
}

/// Every visible segment clears the 24 dp conformance floor at Compact, which
/// is why the control needs no widening mechanism: the recipe's own height is
/// already the floor and the horizontal padding puts the width over it.
#[test]
fn every_segment_clears_the_conformance_floor_at_compact() {
    let selected = Signal::new(Some(A));
    let mut tree = measured_tree();
    let floor = tree.theme().input.min_target_conformance;
    let control = tree.add(abc(selected));
    settle(&mut tree, 300.0, 40.0);
    for cell in active_cells(&tree, control) {
        let b = tree.bounds(cell);
        assert!(
            b.width >= floor && b.height >= floor,
            "a Compact segment measured {b:?}, under the {floor} dp floor",
        );
    }
}
