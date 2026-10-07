<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# AccessSubtreeMode

Built-in `SceneItem` implementations.

## Public types

| Kind | Name |
| ---: | :--- |
| `enum` | [`AccessSubtreeMode`](#accesssubtreemode) — How the AT walker treats descendants of an item |
| `struct` | [`ItemA11yOverrides`](#itema11yoverrides) — Builder-level accessibility overrides shared by every built-in `SceneItem` |
| `fn` | [`access_label`](#access_label) — Override the AT name announced for this item |
| `fn` | [`access_description`](#access_description) — Long-form context appended to the item's announcement |
| `fn` | [`access_role`](#access_role) — Override the AccessKit role for this item |
| `fn` | [`access_hidden`](#access_hidden) — Hide this item from the AT tree |
| `fn` | [`access_subtree`](#access_subtree) — Set the AT subtree mode |
| `fn` | [`access_merge_subtree`](#access_merge_subtree) — Convenience: collapse all descendants into this item's AT node so the subtree reads as one element |
| `fn` | [`access_exclude_subtree`](#access_exclude_subtree) — Convenience: prune all descendants from the AT tree |
| `fn` | [`access_value`](#access_value) — Announce a string value for this item (e.g. a formatted data reading like `"42 %"`) |
| `fn` | [`access_numeric_value`](#access_numeric_value) — Announce a numeric value for this item, for slider/gauge-like data marks whose magnitude assistive tech should describe |
| `fn` | [`access_numeric_range`](#access_numeric_range) — Announce the numeric min/max bounds for this item |
| `fn` | [`access_numeric_step`](#access_numeric_step) — Announce the numeric step (per-arrow increment) for this item |

## Public functions

### `ItemA11yOverrides`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `AccessSubtreeMode` | [`subtree_mode()`](#itema11yoverrides-subtree_mode) |

## Detailed description

Five lightweight items cover the common decoration cases:

- `RectItem` — filled / stroked rectangle. Backgrounds, tiles,
  simple decorations.
- `PathItem` — arbitrary vector path with optional fill and
  stroke. The "connector lines between cards" workhorse, with
  per-segment hit-test for stroke-only paths.
- `ImageItem` — a raster image at a local-coord rectangle.
- `TextItem` — unstyled text in a local-coord rectangle, static
  string or signal-bound.
- `GroupItem` — a group container with optional fill / stroke /
  inline label. Visually a labelled box; non-visual groups serve
  as logical AT containers (`Scene::add_a11y_group`).

All built-ins store their geometry in **local item coordinates**
anchored at the origin. Apps construct an item with its size at
origin (`RectItem::new(Rect::new(0.0, 0.0, w, h))`) and place it
in the scene with `Scene::add_item(item, local_pos)`.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

<a id="accesssubtreemode"></a>

## `pub enum AccessSubtreeMode`

How the AT walker treats descendants of an item.

Mirrors the widget-tier `AccessSubtreeMode`: `Inherit` is the
default (descendants emit normally); `Exclude` prunes them from
the AT tree; `Merge` collapses them into the parent so the
subtree reads as a single AT element. Used for "card with rect +
label + indicator dot reads as one card" patterns.

```rust
pub enum AccessSubtreeMode { /* variants */ }
```

### Variants

- **`Inherit`** — Descendants emit their own AT nodes normally. Default.
- **`Exclude`** — Descendants are pruned from the AT tree; the parent item emits as a single AT node with no children.
- **`Merge`** — Descendants' label / description / actions are folded into the parent AT node; descendants are then pruned. The subtree reads as one AT element — useful for "card with icon + label + badge = one selectable card" patterns.

<a id="itema11yoverrides"></a>

## `pub struct ItemA11yOverrides`

Builder-level accessibility overrides shared by every built-in
`SceneItem`. Mirrors the widget-level `.access_*` chain — names
match so muscle memory carries over.

```rust
pub struct ItemA11yOverrides { /* fields */ }
```

### Methods

<a id="itema11yoverrides-subtree_mode"></a>

#### `pub fn subtree_mode(&self) -> AccessSubtreeMode`

Read access for the AT walker.

<a id="access_label"></a>

## `pub fn access_label(...)`

Override the AT name announced for this item. Accepts
anything convertible into `LocalizedString` — most
commonly `tr!(...)` for translated labels, or any plain
string (which auto-converts via `From<String>`).

```rust
pub fn access_label(mut self, label: impl Into<LocalizedString>) -> Self;
```

<a id="access_description"></a>

## `pub fn access_description(...)`

Long-form context appended to the item's announcement.

```rust
pub fn access_description(mut self, description: impl Into<LocalizedString>) -> Self;
```

<a id="access_role"></a>

## `pub fn access_role(...)`

Override the AccessKit role for this item.

```rust
pub fn access_role(mut self, role: accesskit::Role) -> Self;
```

<a id="access_hidden"></a>

## `pub fn access_hidden(...)`

Hide this item from the AT tree.

```rust
pub fn access_hidden(mut self, hidden: bool) -> Self;
```

<a id="access_subtree"></a>

## `pub fn access_subtree(...)`

Set the AT subtree mode. `Merge` collapses descendants
into this item's AT node; `Exclude` prunes them; the
default `Inherit` lets them emit normally.

```rust
pub fn access_subtree(mut self, mode: $crate::items::AccessSubtreeMode) -> Self;
```

<a id="access_merge_subtree"></a>

## `pub fn access_merge_subtree(...)`

Convenience: collapse all descendants into this item's
AT node so the subtree reads as one element.

```rust
pub fn access_merge_subtree(mut self) -> Self;
```

<a id="access_exclude_subtree"></a>

## `pub fn access_exclude_subtree(...)`

Convenience: prune all descendants from the AT tree.

```rust
pub fn access_exclude_subtree(mut self) -> Self;
```

<a id="access_value"></a>

## `pub fn access_value(...)`

Announce a string value for this item (e.g. a formatted data
reading like `"42 %"`). Mirrors the widget-tier `.access_value`.

```rust
pub fn access_value(mut self, value: impl Into<LocalizedString>) -> Self;
```

<a id="access_numeric_value"></a>

## `pub fn access_numeric_value(...)`

Announce a numeric value for this item, for slider/gauge-like data
marks whose magnitude assistive tech should describe. Pair with
`access_numeric_range` /
`access_numeric_step` for full
range semantics.

```rust
pub fn access_numeric_value(mut self, value: f64) -> Self;
```

<a id="access_numeric_range"></a>

## `pub fn access_numeric_range(...)`

Announce the numeric min/max bounds for this item.

```rust
pub fn access_numeric_range(mut self, min: f64, max: f64) -> Self;
```

<a id="access_numeric_step"></a>

## `pub fn access_numeric_step(...)`

Announce the numeric step (per-arrow increment) for this item.

```rust
pub fn access_numeric_step(mut self, step: f64) -> Self;
```
