<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Property-Based Testing Reference

Proptest was introduced to the workspace on the branch that produced this
document, into a codebase that had zero property tests before it. There are
now ~92 properties across `teksilo-tokens`, `teksilo-data`, `teksilo-scene`,
and `teksilo-widgets`, and they found eight real bugs (see *What this
found* below) — including one that exhausted 61 GiB of RAM and forced three
hard reboots of the developer's workstation before it was diagnosed. That
incident is itself part of what this document exists to prevent from
happening again.

Mental model in one line:

```
proptest generates hundreds of inputs per property and shrinks any failure to a minimal counterexample — cargo-fuzz's coverage, on stable, in `cargo test`
```

The convention below is not invented for Teksilo. It is carried over
unchanged from the author's sibling repos `../text-typeset` and
`../text-document`, which have run proptest for longer; this document
writes that convention down for this workspace so it applies uniformly
here too, rather than living only as tribal knowledge.

---

## Convention

**File placement.** Two shapes, chosen by the tested item's visibility —
never by convenience:

- **`tests/*.rs` integration test** when the target is `pub` and reachable
  from outside the crate. Example:
  [`crates/teksilo-tokens/tests/prop_color.rs`](../crates/teksilo-tokens/tests/prop_color.rs)
  tests `Color`, a public type re-exported at the crate root.
- **Inline `#[cfg(test)] mod proptests`**, placed as a sibling of the
  existing `#[cfg(test)] mod tests` in the same file, when the target is
  `pub(crate)` or otherwise unreachable from `tests/`. The module doc must
  say *why* it lives inline rather than assume the reader can tell. Three
  worked examples, each stating a different reason:
  - [`crates/teksilo-widgets/src/splitter/distribute.rs`](../crates/teksilo-widgets/src/splitter/distribute.rs):
    `distribute` is `pub fn`, but its declaring module (`mod distribute;` in
    `splitter.rs`) is private and not re-exported, so the function is
    unreachable from an external test crate even though the fn signature
    itself says `pub`.
  - [`crates/teksilo-scene/src/index.rs`](../crates/teksilo-scene/src/index.rs):
    `GridHashIndex` is declared `pub struct`, but it lives inside
    `pub(crate) mod index;` in `lib.rs` — the module's visibility caps the
    struct's, regardless of the `pub` on the struct itself. Reading a `pub`
    keyword on the item is not sufficient to decide placement; check the
    declaring module's visibility too.
  - [`crates/teksilo-widgets/src/primitives/column_flow.rs`](../crates/teksilo-widgets/src/primitives/column_flow.rs):
    `ColumnFlow` itself is public, but `balance_columns` — the pure function
    the suite actually targets — is `pub(crate)`.

  **Never widen an item's visibility to make it reachable from `tests/`.**
  If the item is `pub(crate)`, the test lives inline; that is the whole
  decision procedure.

**One property per `proptest! {}` block**, each preceded by a numbered
banner comment stating the exact claim under test:

```rust
// ── 12. for_inactive_window desaturates exactly the accent family and
//      leaves everything else (except chart_palette) untouched, for an
//      arbitrary accent color — not just the two shipped IntUI presets ──
proptest! {
    #[test]
    fn for_inactive_window_desaturates_only_the_accent_field(accent in arb_color()) {
        // ...
    }
}
```

**Forbidden**, deliberately, so every suite reads the same way: the
`#[proptest]` attribute macro, `prop_compose!`, `#[derive(Arbitrary)]`, a
manually driven `TestRunner`, or a hand-rolled RNG. Strategies are built by
hand from `proptest::prelude` combinators (`prop_oneof!`, `.prop_map`,
`.prop_flat_map`, `prop::collection::vec`, …).

**Hand-written local `fn arb_x() -> impl Strategy<Value = X>` generators,
one set per file.** There is no shared generator module, and that is a
deliberate choice, not an oversight: `arb_parent_sel`/`arb_insert_ops`
appear near-verbatim in both
[`prop_tree_slice.rs`](../crates/teksilo-data/tests/prop_tree_slice.rs) and
[`prop_tree_checked.rs`](../crates/teksilo-data/tests/prop_tree_checked.rs)
rather than being factored out — per-file duplication is the accepted cost
of keeping each suite's generators legible and independently auditable
without chasing a shared abstraction across files.

**Case counts are the only tuning knob**, set per block:

```rust
#![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
```

There is no `proptest.toml` anywhere in this workspace. The default (256,
proptest's own default) is left alone for ordinary properties; blocks that
are cheap and unusually valuable (an oracle-vs-brute-force check, a
panic-freedom sweep over malformed input) opt into 512 or 1024 explicitly,
with the reason stated next to the override — see
`from_hex_never_panics_and_stays_in_range` (1024, attacker-adjacent hex
parsing) or the oracle properties in
[`prop_sort_filter.rs`](../crates/teksilo-data/tests/prop_sort_filter.rs)
(512, cheap per-case). The manual override for a one-off deeper run is
`PROPTEST_CASES=N cargo test -p <crate> ...` — every suite's module doc
states the exact invocation.

**Assertions are `prop_assert!`/`prop_assert_eq!` only**, and always carry a
format-string message citing the actual values that failed, never a bare
condition:

```rust
prop_assert!(
    from_brute.is_subset(&from_index),
    "cell_size={} query={:?}: index missed true intersections {:?} (model={:?})",
    cell_size, q, from_brute.difference(&from_index).collect::<Vec<_>>(), model,
);
```

A bare `assert!(x)` inside a `proptest!` block is a debugging dead end: the
shrinker hands you a minimal failing case, and a bare assertion throws that
context away at the exact moment it matters most.

**Test names read as English claims describing the property**, never
`test_`-prefixed:
`hex_roundtrip_is_stable_after_one_quantization`,
`selection_indices_never_dangle_past_the_end_of_the_list`,
`reorder_within_stays_acyclic_and_conserves_the_node_set`,
`a_single_items_cell_footprint_never_exceeds_the_per_item_cap`.

**`.proptest-regressions` files are committed**, never gitignored — proptest
reruns every recorded seed before generating novel cases, so a shrunk
counterexample stays a permanent regression check once found. Two on-disk
shapes, matching the two file-placement shapes above:

- `tests/*.rs` suites: the regressions file sits next to the test file,
  e.g. [`crates/teksilo-data/tests/prop_sort_filter.proptest-regressions`](../crates/teksilo-data/tests/prop_sort_filter.proptest-regressions).
- Inline `mod proptests` suites: proptest names the file after the module
  path and roots it at the crate, e.g.
  [`crates/teksilo-widgets/proptest-regressions/splitter/distribute.txt`](../crates/teksilo-widgets/proptest-regressions/splitter/distribute.txt)
  and `.../proptest-regressions/common/row_offsets.txt`.

A suite with no regressions file (`prop_tree_slice.rs`, `column_flow.rs`,
`column_geometry.rs` in this workspace today) simply never shrank a
failure — that is a legitimate outcome, not a sign the suite is incomplete.

**Stated rationale** (from the workspace `Cargo.toml`, next to the
`proptest = "1"` dependency line): `cargo-fuzz` needs a nightly toolchain
(`libfuzzer-sys` links against compiler-rt's fuzzing runtime), which isn't a
guaranteed CI dependency. Proptest gives the same "never panics on weird
input" coverage plus shrinking, on stable, as a plain `[dev-dependencies]`
entry — no separate fuzzing job, no nightly pin. Every crate with a suite
re-states this in its own `Cargo.toml` next to `proptest = { workspace =
true }` rather than assuming the reader finds the workspace manifest.

---

## Scope — what belongs here, what does not

Proptest owns **relational** properties: statements that must hold between
an input and an output, or between two independent ways of computing the
same thing, for every input in a domain — not statements about one exact
rendered result.

| Shape | What it checks | Example |
|---|---|---|
| Round-trip | encode then decode (or the reverse) reproduces the original, or a documented fixed point | `hex_roundtrip_is_stable_after_one_quantization`, `hsv_roundtrip_holds_for_arbitrary_opaque_colors` (teksilo-tokens) |
| Idempotence | a second application changes nothing once the first has converged | `reaggregate_is_a_noop_when_the_model_is_already_consistent` (teksilo-data), `reinserting_identical_bounds_is_idempotent` (teksilo-scene) |
| Conservation | a quantity (item multiset, total height, node set) survives a transform exactly | `move_items_preserves_the_item_multiset` (teksilo-data), `total_conserves_the_sum_of_heights_and_gaps_and_insets` (teksilo-widgets) |
| Monotonicity | an ordered change in input can only move the output in one direction | `query_is_monotonic_in_rect_containment` (teksilo-scene), `desaturation_spread_is_monotone_in_amount` (teksilo-tokens) |
| Oracle vs. brute force | a fast/incremental/cached path agrees with an independent from-scratch recompute | `the_incremental_tree_projection_equals_a_full_recompute` (teksilo-data), `query_narrowed_matches_brute_force` (teksilo-scene) |
| Metamorphic | the same operation under a transformed input yields a predictably related output | `checked_state_survives_a_reload_with_a_different_shape_then_resyncs_on_reaggregate` (teksilo-data) |
| Determinism | identical inputs always produce identical outputs | `distribute_is_deterministic_for_identical_inputs` (teksilo-widgets), `query_is_independent_of_insertion_order` (teksilo-scene) |
| Panic-freedom | the function returns rather than unwinds, across arbitrary — including malformed — input | `from_hex_never_panics_and_stays_in_range`, `query_never_panics` |

It does **not** own exact pixel or glyph output. A rendered bitmap is
font-version- and shaper-version-dependent, so pinning one down as a
property assertion means the property breaks on every font update rather
than on a real regression — that is `insta` snapshot territory in
`../text-typeset`, which owns real shaping/bidi/line-break/raster coverage
against real fonts. Proptest here never touches a GPU, a display server, or
real font shaping; every suite in this workspace runs against pure data
(`Color`, `ListModel`, `TreeModel`, `GridHashIndex`, `PrefixSumOffsets`,
`distribute`, …), headless, with no rendering in the loop.

The test for a candidate: can you state it as *"for every valid `x`, `f(x)`
relates to `x` (or to `g(x)`, an independently written second computation)
in such-and-such a way"* — without needing to look at a rendered frame to
know if it passed? If yes, it is proptest's. If the honest assertion is
"this exact bitmap" or "this exact glyph outline", it belongs in a
snapshot test instead.

---

## Generator cost discipline

This is the section that matters operationally. A generator explores a
**product** space, and proptest will find the worst corner of that space
within a few hundred cases — including corners the author never
considered reachable.

**Cost the most expensive combination before writing the generator**, and
record that reasoning in a comment next to it. Every generator in this
workspace carries one; see the cost comment on `arb_list_op` in
[`prop_list_and_selection.rs`](../crates/teksilo-data/tests/prop_list_and_selection.rs)
or on `arb_pane_with_wild_bounds` in
[`distribute.rs`](../crates/teksilo-widgets/src/splitter/distribute.rs) —
each states the worst-case element count and the worst-case per-op cost
before the strategy is defined, not after a failure.

**Bound every `prop::collection::vec` length.** Keep the modeled state
small — a tree of 20–30 nodes, an op sequence of 30 steps, a pane list of
8. Properties find bugs through *many small cases*, not a few large ones;
a generator that can build a 10,000-node tree buys nothing over one capped
at 30 except CI time and a harder-to-read shrunk counterexample.

**Couple dependent inputs.** Drawing two related quantities from two
independent strategies is the specific trap that caused the incident this
document opened with. `GridHashIndex`'s generator originally drew a rect's
extent and the grid's `cell_size` from two separate `arb_cell_size()`
calls: a rect sized for a 256 px grid (extent up to `256 * 64`) inserted
into an unrelated 1 px grid spans roughly 268 million cells. Separately,
`cell_size: 1.0` paired with the incident's own `1e6` extent asked for
`(1e6+1)² ≈ 1e12` cells — about 8 TB for the `Vec<(i32, i32)>` alone. That
exhausted 61 GiB of RAM and forced three hard reboots before it was
diagnosed as a generator bug compounding a real one.

The fix is `prop_flat_map`, so the dependent quantity is *derived* from the
one it depends on rather than drawn independently. The worked example is
`arb_grid_and_two_rects` in
[`crates/teksilo-scene/src/index.rs`](../crates/teksilo-scene/src/index.rs):

```rust
fn arb_grid_and_two_rects() -> impl Strategy<Value = (f32, Rect, Rect)> {
    arb_cell_size().prop_flat_map(|cs| (Just(cs), arb_rect(cs), arb_rect(cs)))
}
```

`arb_rect(cs)` bounds its extent to at most `cs * MAX_CELLS_PER_AXIS`
cells *for that specific `cs`*, so no generated pair can ever reproduce the
mismatch. Every property in that suite that builds a grid and inserts into
it draws its rects from here — never from two independent calls.

**When a suite OOMs, the unbounded allocation in the production code is
usually the real finding — fix or report that, not just the generator.**
That is exactly what happened here: `cells_for_rect` computed
`(width / cell_size) * (height / cell_size)` cells and reserved that many
`(i32, i32)` slots *before* the fill loop ran, with no upper bound and with
`i32` arithmetic that could itself overflow. A single oversized item —
a scene backdrop, a full-document canvas rect, both reachable in
production, not exotic — could OOM or crash a real app with no adversarial
input required. The fix (`b7f6e066`, see *What this found* below) added
`MAX_CELLS_PER_ITEM` and an always-scanned `oversized: HashMap<ItemId,
Rect>` side list for any item whose AABB would exceed it. Shrinking the
generator down to the smallest range that doesn't crash would have
silenced the finding without touching the actual bug.

---

## The safe run protocol

**Never execute a suite's binary directly under `cargo test`.** A bare
`cargo test` runs every generator unbounded — exactly the condition that
caused the incident. Always build and run separately, with a hard memory
cap on the run:

```bash
cargo test -p <crate> --lib --no-run
BIN=$(cargo test -p <crate> --lib --no-run --message-format=json 2>/dev/null \
  | jq -r 'select(.executable != null) | .executable' | tail -1)
( ulimit -v 6000000 -t 300; "$BIN" <filter> --test-threads=1 )
```

- `--no-run` builds the test binary without executing anything, so a
  pathological generator can't run before you've had a chance to cap it.
- `--message-format=json` on the same `--no-run` build reports the exact
  binary path (`.executable`) without guessing at `target/debug/deps/...`
  hashes.
- `ulimit -v 6000000` caps the subshell's virtual address space at ~6 GB
  (adjust to the machine); `ulimit -t 300` caps CPU time at 5 minutes as
  cheap insurance against a runaway loop that isn't merely allocating.
  `--test-threads=1` keeps the cap meaningful — proptest's own case
  parallelism would otherwise multiply the peak by however many threads
  are live.

This turns what would otherwise be a machine-killing OOM (invisible until
the OS starts swapping and the desktop freezes) into a clean, immediate
`memory allocation of N bytes failed` from the process itself — a report
you can read, not a hard reboot.

---

## Working with a property that fails

**Never weaken a property to make it green.** Do not relax the assertion,
narrow the generator to dodge the failing input, add a `prop_assume!` that
filters the failing case out, or delete the property. Any of those hides
the finding instead of resolving it.

Instead: record the shrunk counterexample verbatim (proptest already does
this in the `.proptest-regressions` file — e.g.
`cc 48dc58d1... # shrinks to ops = [InsertRoot(0, 0), InsertRoot(1, 3), SetSort(Some(Descending)), Update(7, 3)], mode = HideNonMatching`
in
[`prop_sort_filter.proptest-regressions`](../crates/teksilo-data/tests/prop_sort_filter.proptest-regressions)),
read the implementation the property targets, and decide honestly which of
the following applies.

**A property that merely restates the implementation is worthless.**
`prop_list_and_selection.rs`'s property 5
(`single_mode_never_holds_more_than_one_selected_index`) notes in its own
comment that `select_all` "does not appear to special-case `Single` mode in
the source (it unconditionally selects `0..count`)" — i.e. its first draft
was paraphrasing what the code did rather than stating a contract. Worse,
an earlier draft of the *neighbouring* property 7
(`select_all_replaces_rather_than_unions_the_previous_selection`) generated
`SelectionMode::Single` too and expected `0..second_count` there — which
directly contradicts property 5's own invariant (`0..second_count` holds
more than one index whenever `second_count > 1`). Two properties in the
same file, each individually plausible, asserting incompatible things. The
resolution recorded in property 7's comment: narrow it to
`Multi`-only, with the reasoning why written down — "whether `select_all`
should no-op or collapse to one index in `Single` mode is that [other]
property's business, not this one's." Property 5 turned out to state the
real contract; property 7 was fixed to stop overreaching into it.

The same shape recurs in
[`prop_sort_filter.rs`](../crates/teksilo-data/tests/prop_sort_filter.rs)'s
property 6: a brute-force "matches ∪ descendants" oracle agrees with
`SortFilterTreeModel` for `HideNonMatching` and `KeepAncestors`, but not for
`KeepDescendants` — traced by hand (not by running anything) to
`flatten_visible` starting its walk at each top-level root and bailing out
immediately if the root itself isn't visible, so a matching descendant
under a non-matching root is never reached even though the brute-force
definition says it should be visible. `tree_row_filter.rs`'s own module doc
already calls this divergence deliberate. The property was narrowed to the
two modes the crate itself claims are equivalent
(`arb_ancestor_preserving_filter_mode`), with the reasoning recorded in the
property's comment, rather than asserting a stricter promise than the type
actually makes.

**When it is a real bug but the fix is a design decision, park it —
`#[ignore]`, the counterexample, and a "do NOT weaken this assertion" note
— never delete it silently.** Two properties in this workspace went through
exactly that arc and were later resolved; both are worth reading as a pair,
before-and-after, in git history:

- `distribute.rs`'s NaN-bound property was committed as
  `#[ignore = "unresolved: NaN min/max panics inside f32::clamp — see comment"]`
  with a comment explaining the panic, why it is reachable (`PaneDescriptor`
  bounds are app-supplied; a min derived from a `0/0` ratio is NaN), and
  explicitly declining to guess at the right normalization without owner
  review. It was later un-ignored once `03341d1f` (see below) picked a
  resolution.
- `row_metrics.rs`'s uniform-vs-exact agreement property was committed
  `#[ignore = "unresolved: uniform and exact modes disagree on all-zero
  heights — see comment"]`, again with the reasoning ("touches every
  `PrefixSumOffsets` consumer, so it is left for review") and was later
  un-ignored by `a788e191`.

Both comments, while parked, state outright: *"Do NOT weaken this assertion."*

**Pin a shrunk counterexample as a named `#[test]` when it represents a bug
worth remembering permanently**, alongside — not instead of — the
`proptest!` property that found it:
[`oversized_1e6_extent_at_cell_size_one_never_allocates_the_pathological_cell_count`](../crates/teksilo-scene/src/index.rs)
is the literal incident input (`Rect::new(0.0, 0.0, 1_000_000.0,
1_000_000.0)` at `cell_size: 1.0`) pinned as a plain, deterministic
`#[test]` in `mod tests` — so the exact input that took a workstation down
three times stays a permanent regression check independent of proptest's
own seed file.

---

## What this found

Eight bugs across four crates. The pattern is worth stating plainly: with
one exception (the `GridHashIndex` unbounded allocation, a missing
resource bound rather than a disagreement between two things), every one
was an **inconsistency** — one code path failing to honor an invariant a
sibling path already upheld — not a wrong formula. That is the class
example-based tests structurally cannot reach, because writing an example
that exercises it requires already suspecting the specific pairing that
diverges.

| Crate | Bug | Shape |
|---|---|---|
| `teksilo-tokens` | `Color::mix`'s `t.clamp(0.0, 1.0)` propagates a NaN factor into every channel, since `f32::clamp` returns NaN for a NaN input | NaN-unsafe guard written for the ordered case |
| `teksilo-scene` | `GridHashIndex::cells_for_rect` reserved one `(i32, i32)` slot per covered cell with no upper bound; a single oversized item could request ~1e12 cells | unbounded allocation, not a disagreement |
| `teksilo-data` | `SelectionModel::select_all` checked `SelectionMode::None` but not `Single`, so it selected the full `0..count` range on a single-selection model | one mutator not upholding an invariant every sibling mutator (`select`, `toggle`, `extend_to`, `select_indices`) already honored |
| `teksilo-data` | `SortFilterListModel`'s incremental `ItemUpdated` fast path bailed out to a full rebuild only on a `Greater` comparison, not a tie | fast path disagreeing with the full stable-sort recompute it exists to optimise |
| `teksilo-data` | `SortFilterTreeModel`'s incremental `NodeUpdated` fast path had the identical tie-blindness, independently, in a different file | same class of bug, duplicated logic drifting apart |
| `teksilo-data` | `KeyedTreeCheckedModel::prune_missing` used an empty stale-key list as its "nothing changed" signal, but a removed subtree that was never explicitly checked also produces an empty stale list | untracked-ness conflated with unchanged; skipped `reaggregate()` left a stale ancestor tristate |
| `teksilo-widgets` | `distribute`'s phase-0 clamp read `if lo > hi { lo } else { req.clamp(lo, hi) }` — the guard catches a finite `min > max` but not a NaN bound, since both `NaN > hi` and `lo > NaN` are `false` | the same NaN-unsafe-guard shape as the `Color::mix` bug, in an unrelated crate |
| `teksilo-widgets` | `RowMetrics::uniform` and `RowMetrics::exact` describe the same geometry but disagreed on an all-zero-height, all-zero-spacing table (a fully collapsed or filtered list): uniform answers row 0, the offset table's `partition_point` answers the last row | two modes describing one geometry differently |

Two of these — the `Color::mix` NaN hole and the `distribute` NaN hole — are
literally the same defect shape (`f32::clamp` panics or propagates NaN; a
hand-written `>` guard doesn't catch NaN because every NaN comparison is
`false`) found independently in two unrelated crates by two unrelated
suites. Not one bug in this list is a case of the underlying formula itself
being wrong.

---

## Where the suites live

| Crate | File(s) |
|---|---|
| `teksilo-tokens` | [`tests/prop_color.rs`](../crates/teksilo-tokens/tests/prop_color.rs) |
| `teksilo-data` | [`tests/prop_list_and_selection.rs`](../crates/teksilo-data/tests/prop_list_and_selection.rs), [`tests/prop_tree_slice.rs`](../crates/teksilo-data/tests/prop_tree_slice.rs), [`tests/prop_tree_checked.rs`](../crates/teksilo-data/tests/prop_tree_checked.rs), [`tests/prop_sort_filter.rs`](../crates/teksilo-data/tests/prop_sort_filter.rs) |
| `teksilo-scene` | [`src/index.rs`](../crates/teksilo-scene/src/index.rs) (`mod proptests`, inline) |
| `teksilo-widgets` | [`src/common/row_offsets.rs`](../crates/teksilo-widgets/src/common/row_offsets.rs), [`src/common/row_metrics.rs`](../crates/teksilo-widgets/src/common/row_metrics.rs), [`src/primitives/column_flow.rs`](../crates/teksilo-widgets/src/primitives/column_flow.rs), [`src/common/column_geometry.rs`](../crates/teksilo-widgets/src/common/column_geometry.rs), [`src/splitter/distribute.rs`](../crates/teksilo-widgets/src/splitter/distribute.rs) (all `mod proptests`, inline) |

Each file's own module doc states its case-count defaults, its
`PROPTEST_CASES` override invocation, and — where relevant — the specific
regression it was written against. Read the target file's doc comment
before adding a property to it; the conventions above are enforced by
precedent, not by a lint.
