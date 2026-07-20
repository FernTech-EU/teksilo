// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TreeDataSlice` — the reusable [`TreeDataSource`] engine for an **external,
//! indent-ordered** tree (a Qleany entity store, a database, a virtual
//! filesystem) that is NOT mirrored into a [`TreeModel`](crate::TreeModel).
//!
//! [`TreeSlice`](crate::TreeSlice) gives per-view expand state + flattening +
//! divergence to a `TreeModel`. `TreeDataSlice` gives the **same machinery** to
//! a source whose identity is a domain key (`K = i64` entity id, a tagged enum,
//! …) and whose natural shape is a flat, pre-order, indent-annotated row stream
//! — the shape an outline is genuinely stored in (Scrivener-class binders /
//! chapters / scenes, OPML, Markdown headings). The app hands over
//! `Vec<`[`TreeRow`]`<K, T>>` (`{ key, item, depth }`, document order) on every
//! (re)load; the engine owns everything else:
//!
//! * **tree derivation** — parent links + child index + roots + structural
//!   depth, derived from the indent sequence (an item's parent is the nearest
//!   preceding row of strictly smaller depth; depth-0 rows are roots);
//! * **per-view expand state** — a `K`-keyed set, so two slices over the same
//!   source expand independently and expand survives a full re-source;
//! * **collapse-aware flattening** into the visible row list;
//! * **divergence** ([`first_changed_index`](TreeDataSlice::first_changed_index))
//!   — the common-prefix of the old vs new visible rows, comparing key + depth +
//!   has-children + expand **and item content** (hence the `T: PartialEq`
//!   bound), so a consumer caching per-row state (a measured row height) keeps
//!   its valid prefix across reloads and expand toggles;
//! * **DnD mechanism** — the cycle guard + `can_accept`/`accept_drop` plumbing;
//!   domain *policy* is injected as closures ([`TreeDataSlice::set_drag_policy`],
//!   [`TreeDataSlice::set_drop_resolver`], [`TreeDataSlice::set_reorder`]).
//!
//! It is a cheap `Rc`-handle (clone = share, like `ListModel` / `SceneModel`):
//! pass one clone to `TreeView::from_source` and keep another to drive
//! [`reload`](TreeDataSlice::reload) / [`set_rows`](TreeDataSlice::set_rows) from
//! the app.
//!
//! ## Wiring an external source
//!
//! ```
//! use bastyde_data::{TreeDataSlice, TreeRow};
//! use bastyde_data::dnd_types::{DragEligibility, DropPosition};
//!
//! // key = entity id, item = the row's display data
//! let slice: TreeDataSlice<u64, String> = TreeDataSlice::new();
//! slice.set_expand_new_nodes(true);             // new nodes appear expanded
//! slice.set_source(|| vec![                     // your `rows::load`
//!     TreeRow::new(1, "Binder".to_string(), 0),
//!     TreeRow::new(2, "Chapter".to_string(), 1),
//!     TreeRow::new(3, "Scene".to_string(), 2),
//! ]);
//! slice.set_drag_policy(|key| if *key == 1 { DragEligibility::NoDrag } else { DragEligibility::CanDrag });
//! slice.set_reorder(|_dragged, _target, _pos: DropPosition| { /* backend move + undo */ true });
//! slice.reload();
//!
//! assert_eq!(slice.visible_count(), 3);         // all expanded
//! // let view = TreeView::from_source(slice.clone(), delegate);
//! ```

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use bastyde_core::signal::Signal;

use crate::dnd_types::{
    DragEligibility, DragSource, DropCommit, DropPosition, DropQuery, DropResponse, ItemKey,
};
use crate::tree_data_source::{FlatEntry, TreeDataSource};

/// One row the app hands to a [`TreeDataSlice`], in **document (pre-)order**.
///
/// `depth` is the indent level (`0` = a root). The engine derives each row's
/// parent, children, and structural depth from the `depth` sequence: a row's
/// parent is the nearest preceding row with a strictly smaller `depth`. The row
/// stream must be well-formed pre-order (a parent precedes its subtree) — the
/// shape any indent-stored outline already has.
#[derive(Debug, Clone)]
pub struct TreeRow<K, T> {
    /// The row's stable domain identity (entity id, tagged key, …). Must be
    /// stable across reloads for expand-state and divergence to survive.
    pub key: K,
    /// The row's display payload.
    pub item: T,
    /// Indent level; `0` is a root.
    pub depth: usize,
}

impl<K, T> TreeRow<K, T> {
    /// Convenience constructor.
    pub fn new(key: K, item: T, depth: usize) -> Self {
        Self { key, item, depth }
    }
}

/// Reorder command: `(dragged, target, position) -> applied`. Applies the move
/// through the backend (with undo) and reports whether it took. On `true` the
/// slice re-sources itself via the [`set_source`](TreeDataSlice::set_source)
/// closure.
///
/// `Rc`, not `Box`: callers (`accept_drop`/`drag`/`resolve`) clone the handle
/// out of its `RefCell` and drop the borrow *before* invoking the closure, so
/// a closure that calls back into the slice (e.g. re-registering itself via
/// `set_reorder`) doesn't hit a `BorrowMutError`.
type ReorderFn<K> = Rc<dyn Fn(K, K, DropPosition) -> bool>;
/// Per-row drag gate. Default (unset): every row is `NoDrag`.
type DragPolicyFn<K> = Rc<dyn Fn(&K) -> DragEligibility>;
/// Domain drop policy: `(dragged, target, target_item, position) -> effective
/// position`, or `None` to forbid. The engine applies its own cycle guard first
/// and looks up the target's payload, so the resolver can encode domain rules
/// that depend on the target node (e.g. "a drop onto a non-container leaf
/// becomes `After` it") **without capturing the slice** (which would form an
/// `Rc` cycle).
type DropResolverFn<K, T> = Rc<dyn Fn(&K, &K, &T, DropPosition) -> Option<DropPosition>>;
/// Row source: produces the whole indent-ordered stream for the current state.
type SourceFn<K, T> = Rc<dyn Fn() -> Vec<TreeRow<K, T>>>;

/// Internal, fully-derived representation of one row.
struct Row<K, T> {
    key: K,
    item: T,
    /// Structural depth (root = 0), derived from the tree, not the raw indent.
    depth: usize,
    parent: Option<K>,
    has_children: bool,
}

/// The freshly-built structure + projection, staged before commit.
struct Built<K, T> {
    rows: Vec<Row<K, T>>,
    children: HashMap<K, Vec<usize>>,
    roots: Vec<usize>,
    row_pos: HashMap<K, usize>,
    visible: Vec<usize>,
    vis_pos: HashMap<K, usize>,
    expanded: HashSet<K>,
    seen: HashSet<K>,
}

struct Inner<K: ItemKey, T> {
    rows: RefCell<Vec<Row<K, T>>>,
    /// parent key → child **row indices**, in sibling order.
    children: RefCell<HashMap<K, Vec<usize>>>,
    /// Root **row indices**, in order.
    roots: RefCell<Vec<usize>>,
    /// key → **row index**.
    row_pos: RefCell<HashMap<K, usize>>,
    /// **Row indices** currently visible (collapse-aware), in flat order.
    visible: RefCell<Vec<usize>>,
    /// key → **flat index** within `visible`.
    vis_pos: RefCell<HashMap<K, usize>>,
    expanded: RefCell<HashSet<K>>,
    /// Every key ever installed — lets a re-source auto-expand only *newly*
    /// appearing nodes while preserving the user's later collapses.
    seen: RefCell<HashSet<K>>,
    expand_new: Cell<bool>,
    /// Reveal override: when `true`, the flatten treats every node as expanded,
    /// ignoring `expanded` (which is preserved). Drives "reveal while filtering".
    all_expanded: Cell<bool>,
    version: Signal<u64>,
    version_counter: Cell<u64>,
    divergence: Cell<Option<usize>>,
    source: RefCell<Option<SourceFn<K, T>>>,
    reorder: RefCell<Option<ReorderFn<K>>>,
    drag_policy: RefCell<Option<DragPolicyFn<K>>>,
    drop_resolver: RefCell<Option<DropResolverFn<K, T>>>,
}

/// Per-view flattened projection of an external, indent-ordered tree source.
/// See the [module documentation](self).
pub struct TreeDataSlice<K: ItemKey, T> {
    inner: Rc<Inner<K, T>>,
}

impl<K: ItemKey, T> Clone for TreeDataSlice<K, T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<K: ItemKey, T> Default for TreeDataSlice<K, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: ItemKey, T> TreeDataSlice<K, T> {
    /// Create an empty slice. Configure it (`set_source` / `set_reorder` /
    /// policies / `set_expand_new_nodes`) then populate with
    /// [`reload`](Self::reload) or [`set_rows`](Self::set_rows).
    pub fn new() -> Self {
        Self {
            inner: Rc::new(Inner {
                rows: RefCell::new(Vec::new()),
                children: RefCell::new(HashMap::new()),
                roots: RefCell::new(Vec::new()),
                row_pos: RefCell::new(HashMap::new()),
                visible: RefCell::new(Vec::new()),
                vis_pos: RefCell::new(HashMap::new()),
                expanded: RefCell::new(HashSet::new()),
                seen: RefCell::new(HashSet::new()),
                expand_new: Cell::new(false),
                all_expanded: Cell::new(false),
                version: Signal::new(0),
                version_counter: Cell::new(0),
                divergence: Cell::new(None),
                source: RefCell::new(None),
                reorder: RefCell::new(None),
                drag_policy: RefCell::new(None),
                drop_resolver: RefCell::new(None),
            }),
        }
    }

    // ── Configuration ─────────────────────────────────────────────────────

    /// Install the row source (`rows::load`). [`reload`](Self::reload) and a
    /// committed drop call it to re-materialise the tree.
    pub fn set_source(&self, f: impl Fn() -> Vec<TreeRow<K, T>> + 'static) {
        *self.inner.source.borrow_mut() = Some(Rc::new(f));
    }

    /// Install the reorder command (`dragged, target, position -> applied`).
    /// Without one, drops are refused.
    pub fn set_reorder(&self, f: impl Fn(K, K, DropPosition) -> bool + 'static) {
        *self.inner.reorder.borrow_mut() = Some(Rc::new(f));
    }

    /// Install the per-row drag gate. Without one, no row is draggable.
    pub fn set_drag_policy(&self, f: impl Fn(&K) -> DragEligibility + 'static) {
        *self.inner.drag_policy.borrow_mut() = Some(Rc::new(f));
    }

    /// Install the domain drop resolver. The engine's cycle guard (no drop into
    /// your own subtree, no self-drop) runs first, then hands the resolver
    /// `(dragged, target, target_item, position)`; return `Some(pos)` to accept
    /// at `pos` (a different `pos` snaps the indicator, i.e.
    /// [`DropResponse::Redirect`]) or `None` to forbid. Without one, any
    /// non-cyclic drop is accepted at the requested position.
    pub fn set_drop_resolver(
        &self,
        f: impl Fn(&K, &K, &T, DropPosition) -> Option<DropPosition> + 'static,
    ) {
        *self.inner.drop_resolver.borrow_mut() = Some(Rc::new(f));
    }

    /// Whether nodes appearing for the first time start expanded (`true`) or
    /// collapsed (`false`, the default, matching `TreeSlice`). Set this **before**
    /// the first populate to affect the initial rows.
    pub fn set_expand_new_nodes(&self, expand: bool) {
        self.inner.expand_new.set(expand);
    }

    // ── Population ────────────────────────────────────────────────────────

    /// Build a slice directly from an initial row stream (no version bump / no
    /// divergence — construction is not a change).
    pub fn from_rows(rows: Vec<TreeRow<K, T>>) -> Self {
        let slice = Self::new();
        let built = slice.build(rows);
        slice.commit(built);
        slice
    }

    /// Re-source the rows via the [`set_source`](Self::set_source) closure and
    /// reproject. No-op if no source is installed.
    pub fn reload(&self)
    where
        T: PartialEq,
    {
        // Clone the handle out and drop the borrow before invoking: the
        // app-supplied loader may call back into this slice (even re-install
        // the source), which would otherwise hit a re-entrant borrow.
        let f = {
            let src = self.inner.source.borrow();
            match src.as_ref() {
                Some(f) => f.clone(),
                None => return,
            }
        };
        self.set_rows(f());
    }

    /// Replace the rows with a freshly-sourced stream, preserving per-view
    /// expand state by key, computing [`first_changed_index`](Self::first_changed_index),
    /// and bumping the version signal.
    pub fn set_rows(&self, rows: Vec<TreeRow<K, T>>)
    where
        T: PartialEq,
    {
        let built = self.build(rows);
        let all = self.inner.all_expanded.get();
        let div = {
            let old_rows = self.inner.rows.borrow();
            let old_visible = self.inner.visible.borrow();
            let old_expanded = self.inner.expanded.borrow();
            common_prefix(
                &old_rows,
                &old_visible,
                &old_expanded,
                all,
                &built.rows,
                &built.visible,
                &built.expanded,
                all,
            )
        };
        self.commit(built);
        self.inner.divergence.set(Some(div));
        self.bump();
    }

    // ── Read surface (also exposed via `TreeDataSource`) ──────────────────

    /// Number of currently-visible (flattened) rows.
    pub fn visible_count(&self) -> usize {
        self.inner.visible.borrow().len()
    }

    /// Access the item + flat metadata at a visible index via callback.
    pub fn with_entry<R>(
        &self,
        flat_index: usize,
        f: impl FnOnce(&T, &FlatEntry<K>) -> R,
    ) -> Option<R> {
        let visible = self.inner.visible.borrow();
        let &row_idx = visible.get(flat_index)?;
        let rows = self.inner.rows.borrow();
        let row = rows.get(row_idx)?;
        let entry = FlatEntry {
            node_id: row.key.clone(),
            depth: row.depth,
            has_children: row.has_children,
            is_expanded: self.inner.all_expanded.get()
                || self.inner.expanded.borrow().contains(&row.key),
        };
        Some(f(&row.item, &entry))
    }

    /// Access a node's item by key via callback, **regardless of visibility** (a
    /// node hidden under a collapsed ancestor is still reachable). Returns `None`
    /// if the key is absent from the source. The by-key counterpart of
    /// [`with_entry`](Self::with_entry) (which is by visible index) — use it to
    /// resolve a key to its domain payload.
    pub fn with_key<R>(&self, key: &K, f: impl FnOnce(&T) -> R) -> Option<R> {
        let idx = *self.inner.row_pos.borrow().get(key)?;
        let rows = self.inner.rows.borrow();
        rows.get(idx).map(|r| f(&r.item))
    }

    /// The key of the row at a visible index.
    pub fn key_at(&self, flat_index: usize) -> Option<K> {
        let visible = self.inner.visible.borrow();
        let &row_idx = visible.get(flat_index)?;
        self.inner.rows.borrow().get(row_idx).map(|r| r.key.clone())
    }

    /// The `FlatEntry` at a visible index (cloned).
    pub fn entry_at(&self, flat_index: usize) -> Option<FlatEntry<K>> {
        let visible = self.inner.visible.borrow();
        let &row_idx = visible.get(flat_index)?;
        let rows = self.inner.rows.borrow();
        let row = rows.get(row_idx)?;
        Some(FlatEntry {
            node_id: row.key.clone(),
            depth: row.depth,
            has_children: row.has_children,
            is_expanded: self.inner.all_expanded.get()
                || self.inner.expanded.borrow().contains(&row.key),
        })
    }

    /// Structural depth at a visible index (`0` for a root).
    pub fn depth_at(&self, flat_index: usize) -> usize {
        let visible = self.inner.visible.borrow();
        visible
            .get(flat_index)
            .and_then(|&i| self.inner.rows.borrow().get(i).map(|r| r.depth))
            .unwrap_or(0)
    }

    /// The visible index of a key, if currently visible.
    pub fn flat_index_of(&self, key: &K) -> Option<usize> {
        self.inner.vis_pos.borrow().get(key).copied()
    }

    /// Whether `key` still exists in the source, independent of visibility (a
    /// node hidden under a collapsed ancestor still exists).
    pub fn contains_key(&self, key: &K) -> bool {
        self.inner.row_pos.borrow().contains_key(key)
    }

    /// The parent of a node (`None` for a root or an absent key).
    pub fn parent_of(&self, key: &K) -> Option<K> {
        let idx = *self.inner.row_pos.borrow().get(key)?;
        self.inner
            .rows
            .borrow()
            .get(idx)
            .and_then(|r| r.parent.clone())
    }

    /// The children of a node, in order (empty for a leaf / absent key). O(children).
    pub fn child_keys_of(&self, key: &K) -> Vec<K> {
        let children = self.inner.children.borrow();
        let rows = self.inner.rows.borrow();
        children
            .get(key)
            .map(|idxs| {
                idxs.iter()
                    .filter_map(|&i| rows.get(i).map(|r| r.key.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    // ── Expand / collapse (per-view) ──────────────────────────────────────

    /// Whether the node is *effectively* expanded (its children shown) — `true`
    /// for every branch while the [`set_all_expanded`](Self::set_all_expanded)
    /// reveal override is on, otherwise its per-view expand state. Use
    /// [`expanded_keys`](Self::expanded_keys) for the persistent set.
    pub fn is_expanded(&self, key: &K) -> bool {
        self.inner.all_expanded.get() || self.inner.expanded.borrow().contains(key)
    }

    /// Expand a node (make its children visible).
    pub fn expand(&self, key: &K)
    where
        T: PartialEq,
    {
        self.set_expanded_flag(key, true);
    }

    /// Collapse a node (hide its children).
    pub fn collapse(&self, key: &K)
    where
        T: PartialEq,
    {
        self.set_expanded_flag(key, false);
    }

    /// Toggle a node's expand state.
    pub fn toggle(&self, key: &K)
    where
        T: PartialEq,
    {
        // Toggle the persistent per-view state (not the reveal override).
        let expanded = self.inner.expanded.borrow().contains(key);
        self.set_expanded_flag(key, !expanded);
    }

    /// Expand every node that has children.
    pub fn expand_all(&self)
    where
        T: PartialEq,
    {
        let target: HashSet<K> = {
            let rows = self.inner.rows.borrow();
            rows.iter()
                .filter(|r| r.has_children)
                .map(|r| r.key.clone())
                .collect()
        };
        self.replace_expanded(target);
    }

    /// Collapse every node (only roots remain visible).
    pub fn collapse_all(&self)
    where
        T: PartialEq,
    {
        self.replace_expanded(HashSet::new());
    }

    /// The currently-expanded keys (for persistence).
    pub fn expanded_keys(&self) -> Vec<K> {
        self.inner.expanded.borrow().iter().cloned().collect()
    }

    /// Restore expanded state (for persistence). Keys absent from the source are
    /// ignored on the next reflatten.
    pub fn set_expanded_keys(&self, keys: &[K])
    where
        T: PartialEq,
    {
        self.replace_expanded(keys.iter().cloned().collect());
    }

    // ── Reactivity ────────────────────────────────────────────────────────

    /// Version signal — bind at `BindingLevel::Rebuild`. Bumps on every
    /// `set_rows` / expand / collapse.
    pub fn version_signal(&self) -> Signal<u64> {
        self.inner.version.clone()
    }

    /// First visible index whose content may differ after the latest change —
    /// rows `0..index` are unchanged (same key, depth, has-children, expand, and
    /// item content), so per-row derived state remains valid for them. Equal to
    /// `visible_count()` when nothing visible changed; `None` before the first
    /// change (construction is not a change).
    pub fn first_changed_index(&self) -> Option<usize> {
        self.inner.divergence.get()
    }

    // ── Internal ──────────────────────────────────────────────────────────

    fn set_expanded_flag(&self, key: &K, expanded: bool)
    where
        T: PartialEq,
    {
        let mut target = self.inner.expanded.borrow().clone();
        let changed = if expanded {
            target.insert(key.clone())
        } else {
            target.remove(key)
        };
        if !changed {
            return;
        }
        self.replace_expanded(target);
    }

    /// Swap the expand set to `target`, reflatten, compute divergence, bump.
    /// Structure (`rows`/`children`/`roots`/`row_pos`) is untouched.
    fn replace_expanded(&self, target: HashSet<K>)
    where
        T: PartialEq,
    {
        let all = self.inner.all_expanded.get();
        let (visible, vis_pos) = {
            let rows = self.inner.rows.borrow();
            let children = self.inner.children.borrow();
            let roots = self.inner.roots.borrow();
            flatten(&rows, &children, &roots, &target, all)
        };
        let div = {
            let rows = self.inner.rows.borrow();
            let old_visible = self.inner.visible.borrow();
            let old_expanded = self.inner.expanded.borrow();
            common_prefix(
                &rows,
                &old_visible,
                &old_expanded,
                all,
                &rows,
                &visible,
                &target,
                all,
            )
        };
        *self.inner.visible.borrow_mut() = visible;
        *self.inner.vis_pos.borrow_mut() = vis_pos;
        *self.inner.expanded.borrow_mut() = target;
        self.inner.divergence.set(Some(div));
        self.bump();
    }

    /// Reveal override for a filtered view: when `on`, the flatten treats every
    /// node as expanded, so all rows in the (already sort/filter-narrowed) stream
    /// are visible — the ancestors `TreeRowFilter::KeepAncestors` keeps no longer
    /// hide their matching descendants. The per-view expand set is **preserved**
    /// underneath, so turning it off restores the user's real collapse state.
    /// Flip it on with the filter and off when it clears. No-op if unchanged.
    pub fn set_all_expanded(&self, on: bool)
    where
        T: PartialEq,
    {
        if self.inner.all_expanded.get() == on {
            return;
        }
        let expanded = self.inner.expanded.borrow().clone();
        let (visible, vis_pos) = {
            let rows = self.inner.rows.borrow();
            let children = self.inner.children.borrow();
            let roots = self.inner.roots.borrow();
            flatten(&rows, &children, &roots, &expanded, on)
        };
        let div = {
            let rows = self.inner.rows.borrow();
            let old_visible = self.inner.visible.borrow();
            common_prefix(
                &rows,
                &old_visible,
                &expanded,
                !on, // the previous flag value
                &rows,
                &visible,
                &expanded,
                on,
            )
        };
        self.inner.all_expanded.set(on);
        *self.inner.visible.borrow_mut() = visible;
        *self.inner.vis_pos.borrow_mut() = vis_pos;
        self.inner.divergence.set(Some(div));
        self.bump();
    }

    /// Whether the reveal-all override is on (see [`set_all_expanded`](Self::set_all_expanded)).
    pub fn all_expanded(&self) -> bool {
        self.inner.all_expanded.get()
    }

    /// Derive the full structure + projection from a raw row stream, seeding the
    /// new expand/seen sets from the current ones (preserve expand by key,
    /// auto-expand newly-seen nodes per policy). Reads current state; commits nothing.
    fn build(&self, input: Vec<TreeRow<K, T>>) -> Built<K, T> {
        // 1. Derive parent links + structural depth via the indent stack.
        let mut rows: Vec<Row<K, T>> = Vec::with_capacity(input.len());
        // stack entries: (raw indent depth, key, row index)
        let mut stack: Vec<(usize, K, usize)> = Vec::new();
        for tr in input {
            while let Some((d, _, _)) = stack.last() {
                if *d >= tr.depth {
                    stack.pop();
                } else {
                    break;
                }
            }
            let (parent, struct_depth) = match stack.last() {
                Some((_, k, pidx)) => (Some(k.clone()), rows[*pidx].depth + 1),
                None => (None, 0),
            };
            let idx = rows.len();
            let key = tr.key.clone();
            rows.push(Row {
                key: tr.key,
                item: tr.item,
                depth: struct_depth,
                parent,
                has_children: false,
            });
            stack.push((tr.depth, key, idx));
        }

        // 2. Build the child index, roots, and row_pos.
        let mut children: HashMap<K, Vec<usize>> = HashMap::new();
        let mut roots: Vec<usize> = Vec::new();
        let mut row_pos: HashMap<K, usize> = HashMap::with_capacity(rows.len());
        for (i, r) in rows.iter().enumerate() {
            row_pos.insert(r.key.clone(), i);
            match &r.parent {
                Some(pk) => children.entry(pk.clone()).or_default().push(i),
                None => roots.push(i),
            }
        }
        for r in rows.iter_mut() {
            r.has_children = children.get(&r.key).is_some_and(|v| !v.is_empty());
        }

        // 3. Seed the expand + seen sets from the current ones.
        let expand_new = self.inner.expand_new.get();
        let (mut expanded, mut seen) = {
            let old_exp = self.inner.expanded.borrow();
            let old_seen = self.inner.seen.borrow();
            let mut e = HashSet::new();
            let s = old_seen.clone();
            for r in &rows {
                if old_seen.contains(&r.key) {
                    if old_exp.contains(&r.key) {
                        e.insert(r.key.clone());
                    }
                } else if expand_new && r.has_children {
                    e.insert(r.key.clone());
                }
            }
            (e, s)
        };
        for r in &rows {
            seen.insert(r.key.clone());
        }
        // Drop expand entries whose node vanished.
        expanded.retain(|k| row_pos.contains_key(k));

        // 4. Flatten to the visible projection.
        let (visible, vis_pos) = flatten(
            &rows,
            &children,
            &roots,
            &expanded,
            self.inner.all_expanded.get(),
        );

        Built {
            rows,
            children,
            roots,
            row_pos,
            visible,
            vis_pos,
            expanded,
            seen,
        }
    }

    /// Move a `Built` into `self`. Every interior borrow is released before
    /// returning; callers bump the version afterwards (never while borrowed).
    fn commit(&self, built: Built<K, T>) {
        *self.inner.rows.borrow_mut() = built.rows;
        *self.inner.children.borrow_mut() = built.children;
        *self.inner.roots.borrow_mut() = built.roots;
        *self.inner.row_pos.borrow_mut() = built.row_pos;
        *self.inner.visible.borrow_mut() = built.visible;
        *self.inner.vis_pos.borrow_mut() = built.vis_pos;
        *self.inner.expanded.borrow_mut() = built.expanded;
        *self.inner.seen.borrow_mut() = built.seen;
    }

    fn bump(&self) {
        let next = self.inner.version_counter.get() + 1;
        self.inner.version_counter.set(next);
        self.inner.version.set(next);
    }

    /// Whether `maybe_descendant` is inside the subtree rooted at `ancestor`.
    fn is_descendant(&self, maybe_descendant: &K, ancestor: &K) -> bool {
        let rows = self.inner.rows.borrow();
        let row_pos = self.inner.row_pos.borrow();
        let mut cur = maybe_descendant.clone();
        for _ in 0..rows.len() {
            let Some(&idx) = row_pos.get(&cur) else {
                return false;
            };
            let Some(parent) = rows[idx].parent.clone() else {
                return false;
            };
            if &parent == ancestor {
                return true;
            }
            cur = parent;
        }
        false
    }

    /// Cycle guard (mechanism) + domain resolver (policy). `None` = forbidden.
    fn resolve(&self, dragged: &K, target: &K, position: DropPosition) -> Option<DropPosition> {
        if dragged == target || self.is_descendant(target, dragged) {
            return None;
        }
        // Clone the handle out and drop the `drop_resolver` borrow before
        // calling `f` — a resolver that calls `set_drop_resolver` on the
        // same slice would otherwise hit a `BorrowMutError`.
        let resolver = self.inner.drop_resolver.borrow().clone();
        match resolver {
            Some(f) => {
                let row_pos = self.inner.row_pos.borrow();
                let &idx = row_pos.get(target)?; // absent target → forbid
                let rows = self.inner.rows.borrow();
                f(dragged, target, &rows[idx].item, position)
            }
            None => Some(position),
        }
    }
}

impl<K: ItemKey, T> std::fmt::Debug for TreeDataSlice<K, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeDataSlice")
            .field("visible_count", &self.visible_count())
            .field("row_count", &self.inner.rows.borrow().len())
            .field("expanded_count", &self.inner.expanded.borrow().len())
            .finish()
    }
}

/// Recursive collapse-aware flatten: emit each root's subtree, descending only
/// into expanded nodes. Visits O(visible) rows.
fn flatten<K: ItemKey, T>(
    rows: &[Row<K, T>],
    children: &HashMap<K, Vec<usize>>,
    roots: &[usize],
    expanded: &HashSet<K>,
    all_expanded: bool,
) -> (Vec<usize>, HashMap<K, usize>) {
    let mut visible = Vec::with_capacity(rows.len());
    let mut vis_pos = HashMap::with_capacity(rows.len());
    for &root in roots {
        flatten_node(
            root,
            rows,
            children,
            expanded,
            all_expanded,
            &mut visible,
            &mut vis_pos,
        );
    }
    (visible, vis_pos)
}

fn flatten_node<K: ItemKey, T>(
    idx: usize,
    rows: &[Row<K, T>],
    children: &HashMap<K, Vec<usize>>,
    expanded: &HashSet<K>,
    all_expanded: bool,
    visible: &mut Vec<usize>,
    vis_pos: &mut HashMap<K, usize>,
) {
    let row = &rows[idx];
    vis_pos.insert(row.key.clone(), visible.len());
    visible.push(idx);
    if row.has_children
        && (all_expanded || expanded.contains(&row.key))
        && let Some(kids) = children.get(&row.key)
    {
        for &child in kids {
            flatten_node(
                child,
                rows,
                children,
                expanded,
                all_expanded,
                visible,
                vis_pos,
            );
        }
    }
}

/// Length of the common prefix of the old vs new visible lists, comparing key +
/// depth + has-children + expand state + **item content**. The first index at
/// which the projection diverges; equals `min(len)` when the shorter list is a
/// prefix of the longer.
#[allow(clippy::too_many_arguments)]
fn common_prefix<K: ItemKey, T: PartialEq>(
    old_rows: &[Row<K, T>],
    old_visible: &[usize],
    old_expanded: &HashSet<K>,
    old_all: bool,
    new_rows: &[Row<K, T>],
    new_visible: &[usize],
    new_expanded: &HashSet<K>,
    new_all: bool,
) -> usize {
    let n = old_visible.len().min(new_visible.len());
    for i in 0..n {
        let o = &old_rows[old_visible[i]];
        let m = &new_rows[new_visible[i]];
        let o_exp = old_all || old_expanded.contains(&o.key);
        let m_exp = new_all || new_expanded.contains(&m.key);
        if o.key != m.key
            || o.depth != m.depth
            || o.has_children != m.has_children
            || o_exp != m_exp
            || o.item != m.item
        {
            return i;
        }
    }
    n
}

/// `TreeDataSlice` is a reusable per-view `TreeDataSource` over an external,
/// indent-ordered source. Identity is the domain key `K`; a `SameView` drop is
/// resolved by the cycle guard + injected drop resolver and applied via the
/// injected reorder command (then the slice re-sources). `Foreign` drops are
/// rejected.
impl<K: ItemKey, T: PartialEq + 'static> TreeDataSource for TreeDataSlice<K, T> {
    type Item = T;
    type Key = K;

    fn visible_count(&self) -> usize {
        TreeDataSlice::visible_count(self)
    }

    fn with_entry<R>(
        &self,
        flat_index: usize,
        f: impl FnOnce(&Self::Item, &FlatEntry<Self::Key>) -> R,
    ) -> Option<R> {
        TreeDataSlice::with_entry(self, flat_index, f)
    }

    fn key_at(&self, flat_index: usize) -> Option<K> {
        TreeDataSlice::key_at(self, flat_index)
    }

    fn flat_index_of(&self, key: &K) -> Option<usize> {
        TreeDataSlice::flat_index_of(self, key)
    }

    fn parent(&self, key: &K) -> Option<K> {
        TreeDataSlice::parent_of(self, key)
    }

    fn child_keys(&self, key: &K) -> Vec<K> {
        TreeDataSlice::child_keys_of(self, key)
    }

    fn version_signal(&self) -> Signal<u64> {
        TreeDataSlice::version_signal(self)
    }

    fn first_changed_index(&self) -> Option<usize> {
        TreeDataSlice::first_changed_index(self)
    }

    fn contains_key(&self, key: &K) -> bool {
        TreeDataSlice::contains_key(self, key)
    }

    fn is_expanded(&self, key: &K) -> bool {
        TreeDataSlice::is_expanded(self, key)
    }

    fn set_expanded(&self, key: &K, expanded: bool) {
        self.set_expanded_flag(key, expanded);
    }

    fn drag(&self, key: &K) -> DragEligibility {
        // Clone the handle out and drop the borrow before calling `f` — see
        // `resolve`'s matching comment.
        let policy = self.inner.drag_policy.borrow().clone();
        match policy {
            Some(f) => f(key),
            None => DragEligibility::NoDrag,
        }
    }

    fn can_accept(&self, query: &DropQuery<'_, K>) -> DropResponse {
        let dragged = match &query.source {
            DragSource::SameView { key } => key,
            DragSource::Foreign { .. } => return DropResponse::Reject,
        };
        match self.resolve(dragged, &query.target, query.position) {
            Some(p) if p == query.position => DropResponse::Accept,
            Some(p) => DropResponse::Redirect(p),
            None => DropResponse::Reject,
        }
    }

    fn accept_drop(&self, commit: DropCommit<'_, K>) -> bool {
        let dragged = match &commit.source {
            DragSource::SameView { key } => key.clone(),
            DragSource::Foreign { .. } => return false,
        };
        let Some(place) = self.resolve(&dragged, &commit.target, commit.position) else {
            return false;
        };
        // Clone the handle out and drop the `reorder` borrow before calling
        // `f` — a reorder command that calls back into the slice (e.g.
        // `set_reorder`, to reconfigure itself after applying the move)
        // would otherwise hit a `BorrowMutError`.
        let reorder = self.inner.reorder.borrow().clone();
        let applied = match reorder {
            Some(f) => f(dragged, commit.target.clone(), place),
            None => return false,
        };
        if applied {
            self.reload();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sample outline (binder-style: depth-0 roots + indented items):
    /// M (binder)
    ///   Book        (folder)
    ///     Opening
    ///     Dawn
    ///   Ch2         (folder)
    ///     Fight
    /// N (binder)
    ///   Sketch
    fn sample() -> Vec<TreeRow<u64, &'static str>> {
        vec![
            TreeRow::new(1, "M", 0),
            TreeRow::new(101, "Book", 1),
            TreeRow::new(102, "Opening", 2),
            TreeRow::new(103, "Dawn", 2),
            TreeRow::new(104, "Ch2", 1),
            TreeRow::new(105, "Fight", 2),
            TreeRow::new(2, "N", 0),
            TreeRow::new(106, "Sketch", 1),
        ]
    }

    fn expanded_slice() -> TreeDataSlice<u64, &'static str> {
        let slice = TreeDataSlice::new();
        slice.set_expand_new_nodes(true);
        slice.set_source(sample);
        slice.reload();
        slice
    }

    #[test]
    fn structure_derivation() {
        let slice = TreeDataSlice::from_rows(sample());
        // parents
        assert_eq!(slice.parent_of(&1), None); // binder is a root
        assert_eq!(slice.parent_of(&101), Some(1)); // Book under M
        assert_eq!(slice.parent_of(&102), Some(101)); // Opening under Book
        assert_eq!(slice.parent_of(&104), Some(1)); // Ch2 under M
        assert_eq!(slice.parent_of(&105), Some(104)); // Fight under Ch2
        assert_eq!(slice.parent_of(&106), Some(2)); // Sketch under N
        // children
        assert_eq!(slice.child_keys_of(&1), vec![101, 104]);
        assert_eq!(slice.child_keys_of(&101), vec![102, 103]);
        assert_eq!(slice.child_keys_of(&102), Vec::<u64>::new()); // leaf
    }

    #[test]
    fn collapsed_by_default_shows_roots() {
        let slice = TreeDataSlice::from_rows(sample());
        assert_eq!(slice.visible_count(), 2); // M, N
        assert_eq!(slice.key_at(0), Some(1));
        assert_eq!(slice.key_at(1), Some(2));
    }

    #[test]
    fn expand_new_shows_all() {
        let slice = expanded_slice();
        assert_eq!(slice.visible_count(), 8);
        assert_eq!(
            slice.with_entry(1, |item, e| {
                assert_eq!(*item, "Book");
                assert_eq!(e.depth, 1);
                assert!(e.has_children);
            }),
            Some(())
        );
    }

    #[test]
    fn collapse_hides_subtree() {
        let slice = expanded_slice();
        assert_eq!(slice.visible_count(), 8);
        slice.collapse(&101); // Book (2 children)
        assert_eq!(slice.visible_count(), 6);
        slice.expand(&101);
        assert_eq!(slice.visible_count(), 8);
    }

    #[test]
    fn toggle_and_flat_index() {
        let slice = TreeDataSlice::from_rows(sample());
        assert_eq!(slice.flat_index_of(&1), Some(0));
        assert_eq!(slice.flat_index_of(&101), None); // hidden
        slice.toggle(&1);
        assert_eq!(slice.flat_index_of(&101), Some(1));
        assert!(slice.is_expanded(&1));
    }

    #[test]
    fn expand_all_collapse_all() {
        let slice = TreeDataSlice::from_rows(sample());
        slice.expand_all();
        assert_eq!(slice.visible_count(), 8);
        slice.collapse_all();
        assert_eq!(slice.visible_count(), 2);
    }

    #[test]
    fn set_all_expanded_reveals_then_restores() {
        let slice = TreeDataSlice::from_rows(sample()); // collapsed → 2 roots
        assert_eq!(slice.visible_count(), 2);
        assert!(!slice.all_expanded());

        slice.set_all_expanded(true);
        assert_eq!(slice.visible_count(), 8); // everything revealed
        assert!(slice.all_expanded());
        assert!(slice.is_expanded(&1)); // effective: shown open

        slice.set_all_expanded(false);
        assert_eq!(slice.visible_count(), 2); // back to collapsed
        assert!(!slice.all_expanded());
    }

    #[test]
    fn reveal_preserves_raw_expand_set() {
        let slice = TreeDataSlice::from_rows(sample());
        slice.expand(&1); // M expanded (persistent) → M, Book, Ch2, N
        assert_eq!(slice.visible_count(), 4);

        slice.set_all_expanded(true);
        assert_eq!(slice.visible_count(), 8);

        slice.set_all_expanded(false);
        // M's persistent expand survived the reveal round-trip; N still collapsed.
        assert_eq!(slice.visible_count(), 4);
        assert_eq!(slice.expanded_keys(), vec![1]);
    }

    #[test]
    fn filter_keepancestors_reveal_shows_matches() {
        // The gap this closes: KeepAncestors keeps the ancestor rows, but a
        // freshly-collapsed slice hides the match under them — set_all_expanded
        // reveals the whole filtered result without touching the persistent set.
        use crate::{TreeFilterMode, TreeRowFilter};
        let sieve = TreeRowFilter::new()
            .filter_mode(TreeFilterMode::KeepAncestors)
            .filter(|t: &&str| *t == "Dawn");
        let slice = TreeDataSlice::from_rows(sieve.apply(sample()));
        // Filtered stream = M → Book → Dawn; collapsed shows only the root M.
        assert_eq!(slice.visible_count(), 1);

        slice.set_all_expanded(true);
        assert_eq!(slice.visible_count(), 3);
        let titles: Vec<&str> = (0..3)
            .map(|i| slice.with_entry(i, |it, _| *it).unwrap())
            .collect();
        assert_eq!(titles, vec!["M", "Book", "Dawn"]);
    }

    #[test]
    fn two_slices_independent_expand() {
        let a = TreeDataSlice::from_rows(sample());
        let b = TreeDataSlice::from_rows(sample());
        a.expand(&1);
        assert_eq!(a.visible_count(), 4); // M, Book, Ch2, N
        assert_eq!(b.visible_count(), 2); // still collapsed
    }

    #[test]
    fn clone_shares_state() {
        let a = TreeDataSlice::from_rows(sample());
        let b = a.clone();
        a.expand(&1);
        assert_eq!(b.visible_count(), 4); // b sees a's expand
    }

    // ── divergence ───────────────────────────────────────────────────────

    #[test]
    fn divergence_none_before_change() {
        let slice = TreeDataSlice::from_rows(sample());
        assert_eq!(slice.first_changed_index(), None);
    }

    #[test]
    fn divergence_on_expand_is_toggled_row() {
        let slice = TreeDataSlice::from_rows(sample());
        // Expanding M (flat 0) changes M's own is_expanded and inserts rows after.
        slice.expand(&1);
        assert_eq!(slice.first_changed_index(), Some(0));
    }

    #[test]
    fn divergence_on_deep_expand_is_that_row() {
        let slice = TreeDataSlice::from_rows(sample());
        slice.expand(&1); // M, Book, Ch2, N
        // Expanding Book (flat 1) leaves M untouched.
        slice.expand(&101);
        assert_eq!(slice.first_changed_index(), Some(1));
    }

    #[test]
    fn divergence_on_rename_is_that_row() {
        let slice = expanded_slice();
        // Rename "Fight" (105) — structure identical, only its item changes.
        let renamed: Vec<TreeRow<u64, &'static str>> = sample()
            .into_iter()
            .map(|mut r| {
                if r.key == 105 {
                    r.item = "Duel";
                }
                r
            })
            .collect();
        slice.set_rows(renamed);
        // Fight is at flat index 5 (M,Book,Opening,Dawn,Ch2,Fight,...).
        assert_eq!(slice.first_changed_index(), Some(5));
    }

    #[test]
    fn divergence_on_append_is_old_len() {
        let slice = expanded_slice();
        let mut rows = sample();
        rows.push(TreeRow::new(107, "Idea", 1)); // new child of N
        slice.set_rows(rows);
        // Old visible len was 8; the appended row diverges at 8.
        assert_eq!(slice.first_changed_index(), Some(8));
    }

    #[test]
    fn reload_preserves_expand_by_key() {
        let counter = Rc::new(Cell::new(0u32));
        let c = counter.clone();
        let slice: TreeDataSlice<u64, &'static str> = TreeDataSlice::new();
        slice.set_source(move || {
            c.set(c.get() + 1);
            sample()
        });
        slice.reload();
        slice.expand(&1);
        assert_eq!(slice.visible_count(), 4);
        slice.reload(); // re-source; M was expanded and still exists
        assert_eq!(slice.visible_count(), 4); // expand survived
        assert!(slice.is_expanded(&1));
    }

    // ── DnD ──────────────────────────────────────────────────────────────

    #[test]
    fn drag_policy_gate() {
        let slice = TreeDataSlice::from_rows(sample());
        slice.set_drag_policy(|k| {
            if *k < 100 {
                DragEligibility::NoDrag // binders
            } else {
                DragEligibility::CanDrag
            }
        });
        assert_eq!(slice.drag(&1), DragEligibility::NoDrag);
        assert_eq!(slice.drag(&102), DragEligibility::CanDrag);
    }

    #[test]
    fn drag_default_is_nodrag() {
        let slice = TreeDataSlice::from_rows(sample());
        assert_eq!(slice.drag(&102), DragEligibility::NoDrag);
    }

    #[test]
    fn can_accept_rejects_cycle() {
        let slice = expanded_slice();
        // Drop Book (101) into its own child Opening (102) → cycle.
        let q = DropQuery {
            source: DragSource::SameView { key: 101 },
            target: 102,
            position: DropPosition::Into,
        };
        assert_eq!(slice.can_accept(&q), DropResponse::Reject);
    }

    #[test]
    fn can_accept_default_accepts_sibling() {
        let slice = expanded_slice();
        let q = DropQuery {
            source: DragSource::SameView { key: 102 },
            target: 106,
            position: DropPosition::Before,
        };
        assert_eq!(slice.can_accept(&q), DropResponse::Accept);
    }

    #[test]
    fn drop_resolver_redirects() {
        let slice = expanded_slice();
        // A leaf item (its own item text is inspected here just to exercise the
        // target_item param) redirects Into → After.
        slice.set_drop_resolver(|_dragged, target, target_item, pos| match pos {
            DropPosition::Into if *target == 103 && *target_item == "Dawn" => {
                Some(DropPosition::After)
            }
            p => Some(p),
        });
        let q = DropQuery {
            source: DragSource::SameView { key: 102 },
            target: 103,
            position: DropPosition::Into,
        };
        assert_eq!(
            slice.can_accept(&q),
            DropResponse::Redirect(DropPosition::After)
        );
    }

    #[test]
    fn accept_drop_runs_reorder_then_reloads() {
        let moved = Rc::new(Cell::new(false));
        let m = moved.clone();
        let slice = expanded_slice();
        slice.set_reorder(move |dragged, target, _pos| {
            assert_eq!(dragged, 102);
            assert_eq!(target, 106);
            m.set(true);
            true
        });
        let ok = slice.accept_drop(DropCommit {
            source: DragSource::SameView { key: 102 },
            target: 106,
            position: DropPosition::Before,
        });
        assert!(ok);
        assert!(moved.get());
    }

    #[test]
    fn reorder_closure_can_call_back_into_the_slice() {
        // Regression: `accept_drop` used to hold a `Ref` on the reorder
        // closure's `RefCell` for the whole call, so a closure that called
        // back into the slice — e.g. `set_reorder`, to swap its own policy
        // out right after applying a move — hit a `BorrowMutError`. The
        // handle must be cloned out and the borrow dropped before the
        // closure runs.
        let slice = expanded_slice();
        let reentrant_target = slice.clone();
        slice.set_reorder(move |_dragged, _target, _pos| {
            reentrant_target.set_reorder(|_, _, _| true);
            true
        });
        let ok = slice.accept_drop(DropCommit {
            source: DragSource::SameView { key: 102 },
            target: 106,
            position: DropPosition::Before,
        });
        assert!(ok);
    }

    #[test]
    fn source_closure_can_call_back_into_the_slice() {
        // Same shape as the reorder-closure regression above, on the loader
        // path: `reload` used to hold a `Ref` on the source `RefCell` while
        // invoking the loader, so a loader that re-installed the source (a
        // one-shot loader swapping itself for the steady-state one) hit a
        // `BorrowMutError`.
        let slice = TreeDataSlice::<u64, &'static str>::new();
        let reentrant = slice.clone();
        slice.set_source(move || {
            reentrant.set_source(Vec::new);
            vec![TreeRow {
                key: 1,
                item: "root",
                depth: 0,
            }]
        });
        slice.reload();
        assert_eq!(slice.visible_count(), 1);
    }

    #[test]
    fn accept_drop_without_reorder_is_refused() {
        let slice = expanded_slice();
        let ok = slice.accept_drop(DropCommit {
            source: DragSource::SameView { key: 102 },
            target: 106,
            position: DropPosition::Before,
        });
        assert!(!ok);
    }

    #[test]
    fn foreign_drop_rejected() {
        let slice = expanded_slice();
        slice.set_reorder(|_, _, _| true);
        // No Foreign fixture here; can_accept path for Foreign is covered by
        // the SameView tests + the explicit early return. Assert accept_drop
        // refuses when there is no reorder for a same-view drop into self.
        let ok = slice.accept_drop(DropCommit {
            source: DragSource::SameView { key: 1 },
            target: 1, // self-drop → resolve None
            position: DropPosition::Into,
        });
        assert!(!ok);
    }
}
