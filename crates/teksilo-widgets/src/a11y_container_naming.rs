// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Containers named by the title they already paint.
//!
//! A container that copies its visible title into its own name says the
//! same string twice: once as the container's name, once as the label
//! under it. The `labelled_by` relation says it once — the consumer
//! builds the container's name from its targets' values
//! (`accesskit_consumer::node::write_label`) — and leaves the title a
//! label a reader can still find and review by character.
//!
//! Two things make this fragile, and both are locked here: a node's own
//! label *wins* over the relation in the consumer, so a container that
//! sets both silently announces the copy; and a target absent from the
//! tree panics the relation walk, so the relation must never outlive its
//! label.

#![cfg(test)]

use teksilo_core::accesskit::{NodeId, Role, TreeUpdate};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;

/// The name an adapter would announce for `id`, resolved the way the
/// consumer resolves it — through `labelled_by` when the node has no name
/// of its own.
fn announced_name(update: &TreeUpdate, id: WidgetId) -> Option<String> {
    let target = teksilo_core::accessibility::widget_id_to_node_id(id);
    let consumer = accesskit_consumer::Tree::new(update.clone(), false);
    let state = consumer.state();
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if node.locate().0 == target {
            return node.label();
        }
        for child in node.children() {
            stack.push(child);
        }
    }
    None
}

/// Every `Role::Label` in the tree whose text is exactly `text`.
fn labels_reading(update: &TreeUpdate, text: &str) -> Vec<NodeId> {
    update
        .nodes
        .iter()
        .filter(|(_, node)| {
            node.role() == Role::Label && (node.value() == Some(text) || node.label() == Some(text))
        })
        .map(|(id, _)| *id)
        .collect()
}

fn laid_out(mut tree: WidgetTree) -> (WidgetTree, TreeUpdate) {
    tree.layout(teksilo_canvas::SizeProposal::exact(600.0, 400.0));
    let update = tree.sync_accessibility();
    (tree, update)
}

#[test]
fn a_dialog_is_named_by_the_title_it_paints() {
    use crate::dialog::{DialogContent, ModalContainer};
    use teksilo_i18n::lit;

    let mut tree = WidgetTree::new();
    let dialog = tree.add(ModalContainer::new(
        DialogContent::new().title(lit!("Unsaved changes")),
    ));
    let (_tree, update) = laid_out(tree);

    assert_eq!(
        announced_name(&update, dialog).as_deref(),
        Some("Unsaved changes"),
        "the dialog must announce its visible title"
    );
    assert_eq!(
        labels_reading(&update, "Unsaved changes").len(),
        1,
        "the title is one label, reachable and reviewable, not a second copy"
    );
    let leaks = teksilo_core::accessibility::audit::duplicate_label_leaks(&update);
    assert!(
        leaks.is_empty(),
        "a label repeats an ancestor's name: {leaks:?}"
    );
}

#[test]
fn a_drop_zone_is_named_by_the_prompt_it_paints() {
    use crate::drop_zone::DropZone;
    use teksilo_i18n::lit;

    let mut tree = WidgetTree::new();
    let zone = tree.add(DropZone::new(lit!("Drop files here")));
    let (_tree, update) = laid_out(tree);

    assert_eq!(
        announced_name(&update, zone).as_deref(),
        Some("Drop files here")
    );
    assert_eq!(labels_reading(&update, "Drop files here").len(), 1);
}

#[test]
fn a_title_that_goes_away_does_not_leave_a_dangling_relation() {
    // A relation whose target never reached the tree panics
    // `accesskit_consumer`'s relation iterator before it can build the
    // name. The post-walk strip pass is what keeps that from happening;
    // this proves the container survives it rather than crashing.
    use crate::dialog::{DialogContent, ModalContainer};
    use teksilo_i18n::lit;

    let mut tree = WidgetTree::new();
    let dialog = tree.add(ModalContainer::new(
        DialogContent::new().title(lit!("Gone soon")),
    ));
    tree.layout(teksilo_canvas::SizeProposal::exact(600.0, 400.0));
    let _ = tree.sync_accessibility();

    tree.set_dormant(dialog);
    let update = tree.sync_accessibility();
    for (id, node) in &update.nodes {
        for target in node.labelled_by() {
            assert!(
                update.nodes.iter().any(|(n, _)| n == target),
                "node {id:?} points at {target:?}, which is not in the tree"
            );
        }
    }
}
