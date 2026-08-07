// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Property tests for the tri-state checkbox family: `CheckedModel` (flat),
//! `TreeCheckedModel<T>` (a `TreeModel<T>`-keyed tree), and
//! `KeyedTreeCheckedModel<K>` (the domain-key-keyed twin over a
//! `TreeDataSlice`/`TreeDataSource`) — crates/teksilo-data/src/checked_model.rs,
//! tree_checked_model.rs, keyed_tree_checked_model.rs, check_state.rs.
//!
//! The descendant→ancestor tristate aggregation
//! (`AggregateMode::DescendantsDriveAncestors`) has a crisp, checkable spec —
//! for every node, `Checked` iff every leaf descendant is checked, `Unchecked`
//! iff none are, `Indeterminate` otherwise — which makes it an excellent
//! oracle target: several properties below re-derive that aggregate from
//! scratch by walking the tree and compare it against the model's own
//! `check_state`, rather than re-implementing the model's own recompute logic.
//!
//! Contracts asserted here:
//! - The headline oracle (properties 1 and 5): after ANY sequence of
//!   check/uncheck/leaf-toggle operations, every node's/key's state matches
//!   the brute-force leaf aggregate — checked independently for the
//!   `NodeId`-keyed `TreeCheckedModel` and the domain-keyed
//!   `KeyedTreeCheckedModel`, which hand-duplicate the same cascade logic in
//!   two separate files (per the module docs: "Semantics, cascade behaviour...
//!   are identical to `TreeCheckedModel`") and so are exactly the kind of
//!   code that drifts apart under an isolated fix.
//! - Property 2 is a targeted regression for `03f2db0c` ("fix(data): scope
//!   cascade suppression per-node"), which this branch picked up during its
//!   rebase onto `main`: an app observer that reacts to one node's cascade by
//!   checking a structurally-unrelated node must still get that node's own
//!   full cascade, even while the first node's cascade is still unwinding —
//!   the exact shape the old model-wide suppression flag got wrong.
//! - Property 3: toggling the same LEAF twice is a genuine involution
//!   (`toggle`'s two-state leaf cycle, not the three-state branch cycle) and
//!   restores every tracked node's state exactly — a real round-trip, unlike
//!   "check(X) then uncheck(X)" in general (see the property's own comment
//!   for the counter-example that rules that phrasing out).
//! - Property 4: the `Signal<bool>` bridge (`bool_signal_for`) stays in sync
//!   with the tristate signal for every leaf, however the write arrives —
//!   direct bool writes, direct tristate check/uncheck/toggle, or an
//!   ancestor's cascade.
//! - Properties 6 and 7: `reaggregate()` is idempotent — a no-op when the
//!   model is already consistent, and a true fixed point (a second call
//!   changes nothing) even from a deliberately-forced-inconsistent state.
//! - Property 8: `prune_missing` drops exactly the keys its `exists`
//!   predicate rejects and leaves every surviving key correctly re-aggregated
//!   against the NEW (post-prune) tree shape — verifying both halves of its
//!   doc comment's claim.
//! - Property 9: the crate's stated reason `KeyedTreeCheckedModel` exists at
//!   all — checked state survives a re-source with the same domain keys but
//!   a completely different tree shape, immediately (before any
//!   reaggregate) and correctly (after one).
//! - Property 10: `CheckedModel`'s index-shifting (`adjust_for_insert/
//!   remove/move`) reproduces an independent "logical row identity" oracle
//!   after arbitrary interleaved check/uncheck/insert/remove/move sequences.
//!
//! Why proptest rather than more hand-written examples: every `mod tests`
//! block in the three source files already covers individual operations in
//! isolation (a single check, a single cascade, a single prune). What they
//! do not cover is arbitrary *sequences* — which is exactly where an
//! aggregation bug, an idempotence violation, or a reentrancy regression like
//! `03f2db0c` lives. Mirrors the proptest convention used in the sibling
//! `../text-typeset` / `../text-document` repos and this crate's own
//! `prop_list_and_selection.rs` / `prop_sort_filter.rs` / `prop_tree_slice.rs`:
//! integration test for a public target, one property per `proptest! {}`
//! block, hand-written local `arb_*` generators (no `prop_compose!`/
//! `Arbitrary`, no shared generator module — per-file duplication is the
//! accepted convention here). Override the per-block default of 256 cases
//! with `PROPTEST_CASES=N cargo test -p teksilo-data --test prop_tree_checked`.

use std::collections::{HashMap, HashSet};

use proptest::prelude::*;
use teksilo_core::Signal;
use teksilo_data::{
    CheckState, CheckedModel, KeyedTreeCheckedModel, NodeId, TreeCheckedModel, TreeDataSlice,
    TreeModel, TreeRow,
};

// ── Shared tree-building plumbing (local to this file per house style) ──────

/// A single insert step: `None` (or any selector drawn before any node
/// exists) inserts a root; `Some(sel)` inserts a child of an already-existing
/// node, chosen by `sel % (nodes inserted so far)`. `Some` is weighted 4:1
/// over `None` so most generated shapes are one connected tree with the
/// occasional extra root — mirrors `prop_tree_slice.rs`'s `arb_parent_sel`.
fn arb_parent_sel(max_nodes: usize) -> impl Strategy<Value = Option<u16>> {
    prop_oneof![
        1 => Just(None),
        4 => (0u16..max_nodes as u16).prop_map(Some),
    ]
}

/// A bounded sequence of insert steps. A `Vec` of length `n` (`1..=max_nodes`)
/// always yields a tree of exactly `n` nodes — `max_nodes` is therefore the
/// actual worst-case node count, not a multiplicative bound: `O(max_nodes)`
/// `insert_root`/`insert_child` calls, each `O(1)` plus an `O(children)` vec
/// insert at the tail (always an append, since the child index used is always
/// the current sibling count).
fn arb_insert_ops(max_nodes: usize) -> impl Strategy<Value = Vec<Option<u16>>> {
    prop::collection::vec(arb_parent_sel(max_nodes), 1..=max_nodes)
}

/// Insert `ops` into a FRESH `TreeModel<()>`, returning it alongside the
/// `NodeId` of each inserted node in insertion order (so `ids[i]` is exactly
/// the node `ops[i]` created — a stable, index-based handle proptest's
/// shrinker can reason about without knowing any `NodeId` value up front).
fn build_tree(ops: &[Option<u16>]) -> (TreeModel<()>, Vec<NodeId>) {
    let tree = TreeModel::new();
    let ids = append_tree(&tree, ops);
    (tree, ids)
}

/// Insert `ops` as ADDITIONAL nodes into an already-existing `tree` (used
/// only by the reentrancy property, which needs two structurally disjoint
/// groups sharing one `TreeCheckedModel`/`TreeModel`). Selector indices in
/// `ops` are local to this batch: `ops[i]`'s selector picks among
/// `ids[0..i]`, the nodes this SAME call has already inserted, never an
/// earlier batch's nodes — so two calls on the same tree always produce two
/// disjoint forests.
fn append_tree(tree: &TreeModel<()>, ops: &[Option<u16>]) -> Vec<NodeId> {
    let mut ids: Vec<NodeId> = Vec::with_capacity(ops.len());
    for (i, sel) in ops.iter().enumerate() {
        let node = match sel {
            Some(s) if i > 0 => {
                let parent = ids[(*s as usize) % i];
                let idx = tree.child_count(parent);
                tree.insert_child(parent, idx, ())
            }
            _ => {
                let idx = tree.root_count();
                tree.insert_root(idx, ())
            }
        };
        ids.push(node);
    }
    ids
}

/// Convert a `TreeModel<()>` (as built by `build_tree`) into the flat,
/// depth-ordered `TreeRow` stream `TreeDataSlice` expects: true pre-order (a
/// node immediately followed by its whole subtree) with `depth` equal to the
/// node's actual structural depth. Keys are the node's position in `ids` (its
/// insertion index) — the SAME index space `Op::Check`/`Uncheck`/
/// `ToggleLeaf` already reference for the `NodeId`-keyed properties, so the
/// identical `Op` sequence drives both the `TreeCheckedModel` and the
/// `KeyedTreeCheckedModel` properties without any translation at use sites.
fn preorder_rows(tree: &TreeModel<()>, ids: &[NodeId]) -> Vec<TreeRow<u64, ()>> {
    let key_of: HashMap<NodeId, u64> = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i as u64))
        .collect();
    let mut rows = Vec::with_capacity(ids.len());
    fn walk(
        tree: &TreeModel<()>,
        node: NodeId,
        depth: usize,
        key_of: &HashMap<NodeId, u64>,
        rows: &mut Vec<TreeRow<u64, ()>>,
    ) {
        rows.push(TreeRow::new(key_of[&node], (), depth));
        for child in tree.children(node) {
            walk(tree, child, depth + 1, key_of, rows);
        }
    }
    for i in 0..tree.root_count() {
        walk(tree, tree.root(i), 0, &key_of, &mut rows);
    }
    rows
}

/// Combine child states into the tristate aggregate: all-`Checked` →
/// `Checked`, all-`Unchecked` → `Unchecked`, otherwise `Indeterminate`.
/// Mirrors `recompute_from_children` in both `tree_checked_model.rs` and
/// `keyed_tree_checked_model.rs`. Precondition: `states` is non-empty (a
/// leaf never reaches this — it returns its own tracked value instead, see
/// `brute_force_tree_state` / `brute_force_keyed_state`).
fn combine_tristate(states: impl IntoIterator<Item = CheckState>) -> CheckState {
    let mut any_checked = false;
    let mut any_unchecked = false;
    for s in states {
        match s {
            CheckState::Checked => any_checked = true,
            CheckState::Unchecked => any_unchecked = true,
            CheckState::Indeterminate => {
                any_checked = true;
                any_unchecked = true;
            }
        }
    }
    match (any_checked, any_unchecked) {
        (true, false) => CheckState::Checked,
        (false, true) => CheckState::Unchecked,
        _ => CheckState::Indeterminate,
    }
}

/// The oracle for `TreeCheckedModel`: recursively re-derive `node`'s expected
/// state purely from the tree SHAPE and its leaves' own tracked values —
/// never from a branch's own stored `check_state` — so an aggregation bug at
/// any level is caught rather than silently reproduced.
fn brute_force_tree_state(
    tree: &TreeModel<()>,
    model: &TreeCheckedModel<()>,
    node: NodeId,
) -> CheckState {
    let children = tree.children(node);
    if children.is_empty() {
        return model.check_state(node);
    }
    combine_tristate(
        children
            .into_iter()
            .map(|c| brute_force_tree_state(tree, model, c)),
    )
}

/// The `KeyedTreeCheckedModel` counterpart of `brute_force_tree_state`, over
/// a `TreeDataSlice`'s live `child_keys_of` shape instead of `TreeModel::children`.
fn brute_force_keyed_state(
    slice: &TreeDataSlice<u64, ()>,
    model: &KeyedTreeCheckedModel<u64>,
    key: u64,
) -> CheckState {
    let children = slice.child_keys_of(&key);
    if children.is_empty() {
        return model.check_state(&key);
    }
    combine_tristate(
        children
            .into_iter()
            .map(|c| brute_force_keyed_state(slice, model, c)),
    )
}

/// A tree-mutation-free operation applied to an already-built forest: `Check`
/// / `Uncheck` target ANY node (branch or leaf) — matching `check`/`uncheck`,
/// which set `Checked`/`Unchecked` unconditionally regardless of node kind.
/// `ToggleLeaf` is applied only when the targeted node turns out to be a
/// leaf; branches silently skip it. This is deliberate, not a simplification:
/// `TreeCheckedModel::toggle` on a BRANCH under `DescendantsDriveAncestors`
/// falls into `current.next_tristate()` (the full three-state cycle), which
/// can set a branch directly to `Indeterminate` regardless of what its
/// children currently hold — a real, documented divergence from the
/// aggregation invariant (the user is allowed to force a tristate box to the
/// dash state), not a bug. Including it in the headline oracle below would
/// therefore falsify a property that IS true of every other operation this
/// suite exercises. Only a LEAF's toggle is a clean two-state cycle
/// (`Unchecked ↔ Checked`), so it belongs in the oracle.
#[derive(Debug, Clone, Copy)]
enum Op {
    Check(usize),
    Uncheck(usize),
    ToggleLeaf(usize),
}

/// Cost of the worst case this can produce: `n` is capped by the caller
/// (≤20 in every property below) and each `Op` does `O(n)` work inside the
/// model (a cascade touches at most every descendant, an ancestor-recompute
/// walks at most every ancestor) — so a 30-op sequence over a 20-node tree is
/// on the order of `20 * 30 = 600` signal writes, each followed by an
/// `O(n)`-ish (depth-bounded) brute-force re-verification. Trivial.
fn arb_op(n: usize) -> impl Strategy<Value = Op> {
    prop_oneof![
        (0..n).prop_map(Op::Check),
        (0..n).prop_map(Op::Uncheck),
        (0..n).prop_map(Op::ToggleLeaf),
    ]
}

/// Couple the op sequence's index range to the ACTUAL node count instead of
/// drawing two independent unbounded quantities: `ops` can only ever
/// reference a valid `0..tree_ops.len()` node index, so no field can ever
/// request work proportional to anything other than the tree's own
/// (separately-bounded) size.
fn arb_case(
    max_nodes: usize,
    max_ops: usize,
) -> impl Strategy<Value = (Vec<Option<u16>>, Vec<Op>)> {
    arb_insert_ops(max_nodes).prop_flat_map(move |tree_ops| {
        let n = tree_ops.len();
        prop::collection::vec(arb_op(n), 0..=max_ops).prop_map(move |ops| (tree_ops.clone(), ops))
    })
}

/// Apply one `Op` to `ids[i]` (a `NodeId`-keyed `TreeCheckedModel`).
fn apply_tree_op(tree: &TreeModel<()>, model: &TreeCheckedModel<()>, ids: &[NodeId], op: Op) {
    match op {
        Op::Check(i) => model.check(ids[i]),
        Op::Uncheck(i) => model.uncheck(ids[i]),
        Op::ToggleLeaf(i) => {
            if tree.children(ids[i]).is_empty() {
                model.toggle(ids[i]);
            }
        }
    }
}

/// Apply one `Op` to key `i as u64` (a domain-`u64`-keyed `KeyedTreeCheckedModel`).
fn apply_keyed_op(slice: &TreeDataSlice<u64, ()>, model: &KeyedTreeCheckedModel<u64>, op: Op) {
    match op {
        Op::Check(i) => model.check(i as u64),
        Op::Uncheck(i) => model.uncheck(i as u64),
        Op::ToggleLeaf(i) => {
            let key = i as u64;
            if slice.child_keys_of(&key).is_empty() {
                model.toggle(key);
            }
        }
    }
}

// ── 1. every node's state matches the brute-force leaf aggregate (ORACLE) ──
// The headline property: after any sequence of check/uncheck/leaf-toggle
// operations, every node's `check_state` matches an independent recomputation
// from the tree shape and leaf values alone (see `brute_force_tree_state`'s
// doc for why it never trusts a branch's own stored value).

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn every_node_state_matches_the_leaf_aggregate_after_any_check_uncheck_sequence(
        (tree_ops, ops) in arb_case(20, 30)
    ) {
        let (tree, ids) = build_tree(&tree_ops);
        let model = TreeCheckedModel::new(tree.clone());

        for op in &ops {
            apply_tree_op(&tree, &model, &ids, *op);
            for &id in &ids {
                let actual = model.check_state(id);
                let expected = brute_force_tree_state(&tree, &model, id);
                prop_assert_eq!(
                    actual, expected,
                    "node {:?} has state {:?} but the brute-force leaf aggregate says {:?} \
                     after op {:?} (tree_ops={:?}, full ops={:?})",
                    id, actual, expected, op, tree_ops, ops
                );
            }
        }
    }
}

// ── 2. cascade suppression is scoped per-node: a reentrant check from an
//      observer still recomputes its own ancestors (regression, 03f2db0c) ──
// `trigger` is always a leaf child of `root_a`, so checking `root_a` cascades
// a write INTO `trigger` via `write_state` (which suppresses `trigger`'s own
// id for the duration of that single write). An observer attached directly to
// `trigger`'s signal fires from inside that suppressed window and reentrantly
// checks `sentinel`, a leaf under the wholly disjoint `root_b` — before the
// fix, a single model-wide suppression flag (rather than one scoped to the
// specific node being written) silently swallowed `sentinel`'s own cascade,
// leaving `root_b` stuck stale. Uses a guaranteed root+children "star" shape
// rather than `build_tree`'s fully arbitrary one, specifically so this
// ancestor/descendant relationship exists in EVERY generated case, not just
// probabilistically.

fn append_star(tree: &TreeModel<()>, k: usize) -> Vec<NodeId> {
    let root = tree.insert_root(tree.root_count(), ());
    let mut ids = vec![root];
    for i in 0..k {
        ids.push(tree.insert_child(root, i, ()));
    }
    ids
}

/// Cost: both stars capped at `max_children` (8) leaves each, so the combined
/// tree never exceeds `2 * (1 + 8) = 18` nodes; `ops` is bounded to `max_ops`
/// (30) steps of `O(n)` work each — the same bound `arb_case` uses.
fn arb_reentrancy_case(
    max_children: usize,
    max_ops: usize,
) -> impl Strategy<Value = (usize, usize, Vec<Op>)> {
    (1..=max_children, 1..=max_children).prop_flat_map(move |(ka, kb)| {
        let n = (1 + ka) + (1 + kb);
        prop::collection::vec(arb_op(n), 0..=max_ops).prop_map(move |ops| (ka, kb, ops))
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn reentrant_check_from_an_observer_still_recomputes_its_own_ancestors(
        (ka, kb, ops) in arb_reentrancy_case(8, 30)
    ) {
        let tree = TreeModel::<()>::new();
        let ids_a = append_star(&tree, ka); // [root_a, child_0, ...]
        let ids_b = append_star(&tree, kb); // [root_b, child_0, ...], disjoint from group A
        let model = TreeCheckedModel::new(tree.clone());

        let trigger = ids_a[1];
        let sentinel = ids_b[1];

        let model_for_observer = model.clone();
        let _obs = model.signal_for(trigger).observe(move |state| {
            if *state == CheckState::Checked {
                model_for_observer.check(sentinel);
            }
        });

        let mut ids = ids_a.clone();
        ids.extend(ids_b.clone());

        for op in &ops {
            apply_tree_op(&tree, &model, &ids, *op);
            for &id in &ids {
                let actual = model.check_state(id);
                let expected = brute_force_tree_state(&tree, &model, id);
                prop_assert_eq!(
                    actual, expected,
                    "node {:?} has state {:?} but the brute-force leaf aggregate says {:?} \
                     after op {:?}, with a reentrant observer checking an unrelated node \
                     from inside trigger's own cascade (ka={}, kb={})",
                    id, actual, expected, op, ka, kb
                );
            }
        }
    }
}

// ── 3. toggling the same leaf twice restores every tracked node's state
//      (ROUND-TRIP) ──
// `toggle` on a LEAF under `DescendantsDriveAncestors` is a genuine two-state
// involution (`Unchecked <-> Checked` regardless of the leaf's current
// value — see `toggle`'s match arms), so toggling it twice restores that
// leaf's exact prior value; since nothing else was written in between, every
// ancestor recomputes to the exact same values it held before, and every
// unrelated node was never touched at all. NOTE this is deliberately narrower
// than "check(X) then uncheck(X) restores the tree": that phrasing is FALSE
// in general — e.g. root with children A=Checked, B=Unchecked (root
// Indeterminate); check(root) cascades both to Checked; uncheck(root)
// cascades both to Unchecked — the tree ends at {A=Unchecked, B=Unchecked,
// root=Unchecked}, not the original {A=Checked, B=Unchecked,
// root=Indeterminate}. Only the leaf-double-toggle phrasing holds
// unconditionally.

proptest! {
    #[test]
    fn toggling_a_leaf_twice_restores_every_nodes_state(
        (tree_ops, ops) in arb_case(20, 30)
    ) {
        let (tree, ids) = build_tree(&tree_ops);
        let model = TreeCheckedModel::new(tree.clone());

        for op in &ops {
            apply_tree_op(&tree, &model, &ids, *op);
        }

        // Every generated forest has at least one node, and every finite
        // forest has at least one leaf — this is never actually skipped, the
        // guard just avoids an `unwrap` panic in place of a graceful failure
        // if that invariant were somehow violated.
        let Some(&leaf) = ids.iter().find(|&&id| tree.children(id).is_empty()) else {
            return Ok(());
        };

        let before: Vec<CheckState> = ids.iter().map(|&id| model.check_state(id)).collect();
        model.toggle(leaf);
        model.toggle(leaf);
        let after: Vec<CheckState> = ids.iter().map(|&id| model.check_state(id)).collect();

        prop_assert_eq!(
            after, before,
            "toggling leaf {:?} twice must restore every node's state exactly \
             (tree_ops={:?}, ops={:?})",
            leaf, tree_ops, ops
        );
    }
}

// ── 4. the Signal<bool> bridge stays consistent with the tristate signal
//      for every leaf (INVARIANT) ──
// `bool_signal_for(leaf)` is documented as a writable two-state projection
// kept in sync with the tristate signal in both directions. Checked here
// against every write path that can touch a leaf: a direct bool write, a
// direct tristate check/uncheck/toggle on the leaf itself, AND an ancestor's
// check/uncheck cascading into it.

#[derive(Debug, Clone, Copy)]
enum BridgeOp {
    Check(usize),
    Uncheck(usize),
    ToggleLeaf(usize),
    SetBool(usize, bool),
}

/// Cost: identical shape to `arb_op`, plus one extra `bool` field — no
/// additional cost dimension.
fn arb_bridge_op(n: usize) -> impl Strategy<Value = BridgeOp> {
    prop_oneof![
        (0..n).prop_map(BridgeOp::Check),
        (0..n).prop_map(BridgeOp::Uncheck),
        (0..n).prop_map(BridgeOp::ToggleLeaf),
        (0..n, any::<bool>()).prop_map(|(i, b)| BridgeOp::SetBool(i, b)),
    ]
}

fn arb_bridge_case(
    max_nodes: usize,
    max_ops: usize,
) -> impl Strategy<Value = (Vec<Option<u16>>, Vec<BridgeOp>)> {
    arb_insert_ops(max_nodes).prop_flat_map(move |tree_ops| {
        let n = tree_ops.len();
        prop::collection::vec(arb_bridge_op(n), 0..=max_ops)
            .prop_map(move |ops| (tree_ops.clone(), ops))
    })
}

proptest! {
    #[test]
    fn bool_signal_bridge_matches_tristate_checked_for_every_leaf(
        (tree_ops, ops) in arb_bridge_case(20, 30)
    ) {
        let (tree, ids) = build_tree(&tree_ops);
        let model = TreeCheckedModel::new(tree.clone());

        // Wire the bridge for every leaf up front, keeping the returned
        // `Signal<bool>` handles so we can read them without re-deriving the
        // bridge each time.
        let leaf_bools: Vec<(NodeId, Signal<bool>)> = ids
            .iter()
            .copied()
            .filter(|&id| tree.children(id).is_empty())
            .map(|id| (id, model.bool_signal_for(id)))
            .collect();

        for op in &ops {
            match *op {
                BridgeOp::Check(i) => model.check(ids[i]),
                BridgeOp::Uncheck(i) => model.uncheck(ids[i]),
                BridgeOp::ToggleLeaf(i) => {
                    if tree.children(ids[i]).is_empty() {
                        model.toggle(ids[i]);
                    }
                }
                BridgeOp::SetBool(i, value) => {
                    if tree.children(ids[i]).is_empty() {
                        model.bool_signal_for(ids[i]).set(value);
                    }
                }
            }
            for (id, bool_sig) in &leaf_bools {
                let tri = model.check_state(*id);
                prop_assert_eq!(
                    bool_sig.get(), tri == CheckState::Checked,
                    "leaf {:?}: bool signal is {} but tristate is {:?} (expected bool == \
                     (tristate == Checked)) after op {:?} (tree_ops={:?})",
                    id, bool_sig.get(), tri, op, tree_ops
                );
            }
        }
    }
}

// ── 5. every key's state matches the brute-force leaf aggregate (ORACLE,
//      KeyedTreeCheckedModel) ──
// The `KeyedTreeCheckedModel` counterpart of property 1, over a
// `TreeDataSlice`-backed tree instead of a `TreeModel` — since the two models
// hand-duplicate the same cascade logic in separate files, this checks the
// SAME headline contract holds for the OTHER implementation.

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn every_key_state_matches_the_leaf_aggregate_after_any_check_uncheck_sequence_keyed(
        (tree_ops, ops) in arb_case(20, 30)
    ) {
        let (tree, ids) = build_tree(&tree_ops);
        let rows = preorder_rows(&tree, &ids);
        let slice = TreeDataSlice::from_rows(rows);
        let model = KeyedTreeCheckedModel::from_source(slice.clone());
        let n = ids.len();

        for op in &ops {
            apply_keyed_op(&slice, &model, *op);
            for i in 0..n {
                let key = i as u64;
                let actual = model.check_state(&key);
                let expected = brute_force_keyed_state(&slice, &model, key);
                prop_assert_eq!(
                    actual, expected,
                    "key {} has state {:?} but the brute-force leaf aggregate says {:?} \
                     after op {:?} (tree_ops={:?}, full ops={:?})",
                    key, actual, expected, op, tree_ops, ops
                );
            }
        }
    }
}

// ── 6. reaggregate() is a no-op when the model is already consistent
//      (IDEMPOTENCE) ──
// A check/uncheck/leaf-toggle-only sequence always leaves the model
// consistent (property 5), so `reaggregate()` — which recomputes every
// tracked branch key's aggregate from its CURRENT children — must not change
// anything.

proptest! {
    #[test]
    fn reaggregate_is_a_noop_when_the_model_is_already_consistent(
        (tree_ops, ops) in arb_case(20, 30)
    ) {
        let (tree, ids) = build_tree(&tree_ops);
        let rows = preorder_rows(&tree, &ids);
        let slice = TreeDataSlice::from_rows(rows);
        let model = KeyedTreeCheckedModel::from_source(slice.clone());
        let n = ids.len();

        for op in &ops {
            apply_keyed_op(&slice, &model, *op);
        }

        let before: Vec<CheckState> = (0..n).map(|i| model.check_state(&(i as u64))).collect();
        model.reaggregate();
        let after: Vec<CheckState> = (0..n).map(|i| model.check_state(&(i as u64))).collect();

        prop_assert_eq!(
            after, before,
            "reaggregate() must not change any key's state when the model is already \
             consistent (tree_ops={:?}, ops={:?})",
            tree_ops, ops
        );
    }
}

// ── 7. reaggregate() reaches a fixed point even from a forced-inconsistent
//      state (IDEMPOTENCE) ──
// Distinct from property 6 (which assumes the model is already consistent):
// here a handful of BRANCH keys are forced directly to an arbitrary tristate
// value via `signal_for(key).set(...)`, bypassing check/uncheck/toggle
// entirely (simulating stale/external data, or the documented "toggle a
// branch to Indeterminate" quirk) — so the model is not necessarily
// consistent going in. `reaggregate()` must still converge: a second call
// right after the first must be a true no-op.

proptest! {
    #[test]
    fn reaggregate_reaches_a_fixed_point_even_from_a_forced_inconsistent_state(
        (tree_ops, ops, force_indices) in arb_case(20, 30).prop_flat_map(|(tree_ops, ops)| {
            let n = tree_ops.len();
            prop::collection::vec(0..n, 0..=5).prop_map(move |idxs| (tree_ops.clone(), ops.clone(), idxs))
        })
    ) {
        let (tree, ids) = build_tree(&tree_ops);
        let rows = preorder_rows(&tree, &ids);
        let slice = TreeDataSlice::from_rows(rows);
        let model = KeyedTreeCheckedModel::from_source(slice.clone());
        let n = ids.len();

        for op in &ops {
            apply_keyed_op(&slice, &model, *op);
        }

        for i in force_indices {
            let key = i as u64;
            if !slice.child_keys_of(&key).is_empty() {
                model.signal_for(key).set(CheckState::Indeterminate);
            }
        }

        model.reaggregate();
        let after_one: Vec<CheckState> = (0..n).map(|i| model.check_state(&(i as u64))).collect();
        model.reaggregate();
        let after_two: Vec<CheckState> = (0..n).map(|i| model.check_state(&(i as u64))).collect();

        prop_assert_eq!(
            after_two, after_one,
            "a second reaggregate() must be a no-op once the first has converged \
             (tree_ops={:?}, ops={:?})",
            tree_ops, ops
        );
    }
}

// ── 8. prune_missing drops exactly the removed keys and reaggregates
//      surviving ancestors correctly (INVARIANT / ORACLE) ──
// Verifies both halves of `prune_missing`'s doc comment: removed keys read
// back as `Unchecked` (forgotten) and are absent from `checked_keys()`; every
// SURVIVING key matches the brute-force leaf aggregate over the NEW
// (post-prune) tree shape, not the stale pre-prune one.

proptest! {
    #[test]
    fn prune_missing_drops_removed_keys_and_reaggregates_surviving_ancestors(
        (tree_ops, ops, victim_idx) in arb_case(20, 30).prop_flat_map(|(tree_ops, ops)| {
            let n = tree_ops.len();
            (0..n).prop_map(move |v| (tree_ops.clone(), ops.clone(), v))
        })
    ) {
        let (tree, ids) = build_tree(&tree_ops);
        let rows = preorder_rows(&tree, &ids);
        let slice = TreeDataSlice::from_rows(rows.clone());
        let model = KeyedTreeCheckedModel::from_source(slice.clone());
        let n = ids.len();

        for op in &ops {
            apply_keyed_op(&slice, &model, *op);
        }

        // Remove the victim's *entire* subtree: filtering a contiguous
        // pre-order run out of `rows` keeps every SURVIVING row's raw depth
        // untouched, so the remaining structure re-derives identically to
        // the original minus the removed subtree (no accidental
        // re-parenting of orphaned children).
        let victim = victim_idx as u64;
        let mut removed: HashSet<u64> = HashSet::new();
        let mut stack = vec![victim];
        while let Some(k) = stack.pop() {
            if removed.insert(k) {
                stack.extend(slice.child_keys_of(&k));
            }
        }
        let new_rows: Vec<TreeRow<u64, ()>> = rows.into_iter().filter(|r| !removed.contains(&r.key)).collect();
        slice.set_rows(new_rows);

        model.prune_missing(|k| slice.contains_key(k));

        for i in 0..n {
            let key = i as u64;
            if removed.contains(&key) {
                prop_assert_eq!(
                    model.check_state(&key), CheckState::Unchecked,
                    "removed key {} must read back as Unchecked (forgotten) after prune_missing \
                     (victim={}, tree_ops={:?})",
                    key, victim, tree_ops
                );
                prop_assert!(
                    !model.checked_keys().contains(&key),
                    "removed key {} must not appear in checked_keys() after prune_missing",
                    key
                );
            } else {
                let actual = model.check_state(&key);
                let expected = brute_force_keyed_state(&slice, &model, key);
                prop_assert_eq!(
                    actual, expected,
                    "surviving key {} has state {:?} but the brute-force leaf aggregate over \
                     the POST-prune tree shape says {:?} (victim={}, tree_ops={:?})",
                    key, actual, expected, victim, tree_ops
                );
            }
        }
    }
}

// ── 9. checked state survives a reload with a different shape, then
//      resyncs once reaggregate() is called (METAMORPHIC) ──
// The crate's stated reason `KeyedTreeCheckedModel` exists at all (module
// doc: "a node's check state survives the tree reloading — a checked scene
// stays checked after the backend refreshes"). Re-sources over the SAME
// domain key set (0..n) but a completely independent second tree shape:
// immediately after `set_rows`, every surviving key's RAW check state must
// be byte-for-byte unchanged (nothing about `set_rows` touches the model's
// own state map); after an explicit `reaggregate()`, every key must match
// the brute-force aggregate over the NEW shape.

/// Exactly `n` insert steps (same key range `0..n` as the original tree),
/// for the "same keys, different shape" reload. Cost: `n` is already capped
/// at 20 by the caller (`arb_case`'s own bound).
fn arb_insert_ops_exact(n: usize) -> impl Strategy<Value = Vec<Option<u16>>> {
    prop::collection::vec(arb_parent_sel(n), n..=n)
}

proptest! {
    #[test]
    fn checked_state_survives_a_reload_with_a_different_shape_then_resyncs_on_reaggregate(
        (tree_ops, ops, tree_ops2) in arb_case(20, 30).prop_flat_map(|(tree_ops, ops)| {
            let n = tree_ops.len();
            arb_insert_ops_exact(n).prop_map(move |ops2| (tree_ops.clone(), ops.clone(), ops2))
        })
    ) {
        let (tree, ids) = build_tree(&tree_ops);
        let rows = preorder_rows(&tree, &ids);
        let slice = TreeDataSlice::from_rows(rows);
        let model = KeyedTreeCheckedModel::from_source(slice.clone());
        let n = ids.len();

        for op in &ops {
            apply_keyed_op(&slice, &model, *op);
        }

        let before: Vec<CheckState> = (0..n).map(|i| model.check_state(&(i as u64))).collect();

        // Re-source with the SAME domain keys (0..n) but a fresh,
        // independently-built tree shape — simulating a backend reload that
        // reorganises the outline. `set_rows` never touches the model's own
        // `state` map; only `prune_missing`/`reaggregate` read the tree again.
        let (tree2, ids2) = build_tree(&tree_ops2);
        let rows2 = preorder_rows(&tree2, &ids2);
        slice.set_rows(rows2);

        let after_reload: Vec<CheckState> = (0..n).map(|i| model.check_state(&(i as u64))).collect();
        prop_assert_eq!(
            after_reload, before.clone(),
            "raw check state for every key must be byte-for-byte unchanged immediately \
             after set_rows reshapes the source, before any reaggregate \
             (tree_ops={:?}, tree_ops2={:?}, ops={:?})",
            tree_ops, tree_ops2, ops
        );

        model.reaggregate();
        for i in 0..n {
            let key = i as u64;
            let actual = model.check_state(&key);
            let expected = brute_force_keyed_state(&slice, &model, key);
            prop_assert_eq!(
                actual, expected,
                "key {} must match the brute-force leaf aggregate over the NEW shape once \
                 reaggregate() is called after a reload (tree_ops2={:?})",
                key, tree_ops2
            );
        }
    }
}

// ── 10. checked_indices() matches a logical-identity model after arbitrary
//       interleaved check/uncheck/insert/remove/move (ORACLE / CONSERVATION,
//       CheckedModel) ──
// `CheckedModel` is flat (no aggregation) — its own headline contract is
// that `adjust_for_insert`/`adjust_for_remove`/`adjust_for_move` keep checked
// rows attached to their LOGICAL identity across index shifts, not their
// numeric index. An independent oracle tracks each row's identity as a
// unique `u64` alongside a plain `Vec` (mirroring exactly what a real
// `ListModel` reorder does), and after every op, translates that oracle's
// current checked-identity set back into positions and compares against
// `checked_indices()`.

#[derive(Debug, Clone, Copy)]
enum FlatOp {
    Check(usize),
    Uncheck(usize),
    Insert(usize, usize),
    Remove(usize, usize),
    Move(usize, usize, usize),
}

/// Cost: every position field is a raw `usize` in `0..1000`, reduced modulo
/// the model's CURRENT logical length at apply time (the "couple at apply
/// time" pattern `prop_list_and_selection.rs`'s `apply_list_op` already
/// establishes for a stateful, length-drifting index — there is no way to
/// `prop_flat_map` against a length that only exists mid-replay). `count`
/// fields are bounded directly to `1..=4` at generation time (they don't
/// depend on anything that changes at runtime), so a whole 30-op sequence
/// starting from at most 15 rows never grows the logical row count past
/// roughly `15 + 30*4 = 135` — trivial.
fn arb_flat_op() -> impl Strategy<Value = FlatOp> {
    prop_oneof![
        (0usize..1000).prop_map(FlatOp::Check),
        (0usize..1000).prop_map(FlatOp::Uncheck),
        (0usize..1000, 1usize..=4).prop_map(|(p, c)| FlatOp::Insert(p, c)),
        (0usize..1000, 1usize..=4).prop_map(|(p, c)| FlatOp::Remove(p, c)),
        (0usize..1000, 0usize..1000, 1usize..=4).prop_map(|(f, t, c)| FlatOp::Move(f, t, c)),
    ]
}

fn arb_flat_case() -> impl Strategy<Value = (usize, Vec<FlatOp>)> {
    (0usize..=15, prop::collection::vec(arb_flat_op(), 0..=30))
}

proptest! {
    #[test]
    fn checked_indices_matches_a_logical_identity_model_after_arbitrary_shifts(
        (initial_len, ops) in arb_flat_case()
    ) {
        let m = CheckedModel::new();
        let mut logical: Vec<u64> = (0..initial_len as u64).collect();
        let mut next_id: u64 = initial_len as u64;
        let mut checked_ids: HashSet<u64> = HashSet::new();

        for op in &ops {
            match *op {
                FlatOp::Check(raw) => {
                    if !logical.is_empty() {
                        let i = raw % logical.len();
                        m.check(i);
                        checked_ids.insert(logical[i]);
                    }
                }
                FlatOp::Uncheck(raw) => {
                    if !logical.is_empty() {
                        let i = raw % logical.len();
                        m.uncheck(i);
                        checked_ids.remove(&logical[i]);
                    }
                }
                FlatOp::Insert(raw_at, count) => {
                    let at = raw_at % (logical.len() + 1);
                    m.adjust_for_insert(at, count);
                    let new_ids: Vec<u64> = (0..count as u64).map(|k| next_id + k).collect();
                    next_id += count as u64;
                    logical.splice(at..at, new_ids);
                }
                FlatOp::Remove(raw_at, raw_count) => {
                    if !logical.is_empty() {
                        let at = raw_at % logical.len();
                        let count = raw_count.min(logical.len() - at);
                        m.adjust_for_remove(at, count);
                        for id in logical.drain(at..at + count) {
                            checked_ids.remove(&id);
                        }
                    }
                }
                FlatOp::Move(raw_from, raw_to, raw_count) => {
                    if !logical.is_empty() {
                        let from = raw_from % logical.len();
                        let count = raw_count.min(logical.len() - from);
                        if count > 0 {
                            // `to` is a POST-REMOVAL index into the
                            // remaining `logical.len() - count` slots,
                            // matching `CheckedModel::adjust_for_move`'s
                            // documented contract (mirrors `ListModel::move_item`).
                            let remaining = logical.len() - count;
                            let to = raw_to % (remaining + 1);
                            m.adjust_for_move(from, to, count);
                            let block: Vec<u64> = logical.drain(from..from + count).collect();
                            for (k, id) in block.into_iter().enumerate() {
                                logical.insert(to + k, id);
                            }
                        }
                    }
                }
            }

            let expected: Vec<usize> = (0..logical.len())
                .filter(|&i| checked_ids.contains(&logical[i]))
                .collect();
            prop_assert_eq!(
                m.checked_indices(), expected,
                "checked_indices() diverged from the logical-identity model after op {:?} \
                 (initial_len={}, ops={:?})",
                op, initial_len, ops
            );
        }
    }
}
