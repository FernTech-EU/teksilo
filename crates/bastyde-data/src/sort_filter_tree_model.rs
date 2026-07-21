// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Composable sort + filter projection over a hierarchical tree.
//!
//! `SortFilterTreeModel<T>` wraps a [`TreeModel<T>`] and exposes a
//! `TreeSlice`-shaped API whose visible nodes are determined by:
//!
//! 1. **Filtering** with one of three [`TreeFilterMode`] strategies:
//!    - `HideNonMatching` — strict per-node match (hides ancestors of
//!      matches if they don't themselves match).
//!    - `KeepAncestors` — file-tree convention: an ancestor stays visible
//!      whenever any descendant matches. **Default.**
//!    - `KeepDescendants` — once a node matches, its entire subtree stays
//!      visible (useful for "show me this branch").
//! 2. **Sorting** applied per-parent: comparators reorder *siblings* but
//!    never cross levels.
//!
//! The proxy owns its own expand/collapse state (independent of any
//! `TreeSlice` over the same `TreeModel`) and bumps `version_signal` on
//! every projection rebuild — `TreeTableView` binds to that to know when to
//! rebuild its row tree.
//!
//! A single-node `TreeChange::NodeUpdated` with **no filter active** skips
//! the full filter/sort/flatten recompute: the node's rank among its
//! siblings is checked against its immediate neighbours (tree sort never
//! crosses levels) and, if stable, only `first_changed_index()` and
//! `version_signal()` advance. Any active filter falls back to the full
//! rebuild (a node's own match verdict can cascade to ancestors and/or
//! descendants depending on `TreeFilterMode`, so cheaply proving no
//! cascade isn't possible without re-deriving visibility). See
//! `try_incremental_node_update` for the full reasoning.
//!
//! ## Selection semantics
//!
//! Selection on a sorted/filtered tree view is tracked by **flat (visible)
//! index**, mirroring `SortFilterListModel`. After a projection rebuild a
//! downstream [`SelectionModel`](crate::SelectionModel) keeps the same
//! numerical indices selected even though they may now point at different
//! nodes. Apps that want identity-based selection should observe
//! `version_signal()` and rewrite the selection from `NodeId`s after each
//! bump.
//!
//! ```rust
//! # use bastyde_data::{TreeModel, SortFilterTreeModel, SortDirection, TreeFilterMode};
//! let tree: TreeModel<&'static str> = TreeModel::new();
//! let src  = tree.insert_root(0, "src");
//! let docs = tree.insert_root(1, "docs");
//! tree.insert_child(src, 0, "main.rs");
//! tree.insert_child(docs, 0, "readme.md");
//!
//! let proxy = SortFilterTreeModel::new(tree)
//!     .filter_mode(TreeFilterMode::KeepAncestors)
//!     .with_comparator("name", |a: &&str, b: &&str| a.cmp(b))
//!     .with_predicate("name", |text| {
//!         let needle = text.to_string();
//!         Box::new(move |row: &&str| row.contains(&needle))
//!     });
//!
//! // Only roots visible initially (collapsed).
//! assert_eq!(proxy.visible_count(), 2);
//!
//! proxy.set_filter("name", ".rs");
//! // KeepAncestors: src (parent of main.rs) stays visible even though it
//! // doesn't match itself.
//! assert!(proxy.visible_count() >= 1);
//! proxy.clear_filters();
//!
//! proxy.expand(src);
//! assert_eq!(proxy.visible_count(), 3); // src + main.rs + docs
//! ```

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use bastyde_core::ObserverHandle;
use bastyde_core::signal::Signal;

use crate::dnd_types::{DragEligibility, DragSource, DropCommit, DropQuery, DropResponse};
use crate::sort_filter_list_model::SortDirection;
use crate::tree_change::{NodeId, TreeChange};
use crate::tree_data_source::{
    FlatEntry, TreeDataSource, tree_apply_reorder, tree_is_desc_or_self,
};
use crate::tree_model::TreeModel;

/// Filter strategy used by [`SortFilterTreeModel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TreeFilterMode {
    /// Hide rows that don't match. Children of hidden parents stay hidden too.
    HideNonMatching,
    /// Keep ancestors of matching descendants visible (file-tree convention).
    #[default]
    KeepAncestors,
    /// Keep matching rows AND their entire subtree.
    KeepDescendants,
}

type Comparator<T> = Rc<dyn Fn(&T, &T) -> Ordering>;
type PredicateFactory<T> = Rc<dyn Fn(&str) -> Box<dyn Fn(&T) -> bool>>;

struct Inner<T: 'static> {
    tree: TreeModel<T>,
    expanded: HashSet<NodeId>,
    flattened: Vec<FlatEntry>,
    /// `NodeId` → flat index, rebuilt alongside `flattened` on every
    /// projection rebuild. Keeps `flat_index_of` O(1) (mirrors
    /// `TreeDataSlice`'s `vis_pos`) instead of a linear scan.
    positions: HashMap<NodeId, usize>,
    comparators: HashMap<String, Comparator<T>>,
    predicate_factories: HashMap<String, PredicateFactory<T>>,
    sort: Option<(String, SortDirection)>,
    filters: HashMap<String, String>,
    filter_mode: TreeFilterMode,
    version: Signal<u64>,
    version_counter: Cell<u64>,
    /// First flat index whose content may differ after the latest rebuild.
    /// See [`SortFilterTreeModel::first_changed_index`].
    last_divergence: Option<usize>,
    sort_signal: Option<Signal<Option<(String, SortDirection)>>>,
    filters_signal: Option<Signal<HashMap<String, String>>>,
    _tree_handle: Option<ObserverHandle>,
    _sort_handle: Option<ObserverHandle>,
    _filters_handle: Option<ObserverHandle>,
}

/// Hierarchical projection over a `TreeModel<T>` driven by sort + filter
/// signals. Exposes a `TreeSlice`-shaped read API consumed by `TreeTableView`.
pub struct SortFilterTreeModel<T: 'static> {
    inner: Rc<RefCell<Inner<T>>>,
}

impl<T: 'static> SortFilterTreeModel<T> {
    /// Wrap a `TreeModel<T>`. The projection starts as the identity
    /// (everything visible, no sort, all roots collapsed).
    pub fn new(tree: TreeModel<T>) -> Self {
        let inner = Rc::new(RefCell::new(Inner {
            tree: tree.clone(),
            expanded: HashSet::new(),
            flattened: Vec::new(),
            positions: HashMap::new(),
            comparators: HashMap::new(),
            predicate_factories: HashMap::new(),
            sort: None,
            filters: HashMap::new(),
            filter_mode: TreeFilterMode::default(),
            version: Signal::new(0),
            version_counter: Cell::new(0),
            last_divergence: None,
            sort_signal: None,
            filters_signal: None,
            _tree_handle: None,
            _sort_handle: None,
            _filters_handle: None,
        }));

        let weak = Rc::downgrade(&inner);
        let tree_handle = tree.observe_changes(move |change| {
            if let Some(strong) = weak.upgrade() {
                rebuild_and_bump_with(&strong, Some(change));
            }
        });
        inner.borrow_mut()._tree_handle = Some(tree_handle);

        rebuild_and_bump(&inner);
        Self { inner }
    }

    /// Register a comparator for a column id. Chainable.
    pub fn with_comparator(
        self,
        col_id: impl Into<String>,
        cmp: impl Fn(&T, &T) -> Ordering + 'static,
    ) -> Self {
        self.inner
            .borrow_mut()
            .comparators
            .insert(col_id.into(), Rc::new(cmp));
        rebuild_and_bump(&self.inner);
        self
    }

    /// Register a predicate factory for a column id. Chainable.
    pub fn with_predicate(
        self,
        col_id: impl Into<String>,
        factory: impl Fn(&str) -> Box<dyn Fn(&T) -> bool> + 'static,
    ) -> Self {
        self.inner
            .borrow_mut()
            .predicate_factories
            .insert(col_id.into(), Rc::new(factory));
        rebuild_and_bump(&self.inner);
        self
    }

    /// Set the filter mode (default `KeepAncestors`). Chainable.
    pub fn filter_mode(self, mode: TreeFilterMode) -> Self {
        self.inner.borrow_mut().filter_mode = mode;
        rebuild_and_bump(&self.inner);
        self
    }

    /// Bind a sort signal — typically `TreeTableView::sort_signal()`.
    pub fn sort_signal(&self, signal: Signal<Option<(String, SortDirection)>>) {
        self.inner.borrow_mut().sort = signal.get();
        rebuild_and_bump(&self.inner);
        let weak = Rc::downgrade(&self.inner);
        let handle = signal.observe(move |new| {
            if let Some(strong) = weak.upgrade() {
                strong.borrow_mut().sort = new.clone();
                rebuild_and_bump(&strong);
            }
        });
        let mut g = self.inner.borrow_mut();
        g.sort_signal = Some(signal);
        g._sort_handle = Some(handle);
    }

    /// Bind a filters signal — typically `TreeTableView::filters_signal()`.
    pub fn filters_signal(&self, signal: Signal<HashMap<String, String>>) {
        self.inner.borrow_mut().filters = signal.get();
        rebuild_and_bump(&self.inner);
        let weak = Rc::downgrade(&self.inner);
        let handle = signal.observe(move |new| {
            if let Some(strong) = weak.upgrade() {
                strong.borrow_mut().filters = new.clone();
                rebuild_and_bump(&strong);
            }
        });
        let mut g = self.inner.borrow_mut();
        g.filters_signal = Some(signal);
        g._filters_handle = Some(handle);
    }

    /// Set the active sort imperatively. Routes through the bound signal
    /// when present.
    pub fn set_sort(&self, col_id: Option<&str>, dir: SortDirection) {
        let new = col_id.map(|c| (c.to_string(), dir));
        let sig = self.inner.borrow().sort_signal.clone();
        if let Some(sig) = sig {
            sig.set(new);
            return;
        }
        self.inner.borrow_mut().sort = new;
        rebuild_and_bump(&self.inner);
    }

    /// Clear the active sort.
    pub fn clear_sort(&self) {
        let sig = self.inner.borrow().sort_signal.clone();
        if let Some(sig) = sig {
            sig.set(None);
            return;
        }
        self.inner.borrow_mut().sort = None;
        rebuild_and_bump(&self.inner);
    }

    /// Set or clear a single column's filter.
    pub fn set_filter(&self, col_id: &str, text: &str) {
        let sig = self.inner.borrow().filters_signal.clone();
        if let Some(sig) = sig {
            let mut m = sig.get();
            if text.is_empty() {
                m.remove(col_id);
            } else {
                m.insert(col_id.to_string(), text.to_string());
            }
            sig.set(m);
            return;
        }
        {
            let mut g = self.inner.borrow_mut();
            if text.is_empty() {
                g.filters.remove(col_id);
            } else {
                g.filters.insert(col_id.to_string(), text.to_string());
            }
        }
        rebuild_and_bump(&self.inner);
    }

    /// Clear every column's filter.
    pub fn clear_filters(&self) {
        let sig = self.inner.borrow().filters_signal.clone();
        if let Some(sig) = sig {
            sig.set(HashMap::new());
            return;
        }
        self.inner.borrow_mut().filters.clear();
        rebuild_and_bump(&self.inner);
    }

    // ── Slice-shaped read API ─────────────────────────────────────────────

    /// Number of currently visible (non-filtered, non-collapsed) nodes in the flat list.
    pub fn visible_count(&self) -> usize {
        self.inner.borrow().flattened.len()
    }

    /// Call `f` with the item and [`FlatEntry`] metadata at `flat_index`, returning `f`'s result.
    pub fn with_entry<R>(
        &self,
        flat_index: usize,
        f: impl FnOnce(&T, &FlatEntry) -> R,
    ) -> Option<R> {
        let (tree, entry) = {
            let g = self.inner.borrow();
            let entry = g.flattened.get(flat_index)?.clone();
            (g.tree.clone(), entry)
        };
        let node_id = entry.node_id;
        tree.with_item(node_id, |item| f(item, &entry))
    }

    /// Return the [`NodeId`] of the node at `flat_index`, or `None` if the index is out of range.
    pub fn visible_node_id(&self, flat_index: usize) -> Option<NodeId> {
        self.inner
            .borrow()
            .flattened
            .get(flat_index)
            .map(|e| e.node_id)
    }

    /// Return a clone of the [`FlatEntry`] at `flat_index`, or `None` if out of range.
    pub fn entry_at(&self, flat_index: usize) -> Option<FlatEntry> {
        self.inner.borrow().flattened.get(flat_index).cloned()
    }

    /// Return the flat index of `node` in the current visible list, or `None`
    /// if it is not visible. O(1) — backed by a position map rebuilt on
    /// every projection rebuild.
    pub fn flat_index_of(&self, node: NodeId) -> Option<usize> {
        self.inner.borrow().positions.get(&node).copied()
    }

    /// Whether `node` is currently expanded in this projection.
    pub fn is_expanded(&self, node: NodeId) -> bool {
        self.inner.borrow().expanded.contains(&node)
    }

    /// Expand `node`, revealing its children in the flat list. Rebuilds and bumps the version signal.
    pub fn expand(&self, node: NodeId) {
        let inserted = self.inner.borrow_mut().expanded.insert(node);
        if inserted {
            rebuild_and_bump(&self.inner);
        }
    }

    /// Collapse `node`, hiding its children. Rebuilds and bumps the version signal.
    pub fn collapse(&self, node: NodeId) {
        let removed = self.inner.borrow_mut().expanded.remove(&node);
        if removed {
            rebuild_and_bump(&self.inner);
        }
    }

    /// Toggle the expanded state of `node`. Always rebuilds and bumps the version signal.
    pub fn toggle(&self, node: NodeId) {
        {
            let mut g = self.inner.borrow_mut();
            if g.expanded.contains(&node) {
                g.expanded.remove(&node);
            } else {
                g.expanded.insert(node);
            }
        }
        rebuild_and_bump(&self.inner);
    }

    /// Expand every node that has children, making the full tree visible.
    pub fn expand_all(&self) {
        let nodes_with_children: Vec<NodeId> = {
            let g = self.inner.borrow();
            collect_nodes_with_children(&g.tree)
        };
        {
            let mut g = self.inner.borrow_mut();
            for n in nodes_with_children {
                g.expanded.insert(n);
            }
        }
        rebuild_and_bump(&self.inner);
    }

    /// Collapse every node, leaving only roots visible.
    pub fn collapse_all(&self) {
        self.inner.borrow_mut().expanded.clear();
        rebuild_and_bump(&self.inner);
    }

    /// Bumps on every projection rebuild — bind in `TreeTableView::build`
    /// at `BindingLevel::Rebuild`.
    pub fn version_signal(&self) -> Signal<u64> {
        self.inner.borrow().version.clone()
    }

    /// First flat index whose content may differ from before the latest
    /// projection rebuild — rows `0..index` are the same nodes, at the
    /// same depths, with the same expand state as before, so per-row
    /// derived state (e.g. a measured row height) remains valid for them.
    /// Equal to `visible_count()` when the visible list is unchanged.
    ///
    /// `None` means unknown (no rebuild observed yet) — treat as a full
    /// change. The value describes the **latest** rebuild only; read it
    /// synchronously from a `version_signal()` observer (observers fire
    /// inline on every bump, so per-change reads cannot miss a value).
    pub fn first_changed_index(&self) -> Option<usize> {
        self.inner.borrow().last_divergence
    }

    /// Return the underlying [`TreeModel`] handle for direct mutation outside the projection.
    pub fn tree(&self) -> TreeModel<T> {
        self.inner.borrow().tree.clone()
    }
}

impl<T: 'static> Clone for SortFilterTreeModel<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: 'static> std::fmt::Debug for SortFilterTreeModel<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let g = self.inner.borrow();
        f.debug_struct("SortFilterTreeModel")
            .field("visible_count", &g.flattened.len())
            .field("expanded_count", &g.expanded.len())
            .field("sort", &g.sort)
            .field("filter_mode", &g.filter_mode)
            .field("filter_count", &g.filters.len())
            .finish()
    }
}

/// `SortFilterTreeModel` is a `TreeDataSource` so it drops directly into a
/// `TreeView` / `TreeTableView`. DnD is left inert (reordering a sort/filter
/// projection is ill-defined); apps that need a reorderable tree feed a
/// `TreeSlice` or a bespoke source.
impl<T: 'static> TreeDataSource for SortFilterTreeModel<T> {
    type Item = T;
    type Key = NodeId;

    fn visible_count(&self) -> usize {
        SortFilterTreeModel::visible_count(self)
    }

    fn with_entry<R>(
        &self,
        flat_index: usize,
        f: impl FnOnce(&Self::Item, &FlatEntry<Self::Key>) -> R,
    ) -> Option<R> {
        SortFilterTreeModel::with_entry(self, flat_index, f)
    }

    fn key_at(&self, flat_index: usize) -> Option<NodeId> {
        self.visible_node_id(flat_index)
    }

    fn flat_index_of(&self, key: &NodeId) -> Option<usize> {
        SortFilterTreeModel::flat_index_of(self, *key)
    }

    fn parent(&self, key: &NodeId) -> Option<NodeId> {
        self.tree().parent(*key)
    }

    fn child_keys(&self, key: &NodeId) -> Vec<NodeId> {
        self.tree().children(*key)
    }

    fn version_signal(&self) -> Signal<u64> {
        SortFilterTreeModel::version_signal(self)
    }

    fn first_changed_index(&self) -> Option<usize> {
        SortFilterTreeModel::first_changed_index(self)
    }

    fn contains_key(&self, key: &NodeId) -> bool {
        // Existence against the backing tree, not the filtered/visible
        // projection, so a node hidden by a collapse or filter keeps its
        // keyed selection (only a genuine delete prunes it).
        self.tree().with_item(*key, |_| ()).is_some()
    }

    fn is_expanded(&self, key: &NodeId) -> bool {
        SortFilterTreeModel::is_expanded(self, *key)
    }

    fn set_expanded(&self, key: &NodeId, expanded: bool) {
        if expanded {
            self.expand(*key);
        } else {
            self.collapse(*key);
        }
    }

    fn drag(&self, _key: &NodeId) -> DragEligibility {
        DragEligibility::CanDrag
    }

    /// Same-view reorder is allowed unless it would create a cycle — a node may
    /// not land on itself or inside its own subtree. Mirrors
    /// [`TreeSlice`](crate::TreeSlice); both project the same `TreeModel`, so
    /// both owe callers the same verdict.
    fn can_accept(&self, query: &DropQuery<'_, NodeId>) -> DropResponse {
        match &query.source {
            DragSource::SameView { key: source } => {
                if *source == query.target
                    || tree_is_desc_or_self(&self.tree(), query.target, *source)
                {
                    DropResponse::Reject
                } else {
                    DropResponse::Accept
                }
            }
            DragSource::Foreign { .. } => DropResponse::Reject,
        }
    }

    fn accept_drop(&self, commit: DropCommit<'_, NodeId>) -> bool {
        match commit.source {
            DragSource::SameView { key: source } => {
                tree_apply_reorder(&self.tree(), source, commit.target, commit.position)
            }
            DragSource::Foreign { .. } => false,
        }
    }

    fn on_drag_out(&self, key: &NodeId) {
        // Source-side completion for a foreign move: drop the node (and its
        // subtree) accepted elsewhere. Re-check existence first — an earlier
        // removal in the same batch could have already freed this node, and
        // `TreeModel::remove` panics on a stale key.
        if self.tree().with_item(*key, |_| ()).is_some() {
            self.tree().remove(*key);
        }
    }
}

// ── Internals ───────────────────────────────────────────────────────────────

/// Explicit-stack pre-order walk — depth-bounded only by tree size, not call
/// stack, so a pathologically deep tree can't overflow it.
fn collect_nodes_with_children<T: 'static>(tree: &TreeModel<T>) -> Vec<NodeId> {
    let mut out = Vec::new();
    let root_count = tree.root_count();
    let mut stack: Vec<NodeId> = (0..root_count).rev().map(|i| tree.root(i)).collect();
    while let Some(node) = stack.pop() {
        if tree.has_children(node) {
            out.push(node);
            for child in tree.children(node).into_iter().rev() {
                stack.push(child);
            }
        }
    }
    out
}

fn rebuild_and_bump<T: 'static>(inner_rc: &Rc<RefCell<Inner<T>>>) {
    rebuild_and_bump_with(inner_rc, None);
}

fn rebuild_and_bump_with<T: 'static>(
    inner_rc: &Rc<RefCell<Inner<T>>>,
    upstream: Option<&TreeChange>,
) {
    // Fast path: a single-node content edit that can't have moved anything
    // (see `try_incremental_node_update`) skips the O(n) filter/sort/
    // flatten recompute entirely.
    if let Some(TreeChange::NodeUpdated { node }) = upstream
        && try_incremental_node_update(inner_rc, *node)
    {
        return;
    }

    // Read inputs out of the borrow before doing the work — observers
    // attached to the version signal may call back into the proxy.
    let (tree, predicates, sort, filter_mode, expanded) = {
        let g = inner_rc.borrow();
        let predicates: Vec<Box<dyn Fn(&T) -> bool>> = g
            .filters
            .iter()
            .filter(|(_, t)| !t.is_empty())
            .filter_map(|(c, t)| g.predicate_factories.get(c).map(|f| f(t)))
            .collect();
        let sort_cmp = g
            .sort
            .as_ref()
            .and_then(|(col, dir)| g.comparators.get(col).cloned().map(|c| (c, *dir)));
        (
            g.tree.clone(),
            predicates,
            sort_cmp,
            g.filter_mode,
            g.expanded.clone(),
        )
    };

    let visible = compute_visibility(&tree, &predicates, filter_mode);

    let mut flat: Vec<FlatEntry> = Vec::new();
    let root_count = tree.root_count();
    let mut roots: Vec<NodeId> = (0..root_count).map(|i| tree.root(i)).collect();
    if let Some((cmp, dir)) = &sort {
        sort_siblings(&tree, &mut roots, cmp, *dir);
    }
    for root in roots {
        flatten_visible(&tree, root, 0, &visible, &expanded, &sort, &mut flat);
    }

    let next_version = {
        let mut g = inner_rc.borrow_mut();
        // First flat index at which the new projection diverges from the
        // old one. `NodeId`s are stable slotmap keys, so equal entries
        // denote the same node at the same depth/expand state. A
        // NodeUpdated leaves the structure identical but changes the
        // node's content — fold its flat position in.
        let mut d = g
            .flattened
            .iter()
            .zip(flat.iter())
            .take_while(|(a, b)| a == b)
            .count();
        if let Some(TreeChange::NodeUpdated { node }) = upstream
            && let Some(p) = flat.iter().position(|e| e.node_id == *node)
        {
            d = d.min(p);
        }
        g.last_divergence = Some(d);
        g.flattened = flat;
        g.positions = g
            .flattened
            .iter()
            .enumerate()
            .map(|(i, e)| (e.node_id, i))
            .collect();
        let v = g.version_counter.get() + 1;
        g.version_counter.set(v);
        v
    };
    let signal = inner_rc.borrow().version.clone();
    signal.set(next_version);
}

/// Fast path for a single-node `TreeChange::NodeUpdated`: skip the O(n)
/// filter/sort/flatten recompute when nothing about visibility or sibling
/// order could have changed. This type has no per-row change event — unlike
/// `SortFilterListModel`'s `DataChange::ItemUpdated`, consumers here only
/// ever see `version_signal()` + the `first_changed_index()` divergence
/// side-channel — so "targeted update" for this type means: skip the
/// recompute, but still correctly advance the divergence floor and bump the
/// version signal (a content change is still observable and must still be
/// signalled). Returns `false` when it can't prove safety; the caller falls
/// back to the existing full rebuild.
///
/// Only attempted when **no filter is active**. With a filter engaged, a
/// node's own match verdict can cascade to ancestors (`KeepAncestors`),
/// descendants (`KeepDescendants` / `HideNonMatching`), or both — and the
/// raw per-node filter-visibility set (independent of expand/collapse
/// state) isn't persisted across rebuilds, so cheaply proving "nothing else
/// moved" isn't possible without re-deriving it, which is exactly what the
/// full rebuild already does safely. With no filter, every node is
/// unconditionally visible, so the only thing a content edit can change is
/// the node's rank among its **siblings** under an active sort (tree sort
/// never crosses levels — see the module docs) — checked against its
/// immediate same-depth flat neighbours, which, with no filter hiding
/// anyone, are exactly its adjacent sorted siblings.
fn try_incremental_node_update<T: 'static>(inner_rc: &Rc<RefCell<Inner<T>>>, node: NodeId) -> bool {
    let mut g = inner_rc.borrow_mut();

    if g.filters.values().any(|t| !t.is_empty()) {
        return false;
    }

    let divergence = match g.positions.get(&node).copied() {
        // Hidden under a collapsed ancestor — nothing visible changed.
        None => g.flattened.len(),
        Some(old_pos) => {
            let depth = g.flattened[old_pos].depth;
            if let Some((col_id, dir)) = g.sort.clone()
                && let Some(cmp) = g.comparators.get(&col_id).cloned()
            {
                let tree = g.tree.clone();
                let descending = dir == SortDirection::Descending;
                let cmp_nodes = |a: NodeId, b: NodeId| -> Ordering {
                    let ord = Cell::new(Ordering::Equal);
                    tree.with_item(a, |va| {
                        tree.with_item(b, |vb| {
                            ord.set(cmp(va, vb));
                        });
                    });
                    if descending {
                        ord.get().reverse()
                    } else {
                        ord.get()
                    }
                };
                // A neighbour that now compares `Equal` matters as much as one
                // we've moved past. The full reprojection sorts siblings with
                // `Vec::sort_by`, which is stable, so a run of equal keys keeps
                // its original *sibling* order; leaving the node where it
                // happens to sit would make this fast path disagree with the
                // rebuild it is meant to be an optimisation of, and the row
                // would jump the next time an unrelated edit forced a full
                // reprojection.
                //
                // Unlike the flat `SortFilterListModel` case — where the tie
                // break is the source index and so is available right here —
                // recovering a node's original sibling index means walking
                // `tree.children(parent)`. Rather than pay that on every
                // update, this bails out on any tie. The cost is that a tree
                // sorted on a low-cardinality key (a status column, say)
                // falls back to a full reprojection more often; if that ever
                // shows up in a profile, comparing sibling indices on the tie
                // path alone would recover the fast path.
                let before = same_depth_neighbor_before(&g.flattened, old_pos, depth);
                if before.is_some_and(|prev| cmp_nodes(prev, node) != Ordering::Less) {
                    return false; // moved before, or now ties with, its predecessor
                }
                let after = same_depth_neighbor_after(&g.flattened, old_pos, depth);
                if after.is_some_and(|next| cmp_nodes(node, next) != Ordering::Less) {
                    return false; // moved past, or now ties with, its successor
                }
            }
            old_pos
        }
    };

    g.last_divergence = Some(divergence);
    let v = g.version_counter.get() + 1;
    g.version_counter.set(v);
    let signal = g.version.clone();
    drop(g);
    signal.set(v);
    true
}

/// Walk backward from `from`, skipping deeper entries (descendants of an
/// earlier sibling), stopping at the first entry at `depth` (the previous
/// sibling) or shallower (no previous sibling — the group was exited).
fn same_depth_neighbor_before(
    flattened: &[FlatEntry],
    from: usize,
    depth: usize,
) -> Option<NodeId> {
    let mut i = from;
    while i > 0 {
        i -= 1;
        let e = &flattened[i];
        if e.depth == depth {
            return Some(e.node_id);
        }
        if e.depth < depth {
            return None;
        }
    }
    None
}

/// Forward counterpart of [`same_depth_neighbor_before`].
fn same_depth_neighbor_after(flattened: &[FlatEntry], from: usize, depth: usize) -> Option<NodeId> {
    let mut i = from + 1;
    while i < flattened.len() {
        let e = &flattened[i];
        if e.depth == depth {
            return Some(e.node_id);
        }
        if e.depth < depth {
            return None;
        }
        i += 1;
    }
    None
}

fn sort_siblings<T: 'static>(
    tree: &TreeModel<T>,
    nodes: &mut [NodeId],
    cmp: &Comparator<T>,
    dir: SortDirection,
) {
    let descending = dir == SortDirection::Descending;
    nodes.sort_by(|&a, &b| {
        let ord = Cell::new(Ordering::Equal);
        tree.with_item(a, |va| {
            tree.with_item(b, |vb| {
                ord.set(cmp(va, vb));
            });
        });
        let mut o = ord.get();
        if descending {
            o = o.reverse();
        }
        o
    });
}

fn compute_visibility<T: 'static>(
    tree: &TreeModel<T>,
    predicates: &[Box<dyn Fn(&T) -> bool>],
    mode: TreeFilterMode,
) -> HashSet<NodeId> {
    if predicates.is_empty() {
        let mut out = HashSet::new();
        for i in 0..tree.root_count() {
            mark_subtree_visible(tree, tree.root(i), &mut out);
        }
        return out;
    }
    let mut visible = HashSet::new();
    match mode {
        TreeFilterMode::HideNonMatching => {
            for i in 0..tree.root_count() {
                visit_hide_non_matching(tree, tree.root(i), predicates, &mut visible);
            }
        }
        TreeFilterMode::KeepAncestors => {
            for i in 0..tree.root_count() {
                visit_keep_ancestors(tree, tree.root(i), predicates, &mut visible);
            }
        }
        TreeFilterMode::KeepDescendants => {
            for i in 0..tree.root_count() {
                visit_keep_descendants(tree, tree.root(i), predicates, &mut visible);
            }
        }
    }
    visible
}

fn matches_all<T: 'static>(
    tree: &TreeModel<T>,
    node: NodeId,
    predicates: &[Box<dyn Fn(&T) -> bool>],
) -> bool {
    let result = Cell::new(true);
    tree.with_item(node, |item| {
        for p in predicates {
            if !p(item) {
                result.set(false);
                return;
            }
        }
    });
    result.get()
}

/// Explicit-stack pre-order walk (equivalent order to the recursive form —
/// membership in `visible` doesn't depend on traversal order).
fn visit_hide_non_matching<T: 'static>(
    tree: &TreeModel<T>,
    root: NodeId,
    predicates: &[Box<dyn Fn(&T) -> bool>],
    visible: &mut HashSet<NodeId>,
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if matches_all(tree, node, predicates) {
            visible.insert(node);
        }
        for child in tree.children(node).into_iter().rev() {
            stack.push(child);
        }
    }
}

/// Bottom-up (descendants before ancestors) aggregation without recursion:
/// collect the subtree in pre-order first, then walk it **reversed** — for
/// any tree, reversed pre-order visits every node only after all of its
/// descendants, so by the time a node is processed, `visible` already holds
/// the final verdict for every child and can be queried directly.
fn visit_keep_ancestors<T: 'static>(
    tree: &TreeModel<T>,
    root: NodeId,
    predicates: &[Box<dyn Fn(&T) -> bool>],
    visible: &mut HashSet<NodeId>,
) {
    let mut pre_order = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        pre_order.push(node);
        for child in tree.children(node).into_iter().rev() {
            stack.push(child);
        }
    }

    for &node in pre_order.iter().rev() {
        let self_matches = matches_all(tree, node, predicates);
        let any_descendant_visible = tree.children(node).iter().any(|c| visible.contains(c));
        if self_matches || any_descendant_visible {
            visible.insert(node);
        }
    }
}

/// Explicit-stack pre-order walk threading `ancestor_matched` down through
/// the stack instead of a recursive call argument.
fn visit_keep_descendants<T: 'static>(
    tree: &TreeModel<T>,
    root: NodeId,
    predicates: &[Box<dyn Fn(&T) -> bool>],
    visible: &mut HashSet<NodeId>,
) {
    let mut stack = vec![(root, false)];
    while let Some((node, ancestor_matched)) = stack.pop() {
        let self_matches = matches_all(tree, node, predicates);
        let here = self_matches || ancestor_matched;
        if here {
            visible.insert(node);
        }
        for child in tree.children(node).into_iter().rev() {
            stack.push((child, here));
        }
    }
}

fn mark_subtree_visible<T: 'static>(
    tree: &TreeModel<T>,
    root: NodeId,
    visible: &mut HashSet<NodeId>,
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        visible.insert(node);
        for child in tree.children(node).into_iter().rev() {
            stack.push(child);
        }
    }
}

/// Explicit-stack pre-order walk, pushing children in reverse so `pop()`
/// yields them in the same (possibly just-sorted) order the recursive form
/// visited them in — output order is bit-for-bit identical.
fn flatten_visible<T: 'static>(
    tree: &TreeModel<T>,
    root: NodeId,
    depth: usize,
    visible: &HashSet<NodeId>,
    expanded: &HashSet<NodeId>,
    sort: &Option<(Comparator<T>, SortDirection)>,
    out: &mut Vec<FlatEntry>,
) {
    if !visible.contains(&root) {
        return;
    }
    let mut stack = vec![(root, depth)];
    while let Some((node, depth)) = stack.pop() {
        let mut children = tree.children(node);
        children.retain(|c| visible.contains(c));
        let has_children = !children.is_empty();
        let is_expanded = expanded.contains(&node);

        out.push(FlatEntry {
            node_id: node,
            depth,
            has_children,
            is_expanded,
        });

        if is_expanded && has_children {
            if let Some((cmp, dir)) = sort {
                sort_siblings(tree, &mut children, cmp, *dir);
            }
            for child in children.into_iter().rev() {
                stack.push((child, depth + 1));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sample tree used across most tests:
    /// ```text
    /// docs
    ///   readme.md
    ///   guide.md
    /// src
    ///   main.rs
    ///   lib.rs
    ///   util
    ///     hash.rs
    /// build.txt
    /// ```
    fn sample() -> (
        TreeModel<&'static str>,
        NodeId,
        NodeId,
        NodeId,
        NodeId,
        NodeId,
    ) {
        let t = TreeModel::new();
        let docs = t.insert_root(0, "docs");
        let readme = t.insert_child(docs, 0, "readme.md");
        t.insert_child(docs, 1, "guide.md");
        let src = t.insert_root(1, "src");
        let main = t.insert_child(src, 0, "main.rs");
        t.insert_child(src, 1, "lib.rs");
        let util = t.insert_child(src, 2, "util");
        t.insert_child(util, 0, "hash.rs");
        t.insert_root(2, "build.txt");
        (t, docs, src, util, readme, main)
    }

    fn collect_visible<T: Clone + 'static>(p: &SortFilterTreeModel<T>) -> Vec<T> {
        (0..p.visible_count())
            .filter_map(|i| p.with_entry(i, |t, _| t.clone()))
            .collect()
    }

    #[test]
    fn initial_state_shows_only_roots() {
        let (tree, _, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree);
        assert_eq!(collect_visible(&proxy), vec!["docs", "src", "build.txt"]);
    }

    #[test]
    fn expand_reveals_children() {
        let (tree, docs, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree);
        proxy.expand(docs);
        assert_eq!(
            collect_visible(&proxy),
            vec!["docs", "readme.md", "guide.md", "src", "build.txt"]
        );
    }

    #[test]
    fn sort_orders_siblings_per_parent() {
        let (tree, _, src, _, _, _) = sample();
        let proxy =
            SortFilterTreeModel::new(tree).with_comparator("name", |a: &&str, b: &&str| a.cmp(b));
        proxy.expand(src);
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        // Roots ascending: build.txt, docs, src.
        // src children ascending: lib.rs, main.rs, util.
        let v = collect_visible(&proxy);
        assert_eq!(
            v,
            vec!["build.txt", "docs", "src", "lib.rs", "main.rs", "util"]
        );
    }

    #[test]
    fn sort_descending() {
        let (tree, _, _, _, _, _) = sample();
        let proxy =
            SortFilterTreeModel::new(tree).with_comparator("name", |a: &&str, b: &&str| a.cmp(b));
        proxy.set_sort(Some("name"), SortDirection::Descending);
        assert_eq!(collect_visible(&proxy), vec!["src", "docs", "build.txt"]);
    }

    #[test]
    fn hide_non_matching_strict() {
        let (tree, _, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree)
            .filter_mode(TreeFilterMode::HideNonMatching)
            .with_predicate("name", |t| {
                let needle = t.to_string();
                Box::new(move |row: &&str| row.contains(&needle))
            });
        proxy.expand_all();
        proxy.set_filter("name", "rs");
        // Only files ending in .rs match. Their parents are NOT visible
        // because they don't match themselves.
        let v = collect_visible(&proxy);
        // src is hidden; main.rs/lib.rs/hash.rs are visible at flat-depth
        // computed against the *current* parent — since src/util are hidden
        // their would-be children are also walked but the children of
        // hidden parents don't get included because flatten_visible
        // short-circuits. So we expect an empty visible set in
        // HideNonMatching mode here.
        assert!(v.is_empty(), "got {:?}", v);
    }

    #[test]
    fn keep_ancestors_shows_path_to_match() {
        let (tree, _, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree)
            .filter_mode(TreeFilterMode::KeepAncestors)
            .with_predicate("name", |t| {
                let needle = t.to_string();
                Box::new(move |row: &&str| row.contains(&needle))
            });
        proxy.expand_all();
        proxy.set_filter("name", "hash");
        // Path docs/readme.md doesn't match → hidden.
        // src/util/hash.rs matches → src + util + hash.rs visible.
        // build.txt doesn't match → hidden.
        assert_eq!(collect_visible(&proxy), vec!["src", "util", "hash.rs"]);
    }

    #[test]
    fn keep_descendants_keeps_subtree_of_matches() {
        let (tree, _, src, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree)
            .filter_mode(TreeFilterMode::KeepDescendants)
            .with_predicate("name", |t| {
                let needle = t.to_string();
                Box::new(move |row: &&str| *row == needle)
            });
        proxy.expand(src);
        proxy.set_filter("name", "src");
        // src matches → its entire subtree is included (children + util,
        // but util's child hash.rs only appears if util is expanded).
        let v = collect_visible(&proxy);
        // src is visible. Its children (main.rs, lib.rs, util) are visible
        // because src is expanded.
        assert!(v.contains(&"src"));
        assert!(v.contains(&"main.rs"));
        assert!(v.contains(&"lib.rs"));
        assert!(v.contains(&"util"));
        assert!(!v.contains(&"docs"));
        assert!(!v.contains(&"build.txt"));
    }

    #[test]
    fn empty_filter_text_clears_predicate() {
        let (tree, _, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree).with_predicate("name", |t| {
            let needle = t.to_string();
            Box::new(move |row: &&str| row.contains(&needle))
        });
        proxy.set_filter("name", "rs");
        assert!(proxy.visible_count() < 3);
        proxy.set_filter("name", "");
        assert_eq!(proxy.visible_count(), 3);
    }

    #[test]
    fn clear_filters_resets_view() {
        let (tree, _, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree).with_predicate("name", |t| {
            let needle = t.to_string();
            Box::new(move |row: &&str| row.contains(&needle))
        });
        proxy.set_filter("name", "rs");
        proxy.clear_filters();
        assert_eq!(proxy.visible_count(), 3);
    }

    #[test]
    fn version_signal_bumps() {
        let (tree, docs, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree.clone());
        let v0 = proxy.version_signal().get();

        proxy.expand(docs);
        let v1 = proxy.version_signal().get();
        assert!(v1 > v0);

        tree.insert_root(0, "x");
        let v2 = proxy.version_signal().get();
        assert!(v2 > v1);
    }

    #[test]
    fn bound_sort_signal_drives_view() {
        let (tree, _, _, _, _, _) = sample();
        let proxy =
            SortFilterTreeModel::new(tree).with_comparator("name", |a: &&str, b: &&str| a.cmp(b));
        let sig: Signal<Option<(String, SortDirection)>> = Signal::new(None);
        proxy.sort_signal(sig.clone());
        sig.set(Some(("name".to_string(), SortDirection::Ascending)));
        assert_eq!(collect_visible(&proxy), vec!["build.txt", "docs", "src"]);
        sig.set(None);
        assert_eq!(collect_visible(&proxy), vec!["docs", "src", "build.txt"]);
    }

    #[test]
    fn bound_filters_signal_drives_view() {
        let (tree, _, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree)
            .filter_mode(TreeFilterMode::HideNonMatching)
            .with_predicate("name", |t| {
                let needle = t.to_string();
                Box::new(move |row: &&str| row.contains(&needle))
            });
        let sig: Signal<HashMap<String, String>> = Signal::new(HashMap::new());
        proxy.filters_signal(sig.clone());

        let mut m = HashMap::new();
        m.insert("name".to_string(), "build".to_string());
        sig.set(m);
        // Only build.txt matches.
        assert_eq!(collect_visible(&proxy), vec!["build.txt"]);
    }

    #[test]
    fn expand_collapse_round_trip() {
        let (tree, docs, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree);
        proxy.expand(docs);
        assert_eq!(proxy.visible_count(), 5);
        proxy.collapse(docs);
        assert_eq!(proxy.visible_count(), 3);
        proxy.toggle(docs);
        assert!(proxy.is_expanded(docs));
    }

    #[test]
    fn expand_all_then_collapse_all() {
        let (tree, _, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree);
        proxy.expand_all();
        // docs, readme.md, guide.md, src, main.rs, lib.rs, util, hash.rs, build.txt
        assert_eq!(proxy.visible_count(), 9);
        proxy.collapse_all();
        assert_eq!(proxy.visible_count(), 3);
    }

    #[test]
    fn flat_index_of() {
        let (tree, docs, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree);
        assert_eq!(proxy.flat_index_of(docs), Some(0));
    }

    #[test]
    fn tree_mutation_triggers_rebuild() {
        let (tree, _, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree.clone());
        let v0 = proxy.version_signal().get();
        tree.insert_root(0, "first");
        let v1 = proxy.version_signal().get();
        assert!(v1 > v0);
        assert_eq!(proxy.visible_count(), 4);
    }

    #[test]
    fn sort_filter_compose() {
        // Filter to KeepAncestors of '.rs', sort siblings ascending.
        let (tree, _, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree)
            .filter_mode(TreeFilterMode::KeepAncestors)
            .with_comparator("name", |a: &&str, b: &&str| a.cmp(b))
            .with_predicate("name", |t| {
                let needle = t.to_string();
                Box::new(move |row: &&str| row.contains(&needle))
            });
        proxy.expand_all();
        proxy.set_filter("name", ".rs");
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        // Visible: src (path to matches), src/util (path to hash.rs), and
        // the .rs files themselves: lib.rs, main.rs, hash.rs.
        // Sorted: src ; lib.rs, main.rs, util ; hash.rs.
        // Ordering at depth-1 inside src: lib.rs, main.rs, util (asc).
        // util/hash.rs at depth 2.
        let v = collect_visible(&proxy);
        assert_eq!(v, vec!["src", "lib.rs", "main.rs", "util", "hash.rs"]);
    }

    #[test]
    fn unregistered_sort_column_keeps_source_order() {
        let (tree, _, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree);
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        assert_eq!(collect_visible(&proxy), vec!["docs", "src", "build.txt"]);
    }

    #[test]
    fn clone_shares_state() {
        let (tree, docs, _, _, _, _) = sample();
        let p1 = SortFilterTreeModel::new(tree);
        let p2 = p1.clone();
        p1.expand(docs);
        assert_eq!(p2.visible_count(), p1.visible_count());
    }

    // ── first_changed_index (divergence) ────────────────────────────────

    #[test]
    fn divergence_on_expand_is_the_toggled_row() {
        let (tree, _, src, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree);

        // Roots: docs (0), src (1), build.txt (2). Expanding src changes
        // src's own entry (is_expanded) and inserts its children — docs
        // (flat 0) is untouched.
        proxy.expand(src);
        assert_eq!(proxy.first_changed_index(), Some(1));

        proxy.collapse(src);
        assert_eq!(proxy.first_changed_index(), Some(1));
    }

    #[test]
    fn divergence_on_append_is_old_len() {
        let (tree, _, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree.clone());

        tree.insert_root(3, "extra"); // old visible: docs, src, build.txt
        assert_eq!(proxy.first_changed_index(), Some(3));
    }

    #[test]
    fn divergence_on_sort_flip_is_first_reordered_row() {
        let (tree, _, _, _, _, _) = sample();
        let proxy =
            SortFilterTreeModel::new(tree).with_comparator("name", |a: &&str, b: &&str| a.cmp(b));

        // [docs, src, build.txt] → ascending [build.txt, docs, src].
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        assert_eq!(proxy.first_changed_index(), Some(0));
    }

    #[test]
    fn divergence_on_node_update_is_its_flat_index() {
        let (tree, _, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree.clone());

        let build_txt = tree.root(2);
        tree.update(build_txt, "build.log");
        assert_eq!(proxy.first_changed_index(), Some(2));
    }

    #[test]
    fn divergence_on_invisible_update_is_visible_count() {
        let (tree, _, _, _, readme, _) = sample();
        let proxy = SortFilterTreeModel::new(tree.clone());

        // readme.md is hidden (docs collapsed) — nothing visible changed.
        tree.update(readme, "readme.txt");
        assert_eq!(proxy.first_changed_index(), Some(proxy.visible_count()));
    }

    // ── flat_index_of position map ──────────────────────────────────────

    /// The position map underlying `flat_index_of` must agree with
    /// iteration order (`visible_node_id`) after every kind of
    /// rebuild-triggering mutation.
    fn assert_positions_match_iteration_order<T: 'static>(proxy: &SortFilterTreeModel<T>) {
        for i in 0..proxy.visible_count() {
            let node = proxy.visible_node_id(i).unwrap();
            assert_eq!(
                proxy.flat_index_of(node),
                Some(i),
                "flat_index_of({node:?}) should be the iteration position {i}"
            );
        }
    }

    #[test]
    fn flat_index_of_matches_iteration_order_across_mutations() {
        let (tree, docs, _, _, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree.clone())
            .filter_mode(TreeFilterMode::KeepAncestors)
            .with_comparator("name", |a: &&str, b: &&str| a.cmp(b))
            .with_predicate("name", |t| {
                let needle = t.to_string();
                Box::new(move |row: &&str| row.contains(&needle))
            });
        assert_positions_match_iteration_order(&proxy);

        proxy.expand_all();
        assert_positions_match_iteration_order(&proxy);

        proxy.set_filter("name", "rs");
        assert_positions_match_iteration_order(&proxy);

        proxy.set_sort(Some("name"), SortDirection::Ascending);
        assert_positions_match_iteration_order(&proxy);

        proxy.collapse(docs);
        assert_positions_match_iteration_order(&proxy);

        proxy.clear_filters();
        assert_positions_match_iteration_order(&proxy);

        tree.insert_root(3, "extra");
        assert_positions_match_iteration_order(&proxy);
    }

    // ── Depth-safe walks (flatten_visible / mark_subtree_visible / visit_*) ─

    /// `expand_all`, `flatten_visible`, and all three `visit_*` filter
    /// strategies are explicit-stack walks; a 50,000-deep single-child
    /// chain must not overflow the call stack in any of them.
    #[test]
    fn deep_chain_filters_each_mode_without_overflow() {
        const DEPTH: usize = 50_000;
        let tree: TreeModel<usize> = TreeModel::new();
        let root = tree.insert_root(0, 0usize);
        let mut leaf = root;
        for i in 1..DEPTH {
            leaf = tree.insert_child(leaf, 0, i);
        }
        let _ = leaf;
        let needle = DEPTH - 1;
        let matches_leaf = move |item: &usize| *item == needle;

        // KeepAncestors: the single match's whole ancestor chain (every
        // node in this linear tree) stays visible — exercises
        // visit_keep_ancestors' bottom-up aggregation at full depth.
        let proxy = SortFilterTreeModel::new(tree.clone())
            .filter_mode(TreeFilterMode::KeepAncestors)
            .with_predicate("v", move |_| Box::new(matches_leaf));
        proxy.expand_all();
        proxy.set_filter("v", "match");
        assert_eq!(proxy.visible_count(), DEPTH);

        // HideNonMatching: only the leaf matches, but the whole-path rule
        // requires every ancestor to match too — nothing survives.
        let proxy = SortFilterTreeModel::new(tree.clone())
            .filter_mode(TreeFilterMode::HideNonMatching)
            .with_predicate("v", move |_| Box::new(matches_leaf));
        proxy.expand_all();
        proxy.set_filter("v", "match");
        assert_eq!(proxy.visible_count(), 0);

        // KeepDescendants: the match's subtree (itself, a leaf) is
        // visible, but flatten_visible starts at the real tree root, which
        // isn't on the visible set — so nothing is emitted. Documented
        // divergence from KeepAncestors (see TreeRowFilter's module docs
        // for the equivalent rule on the TreeDataSlice pipeline).
        let proxy = SortFilterTreeModel::new(tree)
            .filter_mode(TreeFilterMode::KeepDescendants)
            .with_predicate("v", move |_| Box::new(matches_leaf));
        proxy.expand_all();
        proxy.set_filter("v", "match");
        assert_eq!(proxy.visible_count(), 0);
    }

    // ── Incremental NodeUpdated fast path ───────────────────────────────

    #[test]
    fn node_update_fast_path_skips_full_resort_when_order_stable() {
        let (tree, _, _, util, _, _) = sample();
        let calls = Rc::new(Cell::new(0usize));
        let c = calls.clone();
        let proxy = SortFilterTreeModel::new(tree.clone()).with_comparator(
            "name",
            move |a: &&str, b: &&str| {
                c.set(c.get() + 1);
                a.cmp(b)
            },
        );
        proxy.expand_all();
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        // src children ascending: lib.rs, main.rs, util (last).
        let v0 = proxy.version_signal().get();
        calls.set(0); // isolate calls made by the update below

        // "utility" still sorts after "main.rs" and has no next sibling —
        // rank among siblings is unchanged.
        tree.update(util, "utility");

        assert_eq!(
            calls.get(),
            1,
            "only the one stable-neighbour comparison should run, not a full resort"
        );
        assert_eq!(proxy.first_changed_index(), Some(7)); // util's flat index
        assert!(
            proxy.version_signal().get() > v0,
            "content changed — must still bump"
        );
        assert_eq!(
            collect_visible(&proxy),
            vec![
                "build.txt",
                "docs",
                "guide.md",
                "readme.md",
                "src",
                "lib.rs",
                "main.rs",
                "utility",
                "hash.rs"
            ]
        );
    }

    #[test]
    fn node_update_that_reorders_falls_back_to_full_rebuild_with_correct_order() {
        let (tree, _, _, util, _, _) = sample();
        let proxy = SortFilterTreeModel::new(tree.clone())
            .with_comparator("name", |a: &&str, b: &&str| a.cmp(b));
        proxy.expand_all();
        proxy.set_sort(Some("name"), SortDirection::Ascending);
        // src children ascending: lib.rs, main.rs, util.

        // Renaming to "abc" sorts before every other src child.
        tree.update(util, "abc");

        assert_eq!(
            collect_visible(&proxy),
            vec![
                "build.txt",
                "docs",
                "guide.md",
                "readme.md",
                "src",
                "abc",
                "hash.rs",
                "lib.rs",
                "main.rs"
            ]
        );
    }

    #[test]
    fn node_update_under_active_filter_still_produces_correct_visibility() {
        let (tree, _, _, _, _, main) = sample();
        let proxy = SortFilterTreeModel::new(tree.clone())
            .filter_mode(TreeFilterMode::KeepAncestors)
            .with_predicate("name", |t| {
                let needle = t.to_string();
                Box::new(move |row: &&str| row.contains(&needle))
            });
        proxy.expand_all();
        proxy.set_filter("name", "main");
        assert_eq!(collect_visible(&proxy), vec!["src", "main.rs"]);

        // Filtered in → out: rename away from the match.
        tree.update(main, "entry.rs");
        assert_eq!(collect_visible(&proxy), Vec::<&str>::new());

        // Filtered out → in: rename back to match again.
        tree.update(main, "main.rs");
        assert_eq!(collect_visible(&proxy), vec!["src", "main.rs"]);
    }
}
