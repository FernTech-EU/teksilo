//! Property tests for `ListModel<T>` (crates/bastyde-data/src/list_model.rs)
//! and `SelectionModel` (crates/bastyde-data/src/selection_model.rs) — and,
//! critically, the two wired together the way production code wires them:
//! `bastyde-widgets/src/data_views.rs`'s `RowSelection::from_index` feeds
//! every `ListModel::observe_changes` notification into
//! `SelectionModel::adjust_for_insert` / `adjust_for_remove` /
//! `adjust_for_move` / `clear`. That wiring is reproduced verbatim in the
//! `selection_indices_never_dangle_past_the_end_of_the_list` property below.
//!
//! Contracts asserted here:
//! - `move_items` (the contiguous block-move primitive behind multi-row
//!   drag-drop reorder) conserves the exact multiset of items, lands the
//!   moved block as a single contiguous run in its original relative order,
//!   and is a true no-op — unchanged list, zero `DataChange` emitted — when
//!   the destination is where the block already sits.
//! - After an arbitrary interleaved sequence of `ListModel` mutations and
//!   `SelectionModel` selections (`Single` and `Multi` mode both), no
//!   selected index is ever `>= len()` — the index-drift bug class
//!   `adjust_for_*` exists to prevent.
//! - `SelectionMode::Single` never holds more than one selected index across
//!   an arbitrary sequence of selection operations.
//! - Shift+click range extension (`extend_to`) is symmetric: selecting an
//!   anchor then extending to a target selects the same set as the reverse.
//! - `select_all` fully replaces (never unions with) the previous selection,
//!   and `select_all` followed by `clear` always returns to empty.
//! - `reconcile_by_key` (the peer-reload / merge-without-losing-selection
//!   primitive) reaches exactly the requested end state and never falls
//!   back to `DataChange::Reset`, generalizing the hand-picked before/after
//!   scenarios already in `list_model.rs`'s own `mod tests` to arbitrary
//!   unique-keyed row sets.
//! - The strongest property here: replaying the sequence of `DataChange`
//!   events a plain `ListModel` op sequence emits — reconstructing each
//!   event's payload from the model's *post-mutation* state the way a real
//!   observer (`ListView`, `TableView`) is documented to be able to
//!   (`notify` always runs after the internal borrow is dropped) — against
//!   an independent, naively-maintained `Vec<i32>` reproduces the model's
//!   actual final contents exactly. This is the only property here that
//!   checks the *notifications* are complete and correct, not just each
//!   mutator's direct effect on the model itself.
//!
//! Why proptest rather than more hand-written examples: `list_model.rs` and
//! `selection_model.rs` already have thorough example-based coverage for
//! single operations and their individual `DataChange` payloads; what they
//! do not cover is arbitrary *sequences* of operations interacting — which
//! is exactly where index-drift and stale-anchor bugs live, and exactly what
//! `RowSelection::from_index`'s glue exists to prevent in production. Mirrors
//! the proptest convention used in the sibling `../text-typeset` /
//! `../text-document` repos and this worktree's own
//! `crates/bastyde-tokens/tests/prop_color.rs` / `crates/bastyde-scene/src/
//! index.rs`'s `mod proptests`: integration test for a public target, one
//! property per `proptest! {}` block, hand-written local `arb_*` generators
//! (no shared generator module — per-file duplication is the accepted
//! convention here). Override the per-block default of 256 cases with
//! `PROPTEST_CASES=N cargo test -p bastyde-data --test prop_list_and_selection`.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_data::{DataChange, ListModel, SelectionMode, SelectionModel};
use proptest::prelude::*;

// ─────────────────────────── shared test plumbing ───────────────────────────
// (Local to this file per house style — not factored into a shared module.)

/// Read the full contents of a `ListModel<i32>` into a plain `Vec`, mirroring
/// the `snapshot`/`order` helpers already in `list_model.rs`'s own `mod
/// tests`.
fn snapshot(model: &ListModel<i32>) -> Vec<i32> {
    (0..model.len())
        .map(|i| model.with_item(i, |v| *v).unwrap())
        .collect()
}

/// A single `ListModel` mutation with independently-generated raw fields.
/// Index/gap fields are plain bounded `usize`s, not yet reduced against any
/// particular list length — see `apply_list_op`, which folds them against
/// the model's *current* length at interpretation time. That is the "couple
/// dependent inputs" pattern applied to a stateful op sequence: the model's
/// length is a moving target across the sequence, so it cannot be baked into
/// the generator up front the way `arb_move_spec` below bakes a fixed `len`
/// into its `idx`/`gap` fields via one `prop_flat_map`.
#[derive(Debug, Clone)]
enum ListOp {
    Push(i32),
    Insert(usize, i32),
    Remove(usize),
    Set(usize, i32),
    MoveItem(usize, usize),
    MoveItems(Vec<usize>, usize),
    ReplaceAll(Vec<i32>),
    Clear,
}

/// Cost of the most expensive `ListOp`: `MoveItems` carries a `Vec<usize>`
/// capped at 8 entries and `ReplaceAll` a `Vec<i32>` capped at 10 — both
/// trivial allocations. Every index/gap field is a raw `usize` in `0..64`,
/// reduced modulo the model's live length before use (`apply_list_op`), so
/// no field can ever request work proportional to anything other than the
/// list's own (separately-bounded, see `arb_ops`) size.
fn arb_list_op() -> impl Strategy<Value = ListOp> {
    prop_oneof![
        any::<i32>().prop_map(ListOp::Push),
        (0usize..64, any::<i32>()).prop_map(|(i, v)| ListOp::Insert(i, v)),
        (0usize..64).prop_map(ListOp::Remove),
        (0usize..64, any::<i32>()).prop_map(|(i, v)| ListOp::Set(i, v)),
        (0usize..64, 0usize..64).prop_map(|(f, t)| ListOp::MoveItem(f, t)),
        (prop::collection::vec(0usize..64, 0..8), 0usize..64)
            .prop_map(|(idx, gap)| ListOp::MoveItems(idx, gap)),
        prop::collection::vec(any::<i32>(), 0..10).prop_map(ListOp::ReplaceAll),
        Just(ListOp::Clear),
    ]
}

/// Apply one `ListOp` to `model`, reducing any raw index/gap field modulo
/// the model's CURRENT length so every call is in-bounds (the mutators are
/// documented to panic out of bounds). Guards every `% len` against `len ==
/// 0` (division by zero), which also means "no-op on an empty list" for
/// `Remove`/`Set`/`MoveItem`/`MoveItems` — exactly the behaviour a random op
/// sequence needs when it happens to hit an empty list mid-sequence.
fn apply_list_op(model: &ListModel<i32>, op: &ListOp) {
    let len = model.len();
    match op {
        ListOp::Push(v) => model.push(*v),
        ListOp::Insert(i, v) => model.insert(i % (len + 1), *v),
        ListOp::Remove(i) => {
            if len > 0 {
                model.remove(i % len);
            }
        }
        ListOp::Set(i, v) => {
            if len > 0 {
                model.set(i % len, *v);
            }
        }
        ListOp::MoveItem(f, t) => {
            if len > 0 {
                model.move_item(f % len, t % len);
            }
        }
        ListOp::MoveItems(raw, gap) => {
            if len > 0 {
                let idx: Vec<usize> = raw.iter().map(|i| i % len).collect();
                model.move_items(&idx, gap % (len + 1));
            }
        }
        ListOp::ReplaceAll(items) => model.replace_all(items.clone()),
        ListOp::Clear => model.clear(),
    }
}

/// A `ListModel` op interleaved with a `SelectionModel` op, for the combined
/// list+selection stateful properties.
#[derive(Debug, Clone)]
enum Op {
    List(ListOp),
    Select(usize),
    Toggle(usize),
    ExtendTo(usize),
    SelectAll,
    SelectIndices(Vec<usize>, bool),
    ClearSelection,
}

/// Cost: `Op::List` inherits `arb_list_op`'s bound; `SelectIndices` carries a
/// `Vec<usize>` capped at 8 raw entries. Every selection-facing index field
/// is likewise reduced modulo the model's live length in `apply_op` (and
/// skipped entirely when the model is empty), so a selection op can only
/// ever select an index that is genuinely `< len()` at the moment it runs —
/// which is what makes "does a LATER structural change let that index drift
/// out of range" the property actually worth checking, rather than a
/// property refuted trivially by seeding garbage indices up front.
fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        arb_list_op().prop_map(Op::List),
        (0usize..64).prop_map(Op::Select),
        (0usize..64).prop_map(Op::Toggle),
        (0usize..64).prop_map(Op::ExtendTo),
        Just(Op::SelectAll),
        (prop::collection::vec(0usize..64, 0..8), any::<bool>())
            .prop_map(|(idx, additive)| Op::SelectIndices(idx, additive)),
        Just(Op::ClearSelection),
    ]
}

/// Op sequences capped at 30 steps, per the house cost-discipline rule
/// (small op counts, not large ones, find bugs). Combined with `arb_op`'s
/// own O(1)-ish per-step cost, the worst case for a whole sequence is on the
/// order of 30 `Vec<i32>` mutations over a list that itself never grows
/// past roughly 30-40 items (each step adds at most one element via `Push`/
/// `Insert`, or resets to at most 10 via `ReplaceAll`) — negligible.
fn arb_ops() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(arb_op(), 0..30)
}

fn apply_op(model: &ListModel<i32>, sel: &SelectionModel, op: &Op) {
    match op {
        Op::List(list_op) => apply_list_op(model, list_op),
        Op::Select(i) => {
            let len = model.len();
            if len > 0 {
                sel.select(i % len);
            }
        }
        Op::Toggle(i) => {
            let len = model.len();
            if len > 0 {
                sel.toggle(i % len);
            }
        }
        Op::ExtendTo(i) => {
            let len = model.len();
            if len > 0 {
                sel.extend_to(i % len);
            }
        }
        Op::SelectAll => sel.select_all(model.len()),
        Op::SelectIndices(raw, additive) => {
            let len = model.len();
            if len > 0 {
                let idx: Vec<usize> = raw.iter().map(|i| i % len).collect();
                sel.select_indices(idx, *additive);
            }
        }
        Op::ClearSelection => sel.clear(),
    }
}

/// A block-move spec: `len` (list size), `idx` (candidate indices to move —
/// deliberately allowed to range up to `len` inclusive, one past the last
/// valid index, to also exercise `move_items`'s documented "out-of-range
/// entries are ignored" filtering), and `gap` (destination, `0..=len`). All
/// three are derived from one `prop_flat_map` over `len` rather than drawn
/// independently, so no generated triple can ever describe an index outside
/// what `move_items` itself is documented to accept.
///
/// Cost: `len` capped at 20, `idx` capped at 8 entries — one `move_items`
/// call over a <=20-element `Vec<i32>`, negligible.
fn arb_move_spec() -> impl Strategy<Value = (usize, Vec<usize>, usize)> {
    (0usize..20).prop_flat_map(|len| {
        let idx_strategy = prop::collection::vec(0..=len, 0..=8usize.min(len + 1));
        let gap_strategy = 0..=len;
        (Just(len), idx_strategy, gap_strategy)
    })
}

/// A block-move spec guaranteed to be a true no-op: `count` contiguous items
/// starting at `i0` (which always fits inside `0..len` by construction,
/// since `i0` is drawn from `0..=(len - count)`) moved to `gap == i0` — i.e.
/// "move this block to right where it already is". `len`, `count`, and `i0`
/// are chained through nested `prop_flat_map`s so `count` never exceeds
/// `len` and `i0` never lets the block run off the end.
///
/// Cost: `len` capped at 20; the whole strategy just picks three small
/// integers, no collections at all.
fn arb_noop_move_spec() -> impl Strategy<Value = (usize, usize, usize)> {
    (1usize..20).prop_flat_map(|len| {
        (1..=len)
            .prop_flat_map(move |count| (0..=(len - count)).prop_map(move |i0| (len, count, i0)))
    })
}

/// A `Vec<i32>` with all-unique elements (order arbitrary), for exercising
/// `reconcile_by_key` with `key_fn = identity` — `reconcile_by_key` panics
/// on a duplicate key, so uniqueness is a precondition, enforced here by
/// deduping (keep-first) rather than by filtering the strategy (which would
/// need rejection sampling).
///
/// Cost: source vec capped at 15 elements in a narrow `-20..20` range (so
/// dedup collapses to a plausibly-small-but-not-trivial unique set most of
/// the time); the dedup pass itself is a single O(n) `HashSet`-backed
/// `retain`.
fn arb_unique_rows() -> impl Strategy<Value = Vec<i32>> {
    prop::collection::vec(-20i32..20, 0..15).prop_map(|mut v| {
        let mut seen = std::collections::HashSet::new();
        v.retain(|x| seen.insert(*x));
        v
    })
}

// ── 1. move_items preserves the item multiset ──
// The doc on `move_items` promises a reordering primitive: items are
// relocated, never created or destroyed. Using unique tags `0..len` as the
// item values makes "was anything duplicated or dropped" directly visible
// as a multiset (sorted-vec) equality check.

proptest! {
    #[test]
    fn move_items_preserves_the_item_multiset((len, idx, gap) in arb_move_spec()) {
        let items: Vec<i32> = (0..len as i32).collect();
        let model = ListModel::from_vec(items.clone());

        model.move_items(&idx, gap);

        let mut before = items;
        let mut after = snapshot(&model);
        before.sort_unstable();
        after.sort_unstable();
        prop_assert_eq!(
            &before, &after,
            "move_items(idx={:?}, gap={}) on a {}-element list changed the item multiset: before={:?} after={:?}",
            idx, gap, len, before, after
        );
    }
}

// ── 2. move_items lands the moved block contiguously, preserving its
//      relative order ──
// Documented explicitly: "so they land contiguously at a drop gap,
// preserving their relative order". Checked by locating each originally-
// selected tag's post-move position and asserting those positions are both
// strictly increasing (order preserved) and form one unbroken run
// (contiguity).

proptest! {
    #[test]
    fn move_items_moved_block_lands_contiguously_preserving_relative_order((len, idx, gap) in arb_move_spec()) {
        let items: Vec<i32> = (0..len as i32).collect();
        let model = ListModel::from_vec(items);

        // Mirror move_items' own dedup/filter so we know exactly which tags
        // were actually moved (out-of-range / duplicate entries in `idx`
        // are documented to be ignored).
        let mut moved: Vec<usize> = idx.iter().copied().filter(|&i| i < len).collect();
        moved.sort_unstable();
        moved.dedup();
        if moved.is_empty() {
            // Nothing in `idx` was in range: move_items is a documented
            // no-op, and there is no moved block whose order to check.
            return Ok(());
        }
        let expected_tags: Vec<i32> = moved.iter().map(|&i| i as i32).collect();

        model.move_items(&idx, gap);
        let after = snapshot(&model);
        let mut positions: Vec<usize> = expected_tags
            .iter()
            .map(|tag| {
                after
                    .iter()
                    .position(|v| v == tag)
                    .unwrap_or_else(|| panic!("moved tag {tag} missing from post-move list {after:?}"))
            })
            .collect();

        prop_assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "moved block's relative order was not preserved: tags={:?} landed at positions={:?} (idx={:?}, gap={})",
            expected_tags, positions, idx, gap
        );
        positions.sort_unstable();
        let span = positions.last().unwrap() - positions.first().unwrap() + 1;
        prop_assert_eq!(
            span, positions.len(),
            "moved block did not land contiguously: tags={:?} positions={:?} (idx={:?}, gap={})",
            expected_tags, positions, idx, gap
        );
    }
}

// ── 3. move_items to the block's own current position is a true no-op ──
// When the destination gap equals the moved block's own start, the code
// path takes the "no net movement" branch and skips notification entirely
// (see the `else if contiguous { /* No net movement */ }` arm in
// `list_model.rs`). Verify both halves of that promise: the list contents
// are byte-for-byte unchanged, AND no `DataChange` fires at all.

proptest! {
    #[test]
    fn move_items_is_a_noop_when_the_block_already_sits_at_the_destination(
        (len, count, i0) in arb_noop_move_spec(),
    ) {
        let items: Vec<i32> = (0..len as i32).collect();
        let model = ListModel::from_vec(items.clone());
        let log: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        let _handle = model.observe_changes(move |c| l.borrow_mut().push(c.clone()));

        let idx: Vec<usize> = (i0..i0 + count).collect();
        model.move_items(&idx, i0);

        prop_assert_eq!(
            snapshot(&model), items,
            "moving block {:?} to its own start {} should leave the list unchanged",
            idx, i0
        );
        prop_assert!(
            log.borrow().is_empty(),
            "moving block {:?} to its own start {} should emit no DataChange, got {:?}",
            idx, i0, log.borrow()
        );
    }
}

// ── 4. selection indices never dangle past the end of the list ──
// The classic index-drift bug class: wire a `SelectionModel` to a
// `ListModel` exactly the way `bastyde-widgets`'s `RowSelection::from_index`
// does in production (`ItemsInserted`/`ItemsRemoved`/`ItemsMoved` ->
// `adjust_for_*`, `Reset` -> `clear`), then drive an arbitrary interleaved
// sequence of list mutations and selections and check, after EVERY step,
// that every selected index is still `< len()`.

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn selection_indices_never_dangle_past_the_end_of_the_list(
        mode in prop_oneof![Just(SelectionMode::Single), Just(SelectionMode::Multi)],
        ops in arb_ops(),
    ) {
        let model: ListModel<i32> = ListModel::new();
        let sel = SelectionModel::new(mode);

        // Production wiring, reproduced verbatim from
        // `RowSelection::from_index` (crates/bastyde-widgets/src/data_views.rs).
        let sel_for_obs = sel.clone();
        let _handle = model.observe_changes(move |change| match change {
            DataChange::ItemsInserted { range } => {
                sel_for_obs.adjust_for_insert(range.start, range.end - range.start);
            }
            DataChange::ItemsRemoved { range } => {
                sel_for_obs.adjust_for_remove(range.start, range.end - range.start);
            }
            DataChange::ItemsMoved { from, to, count } => {
                sel_for_obs.adjust_for_move(*from, *to, *count);
            }
            DataChange::Reset => sel_for_obs.clear(),
            DataChange::ItemUpdated { .. } | DataChange::WindowLoaded { .. } => {}
        });

        for op in &ops {
            apply_op(&model, &sel, op);
            let len = model.len();
            for idx in sel.selected_indices() {
                prop_assert!(
                    idx < len,
                    "dangling selected index {} >= len {} after op {:?} in {:?} mode; full ops={:?}",
                    idx, len, op, mode, ops
                );
            }
        }
    }
}

// ── 5. Single mode never holds more than one selected index ──
// `SelectionMode::Single` is documented as "at most one item selected at a
// time" — checked across an arbitrary sequence of selection operations
// (list-mutation ops from the shared `Op` generator are skipped here: the
// point is to probe every selection-facing entry point against a fixed-size
// list, not to also exercise structural drift, which property 4 already
// covers). `select_all` in particular does not appear to special-case
// `Single` mode in the source (it unconditionally selects `0..count`) — if
// that is intentional, this property should be narrowed; as written it
// states the mode's own doc comment literally.

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn single_mode_never_holds_more_than_one_selected_index(
        len in 1usize..30,
        ops in arb_ops(),
    ) {
        let model = ListModel::from_vec((0..len as i32).collect::<Vec<_>>());
        let sel = SelectionModel::new(SelectionMode::Single);

        for op in &ops {
            if let Op::List(_) = op {
                continue;
            }
            apply_op(&model, &sel, op);
            prop_assert!(
                sel.count() <= 1,
                "Single-mode selection held {} indices ({:?}) after op {:?}; full ops={:?}",
                sel.count(), sel.selected_indices(), op, ops
            );
        }
    }
}

// ── 6. shift+click anchor range is symmetric ──
// `extend_to` extends from the stored anchor to a target, inclusive of
// both ends. Selecting `a` then extending to `t` should select exactly the
// same set as selecting `t` then extending to `a` — the interval `[min,
// max]` doesn't care which endpoint was the anchor.

proptest! {
    #[test]
    fn shift_click_anchor_range_is_symmetric_in_anchor_and_target(
        len in 1usize..40,
        raw_a in 0usize..40,
        raw_t in 0usize..40,
    ) {
        let a = raw_a % len;
        let t = raw_t % len;

        let sel1 = SelectionModel::new(SelectionMode::Multi);
        sel1.select(a);
        sel1.extend_to(t);

        let sel2 = SelectionModel::new(SelectionMode::Multi);
        sel2.select(t);
        sel2.extend_to(a);

        prop_assert_eq!(
            sel1.selected_indices(), sel2.selected_indices(),
            "select({})+extend_to({}) should equal select({})+extend_to({}): {:?} vs {:?}",
            a, t, t, a, sel1.selected_indices(), sel2.selected_indices()
        );
    }
}

// ── 7. select_all replaces rather than unions the previous selection ──
// Selecting all is a full replace, not an incremental union: a second
// `select_all` with a smaller count leaves exactly `0..second_count`, with no
// leftover indices surviving from the first, larger call.
//
// Deliberately `Multi`-only. An earlier draft of this property also generated
// `Single` and expected `0..second_count` there, which (a) merely restated the
// implementation rather than a contract, and (b) directly contradicted
// property 5 — `0..second_count` holds more than one index whenever
// `second_count > 1`. Whether `select_all` should no-op or collapse to one
// index in `Single` mode is that property's business, not this one's.

proptest! {
    #[test]
    fn select_all_replaces_rather_than_unions_the_previous_selection(
        first_count in 0usize..50,
        second_count in 0usize..50,
    ) {
        let sel = SelectionModel::new(SelectionMode::Multi);
        sel.select_all(first_count);
        sel.select_all(second_count);

        let expected: Vec<usize> = (0..second_count).collect();
        prop_assert_eq!(
            sel.selected_indices(), expected,
            "select_all({}) after select_all({}) should hold exactly 0..{}, got {:?}",
            second_count, first_count, second_count, sel.selected_indices()
        );
    }
}

// ── 8. select_all then clear returns to empty ──

proptest! {
    #[test]
    fn select_all_then_clear_returns_to_empty_selection(
        count in 0usize..80,
        mode in prop_oneof![Just(SelectionMode::Single), Just(SelectionMode::Multi)],
    ) {
        let sel = SelectionModel::new(mode);
        sel.select_all(count);
        sel.clear();

        prop_assert_eq!(
            sel.count(), 0,
            "select_all({}) then clear() should leave 0 selected in {:?} mode, got {}",
            count, mode, sel.count()
        );
        prop_assert!(sel.selected_indices().is_empty());
    }
}

// ── 9. reconcile_by_key reaches exactly the new rows, never via Reset ──
// Generalizes the hand-picked before/after scenarios in `list_model.rs`'s
// own `reconcile_never_emits_reset` test to arbitrary unique-keyed row
// sets: whatever `before` the model starts with, reconciling to `after`
// must land on exactly `after` (same keys, same order, same content) and
// must never fall back to `DataChange::Reset` — the whole point of
// `reconcile_by_key` over `replace_all` is to avoid wiping a live
// `SelectionModel` via `Reset`.

proptest! {
    #[test]
    fn reconcile_by_key_reaches_exactly_the_new_rows_with_no_reset(
        before in arb_unique_rows(),
        after in arb_unique_rows(),
    ) {
        let model = ListModel::from_vec(before.clone());
        let log: Rc<RefCell<Vec<DataChange>>> = Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        let _handle = model.observe_changes(move |c| l.borrow_mut().push(c.clone()));

        model.reconcile_by_key(after.clone(), |v| *v);

        let result = snapshot(&model);
        prop_assert_eq!(
            &result, &after,
            "reconcile_by_key should reach exactly `after`: before={:?} after={:?} got={:?}",
            before, after, result
        );
        prop_assert!(
            !log.borrow().iter().any(|c| matches!(c, DataChange::Reset)),
            "reconcile_by_key must never emit Reset: before={:?} after={:?} log={:?}",
            before, after, log.borrow()
        );
    }
}

// ── 10. DataChange events, replayed against a naive Vec, reproduce the
//       model's contents (ORACLE) ──
// The strongest property in this file: an independent `Vec<i32>`, updated
// purely by interpreting each `DataChange` the model emits (reading the
// model's post-mutation state for values a bare range/index doesn't carry,
// exactly as `notify`'s "borrow already dropped" guarantee permits any real
// observer to do), must end up byte-for-byte identical to the model's own
// contents after an arbitrary op sequence. This checks that the
// notifications are a *complete* description of the change, not just that
// each mutator does the right thing to the model in isolation.

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn list_change_events_replayed_against_a_naive_vec_reproduce_the_models_contents(
        ops in prop::collection::vec(arb_list_op(), 0..30),
    ) {
        let model: ListModel<i32> = ListModel::new();

        let oracle: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
        let oracle_obs = oracle.clone();
        let model_obs = model.clone();
        let _handle = model.observe_changes(move |change| {
            let mut oracle = oracle_obs.borrow_mut();
            match change {
                DataChange::ItemsInserted { range } => {
                    for i in range.clone() {
                        let v = model_obs
                            .with_item(i, |v| *v)
                            .expect("model already reflects the insert by notify time");
                        oracle.insert(i, v);
                    }
                }
                DataChange::ItemsRemoved { range } => {
                    let count = range.end - range.start;
                    for _ in 0..count {
                        oracle.remove(range.start);
                    }
                }
                DataChange::ItemUpdated { index } => {
                    let v = model_obs
                        .with_item(*index, |v| *v)
                        .expect("model already reflects the update by notify time");
                    oracle[*index] = v;
                }
                DataChange::ItemsMoved { from, to, count } => {
                    let block: Vec<i32> = oracle.drain(*from..*from + *count).collect();
                    for (off, v) in block.into_iter().enumerate() {
                        oracle.insert(*to + off, v);
                    }
                }
                DataChange::Reset => {
                    oracle.clear();
                    oracle.extend((0..model_obs.len()).map(|i| model_obs.with_item(i, |v| *v).unwrap()));
                }
                // `ListModel` never emits this itself — it exists for lazy/
                // windowed `ListDataSource` implementors — included only for
                // match exhaustiveness.
                DataChange::WindowLoaded { .. } => {}
            }
        });

        for op in &ops {
            apply_list_op(&model, op);
        }

        let actual = snapshot(&model);
        let reconstructed = oracle.borrow().clone();
        prop_assert_eq!(
            reconstructed, actual,
            "replaying DataChange events did not reproduce the model's final contents; ops={:?}",
            ops
        );
    }
}
