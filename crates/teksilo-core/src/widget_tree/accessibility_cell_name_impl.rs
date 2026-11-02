// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A table cell's name, taken from the text it shows.
//!
//! ARIA gives a cell, a grid cell and a row or column header their name from
//! their content, and a browser computes it so: a cell showing "README" is
//! named "README". `accesskit_consumer` computes a name from content only for
//! button-like roles (`labelled_by` falls back to the descendants for a
//! button, a checkbox, a link, a menu item and a radio button, never for a
//! cell, `node.rs:731-753`), and every adapter reads the name it gives. So a
//! cell whose text is in a `Role::Label` child has no name: on Windows the UIA
//! element is a `DataItem` with an empty `Name`, and NVDA reads a table's rows
//! as "data item, 2 of 5" without a word of what they hold.
//!
//! This pass names such a cell after its text, through `labelled_by`, which
//! the consumer turns into the name every adapter exposes, and which keeps
//! the text in one place: the label children stay in the tree, and the name
//! follows them. The targets are the ones the consumer itself takes for a
//! button: each `Role::Label` and `Role::Image` met in reading order, looking
//! through `Role::GenericContainer` layout nodes and nothing else, so a cell
//! holding a control (a checkbox, a text field) is not named after it.
//!
//! A cell that has a name, or a `labelled_by` relation, keeps it. A cell inside
//! a text surface (a rich-text table in an editor or a viewer) is left alone:
//! there the text is read as text, through its runs, and a name would say it
//! twice.
//!
//! The pass runs on every platform but macOS ([`names_cells_from_content`]).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

thread_local! {
    static NAMING_OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Whether the pass names cells: on every platform but macOS.
///
/// `accesskit_macos` (0.27) exposes a cell's name as its `AXTitle`, and
/// VoiceOver reads a cell's title and then its static-text children,
/// dropping the repeat only when a single child says the same as the
/// title. A cell holding two labels ("assets", "24 entries") was read twice
/// ("assets 24 entries, assets 24 entries"). VoiceOver already reads an
/// unnamed cell from its children, once, which is the shape the pass would
/// replace there. UIA and AT-SPI read a cell by its name alone, so on
/// every other platform the name is what gets the text heard.
///
/// Tests override it per thread (`CellNamingOverride`, unit tests only), so
/// both shapes are tested on every host; integration tests get the
/// platform's shape.
pub(super) fn names_cells_from_content() -> bool {
    NAMING_OVERRIDE
        .with(Cell::get)
        .unwrap_or(cfg!(not(target_os = "macos")))
}

/// Forces [`names_cells_from_content`] on this thread until dropped.
#[cfg(test)]
pub(crate) struct CellNamingOverride(Option<bool>);

#[cfg(test)]
impl CellNamingOverride {
    pub(crate) fn new(name: bool) -> Self {
        Self(NAMING_OVERRIDE.with(|o| o.replace(Some(name))))
    }
}

#[cfg(test)]
impl Drop for CellNamingOverride {
    fn drop(&mut self) {
        NAMING_OVERRIDE.with(|o| o.set(self.0));
    }
}

use accesskit::{Node, NodeId, Role};

/// The roles ARIA names from their content that the consumer does not.
fn named_from_content(role: Role) -> bool {
    matches!(
        role,
        Role::Cell | Role::GridCell | Role::RowHeader | Role::ColumnHeader
    )
}

/// The roles whose subtree is a text surface, read as text.
fn text_surface(role: Role) -> bool {
    matches!(
        role,
        Role::Document
            | Role::MultilineTextInput
            | Role::TextInput
            | Role::SearchInput
            | Role::Terminal
            | Role::WebView
    )
}

/// Whether `node` gives a name the consumer would read: a label by its value
/// (the consumer reads a `Role::Label`'s value and never its `label()`, see
/// `Node::label_comes_from_value`), an image by its own name.
fn has_text(node: &Node) -> bool {
    let text = match node.role() {
        Role::Label => node.value(),
        Role::Image => node.label(),
        _ => None,
    };
    text.is_some_and(|t| !t.trim().is_empty())
}

/// Name every unnamed table cell after the labels and images it holds. Runs
/// on the nodes an update carries, after relations to absent nodes are
/// stripped, so every target it adds is present.
///
/// `index` is each node's position in `nodes`, the map the emit code already
/// built for its strips; a node missing from it is not looked into.
pub(super) fn name_cells_from_content(
    nodes: &mut [(NodeId, Node)],
    index: &HashMap<NodeId, usize>,
) {
    // Most updates carry no table: answer without building anything.
    if !names_cells_from_content() || !nodes.iter().any(|(_, n)| named_from_content(n.role())) {
        return;
    }
    let node = |id: NodeId| index.get(&id).map(|&i| &nodes[i].1);

    // The cells inside a text surface, found from the surfaces down rather
    // than from every cell up, so no parent map of the whole tree is built:
    // an update holds few text surfaces, and only their subtrees are walked.
    let mut in_text_surface: HashSet<NodeId> = HashSet::new();
    for (_, surface) in nodes.iter().filter(|(_, n)| text_surface(n.role())) {
        let mut stack: Vec<NodeId> = surface.children().to_vec();
        while let Some(at) = stack.pop() {
            let Some(n) = node(at) else { continue };
            if named_from_content(n.role()) {
                in_text_surface.insert(at);
            }
            stack.extend_from_slice(n.children());
        }
    }

    let mut names: Vec<(usize, Vec<NodeId>)> = Vec::new();
    for (i, (id, cell)) in nodes.iter().enumerate() {
        if !named_from_content(cell.role())
            || cell.label().is_some()
            || !cell.labelled_by().is_empty()
            || in_text_surface.contains(id)
        {
            continue;
        }
        let mut found = Vec::new();
        let mut stack: Vec<NodeId> = cell.children().iter().rev().copied().collect();
        while let Some(at) = stack.pop() {
            let Some(n) = node(at) else { continue };
            if n.is_hidden() {
                continue;
            }
            match n.role() {
                Role::Label | Role::Image => {
                    if has_text(n) {
                        found.push(at);
                    }
                }
                Role::GenericContainer => stack.extend(n.children().iter().rev().copied()),
                _ => {}
            }
        }
        if !found.is_empty() {
            names.push((i, found));
        }
    }
    for (i, targets) in names {
        nodes[i].1.set_labelled_by(targets);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(text: &str) -> Node {
        let mut n = Node::new(Role::Label);
        n.set_value(text);
        n
    }

    fn with_children(role: Role, children: &[u64]) -> Node {
        let mut n = Node::new(role);
        n.set_children(children.iter().map(|&c| NodeId(c)).collect::<Vec<_>>());
        n
    }

    /// Run the pass with the position map the emit code hands it, in the
    /// shape of every platform but macOS, whatever the host.
    fn name(nodes: &mut [(NodeId, Node)]) {
        let _on = CellNamingOverride::new(true);
        let index: HashMap<NodeId, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (*id, i))
            .collect();
        name_cells_from_content(nodes, &index);
    }

    fn named(nodes: &[(NodeId, Node)], id: u64) -> Vec<NodeId> {
        nodes
            .iter()
            .find(|(n, _)| *n == NodeId(id))
            .map(|(_, n)| n.labelled_by().to_vec())
            .unwrap_or_default()
    }

    #[test]
    fn a_cell_is_labelled_by_the_text_it_holds_through_layout_nodes() {
        let mut nodes = vec![
            (NodeId(1), with_children(Role::Table, &[2])),
            (NodeId(2), with_children(Role::Row, &[3, 7])),
            (NodeId(3), with_children(Role::Cell, &[4])),
            (NodeId(4), with_children(Role::GenericContainer, &[5, 6])),
            (NodeId(5), label("README.md")),
            (NodeId(6), label("Project overview.")),
            (NodeId(7), with_children(Role::GridCell, &[8])),
            (NodeId(8), label("markdown")),
        ];
        name(&mut nodes);
        assert_eq!(named(&nodes, 3), vec![NodeId(5), NodeId(6)]);
        assert_eq!(named(&nodes, 7), vec![NodeId(8)]);
        assert!(named(&nodes, 2).is_empty(), "a row is not named here");
    }

    #[test]
    fn a_cell_keeps_its_own_name_and_is_not_named_after_a_control_or_hidden_text() {
        let mut own = with_children(Role::Cell, &[3]);
        own.set_label("Mine");
        let mut hidden = label("secret");
        hidden.set_hidden();
        let mut nodes = vec![
            (NodeId(1), with_children(Role::Row, &[2, 4, 7])),
            (NodeId(2), own),
            (NodeId(3), label("shown")),
            (NodeId(4), with_children(Role::Cell, &[5, 6])),
            (NodeId(5), with_children(Role::CheckBox, &[])),
            (NodeId(6), hidden),
            (NodeId(7), with_children(Role::Cell, &[8])),
            (NodeId(8), with_children(Role::Button, &[9])),
            (NodeId(9), label("inside a control")),
        ];
        name(&mut nodes);
        assert!(named(&nodes, 2).is_empty(), "its own name stands");
        assert!(
            named(&nodes, 4).is_empty(),
            "a control and hidden text name nothing"
        );
        assert!(
            named(&nodes, 7).is_empty(),
            "text inside a control is the control's"
        );
    }

    #[test]
    fn a_cell_in_a_text_surface_is_left_to_its_text() {
        let mut nodes = vec![
            (NodeId(1), with_children(Role::MultilineTextInput, &[2])),
            (NodeId(2), with_children(Role::Table, &[3])),
            (NodeId(3), with_children(Role::Row, &[4])),
            (NodeId(4), with_children(Role::Cell, &[5])),
            (NodeId(5), label("cell text")),
        ];
        name(&mut nodes);
        assert!(named(&nodes, 4).is_empty());
    }

    #[test]
    fn a_label_names_a_cell_by_its_value_only_as_the_consumer_reads_it() {
        // The consumer reads a `Role::Label` target's value and never its
        // `label()`: a label whose text is only in `label()` would name the
        // cell nothing, so it is not taken.
        let mut by_label = Node::new(Role::Label);
        by_label.set_label("not read");
        let mut nodes = vec![
            (NodeId(1), with_children(Role::Row, &[2, 4])),
            (NodeId(2), with_children(Role::Cell, &[3])),
            (NodeId(3), by_label),
            (NodeId(4), with_children(Role::Cell, &[5])),
            (NodeId(5), label("read")),
        ];
        name(&mut nodes);
        assert!(named(&nodes, 2).is_empty(), "a label() alone names nothing");
        assert_eq!(named(&nodes, 4), vec![NodeId(5)]);
    }

    #[test]
    fn a_cell_beside_a_text_surface_is_still_named() {
        // Only the cells under a surface are left to their text.
        let mut nodes = vec![
            (NodeId(1), with_children(Role::GenericContainer, &[2, 6])),
            (NodeId(2), with_children(Role::Document, &[3])),
            (NodeId(3), with_children(Role::Table, &[4])),
            (NodeId(4), with_children(Role::Cell, &[5])),
            (NodeId(5), label("in the document")),
            (NodeId(6), with_children(Role::Grid, &[7])),
            (NodeId(7), with_children(Role::GridCell, &[8])),
            (NodeId(8), label("in the grid")),
        ];
        name(&mut nodes);
        assert!(named(&nodes, 4).is_empty());
        assert_eq!(named(&nodes, 7), vec![NodeId(8)]);
    }

    /// A tree holding one table cell with two labels, through the real walk.
    fn table_tree() -> (crate::widget_tree::WidgetTree, crate::widget_id::WidgetId) {
        use crate::test_widgets::{FillWidget, StackWidget};
        use crate::widget_builder::WidgetBuilder;
        let mut tree = crate::widget_tree::WidgetTree::new();
        let name = tree.add(
            FillWidget::new()
                .access_role(Role::Label)
                .access_value("assets"),
        );
        let count = tree.add(
            FillWidget::new()
                .access_role(Role::Label)
                .access_value("24 entries"),
        );
        let cell = tree.add(
            StackWidget::new()
                .child(name)
                .child(count)
                .access_role(Role::Cell),
        );
        let row = tree.add(StackWidget::new().child(cell).access_role(Role::Row));
        tree.add(StackWidget::new().child(row).access_role(Role::Table));
        tree.layout(teksilo_canvas::SizeProposal::exact(200.0, 100.0));
        (tree, cell)
    }

    fn cell_labelled_by(on: bool) -> Vec<NodeId> {
        let _shape = CellNamingOverride::new(on);
        let (mut tree, cell) = table_tree();
        let id = crate::accessibility::widget_id_to_node_id(cell);
        tree.sync_accessibility()
            .nodes
            .iter()
            .find(|(n, _)| *n == id)
            .map(|(_, n)| n.labelled_by().to_vec())
            .expect("the cell is emitted")
    }

    #[test]
    fn on_windows_and_linux_a_cell_is_named_by_its_labels() {
        assert_eq!(cell_labelled_by(true).len(), 2);
    }

    #[test]
    fn on_macos_a_cell_is_left_to_its_children() {
        // VoiceOver reads the cell's children; a name as well would be heard
        // twice.
        assert!(cell_labelled_by(false).is_empty());
    }

    #[test]
    fn the_platform_shape_is_off_on_macos_only() {
        assert_eq!(names_cells_from_content(), !cfg!(target_os = "macos"));
    }
}
