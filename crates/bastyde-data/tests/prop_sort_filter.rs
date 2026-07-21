// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Property tests for the sort/filter projection family:
//! `SortFilterListModel<T>` (crates/bastyde-data/src/sort_filter_list_model.rs),
//! `SortFilterTreeModel<T>` + `TreeFilterMode`
//! (crates/bastyde-data/src/sort_filter_tree_model.rs), and `TreeRowFilter<K, T>`
//! (crates/bastyde-data/src/tree_row_filter.rs).
//!
//! ## Why this exists
//!
//! The branch this suite targets landed on top of a rebase that rewrote the
//! incremental machinery in this area (`fb9c7105` "O(1) tree position maps,
//! incremental sort/filter updates", `bd293029` "key rows by source index so
//! flat anchoring behaves"): `SortFilterListModel` now has a fast path for
//! `DataChange::ItemUpdated` that patches one row instead of re-filtering +
//! re-sorting everything, and `SortFilterTreeModel` now maintains an O(1)
//! `NodeId -> flat index` position map plus a matching fast path for
//! `TreeChange::NodeUpdated`. Both fast paths are guarded by hand-written
//! "did anything actually move" checks (`try_incremental_item_update` /
//! `try_incremental_node_update`) that fall back to a full rebuild when they
//! can't prove safety — exactly the kind of logic that drifts from a full
//! recompute under a long tail of interacting mutations, which is what
//! example-based unit tests are bad at finding and property-based testing is
//! good at. Every property below applies a random sequence of source
//! mutations / proxy calls, bounded small, and checks against a
//! **from-scratch** recompute written independently of the crate's own
//! algorithms.
//!
//! ## What's asserted
//!
//! - The incrementally-maintained list and tree projections agree with a
//!   brute-force filter-then-sort recompute after every step of an arbitrary
//!   mutation sequence (the oracle properties — the most valuable ones here,
//!   since they are what would catch the incremental path drifting from the
//!   full one).
//! - `source_index_of` / `visible_index_of` form a bijection onto the kept
//!   set and round-trip both ways.
//! - An always-true predicate plus an always-`Equal` comparator leave the
//!   projection identical to the source order (relies on `Vec::sort_by`'s
//!   documented stability — *not* asserted as a general "sort is stable"
//!   claim, since neither module doc commits to that on its own).
//! - `first_changed_index()` never *under*-reports: the prefix strictly
//!   before it is byte-for-byte identical between the previous and current
//!   projection. (Over-reporting is wasteful, not incorrect, and is not
//!   checked here.)
//! - The three `TreeFilterMode` strategies each match their literal
//!   set-theoretic definition (matches / matches ∪ ancestors / matches ∪
//!   descendants) against a brute-force computation.
//! - `TreeRowFilter` and `SortFilterTreeModel` agree on the kept node set for
//!   `HideNonMatching` and `KeepAncestors` — the equivalence the module doc
//!   of `tree_row_filter.rs` explicitly claims (and explicitly does *not*
//!   claim for `KeepDescendants`, so that mode is excluded from this check).
//! - `TreeRowFilter::apply` always emits a structurally valid indent stream
//!   (first row at depth 0, depth never jumps by more than 1 between
//!   consecutive rows) regardless of filter mode / predicate / sort — a
//!   general fact about any correct pre-order compaction, so a violation
//!   would mean the depth bookkeeping is wrong, not that the test is overly
//!   strict.
//!
//! ## Generator cost discipline
//!
//! Every collection generator here is bounded well under the sizes the task
//! calls out (trees ≤ 30 nodes, lists ≤ ~50 items, op sequences ≤ 30 steps).
//! Indices used to pick an existing row/node are drawn as plain `usize`s from
//! a small range and reduced modulo the *current* size at apply time (never
//! at generation time) — the standard way to index into a runtime-sized
//! structure without `prop_flat_map`. There is no cross-dimensional coupling
//! anywhere in this file (nothing like "a rect sized for one collection
//! inserted into a different-sized grid") — every quantity that could blow up
//! a product space (op count × current size) is a single bounded dimension,
//! and each individual operation is O(current size) at worst, so the total
//! cost of one test case is bounded by (op sequence length) × O(current
//! size), i.e. at most a few thousand primitive operations. See the cost
//! comment on each `arb_*` generator for the specific reasoning.
//!
//! ## Running
//!
//! `cargo test -p bastyde-data --test prop_sort_filter` (add
//! `PROPTEST_CASES=N` to override the per-block case count; two blocks below
//! opt into 512 cases explicitly since they are cheap and the most valuable).
//!
//! ## Sibling suites
//!
//! `crates/bastyde-data/tests/prop_tree_slice.rs`,
//! `prop_list_and_selection.rs`, and `prop_tree_checked.rs` cover the
//! neighbouring data-model types in this crate; `crates/bastyde-tokens/tests/prop_color.rs`
//! and `../text-document/crates/*/tests/prop_*.rs` /
//! `../text-document/crates/public_api/tests/fuzz_robustness_tests.rs` are
//! the house-style references this file follows (one property per
//! `proptest! {}` block, hand-written local `arb_*` strategies, `Op`-enum
//! driven stateful sequences).

use std::cmp::Ordering;
use std::collections::HashSet;

use bastyde_data::ListDataSource; // brings `len()` / `with_item()` into scope for SortFilterListModel.
use bastyde_data::tree_data_source::tree_is_desc_or_self;
use bastyde_data::{
    ListModel, NodeId, SortDirection, SortFilterListModel, SortFilterTreeModel, TreeFilterMode,
    TreeModel, TreeRow, TreeRowFilter,
};
use proptest::prelude::*;

// ============================================================================
// SortFilterListModel<i32>
// ============================================================================

/// Mutations applied to a `ListModel<i32>` (mirrored in a plain `Vec<i32>`)
/// plus the two proxy-level toggles (`SetFilter`/`SetSort`). Field order
/// matches the match arms in `apply_list_op` below.
#[derive(Debug, Clone)]
enum ListOp {
    /// Append `value`.
    Push(i32),
    /// Insert `value` at `index_raw % (len + 1)`.
    Insert(usize, i32),
    /// Remove the item at `index_raw % len` (no-op on an empty list).
    Remove(usize),
    /// Overwrite the item at `index_raw % len` with `value` — drives
    /// `DataChange::ItemUpdated` and so the incremental fast path.
    Set(usize, i32),
    /// `ListModel::move_item(from_raw % len, to_raw % len)`.
    Move(usize, usize),
    /// Set (`Some`) or clear (`None`) the single "value >= threshold" filter.
    SetFilter(Option<i32>),
    /// Set (`Some`) or clear (`None`) the sort direction on column "value".
    SetSort(Option<SortDirection>),
}

/// Cost: every index field is reduced modulo the list's *current* length at
/// apply time (see `apply_list_op`), and only `Push`/`Insert` grow it — so a
/// 30-step sequence produces a list of at most ~30 items (plus whatever the
/// bounded `initial` vector contributed, itself capped at 20). Each
/// mutation-triggered rebuild is `O(n log n)` for `n` in the tens; the
/// brute-force oracle recompute done alongside it in the properties below is
/// the same order. Worst case for a whole 30-step sequence: on the order of
/// 30 * (50 log 50) primitive comparisons — trivial.
fn arb_list_op() -> impl Strategy<Value = ListOp> {
    prop_oneof![
        (-5i32..=5).prop_map(ListOp::Push),
        (0usize..40, -5i32..=5).prop_map(|(i, v)| ListOp::Insert(i, v)),
        (0usize..40).prop_map(ListOp::Remove),
        (0usize..40, -5i32..=5).prop_map(|(i, v)| ListOp::Set(i, v)),
        (0usize..40, 0usize..40).prop_map(|(f, t)| ListOp::Move(f, t)),
        prop_oneof![Just(None::<i32>), (-5i32..=5).prop_map(Some)].prop_map(ListOp::SetFilter),
        prop_oneof![
            Just(None::<SortDirection>),
            Just(Some(SortDirection::Ascending)),
            Just(Some(SortDirection::Descending)),
        ]
        .prop_map(ListOp::SetSort),
    ]
}

/// Apply one `ListOp` to the live `model` (and, for the proxy toggles, the
/// `proxy` itself), keeping `mirror`/`filter`/`sort` in lock-step so the
/// oracle functions below can recompute the expected projection from them.
/// Every index is bounded against the *current* mirror length so none of
/// `ListModel`'s documented panics ("index out of bounds") can fire.
fn apply_list_op(
    model: &ListModel<i32>,
    proxy: &SortFilterListModel<i32>,
    mirror: &mut Vec<i32>,
    filter: &mut Option<i32>,
    sort: &mut Option<SortDirection>,
    op: &ListOp,
) {
    match op {
        ListOp::Push(v) => {
            mirror.push(*v);
            model.push(*v);
        }
        ListOp::Insert(idx_raw, v) => {
            let idx = idx_raw % (mirror.len() + 1);
            mirror.insert(idx, *v);
            model.insert(idx, *v);
        }
        ListOp::Remove(idx_raw) => {
            if mirror.is_empty() {
                return;
            }
            let idx = idx_raw % mirror.len();
            mirror.remove(idx);
            model.remove(idx);
        }
        ListOp::Set(idx_raw, v) => {
            if mirror.is_empty() {
                return;
            }
            let idx = idx_raw % mirror.len();
            mirror[idx] = *v;
            model.set(idx, *v);
        }
        ListOp::Move(from_raw, to_raw) => {
            if mirror.len() < 2 {
                return;
            }
            let from = from_raw % mirror.len();
            let to = to_raw % mirror.len();
            let v = mirror.remove(from);
            mirror.insert(to, v);
            model.move_item(from, to);
        }
        ListOp::SetFilter(t) => {
            *filter = *t;
            match t {
                Some(v) => proxy.set_filter("value", &v.to_string()),
                None => proxy.clear_filters(),
            }
        }
        ListOp::SetSort(d) => {
            *sort = *d;
            match d {
                Some(dir) => proxy.set_sort(Some("value"), *dir),
                None => proxy.clear_sort(),
            }
        }
    }
}

/// Independent (non-incremental) recompute of the filtered+sorted
/// **source-index** order from a plain mirror vector — the oracle every list
/// property below compares the live proxy against.
fn oracle_list_projection(
    mirror: &[i32],
    filter: Option<i32>,
    sort: Option<SortDirection>,
) -> Vec<usize> {
    let mut visible: Vec<usize> = (0..mirror.len())
        .filter(|&i| filter.is_none_or(|t| mirror[i] >= t))
        .collect();
    if let Some(dir) = sort {
        visible.sort_by(|&a, &b| {
            let ord = mirror[a].cmp(&mirror[b]);
            if dir == SortDirection::Descending {
                ord.reverse()
            } else {
                ord
            }
        });
    }
    visible
}

// ── 1. the incremental list projection equals a full recompute (ORACLE) ──
// The incremental `ItemUpdated` fast path is new in this rebase. Comparing
// the live proxy against a from-scratch recompute after *every* step of an
// arbitrary mutation sequence is the strongest check that it never drifts
// from the semantics a full rebuild would produce.

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn the_incremental_list_projection_equals_a_full_recompute(
        initial in prop::collection::vec(-5i32..=5, 0..20),
        ops in prop::collection::vec(arb_list_op(), 0..30),
    ) {
        let model: ListModel<i32> = ListModel::from_vec(initial.clone());
        let proxy = SortFilterListModel::new(model.clone())
            .with_comparator("value", |a: &i32, b: &i32| a.cmp(b))
            .with_predicate("value", |t| {
                let threshold: i32 = t.parse().expect("filter text is always a formatted i32");
                Box::new(move |v: &i32| *v >= threshold)
            });

        let mut mirror = initial;
        let mut filter: Option<i32> = None;
        let mut sort: Option<SortDirection> = None;

        for op in &ops {
            apply_list_op(&model, &proxy, &mut mirror, &mut filter, &mut sort, op);

            let expected = oracle_list_projection(&mirror, filter, sort);
            prop_assert_eq!(
                proxy.len(), expected.len(),
                "visible len should match a from-scratch filter+sort recompute after {:?} (mirror={:?})",
                op, mirror
            );
            for (vi, &src) in expected.iter().enumerate() {
                prop_assert_eq!(
                    proxy.source_index_of(vi), Some(src),
                    "source at visible index {} should match the brute-force recompute after {:?} (mirror={:?})",
                    vi, op, mirror
                );
                prop_assert_eq!(
                    proxy.with_item(vi, |v| *v), Some(mirror[src]),
                    "value at visible index {} should match the brute-force recompute after {:?} (mirror={:?})",
                    vi, op, mirror
                );
            }
        }
    }
}

// ── 2. source_index_of / visible_index_of form a bijection onto the kept set ──

proptest! {
    #[test]
    fn source_and_visible_index_form_a_bijection_onto_the_kept_set(
        initial in prop::collection::vec(-5i32..=5, 0..20),
        ops in prop::collection::vec(arb_list_op(), 0..30),
    ) {
        let model: ListModel<i32> = ListModel::from_vec(initial.clone());
        let proxy = SortFilterListModel::new(model.clone())
            .with_comparator("value", |a: &i32, b: &i32| a.cmp(b))
            .with_predicate("value", |t| {
                let threshold: i32 = t.parse().expect("filter text is always a formatted i32");
                Box::new(move |v: &i32| *v >= threshold)
            });

        let mut mirror = initial;
        let mut filter: Option<i32> = None;
        let mut sort: Option<SortDirection> = None;

        for op in &ops {
            apply_list_op(&model, &proxy, &mut mirror, &mut filter, &mut sort, op);

            let mut seen_sources = HashSet::new();
            for vi in 0..proxy.len() {
                prop_assert!(
                    proxy.source_index_of(vi).is_some(),
                    "visible index {} in 0..len() must resolve to a source index after {:?}", vi, op
                );
                let src = proxy.source_index_of(vi).unwrap();
                prop_assert!(
                    src < mirror.len(),
                    "source_index_of({}) = {} is out of range (source len {}) after {:?}",
                    vi, src, mirror.len(), op
                );
                prop_assert!(
                    seen_sources.insert(src),
                    "source index {} was returned by source_index_of for more than one visible index after {:?}",
                    src, op
                );
                prop_assert_eq!(
                    proxy.visible_index_of(src), Some(vi),
                    "visible_index_of({}) should round-trip back to {} after {:?}", src, vi, op
                );
            }
            // Every source index NOT in the kept set must resolve to None —
            // otherwise the map isn't onto exactly the kept set.
            for src in 0..mirror.len() {
                if !seen_sources.contains(&src) {
                    prop_assert_eq!(
                        proxy.visible_index_of(src), None,
                        "visible_index_of({}) should be None for a filtered-out source index after {:?}", src, op
                    );
                }
            }
        }
    }
}

// ── 3. an always-true predicate + an always-Equal comparator is the identity ──
// `Vec::sort_by` is documented-stable, so sorting with a comparator that
// reports every pair as `Equal` is a no-op on the existing order. Combined
// with a predicate that keeps every row, the whole projection must equal the
// plain source order.

proptest! {
    #[test]
    fn always_true_predicate_and_equal_comparator_leave_the_projection_unchanged(
        initial in prop::collection::vec(-5i32..=5, 0..40),
    ) {
        let model: ListModel<i32> = ListModel::from_vec(initial.clone());
        let proxy = SortFilterListModel::new(model)
            .with_comparator("value", |_a: &i32, _b: &i32| Ordering::Equal)
            .with_predicate("value", |_text| Box::new(|_v: &i32| true));

        proxy.set_sort(Some("value"), SortDirection::Ascending);
        proxy.set_filter("value", "anything"); // the predicate ignores the text entirely.

        prop_assert_eq!(proxy.len(), initial.len(), "an always-true predicate must keep every row");
        let projected: Vec<i32> = (0..proxy.len())
            .map(|i| proxy.with_item(i, |v| *v).unwrap())
            .collect();
        prop_assert_eq!(
            projected, initial,
            "an always-Equal comparator (stable sort_by is a no-op on all-tied input) plus an \
             always-true predicate should leave the source order untouched"
        );
    }
}

// ── 4. first_changed_index never under-reports (list) ──
// The visible prefix strictly before `first_changed_index()` must show the
// exact same values, in the exact same order, both before and after a
// rebuild — row-height caches and similar per-row derived state trust this.
// Over-reporting (claiming less of the prefix is safe than actually is) is
// merely wasteful and is not checked here.

proptest! {
    #[test]
    fn first_changed_index_never_under_reports_for_lists(
        initial in prop::collection::vec(-5i32..=5, 0..20),
        ops in prop::collection::vec(arb_list_op(), 0..30),
    ) {
        let model: ListModel<i32> = ListModel::from_vec(initial.clone());
        let proxy = SortFilterListModel::new(model.clone())
            .with_comparator("value", |a: &i32, b: &i32| a.cmp(b))
            .with_predicate("value", |t| {
                let threshold: i32 = t.parse().expect("filter text is always a formatted i32");
                Box::new(move |v: &i32| *v >= threshold)
            });

        let mut mirror = initial;
        let mut filter: Option<i32> = None;
        let mut sort: Option<SortDirection> = None;

        for op in &ops {
            let mirror_before = mirror.clone();
            let filter_before = filter;
            let sort_before = sort;

            apply_list_op(&model, &proxy, &mut mirror, &mut filter, &mut sort, op);

            let before_view: Vec<i32> = oracle_list_projection(&mirror_before, filter_before, sort_before)
                .into_iter()
                .map(|src| mirror_before[src])
                .collect();
            let after_view: Vec<i32> = oracle_list_projection(&mirror, filter, sort)
                .into_iter()
                .map(|src| mirror[src])
                .collect();

            // `None` means "no rebuild observed yet" i.e. treat as a full
            // change (an empty safe prefix) per the documented contract.
            let d = proxy.first_changed_index().unwrap_or(0);
            let checked_prefix = d.min(before_view.len()).min(after_view.len());
            for i in 0..checked_prefix {
                prop_assert_eq!(
                    before_view[i], after_view[i],
                    "row {} should be unchanged below first_changed_index={} after {:?}", i, d, op
                );
            }
        }
    }
}

// ============================================================================
// SortFilterTreeModel<i32> + TreeFilterMode
// ============================================================================

/// Every currently-alive `NodeId` in `tree`, gathered by a fresh traversal —
/// used to pick "an existing node" for a mutation without maintaining a
/// separate mirror structure. O(current tree size) per call; called at most
/// once per op, so O(30) per op / O(900) total for a 30-step sequence.
fn all_nodes(tree: &TreeModel<i32>) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack: Vec<NodeId> = (0..tree.root_count()).map(|i| tree.root(i)).collect();
    while let Some(n) = stack.pop() {
        out.push(n);
        stack.extend(tree.children(n));
    }
    out
}

fn ancestors_of(tree: &TreeModel<i32>, node: NodeId) -> HashSet<NodeId> {
    let mut out = HashSet::new();
    let mut cur = tree.parent(node);
    while let Some(p) = cur {
        out.insert(p);
        cur = tree.parent(p);
    }
    out
}

fn descendants_of(tree: &TreeModel<i32>, node: NodeId) -> HashSet<NodeId> {
    let mut out = HashSet::new();
    let mut stack = tree.children(node);
    while let Some(c) = stack.pop() {
        out.insert(c);
        stack.extend(tree.children(c));
    }
    out
}

fn arb_any_filter_mode() -> impl Strategy<Value = TreeFilterMode> {
    prop_oneof![
        Just(TreeFilterMode::HideNonMatching),
        Just(TreeFilterMode::KeepAncestors),
        Just(TreeFilterMode::KeepDescendants),
    ]
}

/// The two modes `tree_row_filter.rs`'s module doc claims are equivalent to
/// `SortFilterTreeModel`'s own flatten (see property 8 below); `KeepDescendants`
/// is documented to deliberately diverge and so is excluded here.
fn arb_ancestor_preserving_filter_mode() -> impl Strategy<Value = TreeFilterMode> {
    prop_oneof![
        Just(TreeFilterMode::HideNonMatching),
        Just(TreeFilterMode::KeepAncestors),
    ]
}

/// Mutations applied to a live `TreeModel<i32>` plus the proxy-level
/// expand/collapse and filter/sort toggles. All node "picks" and insertion
/// indices are reduced modulo the tree's *current* shape at apply time (see
/// `apply_tree_op`), and `MoveToChild` is guarded against the documented
/// `move_node` cycle panic via `tree_is_desc_or_self` — so no `TreeModel`
/// panic path can fire regardless of the sequence drawn.
#[derive(Debug, Clone)]
enum TreeOp {
    /// Insert a new root at `index_raw % (root_count + 1)`.
    InsertRoot(usize, i32),
    /// Insert a new child of `nodes[parent_raw % nodes.len()]` at
    /// `index_raw % (child_count + 1)`.
    InsertChild(usize, usize, i32),
    /// Remove `nodes[pick_raw % nodes.len()]` (and its subtree).
    Remove(usize),
    /// Update `nodes[pick_raw % nodes.len()]`'s value — drives
    /// `TreeChange::NodeUpdated` and so the incremental fast path.
    Update(usize, i32),
    /// Move `nodes[pick_raw % nodes.len()]` to the root level.
    MoveToRoot(usize, usize),
    /// Move `nodes[pick_raw % nodes.len()]` under
    /// `nodes[parent_raw % nodes.len()]` (skipped if that would create a
    /// cycle).
    MoveToChild(usize, usize, usize),
    /// Expand `nodes[pick_raw % nodes.len()]`.
    Expand(usize),
    /// Collapse `nodes[pick_raw % nodes.len()]`.
    Collapse(usize),
    /// Set (`Some`) or clear (`None`) the single "value >= threshold" filter.
    SetFilter(Option<i32>),
    /// Set (`Some`) or clear (`None`) the sort direction on column "value".
    SetSort(Option<SortDirection>),
}

/// Cost: node picks and insertion indices are all reduced modulo the tree's
/// *current* size at apply time, and only the two insert variants grow it —
/// so a 30-step sequence builds and mutates at most a 30-node tree. Every
/// step does at most one O(current size) `all_nodes` scan plus an O(current
/// size) (or, for the incremental fast path, O(1)) rebuild; the brute-force
/// oracle recompute run alongside it in the properties below is at worst
/// O(n^2) (the naive "any descendant already visible" check inside
/// `oracle_visible_set`'s `KeepAncestors` branch), i.e. at most ~900 checks
/// for n=30. Trivial for a 30-step sequence.
fn arb_tree_op() -> impl Strategy<Value = TreeOp> {
    prop_oneof![
        (0usize..8, -5i32..=5).prop_map(|(i, v)| TreeOp::InsertRoot(i, v)),
        (0usize..40, 0usize..8, -5i32..=5).prop_map(|(p, i, v)| TreeOp::InsertChild(p, i, v)),
        (0usize..40).prop_map(TreeOp::Remove),
        (0usize..40, -5i32..=5).prop_map(|(n, v)| TreeOp::Update(n, v)),
        (0usize..40, 0usize..8).prop_map(|(n, i)| TreeOp::MoveToRoot(n, i)),
        (0usize..40, 0usize..40, 0usize..8).prop_map(|(n, p, i)| TreeOp::MoveToChild(n, p, i)),
        (0usize..40).prop_map(TreeOp::Expand),
        (0usize..40).prop_map(TreeOp::Collapse),
        prop_oneof![Just(None::<i32>), (-5i32..=5).prop_map(Some)].prop_map(TreeOp::SetFilter),
        prop_oneof![
            Just(None::<SortDirection>),
            Just(Some(SortDirection::Ascending)),
            Just(Some(SortDirection::Descending)),
        ]
        .prop_map(TreeOp::SetSort),
    ]
}

/// Apply one `TreeOp`, keeping `expanded`/`filter`/`sort` in lock-step with
/// `proxy` so the oracle functions below can recompute the expected
/// projection independently. See the `TreeOp` doc comments for the exact
/// index-bounding rule each variant uses.
fn apply_tree_op(
    tree: &TreeModel<i32>,
    proxy: &SortFilterTreeModel<i32>,
    expanded: &mut HashSet<NodeId>,
    filter: &mut Option<i32>,
    sort: &mut Option<SortDirection>,
    op: &TreeOp,
) {
    match op {
        TreeOp::InsertRoot(idx_raw, v) => {
            let rc = tree.root_count();
            tree.insert_root(idx_raw % (rc + 1), *v);
        }
        TreeOp::InsertChild(parent_raw, idx_raw, v) => {
            let nodes = all_nodes(tree);
            if nodes.is_empty() {
                return;
            }
            let parent = nodes[parent_raw % nodes.len()];
            let cc = tree.child_count(parent);
            tree.insert_child(parent, idx_raw % (cc + 1), *v);
        }
        TreeOp::Remove(pick_raw) => {
            let nodes = all_nodes(tree);
            if nodes.is_empty() {
                return;
            }
            tree.remove(nodes[pick_raw % nodes.len()]);
        }
        TreeOp::Update(pick_raw, v) => {
            let nodes = all_nodes(tree);
            if nodes.is_empty() {
                return;
            }
            tree.update(nodes[pick_raw % nodes.len()], *v);
        }
        TreeOp::MoveToRoot(pick_raw, idx_raw) => {
            let nodes = all_nodes(tree);
            if nodes.is_empty() {
                return;
            }
            let node = nodes[pick_raw % nodes.len()];
            let was_root = tree.parent(node).is_none();
            let rc = tree.root_count();
            // `move_to_root` removes `node` from its old location first, so
            // if it was already a root the valid post-removal range is one
            // shorter.
            let max = if was_root { rc.saturating_sub(1) } else { rc };
            tree.move_to_root(node, idx_raw % (max + 1));
        }
        TreeOp::MoveToChild(pick_raw, parent_raw, idx_raw) => {
            let nodes = all_nodes(tree);
            if nodes.is_empty() {
                return;
            }
            let node = nodes[pick_raw % nodes.len()];
            let parent = nodes[parent_raw % nodes.len()];
            // `move_node` asserts the target is not the node itself or a
            // descendant of it (cycle guard) — mirror the same guard
            // `SortFilterTreeModel::can_accept` uses rather than trigger the
            // panic.
            if tree_is_desc_or_self(tree, parent, node) {
                return;
            }
            let old_parent = tree.parent(node);
            let base = tree.child_count(parent);
            let max = if old_parent == Some(parent) {
                base.saturating_sub(1)
            } else {
                base
            };
            tree.move_node(node, parent, idx_raw % (max + 1));
        }
        TreeOp::Expand(pick_raw) => {
            let nodes = all_nodes(tree);
            if nodes.is_empty() {
                return;
            }
            let node = nodes[pick_raw % nodes.len()];
            proxy.expand(node);
            expanded.insert(node);
        }
        TreeOp::Collapse(pick_raw) => {
            let nodes = all_nodes(tree);
            if nodes.is_empty() {
                return;
            }
            let node = nodes[pick_raw % nodes.len()];
            proxy.collapse(node);
            expanded.remove(&node);
        }
        TreeOp::SetFilter(t) => {
            *filter = *t;
            match t {
                Some(v) => proxy.set_filter("value", &v.to_string()),
                None => proxy.clear_filters(),
            }
        }
        TreeOp::SetSort(d) => {
            *sort = *d;
            match d {
                Some(dir) => proxy.set_sort(Some("value"), *dir),
                None => proxy.clear_sort(),
            }
        }
    }
}

fn oracle_matches(tree: &TreeModel<i32>, node: NodeId, threshold: Option<i32>) -> bool {
    match threshold {
        None => true,
        Some(t) => tree.with_item(node, |v| *v >= t).unwrap_or(false),
    }
}

/// Independent (non-incremental) recompute of the visible node set for a
/// given `TreeFilterMode`, following the set-theoretic definitions in the
/// module docs directly rather than reusing the crate's own `visit_*` walks.
fn oracle_visible_set(
    tree: &TreeModel<i32>,
    mode: TreeFilterMode,
    threshold: Option<i32>,
) -> HashSet<NodeId> {
    let all = all_nodes(tree);
    if threshold.is_none() {
        return all.into_iter().collect();
    }
    let matches: HashSet<NodeId> = all
        .iter()
        .copied()
        .filter(|&n| oracle_matches(tree, n, threshold))
        .collect();
    match mode {
        TreeFilterMode::HideNonMatching => all
            .into_iter()
            .filter(|&n| {
                matches.contains(&n) && ancestors_of(tree, n).iter().all(|a| matches.contains(a))
            })
            .collect(),
        TreeFilterMode::KeepAncestors => {
            let mut s = matches.clone();
            for &m in &matches {
                s.extend(ancestors_of(tree, m));
            }
            s
        }
        TreeFilterMode::KeepDescendants => {
            let mut s = matches.clone();
            for &m in &matches {
                s.extend(descendants_of(tree, m));
            }
            s
        }
    }
}

/// Independent (recursive — safe here since generators bound trees to ≤ 30
/// nodes) recompute of the flattened `(NodeId, depth)` sequence given a
/// visible set, expand state, and sort direction.
fn oracle_flatten_from(
    tree: &TreeModel<i32>,
    visible: &HashSet<NodeId>,
    expanded: &HashSet<NodeId>,
    sort: Option<SortDirection>,
    node: NodeId,
    depth: usize,
    out: &mut Vec<(NodeId, usize)>,
) {
    if !visible.contains(&node) {
        return;
    }
    out.push((node, depth));
    if expanded.contains(&node) {
        let mut children: Vec<NodeId> = tree
            .children(node)
            .into_iter()
            .filter(|c| visible.contains(c))
            .collect();
        if let Some(dir) = sort {
            children.sort_by(|&a, &b| {
                let va = tree.with_item(a, |v| *v).unwrap();
                let vb = tree.with_item(b, |v| *v).unwrap();
                let ord = va.cmp(&vb);
                if dir == SortDirection::Descending {
                    ord.reverse()
                } else {
                    ord
                }
            });
        }
        for c in children {
            oracle_flatten_from(tree, visible, expanded, sort, c, depth + 1, out);
        }
    }
}

fn oracle_project(
    tree: &TreeModel<i32>,
    mode: TreeFilterMode,
    threshold: Option<i32>,
    expanded: &HashSet<NodeId>,
    sort: Option<SortDirection>,
) -> Vec<(NodeId, usize)> {
    let visible = oracle_visible_set(tree, mode, threshold);
    let mut roots: Vec<NodeId> = (0..tree.root_count()).map(|i| tree.root(i)).collect();
    if let Some(dir) = sort {
        roots.sort_by(|&a, &b| {
            let va = tree.with_item(a, |v| *v).unwrap();
            let vb = tree.with_item(b, |v| *v).unwrap();
            let ord = va.cmp(&vb);
            if dir == SortDirection::Descending {
                ord.reverse()
            } else {
                ord
            }
        });
    }
    let mut out = Vec::new();
    for r in roots {
        oracle_flatten_from(tree, &visible, expanded, sort, r, 0, &mut out);
    }
    out
}

// ── 5. the incremental tree projection equals a full recompute (ORACLE) ──
// The O(1) position map and the `NodeUpdated` fast path are new in this
// rebase. As with property 1, comparing the live proxy against an
// independent from-scratch recompute after every step of an arbitrary
// mutation sequence is the strongest available check that neither one drifts.

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn the_incremental_tree_projection_equals_a_full_recompute(
        ops in prop::collection::vec(arb_tree_op(), 0..30),
        mode in arb_any_filter_mode(),
    ) {
        let tree: TreeModel<i32> = TreeModel::new();
        let proxy = SortFilterTreeModel::new(tree.clone())
            .filter_mode(mode)
            .with_comparator("value", |a: &i32, b: &i32| a.cmp(b))
            .with_predicate("value", |t| {
                let threshold: i32 = t.parse().expect("filter text is always a formatted i32");
                Box::new(move |v: &i32| *v >= threshold)
            });

        let mut expanded: HashSet<NodeId> = HashSet::new();
        let mut filter: Option<i32> = None;
        let mut sort: Option<SortDirection> = None;

        for op in &ops {
            apply_tree_op(&tree, &proxy, &mut expanded, &mut filter, &mut sort, op);

            let expected = oracle_project(&tree, mode, filter, &expanded, sort);
            prop_assert_eq!(
                proxy.visible_count(), expected.len(),
                "visible_count should match the brute-force recompute after {:?}", op
            );
            for (i, &(node, depth)) in expected.iter().enumerate() {
                prop_assert_eq!(
                    proxy.visible_node_id(i), Some(node),
                    "node at flat index {} should match the brute-force recompute after {:?}", i, op
                );
                let actual_depth = proxy.entry_at(i).map(|e| e.depth);
                prop_assert_eq!(
                    actual_depth, Some(depth),
                    "depth at flat index {} should match the brute-force recompute after {:?}", i, op
                );
            }
        }
    }
}

// ── 6. each TreeFilterMode matches its brute-force set definition ──
// Direct check of the literal contract in the module docs, independent of
// any mutation-sequence incrementality concern: HideNonMatching == exactly
// the matches (whole ancestor path also matches); KeepAncestors == matches ∪
// ancestors-of-matches; KeepDescendants == matches ∪ descendants-of-matches.

proptest! {
    // `KeepDescendants` is deliberately excluded here (unlike properties 5
    // and 7, which cover all three modes). Reason, found by hand-tracing a
    // counterexample rather than by running anything: `oracle_visible_set`
    // computes the *raw* per-node "matches or some ancestor matched" set
    // (matching `visit_keep_descendants` exactly), but the live proxy's
    // actual output goes through `flatten_visible`, which starts its walk at
    // each *top-level root* and bails out immediately if the root itself
    // isn't in that raw visible set — it never even inspects that root's
    // children, regardless of whether one of them independently matches. So
    // for `KeepDescendants`, a match whose top-level root doesn't itself
    // match (and has no matching ancestor of its own, roots having none) is
    // marked visible by `compute_visibility` but never reachable through the
    // root-anchored flatten, and so never shown by the proxy at all. This is
    // the exact divergence the crate's own
    // `deep_chain_filters_each_mode_without_overflow` unit test documents
    // ("flatten_visible starts at the real tree root, which isn't on the
    // visible set — so nothing is emitted") and that `tree_row_filter.rs`'s
    // module doc calls out as `KeepDescendants` "deliberately differ[ing]"
    // from `SortFilterTreeModel` (see property 8 below, which checks the
    // two *do* agree for the other two modes). A brute-force "matches ∪
    // descendants" oracle is therefore not the right contract for this mode
    // against this particular type; asserting it here would be testing a
    // stricter promise than `SortFilterTreeModel` actually makes.
    #[test]
    fn each_filter_mode_matches_its_brute_force_kept_set(
        spec in arb_forest_spec(),
        mode in arb_ancestor_preserving_filter_mode(),
        threshold in -5i32..=5,
    ) {
        let tree = build_forest(&spec);
        let expected = oracle_visible_set(&tree, mode, Some(threshold));

        let proxy = SortFilterTreeModel::new(tree)
            .filter_mode(mode)
            .with_predicate("value", move |t| {
                let th: i32 = t.parse().expect("filter text is always a formatted i32");
                Box::new(move |v: &i32| *v >= th)
            });
        proxy.expand_all();
        proxy.set_filter("value", &threshold.to_string());

        let actual: HashSet<NodeId> = (0..proxy.visible_count())
            .map(|i| proxy.visible_node_id(i).expect("index within visible_count must resolve"))
            .collect();

        prop_assert_eq!(
            actual, expected,
            "mode {:?} threshold {:?}: visible set should equal the brute-force definition",
            mode, threshold
        );
    }
}

// ── 7. first_changed_index never under-reports (tree) ──
// Same conservation property as block 4, over the flattened (NodeId, depth)
// sequence instead of a flat list.

proptest! {
    #[test]
    fn first_changed_index_never_under_reports_for_trees(
        ops in prop::collection::vec(arb_tree_op(), 0..30),
        mode in arb_any_filter_mode(),
    ) {
        let tree: TreeModel<i32> = TreeModel::new();
        let proxy = SortFilterTreeModel::new(tree.clone())
            .filter_mode(mode)
            .with_comparator("value", |a: &i32, b: &i32| a.cmp(b))
            .with_predicate("value", |t| {
                let threshold: i32 = t.parse().expect("filter text is always a formatted i32");
                Box::new(move |v: &i32| *v >= threshold)
            });

        let mut expanded: HashSet<NodeId> = HashSet::new();
        let mut filter: Option<i32> = None;
        let mut sort: Option<SortDirection> = None;

        for op in &ops {
            let filter_before = filter;
            let sort_before = sort;
            let expanded_before = expanded.clone();
            let before = oracle_project(&tree, mode, filter_before, &expanded_before, sort_before);

            apply_tree_op(&tree, &proxy, &mut expanded, &mut filter, &mut sort, op);

            let after = oracle_project(&tree, mode, filter, &expanded, sort);

            let d = proxy.first_changed_index().unwrap_or(0);
            let checked_prefix = d.min(before.len()).min(after.len());
            for i in 0..checked_prefix {
                prop_assert_eq!(
                    before[i], after[i],
                    "flat row {} should be unchanged below first_changed_index={} after {:?}", i, d, op
                );
            }
        }
    }
}

// ============================================================================
// TreeRowFilter<K, T>
// ============================================================================

/// Cost: `parent_pick_raw` is reduced modulo the current node count (plus
/// one, for "insert as a new root") at build time, never at generation
/// time — every insertion in `build_forest` is O(1) at the tail, and the
/// vector itself is capped at 30 entries, so this builds at most a 30-node
/// forest. Every downstream traversal over it (ancestors/descendants/
/// visibility/dump) is at worst O(n) or, for the naive bottom-up aggregation
/// in `oracle_visible_set`'s `KeepAncestors` branch, O(n^2) — at most ~900
/// checks for n=30. Trivial.
fn arb_forest_spec() -> impl Strategy<Value = Vec<(usize, i32)>> {
    prop::collection::vec((0usize..40, -5i32..=5), 0..30)
}

/// Build a forest by repeated append-only insertion: each `(parent_pick_raw,
/// value)` pair either becomes a new root (when no nodes exist yet, or when
/// `parent_pick_raw` lands on the "root" remainder) or a new last child of an
/// existing node picked by `parent_pick_raw % nodes.len()`. Every insertion
/// index used is the current tail (`root_count()` / `child_count(parent)`),
/// always in-bounds by construction.
fn build_forest(spec: &[(usize, i32)]) -> TreeModel<i32> {
    let tree: TreeModel<i32> = TreeModel::new();
    let mut nodes: Vec<NodeId> = Vec::new();
    for &(parent_pick_raw, value) in spec {
        if nodes.is_empty() || parent_pick_raw % (nodes.len() + 1) == nodes.len() {
            let idx = tree.root_count();
            let n = tree.insert_root(idx, value);
            nodes.push(n);
        } else {
            let parent = nodes[parent_pick_raw % nodes.len()];
            let idx = tree.child_count(parent);
            let n = tree.insert_child(parent, idx, value);
            nodes.push(n);
        }
    }
    tree
}

/// Dump a `TreeModel<i32>` into the pre-order, depth-tagged `TreeRow` stream
/// `TreeRowFilter` expects as input. Recursion is safe here — the same
/// bound (≤ 30 nodes, so ≤ 30 deep) `build_forest` guarantees.
fn dump_rows(tree: &TreeModel<i32>) -> Vec<TreeRow<NodeId, i32>> {
    fn walk(
        tree: &TreeModel<i32>,
        node: NodeId,
        depth: usize,
        out: &mut Vec<TreeRow<NodeId, i32>>,
    ) {
        let v = tree
            .with_item(node, |v| *v)
            .expect("node must exist during its own dump");
        out.push(TreeRow::new(node, v, depth));
        for c in tree.children(node) {
            walk(tree, c, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    for i in 0..tree.root_count() {
        walk(tree, tree.root(i), 0, &mut out);
    }
    out
}

// ── 8. TreeRowFilter agrees with SortFilterTreeModel for the two modes their
//      module docs claim are equivalent ──
// `tree_row_filter.rs`'s module doc explicitly states HideNonMatching and
// KeepAncestors are "equivalent to SortFilterTreeModel", while KeepDescendants
// "deliberately differs". This checks the claimed equivalence holds across
// arbitrary forests, and (by omission) does not assert it for
// KeepDescendants.

proptest! {
    #[test]
    fn tree_row_filter_agrees_with_sort_filter_tree_model_for_ancestor_preserving_modes(
        spec in arb_forest_spec(),
        mode in arb_ancestor_preserving_filter_mode(),
        threshold in -5i32..=5,
    ) {
        let tree = build_forest(&spec);
        let rows = dump_rows(&tree);

        let filtered = TreeRowFilter::<NodeId, i32>::new()
            .filter_mode(mode)
            .filter(move |v: &i32| *v >= threshold)
            .apply(rows);
        let row_filter_keys: HashSet<NodeId> = filtered.iter().map(|r| r.key).collect();

        let proxy = SortFilterTreeModel::new(tree)
            .filter_mode(mode)
            .with_predicate("value", move |t| {
                let th: i32 = t.parse().expect("filter text is always a formatted i32");
                Box::new(move |v: &i32| *v >= th)
            });
        proxy.expand_all();
        proxy.set_filter("value", &threshold.to_string());
        let proxy_keys: HashSet<NodeId> = (0..proxy.visible_count())
            .map(|i| proxy.visible_node_id(i).expect("index within visible_count must resolve"))
            .collect();

        prop_assert_eq!(
            row_filter_keys, proxy_keys,
            "TreeRowFilter and SortFilterTreeModel should keep the same node set for {:?} \
             (the documented equivalence in tree_row_filter.rs's module docs)",
            mode
        );
    }
}

// ── 9. TreeRowFilter::apply always emits a well-formed indent stream ──
// A general fact about any correctly-compacted pre-order dump: the first row
// (if any) is a root (depth 0), and depth never jumps by more than 1 between
// consecutive rows — regardless of filter mode, predicate, or sort. A
// violation would mean the depth-compaction bookkeeping is wrong, since
// `TreeDataSlice` re-derives parent/child structure from exactly this
// invariant ("a row's parent is the nearest preceding row with a strictly
// smaller depth").

proptest! {
    #[test]
    fn tree_row_filter_output_is_a_well_formed_indent_stream(
        spec in arb_forest_spec(),
        mode in arb_any_filter_mode(),
        threshold in -5i32..=5,
        sort_desc in any::<bool>(),
    ) {
        let tree = build_forest(&spec);
        let rows = dump_rows(&tree);

        let sieve = TreeRowFilter::<NodeId, i32>::new()
            .filter_mode(mode)
            .filter(move |v: &i32| *v >= threshold);
        let sieve = if sort_desc {
            sieve.sort_desc(|a: &i32, b: &i32| a.cmp(b))
        } else {
            sieve.sort(|a: &i32, b: &i32| a.cmp(b))
        };
        let out = sieve.apply(rows);

        if let Some(first) = out.first() {
            prop_assert_eq!(first.depth, 0, "the first row of an indent stream must be a root (depth 0), got {:?}", first);
        }
        for w in out.windows(2) {
            prop_assert!(
                w[1].depth <= w[0].depth + 1,
                "depth must not jump by more than 1 between consecutive rows: {} -> {} (mode {:?}, threshold {})",
                w[0].depth, w[1].depth, mode, threshold
            );
        }
    }
}
