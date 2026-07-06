// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TreeRowFilter` — sort + tree-aware filter over a [`TreeRow`] stream.
//!
//! The composable sort/filter stage for the [`TreeDataSlice`](crate::TreeDataSlice)
//! pipeline. Where [`SortFilterTreeModel`](crate::SortFilterTreeModel) is a full
//! projection *over an in-memory `TreeModel`* (it owns its own expand state), an
//! external tree already has its expand/flatten projection — the
//! `TreeDataSlice`. Stacking a second projection on top would mean two expand
//! states. So for external trees, sort/filter belongs **below** the slice, as a
//! transform of its raw indent-ordered input:
//!
//! ```text
//! rows::load()  →  TreeRowFilter::apply  →  TreeDataSlice::set_source  →  TreeView
//!               \___ Vec<TreeRow> → Vec<TreeRow> ___/     \___ the one projection ___/
//! ```
//!
//! It uses the same three [`TreeFilterMode`] strategies and sorts siblings per
//! parent, then re-emits a valid indent-ordered stream (surviving nodes' depths
//! are compacted onto their nearest surviving ancestor, which `TreeDataSlice`
//! re-derives into a clean tree):
//!
//! - **`KeepAncestors`** — a node stays if it matches or any descendant matches
//!   (the outline-search behaviour; equivalent to `SortFilterTreeModel`).
//! - **`HideNonMatching`** — a node stays only if it *and every ancestor* match
//!   (children of a hidden parent stay hidden; equivalent to `SortFilterTreeModel`).
//! - **`KeepDescendants`** — a match keeps its whole subtree, surfaced even when
//!   the match's own ancestors don't match (the subtree compacts onto a root).
//!   This deliberately differs from `SortFilterTreeModel`, whose flatten drops a
//!   match unless its full ancestor path is visible — which defeats the mode's
//!   "keep the match and its subtree" intent.
//!
//! ## Example
//!
//! ```
//! use bastyde_data::{TreeRowFilter, TreeRow, TreeFilterMode};
//!
//! let rows = vec![
//!     TreeRow::new(1u64, "Book One", 0),
//!     TreeRow::new(2, "Opening", 1),
//!     TreeRow::new(3, "The Dawn Raid", 1),
//!     TreeRow::new(4, "Notes", 0),
//! ];
//!
//! // Outline search: keep matches and the folders that lead to them.
//! let sieve = TreeRowFilter::new()
//!     .filter_mode(TreeFilterMode::KeepAncestors)
//!     .filter(|title: &&str| title.contains("Dawn"));
//! let out = sieve.apply(rows);
//! // "Book One" (ancestor of the match) + "The Dawn Raid".
//! assert_eq!(out.iter().map(|r| r.item).collect::<Vec<_>>(), vec!["Book One", "The Dawn Raid"]);
//! ```

use std::cmp::Ordering;
use std::marker::PhantomData;

use crate::dnd_types::ItemKey;
use crate::sort_filter_tree_model::TreeFilterMode;
use crate::tree_data_slice::TreeRow;

type Predicate<T> = Box<dyn Fn(&T) -> bool>;
type Comparator<T> = Box<dyn Fn(&T, &T) -> Ordering>;

/// A reusable sort + tree-aware filter over a `Vec<`[`TreeRow`]`<K, T>>`. Build
/// it once, [`apply`](Self::apply) it to each freshly-sourced row stream (e.g.
/// inside a `TreeDataSlice::set_source` closure). See the [module docs](self).
pub struct TreeRowFilter<K: ItemKey, T> {
    predicate: Option<Predicate<T>>,
    mode: TreeFilterMode,
    comparator: Option<Comparator<T>>,
    _k: PhantomData<fn() -> K>,
}

impl<K: ItemKey, T: 'static> Default for TreeRowFilter<K, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: ItemKey, T: 'static> TreeRowFilter<K, T> {
    /// An identity transform (no filter, no sort). Chain [`filter`](Self::filter)
    /// / [`sort`](Self::sort) to configure it.
    pub fn new() -> Self {
        Self {
            predicate: None,
            mode: TreeFilterMode::default(),
            comparator: None,
            _k: PhantomData,
        }
    }

    /// Set the filter strategy (how ancestors/descendants of a match are kept).
    /// Defaults to `TreeFilterMode::default()`.
    pub fn filter_mode(mut self, mode: TreeFilterMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the match predicate over the row item. A row "matches" when `pred`
    /// returns `true`; the [`filter_mode`](Self::filter_mode) decides what else
    /// stays visible. With no predicate every row is kept.
    pub fn filter(mut self, pred: impl Fn(&T) -> bool + 'static) -> Self {
        self.predicate = Some(Box::new(pred));
        self
    }

    /// Sort siblings (ascending) by a comparator on the row item. Parent/child
    /// structure is preserved — only the order within each parent changes.
    pub fn sort(mut self, cmp: impl Fn(&T, &T) -> Ordering + 'static) -> Self {
        self.comparator = Some(Box::new(cmp));
        self
    }

    /// Sort siblings (descending) by a comparator on the row item.
    pub fn sort_desc(mut self, cmp: impl Fn(&T, &T) -> Ordering + 'static) -> Self {
        self.comparator = Some(Box::new(move |a, b| cmp(a, b).reverse()));
        self
    }

    /// Apply the filter + sort to an indent-ordered row stream, returning a new
    /// indent-ordered stream. `O(n log n)` for the sort, `O(n)` otherwise.
    pub fn apply(&self, rows: Vec<TreeRow<K, T>>) -> Vec<TreeRow<K, T>> {
        // Fast path: nothing to do.
        if self.predicate.is_none() && self.comparator.is_none() {
            return rows;
        }
        let n = rows.len();

        // 1. Derive parent/children/roots from the indent depths (the same
        //    nearest-preceding-smaller-depth rule `TreeDataSlice` uses).
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut parent_of: Vec<Option<usize>> = vec![None; n];
        let mut roots: Vec<usize> = Vec::new();
        let mut stack: Vec<(usize, usize)> = Vec::new(); // (depth, index)
        for (i, row) in rows.iter().enumerate() {
            while let Some(&(d, _)) = stack.last() {
                if d >= row.depth {
                    stack.pop();
                } else {
                    break;
                }
            }
            match stack.last() {
                Some(&(_, parent)) => {
                    children[parent].push(i);
                    parent_of[i] = Some(parent);
                }
                None => roots.push(i),
            }
            stack.push((row.depth, i));
        }

        // 2. Visibility per filter mode.
        let visible = self.compute_visible(&rows, &children, &roots, &parent_of);

        // 3. Sort siblings (and roots) by the comparator.
        if let Some(cmp) = &self.comparator {
            roots.sort_by(|&a, &b| cmp(&rows[a].item, &rows[b].item));
            for list in children.iter_mut() {
                list.sort_by(|&a, &b| cmp(&rows[a].item, &rows[b].item));
            }
        }

        // 4. Pre-order DFS: emit visible nodes; a hidden node contributes no
        //    depth, so a visible child of a hidden parent compacts onto the
        //    nearest surviving ancestor.
        let mut emit: Vec<(usize, usize)> = Vec::with_capacity(n);
        for &root in &roots {
            emit_dfs(root, 0, &children, &visible, &mut emit);
        }

        // 5. Move each surviving row out (once) at its compacted depth.
        let mut slots: Vec<Option<TreeRow<K, T>>> = rows.into_iter().map(Some).collect();
        emit.into_iter()
            .map(|(i, depth)| {
                let mut row = slots[i].take().expect("each node emitted at most once");
                row.depth = depth;
                row
            })
            .collect()
    }

    fn compute_visible(
        &self,
        rows: &[TreeRow<K, T>],
        children: &[Vec<usize>],
        roots: &[usize],
        parent_of: &[Option<usize>],
    ) -> Vec<bool> {
        let Some(pred) = &self.predicate else {
            return vec![true; rows.len()];
        };
        let matches: Vec<bool> = rows.iter().map(|r| pred(&r.item)).collect();
        let mut visible = vec![false; rows.len()];
        match self.mode {
            TreeFilterMode::HideNonMatching => {
                // Whole-path match: a node is visible only if it matches AND its
                // parent is visible ("children of hidden parents stay hidden").
                // Rows are pre-order, so a parent's visibility is decided first.
                for i in 0..rows.len() {
                    visible[i] = matches[i] && parent_of[i].is_none_or(|p| visible[p]);
                }
            }
            TreeFilterMode::KeepAncestors => {
                for &r in roots {
                    keep_ancestors(r, children, &matches, &mut visible);
                }
            }
            TreeFilterMode::KeepDescendants => {
                for &r in roots {
                    keep_descendants(r, children, &matches, false, &mut visible);
                }
            }
        }
        visible
    }
}

/// Post-order: a node is visible if it matches or any descendant is visible.
/// Returns whether the subtree rooted here has any visible node.
fn keep_ancestors(
    i: usize,
    children: &[Vec<usize>],
    matches: &[bool],
    visible: &mut [bool],
) -> bool {
    let mut any_descendant = false;
    for &c in &children[i] {
        if keep_ancestors(c, children, matches, visible) {
            any_descendant = true;
        }
    }
    if matches[i] || any_descendant {
        visible[i] = true;
        true
    } else {
        false
    }
}

/// Pre-order: once a node matches, its whole subtree stays visible.
fn keep_descendants(
    i: usize,
    children: &[Vec<usize>],
    matches: &[bool],
    ancestor_matched: bool,
    visible: &mut [bool],
) {
    let here = matches[i] || ancestor_matched;
    if here {
        visible[i] = true;
    }
    for &c in &children[i] {
        keep_descendants(c, children, matches, here, visible);
    }
}

/// Emit visible nodes in pre-order; hidden nodes add no depth (their visible
/// descendants compact onto the nearest surviving ancestor).
fn emit_dfs(
    i: usize,
    out_depth: usize,
    children: &[Vec<usize>],
    visible: &[bool],
    emit: &mut Vec<(usize, usize)>,
) {
    let child_depth = if visible[i] {
        emit.push((i, out_depth));
        out_depth + 1
    } else {
        out_depth
    };
    for &c in &children[i] {
        emit_dfs(c, child_depth, children, visible, emit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Manuscript(0)
    //   Book One(1)
    //     Opening(2)
    //     Dawn(2)
    //   Chapter Two(1)
    //     Fight(2)
    // Notes(0)
    //   Sketch(1)
    fn sample() -> Vec<TreeRow<u64, &'static str>> {
        vec![
            TreeRow::new(1, "Manuscript", 0),
            TreeRow::new(2, "Book One", 1),
            TreeRow::new(3, "Opening", 2),
            TreeRow::new(4, "Dawn", 2),
            TreeRow::new(5, "Chapter Two", 1),
            TreeRow::new(6, "Fight", 2),
            TreeRow::new(7, "Notes", 0),
            TreeRow::new(8, "Sketch", 1),
        ]
    }

    fn titles(rows: &[TreeRow<u64, &'static str>]) -> Vec<&'static str> {
        rows.iter().map(|r| r.item).collect()
    }

    #[test]
    fn identity_passes_through() {
        let out = TreeRowFilter::new().apply(sample());
        assert_eq!(out.len(), 8);
        assert_eq!(titles(&out), titles(&sample()));
    }

    #[test]
    fn keep_ancestors_shows_path_to_match() {
        let out = TreeRowFilter::new()
            .filter_mode(TreeFilterMode::KeepAncestors)
            .filter(|t: &&str| *t == "Dawn")
            .apply(sample());
        // Dawn + its ancestors (Book One, Manuscript). Depths compacted 0,1,2.
        assert_eq!(titles(&out), vec!["Manuscript", "Book One", "Dawn"]);
        assert_eq!(
            out.iter().map(|r| r.depth).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn keep_descendants_surfaces_subtree_even_under_nonmatching_ancestor() {
        let out = TreeRowFilter::new()
            .filter_mode(TreeFilterMode::KeepDescendants)
            .filter(|t: &&str| *t == "Book One")
            .apply(sample());
        // Book One matches; its ancestor "Manuscript" does NOT. KeepDescendants
        // keeps the match AND its subtree, so Book One + children are surfaced
        // and compacted (Book One becomes a root). This deliberately differs from
        // SortFilterTreeModel's flatten, which drops a match whose ancestor path
        // isn't visible.
        assert_eq!(titles(&out), vec!["Book One", "Opening", "Dawn"]);
        assert_eq!(
            out.iter().map(|r| r.depth).collect::<Vec<_>>(),
            vec![0, 1, 1]
        );
    }

    #[test]
    fn hide_non_matching_requires_whole_path() {
        // "Manuscript" and "Book One" form a connected path from a root, so both
        // survive (self + every ancestor matches).
        let out = TreeRowFilter::new()
            .filter_mode(TreeFilterMode::HideNonMatching)
            .filter(|t: &&str| *t == "Manuscript" || *t == "Book One")
            .apply(sample());
        assert_eq!(titles(&out), vec!["Manuscript", "Book One"]);
        assert_eq!(out.iter().map(|r| r.depth).collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn hide_non_matching_hides_match_under_hidden_parent() {
        // "Opening" matches but its parent "Book One" does not → hidden
        // (children of hidden parents stay hidden).
        let out = TreeRowFilter::new()
            .filter_mode(TreeFilterMode::HideNonMatching)
            .filter(|t: &&str| *t == "Opening")
            .apply(sample());
        assert!(out.is_empty());
    }

    #[test]
    fn empty_match_yields_empty() {
        let out = TreeRowFilter::new()
            .filter_mode(TreeFilterMode::KeepAncestors)
            .filter(|_: &&str| false)
            .apply(sample());
        assert!(out.is_empty());
    }

    #[test]
    fn sort_reorders_siblings_per_parent() {
        let out = TreeRowFilter::new()
            .sort(|a: &&str, b: &&str| a.cmp(b))
            .apply(sample());
        // Roots sorted: Manuscript, Notes. Under Manuscript: Book One, Chapter Two
        // (already ordered); under Book One: Dawn, Opening (was Opening, Dawn).
        assert_eq!(
            titles(&out),
            vec![
                "Manuscript",
                "Book One",
                "Dawn",
                "Opening",
                "Chapter Two",
                "Fight",
                "Notes",
                "Sketch"
            ]
        );
    }

    #[test]
    fn sort_desc_reverses() {
        let out = TreeRowFilter::new()
            .sort_desc(|a: &&str, b: &&str| a.cmp(b))
            .apply(sample());
        // Roots descending: Notes, Manuscript.
        assert_eq!(out[0].item, "Notes");
        assert_eq!(out[1].item, "Sketch");
        assert_eq!(out[2].item, "Manuscript");
    }

    #[test]
    fn filter_then_sort_compose() {
        // Keep the path to Dawn + Opening (both under Book One), then sort
        // siblings ascending — Dawn should precede Opening even though the
        // source order is Opening, Dawn.
        let out = TreeRowFilter::new()
            .filter_mode(TreeFilterMode::KeepAncestors)
            .filter(|t: &&str| *t == "Dawn" || *t == "Opening")
            .sort(|a: &&str, b: &&str| a.cmp(b))
            .apply(sample());
        assert_eq!(
            titles(&out),
            vec!["Manuscript", "Book One", "Dawn", "Opening"]
        );
    }

    #[test]
    fn structure_preserved_when_all_match() {
        let out = TreeRowFilter::new()
            .filter_mode(TreeFilterMode::KeepAncestors)
            .filter(|_: &&str| true)
            .apply(sample());
        assert_eq!(titles(&out), titles(&sample()));
        assert_eq!(
            out.iter().map(|r| r.depth).collect::<Vec<_>>(),
            vec![0, 1, 2, 2, 1, 2, 0, 1]
        );
    }
}
