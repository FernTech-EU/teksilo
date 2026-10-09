<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# CacheMode

Item-coordinate paint caching.

## Public types

| Kind | Name |
| ---: | :--- |
| `enum` | [`CacheMode`](#cachemode) — Per-item paint caching strategy |
| `struct` | [`ItemCoordinateCache`](#itemcoordinatecache) — SceneView's per-item paint cache |

## Public functions

### `ItemCoordinateCache`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new()`](#itemcoordinatecache-new) |
| | **Methods** |
| `Option<&RenderFrame>` | [`get(id: ItemId, raster_scale: f32)`](#itemcoordinatecache-get) |
|  | [`insert(id: ItemId, frame: RenderFrame, raster_scale: f32)`](#itemcoordinatecache-insert) |
|  | [`evict(id: ItemId)`](#itemcoordinatecache-evict) |
|  | [`clear()`](#itemcoordinatecache-clear) |
| `bool` | [`sync_glyph_epoch(current_epoch: u64)`](#itemcoordinatecache-sync_glyph_epoch) |
| `usize` | [`len()`](#itemcoordinatecache-len) |

## Detailed description

When a `SceneItem` returns
`CacheMode::ItemCoordinate` from `SceneItem::cache_mode`,
the `SceneView` caches the item's paint
output as a `RenderFrame` in **local item coordinates**. On
subsequent paint passes the cached frame is replayed via
`Canvas::draw_render_frame` instead of re-running
`item.paint`. Cache validity is keyed by
`Scene::item_change_signal`:
a `LocalBoundsChanged` event for an id evicts that id's entry.

Items whose visual depends on signal state outside of their
`local_bounds` (e.g. `TextItem` with `with_signal_text`) should
NOT use `ItemCoordinate` — the cache won't see signal-driven
repaint dirties. The default for every `SceneItem` is
`CacheMode::None`.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

<a id="cachemode"></a>

## `pub enum CacheMode`

Per-item paint caching strategy.

```rust
pub enum CacheMode { /* variants */ }
```

### Variants

- **`None`** — Re-run `item.paint` every frame. Default for every item.
- **`ItemCoordinate`** — Cache the paint output as a `RenderFrame` keyed by the item's `local_bounds`. Cheap when the item's geometry is stable and its content doesn't depend on external signal state. The cache is dropped on `LocalBoundsChanged` for the id.

<a id="itemcoordinatecache"></a>

## `pub struct ItemCoordinateCache`

SceneView's per-item paint cache. Owned by the SceneView, shared
via `Rc<RefCell<>>` so the paint walk and the item-change
observer can both touch it.

```rust
pub struct ItemCoordinateCache { /* fields */ }
```

### Methods

<a id="itemcoordinatecache-new"></a>

#### `pub fn new() -> Self`

An empty cache.

<a id="itemcoordinatecache-get"></a>

#### `pub fn get(&self, id: ItemId, raster_scale: f32) -> Option<&RenderFrame>`

Borrow the cached frame for `id`, if any — provided it was
recorded at `raster_scale`. A scale mismatch reads as a miss:
the caller re-records and `insert` replaces the
stale entry.

<a id="itemcoordinatecache-insert"></a>

#### `pub fn insert(&mut self, id: ItemId, frame: RenderFrame, raster_scale: f32)`

Insert (or replace) a cached frame for `id`, recorded at
`raster_scale`.

<a id="itemcoordinatecache-evict"></a>

#### `pub fn evict(&mut self, id: ItemId)`

Evict `id`'s entry. Called on `ItemChange::LocalBoundsChanged`
or any other invalidation.

<a id="itemcoordinatecache-clear"></a>

#### `pub fn clear(&mut self)`

Drop every entry. Called when the glyph epoch moves (see
`sync_glyph_epoch`).

<a id="itemcoordinatecache-sync_glyph_epoch"></a>

#### `pub fn sync_glyph_epoch(&mut self, current_epoch: u64) -> bool`

Compare the text backend's current glyph epoch against the one
recorded on the last paint pass; on a change, drop every cached
frame (their baked atlas UVs may reference recycled slots) and
record the new epoch. Returns `true` when the cache was cleared.

<a id="itemcoordinatecache-len"></a>

#### `pub fn len(&self) -> usize`

Number of cached entries (diagnostics / tests).
