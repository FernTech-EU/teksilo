<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# PaintKey

One paint order, read by every picker.

A `SceneView` paints in three passes: the lightweight
`SceneLayer::Under` band, then the heavyweight widget
children (the arena's child walk), then the lightweight
`Over` band. That is a **total order across both
tiers**, and `PaintKey` is that order named as a value, so a hit test can
be written as a comparison instead of as a second, independently-invented
rule.

# What is unified here, and what is not

**The ordering is unified; the narrow phase is shared but not centralised.**
A hit test runs on the pointer's hot path, where the scene's `RefCell` must
not be re-entered (an `ItemChange` observer can be mid-mutation), so the two
dispatch entry points keep their own per-layout snapshots rather than
calling back into `Scene`. What stops them diverging is that
every one of them compares `PaintKey`s and narrow-phases through the same
`ItemShape` value the scene itself would use — not that
they share one function.

The five places that used to answer "what did the pointer hit?" their own
way now all read this module:

| site | what it reads |
|------|---------------|
| `Scene::item_at` / `items_at` | `Scene::paint_key`, descending |
| the `SceneView` tap / hover / cursor snapshot | `HandlerSnapshotEntry::key`, descending |
| the `SceneView` drag snapshot | `DraggableSnapshotEntry::key`, descending |
| `SceneView::paint_band` | `Scene::paint_key`, ascending |
| the arena's own walk | the `Over` veto, below |

A **sixth** picker is deliberately left out of that table: the magnet probe
in `view/gestures_impl.rs` is still ordered by *distance*, because a magnet
handle is grabbed by proximity, runs before the item hit test and takes
priority over it. Folding it into the paint order is a separate decision
with its own owner.

# The occlusion rule

**Occlusion follows press-claiming, not painting.** A lightweight entry
takes a press away from a heavyweight card only if it would *act on that
press* — a tap, a double-tap, a context menu, or a drag. See
`claims_press`. A decorative halo drawn over an embedded note claims
nothing, so it never vetoes, and clicking it leaves the note pressed and
focused.

This is deliberately **not** the widget tier's rule, where a plain
`RectWidget` on top of a `Button` absorbs the click. It matches Qt's
`setAcceptedMouseButtons(NoButton)` and Godot's `mouse_filter = IGNORE`, and
it is what keeps a scene whose foreground is decorative working exactly as
it did.

**A hover affordance is not a press claim.** `on_hover`, `cursor` and
`tooltip` fire on a pointer that merely *passes over*, and none of them is
the beginning of a press. Treating one as a claim looks conservative and is
the opposite: the card leaves the arena's walk, the view becomes the target,
the tap resolves to an item with no `on_tap`, and the click reaches nobody
at all. They are served instead by the view's own hover seam, which runs in
the **preview** pass — on every strict ancestor of the target — and so
reaches the item whether a card won the walk or not.

Note what the rule does **not** change: inside the lightweight tier the
topmost *entry* still wins, and only then are its handlers consulted. A
handler-less item painted on top of a handler-bearing one still blocks it,
as it always has. Press-claiming decides one thing only — whether the
lightweight tier gets to veto a heavyweight card.

# What this rule does *not* reach

It governs the **pointer**: the arena's hit test, and therefore press
feedback, focus-on-release, the touch hold route, the cursor and the view's
own drag recognizer. It does not touch the AccessKit tree, and a platform
explore-by-touch probe resolves through that tree, not through this one. The
two can therefore still disagree about what is under a point — see
`docs/teksilo-scene-a11y.md`, "Where the pointer and the AT probe still
disagree".

## Builder methods at a glance

`rank`, `z`, `seq`, `is_above_widgets`, `bottom`, `rank_floor`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

## `pub const RANK_UNDER`

The `PaintKey::rank` of a lightweight item in the
`Under` band.

```rust
pub const RANK_UNDER: u8 = 0;
```

## `pub const RANK_WIDGET`

The `PaintKey::rank` of a heavyweight widget entry.

```rust
pub const RANK_WIDGET: u8 = 1;
```

## `pub const RANK_OVER`

The `PaintKey::rank` of a lightweight item in the
`Over` band.

```rust
pub const RANK_OVER: u8 = 2;
```

## `pub struct PaintKey`

Where an entry sits in a `SceneView`'s **single** paint
order.

| rank | contents |
|------|----------|
| `RANK_UNDER` | lightweight items in `SceneLayer::Under` |
| `RANK_WIDGET` | heavyweight widget entries, and lightweight items in `SceneLayer::Interleaved` |
| `RANK_OVER` | lightweight items in `SceneLayer::Over` |

Within a rank, ascending `z`; ties broken by `seq`, the entry's
`ItemId` — which is a process-monotone counter, so ties
resolve in **insertion order**. A *higher* key paints later, is therefore on
top, and therefore wins the pointer.

`SceneLayer` is not a separate concept any more: it is
the leading digit of this key.

# Why the tie-break is load-bearing

Paint sorts ascending and stably over an id-ordered candidate list, so the
newest of two equal-`z` items paints last and is on top. The hit snapshots
used to sort **descending** and stably over that same ascending list, which
leaves equal-`z` ties in *ascending* order — so the first match was the
oldest, i.e. the **bottom-most**, item. Paint and hit were exactly inverted
on ties. Comparing whole keys fixes that by construction: a descending sort
on `(rank, z, seq)` really does put the topmost entry first.

# NaN

A non-finite `z` is normalised to `0.0` at construction, so the ordering is
total and `sort_by` is not order-dependent. The old sites compared with
`partial_cmp(..).unwrap_or(Equal)`, which is not a total order.

```rust
pub struct PaintKey { /* fields */ }
```

### Methods

#### `pub fn new(rank: u8, z: f32, seq: u64) -> Self`

Build a key. `z` is normalised (`NaN` → `0.0`) so the ordering is total.

#### `pub const fn rank(self) -> u8`

The tier / band digit — one of `RANK_UNDER`, `RANK_WIDGET`,
`RANK_OVER`.

#### `pub const fn z(self) -> f32`

The entry's z-order within its rank.

#### `pub const fn seq(self) -> u64`

The insertion tie-break — the entry's `ItemId` as a
number.

Stable only as long as the id is. A
`SceneListAdapter` re-mints ids on a
structural `DataChange`, so equal-`z` ordering inside an adapter-owned
run does shuffle on an append. That is a property of the adapter, not a
guarantee this key makes.

#### `pub const fn is_above_widgets(self) -> bool`

True when this key can never lose to a heavyweight entry.

#### `pub const fn bottom() -> Self`

A floor below every real key — "admit everything".

`z` is `-inf` rather than a finite sentinel so that a real entry sitting
at `f32::MIN` is still admitted, and `Ord` compares `z` with
`total_cmp`, under which `-inf` is below every finite value.

#### `pub const fn rank_floor(rank: u8) -> Self`

A floor admitting exactly the entries at `rank` and above.

The coarse form of a floor, and the one the dispatch path uses: "a card
won the arena's walk, so only the band that outranks *every* card may
still take this event". The fine form — one specific entry's key — is
what the `accepts_child_hit` veto needs, because an
`Interleaved` entry is above some
cards and below others.

## `pub fn claims_press(...)`

Whether an entry would **act on a press** — the occlusion rule of this
module, in one predicate.

True when the item is `IS_DRAGGABLE`, or carries
a handler that a press is the beginning of: `on_tap`, `on_double_tap` or
`on_context_menu`. False for everything else.

Read at exactly one place — the above-a-card veto that decides whether the
lightweight tier may take a press away from a heavyweight card. Not
band-scoped: it compares whole `PaintKey`s per child, so an
`Interleaved` claimant vetoes the cards it
is painted over too. Everything
else in the scene still resolves the topmost *entry* and consults its
handlers afterwards.

# Why a hover affordance is not a claim

`on_hover`, `cursor` and `tooltip` are **hover** affordances, and a hover
affordance does not take a press. Counting one as a claim is not a
conservative over-approximation, it is a click-swallower: the veto removes
the card from the arena's walk, the view becomes the target, and the tap
then resolves to an item that has no `on_tap` — so the press reaches
*nobody*, silently. A corkboard that adds a `.tooltip(..)` to an `Over` hint
region would make every note beneath it un-clickable, un-focusable and
un-editable by mouse.

All three keep working, and they never needed the veto to: the view's own
hover seam runs in the **preview** pass, which fires on every strict
ancestor of the target, so it reaches the item whether a card won the walk
or not. The cursor is the one that has to be arbitrated rather than merely
allowed, and it is — by the `SceneView`'s cursor rule (an `Over` item's
declared cursor wins, silence otherwise), not by taking the card's press
away.

`accepts_drops` is not a claim either, for the same reason in the other
direction: a drop is the *release* of somebody else's drag, not a press on
this item, and nothing in the crate routes a drop through this predicate.

```rust
pub fn claims_press(handlers: Option<&SceneItemHandlerSet>, flags: ItemFlags) -> bool;
```

## `pub fn hit_testable(...)`

Whether an entry takes part in pointer hit-testing at all.

The single home of two flag contracts that used to be written nowhere and
honoured nowhere:

* `IS_VISIBLE` — "the item is neither painted nor
  hit-tested". `effectively_visible` is
  `Scene::is_effectively_visible`,
  so a visible item under a hidden ancestor is excluded too.
* `IS_ENABLED` — "disabled items are still painted
  but pass clicks through to items beneath".

Applied by `Scene::item_at` / `items_at` / `item_at_in_view` and by both of
the `SceneView`'s per-layout snapshots, so the eager query and the dispatch
path answer the same question.

```rust
pub fn hit_testable(effectively_visible: bool, flags: ItemFlags) -> bool;
```
