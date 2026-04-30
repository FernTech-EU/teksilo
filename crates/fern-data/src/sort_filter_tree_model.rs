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
//! every projection rebuild — `TreeTable` binds to that to know when to
//! rebuild its row tree.
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

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use fern_core::ObserverHandle;
use fern_core::signal::Signal;

use crate::sort_filter_list_model::SortDirection;
use crate::tree_change::NodeId;
use crate::tree_model::TreeModel;
use crate::tree_slice::FlatEntry;

/// Filter strategy used by [`SortFilterTreeModel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeFilterMode {
    /// Hide rows that don't match. Children of hidden parents stay hidden too.
    HideNonMatching,
    /// Keep ancestors of matching descendants visible (file-tree convention).
    KeepAncestors,
    /// Keep matching rows AND their entire subtree.
    KeepDescendants,
}

impl Default for TreeFilterMode {
    fn default() -> Self {
        TreeFilterMode::KeepAncestors
    }
}

type Comparator<T> = Rc<dyn Fn(&T, &T) -> Ordering>;
type PredicateFactory<T> = Rc<dyn Fn(&str) -> Box<dyn Fn(&T) -> bool>>;

struct Inner<T: 'static> {
    tree: TreeModel<T>,
    expanded: HashSet<NodeId>,
    flattened: Vec<FlatEntry>,
    comparators: HashMap<String, Comparator<T>>,
    predicate_factories: HashMap<String, PredicateFactory<T>>,
    sort: Option<(String, SortDirection)>,
    filters: HashMap<String, String>,
    filter_mode: TreeFilterMode,
    version: Signal<u64>,
    version_counter: Cell<u64>,
    sort_signal: Option<Signal<Option<(String, SortDirection)>>>,
    filters_signal: Option<Signal<HashMap<String, String>>>,
    _tree_handle: Option<ObserverHandle>,
    _sort_handle: Option<ObserverHandle>,
    _filters_handle: Option<ObserverHandle>,
}

/// Hierarchical projection over a `TreeModel<T>` driven by sort + filter
/// signals. Exposes a `TreeSlice`-shaped read API consumed by `TreeTable`.
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
            comparators: HashMap::new(),
            predicate_factories: HashMap::new(),
            sort: None,
            filters: HashMap::new(),
            filter_mode: TreeFilterMode::default(),
            version: Signal::new(0),
            version_counter: Cell::new(0),
            sort_signal: None,
            filters_signal: None,
            _tree_handle: None,
            _sort_handle: None,
            _filters_handle: None,
        }));

        let weak = Rc::downgrade(&inner);
        let tree_handle = tree.observe_changes(move |_change| {
            if let Some(strong) = weak.upgrade() {
                rebuild_and_bump(&strong);
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

    /// Bind a sort signal — typically `TreeTable::sort_signal()`.
    pub fn bind_sort_signal(&self, signal: Signal<Option<(String, SortDirection)>>) {
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

    /// Bind a filters signal — typically `TreeTable::filters_signal()`.
    pub fn bind_filters_signal(&self, signal: Signal<HashMap<String, String>>) {
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

    pub fn visible_count(&self) -> usize {
        self.inner.borrow().flattened.len()
    }

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

    pub fn visible_node_id(&self, flat_index: usize) -> Option<NodeId> {
        self.inner
            .borrow()
            .flattened
            .get(flat_index)
            .map(|e| e.node_id)
    }

    pub fn entry_at(&self, flat_index: usize) -> Option<FlatEntry> {
        self.inner.borrow().flattened.get(flat_index).cloned()
    }

    pub fn flat_index_of(&self, node: NodeId) -> Option<usize> {
        self.inner
            .borrow()
            .flattened
            .iter()
            .position(|e| e.node_id == node)
    }

    pub fn is_expanded(&self, node: NodeId) -> bool {
        self.inner.borrow().expanded.contains(&node)
    }

    pub fn expand(&self, node: NodeId) {
        let inserted = self.inner.borrow_mut().expanded.insert(node);
        if inserted {
            rebuild_and_bump(&self.inner);
        }
    }

    pub fn collapse(&self, node: NodeId) {
        let removed = self.inner.borrow_mut().expanded.remove(&node);
        if removed {
            rebuild_and_bump(&self.inner);
        }
    }

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

    pub fn collapse_all(&self) {
        self.inner.borrow_mut().expanded.clear();
        rebuild_and_bump(&self.inner);
    }

    /// Bumps on every projection rebuild — bind in `TreeTable::build`
    /// at `BindingLevel::Rebuild`.
    pub fn version_signal(&self) -> Signal<u64> {
        self.inner.borrow().version.clone()
    }

    /// Underlying tree handle (for direct mutation).
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

// ── Internals ───────────────────────────────────────────────────────────────

fn collect_nodes_with_children<T: 'static>(tree: &TreeModel<T>) -> Vec<NodeId> {
    let mut out = Vec::new();
    let root_count = tree.root_count();
    for i in 0..root_count {
        collect_recurse(tree, tree.root(i), &mut out);
    }
    out
}

fn collect_recurse<T: 'static>(tree: &TreeModel<T>, node: NodeId, out: &mut Vec<NodeId>) {
    if tree.has_children(node) {
        out.push(node);
        for child in tree.children(node) {
            collect_recurse(tree, child, out);
        }
    }
}

fn rebuild_and_bump<T: 'static>(inner_rc: &Rc<RefCell<Inner<T>>>) {
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
        let sort_cmp = g.sort.as_ref().and_then(|(col, dir)| {
            g.comparators.get(col).cloned().map(|c| (c, *dir))
        });
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
        g.flattened = flat;
        let v = g.version_counter.get() + 1;
        g.version_counter.set(v);
        v
    };
    let signal = inner_rc.borrow().version.clone();
    signal.set(next_version);
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
                visit_keep_descendants(tree, tree.root(i), predicates, false, &mut visible);
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

fn visit_hide_non_matching<T: 'static>(
    tree: &TreeModel<T>,
    node: NodeId,
    predicates: &[Box<dyn Fn(&T) -> bool>],
    visible: &mut HashSet<NodeId>,
) {
    if matches_all(tree, node, predicates) {
        visible.insert(node);
    }
    for child in tree.children(node) {
        visit_hide_non_matching(tree, child, predicates, visible);
    }
}

fn visit_keep_ancestors<T: 'static>(
    tree: &TreeModel<T>,
    node: NodeId,
    predicates: &[Box<dyn Fn(&T) -> bool>],
    visible: &mut HashSet<NodeId>,
) -> bool {
    let mut any_descendant_visible = false;
    for child in tree.children(node) {
        if visit_keep_ancestors(tree, child, predicates, visible) {
            any_descendant_visible = true;
        }
    }
    let self_matches = matches_all(tree, node, predicates);
    if self_matches || any_descendant_visible {
        visible.insert(node);
        true
    } else {
        false
    }
}

fn visit_keep_descendants<T: 'static>(
    tree: &TreeModel<T>,
    node: NodeId,
    predicates: &[Box<dyn Fn(&T) -> bool>],
    ancestor_matched: bool,
    visible: &mut HashSet<NodeId>,
) {
    let self_matches = matches_all(tree, node, predicates);
    let here = self_matches || ancestor_matched;
    if here {
        visible.insert(node);
    }
    for child in tree.children(node) {
        visit_keep_descendants(tree, child, predicates, here, visible);
    }
}

fn mark_subtree_visible<T: 'static>(
    tree: &TreeModel<T>,
    node: NodeId,
    visible: &mut HashSet<NodeId>,
) {
    visible.insert(node);
    for child in tree.children(node) {
        mark_subtree_visible(tree, child, visible);
    }
}

fn flatten_visible<T: 'static>(
    tree: &TreeModel<T>,
    node: NodeId,
    depth: usize,
    visible: &HashSet<NodeId>,
    expanded: &HashSet<NodeId>,
    sort: &Option<(Comparator<T>, SortDirection)>,
    out: &mut Vec<FlatEntry>,
) {
    if !visible.contains(&node) {
        return;
    }
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
        for child in children {
            flatten_visible(tree, child, depth + 1, visible, expanded, sort, out);
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
    fn sample()
    -> (TreeModel<&'static str>, NodeId, NodeId, NodeId, NodeId, NodeId)
    {
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
        let proxy = SortFilterTreeModel::new(tree)
            .with_comparator("name", |a: &&str, b: &&str| a.cmp(b));
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
        let proxy = SortFilterTreeModel::new(tree)
            .with_comparator("name", |a: &&str, b: &&str| a.cmp(b));
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
        let proxy = SortFilterTreeModel::new(tree)
            .with_comparator("name", |a: &&str, b: &&str| a.cmp(b));
        let sig: Signal<Option<(String, SortDirection)>> = Signal::new(None);
        proxy.bind_sort_signal(sig.clone());
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
        proxy.bind_filters_signal(sig.clone());

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
}
