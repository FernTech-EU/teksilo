// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Recursive expand and collapse over a flattened tree.
//!
//! `*` expands the focused row and every descendant — the reading Windows,
//! Qt's `QTreeView::expandRecursively` and GTK3's `GtkTreeView` all share, and
//! the one macOS spells `⌥→` via `expandItem:expandChildren:`. Neither
//! `TreeView` nor `TreeTableView` has a primitive for it: both can only expand
//! one row at a time, and they reach that row through different traits
//! (`TreeSource` vs `RowNavigator`).
//!
//! So the walk lives here, expressed against three closures rather than either
//! trait. It works entirely in *visible* index space, which is the only space
//! both views share: expanding a row inserts its children directly after it,
//! so a depth-first sweep is a forward scan that stops when the depth falls
//! back to the starting row's.
//!
//! Both directions are O(subtree²) in the worst case — collapse rescans to
//! find the next deepest expanded row rather than tracking a stack. That is
//! deliberate: this runs once per keystroke, on a human-triggered action, and
//! a scan that re-reads the live flattening cannot go stale halfway through
//! the way a cached stack of indices would when the rows under it shift.

/// The three things a recursive expand needs from a view, and the only three
/// both tree views can supply.
pub(crate) struct SubtreeOps<'a> {
    /// How many rows the flattening currently holds.
    pub(crate) visible_count: &'a dyn Fn() -> usize,
    /// `(depth, has_children, is_expanded)` for a visible row, or `None` when
    /// the row is out of range or its data has not loaded.
    pub(crate) row: &'a dyn Fn(usize) -> Option<(usize, bool, bool)>,
    /// Expand or collapse one row.
    pub(crate) set_expanded: &'a dyn Fn(usize, bool),
}

impl SubtreeOps<'_> {
    fn depth_of(&self, index: usize) -> Option<usize> {
        (self.row)(index).map(|(d, _, _)| d)
    }
}

/// Expand `row` and every descendant.
///
/// A no-op on a row that has no children, so `*` on a leaf does nothing rather
/// than reporting success.
pub(crate) fn expand_subtree(ops: &SubtreeOps, row: usize) -> bool {
    let Some((base_depth, has_children, _)) = (ops.row)(row) else {
        return false;
    };
    if !has_children {
        return false;
    }
    (ops.set_expanded)(row, true);
    // Walk forward through the rows the expansion just revealed. Each
    // expansion inserts more rows immediately after `i`, so advancing by one
    // descends into them — depth-first, without a stack.
    let mut i = row + 1;
    while i < (ops.visible_count)() {
        let Some((depth, child_has_children, expanded)) = (ops.row)(i) else {
            break;
        };
        if depth <= base_depth {
            break; // left the subtree
        }
        if child_has_children && !expanded {
            (ops.set_expanded)(i, true);
        }
        i += 1;
    }
    true
}

/// Collapse `row` and every descendant, so re-expanding it shows the subtree
/// folded rather than restored to how the user left it.
pub(crate) fn collapse_subtree(ops: &SubtreeOps, row: usize) -> bool {
    let Some((base_depth, has_children, _)) = (ops.row)(row) else {
        return false;
    };
    if !has_children {
        return false;
    }
    // Collapse the deepest expanded descendant first. Collapsing a row only
    // ever removes rows below it, so the *last* expanded row inside the
    // subtree is always safe to fold: nothing before it can be hidden by the
    // operation, and the next scan sees a strictly smaller subtree.
    while let Some(target) = last_expanded_descendant(ops, row, base_depth) {
        (ops.set_expanded)(target, false);
    }
    (ops.set_expanded)(row, false);
    true
}

fn last_expanded_descendant(ops: &SubtreeOps, row: usize, base_depth: usize) -> Option<usize> {
    let mut found = None;
    let mut i = row + 1;
    while i < (ops.visible_count)() {
        let Some((depth, has_children, expanded)) = (ops.row)(i) else {
            break;
        };
        if depth <= base_depth {
            break;
        }
        if has_children && expanded {
            found = Some(i);
        }
        i += 1;
    }
    found
}

/// The first child of `row`, if it is expanded and has one.
///
/// `Right` on an already-open node moves into it — required by the ARIA tree
/// pattern and documented for Windows' tree view ("display the current
/// selection, or select the first subfolder"). The first child is simply the
/// next visible row, since expanding inserts children directly after a parent.
pub(crate) fn first_child(ops: &SubtreeOps, row: usize) -> Option<usize> {
    let (depth, has_children, expanded) = (ops.row)(row)?;
    if !has_children || !expanded {
        return None;
    }
    let child = row + 1;
    (child < (ops.visible_count)() && ops.depth_of(child) == Some(depth + 1)).then_some(child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A flattening built from `(depth, has_children)` pairs plus a set of
    /// expanded node ids, re-derived on every read the way a real source is.
    struct FakeTree {
        /// `(depth, has_children)` for every node in document order.
        nodes: Vec<(usize, bool)>,
        expanded: RefCell<Vec<bool>>,
    }

    impl FakeTree {
        fn new(nodes: Vec<(usize, bool)>) -> Self {
            let n = nodes.len();
            Self {
                nodes,
                expanded: RefCell::new(vec![false; n]),
            }
        }

        /// Node ids currently visible, in order.
        fn visible(&self) -> Vec<usize> {
            let expanded = self.expanded.borrow();
            let mut out = Vec::new();
            let mut hidden_below: Option<usize> = None;
            for (i, &(depth, _)) in self.nodes.iter().enumerate() {
                if let Some(d) = hidden_below {
                    if depth > d {
                        continue;
                    }
                    hidden_below = None;
                }
                out.push(i);
                if !expanded[i] {
                    hidden_below = Some(depth);
                }
            }
            out
        }

        fn ops(
            &self,
        ) -> (
            impl Fn() -> usize,
            impl Fn(usize) -> Option<(usize, bool, bool)>,
            impl Fn(usize, bool),
        ) {
            let count = || self.visible().len();
            let row = move |i: usize| {
                let vis = self.visible();
                vis.get(i).map(|&id| {
                    let (depth, has_children) = self.nodes[id];
                    (depth, has_children, self.expanded.borrow()[id])
                })
            };
            let set = move |i: usize, on: bool| {
                let vis = self.visible();
                if let Some(&id) = vis.get(i) {
                    self.expanded.borrow_mut()[id] = on;
                }
            };
            (count, row, set)
        }
    }

    /// root(0) > a(1) > a1(2), a2(2); b(1) > b1(2); leaf(1)
    fn sample() -> FakeTree {
        FakeTree::new(vec![
            (0, true),  // 0 root
            (1, true),  // 1 a
            (2, false), // 2 a1
            (2, false), // 3 a2
            (1, true),  // 4 b
            (2, false), // 5 b1
            (1, false), // 6 leaf
        ])
    }

    #[test]
    fn expanding_a_subtree_opens_every_descendant_in_one_press() {
        let t = sample();
        let (c, r, s) = t.ops();
        let ops = SubtreeOps {
            visible_count: &c,
            row: &r,
            set_expanded: &s,
        };
        assert!(expand_subtree(&ops, 0));
        assert_eq!(t.visible(), vec![0, 1, 2, 3, 4, 5, 6], "the whole tree");
    }

    #[test]
    fn collapsing_a_subtree_folds_the_descendants_too() {
        let t = sample();
        let (c, r, s) = t.ops();
        let ops = SubtreeOps {
            visible_count: &c,
            row: &r,
            set_expanded: &s,
        };
        expand_subtree(&ops, 0);
        assert!(collapse_subtree(&ops, 0));
        assert_eq!(t.visible(), vec![0]);
        // Re-expanding one level must show a *folded* subtree, not the state
        // the user left behind — that is the difference between a recursive
        // collapse and collapsing the root alone.
        (ops.set_expanded)(0, true);
        assert_eq!(t.visible(), vec![0, 1, 4, 6]);
    }

    #[test]
    fn a_leaf_reports_that_it_did_nothing() {
        let t = sample();
        let (c, r, s) = t.ops();
        let ops = SubtreeOps {
            visible_count: &c,
            row: &r,
            set_expanded: &s,
        };
        expand_subtree(&ops, 0);
        // Visible row 6 is the childless "leaf" sibling.
        assert!(!expand_subtree(&ops, 6));
        assert!(!collapse_subtree(&ops, 6));
    }

    #[test]
    fn expanding_a_subtree_leaves_the_rows_outside_it_alone() {
        let t = sample();
        let (c, r, s) = t.ops();
        let ops = SubtreeOps {
            visible_count: &c,
            row: &r,
            set_expanded: &s,
        };
        (ops.set_expanded)(0, true); // root only: [0, 1, 4, 6]
        // Visible row 1 is node `a`; expanding its subtree must not open `b`.
        assert!(expand_subtree(&ops, 1));
        assert_eq!(t.visible(), vec![0, 1, 2, 3, 4, 6], "b stays folded");
    }

    #[test]
    fn right_on_an_open_node_finds_its_first_child() {
        let t = sample();
        let (c, r, s) = t.ops();
        let ops = SubtreeOps {
            visible_count: &c,
            row: &r,
            set_expanded: &s,
        };
        // Closed: there is nothing to move into yet, which is why the ARIA
        // rule needs two presses to descend.
        assert_eq!(first_child(&ops, 0), None);
        (ops.set_expanded)(0, true);
        assert_eq!(first_child(&ops, 0), Some(1));
        // A childless row never reports one.
        assert_eq!(first_child(&ops, 3), None);
    }
}
