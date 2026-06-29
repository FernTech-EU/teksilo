<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SceneViewState

`SceneViewState` — a snapshot of a `SceneView`'s
pan / zoom / rotation, suitable for persistence between sessions.

## Pattern

```ignore
use bastyde_scene::{Scene, SceneView, SceneViewState};

// On load: read from your persistence layer (bastyde-settings,
// a custom JSON file, etc.) and pass to SceneView.
let saved: SceneViewState = my_settings.scene_view.get();
let view = SceneView::new(scene);
view.restore_state(saved);

// On exit / periodic flush: snapshot and persist.
let current: SceneViewState = view.state();
my_settings.scene_view.set(current);
```

## Why a plain struct, not Serialize

`bastyde-scene` deliberately doesn't depend on `serde`. Apps that
want to persist via `bastyde-settings` (which is `serde`-based)
either:

- Add their own newtype wrapper that implements
  `Serialize / Deserialize`, OR
- Store the fields individually (`pan_x`, `pan_y`, `zoom`,
  `rotation`) as scalar `SettingsKey<f32>`s in a
  `SettingsStore`.

The struct is plain-old-data — manual round-trip is trivial.

## Builder methods at a glance

`IDENTITY`, `pan`, `is_identity`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_scene/index.html)

## `pub struct SceneViewState`

Snapshot of a SceneView's view transform: pan offset, zoom
factor, and rotation in radians. Use `SceneView::state` to
capture the current values; `SceneView::restore_state` to
apply a saved snapshot.

```rust
pub struct SceneViewState { /* fields */ }
```

### Methods

#### `pub const IDENTITY: SceneViewState = SceneViewState { pan_x: 0.0, pan_y: 0.0, zoom: 1.0, rotation: 0.0, };`

The identity view state: no pan, zoom 1.0, no rotation.

#### `pub fn new(pan: Vec2, zoom: f32, rotation: f32) -> Self`

Construct a new state with the given pan / zoom / rotation.

#### `pub fn pan(&self) -> Vec2`

Pan offset as a `Vec2`.

#### `pub fn is_identity(&self) -> bool`

Whether this state is the identity (no pan, zoom 1.0, no
rotation). Useful for skipping persistence of fresh-default
SceneViews.
