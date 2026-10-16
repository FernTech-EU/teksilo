// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The accessibility tree of a page an engine renders offscreen, mirrored
//! under the `WebView`'s own node.
//!
//! A native engine view publishes its page's tree to the OS itself. An engine
//! that renders offscreen (Servo) hands it to its embedder instead, as
//! incremental AccessKit updates, and the backend passes them on as
//! [`WebViewEvent::PageAccessibility`](crate::WebViewEvent). The widget keeps
//! the page's nodes ([`PageTree`]) and emits those reachable from the page's
//! root as synthetic children of its node, keyed by the page's own node ids
//! (`SyntheticKind::WebPageNode`), so a node keeps its id across updates and a
//! screen reader can read the page's structure and text.
//!
//! It is a mirror to read, not to act on: Servo takes no AccessKit actions
//! back. Activating the page goes through the frame (Enter, or the frame's own
//! `Click`) and the input the widget forwards.

use std::collections::{HashMap, HashSet};

use teksilo_core::accessibility::{AccessNodeBuilder, SyntheticKind, synthetic_node_id};
use teksilo_core::accesskit::{Node, NodeId};
use teksilo_core::widget_id::WidgetId;

/// An incremental update of a page's accessibility tree, in page node ids.
#[derive(Debug, Clone)]
pub struct PageAccessibilityUpdate {
    /// Nodes added or changed, each whole.
    pub nodes: Vec<(u64, Node)>,
    /// The page's root, when the update names it.
    pub root: Option<u64>,
}

/// Deepest page nesting mirrored: past it, a pathological page's subtree is
/// left out rather than walked.
const MAX_DEPTH: usize = 256;

/// The page's nodes as the last updates left them.
#[derive(Debug, Default)]
pub(crate) struct PageTree {
    nodes: HashMap<u64, Node>,
    root: Option<u64>,
}

impl PageTree {
    /// Fold `update` in. Nodes no longer reachable from the root stay in the
    /// map but are not emitted; a new root drops them.
    pub(crate) fn apply(&mut self, update: PageAccessibilityUpdate) {
        if let Some(root) = update.root
            && self.root != Some(root)
        {
            self.nodes.clear();
            self.root = Some(root);
        }
        self.nodes.extend(update.nodes);
    }

    /// Emit the page under `builder`'s node.
    pub(crate) fn emit(&self, owner: WidgetId, builder: &mut AccessNodeBuilder) {
        let Some(root) = self.root.filter(|r| self.nodes.contains_key(r)) else {
            return;
        };
        let reachable = self.reachable(root);
        let id = |page_id: u64| synthetic_node_id(owner, page_id, SyntheticKind::WebPageNode);
        let mut stack = vec![(None, root)];
        while let Some((parent, page_id)) = stack.pop() {
            let Some(node) = self.nodes.get(&page_id) else {
                continue;
            };
            let mut copy = node.clone();
            remap(&mut copy, &reachable, &id);
            let pushed = builder.push_scene_child_under(
                parent,
                page_id,
                SyntheticKind::WebPageNode,
                move |b| *b.inner_mut() = copy,
            );
            // Children are pushed in reverse so they come off the stack, and
            // into the parent, in document order.
            for child in node.children().iter().rev() {
                if reachable.contains(&child.0) {
                    stack.push((Some(pushed), child.0));
                }
            }
        }
    }

    /// The page ids reachable from `root` through children, at most
    /// [`MAX_DEPTH`] deep, each once.
    fn reachable(&self, root: u64) -> HashSet<u64> {
        let mut seen = HashSet::new();
        let mut stack = vec![(root, 0usize)];
        while let Some((id, depth)) = stack.pop() {
            if depth > MAX_DEPTH || !seen.insert(id) {
                continue;
            }
            if let Some(node) = self.nodes.get(&id) {
                stack.extend(node.children().iter().map(|c| (c.0, depth + 1)));
            }
        }
        seen
    }
}

/// `node`'s references to other page nodes, rewritten to their mirrored ids;
/// a reference to a node not mirrored is dropped rather than left pointing
/// into Teksilo's own tree. Children are rebuilt by the walk, so they are
/// cleared here.
fn remap(node: &mut Node, mirrored: &HashSet<u64>, id: &impl Fn(u64) -> NodeId) {
    let one = |target: Option<NodeId>| target.filter(|t| mirrored.contains(&t.0)).map(|t| id(t.0));
    let many = |targets: &[NodeId]| -> Vec<NodeId> {
        targets
            .iter()
            .filter(|t| mirrored.contains(&t.0))
            .map(|t| id(t.0))
            .collect()
    };
    node.clear_children();
    macro_rules! remap_many {
        ($($get:ident, $set:ident, $clear:ident);* $(;)?) => {$(
            let targets = many(node.$get());
            if targets.is_empty() { node.$clear(); } else { node.$set(targets); }
        )*};
    }
    remap_many!(
        controls, set_controls, clear_controls;
        details, set_details, clear_details;
        described_by, set_described_by, clear_described_by;
        flow_to, set_flow_to, clear_flow_to;
        labelled_by, set_labelled_by, clear_labelled_by;
        owns, set_owns, clear_owns;
        radio_group, set_radio_group, clear_radio_group;
    );
    macro_rules! remap_one {
        ($($get:ident, $set:ident, $clear:ident);* $(;)?) => {$(
            match one(node.$get()) {
                Some(target) => node.$set(target),
                None => node.$clear(),
            }
        )*};
    }
    remap_one!(
        active_descendant, set_active_descendant, clear_active_descendant;
        error_message, set_error_message, clear_error_message;
        in_page_link_target, set_in_page_link_target, clear_in_page_link_target;
        member_of, set_member_of, clear_member_of;
        next_on_line, set_next_on_line, clear_next_on_line;
        previous_on_line, set_previous_on_line, clear_previous_on_line;
        popup_for, set_popup_for, clear_popup_for;
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::accesskit::Role;

    fn node(role: Role, label: &str, children: &[u64]) -> Node {
        let mut n = Node::new(role);
        n.set_label(label);
        n.set_children(children.iter().map(|c| NodeId(*c)).collect::<Vec<_>>());
        n
    }

    #[test]
    fn updates_fold_in_and_a_new_root_starts_over() {
        let mut tree = PageTree::default();
        tree.apply(PageAccessibilityUpdate {
            nodes: vec![
                (1, node(Role::RootWebArea, "", &[2])),
                (2, node(Role::Heading, "Old", &[])),
            ],
            root: Some(1),
        });
        tree.apply(PageAccessibilityUpdate {
            nodes: vec![(2, node(Role::Heading, "New", &[]))],
            root: None,
        });
        assert_eq!(tree.nodes[&2].label(), Some("New"));
        assert_eq!(tree.reachable(1), HashSet::from([1, 2]));

        tree.apply(PageAccessibilityUpdate {
            nodes: vec![(10, node(Role::RootWebArea, "", &[]))],
            root: Some(10),
        });
        assert!(
            !tree.nodes.contains_key(&2),
            "a new document drops the old one"
        );
    }

    #[test]
    fn a_cycle_in_a_page_tree_is_walked_once() {
        let mut tree = PageTree::default();
        tree.apply(PageAccessibilityUpdate {
            nodes: vec![
                (1, node(Role::RootWebArea, "", &[2])),
                (2, node(Role::Group, "", &[1])),
            ],
            root: Some(1),
        });
        assert_eq!(tree.reachable(1).len(), 2);
    }

    #[test]
    fn a_relation_to_a_node_not_mirrored_is_dropped() {
        let mut n = node(Role::TextInput, "", &[]);
        n.set_labelled_by(vec![NodeId(5), NodeId(99)]);
        n.set_active_descendant(NodeId(99));
        let mirrored = HashSet::from([5]);
        remap(&mut n, &mirrored, &|id| NodeId(id + 1000));
        assert_eq!(n.labelled_by(), &[NodeId(1005)]);
        assert_eq!(n.active_descendant(), None);
    }
}
