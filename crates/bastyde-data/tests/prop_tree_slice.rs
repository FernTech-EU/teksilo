// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Property tests for `TreeSlice<T>` (crates/bastyde-data/src/tree_slice.rs)
//! and the `TreeModel<T>` it projects (crates/bastyde-data/src/tree_model.rs).
//!
//! `TreeSlice` is the built-in per-view flattened projection behind
//! `TreeView`/`TreeTableView`: it maintains an independent expand/collapse
//! set over a shared `TreeModel`, re-flattens on every `TreeChange`, and
//! publishes a `first_changed_index()` divergence hint that row-height
//! caches trust to keep a valid prefix across a reflatten. Two headline
//! design claims live here — "two slices over one model have fully
//! independent expand state" and "`first_changed_index` never
//! under-reports" — plus the `TreeDataSource::reorder_within` default
//! (drag-reorder across a whole multi-selection), which is the most
//! plausible source of a real bug in this area: an earlier, unfinished
//! investigation suspected it either creates a cycle or fails to reject a
//! drop into its own dragged subtree.
//!
//! Every generator here builds a tree via a bounded sequence of "insert one
//! node, parented to an already-existing node chosen by index modulo the
//! current count" steps, so a strategy of length `n` always yields exactly
//! `n` nodes with no possibility of an invalid parent/index — this also
//! means tree size is capped directly by the `Vec` length bound proptest
//! shrinks over, with no risk of the multiplicative blow-up a
//! depth-times-branching generator could produce. See the cost comment on
//! each `arb_*` function for the worst case it can produce.
//!
//! Why proptest rather than a fixed example table: `rebuild_flat_list`'s
//! zip-based prefix diff and `reorder_within`'s subtree-descendant filter
//! are exactly the kind of index/ownership arithmetic that is easy to get
//! right for the worked examples in `mod tests` below and wrong for an
//! unanticipated tree shape — proptest's shrinking turns a failure here
//! into a minimal repro instead of a 40-node stack trace. Override the
//! iteration count with `PROPTEST_CASES=N cargo test -p bastyde-data --test
//! prop_tree_slice`. Sibling suites in this directory
//! (`prop_list_and_selection.rs`, `prop_sort_filter.rs`,
//! `prop_tree_checked.rs`) cover the flat-list, sort/filter-projection, and
//! checkbox-aggregation data models respectively; this file is the
//! hierarchical-projection counterpart. Generators are hand-written locally
//! (no `prop_compose!`/`Arbitrary`), matching the `../text-typeset` /
//! `../text-document` house style.

use std::collections::HashSet;

use bastyde_data::{DropPosition, FlatEntry, NodeId, TreeDataSource, TreeModel, TreeSlice};
use proptest::prelude::*;

// ── Shared generators and helpers ───────────────────────────────────────

/// A single insert step: `None` (or any selector drawn before any node
/// exists) inserts a root; `Some(sel)` inserts a child of an
/// already-existing node, chosen by `sel % (nodes inserted so far)`. `Some`
/// is weighted 4:1 over `None` so most generated shapes are a connected
/// tree with the occasional extra root (mirroring the `sample_tree` helper
/// in `tree_slice.rs`'s own unit tests: A / B / C at the root, subtrees
/// underneath).
fn arb_parent_sel(max_nodes: usize) -> impl Strategy<Value = Option<u16>> {
    prop_oneof![
        1 => Just(None),
        4 => (0u16..max_nodes as u16).prop_map(Some),
    ]
}

/// A bounded sequence of insert steps. Each step inserts exactly one node,
/// so a `Vec` of length `n` (`1..=max_nodes`) always yields a tree of
/// exactly `n` nodes — `max_nodes` is therefore the actual worst-case node
/// count, not a multiplicative bound: building it is `O(max_nodes)`
/// `insert_root`/`insert_child` calls, each `O(1)` plus an `O(children)`
/// vec insert at the tail (always an append, since the child index used is
/// always the current count). Every caller below passes `max_nodes` in
/// 16..=24, so the worst tree this ever builds is 24 nodes.
fn arb_insert_ops(max_nodes: usize) -> impl Strategy<Value = Vec<Option<u16>>> {
    prop::collection::vec(arb_parent_sel(max_nodes), 1..=max_nodes)
}

/// Build a `TreeModel<u32>` from `ops` (see `arb_insert_ops`), returning it
/// alongside the `NodeId` of each inserted node in insertion order (so
/// `nodes[i]` is exactly the node `ops[i]` created — a stable, index-based
/// handle proptest's shrinker can reason about without knowing any
/// `NodeId` value up front). Every node's payload is its insertion index,
/// purely so failure messages can show which generated step produced
/// which node.
fn build_tree(ops: &[Option<u16>]) -> (TreeModel<u32>, Vec<NodeId>) {
    let tree = TreeModel::new();
    let mut nodes: Vec<NodeId> = Vec::with_capacity(ops.len());
    for (i, sel) in ops.iter().enumerate() {
        let payload = i as u32;
        let node = match sel {
            Some(s) if i > 0 => {
                let parent = nodes[(*s as usize) % i];
                let idx = tree.child_count(parent);
                tree.insert_child(parent, idx, payload)
            }
            _ => {
                let idx = tree.root_count();
                tree.insert_root(idx, payload)
            }
        };
        nodes.push(node);
    }
    (tree, nodes)
}

/// Independent re-implementation of "flatten the tree honouring an expand
/// set", written from the `FlatEntry` contract alone (depth 0 at the
/// roots, an expanded node's children spliced in immediately after it, in
/// source order) rather than by reading `TreeSlice`'s own (private)
/// `flatten_node` — this is the oracle for property 1.
fn brute_force_flatten<T>(tree: &TreeModel<T>, expanded: &HashSet<NodeId>) -> Vec<FlatEntry> {
    let mut out = Vec::new();
    let mut stack: Vec<(NodeId, usize)> = (0..tree.root_count())
        .rev()
        .map(|i| (tree.root(i), 0))
        .collect();
    while let Some((node, depth)) = stack.pop() {
        let has_children = tree.has_children(node);
        let is_expanded = expanded.contains(&node);
        out.push(FlatEntry {
            node_id: node,
            depth,
            has_children,
            is_expanded,
        });
        if is_expanded && has_children {
            for child in tree.children(node).into_iter().rev() {
                stack.push((child, depth + 1));
            }
        }
    }
    out
}

/// Walk `node`'s ancestor chain up to the root (inclusive of `node`
/// itself). Returns `None` if a node is revisited before the walk
/// terminates naturally — i.e. an actual cycle, which a correct tree can
/// never contain. `cap` bounds the walk so a genuine cycle is *detected*
/// (as `None`) instead of looped over forever, since an infinite loop here
/// is exactly the failure mode this suite is hunting for in
/// `reorder_within`.
fn ancestor_chain<T>(tree: &TreeModel<T>, node: NodeId, cap: usize) -> Option<Vec<NodeId>> {
    let mut seen = HashSet::new();
    let mut chain = Vec::new();
    let mut cur = Some(node);
    while let Some(n) = cur {
        if !seen.insert(n) || chain.len() > cap {
            return None;
        }
        chain.push(n);
        cur = tree.parent(n);
    }
    Some(chain)
}

/// All nodes reachable from the tree's roots — a fresh top-down DFS,
/// written independently of any private tree-walk in the crate.
fn reachable_nodes<T>(tree: &TreeModel<T>) -> HashSet<NodeId> {
    let mut out = HashSet::new();
    let mut stack: Vec<NodeId> = (0..tree.root_count()).map(|i| tree.root(i)).collect();
    while let Some(n) = stack.pop() {
        if out.insert(n) {
            stack.extend(tree.children(n));
        }
    }
    out
}

fn snapshot(slice: &TreeSlice<u32>) -> Vec<FlatEntry> {
    (0..slice.visible_count())
        .map(|i| slice.entry_at(i).unwrap())
        .collect()
}

/// Like `snapshot`, but also captures each visible row's payload — needed
/// wherever a mutation might be a content-only `TreeModel::update` (which
/// leaves every `FlatEntry` byte-for-byte identical; only the row's data
/// changed).
fn snapshot_with_payload(slice: &TreeSlice<u32>) -> Vec<(FlatEntry, u32)> {
    (0..slice.visible_count())
        .map(|i| {
            slice
                .with_entry(i, |item, entry| (entry.clone(), *item))
                .unwrap()
        })
        .collect()
}

// ── 1. flattened rows match a brute-force DFS honouring the expand set ──
// The strongest property in this file: the ORACLE for "what should
// visible_count()/entry_at() return at all", independent of
// `TreeSlice::flatten_node`. Also folds in the O(1) position-map check
// (`flat_index_of` must agree with iteration order) since it falls out of
// the same generated state for free.

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn flattened_rows_match_a_brute_force_dfs_honouring_the_expand_set(
        (ops, expand_flags) in arb_insert_ops(24).prop_flat_map(|ops| {
            let n = ops.len();
            (Just(ops), prop::collection::vec(any::<bool>(), n))
        })
    ) {
        let (tree, nodes) = build_tree(&ops);
        let slice = TreeSlice::new(tree.clone());
        let mut expanded: HashSet<NodeId> = HashSet::new();
        for (&node, &flag) in nodes.iter().zip(expand_flags.iter()) {
            if flag {
                slice.expand(node);
                expanded.insert(node);
            }
        }

        let expected = brute_force_flatten(&tree, &expanded);
        let actual = snapshot(&slice);
        prop_assert_eq!(
            &actual, &expected,
            "flattened rows diverged from the brute-force DFS oracle for {} nodes with expand flags {:?}",
            nodes.len(), expand_flags
        );

        for (i, entry) in actual.iter().enumerate() {
            prop_assert_eq!(
                slice.flat_index_of(entry.node_id), Some(i),
                "flat_index_of({:?}) should be {} to match the flattened list's own iteration order",
                entry.node_id, i
            );
        }
    }
}

// ── 2. expand then collapse the same node restores the exact previous
//      flattening ──
// A round-trip check under a NON-trivial ambient state: `pre_flags`
// expands an arbitrary subset of other nodes first, and the target node is
// explicitly collapsed before the baseline snapshot is taken, so the
// round trip is exercised against real sibling/ancestor state rather than
// trivially returning to a freshly-constructed (all-collapsed) slice.

proptest! {
    #[test]
    fn expanding_then_collapsing_the_same_node_restores_the_previous_flattening(
        (ops, pre_flags, node_sel) in arb_insert_ops(24).prop_flat_map(|ops| {
            let n = ops.len();
            (Just(ops), prop::collection::vec(any::<bool>(), n), 0..n as u16)
        })
    ) {
        let (tree, nodes) = build_tree(&ops);
        let slice = TreeSlice::new(tree.clone());
        for (&node, &flag) in nodes.iter().zip(pre_flags.iter()) {
            if flag {
                slice.expand(node);
            }
        }

        let node = nodes[node_sel as usize];
        // Force a known starting state for `node` itself regardless of what
        // `pre_flags` did to it, so the toggle below is a genuine
        // collapsed -> expanded -> collapsed round trip.
        slice.collapse(node);
        let baseline = snapshot(&slice);

        slice.expand(node);
        slice.collapse(node);
        let after = snapshot(&slice);

        prop_assert_eq!(
            &after, &baseline,
            "expand({:?}) then collapse({:?}) should restore the exact previous flattening",
            node, node
        );
    }
}

// ── 3. expanding an already-expanded node is a no-op ──
// `TreeSlice::expand` early-returns on `!exp.insert(node)` without calling
// `reflatten_and_notify` — so a second `expand()` of the same node must
// leave the flattened rows, the version signal, AND the divergence hint
// completely untouched.

proptest! {
    #[test]
    fn expanding_an_already_expanded_node_is_a_complete_no_op(
        (ops, node_sel) in arb_insert_ops(24).prop_flat_map(|ops| {
            let n = ops.len();
            (Just(ops), 0..n as u16)
        })
    ) {
        let (tree, nodes) = build_tree(&ops);
        let slice = TreeSlice::new(tree.clone());
        let node = nodes[node_sel as usize];

        slice.expand(node);
        let snapshot_before = snapshot(&slice);
        let version_before = slice.version_signal().get();
        let divergence_before = slice.first_changed_index();

        slice.expand(node); // already expanded

        prop_assert_eq!(
            snapshot(&slice), snapshot_before,
            "re-expanding an already-expanded node ({:?}) changed the flattened rows",
            node
        );
        prop_assert_eq!(
            slice.version_signal().get(), version_before,
            "re-expanding an already-expanded node ({:?}) bumped the version signal", node
        );
        prop_assert_eq!(
            slice.first_changed_index(), divergence_before,
            "re-expanding an already-expanded node ({:?}) changed first_changed_index", node
        );
    }
}

// ── 4. every FlatEntry's depth equals its node's true ancestor count ──
// Checked against `TreeModel::depth` (a parent-pointer walk), which is a
// different code path than `flatten_node`'s depth-while-descending
// tracking — so this is a real cross-check between two independent ways
// of computing the same quantity, not a restatement of either.

proptest! {
    #[test]
    fn every_flat_entrys_depth_equals_its_ancestor_count_in_the_model(
        (ops, expand_flags) in arb_insert_ops(24).prop_flat_map(|ops| {
            let n = ops.len();
            (Just(ops), prop::collection::vec(any::<bool>(), n))
        })
    ) {
        let (tree, nodes) = build_tree(&ops);
        let slice = TreeSlice::new(tree.clone());
        for (&node, &flag) in nodes.iter().zip(expand_flags.iter()) {
            if flag {
                slice.expand(node);
            }
        }

        for i in 0..slice.visible_count() {
            let entry = slice.entry_at(i).unwrap();
            let ancestor_count = tree.depth(entry.node_id);
            prop_assert_eq!(
                entry.depth, ancestor_count,
                "FlatEntry depth {} != tree.depth() {} for node {:?} at flat index {}",
                entry.depth, ancestor_count, entry.node_id, i
            );
        }
    }
}

// ── 5. two TreeSlices over one TreeModel never perturb each other's
//      expand state ──
// The headline "per-view independent expand state" design claim
// (module doc of tree_slice.rs). Rather than a static two-step check, this
// interleaves toggles on two slices sharing one model and re-derives each
// slice's OWN brute-force oracle from ONLY that slice's own toggle
// history after every step — so if a `second`-slice toggle ever leaked
// into `first`'s flattening, `first`'s check against its untouched
// `expected_first` set would fail on the very next step.

#[derive(Debug, Clone, Copy)]
enum SliceOp {
    ToggleFirst(u16),
    ToggleSecond(u16),
}

fn arb_slice_op() -> impl Strategy<Value = SliceOp> {
    prop_oneof![
        any::<u16>().prop_map(SliceOp::ToggleFirst),
        any::<u16>().prop_map(SliceOp::ToggleSecond),
    ]
}

proptest! {
    #[test]
    fn two_slices_over_one_model_never_perturb_each_others_expand_state(
        (ops, slice_ops) in arb_insert_ops(24).prop_flat_map(|ops| {
            (Just(ops), prop::collection::vec(arb_slice_op(), 0..24))
        })
    ) {
        let (tree, nodes) = build_tree(&ops);
        let first = TreeSlice::new(tree.clone());
        let second = TreeSlice::new(tree.clone());
        let mut expected_first: HashSet<NodeId> = HashSet::new();
        let mut expected_second: HashSet<NodeId> = HashSet::new();

        for op in &slice_ops {
            match *op {
                SliceOp::ToggleFirst(sel) => {
                    let node = nodes[(sel as usize) % nodes.len()];
                    first.toggle(node);
                    if !expected_first.remove(&node) {
                        expected_first.insert(node);
                    }
                }
                SliceOp::ToggleSecond(sel) => {
                    let node = nodes[(sel as usize) % nodes.len()];
                    second.toggle(node);
                    if !expected_second.remove(&node) {
                        expected_second.insert(node);
                    }
                }
            }

            let expected_flat_first = brute_force_flatten(&tree, &expected_first);
            prop_assert_eq!(
                snapshot(&first), expected_flat_first,
                "slice `first` diverged from its OWN toggle history after op {:?} (full history so far: {:?}) — a `second`-slice mutation must never leak into `first`",
                op, slice_ops
            );

            let expected_flat_second = brute_force_flatten(&tree, &expected_second);
            prop_assert_eq!(
                snapshot(&second), expected_flat_second,
                "slice `second` diverged from its OWN toggle history after op {:?} (full history so far: {:?}) — a `first`-slice mutation must never leak into `second`",
                op, slice_ops
            );
        }
    }
}

// ── 6. first_changed_index never under-reports ──
// Row-height caches trust that rows `0..first_changed_index()` are
// byte-for-byte (AND payload-for-payload — see `snapshot_with_payload`)
// identical to the previous flattening. Over-reporting is merely wasteful;
// under-reporting corrupts a cache. Driven as an operation sequence
// (insert/remove/update/expand/collapse), re-checked after every step —
// the shape this file's sibling `bastyde-scene` suite uses for its
// insert/remove state-machine property. Cost: the starting tree is
// <=20 nodes and at most 20 more `Insert*` steps can grow it further, so
// the tree never exceeds 40 nodes; each step reflattens that tree once,
// so the whole sequence is O(20 * 40) — trivial.

#[derive(Debug, Clone)]
enum Mutation {
    InsertChild { parent_sel: u16 },
    InsertRoot,
    Remove(u16),
    Update(u16),
    Expand(u16),
    Collapse(u16),
}

fn arb_mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        3 => any::<u16>().prop_map(|s| Mutation::InsertChild { parent_sel: s }),
        1 => Just(Mutation::InsertRoot),
        2 => any::<u16>().prop_map(Mutation::Remove),
        2 => any::<u16>().prop_map(Mutation::Update),
        2 => any::<u16>().prop_map(Mutation::Expand),
        2 => any::<u16>().prop_map(Mutation::Collapse),
    ]
}

/// Every node in `node`'s subtree (inclusive) computed BEFORE removal, so
/// the caller can prune exactly the ids that `TreeModel::remove` is about
/// to free from a `live`-node bookkeeping list.
fn subtree_ids<T>(tree: &TreeModel<T>, node: NodeId) -> HashSet<NodeId> {
    let mut out = HashSet::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        out.insert(n);
        stack.extend(tree.children(n));
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn first_changed_index_never_under_reports(
        (ops, mutations) in arb_insert_ops(20).prop_flat_map(|ops| {
            (Just(ops), prop::collection::vec(arb_mutation(), 0..20))
        })
    ) {
        let (tree, initial_nodes) = build_tree(&ops);
        let slice = TreeSlice::new(tree.clone());
        let mut live: Vec<NodeId> = initial_nodes;
        let mut next_payload: u32 = live.len() as u32;
        let mut prev = snapshot_with_payload(&slice);

        for m in &mutations {
            match m {
                Mutation::InsertChild { parent_sel } => {
                    let node = if live.is_empty() {
                        tree.insert_root(tree.root_count(), next_payload)
                    } else {
                        let parent = live[(*parent_sel as usize) % live.len()];
                        let idx = tree.child_count(parent);
                        tree.insert_child(parent, idx, next_payload)
                    };
                    next_payload += 1;
                    live.push(node);
                }
                Mutation::InsertRoot => {
                    let node = tree.insert_root(tree.root_count(), next_payload);
                    next_payload += 1;
                    live.push(node);
                }
                Mutation::Remove(sel) => {
                    if live.is_empty() {
                        continue;
                    }
                    let node = live[(*sel as usize) % live.len()];
                    let doomed = subtree_ids(&tree, node);
                    tree.remove(node);
                    live.retain(|n| !doomed.contains(n));
                }
                Mutation::Update(sel) => {
                    if live.is_empty() {
                        continue;
                    }
                    let node = live[(*sel as usize) % live.len()];
                    tree.update(node, next_payload);
                    next_payload += 1;
                }
                Mutation::Expand(sel) => {
                    if live.is_empty() {
                        continue;
                    }
                    let node = live[(*sel as usize) % live.len()];
                    slice.expand(node);
                }
                Mutation::Collapse(sel) => {
                    if live.is_empty() {
                        continue;
                    }
                    let node = live[(*sel as usize) % live.len()];
                    slice.collapse(node);
                }
            }

            let idx = slice.first_changed_index().unwrap_or(0);
            let now = snapshot_with_payload(&slice);
            let checked_len = idx.min(prev.len()).min(now.len());
            for i in 0..checked_len {
                prop_assert_eq!(
                    &prev[i], &now[i],
                    "first_changed_index() returned {} (claiming rows 0..{} are unchanged) but row {} differs after {:?} (ops so far: {:?}): {:?} -> {:?}",
                    idx, idx, i, m, mutations, prev[i], now[i]
                );
            }
            prev = now;
        }
    }
}

// ── 7. reorder_within never creates a cycle, over the reference tree from
//      the original investigation ──
// An earlier, never-completed investigation built an exhaustive check over
// this exact 8-node tree (A/A1/A1a/A2/B/B1/B2/C), chasing the hypothesis
// that `reorder_within` either creates a cycle or fails to reject a drop
// into its own dragged subtree. This is a plain #[test] (not
// proptest-driven) because the domain is already small and fully
// enumerable: every ordered source-subset of size 1..=3 from 8 nodes
// (8 + 8*7 + 8*7*6 = 400 sequences) times every target (8) times every
// `DropPosition` (3) = 9600 fixed, deterministic cases — cheap (a fresh
// 8-node tree rebuilt per case) and with no shrinking/explosion risk since
// nothing here is generated.

/// The 8-node reference tree: A / A1 / A1a / A2 / B / B1 / B2 / C. Returns
/// the tree plus its `NodeId`s in a FIXED insertion order (0=A, 1=A1,
/// 2=A1a, 3=A2, 4=B, 5=B1, 6=B2, 7=C) so every fresh rebuild produces a
/// node at the same logical position, even though the underlying slotmap
/// keys differ across rebuilds.
fn build_reference_tree() -> (TreeModel<&'static str>, Vec<NodeId>) {
    let tree = TreeModel::new();
    let a = tree.insert_root(0, "A");
    let a1 = tree.insert_child(a, 0, "A1");
    let a1a = tree.insert_child(a1, 0, "A1a");
    let a2 = tree.insert_child(a, 1, "A2");
    let b = tree.insert_root(1, "B");
    let b1 = tree.insert_child(b, 0, "B1");
    let b2 = tree.insert_child(b, 1, "B2");
    let c = tree.insert_root(2, "C");
    (tree, vec![a, a1, a1a, a2, b, b1, b2, c])
}

/// All (non-empty) sequences of length 1..=k drawn without repetition from
/// `items`, order-sensitive ("ordered subsets" per the original
/// investigation, since `reorder_within` treats `sources` as an ordered
/// drag selection whose later entries anchor off earlier ones). Recursion
/// depth is capped at `k` (<=3 in this file), and the domain (`items.len()
/// == 8`) is fixed and tiny, so this is safe despite the recursive helper.
fn permutations_up_to_len(items: &[NodeId], k: usize) -> Vec<Vec<NodeId>> {
    fn go(
        items: &[NodeId],
        k: usize,
        used: &mut Vec<bool>,
        chosen: &mut Vec<NodeId>,
        out: &mut Vec<Vec<NodeId>>,
    ) {
        if !chosen.is_empty() {
            out.push(chosen.clone());
        }
        if chosen.len() == k {
            return;
        }
        for i in 0..items.len() {
            if used[i] {
                continue;
            }
            used[i] = true;
            chosen.push(items[i]);
            go(items, k, used, chosen, out);
            chosen.pop();
            used[i] = false;
        }
    }
    let mut out = Vec::new();
    let mut used = vec![false; items.len()];
    let mut chosen = Vec::new();
    go(items, k, &mut used, &mut chosen, &mut out);
    out
}

#[test]
fn reorder_within_never_creates_a_cycle_over_the_reference_tree() {
    let (_reference, reference_nodes) = build_reference_tree();
    let idx_of = |n: NodeId| reference_nodes.iter().position(|&r| r == n).unwrap();
    let sources_candidates = permutations_up_to_len(&reference_nodes, 3);
    let positions = [
        DropPosition::Before,
        DropPosition::After,
        DropPosition::Into,
    ];
    let mut checked = 0usize;

    for sources_template in &sources_candidates {
        for &target_template in &reference_nodes {
            for &position in &positions {
                // Fresh tree every case — reorder_within mutates in place,
                // and slotmap keys differ across rebuilds, so remap the
                // fixed logical positions into THIS tree's ids.
                let (tree, nodes) = build_reference_tree();
                let sources: Vec<NodeId> =
                    sources_template.iter().map(|&s| nodes[idx_of(s)]).collect();
                let target = nodes[idx_of(target_template)];

                let target_chain = ancestor_chain(&tree, target, nodes.len())
                    .expect("a freshly built reference tree must be acyclic");
                let cycle_risk = sources.iter().any(|s| target_chain.contains(s));

                let slice = TreeSlice::new(tree.clone());
                let accepted = slice.reorder_within(&sources, &target, position);

                if cycle_risk {
                    assert!(
                        !accepted,
                        "reorder_within accepted dropping {sources:?} onto/inside itself: target {target:?} position {position:?}"
                    );
                }

                for &node in &nodes {
                    assert!(
                        ancestor_chain(&tree, node, nodes.len()).is_some(),
                        "cycle reachable from {node:?} after reorder_within(sources={sources:?}, target={target:?}, position={position:?}, accepted={accepted})"
                    );
                }
                let after = reachable_nodes(&tree);
                let before: HashSet<NodeId> = nodes.iter().copied().collect();
                assert_eq!(
                    after, before,
                    "node set changed after reorder_within(sources={sources:?}, target={target:?}, position={position:?}, accepted={accepted})"
                );

                checked += 1;
            }
        }
    }

    assert_eq!(
        checked,
        sources_candidates.len() * reference_nodes.len() * positions.len(),
        "exhaustive enumeration should have covered every (sources, target, position) combination exactly once"
    );
}

// ── 8. reorder_within stays acyclic and conserves the node set, over
//      arbitrary trees ──
// The randomized generalization of property 7: same acyclic + conserved
// node-set invariants, but over arbitrary generated shapes (<=16 nodes)
// and arbitrary 1..=3-node source selections (including duplicate
// selectors — a degenerate but legal "drag the same row twice" case),
// instead of only the fixed 8-node reference tree.

fn arb_reorder_case(
    max_nodes: usize,
) -> impl Strategy<Value = (Vec<Option<u16>>, Vec<u16>, u16, u8)> {
    arb_insert_ops(max_nodes).prop_flat_map(|ops| {
        (
            Just(ops),
            prop::collection::vec(any::<u16>(), 1..=3),
            any::<u16>(),
            0u8..3,
        )
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn reorder_within_stays_acyclic_and_conserves_the_node_set(
        (ops, source_sels, target_sel, position_sel) in arb_reorder_case(16)
    ) {
        let (tree, nodes) = build_tree(&ops);
        let n = nodes.len();
        let sources: Vec<NodeId> = source_sels.iter().map(|&s| nodes[(s as usize) % n]).collect();
        let target = nodes[(target_sel as usize) % n];
        let position = match position_sel {
            0 => DropPosition::Before,
            1 => DropPosition::After,
            _ => DropPosition::Into,
        };

        let target_chain = ancestor_chain(&tree, target, n)
            .expect("a freshly built tree must be acyclic before any mutation");
        let cycle_risk = sources.iter().any(|s| target_chain.contains(s));

        let slice = TreeSlice::new(tree.clone());
        let accepted = slice.reorder_within(&sources, &target, position);

        if cycle_risk {
            prop_assert!(
                !accepted,
                "reorder_within accepted dropping {:?} onto/inside itself: target {:?} position {:?}",
                sources, target, position
            );
        }

        for &node in &nodes {
            prop_assert!(
                ancestor_chain(&tree, node, n).is_some(),
                "cycle reachable from {:?} after reorder_within(sources={:?}, target={:?}, position={:?}, accepted={})",
                node, sources, target, position, accepted
            );
        }
        let after = reachable_nodes(&tree);
        let before: HashSet<NodeId> = nodes.iter().copied().collect();
        prop_assert_eq!(
            after, before,
            "node set changed after reorder_within(sources={:?}, target={:?}, position={:?}, accepted={})",
            sources, target, position, accepted
        );
    }
}
