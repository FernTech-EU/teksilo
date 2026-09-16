<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SceneSelectionMode

Selection model for `Scene` items.

Mirrors the API of `teksilo_data::SelectionModel` but keyed by
`ItemId` instead of `usize` — the natural address for scene
entries. Click-to-select, Ctrl+click toggle, Shift+click range,
and marquee box-select all flow through this single model;
`SceneView` paints a marquee overlay during the drag and
commits the result via `Scene::items_in_region`,
so a rubber band picks items by the same geometry a click does.

The selection set is exposed as a `Signal<BTreeSet<ItemId>>`
so `SceneItem` paint code can render selected items differently
by binding their colors / strokes to a derived signal:

```
# use teksilo_scene::{SceneModel, SceneSelection, SceneSelectionMode};
# use teksilo_canvas::{Point, Rect};
# use teksilo_tokens::Color;
# let model = SceneModel::new();
# let card_id = model.add_item(teksilo_scene::RectItem::new(Rect::new(0.0, 0.0, 100.0, 80.0)), Point::ZERO);
let selection = SceneSelection::new(SceneSelectionMode::Multi);
let selected = selection.selection_signal();
let stroke_color = selected.map(move |s| {
    if s.contains(&card_id) { Color::BLUE } else { Color::TRANSPARENT }
});
```

## Builder methods at a glance

`mode`, `selection_signal`, `is_selected`, `selected`, `count`, `clear`, `select_one`, `toggle`, `replace`, `extend`, `commit_marquee`, `commit_marquee_region`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/selection/index.html)

## `pub enum SceneSelectionMode`

Selection-mode discriminator. Mirrors `teksilo_data::SelectionMode`.

```rust
pub enum SceneSelectionMode { /* variants */ }
```

### Variants

- **`None`** — Selection disabled. Click does nothing, marquee does nothing.
- **`Single`** — At most one item selected at a time.
- **`Multi`** — Multiple items can be selected; Ctrl+click toggles, Shift+click extends a range from the anchor.

## `pub struct SceneSelection`

Reactive selection state for a `Scene`.

Cheap-to-clone via `Rc` internals — all clones share the same
underlying signal. Pass clones into widget closures or item
`register_bindings` impls without worrying about ownership.

```rust
pub struct SceneSelection { /* fields */ }
```

### Methods

#### `pub fn new(mode: SceneSelectionMode) -> Self`

New selection model with the given mode. Initially empty,
no anchor.

#### `pub fn mode(&self) -> SceneSelectionMode`

The configured selection mode.

#### `pub fn selection_signal(&self) -> Signal<BTreeSet<ItemId>>`

Live selection signal. Bind reactive consumers (item paint,
status-bar item-count labels) to this.

#### `pub fn is_selected(&self, id: ItemId) -> bool`

Whether the given item id is currently selected.

#### `pub fn selected(&self) -> Vec<ItemId>`

Selected item ids in sorted order.

#### `pub fn count(&self) -> usize`

Number of selected items.

#### `pub fn clear(&self)`

Clear the selection. The anchor is also cleared so a
subsequent Shift+click extends from a fresh starting point.

#### `pub fn select_one(&self, id: ItemId)`

Replace the selection with a single item; sets the anchor
for subsequent range extension. No-op in `None` mode.

#### `pub fn toggle(&self, id: ItemId)`

Toggle membership for the given id (Ctrl+click semantic).
Sets the anchor on toggle-on; leaves it unchanged on
toggle-off. No-op in `None` mode; in `Single` mode behaves
like `select_one` if the item is currently unselected, or
`clear` if it is.

#### `pub fn replace(&self, ids: impl IntoIterator<Item = ItemId>)`

Replace the selection with the given set of ids. Used by
marquee on commit. Anchor is cleared. No-op in `None`
mode; in `Single` mode keeps at most one (the first id in
`ids`).

#### `pub fn extend(&self, ids: impl IntoIterator<Item = ItemId>)`

Add `ids` to the existing selection (marquee with
Ctrl-modifier — additive box-select). No-op in `None` mode;
in `Single` mode reduces to `select_one(last)`.

#### `pub fn commit_marquee(&self, scene: &Scene, marquee_rect: Rect, additive: bool)`

Marquee commit helper: replace (or extend, if `additive`)
the selection with every selectable scene item the rectangle picks up.

Sugar for `commit_marquee_region` with a
rectangular region, `ItemSelectionMode::IntersectsItemShape` and unit
view scale.

# Behaviour change

This used to be a pure AABB query. It now consults each item's
`shape`, so a band that merely grazes a
connector's bounding box no longer selects the connector — it has to
cross the stroke. For an item with an identity transform and the
default box shape the result is unchanged; for a **rotated** or scaled
item it is tighter, because the shape test happens in the item's own
frame rather than against its enlarged scene-space hull. Pass
`ItemSelectionMode::IntersectsItemBoundingRect` to
`commit_marquee_region` to get the old rule back.

#### `pub fn commit_marquee_region( &self, scene: &Scene, region: &SceneRegion, mode: ItemSelectionMode, view_scale: f32, additive: bool, )`

Marquee commit over an arbitrary `SceneRegion` and
`ItemSelectionMode` — the rotated-view rubber band (an exact
quadrilateral) and the freehand lasso both arrive here.

Lightweight items and heavyweight widget entries are both candidates —
the spatial index returns ids regardless of kind, and a widget entry's
shape is its box (see
`Scene::item_shape`). `view_scale` is the
live zoom, consulted only by a cosmetic stroke band.
