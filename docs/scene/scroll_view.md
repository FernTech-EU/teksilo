<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SceneScrollView

`SceneScrollView` — a thin composite that gives a `SceneView` draggable
scroll bars, mirroring the widget-tier
`ScrollArea`'s options: the same
`ScrollBarMode` (Overlay / Permanent / Thin, with its Tier-3
`ScrollBarStyle`), per-axis `ScrollBarPolicy` (AsNeeded / AlwaysOn /
AlwaysOff), and thickness. Smooth wheel / keyboard panning and the
overscroll policy stay configured on the wrapped `SceneView` itself (it
already animates pan and honours reduced-motion); the scroll bars simply
track that motion.

## Public functions

### `SceneScrollView`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(view: SceneView)`](#scenescrollview-new) |
| | **Builder methods** |
| `Self` | [`scroll_bar_mode(mode: ScrollBarMode)`](#scenescrollview-scroll_bar_mode) |
| `Self` | [`vertical_policy(policy: ScrollBarPolicy)`](#scenescrollview-vertical_policy) |
| `Self` | [`horizontal_policy(policy: ScrollBarPolicy)`](#scenescrollview-horizontal_policy) |
| `Self` | [`scroll_bar_thickness(thickness: f32)`](#scenescrollview-scroll_bar_thickness) |
| | **Methods** |
| `&Signal<f32>` | [`scroll_pos_x_signal()`](#scenescrollview-scroll_pos_x_signal) |
| `&Signal<f32>` | [`scroll_pos_y_signal()`](#scenescrollview-scroll_pos_y_signal) |
| `&Signal<f32>` | [`max_scroll_x_signal()`](#scenescrollview-max_scroll_x_signal) |
| `&Signal<f32>` | [`max_scroll_y_signal()`](#scenescrollview-max_scroll_y_signal) |
| `&Signal<f32>` | [`viewport_ratio_x_signal()`](#scenescrollview-viewport_ratio_x_signal) |
| `&Signal<f32>` | [`viewport_ratio_y_signal()`](#scenescrollview-viewport_ratio_y_signal) |

## Detailed description

#### Why a wrapper

A `SceneView` wraps its **entire child subtree** in the pan/zoom view
transform (via `set_content_transform`), so scroll bars added as its own
children would pan and zoom along with the content. Instead — exactly like
`ScrollArea` wraps arbitrary content and `SceneMinimap` is a sibling overlay
— this widget hosts the `SceneView` as content plus two reusable
`ScrollBar` children *outside* the transform,
and bridges the bars' scroll signals to the view's `pan_x`/`pan_y`.

#### How the bridge works

The scene's scrollable extent is its **effective pan bounds** (the
`Scene`-declared `pan_bounds` intersected with any view-level
`pan_bounds_override`), falling back to the union of item bounds. With the
standard view transform `screen = zoom*scene + pan + bounds_origin` and the
`SceneView` placed flush at this widget's origin (so `bounds_origin` cancels
the viewport's screen offset), the per-axis mapping in **screen-pixel
units** is:

```text
scroll_pos_x   = -pan_x - extent.x * zoom
max_scroll_x   = (extent.width * zoom - viewport_width).max(0)
viewport_ratio = viewport_width / (extent.width * zoom)
```

and the inverse, when a bar writes a new `scroll_pos_x`:

```text
pan_x = -extent.x * zoom - scroll_pos_x
```

The display direction (camera → bar metrics) is recomputed each
`place_children` — the same place `ScrollArea` computes its metrics — so it
never lags a layout pass. The interaction direction (bar drag → pan) is a
pair of guarded effects, one per axis, that snap the pan **immediately** so
the thumb tracks the cursor 1:1 (the desktop scroll-bar convention). Both
use an epsilon equality guard (the `color_picker` bidirectional-bridge
idiom) so a write arriving from the opposite direction is a no-op and the
loop closes — in particular the bars track the `SceneView`'s own smooth
wheel / keyboard pan animation without fighting it.

Rotation is supported but **approximate**: the mapping is exact only when
`rotation == 0`; while rotated the thumbs track the camera using the
axis-aligned formula above.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

<a id="scenescrollview"></a>

## `pub struct SceneScrollView`

A `SceneView` with draggable scroll bars.

Construct directly from a configured view, or via the
`SceneView::with_scroll_bars` convenience method:

```rust
# use teksilo_scene::{Scene, SceneView, SceneScrollView, ScrollBarMode};
let scrollable = SceneView::new(Scene::new())
    .with_scroll_bars()
    .scroll_bar_mode(ScrollBarMode::Overlay);
# let _ = scrollable;
```

```rust
pub struct SceneScrollView { /* fields */ }
```

### Methods

<a id="scenescrollview-new"></a>

#### `pub fn new(view: SceneView) -> Self`

Wrap a configured `SceneView` in a scroll-bar host. Captures the
view's pan/zoom/model signals before moving it into the arena.

<a id="scenescrollview-scroll_bar_mode"></a>

#### `pub fn scroll_bar_mode(mut self, mode: ScrollBarMode) -> Self`

Set the scroll-bar display mode (Overlay / Permanent / Thin).

<a id="scenescrollview-vertical_policy"></a>

#### `pub fn vertical_policy(mut self, policy: ScrollBarPolicy) -> Self`

Set the vertical scroll-bar visibility policy.

<a id="scenescrollview-horizontal_policy"></a>

#### `pub fn horizontal_policy(mut self, policy: ScrollBarPolicy) -> Self`

Set the horizontal scroll-bar visibility policy.

<a id="scenescrollview-scroll_bar_thickness"></a>

#### `pub fn scroll_bar_thickness(mut self, thickness: f32) -> Self`

Set the scroll-bar thickness (and the gutter width in Permanent mode).

<a id="scenescrollview-scroll_pos_x_signal"></a>

#### `pub fn scroll_pos_x_signal(&self) -> &Signal<f32>`

Horizontal scroll position signal (screen-pixel units), for external
observation. `0` = content's leading edge flush with the viewport.

<a id="scenescrollview-scroll_pos_y_signal"></a>

#### `pub fn scroll_pos_y_signal(&self) -> &Signal<f32>`

Vertical scroll position signal (screen-pixel units).

<a id="scenescrollview-max_scroll_x_signal"></a>

#### `pub fn max_scroll_x_signal(&self) -> &Signal<f32>`

Maximum horizontal scroll offset (`extent.width*zoom - viewport_width`,
or 0 when the content fits). Bind for "is there more to scroll?" chrome.

<a id="scenescrollview-max_scroll_y_signal"></a>

#### `pub fn max_scroll_y_signal(&self) -> &Signal<f32>`

Maximum vertical scroll offset.

<a id="scenescrollview-viewport_ratio_x_signal"></a>

#### `pub fn viewport_ratio_x_signal(&self) -> &Signal<f32>`

Horizontal viewport/content ratio (0.0..1.0) — the relative thumb size.

<a id="scenescrollview-viewport_ratio_y_signal"></a>

#### `pub fn viewport_ratio_y_signal(&self) -> &Signal<f32>`

Vertical viewport/content ratio (0.0..1.0).
