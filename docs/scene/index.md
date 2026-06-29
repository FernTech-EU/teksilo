<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Scene

Every public type in `bastyde-scene`, grouped by category. Each page links to its full rustdoc API reference.

## Items

- [GroupItem](group.md) — `GroupItem` — labelled box / logical AT container
- [ImageItem](image.md) — `ImageItem` — a raster image at a local-coord rectangle
- [PathItem](path.md) — `PathItem` — vector path with optional fill and stroke
- [RectItem](rect.md) — `RectItem` — filled / stroked rectangle in local item coords
- [TextItem](text.md) — `TextItem` — unstyled text in a local-coord rectangle

## Scene

- [A11yGroupId](a11y.md) — Accessibility policies for `SceneView`
- [AccessSubtreeMode](items.md) — Built-in `SceneItem` implementations
- [CacheMode](cache.md) — Item-coordinate paint caching
- [DebugOverlay](view.md) — `SceneView` — the viewport widget that hosts a `Scene` and
- [ItemFlags](flags.md) — Per-item behavior flags
- [ItemId](item.md) — The `SceneItem` trait and its supporting context types
- [Magnet](magnet.md) — Magnetism: typed snap-and-connect between anchor points on scene items
- [Scene](scene.md) — The `Scene` data model — the owner of all items in a pannable/zoomable
- [SceneMinimap](minimap.md) — `SceneMinimap` — a small thumbnail of a `Scene`
- [SceneModel](scene_model.md) — `SceneModel` — a shared, cloneable handle to a `Scene`
- [SceneScrollView](scroll_view.md) — `SceneScrollView` — a thin composite that gives a `SceneView` draggable
- [SceneSelectionMode](selection.md) — Selection model for `Scene` items
- [SceneTapEvent](item_handlers.md) — Per-item event handlers, cursor and tooltip overrides
- [SceneViewState](state.md) — `SceneViewState` — a snapshot of a `SceneView`'s
- [SpatialIndex](index_.md) — Spatial index for `Scene` items
