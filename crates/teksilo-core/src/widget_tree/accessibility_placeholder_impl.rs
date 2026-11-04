// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A placeholder that says what the field's name says is left out.
//!
//! A search field named "Filter the outline" whose placeholder reads
//! "Filter the outline…" was read twice by a screen reader: NVDA speaks the
//! name, then the placeholder (UIA `HelpText`, which `accesskit_windows`
//! fills from it). The placeholder is there to show a sighted user what the
//! name tells a listener, so when the two say the same, the name is enough.
//! A placeholder that adds something (an example, a format) stays.
//!
//! Compared on the name the field ends up with: its own label, or the text of
//! the nodes it is `labelled_by`, which is the name every adapter computes
//! when it has no label of its own. Case, surrounding white space and a
//! trailing ellipsis ("…" or "...") do not count as a difference.

use std::collections::HashMap;

use accesskit::{Node, NodeId, Role};

/// Clear the placeholder of every node in `nodes` whose placeholder repeats
/// its name.
pub(super) fn drop_placeholders_repeating_the_name(nodes: &mut [(NodeId, Node)]) {
    let index: HashMap<NodeId, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();
    let mut repeating = Vec::new();
    for (i, (_, node)) in nodes.iter().enumerate() {
        let Some(placeholder) = node.placeholder() else {
            continue;
        };
        let name = match node.label() {
            Some(label) => Some(label.to_owned()),
            None => {
                let parts: Vec<&str> = node
                    .labelled_by()
                    .iter()
                    .filter_map(|id| index.get(id).map(|&j| &nodes[j].1))
                    .filter_map(text_of)
                    .collect();
                (!parts.is_empty()).then(|| parts.join(" "))
            }
        };
        if name.is_some_and(|name| same_words(&name, placeholder)) {
            repeating.push(i);
        }
    }
    for i in repeating {
        nodes[i].1.clear_placeholder();
    }
}

/// The text a naming node shows: the value of a `Role::Label`, the label of
/// anything else, and failing that the other of the two.
fn text_of(node: &Node) -> Option<&str> {
    let text = if node.role() == Role::Label {
        node.value().or_else(|| node.label())
    } else {
        node.label().or_else(|| node.value())
    };
    text.map(str::trim).filter(|text| !text.is_empty())
}

/// Whether `a` and `b` say the same, ignoring case, surrounding white space
/// and a trailing ellipsis.
fn same_words(a: &str, b: &str) -> bool {
    fn bare(text: &str) -> String {
        let text = text.trim();
        let text = text
            .strip_suffix('\u{2026}')
            .or_else(|| text.strip_suffix("..."))
            .unwrap_or(text);
        text.trim_end().to_lowercase()
    }
    let (a, b) = (bare(a), bare(b));
    !a.is_empty() && a == b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(placeholder: &str) -> Node {
        let mut node = Node::new(Role::TextInput);
        node.set_placeholder(placeholder.to_owned());
        node
    }

    #[test]
    fn a_placeholder_repeating_the_label_is_dropped() {
        let mut node = field("Filter the outline\u{2026}");
        node.set_label("Filter the outline".to_owned());
        let mut nodes = vec![(NodeId(1), node)];
        drop_placeholders_repeating_the_name(&mut nodes);
        assert_eq!(nodes[0].1.placeholder(), None);
    }

    #[test]
    fn a_placeholder_repeating_a_labelled_by_name_is_dropped() {
        let mut node = field("search...");
        node.push_labelled_by(NodeId(2));
        let mut label = Node::new(Role::Label);
        label.set_value("Search".to_owned());
        let mut nodes = vec![(NodeId(1), node), (NodeId(2), label)];
        drop_placeholders_repeating_the_name(&mut nodes);
        assert_eq!(nodes[0].1.placeholder(), None);
    }

    #[test]
    fn a_placeholder_that_adds_something_stays() {
        let mut node = field("e.g. 2027-02-03");
        node.set_label("Deadline".to_owned());
        let mut unnamed = field("Search\u{2026}");
        unnamed.set_role(Role::TextInput);
        let mut nodes = vec![(NodeId(1), node), (NodeId(3), unnamed)];
        drop_placeholders_repeating_the_name(&mut nodes);
        assert_eq!(nodes[0].1.placeholder(), Some("e.g. 2027-02-03"));
        assert_eq!(
            nodes[1].1.placeholder(),
            Some("Search\u{2026}"),
            "an unnamed field keeps the only text it has"
        );
    }
}
