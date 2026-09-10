// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! WCAG 2.2 SC 2.5.7: every dragging operation has a single-pointer route.
//!
//! One module per operation in [the drag census](../../../docs/drag-operation-census.md),
//! and five assertions per operation, because the obligations are not
//! interchangeable: the command **exists** as a menu row, it is **reachable by
//! keyboard**, it is **exposed as an AccessKit custom action**, it makes **the
//! same model change the drag makes**, and it **announces once**.
//!
//! The fourth is the load-bearing one. An alternative that reaches a different
//! end state than the drag is not an alternative, so wherever the drag itself
//! can be driven headlessly the test drives *both* and compares the model.

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::accessibility::widget_id_to_node_id;
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_widgets::*;

// ---------------------------------------------------------------------------
// Shared probes
// ---------------------------------------------------------------------------

/// A fixed-size leaf, so a view's row geometry is exactly predictable.
#[derive(Debug)]
struct FixedLeaf(f32, f32);

impl Widget for FixedLeaf {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(self.0, self.1).into()
    }
}

/// Every custom action `widget` advertises, by description, as an assistive
/// client would read them: out of a real `TreeUpdate`, and only when the node
/// also advertises `Action::CustomAction` — an adapter reports the list through
/// that gate, so a list without it is decoration.
fn custom_actions(tree: &mut WidgetTree, widget: WidgetId) -> Vec<String> {
    let update = tree.sync_accessibility();
    let target = widget_id_to_node_id(widget);
    update
        .nodes
        .iter()
        .find(|(id, _)| *id == target)
        .map(|(_, node)| {
            if !node.supports_action(teksilo_core::accesskit::Action::CustomAction) {
                return Vec::new();
            }
            node.custom_actions()
                .iter()
                .map(|a| a.description.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Invoke the custom action whose description is `label`, the way an assistive
/// client does: by the id the node published for it.
fn invoke_custom_action(tree: &mut WidgetTree, widget: WidgetId, label: &str) -> bool {
    let update = tree.sync_accessibility();
    let target = widget_id_to_node_id(widget);
    let Some(id) = update
        .nodes
        .iter()
        .find(|(id, _)| *id == target)
        .and_then(|(_, node)| {
            node.custom_actions()
                .iter()
                .find(|a| a.description == label)
                .map(|a| a.id)
        })
    else {
        return false;
    };
    let mut ops = teksilo_core::window::NoopWindowOps;
    tree.dispatch_access_action(
        target,
        teksilo_core::accesskit::Action::CustomAction,
        Some(teksilo_core::accesskit::ActionData::CustomAction(id)),
        &mut ops,
    )
}

/// Right-click at `at`, which is how a context menu opens for a mouse.
fn right_click(tree: &mut WidgetTree, at: Point) {
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: at,
        button: PointerButton::Secondary,
        modifiers: Modifiers::NONE,
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: at,
        button: PointerButton::Secondary,
        modifiers: Modifiers::NONE,
    });
}

/// Every label in the accessibility tree — what a screen reader would list.
fn a11y_labels(tree: &mut WidgetTree) -> Vec<String> {
    let update = tree.sync_accessibility();
    update
        .nodes
        .iter()
        .filter_map(|(_, node)| node.label().map(|s| s.to_string()))
        .collect()
}

/// The widget behind the accessibility node whose label is `label`.
fn node_with_label(tree: &mut WidgetTree, label: &str) -> Option<WidgetId> {
    let update = tree.sync_accessibility();
    update
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .and_then(|(id, _)| teksilo_core::accessibility::node_id_to_widget_id_maybe(*id))
}

/// Activate the open menu's row named `label`. Returns whether a row was found.
fn click_menu_row(tree: &mut WidgetTree, label: &str) -> bool {
    match node_with_label(tree, label) {
        Some(id) => {
            tree.click(id);
            true
        }
        None => false,
    }
}

/// Everything the framework has said out loud so far.
fn spoken(tree: &mut WidgetTree) -> Vec<String> {
    let _ = tree.sync_accessibility();
    tree.announcements_since(0)
        .into_iter()
        .map(|a| a.text)
        .collect()
}

/// Every widget in `root`'s subtree whose accessibility role is `role`, in tree
/// order — which for a data view's rows is model-index order.
///
/// Resolved through the role rather than through `children(pane)[i]` because a
/// row that is a drag source is wrapped in a `DragSurface`: the pane's child is
/// the wrapper, while the node carrying the row's accessibility properties (and
/// its custom actions) is inside it.
fn nodes_with_role(
    tree: &WidgetTree,
    root: WidgetId,
    role: teksilo_core::accesskit::Role,
) -> Vec<WidgetId> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if tree.accessibility_node(id).role() == role {
            found.push(id);
        }
        let children = tree.children(id);
        for &child in children.iter().rev() {
            stack.push(child);
        }
    }
    found
}

/// Run a full pointer drag: down, past the threshold, to the target, up.
fn drag(tree: &mut WidgetTree, from: Point, to: Point) {
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: from,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(from.x + 10.0, from.y),
    });
    tree.dispatch_event(WidgetEvent::PointerMove { position: to });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: to,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
}

// ---------------------------------------------------------------------------
// Census row 9 — ListView row reorder
// ---------------------------------------------------------------------------

mod list_view_row_reorder {
    use super::*;
    use teksilo_data::{ListModel, SelectionMode, SelectionModel};

    const ROW: f32 = 30.0;

    struct Fixture {
        tree: WidgetTree,
        view: WidgetId,
        model: ListModel<&'static str>,
        selection: SelectionModel,
    }

    fn fixture() -> Fixture {
        let model = ListModel::from_vec(vec!["alpha", "beta", "gamma", "delta", "epsilon"]);
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new();
        let view = tree.add(
            ListView::new(model.clone(), move |_i, _item, _sel| {
                Box::new(FixedLeaf(200.0, ROW))
            })
            .item_height(ROW)
            .selection(selection.clone())
            .type_ahead_label(|item: &&'static str| (*item).to_string())
            .reorderable(true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        Fixture {
            tree,
            view,
            model,
            selection,
        }
    }

    fn order(model: &ListModel<&'static str>) -> Vec<&'static str> {
        (0..model.len())
            .map(|i| model.with_item(i, |v| *v).expect("in range"))
            .collect()
    }

    fn row(tree: &WidgetTree, view: WidgetId, index: usize) -> WidgetId {
        nodes_with_role(tree, view, teksilo_core::accesskit::Role::ListBoxOption)[index]
    }

    /// **Exists** — the four moves are real menu rows a right-click opens.
    #[test]
    fn a_row_offers_the_four_moves_in_its_context_menu() {
        let mut f = fixture();
        let centre = f.tree.bounds(row(&f.tree, f.view, 2)).center();
        right_click(&mut f.tree, centre);
        let labels = a11y_labels(&mut f.tree);
        for expected in ["Move Up", "Move Down", "Move to Top", "Move to Bottom"] {
            assert!(
                labels.iter().any(|l| l == expected),
                "no {expected:?} row in the menu: {labels:?}"
            );
        }
    }

    /// …and a row already at the top is not offered a move that would do
    /// nothing.
    #[test]
    fn the_first_row_is_not_offered_a_move_that_changes_nothing() {
        let mut f = fixture();
        let centre = f.tree.bounds(row(&f.tree, f.view, 0)).center();
        right_click(&mut f.tree, centre);
        let labels = a11y_labels(&mut f.tree);
        assert!(!labels.iter().any(|l| l == "Move Up"), "{labels:?}");
        assert!(!labels.iter().any(|l| l == "Move to Top"), "{labels:?}");
        assert!(labels.iter().any(|l| l == "Move Down"), "{labels:?}");
    }

    /// **Keyboard-reachable** — with no pointer at all.
    #[test]
    fn alt_home_and_alt_end_reach_both_ends_from_the_keyboard() {
        let mut f = fixture();
        f.selection.select(3);
        f.tree.focus(f.view);
        f.tree.press_key(Key::Home, Modifiers::ALT);
        assert_eq!(
            order(&f.model),
            vec!["delta", "alpha", "beta", "gamma", "epsilon"]
        );
        assert_eq!(f.selection.selected_indices(), vec![0], "selection follows");
        f.tree.press_key(Key::End, Modifiers::ALT);
        assert_eq!(
            order(&f.model),
            vec!["alpha", "beta", "gamma", "epsilon", "delta"]
        );
        assert_eq!(f.selection.selected_indices(), vec![4]);
    }

    /// **A custom action** — the route for a client with no keyboard either.
    #[test]
    fn a_row_advertises_the_moves_as_custom_actions() {
        let mut f = fixture();
        let target = row(&f.tree, f.view, 2);
        let advertised = custom_actions(&mut f.tree, target);
        for expected in ["Move Up", "Move Down", "Move to Top", "Move to Bottom"] {
            assert!(
                advertised.iter().any(|a| a == expected),
                "{expected:?} is not advertised: {advertised:?}"
            );
        }
        assert!(invoke_custom_action(&mut f.tree, target, "Move to Top"));
        assert_eq!(
            order(&f.model),
            vec!["gamma", "alpha", "beta", "delta", "epsilon"]
        );
    }

    /// **The same model change the drag makes** — driven both ways over the
    /// same model, and compared.
    #[test]
    fn every_route_lands_the_row_where_the_drag_lands_it() {
        // The drag: row 0 dropped in the gap after row 3.
        let dragged = {
            let mut f = fixture();
            let from = f.tree.bounds(row(&f.tree, f.view, 0)).center();
            drag(&mut f.tree, from, Point::new(from.x, ROW * 4.0));
            order(&f.model)
        };
        assert_ne!(
            dragged,
            vec!["alpha", "beta", "gamma", "delta", "epsilon"],
            "the drag itself did nothing — the comparison would be vacuous"
        );
        // The keyboard chord, three steps down.
        let by_key = {
            let mut f = fixture();
            f.selection.select(0);
            f.tree.focus(f.view);
            for _ in 0..3 {
                f.tree.press_key(Key::ArrowDown, Modifiers::ALT);
            }
            order(&f.model)
        };
        // The custom action, three invocations.
        let by_action = {
            let mut f = fixture();
            for _ in 0..3 {
                let live = order(&f.model)
                    .iter()
                    .position(|v| *v == "alpha")
                    .expect("still present");
                let target = row(&f.tree, f.view, live);
                assert!(invoke_custom_action(&mut f.tree, target, "Move Down"));
            }
            order(&f.model)
        };
        // The menu row, three times.
        let by_menu = {
            let mut f = fixture();
            for step in 0..3 {
                f.tree.layout(SizeProposal::exact(400.0, 300.0));
                let live = order(&f.model)
                    .iter()
                    .position(|v| *v == "alpha")
                    .expect("still present");
                let centre = f.tree.bounds(row(&f.tree, f.view, live)).center();
                right_click(&mut f.tree, centre);
                f.tree.layout(SizeProposal::exact(400.0, 300.0));
                assert!(
                    click_menu_row(&mut f.tree, "Move Down"),
                    "step {step}: no Move Down row"
                );
            }
            order(&f.model)
        };
        assert_eq!(by_key, dragged, "the chord and the drag disagree");
        assert_eq!(
            by_action, dragged,
            "the custom action and the drag disagree"
        );
        assert_eq!(by_menu, dragged, "the menu row and the drag disagree");
    }

    /// **Announces once** — a keyboard user hears the new position, and hears
    /// it one time.
    #[test]
    fn a_completed_move_announces_its_new_position_once() {
        let mut f = fixture();
        f.selection.select(1);
        f.tree.focus(f.view);
        f.tree.press_key(Key::ArrowDown, Modifiers::ALT);
        assert_eq!(spoken(&mut f.tree), vec!["beta moved to 3 of 5"]);
    }

    /// A refused move says nothing: there is no event to report, and "moved"
    /// when nothing moved is worse than silence.
    #[test]
    fn a_move_that_cannot_happen_says_nothing() {
        let mut f = fixture();
        f.selection.select(0);
        f.tree.focus(f.view);
        f.tree.press_key(Key::ArrowUp, Modifiers::ALT);
        assert_eq!(order(&f.model)[0], "alpha");
        assert!(spoken(&mut f.tree).is_empty());
    }

    /// A view that never offered a drag reorder gains none of this: the
    /// obligation is to make an existing drag reachable, not to add one.
    #[test]
    fn a_view_with_no_drag_reorder_gains_no_commands() {
        let model = ListModel::from_vec(vec!["alpha", "beta", "gamma"]);
        let mut tree = WidgetTree::new();
        let view = tree.add(
            ListView::new(model, move |_i, _item, _sel| {
                Box::new(FixedLeaf(200.0, ROW))
            })
            .item_height(ROW),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let target = row(&tree, view, 1);
        assert!(custom_actions(&mut tree, target).is_empty());
        let centre = tree.bounds(target).center();
        let before = tree.overlay_manager().active_ids().len();
        right_click(&mut tree, centre);
        assert_eq!(
            tree.overlay_manager().active_ids().len(),
            before,
            "a non-reorderable list opened a menu it has nothing to put in"
        );
    }
}

// ---------------------------------------------------------------------------
// Census row 10 — TreeView row reorder AND reparent
// ---------------------------------------------------------------------------

mod tree_view_row_reorder {
    use super::*;
    use teksilo_data::{NodeId, SelectionMode, SelectionModel, TreeModel};

    const ROW: f32 = 28.0;

    struct Fixture {
        tree: WidgetTree,
        view: WidgetId,
        model: TreeModel<&'static str>,
        selection: SelectionModel,
    }

    /// Four roots, collapsed. Roots are always visible (depth 0 never collapses
    /// out), so every command under test is reachable without expanding
    /// anything — and the indent that moves a row *into* another root has to
    /// reveal it to keep it reachable, which is the interesting case.
    fn fixture() -> Fixture {
        let model: TreeModel<&'static str> = TreeModel::new();
        for (i, name) in ["A", "B", "C", "D"].into_iter().enumerate() {
            model.insert_root(i, name);
        }
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new();
        let view = tree.add(
            TreeView::new(model.clone(), |_item: &&'static str, entry, _sel| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, ROW)) as Box<dyn Widget>
            })
            .item_height(ROW)
            .selection(selection.clone())
            .type_ahead_label(|item: &&'static str| (*item).to_string())
            .reorderable(true),
        );
        tree.layout(SizeProposal::exact(400.0, 400.0));
        Fixture {
            tree,
            view,
            model,
            selection,
        }
    }

    /// The tree's shape, as `parent → children` names. This is the model change
    /// a reorder or a reparent makes, and the only thing worth comparing across
    /// routes: expansion is per-view state, not model state.
    fn shape(f: &Fixture) -> Vec<(String, Vec<&'static str>)> {
        fn name(model: &TreeModel<&'static str>, node: NodeId) -> &'static str {
            model.with_item(node, |v| *v).expect("live node")
        }
        let mut out = Vec::new();
        let roots: Vec<NodeId> = (0..f.model.root_count()).map(|i| f.model.root(i)).collect();
        out.push((
            "<roots>".to_string(),
            roots.iter().map(|&n| name(&f.model, n)).collect(),
        ));
        let mut stack = roots;
        while let Some(node) = stack.pop() {
            let children = f.model.children(node);
            if !children.is_empty() {
                out.push((
                    name(&f.model, node).to_string(),
                    children.iter().map(|&n| name(&f.model, n)).collect(),
                ));
            }
            stack.extend(children);
        }
        out.sort();
        out
    }

    fn row(tree: &WidgetTree, view: WidgetId, index: usize) -> WidgetId {
        nodes_with_role(tree, view, teksilo_core::accesskit::Role::TreeItem)[index]
    }

    /// **Exists** — the sibling moves and both reparents are real menu rows.
    #[test]
    fn a_row_offers_its_moves_and_its_reparents() {
        let mut f = fixture();
        let centre = f.tree.bounds(row(&f.tree, f.view, 1)).center();
        right_click(&mut f.tree, centre);
        let labels = a11y_labels(&mut f.tree);
        for expected in [
            "Move Up",
            "Move Down",
            "Move to Top",
            "Move to Bottom",
            "Move Into Previous",
        ] {
            assert!(
                labels.iter().any(|l| l == expected),
                "no {expected:?} row: {labels:?}"
            );
        }
    }

    /// A root has no parent to leave, and the first of its siblings has no
    /// previous sibling to enter.
    #[test]
    fn a_reparent_that_cannot_land_is_not_offered() {
        let mut f = fixture();
        let centre = f.tree.bounds(row(&f.tree, f.view, 0)).center();
        right_click(&mut f.tree, centre);
        let labels = a11y_labels(&mut f.tree);
        assert!(
            !labels.iter().any(|l| l == "Move Out One Level"),
            "a root was offered an outdent: {labels:?}"
        );
        assert!(
            !labels.iter().any(|l| l == "Move Into Previous"),
            "the first sibling was offered an indent: {labels:?}"
        );
    }

    /// **Keyboard-reachable** — the two reparents had no keyboard route at all
    /// before, and the far ends had none either.
    #[test]
    fn alt_arrows_reorder_reparent_and_reach_the_ends() {
        let mut f = fixture();
        f.selection.select(1);
        f.tree.focus(f.view);
        f.tree.press_key(Key::End, Modifiers::ALT);
        assert_eq!(shape(&f)[0].1, vec!["A", "C", "D", "B"]);
        f.tree.layout(SizeProposal::exact(400.0, 400.0));
        // B is last; indent it under D, its previous sibling.
        f.tree.press_key(Key::ArrowRight, Modifiers::ALT);
        assert_eq!(shape(&f)[0].1, vec!["A", "C", "D"]);
        assert_eq!(f.model.children(f.model.root(2)).len(), 1, "B is under D");
        // And straight back out, which is the inverse.
        f.tree.layout(SizeProposal::exact(400.0, 400.0));
        f.tree.press_key(Key::ArrowLeft, Modifiers::ALT);
        assert_eq!(shape(&f)[0].1, vec!["A", "C", "D", "B"]);
    }

    /// **A custom action** for each.
    #[test]
    fn a_row_advertises_its_moves_and_reparents_as_custom_actions() {
        let mut f = fixture();
        let target = row(&f.tree, f.view, 1);
        let advertised = custom_actions(&mut f.tree, target);
        for expected in ["Move Up", "Move to Bottom", "Move Into Previous"] {
            assert!(
                advertised.iter().any(|a| a == expected),
                "{expected:?} is not advertised: {advertised:?}"
            );
        }
        assert!(invoke_custom_action(
            &mut f.tree,
            target,
            "Move Into Previous"
        ));
        assert_eq!(f.model.root_count(), 3);
    }

    /// **The same model change the drag makes** — for a reparent, compared
    /// against a real drop into the middle third of the target row.
    #[test]
    fn every_route_reparents_the_row_the_way_a_drop_into_does() {
        // The drag: B (row 1) released over the middle third of A (row 0).
        let dragged = {
            let mut f = fixture();
            drag(
                &mut f.tree,
                Point::new(50.0, ROW * 1.5),
                Point::new(50.0, ROW * 0.5),
            );
            shape(&f)
        };
        assert_eq!(
            dragged[0].1,
            vec!["A", "C", "D"],
            "the drag itself did nothing — the comparison would be vacuous"
        );
        let by_key = {
            let mut f = fixture();
            f.selection.select(1);
            f.tree.focus(f.view);
            f.tree.press_key(Key::ArrowRight, Modifiers::ALT);
            shape(&f)
        };
        let by_action = {
            let mut f = fixture();
            let target = row(&f.tree, f.view, 1);
            assert!(invoke_custom_action(
                &mut f.tree,
                target,
                "Move Into Previous"
            ));
            shape(&f)
        };
        let by_menu = {
            let mut f = fixture();
            let centre = f.tree.bounds(row(&f.tree, f.view, 1)).center();
            right_click(&mut f.tree, centre);
            f.tree.layout(SizeProposal::exact(400.0, 400.0));
            assert!(click_menu_row(&mut f.tree, "Move Into Previous"));
            shape(&f)
        };
        assert_eq!(by_key, dragged, "the chord and the drop disagree");
        assert_eq!(
            by_action, dragged,
            "the custom action and the drop disagree"
        );
        assert_eq!(by_menu, dragged, "the menu row and the drop disagree");
    }

    /// …and for a sibling move, against a real drop in the gap between rows.
    #[test]
    fn every_route_reorders_the_row_the_way_a_gap_drop_does() {
        let dragged = {
            let mut f = fixture();
            // A (row 0) released just past D's row — the gap after the last root.
            drag(
                &mut f.tree,
                Point::new(50.0, ROW * 0.5),
                Point::new(50.0, ROW * 4.0 - 2.0),
            );
            shape(&f)
        };
        assert_eq!(dragged[0].1, vec!["B", "C", "D", "A"]);
        let by_key = {
            let mut f = fixture();
            f.selection.select(0);
            f.tree.focus(f.view);
            f.tree.press_key(Key::End, Modifiers::ALT);
            shape(&f)
        };
        let by_action = {
            let mut f = fixture();
            let target = row(&f.tree, f.view, 0);
            assert!(invoke_custom_action(&mut f.tree, target, "Move to Bottom"));
            shape(&f)
        };
        assert_eq!(by_key, dragged, "the chord and the drag disagree");
        assert_eq!(
            by_action, dragged,
            "the custom action and the drag disagree"
        );
    }

    /// **Announces once** — a sibling move names the new position among
    /// siblings; a reparent names the new level, because that is what changed.
    #[test]
    fn a_move_announces_its_position_and_a_reparent_its_level() {
        let mut f = fixture();
        f.selection.select(1);
        f.tree.focus(f.view);
        f.tree.press_key(Key::ArrowDown, Modifiers::ALT);
        assert_eq!(spoken(&mut f.tree), vec!["B moved to 3 of 4"]);

        let mut g = fixture();
        g.selection.select(1);
        g.tree.focus(g.view);
        g.tree.press_key(Key::ArrowRight, Modifiers::ALT);
        assert_eq!(spoken(&mut g.tree), vec!["B moved to level 2"]);
    }

    /// The reparent's portable spelling — the accelerator plus `]` / `[` — works
    /// too, and it is the only one bound on macOS.
    #[test]
    fn the_bracket_pair_reparents_on_every_platform() {
        let mut f = fixture();
        f.selection.select(1);
        f.tree.focus(f.view);
        f.tree.press_key(Key::Character(']'), Modifiers::COMMAND);
        assert_eq!(shape(&f)[0].1, vec!["A", "C", "D"], "B indented under A");
        f.tree.layout(SizeProposal::exact(400.0, 400.0));
        f.tree.press_key(Key::Character('['), Modifiers::COMMAND);
        assert_eq!(shape(&f)[0].1, vec!["A", "B", "C", "D"], "and back out");
    }

    /// The moves are gated on the row's **sibling** set, not on the
    /// flattening — a row last among its siblings cannot move down however many
    /// rows follow it on screen.
    ///
    /// A tree of four roots cannot tell the two apart, so this one nests and
    /// expands. The slice handle is only reachable from inside a delegate call,
    /// which is why the fixture captures it there.
    #[test]
    fn the_moves_are_gated_on_the_siblings_not_on_the_flattening() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let model: TreeModel<&'static str> = TreeModel::new();
        let a = model.insert_root(0, "A");
        model.insert_child(a, 0, "A1");
        model.insert_child(a, 1, "A2");
        let b = model.insert_root(1, "B");
        model.insert_child(b, 0, "B1");
        model.insert_root(2, "C");
        let selection = SelectionModel::new(SelectionMode::Single);
        let slice: Rc<RefCell<Option<teksilo_data::tree_slice::TreeSliceHandle<&'static str>>>> =
            Rc::new(RefCell::new(None));
        let mut tree = WidgetTree::new();
        let view = {
            let slice = slice.clone();
            tree.add(
                TreeView::new_with_context(
                    model.clone(),
                    move |_item: &&'static str, entry, _sel, cx| {
                        if slice.borrow().is_none() {
                            *slice.borrow_mut() = Some(cx.slice_handle());
                        }
                        Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, ROW))
                            as Box<dyn Widget>
                    },
                )
                .item_height(ROW)
                .selection(selection.clone())
                .reorderable(true),
            )
        };
        tree.layout(SizeProposal::exact(400.0, 400.0));
        slice
            .borrow()
            .as_ref()
            .expect("a delegate ran and handed over the slice")
            .expand_all();
        tree.layout(SizeProposal::exact(400.0, 400.0));
        let rows = nodes_with_role(&tree, view, teksilo_core::accesskit::Role::TreeItem);
        assert_eq!(rows.len(), 6, "A, A1, A2, B, B1, C");

        // A2 is flat index 2 with three rows below it, and the LAST of A's two
        // children. Neither move toward the end is available to it.
        let mut f = Fixture {
            tree,
            view,
            model,
            selection,
        };
        let a2 = rows[2];
        let advertised = custom_actions(&mut f.tree, a2);
        assert!(advertised.iter().any(|x| x == "Move Up"), "{advertised:?}");
        assert!(
            !advertised.iter().any(|x| x == "Move Down"),
            "A2 is the last of its siblings: {advertised:?}"
        );
        assert!(
            !advertised.iter().any(|x| x == "Move to Bottom"),
            "{advertised:?}"
        );

        // And Alt+End on A1 lands it at the end of A's children, not at the end
        // of the flattening.
        f.selection.select(1);
        f.tree.focus(f.view);
        f.tree.press_key(Key::End, Modifiers::ALT);
        let roots = f.model.root(0);
        assert_eq!(
            f.model
                .children(roots)
                .into_iter()
                .map(|n| f.model.with_item(n, |v| *v).expect("live"))
                .collect::<Vec<_>>(),
            vec!["A2", "A1"],
            "A1 moved to the end of its own siblings"
        );
        assert_eq!(shape(&f)[0].1, vec!["A", "B", "C"], "and stayed inside A");
    }

    /// An indent reveals its new parent, or the row it just moved would be
    /// unreachable — no cursor on it, no way back out.
    #[test]
    fn an_indent_reveals_the_row_it_moved() {
        let mut f = fixture();
        f.selection.select(1);
        f.tree.focus(f.view);
        f.tree.press_key(Key::ArrowRight, Modifiers::ALT);
        f.tree.layout(SizeProposal::exact(400.0, 400.0));
        let rows = nodes_with_role(&f.tree, f.view, teksilo_core::accesskit::Role::TreeItem);
        assert_eq!(rows.len(), 4, "the indented row is still on screen");
        assert_eq!(
            f.selection.selected_indices(),
            vec![1],
            "and the selection followed it"
        );
    }
}

// ---------------------------------------------------------------------------
// Census rows 11, 12, 13 — TableView / TreeTableView / GridView reorder
// ---------------------------------------------------------------------------

mod table_view_row_reorder {
    use super::*;
    use teksilo_data::{ListModel, SelectionMode, SelectionModel};
    use teksilo_widgets::table_view::{Column, ColumnWidth, TableSelectionMode, TableView};

    const ROW: f32 = 30.0;

    struct Fixture {
        tree: WidgetTree,
        view: WidgetId,
        model: ListModel<&'static str>,
        selection: SelectionModel,
    }

    fn fixture() -> Fixture {
        let model = ListModel::from_vec(vec!["alpha", "beta", "gamma", "delta"]);
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new();
        let view = tree.add(
            TableView::new(model.clone())
                .add_column(
                    Column::new(
                        "name",
                        teksilo_i18n::lit!("Name"),
                        |_v: &&'static str, _cx| Box::new(FixedLeaf(180.0, ROW)) as Box<dyn Widget>,
                    )
                    .width(ColumnWidth::Fixed(180.0)),
                )
                .header_height(0.0)
                .row_height(ROW)
                .selection_mode(TableSelectionMode::SingleRow)
                .selection(selection.clone())
                .type_ahead_label(|item: &&'static str| (*item).to_string())
                .reorderable(true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        Fixture {
            tree,
            view,
            model,
            selection,
        }
    }

    fn order(model: &ListModel<&'static str>) -> Vec<&'static str> {
        (0..model.len())
            .map(|i| model.with_item(i, |v| *v).expect("in range"))
            .collect()
    }

    fn row(tree: &WidgetTree, view: WidgetId, index: usize) -> WidgetId {
        nodes_with_role(tree, view, teksilo_core::accesskit::Role::Row)[index]
    }

    /// **Exists.**
    #[test]
    fn a_row_offers_the_four_moves_in_its_context_menu() {
        let mut f = fixture();
        let centre = f.tree.bounds(row(&f.tree, f.view, 1)).center();
        right_click(&mut f.tree, centre);
        let labels = a11y_labels(&mut f.tree);
        for expected in ["Move Up", "Move Down", "Move to Top", "Move to Bottom"] {
            assert!(
                labels.iter().any(|l| l == expected),
                "no {expected:?} row: {labels:?}"
            );
        }
    }

    /// **Keyboard-reachable** — Alt+Home / Alt+End are new here, and so is
    /// every one of the four for a table with no selection model at all.
    #[test]
    fn alt_home_and_alt_end_reach_both_ends_from_the_keyboard() {
        let mut f = fixture();
        f.selection.select(2);
        f.tree.focus(f.view);
        f.tree.press_key(Key::Home, Modifiers::ALT);
        assert_eq!(order(&f.model), vec!["gamma", "alpha", "beta", "delta"]);
        f.tree.press_key(Key::End, Modifiers::ALT);
        assert_eq!(order(&f.model), vec!["alpha", "beta", "delta", "gamma"]);
    }

    /// **A custom action.**
    #[test]
    fn a_row_advertises_the_moves_as_custom_actions() {
        let mut f = fixture();
        let target = row(&f.tree, f.view, 1);
        let advertised = custom_actions(&mut f.tree, target);
        for expected in ["Move Up", "Move Down", "Move to Top", "Move to Bottom"] {
            assert!(
                advertised.iter().any(|a| a == expected),
                "{expected:?} is not advertised: {advertised:?}"
            );
        }
        assert!(invoke_custom_action(&mut f.tree, target, "Move to Bottom"));
        assert_eq!(order(&f.model), vec!["alpha", "gamma", "delta", "beta"]);
    }

    /// **The same model change the drag makes.**
    #[test]
    fn every_route_lands_the_row_where_the_drag_lands_it() {
        let dragged = {
            let mut f = fixture();
            drag(
                &mut f.tree,
                Point::new(50.0, ROW * 0.5),
                Point::new(50.0, ROW * 3.0),
            );
            order(&f.model)
        };
        assert_ne!(
            dragged,
            vec!["alpha", "beta", "gamma", "delta"],
            "the drag itself did nothing — the comparison would be vacuous"
        );
        let steps = dragged.iter().position(|v| *v == "alpha").expect("present");
        let by_key = {
            let mut f = fixture();
            f.selection.select(0);
            f.tree.focus(f.view);
            for _ in 0..steps {
                f.tree.press_key(Key::ArrowDown, Modifiers::ALT);
            }
            order(&f.model)
        };
        let by_action = {
            let mut f = fixture();
            for _ in 0..steps {
                let live = order(&f.model)
                    .iter()
                    .position(|v| *v == "alpha")
                    .expect("present");
                let target = row(&f.tree, f.view, live);
                assert!(invoke_custom_action(&mut f.tree, target, "Move Down"));
            }
            order(&f.model)
        };
        assert_eq!(by_key, dragged, "the chord and the drag disagree");
        assert_eq!(
            by_action, dragged,
            "the custom action and the drag disagree"
        );
    }

    /// **Announces once.**
    #[test]
    fn a_completed_move_announces_its_new_position_once() {
        let mut f = fixture();
        f.selection.select(0);
        f.tree.focus(f.view);
        f.tree.press_key(Key::ArrowDown, Modifiers::ALT);
        assert_eq!(spoken(&mut f.tree), vec!["alpha moved to 2 of 4"]);
    }
}

mod tree_table_view_row_reorder {
    use super::*;
    use teksilo_data::{NodeId, SelectionMode, SelectionModel, TreeModel};
    use teksilo_widgets::TreeTableView;
    use teksilo_widgets::table_view::{Column, ColumnWidth, TableSelectionMode};

    const ROW: f32 = 30.0;

    struct Fixture {
        tree: WidgetTree,
        view: WidgetId,
        model: TreeModel<&'static str>,
        selection: SelectionModel,
    }

    fn fixture() -> Fixture {
        let model: TreeModel<&'static str> = TreeModel::new();
        for (i, name) in ["A", "B", "C", "D"].into_iter().enumerate() {
            model.insert_root(i, name);
        }
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new();
        let view = tree.add(
            TreeTableView::new(model.clone())
                .add_column(
                    Column::new(
                        "name",
                        teksilo_i18n::lit!("Name"),
                        |_v: &&'static str, _cx| Box::new(FixedLeaf(180.0, ROW)) as Box<dyn Widget>,
                    )
                    .width(ColumnWidth::Fixed(180.0)),
                )
                .header_height(0.0)
                .row_height(ROW)
                .selection_mode(TableSelectionMode::SingleRow)
                .selection(selection.clone())
                .type_ahead_label(|item: &&'static str| (*item).to_string())
                .reorderable(true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        Fixture {
            tree,
            view,
            model,
            selection,
        }
    }

    fn roots(f: &Fixture) -> Vec<&'static str> {
        (0..f.model.root_count())
            .map(|i| {
                f.model
                    .with_item(f.model.root(i), |v| *v)
                    .expect("live node")
            })
            .collect()
    }

    fn row(tree: &WidgetTree, view: WidgetId, index: usize) -> WidgetId {
        nodes_with_role(tree, view, teksilo_core::accesskit::Role::Row)[index]
    }

    fn child_names(f: &Fixture, node: NodeId) -> Vec<&'static str> {
        f.model
            .children(node)
            .into_iter()
            .map(|n| f.model.with_item(n, |v| *v).expect("live node"))
            .collect()
    }

    /// **Exists** — the sibling moves and the indent.
    #[test]
    fn a_row_offers_its_moves_and_its_reparents() {
        let mut f = fixture();
        let centre = f.tree.bounds(row(&f.tree, f.view, 1)).center();
        right_click(&mut f.tree, centre);
        let labels = a11y_labels(&mut f.tree);
        for expected in ["Move Up", "Move to Bottom", "Move Into Previous"] {
            assert!(
                labels.iter().any(|l| l == expected),
                "no {expected:?} row: {labels:?}"
            );
        }
    }

    /// **Keyboard-reachable** — the reparents had no keyboard route before.
    #[test]
    fn alt_arrows_reorder_and_reparent_from_the_keyboard() {
        let mut f = fixture();
        f.selection.select(1);
        f.tree.focus(f.view);
        f.tree.press_key(Key::End, Modifiers::ALT);
        assert_eq!(roots(&f), vec!["A", "C", "D", "B"]);
        f.tree.layout(SizeProposal::exact(400.0, 300.0));
        f.tree.press_key(Key::ArrowRight, Modifiers::ALT);
        assert_eq!(roots(&f), vec!["A", "C", "D"]);
        assert_eq!(child_names(&f, f.model.root(2)), vec!["B"]);
    }

    /// **A custom action.**
    #[test]
    fn a_row_advertises_its_moves_as_custom_actions() {
        let mut f = fixture();
        let target = row(&f.tree, f.view, 1);
        let advertised = custom_actions(&mut f.tree, target);
        for expected in ["Move Up", "Move to Bottom", "Move Into Previous"] {
            assert!(
                advertised.iter().any(|a| a == expected),
                "{expected:?} is not advertised: {advertised:?}"
            );
        }
        assert!(invoke_custom_action(
            &mut f.tree,
            target,
            "Move Into Previous"
        ));
        assert_eq!(roots(&f), vec!["A", "C", "D"]);
        assert_eq!(child_names(&f, f.model.root(0)), vec!["B"]);
    }

    /// **The same model change the drag makes** — against a real drop into the
    /// middle third of the target row.
    #[test]
    fn every_route_reparents_the_row_the_way_a_drop_into_does() {
        let dragged = {
            let mut f = fixture();
            drag(
                &mut f.tree,
                Point::new(50.0, ROW * 1.5),
                Point::new(50.0, ROW * 0.5),
            );
            (roots(&f), child_names(&f, f.model.root(0)))
        };
        assert_eq!(
            dragged,
            (vec!["A", "C", "D"], vec!["B"]),
            "the drag itself did not reparent — the comparison would be vacuous"
        );
        let by_key = {
            let mut f = fixture();
            f.selection.select(1);
            f.tree.focus(f.view);
            f.tree.press_key(Key::ArrowRight, Modifiers::ALT);
            (roots(&f), child_names(&f, f.model.root(0)))
        };
        let by_action = {
            let mut f = fixture();
            let target = row(&f.tree, f.view, 1);
            assert!(invoke_custom_action(
                &mut f.tree,
                target,
                "Move Into Previous"
            ));
            (roots(&f), child_names(&f, f.model.root(0)))
        };
        let by_menu = {
            let mut f = fixture();
            let centre = f.tree.bounds(row(&f.tree, f.view, 1)).center();
            right_click(&mut f.tree, centre);
            f.tree.layout(SizeProposal::exact(400.0, 300.0));
            assert!(click_menu_row(&mut f.tree, "Move Into Previous"));
            (roots(&f), child_names(&f, f.model.root(0)))
        };
        assert_eq!(by_key, dragged, "the chord and the drop disagree");
        assert_eq!(
            by_action, dragged,
            "the custom action and the drop disagree"
        );
        assert_eq!(by_menu, dragged, "the menu row and the drop disagree");
    }

    /// **Announces once.**
    #[test]
    fn a_move_announces_its_position_and_a_reparent_its_level() {
        let mut f = fixture();
        f.selection.select(1);
        f.tree.focus(f.view);
        f.tree.press_key(Key::ArrowDown, Modifiers::ALT);
        assert_eq!(spoken(&mut f.tree), vec!["B moved to 3 of 4"]);

        let mut g = fixture();
        g.selection.select(1);
        g.tree.focus(g.view);
        g.tree.press_key(Key::ArrowRight, Modifiers::ALT);
        assert_eq!(spoken(&mut g.tree), vec!["B moved to level 2"]);
    }
}

mod grid_view_tile_reorder {
    use super::*;
    use teksilo_data::{ListModel, SelectionMode, SelectionModel};
    use teksilo_widgets::{GridSizing, GridView};

    const TILE: f32 = 40.0;

    struct Fixture {
        tree: WidgetTree,
        view: WidgetId,
        model: ListModel<&'static str>,
        selection: SelectionModel,
    }

    fn fixture() -> Fixture {
        let model = ListModel::from_vec(vec!["a", "b", "c", "d", "e", "f"]);
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new();
        let view = tree.add(
            GridView::new(model.clone(), |_cx| {
                Box::new(FixedLeaf(TILE, TILE)) as Box<dyn Widget>
            })
            .sizing(GridSizing::Fixed {
                width: TILE,
                height: TILE,
            })
            .selection(selection.clone())
            .type_ahead_label(|i: usize| ["a", "b", "c", "d", "e", "f"][i].to_string())
            .reorderable(true),
        );
        tree.layout(SizeProposal::exact(TILE * 3.0, TILE * 3.0));
        Fixture {
            tree,
            view,
            model,
            selection,
        }
    }

    fn order(model: &ListModel<&'static str>) -> Vec<&'static str> {
        (0..model.len())
            .map(|i| model.with_item(i, |v| *v).expect("in range"))
            .collect()
    }

    fn tile(tree: &WidgetTree, view: WidgetId, index: usize) -> WidgetId {
        nodes_with_role(tree, view, teksilo_core::accesskit::Role::GridCell)[index]
    }

    /// **Exists.**
    #[test]
    fn a_tile_offers_the_four_moves_in_its_context_menu() {
        let mut f = fixture();
        let centre = f.tree.bounds(tile(&f.tree, f.view, 1)).center();
        right_click(&mut f.tree, centre);
        let labels = a11y_labels(&mut f.tree);
        for expected in ["Move Left", "Move Right", "Move to Start", "Move to End"] {
            assert!(
                labels.iter().any(|l| l == expected),
                "no {expected:?} row: {labels:?}"
            );
        }
    }

    /// **Keyboard-reachable** — Alt+Home / Alt+End are new; Alt+Left / Right
    /// step, and a whole-row step (Alt+Down) is the grid's own.
    #[test]
    fn alt_home_and_alt_end_reach_both_ends_from_the_keyboard() {
        let mut f = fixture();
        f.selection.select(2);
        f.tree.focus(f.view);
        f.tree.press_key(Key::Home, Modifiers::ALT);
        assert_eq!(order(&f.model), vec!["c", "a", "b", "d", "e", "f"]);
        f.tree.press_key(Key::End, Modifiers::ALT);
        assert_eq!(order(&f.model), vec!["a", "b", "d", "e", "f", "c"]);
    }

    /// A whole-row step is the grid's own move, not one of the four named ones,
    /// and it still works — the shared decoder must not have swallowed it.
    ///
    /// The step is a *column count*, which the layout decides, so the
    /// destination is read off the grid's own AccessKit column count rather
    /// than assumed: a scrollbar's width changes it.
    #[test]
    fn a_whole_row_step_still_works() {
        let mut f = fixture();
        let cols = f
            .tree
            .sync_accessibility()
            .nodes
            .iter()
            .find(|(id, _)| *id == widget_id_to_node_id(f.view))
            .and_then(|(_, node)| node.column_count())
            .expect("the grid publishes its column count");
        assert!(
            (2..6).contains(&cols),
            "a one-row grid cannot step by a row"
        );
        f.selection.select(0);
        f.tree.focus(f.view);
        f.tree.press_key(Key::ArrowDown, Modifiers::ALT);
        assert_eq!(
            order(&f.model).iter().position(|v| *v == "a"),
            Some(cols),
            "a whole-row step moves the tile by exactly one row"
        );
    }

    /// **A custom action.**
    #[test]
    fn a_tile_advertises_the_moves_as_custom_actions() {
        let mut f = fixture();
        let target = tile(&f.tree, f.view, 1);
        let advertised = custom_actions(&mut f.tree, target);
        for expected in ["Move Left", "Move Right", "Move to Start", "Move to End"] {
            assert!(
                advertised.iter().any(|a| a == expected),
                "{expected:?} is not advertised: {advertised:?}"
            );
        }
        assert!(invoke_custom_action(&mut f.tree, target, "Move to End"));
        assert_eq!(order(&f.model), vec!["a", "c", "d", "e", "f", "b"]);
    }

    /// **The same model change the drag makes.**
    #[test]
    fn every_route_lands_the_tile_where_the_drag_lands_it() {
        let dragged = {
            let mut f = fixture();
            let from = f.tree.bounds(tile(&f.tree, f.view, 0)).center();
            let to = f.tree.bounds(tile(&f.tree, f.view, 2)).center();
            drag(&mut f.tree, from, to);
            order(&f.model)
        };
        assert_ne!(
            dragged,
            vec!["a", "b", "c", "d", "e", "f"],
            "the drag itself did nothing — the comparison would be vacuous"
        );
        let steps = dragged.iter().position(|v| *v == "a").expect("present");
        let by_key = {
            let mut f = fixture();
            f.selection.select(0);
            f.tree.focus(f.view);
            for _ in 0..steps {
                f.tree.press_key(Key::ArrowRight, Modifiers::ALT);
            }
            order(&f.model)
        };
        let by_action = {
            let mut f = fixture();
            for _ in 0..steps {
                let live = order(&f.model)
                    .iter()
                    .position(|v| *v == "a")
                    .expect("present");
                let target = tile(&f.tree, f.view, live);
                assert!(invoke_custom_action(&mut f.tree, target, "Move Right"));
            }
            order(&f.model)
        };
        assert_eq!(by_key, dragged, "the chord and the drag disagree");
        assert_eq!(
            by_action, dragged,
            "the custom action and the drag disagree"
        );
    }

    /// **Announces once** — and the grid's own selection live region does not
    /// speak beside it. A grid publishes "N items selected" as a live value, so
    /// this is the one view where the coalescing has to be checked against a
    /// widget that already talks: the selection the move carries forward is the
    /// selection the user already had, the value does not change, and the move
    /// is the only thing said.
    #[test]
    fn a_completed_move_announces_its_new_position_once() {
        let mut f = fixture();
        f.selection.select(0);
        f.tree.focus(f.view);
        // The frame in which the user made that selection — which is where the
        // grid says "1 item selected".
        assert_eq!(spoken(&mut f.tree), vec!["1 item selected"]);
        let before = f.tree.announcements_since(0).len() as u64;
        f.tree.press_key(Key::ArrowRight, Modifiers::ALT);
        let _ = f.tree.sync_accessibility();
        let after: Vec<String> = f
            .tree
            .announcements_since(before)
            .into_iter()
            .map(|a| a.text)
            .collect();
        assert_eq!(after, vec!["a moved to 2 of 6"]);
    }
}

// ---------------------------------------------------------------------------
// Census row 16 — tab reorder within a bar
// ---------------------------------------------------------------------------

mod tab_reorder {
    use super::*;
    use teksilo_core::signal::Signal;
    use teksilo_data::ListModel;
    use teksilo_widgets::tab_widget::{TabHandle, TabId, TabInfo, TabWidget};

    struct Fixture {
        tree: WidgetTree,
        model: ListModel<TabHandle>,
        /// Title → id, so the model's order can be read back as names. `TabInfo`
        /// keeps its title private, and the id is what the reorder handler moves.
        ids: Vec<(&'static str, TabId)>,
    }

    fn fixture() -> Fixture {
        let selected: Signal<Option<TabId>> = Signal::new(None);
        let ids: Vec<(&'static str, TabId)> = ["A", "B", "C", "D"]
            .into_iter()
            .map(|t| (t, TabId::fresh()))
            .collect();
        let model = ListModel::from_vec(
            ids.iter()
                .map(|(t, id)| {
                    TabHandle::dynamic(*id, "doc", TabInfo::new().title(teksilo_i18n::lit!(*t)), ())
                })
                .collect::<Vec<_>>(),
        );
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(
            TabWidget::new(selected)
                .dynamic_tab::<()>("doc", |_h, _s| {
                    Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
                })
                .dynamic_model(model.clone())
                .reorderable(true)
                .show_scroll_arrows(false)
                .show_overflow_dropdown(false),
        );
        tree.layout(SizeProposal::exact(900.0, 400.0));
        Fixture { tree, model, ids }
    }

    fn order(f: &Fixture) -> Vec<&'static str> {
        (0..f.model.len())
            .map(|i| {
                let id = f.model.with_item(i, |h| h.id).expect("in range");
                f.ids
                    .iter()
                    .find(|(_, known)| *known == id)
                    .map(|(name, _)| *name)
                    .expect("a tab the fixture created")
            })
            .collect()
    }

    fn headers(tree: &WidgetTree) -> Vec<WidgetId> {
        let root = tree.roots()[0];
        nodes_with_role(tree, root, teksilo_core::accesskit::Role::Tab)
    }

    /// **Exists** — the moves are real menu rows on a right-click, which is a
    /// route a tab bar had none of.
    #[test]
    fn a_tab_offers_the_four_moves_in_its_context_menu() {
        let mut f = fixture();
        let centre = f.tree.bounds(headers(&f.tree)[1]).center();
        right_click(&mut f.tree, centre);
        let labels = a11y_labels(&mut f.tree);
        for expected in ["Move Left", "Move Right", "Move to Start", "Move to End"] {
            assert!(
                labels.iter().any(|l| l == expected),
                "no {expected:?} row: {labels:?}"
            );
        }
    }

    /// **Keyboard-reachable** — a tab bar had an AT route and no chord at all.
    #[test]
    fn alt_arrows_and_alt_ends_move_a_focused_tab() {
        let mut f = fixture();
        let second = headers(&f.tree)[1];
        f.tree.focus(second);
        f.tree.press_key(Key::ArrowRight, Modifiers::ALT);
        assert_eq!(order(&f), vec!["A", "C", "B", "D"]);
        f.tree.layout(SizeProposal::exact(900.0, 400.0));
        let moved = headers(&f.tree)[2];
        f.tree.focus(moved);
        f.tree.press_key(Key::Home, Modifiers::ALT);
        assert_eq!(order(&f), vec!["B", "A", "C", "D"]);
    }

    /// …and the bare arrows still navigate. The reorder chord is read ahead of
    /// the navigation arms, which do not look at the modifiers, so this is the
    /// assertion that says the read order did not swallow them.
    #[test]
    fn the_bare_arrows_still_change_the_selected_tab() {
        let mut f = fixture();
        let before = order(&f);
        let second = headers(&f.tree)[1];
        f.tree.focus(second);
        f.tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(order(&f), before, "a bare arrow reordered nothing");
        assert_eq!(
            f.tree.focused(),
            Some(headers(&f.tree)[2]),
            "and it moved the cursor to the next tab"
        );
    }

    /// **A custom action** — the two far ends beside the two steps that shipped.
    #[test]
    fn a_tab_advertises_the_far_ends_as_custom_actions() {
        let mut f = fixture();
        let target = headers(&f.tree)[1];
        let advertised = custom_actions(&mut f.tree, target);
        assert_eq!(
            advertised,
            vec!["Move Left", "Move Right", "Move to Start", "Move to End"]
        );
        assert!(invoke_custom_action(&mut f.tree, target, "Move to End"));
        assert_eq!(order(&f), vec!["A", "C", "D", "B"]);
    }

    /// **The same model change the drag makes** — compared against a real
    /// header drag across the bar.
    #[test]
    fn every_route_lands_the_tab_where_the_drag_lands_it() {
        let dragged = {
            let mut f = fixture();
            let hs = headers(&f.tree);
            let from = f.tree.bounds(hs[0]).center();
            let to = f.tree.bounds(hs[2]).center();
            drag(&mut f.tree, from, to);
            order(&f)
        };
        assert_ne!(
            dragged,
            vec!["A", "B", "C", "D"],
            "the drag itself did nothing — the comparison would be vacuous"
        );
        let steps = dragged.iter().position(|t| *t == "A").expect("present");
        let by_key = {
            let mut f = fixture();
            for _ in 0..steps {
                f.tree.layout(SizeProposal::exact(900.0, 400.0));
                let live = order(&f).iter().position(|t| *t == "A").expect("present");
                let header = headers(&f.tree)[live];
                f.tree.focus(header);
                f.tree.press_key(Key::ArrowRight, Modifiers::ALT);
            }
            order(&f)
        };
        let by_action = {
            let mut f = fixture();
            for _ in 0..steps {
                f.tree.layout(SizeProposal::exact(900.0, 400.0));
                let live = order(&f).iter().position(|t| *t == "A").expect("present");
                let header = headers(&f.tree)[live];
                assert!(invoke_custom_action(&mut f.tree, header, "Move Right"));
            }
            order(&f)
        };
        let by_menu = {
            let mut f = fixture();
            for _ in 0..steps {
                f.tree.layout(SizeProposal::exact(900.0, 400.0));
                let live = order(&f).iter().position(|t| *t == "A").expect("present");
                let centre = f.tree.bounds(headers(&f.tree)[live]).center();
                right_click(&mut f.tree, centre);
                f.tree.layout(SizeProposal::exact(900.0, 400.0));
                assert!(click_menu_row(&mut f.tree, "Move Right"));
            }
            order(&f)
        };
        assert_eq!(by_key, dragged, "the chord and the drag disagree");
        assert_eq!(
            by_action, dragged,
            "the custom action and the drag disagree"
        );
        assert_eq!(by_menu, dragged, "the menu row and the drag disagree");
    }

    /// **Announces once**, naming the tab and its new position.
    #[test]
    fn a_completed_move_announces_its_new_position_once() {
        let mut f = fixture();
        let second = headers(&f.tree)[1];
        f.tree.focus(second);
        f.tree.press_key(Key::ArrowRight, Modifiers::ALT);
        assert_eq!(spoken(&mut f.tree), vec!["B moved to 3 of 4"]);
    }
}

// ---------------------------------------------------------------------------
// Census row 4 — the 2-D saturation × brightness drag
// ---------------------------------------------------------------------------

mod hsv_canvas {
    use super::*;
    use teksilo_core::signal::Signal;
    use teksilo_tokens::Color;
    use teksilo_widgets::ColorPicker;

    /// A picker with everything at its default, because the point of this row is
    /// that an application has to ask for nothing.
    fn fixture() -> (WidgetTree, Signal<Color>) {
        let value = Signal::new(Color::from_hsv(20.0, 0.5, 0.5));
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(ColorPicker::new(value.clone()));
        tree.layout(SizeProposal::exact(600.0, 700.0));
        (tree, value)
    }

    /// The 2-D field: a named group, which is what makes it reachable at all.
    fn field(tree: &WidgetTree) -> WidgetId {
        let root = tree.roots()[0];
        nodes_with_role(tree, root, teksilo_core::accesskit::Role::Group)
            .into_iter()
            .find(|&id| tree.accessibility_node(id).name() == Some("Saturation and brightness"))
            .expect("the picker builds a saturation-and-brightness field")
    }

    /// **Exists** — the numeric entry beside the canvas, on by default. That is
    /// this row's menu-equivalent: A19 words the alternative as numeric entry,
    /// and a colour field has no menu to put rows in.
    #[test]
    fn a_default_picker_shows_its_saturation_and_brightness_numbers() {
        use teksilo_widgets::color_picker::ColorPickerLayout;
        let (tree, _value) = fixture();
        let with_default = nodes_with_role(
            &tree,
            tree.roots()[0],
            teksilo_core::accesskit::Role::SpinButton,
        )
        .len();
        // Turning the row back off is what says the count above is the HSV row
        // and not something else the picker happens to build.
        let value = Signal::new(Color::from_hsv(20.0, 0.5, 0.5));
        let mut off = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        off.add(ColorPicker::new(value).show_hsv_spinners(false));
        off.layout(SizeProposal::exact(600.0, 700.0));
        let without = nodes_with_role(
            &off,
            off.roots()[0],
            teksilo_core::accesskit::Role::SpinButton,
        )
        .len();
        assert_eq!(
            with_default - without,
            3,
            "the default picker must carry the H/S/V numeric entry              ({with_default} spin buttons by default, {without} with it off)"
        );
        // Compact has no room for it, which is why the field's own arrows are
        // the route that has to exist everywhere.
        let value = Signal::new(Color::from_hsv(20.0, 0.5, 0.5));
        let mut compact = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        compact.add(ColorPicker::new(value).layout(ColorPickerLayout::Compact));
        compact.layout(SizeProposal::exact(300.0, 400.0));
        assert!(
            nodes_with_role(
                &compact,
                compact.roots()[0],
                teksilo_core::accesskit::Role::SpinButton,
            )
            .is_empty(),
            "a compact picker builds no numeric entry"
        );
    }

    /// **Keyboard-reachable** — the field is a Tab stop and the arrows step it.
    ///
    /// The Tab-stop assertion is the load-bearing half: `WidgetTree::focus`
    /// succeeds on a node that is not focusable, so focusing the field
    /// explicitly and then pressing a key proves the handler runs and says
    /// nothing about whether a keyboard user can ever get there. Deleting
    /// `focusable(true)` left every other assertion in this module green.
    #[test]
    fn the_arrows_step_saturation_and_brightness() {
        let (mut tree, value) = fixture();
        let canvas = field(&tree);
        assert!(
            tree.tab_stops_within(tree.roots()[0]).contains(&canvas),
            "the field is not reachable by Tab"
        );
        tree.focus(canvas);
        let (_, s0, v0) = value.get().to_hsv();
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        let (_, s1, v1) = value.get().to_hsv();
        assert!(s1 > s0, "ArrowRight raises saturation: {s0} -> {s1}");
        assert!((v1 - v0).abs() < 0.01, "and leaves brightness alone");
        tree.press_key(Key::ArrowUp, Modifiers::NONE);
        let (_, _, v2) = value.get().to_hsv();
        assert!(v2 > v1, "ArrowUp raises brightness: {v1} -> {v2}");
    }

    /// Shift steps ten times as far, so reaching the far end is not a hundred
    /// presses.
    #[test]
    fn shift_makes_the_step_coarse() {
        let (mut tree, value) = fixture();
        let canvas = field(&tree);
        tree.focus(canvas);
        let (_, s0, _) = value.get().to_hsv();
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        let (_, fine, _) = value.get().to_hsv();
        let (mut tree, value) = fixture();
        let canvas = field(&tree);
        tree.focus(canvas);
        tree.press_key(Key::ArrowRight, Modifiers::SHIFT);
        let (_, coarse, _) = value.get().to_hsv();
        assert!(
            coarse - s0 > (fine - s0) * 5.0,
            "coarse {coarse} is not meaningfully further than fine {fine} from {s0}"
        );
    }

    /// **A custom action** per direction. `Action::Increment` cannot serve a
    /// control whose value is a pair — it would not say which axis — so the four
    /// are named.
    #[test]
    fn the_field_advertises_a_named_step_per_axis() {
        let (mut tree, value) = fixture();
        let canvas = field(&tree);
        let advertised = custom_actions(&mut tree, canvas);
        for expected in [
            "Decrease saturation",
            "Increase saturation",
            "Decrease brightness",
            "Increase brightness",
        ] {
            assert!(
                advertised.iter().any(|a| a == expected),
                "{expected:?} is not advertised: {advertised:?}"
            );
        }
        let (_, s0, _) = value.get().to_hsv();
        assert!(invoke_custom_action(
            &mut tree,
            canvas,
            "Increase saturation"
        ));
        let (_, s1, _) = value.get().to_hsv();
        assert!(s1 > s0);
    }

    /// **The same model change the drag makes** — a drag that lands on the
    /// field's own reported position for a colour reaches that colour, and so
    /// does the arrow that walks to it.
    #[test]
    fn the_arrows_reach_a_colour_the_drag_reaches() {
        // Where a press lands is what the drag commits, so the drag's own
        // extreme — the top-right corner, full saturation and full brightness —
        // is the reference.
        let (mut tree, value) = fixture();
        let canvas = field(&tree);
        let b = tree.bounds(canvas);
        drag(
            &mut tree,
            Point::new(b.x + b.width * 0.5, b.y + b.height * 0.5),
            Point::new(b.right() - 1.0, b.y + 1.0),
        );
        let (_, dragged_s, dragged_v) = value.get().to_hsv();
        assert!(
            dragged_s > 0.95 && dragged_v > 0.95,
            "the drag itself did not reach the corner ({dragged_s}, {dragged_v})"
        );

        // Now walk there with coarse arrow steps, which clamp at the same ends.
        let (mut tree, value) = fixture();
        let canvas = field(&tree);
        tree.focus(canvas);
        for _ in 0..12 {
            tree.press_key(Key::ArrowRight, Modifiers::SHIFT);
            tree.press_key(Key::ArrowUp, Modifiers::SHIFT);
        }
        let (_, keyed_s, keyed_v) = value.get().to_hsv();
        assert!(
            (keyed_s - dragged_s).abs() < 0.01 && (keyed_v - dragged_v).abs() < 0.01,
            "the arrows stopped at ({keyed_s}, {keyed_v}), the drag at \
             ({dragged_s}, {dragged_v})"
        );
    }

    /// **Announces once** — both axes, because both are the value.
    #[test]
    fn a_step_announces_the_pair_it_produced() {
        let (mut tree, value) = fixture();
        let canvas = field(&tree);
        tree.focus(canvas);
        let before = {
            let _ = tree.sync_accessibility();
            tree.announcements_since(0).len() as u64
        };
        tree.press_key(Key::ArrowUp, Modifiers::SHIFT);
        let _ = tree.sync_accessibility();
        let said: Vec<String> = tree
            .announcements_since(before)
            .into_iter()
            .map(|a| a.text)
            .collect();
        let (_, s, v) = value.get().to_hsv();
        assert_eq!(
            said,
            vec![format!(
                "Saturation {}%, brightness {}%",
                (s * 100.0).round() as i32,
                (v * 100.0).round() as i32
            )]
        );
    }
}

// ---------------------------------------------------------------------------
// The commands must not cost a node the accessibility it already had
// ---------------------------------------------------------------------------

/// Installing the reorder commands on a node adds accessibility overrides, and
/// several of the nodes they land on already had some.
///
/// A `TreeTableView` row advertises `ScrollIntoView` (the one scroll action all
/// three adapters consume, and a virtualized row's only way back into view) and,
/// on a branch, `Expand` / `Collapse`. Those are builder-level overrides on the
/// same node, so an install that replaced the override block instead of merging
/// into it would take all three away — silently, since nothing else reads them.
#[test]
fn a_reorderable_tree_table_row_keeps_its_scroll_and_expand_actions() {
    use teksilo_core::accesskit::Action;
    use teksilo_data::TreeModel;
    use teksilo_widgets::TreeTableView;
    use teksilo_widgets::table_view::{Column, ColumnWidth};

    let model: TreeModel<&'static str> = TreeModel::new();
    let a = model.insert_root(0, "A");
    model.insert_child(a, 0, "A1");
    model.insert_root(1, "B");
    let mut tree = WidgetTree::new();
    let view = tree.add(
        TreeTableView::new(model)
            .add_column(
                Column::new(
                    "name",
                    teksilo_i18n::lit!("Name"),
                    |_v: &&'static str, _cx| Box::new(FixedLeaf(180.0, 30.0)) as Box<dyn Widget>,
                )
                .width(ColumnWidth::Fixed(180.0)),
            )
            .header_height(0.0)
            .row_height(30.0)
            .reorderable(true),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let rows = nodes_with_role(&tree, view, teksilo_core::accesskit::Role::Row);
    // Row 0 is the branch "A"; row 1 is the leaf "B".
    let branch = tree.accessibility_node(rows[0]);
    assert!(
        branch.actions().contains(&Action::ScrollIntoView),
        "the branch row lost ScrollIntoView: {:?}",
        branch.actions()
    );
    assert!(
        branch.actions().contains(&Action::Expand),
        "the branch row lost Expand: {:?}",
        branch.actions()
    );
    assert!(
        branch.actions().contains(&Action::CustomAction),
        "…and it must still advertise its move commands: {:?}",
        branch.actions()
    );
    let leaf = tree.accessibility_node(rows[1]);
    assert!(
        leaf.actions().contains(&Action::ScrollIntoView),
        "the leaf row lost ScrollIntoView: {:?}",
        leaf.actions()
    );
    assert!(
        !leaf.actions().contains(&Action::Expand),
        "a leaf must not advertise ExpandCollapse: {:?}",
        leaf.actions()
    );
}
