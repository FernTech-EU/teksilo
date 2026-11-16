// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The walker's language pass: a `content_language` declaration reaches the
//! accessibility tree as AccessKit's `language` property.
//!
//! AccessKit inherits `language` itself: `accesskit_consumer`'s
//! `NodeRef::language()` walks the parents, and every platform adapter reads
//! it through the consumer. So a node is given a language only where the one
//! it should have differs from the one it would inherit **in the tree being
//! sent**, which is not the arena:
//!
//! - A layout container that declares a language is still pruned (the pass
//!   runs after the prune, and a node with a property would not be prunable),
//!   and each child promoted past it carries the language in its place.
//! - Overlay content hangs off the window root in the accessibility tree and
//!   off its anchor in the language chain, so it gets its anchor's language.
//! - A node a widget reparents (a scene item under its logical group) is
//!   compared with its new parent, not its arena one.
//! - Synthetic children (text runs, rich-text cells, scene items) belong to
//!   their owner and inherit its language with no work here.
//! - A node whose name is its content's text (a list or tree row the walker
//!   names from its first label, a table cell named after its labels) is
//!   read in the language of that content, since one node has one language
//!   and its name is what a reader hears. A row of a `ListView` whose
//!   delegate declares the language of its text is read in that language,
//!   although the row's own node belongs to the view.
//!
//! The window root carries the application's locale, set where the root is
//! built.

use std::collections::{HashMap, HashSet};

use accesskit::{Node, NodeId};

use super::WidgetTree;
use crate::language::LanguageTag;
use crate::widget_id::WidgetId;

impl WidgetTree {
    /// Give each emitted node the language of its content where it differs
    /// from the language it inherits from its parent in `nodes`.
    ///
    /// A no-op while no widget declares a language. Runs after the
    /// presentational prune and the dangling-child strip, so it sees the tree
    /// as sent. A node that already carries a language (a widget set one
    /// itself) keeps it, and its descendants inherit it.
    ///
    /// `named_from` maps each row the walker named from its content to the
    /// node the name was taken from. A table cell named after its labels
    /// (through `labelled_by`) is found here from its role.
    pub(super) fn emit_content_languages(
        &self,
        nodes: &mut [(NodeId, Node)],
        named_from: &HashMap<NodeId, NodeId>,
    ) {
        if self.arena.content_language_count == 0 {
            return;
        }
        let root = crate::accessibility::root_node_id();
        let index: HashMap<NodeId, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (*id, i))
            .collect();
        let locale = self.locale_signal.get();
        let mut memo = LanguageMemo::default();
        let mut writes: Vec<(usize, String)> = Vec::new();
        let mut visited: HashSet<NodeId> = HashSet::new();
        // (node, the language it inherits from its parent as sent)
        let mut stack: Vec<(NodeId, Option<String>)> = vec![(root, locale.clone())];
        while let Some((nid, inherited)) = stack.pop() {
            let Some(&i) = index.get(&nid) else { continue };
            if !visited.insert(nid) {
                continue;
            }
            let node = &nodes[i].1;
            let own = if nid == root {
                inherited
            } else if let Some(set) = node.language() {
                Some(set.to_owned())
            } else if let Some(widget) = crate::accessibility::node_id_to_widget_id_maybe(nid)
                .filter(|w| self.arena.get(*w).is_some())
            {
                // A node named from its content speaks that content: its
                // language is the content's, when the content is a widget.
                let speaks = named_from
                    .get(&nid)
                    .copied()
                    .or_else(|| named_after_its_cells_content(node))
                    .and_then(|source| {
                        crate::accessibility::node_id_to_widget_id_maybe(source)
                            .filter(|w| self.arena.get(*w).is_some())
                    })
                    .unwrap_or(widget);
                // Undeclared content is in the application's locale, even
                // under a node in another language (a card reparented into a
                // group of another language).
                let wanted = memo
                    .resolve(self, speaks)
                    .map(|tag| tag.as_str().to_owned())
                    .or_else(|| locale.clone());
                match wanted {
                    Some(wanted) if Some(&wanted) != inherited.as_ref() => {
                        writes.push((i, wanted.clone()));
                        Some(wanted)
                    }
                    // AccessKit has no "back to the default": with no locale
                    // to state, the node inherits.
                    _ => inherited,
                }
            } else {
                inherited
            };
            for child in node.children().iter().rev() {
                stack.push((*child, own.clone()));
            }
        }
        for (i, language) in writes {
            nodes[i].1.set_language(language);
        }
    }
}

/// The first node a table cell or header is named after, when it has no name
/// of its own: the label its name is read from (see
/// `accessibility_cell_name_impl`).
fn named_after_its_cells_content(node: &Node) -> Option<NodeId> {
    use accesskit::Role;
    let cell = matches!(
        node.role(),
        Role::Cell | Role::GridCell | Role::RowHeader | Role::ColumnHeader
    );
    (cell && node.label().is_none())
        .then(|| node.labelled_by().first().copied())
        .flatten()
}

/// Each widget's resolved language, filled as the pass asks, so that a tree
/// with declarations costs one walk up per widget rather than one per widget
/// per ancestor.
#[derive(Default)]
struct LanguageMemo {
    resolved: HashMap<WidgetId, Option<LanguageTag>>,
}

impl LanguageMemo {
    /// What [`WidgetTree::content_language`] answers for `id`.
    fn resolve(&mut self, tree: &WidgetTree, id: WidgetId) -> Option<LanguageTag> {
        // Walk up until an answer is known: a memoised node, a declaration
        // that reads `Some`, or the top of the chain. Then write the answer
        // on every node passed.
        let mut chain: Vec<WidgetId> = Vec::new();
        let mut crossed: Vec<WidgetId> = Vec::new();
        let mut current = Some(id);
        let answer = loop {
            let Some(at) = current else { break None };
            if let Some(known) = self.resolved.get(&at) {
                break known.clone();
            }
            if chain.contains(&at) {
                break None;
            }
            chain.push(at);
            if let Some(tag) = tree.arena.own_content_language(at) {
                break Some(tag);
            }
            current = match tree.arena.parent(at) {
                Some(parent) => Some(parent),
                None if crossed.contains(&at) => None,
                None => {
                    crossed.push(at);
                    tree.overlay_manager
                        .find_by_content(at)
                        .and_then(|overlay| tree.overlay_manager.anchor_for(overlay))
                }
            };
        };
        for at in chain {
            self.resolved.insert(at, answer.clone());
        }
        answer
    }
}
