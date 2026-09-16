<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# `teksilo-scene` accessibility

The user-facing reference for shaping a scene's accessibility tree
without touching the framework AT walker. Pairs with the visual
reference at [`teksilo-scene.md`](teksilo-scene.md).

`SceneView` ships an accessible tree out of the box: every visible
heavyweight widget participates as a normal child, every visible
lightweight item gets a synthetic AT node with a role and a
rectangle, Tab cycles in scene-insertion order. This
document covers the levers that override that default when the AT
shape needs to *diverge* from the visual layout — the typical case
for story corkboards, node-graph editors, CAD canvases, anything
where "what the eye sees" and "what the ears need" aren't the same
tree.

---

## Two layers

The AT machinery has two cooperating layers:

1. **Visual default.** [`A11yOffScreenMode`](../crates/teksilo-scene/src/a11y.rs)
   decides which off-viewport entries the walker still emits. Pick
   `Cooperative` (default) when the visual layout *is* a sensible
   reading order; pick `StrictlyParallel` when AT shape diverges
   meaningfully from visual layout.
2. **Logical structural API.** [`A11yGroup`](../crates/teksilo-scene/src/a11y.rs),
   [`A11yNode`](../crates/teksilo-scene/src/a11y.rs), parents,
   relations, auto-graft, and a focus-order callback let apps
   declare an AT tree that has no visual counterpart. Cards live in
   *Acts*, nodes live in *Subgraphs*, components live in *Layers*.

---

## Off-screen mode

```rust
SceneView::new(scene)
    .a11y_off_screen_mode(A11yOffScreenMode::ViewportOnly)
    // ViewportPlusN { n } (default, n=1) | AllItems | ViewportOnly
```

Decides which items the AT walker emits when the user pans / zooms.
`ViewportPlusN { n: 1 }` (the default) emits items in the viewport
plus one viewport-width margin — giving screen-reader users a
one-screen "lookahead" for navigation. `AllItems` always emits
everything (good for small scenes, < ~500 items). `ViewportOnly`
strictly limits emission to the current viewport (large scenes where
off-screen enumeration would overwhelm AT clients) — strictly, including
against a generous [`retention_margin`](../crates/teksilo-scene/src/view.rs);
see the third consequence below.

### It governs both tiers

It used to govern only the lightweight one. A heavyweight card — a real
`Widget` added with `Scene::add_widget` — was handed to the framework walker no
matter where it was, so `ViewportOnly` on a 10 000-card scene still published
10 000 AccessKit nodes and 10 000 Tab stops, and a card 90 000 px away sat in
the Tab ring between two visible ones. The setting silently did not do the one
thing its documentation is for.

It does now, and through the arena rather than through the walker: a card
outside the region is **parked dormant**, which takes it out of paint, the
layout recursion, the accessibility tree and the Tab ring in one move, and keeps
its focus, text and animation state for when the camera brings it back. Measured
on a scene holding 24 cards on screen (`cargo test -p teksilo-scene --test
heavyweight_retention_probe --release -- --nocapture`):

| off-screen cards | AT nodes | Tab stops | AT walk |
| ---: | ---: | ---: | ---: |
| 0 | 28 | 25 | 39 µs |
| 1 000 | 28 | 25 | 50 µs |
| 20 000 | 28 | 25 | 163 µs |
| 50 000 | 28 | 25 | 371 µs |

Before: 1 028 / 20 028 / 50 028 nodes, 1 025 / 20 025 / 50 025 stops, and
769 µs / 18 165 µs / 45 134 µs of walk.

Three consequences worth knowing:

* **`AllItems` parks nothing.** Its promise — "a complete table of contents" —
  is a promise about reachability, so the region it asks for is unbounded and
  every card stays live. That is the opt-out for a view that wants each card
  enumerable wherever it is.
* **The region a card is kept in is never smaller than the region this mode
  asks to enumerate.** The two are unioned, so the walk can never be asked to
  describe a card the arena has parked, and the published tree can never name a
  node that is not in it. [`SceneView::retention_margin`](../crates/teksilo-scene/src/view.rs)
  widens it further, in screen pixels, so a card wakes shortly *before* it
  becomes visible rather than on the frame it does.
* **…and being kept is not the same as being listed.** That containment is
  one-directional on purpose. Where the retention region is *strictly* wider —
  `ViewportOnly` with a non-zero margin — the difference is a band of cards
  that are alive and **not** enumerated: laid out, holding their state, still
  Tab stops, and absent from the AccessKit tree. `retention_margin` is a
  lifecycle knob and this mode is the accessibility statement; raising the
  first must not quietly widen the second, or `ViewportOnly` would not mean
  what it says. The suppression is done through the view's
  `accessibility_children`, which the framework walker uses for both the child
  push and the recursion — so an unlisted card leaves no node and nothing
  naming one. The default mode is not in this case at all: its one-screen
  reach dwarfs the 96 px default margin, so it lists everything it keeps.
* **A card the user is using is pinned wherever it is** — one holding the
  keyboard focus, a captured pointer or an in-flight drag source. Parking it
  would clear the caret or cancel the selection because the *view* moved, which
  is not something the user asked for. The pin covers the listing too, and has
  to: a published tree names its focused node, so a focus the walk did not emit
  is a broken update rather than a missing one. Focus landing on an unlisted
  card publishes it in the same pass — moving the focus is itself a change to
  what a culling parent was asked, so the pass runs even though nothing moved.

---

## A11y mode

```rust
SceneView::new(scene).a11y_mode(A11yMode::Cooperative)
// Cooperative | StrictlyParallel
```

`Cooperative` (default) — items / widgets without a declared logical
parent appear as direct children of the SceneView in the AT tree.
Pick this for charts, dashboards, simple maps where visual layout
*is* the reading order.

`StrictlyParallel` — only entries placed in the logical tree
(`set_a11y_parent`, `add_a11y_group`) are emitted. Items without an
explicit declaration are **suppressed**. Pick this for corkboards /
graph editors where AT shape should ignore visual layout entirely.

---

## Logical groups

A virtual AT container with no visual counterpart. Pure structure:
no hit-test, no paint.

```rust
let act_one = scene.add_a11y_group(
    A11yGroup::builder()
        .label(tr!(act_one()))
        .role(accesskit::Role::Region),
);
scene.set_a11y_parent(A11yNode::Item(scene_card), Some(A11yNode::Group(act_one)));
```

Groups can themselves nest under other groups via
`set_a11y_parent(A11yNode::Group(child_group), Some(A11yNode::Group(parent_group)))`.
Build arbitrary AT-only trees that have no relationship to the
visual layout.

---

## Reparenting

```rust
scene.set_a11y_parent(A11yNode::Item(child), Some(A11yNode::Item(parent)));
scene.set_a11y_parent(A11yNode::Item(card), None);   // back to root
scene.a11y_parent_of(A11yNode::Item(card)) -> Option<A11yNode>
```

`A11yNode` addresses any node in the parallel tree:

| `A11yNode::*` | Targets |
|---|---|
| `Item(ItemId)` | Any scene entry — lightweight item or heavyweight widget added via `Scene::add_widget` |
| `Group(A11yGroupId)` | A logical group declared via `add_a11y_group` |
| `Widget(WidgetId)` | A real interactive widget addressed by its arena id — typically a *descendant* of a heavyweight scene item that should logically belong elsewhere |

For widgets you added via `Scene::add_widget`, prefer
`A11yNode::Item(item_id)` — the walker handles the heavyweight
auto-graft for you (the real widget's `NodeId` lands under the
declared parent without you doing anything special).

---

## Relations

Cross-tree relationships independent of parenting.

```rust
scene.add_a11y_relation(A11yNode::Item(button), A11yRelation::Controls,    A11yNode::Item(menu));
scene.add_a11y_relation(A11yNode::Item(field),  A11yRelation::DescribedBy, A11yNode::Item(error_msg));
scene.add_a11y_relation(A11yNode::Item(node_a), A11yRelation::FlowTo,      A11yNode::Item(node_b));
```

`A11yRelation` variants:

- `Controls` — `from` controls `to` (button opening a menu).
- `DescribedBy` — `from` is described by `to` (cross-item annotation).
- `LabelledBy` — `from` is labelled by `to` (cross-item label).
- `FlowTo` — logical reading flow from `from` to `to`. Many node-graph
  editors use this so VoiceOver / NVDA "next item" follows
  data-flow order rather than scene-insertion order.

---

## Live regions

Mark a scene entry as a polite or assertive live region:

```rust
scene.set_a11y_live(A11yNode::Item(toast), accesskit::Live::Polite);
```

Updates to the entry's AT name / value are announced.

---

## Landmark roles

Promote a group to a landmark for screen-reader navigation:

```rust
scene.set_a11y_landmark(A11yNode::Group(toolbar_group), accesskit::Role::Toolbar);
```

---

## Categories (rotor / quick-nav)

App-defined tags surfaced to AT clients that support categorized
navigation (VoiceOver rotor on macOS, NVDA quick-nav on Windows).
Apps coin their own category names — `"node"`, `"connector"`,
`"comment"` — and bucket items into them:

```rust
scene.set_a11y_categories(A11yNode::Item(node), &[A11yCategory::new("node")]);
scene.set_a11y_categories(A11yNode::Item(edge), &[A11yCategory::new("connector")]);
```

---

## Subtree mode for items

Each `SceneItem` builder carries an
[`AccessSubtreeMode`](../crates/teksilo-scene/src/items.rs):

| Mode | Effect |
|---|---|
| `Inherit` (default) | Descendants emit AT nodes normally. |
| `Exclude` | Descendants are pruned from the AT tree. |
| `Merge` | Descendants' label / value / actions concatenate into this item's AT node and they're pruned individually. The subtree reads as one element. |

```rust
RectItem::new(rect)
    .label(tr!(card_idea_1()))
    .access_merge_subtree();    // card with rect + label + indicator dot reads as one
```

`Merge` is the right pattern for a card whose visual subparts
(background rect, label, status dot) are conceptually one element
for the AT user. `Exclude` is useful for animated decorations whose
emission would be noisy (a pulsing recording dot, a spinner).

---

## Override chain (`access_*` builders)

Every built-in item's builder, every custom item that invokes the
`item_a11y_builders!()` macro, and `A11yGroupBuilder` / `SceneView`
expose a parallel `.access_*` chain that mirrors `WidgetBuilder` on
the widget tier:

```rust
RectItem::new(rect)
    .access_label(tr!(save()))
    .access_description(tr!(save_explanation()))
    .access_role(accesskit::Role::Button)
    .access_subtree(AccessSubtreeMode::Merge);
```

`access_label_literal`, `access_description_literal` (and friends)
are `#[doc(hidden)]` twins for explicitly-untranslated strings.

Data-bearing items — gauges, value marks, progress dots — additionally carry
`access_value` / `access_numeric_value` / `access_numeric_range` /
`access_numeric_step`, mirroring the widget-tier numeric-range overrides:

```rust
RectItem::new(gauge_rect)
    .access_label(tr!(cpu_load()))
    .access_value(lit!("42 %"))
    .access_numeric_value(0.42)
    .access_numeric_range(0.0, 1.0)
    .access_numeric_step(0.01);
```

`access_value` announces a formatted string reading (e.g. `"42 %"`);
`access_numeric_value` / `access_numeric_range` / `access_numeric_step`
populate AccessKit's numeric-value fields so a screen reader can describe
magnitude and bounds, not just a label.

---

## Custom focus order

Apps that need a focus traversal that diverges from scene-insertion
order install a callback. Common cases:

- Story corkboards — Tab follows Acts → Scene cards in story order.
- Node-graph editors — Tab follows data-flow order via `FlowTo`
  relations.
- CAD canvases — Tab follows depth-then-breadth tree order.
- Timelines — Tab follows chronological order.

```rust
SceneView::new(scene).focus_order(|scene, dir, current| {
    // dir: FocusDirection::{Forward, Backward}
    // current: Option<ItemId> — None on initial Tab
    match dir {
        FocusDirection::Forward => next_in_my_order(scene, current),
        FocusDirection::Backward => prev_in_my_order(scene, current),
    }
});
```

The callback is `Fn(&Scene, FocusDirection, Option<ItemId>) -> Option<ItemId>`.
Returning `None` ends the cycle (the focus exits the SceneView and
moves to the next focusable in the parent).

When the focused item is off-viewport, the SceneView calls
[`ensure_visible`](teksilo-scene.md#background--foreground-hooks)
automatically so the focus indicator stays on screen.

---

## SceneView own AT name + nesting

```rust
SceneView::new(scene)
    .a11y_label(tr!(graph_data_area()))
    .nested_a11y(true)              // emit Role::Region instead of Role::Pane
```

`Pane` is the right role for a top-level scene; `Region` is for an
inner scene inside another (a chart's data area inside a chart's
chrome). Switch via `nested_a11y(true)`.

---

## One coordinate space, declared once

**Every rectangle in the scene subtree is published in scene coordinates, and
the camera is declared once as an AccessKit node transform at the top of that
subtree.** There is no knob here and nothing for an app to configure; it is
worth knowing because it decides what the numbers mean when you read a
published tree, and what a `SceneItem` implementation must emit.

A `SceneView` is a fixed viewport over content in a coordinate system of its
own. A heavyweight card at scene (100, 100) keeps arena bounds of (100, 100)
however far the camera has panned — deliberately, so a card parked off-screen
still has a canonical position to come back to — and a lightweight item's
rectangle is stored the same way. Neither is a window-space rectangle.

AccessKit has the mechanism for exactly this. A node's `transform` applies "to
any coordinates within this node and its descendants, including the `bounds`
property of this node", and `bounds` are read "in the coordinate space of the
nearest ancestor with a non-`None` transform". So one declaration at the
boundary carries the camera to everything below it, and the consumer every
platform adapter is built on does the composing:

- a **heavyweight card**, being an ordinary arena child of a content-transform
  node, gets the declaration from the framework walker — which is a general
  rule about `BuildContext::set_content_transform`, not a scene special case;
- a **lightweight item** and a **logical group** get it from the scene's own
  emitter, on the nodes it attaches directly to the `SceneView`'s node;
- everything deeper — a label inside a card, a text run's per-character
  positions, a magnet's anchor — inherits it and emits nothing of its own.

Three things follow, and they are what the arrangement is *for*:

- **The two tiers are in one space.** A card and an item at the same scene
  position advertise the same screen rectangle because they are described
  identically, not because two projections round the same way.
- **A rotated camera is exact.** `Rect` is axis-aligned, so projecting by hand
  yields a bounding box and loses the shape; declaring the transform hands
  AccessKit the rectangle and the mapping, and lets it take the bounding box at
  the last moment — which is what it does with the answer anyway.
- **Explore-by-touch works.** `accesskit_consumer`'s hit test descends by
  inverting each node's transform, so a probe aimed at the painted position
  reaches the card. A pre-projected subtree would be unreachable.

Reading it back: `Node::bounds()` is the raw half and is in **scene**
coordinates. `NodeRef::bounding_box()` is the composed answer, in physical
pixels (the root carries the device scale).
`teksilo_core::accessibility::audit::logical_bounds` is the same composition
stopping short of the root, for a caller that works in logical pixels — an
automation probe aiming a synthetic press, a diagnostic overlay.

Writing it: a `SceneItem` implementation gets
[`SceneItemA11yContext`](../crates/teksilo-scene/src/item.rs) carrying
`scene_bounds` and `local_to_scene`, and **must never multiply an emitted
rectangle by the view transform** — the same rule the framework states for the
device scale factor, one level down. Doing so applies the camera twice.

---

## Runtime mutation — the AT tree follows

The logical AT tree is *separate* from the visual scene, so it needs its own
notification path when the scene changes after mount. Two channels feed the
`SceneView`'s reconcile pass:

- `Scene::item_change_signal` — every item mutation (add / remove / move /
  transform / visibility / opacity / z / layer / reparent). The new card
  materialises, a removed one is destroyed and its AT maps cleaned, a moved one
  gets its fresh scene-space AT bounds.
- `Scene::a11y_change_signal` — *pure* logical-AT mutations that change no item
  geometry (`add_a11y_group`, `set_a11y_parent`, `add_a11y_relation`,
  `set_a11y_live`, `set_a11y_landmark`, `set_a11y_categories`). Without this a
  runtime group add or reparent would be invisible to assistive tech.

A **camera** change is a third path, and it needs one because it changes no
structure, no focus and no arena bounds: the view binds its own
`view_transform_signal` at `BindingLevel::AccessibilityOnly`, which flips
`a11y_dirty` and nothing else. Without it `sync_accessibility` — the function
every platform adapter is fed, as opposed to `accessibility_tree_snapshot` —
kept serving the tree from before the pan, and the published rectangles stood
still for both tiers however far the camera had gone. One derived signal rather
than four: it changes once per camera change however many of pan / zoom /
rotation moved.

**An observer sees a `SceneChange`, not a bare `ItemChange`.** The item channel
carries the change plus the transaction it belongs to, whose it was, whether it
counts as history, and whether it is a per-frame artefact — see
[teksilo-scene.md](teksilo-scene.md) → *The reversible-mutation seam*. Nothing
about the AT walk changes; the envelope is for the data layer, and the
`a11y_change_signal` is a bare counter as before.

A relayout no longer re-walks the AccessKit tree by itself (the walk is cached,
gated on `a11y_dirty`). `SceneView::build()` calls
`ctx.request_accessibility_update()` when it reconciles, which flips that flag —
so any runtime change to the visual *or* logical tree reaches a screen reader on
the next frame. The request is **gated on a `Scene::mutation_version` delta**:
both channels above advance that counter, so a discrete add / remove / move /
reparent / group / relation / live / landmark change always re-walks AT. What it
*won't* do is re-walk AT 60×/s while an `add_item_dynamic` item animates its
bounds — that per-frame churn is suppressed (a screen reader can't use sub-pixel
bounds updates), and the **final** bounds are walked in once when the animation
settles. `Scene::remove` additionally re-roots any still-alive node that was
AT-parented under a removed item (its explicit parent mapping is dropped, exactly
like `remove_a11y_group`). Mark a runtime-added group `Live::Polite` to have the
addition announced. Demo: the "Add Act" button in `cargo run -p scene-corkboard`.

### An undone deletion restores the semantics, not just the pixels

A removal destroys the item's whole slice of the logical AT tree — its AT
parent, its relations, its live-region status, its landmark role, its rotor
categories — and it does so *before* it announces `ItemChange::Removed`. So an
app that reconstructed the item from the event alone would put the rectangle
back and silently lose all five. On a crate whose differentiator is per-item
accessibility, that is the failure worth naming.

`Scene::take(id)` lifts those decorations out instead of dropping them, and
`Scene::restore` puts them back **at the same `ItemId`** — which the whole
logical tree is keyed by, so a restore that minted a fresh id would re-root the
item, break every relation naming it and orphan whatever was AT-parented under
it. `Scene::remove` is the same call with the salvage routed to the edit sink,
so an app with a data layer gets the same completeness without changing which
door it calls.

**Both directions of every edge**, because a removal cuts both:

| edge | what `remove` does | what a restore does |
| --- | --- | --- |
| the item's own AT parent (`removed → parent`) | drops it | re-inserts it, if the parent is still there |
| a **survivor** AT-parented under it (`survivor → removed`) | drops it, re-rooting that survivor at the view root | re-adopts the survivor |
| a relation at either endpoint | drops it | re-attaches it, if both ends are alive |
| live / landmark / categories | drops them | re-inserts them |

Recording only the first row would make a restore silently fail to re-adopt the
survivors — failing at exactly the case the salvage exists for. An edge whose
far end has since been removed is **not** re-attached: re-inserting it would
name a node the walker cannot resolve, which is the dangling-reference class of
defect this tree must not have. Edges are attached once the whole batch is in,
so an edge between two items of one removed subtree survives whatever order the
two ends land in.

The entry also returns to its recorded place in **declaration order** — the
order this walk publishes siblings in, i.e. the order a screen reader reads the
scene in. Appending it would move the item to the end of that reading order for
no reason a user could see.

One heavyweight caveat, stated plainly: a single-view (`Scene::add_widget`)
card whose instance a view has already reaped comes back as an entry with no
widget to materialise, so it is in the scene and in no AT tree.
`RemovedItem::widget_instance_present()` reports it, and content that must
survive an undo goes in through `SceneModel::add_widget_item`, whose payload
every view rebuilds from.

**Multi-view.** When several `SceneView`s share one `SceneModel` (see
[teksilo-scene.md](teksilo-scene.md) → *Shared model & multi-view*), each pane
installs its **own** observers on these two channels and walks its **own**
AccessKit subtree — the gate (`mutation_version` delta) is per-view, and each
pane's synthetic AT nodes declare *that* pane's view transform. A mutation on
the shared model therefore reaches assistive tech for every pane independently.
A heavyweight item added via `add_widget_item` is a type-erased payload, so each
pane's delegate builds its own widget — and the item's `accessibility()` runs
once per pane. The rectangles it emits are the same in every pane, because they
are the scene's; it is the declaration above them that differs.

---

## `SceneCard`'s shape

A [`SceneCard`](../crates/teksilo-scene/src/scene_card.rs) publishes **one**
`Role::Group`, named by `SceneCard::label`, carrying `selected` when its
[`CardMode`] says so and one custom action — **Edit** — which is the non-pointer
twin of the double-click that enters edit mode. It is a tab stop, so a keyboard
user reaches every card with Tab, enters one with Enter and leaves with Esc.

The action is **advertised**, not merely listed, and that distinction is the
whole of whether it exists. An adapter reports a node's custom actions through
`Action::CustomAction` being supported, not through the list being non-empty
(`accesskit_ios-0.2.0/src/node.rs:109` is the one that says so in code), so a
list published without the gate is decoration: named, announced by nothing,
invokable by nobody. Three widgets in this repo shipped that defect, each having
to remember a second call beside `set_custom_actions`. It is now
`AccessNodeBuilder::set_custom_actions` that owns both halves — a non-empty list
advertises the gate, an empty one withdraws it — so a fourth is not reachable.
Assert the gate, never the list: `supports_action(Action::CustomAction)` is what
`the_cards_edit_action_is_advertised_and_not_merely_listed` checks.

One group, not two. The card's default surface is
`teksilo_widgets::Card`, which announces itself as a `Role::Group`; left alone
that is a nameless container *inside* the card's own named one, which a screen
reader reads as a container within a container. So the card asks for
`Role::GenericContainer` on the surface, which is the role the walker prunes,
promoting its children in order. The net result is **node for node identical to
the hand-rolled `Panel` a note page used before** — with the container named,
where the panel's was not. Pinned by
`a_card_publishes_no_more_nodes_than_the_hand_rolled_panel_it_replaces`
(asserted through `sync_accessibility`, the cached door a platform adapter is
handed, not the snapshot that bypasses it).

Everything inside the card is walked by the framework's ordinary walker — the
header's text, the trailing button and a body that publishes `Role::TextRun`
children all hang off the group in the emitted tree.

> ⚠ **They do not currently reach a screen reader, and the card is not what
> stops them.** The default surface is `teksilo_widgets::Card`, whose
> `RecipeCardStyle` body calls `AccessNodeBuilder::set_hidden()` on itself to
> mean "presentational". `set_hidden` is `FilterResult::ExcludeSubtree` in
> `accesskit_consumer::common_filter` — the filter every platform adapter and
> this repo's own `accessibility::audit` read through — so it removes the
> surface **and everything under it**. Measured through a real consumer tree:
>
> ```text
> SceneCard  →  Window > Pane > Group "Note"          (title and body gone)
> Panel      →  Window > Group                        (its children gone)
> StatusBar  →  Window > Status "Status"              (its items gone)
> ```
>
> The parity test below passes because the `Panel` baseline is broken in exactly
> the same way. The intended "presentational" is one line away and the card
> already uses it on the surface's *own* node: `Role::GenericContainer` with no
> properties is `ExcludeNode` — the node goes, its children are promoted.
> Fixing it is a sweep across every content-wrapping recipe surface
> (`recipe_card_style.rs`, `panel.rs`'s `a11y_presentational`, `popover_surface.rs`,
> `tool_box.rs`, `splitter.rs`, `aspect_ratio.rs`, …) rather than a change to the
> card, so it is recorded here rather than made here.

### Tab stops, and the one thing the card does not decide

The card contributes exactly **one** tab stop — itself. It does not take its
body's away while idle, and the reason is mechanical rather than a matter of
taste: `set_tab_stop` reaches one node, and a composite body's tab stops are its
own inner nodes. Nothing short of parking the body dormant takes a subtree out
of the Tab ring.

So a card with a focusable body is two stops, always. An app that wants one stop
per idle note puts a `Switcher` in the body — a read-only viewer and an editor,
driven by the same `CardMode` — because a `Switcher` parks its hidden branch
dormant, which takes it out of focus, hit-testing *and* the accessibility tree
while keeping it mounted, so a mode round-trip does not destroy the caret:

```rust
SceneCard::new(model.clone(), id)
    .mode(mode.clone())
    .body(
        Switcher::new(mode.map(|m| usize::from(*m == CardMode::Editing)))
            .child(RichTextEditor::read_only(doc.clone()))
            .child(RichTextEditor::editor(doc.clone())),
    )
```

That branch is parked until the `visible_when` gate is evaluated in the layout
pass *after* the one that flips the mode — which is after the activating
dispatch has already drained its focus request. So the card asks for the focus
twice: once in the dispatch, for a plain focusable body, and again from
`BuildContext::run_after_mount` on the far side of that pass. Without the second
ask a double-click on a note leaves the keyboard on the card and the caret never
appears, and the two shapes this section recommends — one idle tab stop, and
focus on activation — would be mutually exclusive. The second ask is skipped
when `focus_within` already says the keyboard is inside the card, so an
`on_activate` that placed it deliberately is not overruled.

### What content-driven height does *not* cost

A card under `SizePolicy::HeightForWidth` is measured only when it is given a
non-zero size, so the policy adds no AT nodes and moves none: the AT rectangles
come from the placements, and the placements are recomputed by the relayout the
measurement causes. The write-back is counted out of `Scene::structural_version`,
so it triggers no re-walk of its own.

The honest edge is an `A11yOffScreenMode` wide enough to publish cards outside
the viewport. Those are laid out at `Size::ZERO` and never measured, so what a
screen reader is told about them is the estimate `add_widget_item` was handed —
the same number every other whole-scene query reads for an unrealised card, and
correct the moment the camera reaches one.

Changing the policy *does* re-walk, once, because it is a real decision about
the document and the rectangles that follow from it are new.

[`CardMode`]: ../crates/teksilo-scene/src/scene_card.rs

---

## Worked example: story corkboard

Acts contain Scene cards. Acts are virtual groups; Scene cards are
heavyweight widgets. AT shape ignores visual layout entirely.

```rust
let mut scene = Scene::new();
let act1 = scene.add_a11y_group(A11yGroup::builder().label(tr!(act_1())));
let act2 = scene.add_a11y_group(A11yGroup::builder().label(tr!(act_2())));

let scene_card_1 = scene.add_widget(card_widget("Opening"), Rect::new(0.0, 0.0, 200.0, 120.0));
let scene_card_2 = scene.add_widget(card_widget("Climax"),  Rect::new(220.0, 0.0, 200.0, 120.0));

scene.set_a11y_parent(A11yNode::Item(scene_card_1), Some(A11yNode::Group(act1)));
scene.set_a11y_parent(A11yNode::Item(scene_card_2), Some(A11yNode::Group(act2)));

let view = SceneView::new(scene)
    .a11y_mode(A11yMode::StrictlyParallel)   // ignore visual layout entirely
    .focus_order(|scene, dir, current| story_order_traversal(scene, dir, current));
```

Screen-reader output: "Act 1, Region. Opening, Card. Act 2, Region.
Climax, Card." Tab cycles in story order regardless of where the
cards sit visually.

---

## Magnetism

When a view has magnetism enabled (`SceneView::magnetism(...)`), each
enabled magnet on a **lightweight** item is emitted as a synthetic
`SyntheticKind::SceneMagnet` AT node, a child of the owning item's node,
with `Role::Button` and the magnet's `label` as its name (falling back to
a generic name when unset). This makes anchors screen-reader perceivable
and gives the keyboard connect flow a focus target. Adding, removing, or
enabling a magnet bumps the scene's `a11y_change_signal`, so the AT tree
re-walks with no extra wiring.

The keyboard connect flow uses the roving-`active_descendant` pattern:
the SceneView keeps real arena focus, and while in connect mode it points
its `active_descendant` at the focused magnet's synthetic node, so a
screen reader announces the focused anchor as the user arrows through
them. (The grid-cell roving pattern, applied to scene anchors.)

Connections themselves are **consumer-owned** in AT, exactly as in the
scene model: from your `on_connect`, declare the connection's meaning on
the relation layer, e.g.

```rust
scene.add_a11y_relation(
    A11yNode::Item(source_node),
    A11yRelation::FlowTo,            // or Controls
    A11yNode::Item(target_node),
);
```

Scene provides the relations API; it does not invent connection meaning.
Magnet AT nodes for **heavyweight**-item magnets are a follow-up; the
keyboard state machine and `on_connect` still work for them, only the
`active_descendant` announcement is limited to lightweight-item magnets.

Demo: `cargo run -p scene-magnetism` (a fully keyboard- and
screen-reader-operable node graph).

---

## Worked example: graph editor

Nodes contain Ports; connector lines declare data flow via `FlowTo`.

```rust
let mut scene = Scene::new();

let node_a = scene.add_widget(node_widget("A"), Rect::new(0.0, 0.0, 120.0, 80.0));
let node_b = scene.add_widget(node_widget("B"), Rect::new(300.0, 0.0, 120.0, 80.0));

// Connector lines are lightweight PathItems.
let edge = scene.add_item(
    PathItem::new(connector_path()).stroke(Color::BLACK, 2.0),
    Point::ZERO,
);
scene.set_a11y_categories(A11yNode::Item(edge),  &[A11yCategory::new("connector")]);
scene.set_a11y_categories(A11yNode::Item(node_a), &[A11yCategory::new("node")]);
scene.set_a11y_categories(A11yNode::Item(node_b), &[A11yCategory::new("node")]);

// Logical flow: data flows A → B.
scene.add_a11y_relation(
    A11yNode::Item(node_a), A11yRelation::FlowTo, A11yNode::Item(node_b),
);

let view = SceneView::new(scene)
    .focus_order(|scene, dir, current| flow_order_traversal(scene, dir, current));
```

VoiceOver rotor offers "Nodes" and "Connectors" categories; "next
item" via the rotor follows the user's chosen category.

---

## Worked example: CAD canvas

Components belong to Layers. Layers are virtual groups. AT bounds
are reported in **scene** coordinates so AT users can reason about
"the gear is at (150, 200) in the design" regardless of pan / zoom.

```rust
let mut scene = Scene::new();
let layer_drive = scene.add_a11y_group(A11yGroup::builder().label(tr!(drive_layer())));
let layer_frame = scene.add_a11y_group(A11yGroup::builder().label(tr!(frame_layer())));

let gear = scene.add_widget(gear_widget(), Rect::new(150.0, 200.0, 60.0, 60.0));
scene.set_a11y_parent(A11yNode::Item(gear), Some(A11yNode::Group(layer_drive)));

let beam = scene.add_widget(beam_widget(), Rect::new(0.0, 280.0, 400.0, 20.0));
scene.set_a11y_parent(A11yNode::Item(beam), Some(A11yNode::Group(layer_frame)));

let view = SceneView::new(scene)
    .a11y_mode(A11yMode::StrictlyParallel)
    .nested_a11y(true)
    .a11y_label(tr!(design_canvas()));
```

A CAD client that wants to reason about "where in the design" rather than
"where on the monitor" reads each node's raw `bounds`, which are already the
design's own coordinates, and the view transform from the node's `transform`.
Both are published for every scene node; see *One coordinate space, declared
once* above.

---

## The minimap

[`SceneMinimap`](../crates/teksilo-scene/src/minimap.rs) is a sibling of the
view, not part of its two-tier tree, so it answers for itself — and it has to,
because click-to-recentre is its only function.

It emits a `Role::Group` named `"Scene minimap"` whose **value** is where the
viewport sits:

> Viewport at 42% across, 17% down; showing 25% of the width and 33% of the height

Text rather than `numeric_value` because a 2-D position has no ARIA role and
four numbers do not fit in one field — the reasoning `HsvCanvas` uses for the
saturation-and-brightness field, which is this widget's nearest relative in the
workspace. Read off the same `effective_extent` the picture is projected
through, and refreshed by an `AccessibilityOnly` binding on the viewport signal
beside the `RepaintOnly` one, so panning updates the words as well as the
overlay without a rebuild. `access_readout(|MinimapReadout| …)` replaces the
phrasing (that is the `tr!` seam); `.access_label(tr!(…))` renames the node
through the framework's ordinary override chain.

With an `on_click` callback installed it is focusable and advertises five
actions, all landing in that callback:

| | keyboard | assistive technology |
| --- | --- | --- |
| move the view | arrows; `Shift` for a whole viewport | `ScrollLeft` / `ScrollRight` / `ScrollUp` / `ScrollDown` |
| centre on the content | `Home`, `Enter`, `Space` | `Click` |

`Click` is the position-free half of the tap — an AT client has no point to give
it, so the primary action is the one destination the widget can name on its own.

Both of those routes announce the resulting position through
[`EventContext::announce`], because an arrow press on an unannotated graphic is
otherwise completely silent; the announcement describes the move that was
*asked for*, since the app owns whether and when the pan lands.

The **pointer** route does not announce. The tap has feedback the other two
lack — the user picked the destination by aiming at the picture — and
click-to-recentre is a gesture people repeat, so an utterance per click is a
metronome over whatever was being read. That matches `HsvCanvas`, which
announces from its arrows and its custom actions and not from its drag, and the
data views, whose row reorder announces from the *non-drag* alternative while
the drop is silent. The workspace's one speaking pointer route is the charts'
readout, and only for a coarse pointer that pressed and released without
travelling: that tap is an *inspection* standing in for a hover a finger cannot
perform, so the utterance is its whole product. A minimap tap is a command, so
the exception does not reach it. The node's value carries the new position for
any client that asks.

With **no** callback the node is still emitted — it is a useful read-out — but
it takes no focus and advertises nothing. Reaching it and focusing it are
separate questions: a named, valued node is reached by object / browse
navigation and by the rotor, while Tab is for things you can operate, and a
read-only minimap answers Tab with no arrow, no `Enter` and no action. That is
the dead stop the ARIA practices warn about. An app that wants it in the Tab
order anyway says `.focusable(true)` through the framework's ordinary
`WidgetBuilder` chain — the default is an opinion, not a refusal.

Pinned by [`crates/teksilo-scene/tests/minimap_a11y.rs`](../crates/teksilo-scene/tests/minimap_a11y.rs),
which asserts reachability with `test_api::tab_stops_within` rather than by
focusing the node: `WidgetTree::focus(id)` does not check `focusable`, so
focusing proves the handlers run and nothing about a keyboard user getting
there.

[`EventContext::announce`]: ../crates/teksilo-core/src/announcer.rs

---

## What an overlay may take

Everything positional in the framework resolves from the **arena's** hit
target, not from whichever handler happened to answer: press feedback,
focus-on-release, the touch hold route that opens a tooltip or a context menu,
the cursor, and a drag. So a scene that wins a tap in a handler while the arena
hands the same point to something else does not merely get the tap wrong — it
splits the tap off from the press, the focus and the hold.

That is fixed by making the two agree (see
[Cross-tier hit-testing](teksilo-scene.md#cross-tier-hit-testing)), and the rule
that decides *which* answer they agree on is:

> **A lightweight entry takes a press away from a heavyweight card only if it
> would act on that press** — a tap, a double tap, a context menu, or
> `IS_DRAGGABLE`.

The case that forces it is the one this crate exists to serve. An embedded
`TextInput` note with a **selection halo drawn `Over` it**: the halo is
decoration, it claims nothing, so it never takes the point. Clicking it leaves
the note pressed and focused. Had the rule been "whatever is painted on top
occludes", every existing scene with a decorative foreground would have started
answering the viewport instead of its content.

A **hover** affordance — `on_hover`, `cursor`, `tooltip` — is deliberately not
on that list, and the reason is an accessibility one as much as a correctness
one. Counting one as a claim reads as conservative and is the opposite: the veto
takes the card out of the arena's walk, the view becomes the target, and the tap
then resolves to an item that has no `on_tap`, so the press reaches *nobody* —
the note under a `.tooltip("drop here")` hint region becomes un-clickable,
un-focusable and un-editable, with no error and no handler firing. The
affordances themselves lose nothing: the view's hover seam runs in the preview
pass, on every strict ancestor of the target, so it reaches the item whether a
card won the walk or not, and the cursor is arbitrated on the cursor channel
alone (an `Over` item's declared cursor wins over the card's, and costs the card
no press).

The converse holds too, and is equally deliberate: an `Over` item that *is*
interactive takes the press, the focus and the hold as well as the tap, because
it is what the user aimed at.

**The veto moves no node.** It refuses a *point*; it does not remove anything.
The AccessKit tree, its bounds, and the Tab order are byte-identical with and
without it — a card under an interactive overlay is still keyboard-reachable,
still announced, still has its full advertised bounds. Pinned by
`view::tests::pick_order::the_veto_changes_neither_the_at_tree_nor_the_tab_stops`,
which measures the AT node count and the tab stops as a **delta** between a
decorative overlay and a claiming one over the same card, and by
`a_decorative_over_item_leaves_the_card_its_press_and_its_focus`, which asserts
the arena's own hit target as well as where focus landed.

## Where the pointer and the AT probe still disagree

The rule above governs the **pointer**. It does not reach explore by touch, and
it is worth being exact about that, because the opposite is easy to assume.

A platform explore-by-touch probe resolves through the **AccessKit tree**:
`accesskit_consumer::node_at_point` descends from the root, walking each node's
children **in reverse** and taking the first whose bounds contain the point. The
press-claiming rule changes no node, no bound and no child list, so it cannot
move that answer — and the scene's AT child list is in **emission** order, not
paint order:

```
SceneView (Role::Pane)
  ├── groups                     (structure first)
  ├── lightweight item nodes     ← both bands, in insertion order
  └── heavyweight widget nodes   ← appended by the framework walker, last
```

Reverse walk therefore reaches the heavyweight tier **first, always**, whatever
band the lightweight entry is in. Measured on a note with a 100×100 overlay
exactly on top of it:

| overlay | tap | arena hit + focus | AT probe |
|---------|-----|-------------------|----------|
| decorative halo | the note | the note | the note |
| claiming badge  | the badge | the `SceneView` | the note |

The decorative row — the corkboard case, and the one the rule was chosen for —
agrees on all three. The claiming row does not: the sighted user activates the
badge, the press and focus go to the viewport, and the probe announces the note
underneath. (That the item's own node *is* reachable is not in doubt: probing a
point the overlay covers and the card does not returns the overlay's
`Role::GraphicsObject`. The card wins purely by being emitted later.)

Two separate things are wrong there, and only one of them is a scene defect.

**The viewport is unnamed.** A `SceneView` announces as a bare `Role::Pane`
unless the app calls [`SceneView::a11y_label`]. Name it. A view that owns
interactive `Over` chrome should always be named, because the veto makes it the
focus target for those points by design.

**The AT child order is not the paint order.** This is the real gap, and it is
the AT tier's version of exactly the bug the pointer tier just fixed: two
pickers over one area, ordered by two different rules. The fix is to emit the
`Over` band *after* the heavyweight children, which the scene cannot do today —
`accessibility()` runs before the framework walker descends, so everything the
scene pushes necessarily lands before every card. Closing it needs one of:

- an AT-walker slot in `teksilo-core` that emits a widget's own nodes **after**
  its children — the accessibility analogue of `Widget::post_paint`, which the
  render walker already has and which is what lets the `Over` band paint last in
  the first place; or
- the scene taking over emission of its heavyweight entries into its own logical
  tree (the machinery exists — it is what the auto-graft path already does for
  an entry with a declared logical parent) and ordering every entry by
  `PaintKey`.

It is not done here because it is not a hit-test change: an AccessKit child list
is also the **reading order**, so either route reorders how a screen reader walks
the scene — today structure, then items, then cards; afterwards, interleaved by
z. That is an architecture decision about the AT tree's shape, with an existing
app-facing surface to reconcile (`A11yMode`, `SceneView::focus_order_callback`),
and it belongs with whoever owns that surface rather than inside a picker fix.
The table above is pinned by
`view::tests::pick_order::what_the_press_claiming_rule_does_and_does_not_reconcile`,
so the day it changes, it changes deliberately.

---

## Reference

- Implementation: [`crates/teksilo-scene/src/a11y.rs`](../crates/teksilo-scene/src/a11y.rs),
  [`crates/teksilo-scene/src/scene.rs`](../crates/teksilo-scene/src/scene.rs)
  (the `Scene::add_a11y_*` / `set_a11y_*` API), and the AT walker in
  [`crates/teksilo-scene/src/view.rs`](../crates/teksilo-scene/src/view.rs).
- Widget-tier override surface: [`docs/accessibility-overrides.md`](accessibility-overrides.md).
- Agent/CI automation over this AT surface: [`docs/automation-mcp.md`](automation-mcp.md).
- AccessKit reference: <https://accesskit.dev>.
- `SceneView::a11y_label`: [`crates/teksilo-scene/src/view/builder_impl.rs`](../crates/teksilo-scene/src/view/builder_impl.rs).
