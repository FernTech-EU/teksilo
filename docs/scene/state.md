<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SceneViewState

`SceneViewState` — a snapshot of a `SceneView`'s
pan / zoom / rotation, suitable for persistence between sessions.

## Public functions

### `SceneViewState`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(pan: Vec2, zoom: f32, rotation: f32)`](#sceneviewstate-new) |
| | **Methods** |
| `Vec2` | [`pan()`](#sceneviewstate-pan) |
| `bool` | [`is_identity()`](#sceneviewstate-is_identity) |
| | **Constants and types** |
| `SceneViewState` | [`IDENTITY`](#sceneviewstate-identity) |

## Detailed description

#### Pattern

```ignore
use teksilo_scene::{Scene, SceneView, SceneViewState};

// On load: read from your persistence layer (teksilo-settings,
// a custom JSON file, etc.) and pass to SceneView.
let saved: SceneViewState = my_settings.scene_view.get();
let view = SceneView::new(scene);
view.restore_state(saved);

// On exit / periodic flush: snapshot and persist.
let current: SceneViewState = view.state();
my_settings.scene_view.set(current);
```

#### Why a plain struct, not Serialize

`teksilo-scene` deliberately doesn't depend on `serde`. Apps that
want to persist via `teksilo-settings` (which is `serde`-based)
either:

- Add their own newtype wrapper that implements
  `Serialize / Deserialize`, OR
- Store the fields individually (`pan_x`, `pan_y`, `zoom`,
  `rotation`) as scalar `SettingsKey<f32>`s in a
  `SettingsStore`.

The struct is plain-old-data — manual round-trip is trivial.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

<a id="sceneviewstate"></a>

## `pub struct SceneViewState`

Snapshot of a SceneView's view transform: pan offset, zoom
factor, and rotation in radians. Use `SceneView::state` to
capture the current values; `SceneView::restore_state` to
apply a saved snapshot.

```rust
pub struct SceneViewState { /* fields */ }
```

### Methods

<a id="sceneviewstate-identity"></a>

#### `pub const IDENTITY: SceneViewState = SceneViewState { pan_x: 0.0, pan_y: 0.0, zoom: 1.0, rotation: 0.0, };`

The identity view state: no pan, zoom 1.0, no rotation.

<a id="sceneviewstate-new"></a>

#### `pub fn new(pan: Vec2, zoom: f32, rotation: f32) -> Self`

Construct a new state with the given pan / zoom / rotation.

<a id="sceneviewstate-pan"></a>

#### `pub fn pan(&self) -> Vec2`

Pan offset as a `Vec2`.

<a id="sceneviewstate-is_identity"></a>

#### `pub fn is_identity(&self) -> bool`

Whether this state is the identity (no pan, zoom 1.0, no
rotation). Useful for skipping persistence of fresh-default
SceneViews.
