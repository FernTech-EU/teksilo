<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# DebugOverlay

`SceneView` — the viewport widget that hosts a `Scene` and
places its items at scene coordinates.

## Public types

| Kind | Name |
| ---: | :--- |
| `const` | [`DEFAULT_RETENTION_MARGIN`](#default_retention_margin) — Default `SceneView::retention_margin`, in screen pixels |
| `struct` | [`DebugOverlay`](#debugoverlay) — Visual debug overlays painted on top of normal scene rendering |
| `enum` | [`FocusDirection`](#focusdirection) — Direction passed to a `SceneView::focus_order` callback when the app wants to override the default Tab cycle |
| `struct` | [`SceneView`](#sceneview) — A pannable/zoomable viewport that renders a `Scene`'s items at scene coordinates and routes user input (scroll, pinch, drag, keyboard) back into the camera… |

## Public functions

### `DebugOverlay`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `bool` | [`is_active()`](#debugoverlay-is_active) |
| | **Constants and types** |
| `DebugOverlay` | [`ALL`](#debugoverlay-all) |

### `SceneView`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(scene: Scene)`](#sceneview-new) |
| `Self` | [`with_model(model: SceneModel)`](#sceneview-with_model) |
| | **Builder methods** |
| `Self` | [`selection_mode(mode: crate::selection::SceneSelectionMode)`](#sceneview-selection_mode) |
| `Self` | [`marquee_selection_mode(mode: crate::shape::ItemSelectionMode)`](#sceneview-marquee_selection_mode) |
| `Self` | [`delegate(f: impl Fn(&dyn std::any::Any, ItemId) -> Box<dyn Widget> + 'static)`](#sceneview-delegate) |
| `Self` | [`delegate_typed<P: 'static>(f: impl Fn(&P, ItemId) -> Box<dyn Widget> + 'static)`](#sceneview-delegate_typed) |
| `Self` | [`selection_model(selection: crate::selection::SceneSelection)`](#sceneview-selection_model) |
| `Self` | [`interactive(interactive: bool)`](#sceneview-interactive) |
| `Self` | [`nested_a11y(nested: bool)`](#sceneview-nested_a11y) |
| `Self` | [`a11y_label(label: impl Into<LocalizedString>)`](#sceneview-a11y_label) |
| `Self` | [`debug_overlay(overlay: DebugOverlay)`](#sceneview-debug_overlay) |
| `Self` | [`focus_order<F>(callback: F)`](#sceneview-focus_order) |
| `Self` | [`a11y_mode(mode: crate::a11y::A11yMode)`](#sceneview-a11y_mode) |
| `Self` | [`retention_margin(screen_px: f32)`](#sceneview-retention_margin) |
| `Self` | [`a11y_off_screen_mode(mode: crate::a11y::A11yOffScreenMode)`](#sceneview-a11y_off_screen_mode) |
| `Self` | [`default_size(w: f32, h: f32)`](#sceneview-default_size) |
| `Self` | [`adopt_scene_size(on: bool)`](#sceneview-adopt_scene_size) |
| `Self` | [`drag_mode(mode: impl Into<Prop<crate::item_handlers::DragMode>>)`](#sceneview-drag_mode) |
| `Self` | [`view_state(pan_x: Signal<f32>, pan_y: Signal<f32>, zoom: Signal<f32>, rotation: Signal<f32>)`](#sceneview-view_state) |
| `Self` | [`initial_pan(x: f32, y: f32)`](#sceneview-initial_pan) |
| `Self` | [`initial_zoom(zoom: f32)`](#sceneview-initial_zoom) |
| `Self` | [`initial_rotation(radians: f32)`](#sceneview-initial_rotation) |
| `Self` | [`background<F>(paint: F)`](#sceneview-background) |
| `Self` | [`foreground<F>(paint: F)`](#sceneview-foreground) |
| `Self` | [`wet_layer(layer: crate::WetLayer)`](#sceneview-wet_layer) |
| `Self` | [`magnetism(config: crate::magnet::MagnetismConfig)`](#sceneview-magnetism) |
| `Self` | [`transform_controller(config: crate::transform_session::TransformConfig)`](#sceneview-transform_controller) |
| `Self` | [`min_zoom(v: f32)`](#sceneview-min_zoom) |
| `Self` | [`max_zoom(v: f32)`](#sceneview-max_zoom) |
| `Self` | [`zoom_range_override(range: Option<std::ops::RangeInclusive<f32>>)`](#sceneview-zoom_range_override) |
| `Self` | [`pan_bounds_override(bounds: Option<Rect>)`](#sceneview-pan_bounds_override) |
| `Self` | [`line_height(px: f32)`](#sceneview-line_height) |
| `Self` | [`overscroll_behavior(behavior: OverscrollBehavior)`](#sceneview-overscroll_behavior) |
| | **Methods** |
| `&crate::selection::SceneSelection` | [`selection()`](#sceneview-selection) |
| `SceneModel` | [`model()`](#sceneview-model) |
| `&SceneModel` | [`model_ref()`](#sceneview-model_ref) |
| `bool` | [`flush_marquee_commit()`](#sceneview-flush_marquee_commit) |
| `bool` | [`flush_pending_item_move()`](#sceneview-flush_pending_item_move) |
| `bool` | [`flush_pending_transform()`](#sceneview-flush_pending_transform) |
| `bool` | [`is_nested()`](#sceneview-is_nested) |
| `DebugOverlay` | [`current_debug_overlay()`](#sceneview-current_debug_overlay) |
| `Option<ItemId>` | [`focus_in_direction(direction: FocusDirection, current: Option<ItemId>)`](#sceneview-focus_in_direction) |
| `Option<ItemId>` | [`next_focus(current: Option<ItemId>)`](#sceneview-next_focus) |
| `Option<ItemId>` | [`previous_focus(current: Option<ItemId>)`](#sceneview-previous_focus) |
| `Signal<f32>` | [`pan_x_signal()`](#sceneview-pan_x_signal) |
| `Signal<f32>` | [`pan_y_signal()`](#sceneview-pan_y_signal) |
| `Signal<f32>` | [`zoom_signal()`](#sceneview-zoom_signal) |
| `Signal<f32>` | [`rotation_signal()`](#sceneview-rotation_signal) |
| `Signal<Transform2D>` | [`view_transform_signal()`](#sceneview-view_transform_signal) |
| `f32` | [`current_retention_margin()`](#sceneview-current_retention_margin) |
| `Signal<crate::item_handlers::DragMode>` | [`drag_mode_signal()`](#sceneview-drag_mode_signal) |
| `Signal<Option<crate::transform_session::TransformSession>>` | [`transform_session_signal()`](#sceneview-transform_session_signal) |
| `Option<Signal<bool>>` | [`transform_enabled_signal()`](#sceneview-transform_enabled_signal) |
| `Option<Signal<bool>>` | [`magnetism_enabled_signal()`](#sceneview-magnetism_enabled_signal) |
|  | [`invalidate_item_cache(id: ItemId)`](#sceneview-invalidate_item_cache) |
| `usize` | [`item_cache_len()`](#sceneview-item_cache_len) |
| `Signal<Option<std::ops::RangeInclusive<f32>>>` | [`zoom_range_override_signal()`](#sceneview-zoom_range_override_signal) |
| `Signal<Option<Rect>>` | [`pan_bounds_override_signal()`](#sceneview-pan_bounds_override_signal) |
| `crate::SceneScrollView` | [`with_scroll_bars()`](#sceneview-with_scroll_bars) |
| `std::cell::Ref<'_, Scene>` | [`scene()`](#sceneview-scene) |
| `std::cell::RefMut<'_, Scene>` | [`scene_mut()`](#sceneview-scene_mut) |
| `Option<WidgetId>` | [`widget_id_for(id: ItemId)`](#sceneview-widget_id_for) |
| `Vec2` | [`pan()`](#sceneview-pan) |
| `f32` | [`zoom()`](#sceneview-zoom) |
| `f32` | [`rotation()`](#sceneview-rotation) |
| `Option<f32>` | [`pan_x_animation_target()`](#sceneview-pan_x_animation_target) |
| `Option<f32>` | [`pan_y_animation_target()`](#sceneview-pan_y_animation_target) |
| `Option<f32>` | [`zoom_animation_target()`](#sceneview-zoom_animation_target) |
| `Transform2D` | [`view_transform()`](#sceneview-view_transform) |
| `f32` | [`view_scale()`](#sceneview-view_scale) |
| `Point` | [`map_to_scene(view_pt: Point)`](#sceneview-map_to_scene) |
| `Point` | [`map_from_scene(scene_pt: Point)`](#sceneview-map_from_scene) |
| `Rect` | [`map_rect_to_scene(view_rect: Rect)`](#sceneview-map_rect_to_scene) |
| `Rect` | [`map_rect_from_scene(scene_rect: Rect)`](#sceneview-map_rect_from_scene) |
| `Signal<Rect>` | [`viewport_in_scene_signal()`](#sceneview-viewport_in_scene_signal) |
| `Signal<Size>` | [`viewport_size_signal()`](#sceneview-viewport_size_signal) |
|  | [`pan_to(target: Vec2, duration: Duration)`](#sceneview-pan_to) |
|  | [`set_pan(target: Vec2)`](#sceneview-set_pan) |
|  | [`zoom_to(target: f32, duration: Duration)`](#sceneview-zoom_to) |
|  | [`set_zoom(target: f32)`](#sceneview-set_zoom) |
|  | [`ensure_visible(scene_rect: Rect, margin: f32)`](#sceneview-ensure_visible) |
| `Vec2` | [`ensure_visible_aligned(scene_rect: Rect, margin: f32, align: ScrollAlign, motion: ScrollMotion)`](#sceneview-ensure_visible_aligned) |
|  | [`rotate_to(target_radians: f32, duration: Duration)`](#sceneview-rotate_to) |
|  | [`set_rotation(target_radians: f32)`](#sceneview-set_rotation) |
| `crate::SceneViewState` | [`state()`](#sceneview-state) |
|  | [`restore_state(state: crate::SceneViewState)`](#sceneview-restore_state) |
| `Size` | [`viewport_size()`](#sceneview-viewport_size) |
| `Option<Rect>` | [`scene_content_bounds()`](#sceneview-scene_content_bounds) |
|  | [`fit_to_content()`](#sceneview-fit_to_content) |
|  | [`fit_to_items(ids: &[ItemId])`](#sceneview-fit_to_items) |
|  | [`fit_to_selection()`](#sceneview-fit_to_selection) |

## Detailed description

`SceneView` is the bridge between the model layer (`Scene` /
`SceneModel`) and the render/event pipeline. It
manages a pan/zoom/rotation camera, materialises heavyweight widgets
for delegated items, dispatches pointer events to lightweight item
handlers, and feeds synthetic AT nodes to AccessKit for every visible
lightweight item. Multiple `SceneView`s can share one `SceneModel` and
reconcile independently on every mutation.

#### Composition

- **Placement.** `place_children` plants each materialised
  heavyweight widget at its scene-space rect (composed from the
  item's `local_pos`, `transform`, and parent chain).
- **Paint bands.** Three passes: `paint` draws the `Under` lightweight
  items (backdrop), the arena child-walk draws the heavyweight widgets
  — with an `Interleaved` item's own paint node slotted in among them by
  `z` — then `post_paint` draws the `Over` lightweight items + marquee /
  foreground / debug overlays. `z` orders within each tier; the band
  (`Scene::set_layer`) chooses `Under`,
  `Interleaved` or `Over`. See `docs/teksilo-scene.md` §"Z-order and paint bands".
- **View transform.** Pan / zoom / rotation are four animated
  `Signal<f32>`s on `SceneView`, composed into a derived
  `Signal<Transform2D>` bound via `BuildContext::set_content_transform`
  on the view itself. The render walker pushes that scope around
  the entire subtree, so every materialised widget is visually
  transformed; transform-aware hit-test routes pointer events
  through the same scope.
- **Spatial index.** `place_children` and the paint walk consult
  `Scene::items_in_rect(visible_region)` to skip off-screen items.
- **Idle gating.** Pan / zoom that's reached its terminal tick
  stops scheduling frames via the engine's per-node `paint_epoch`.

#### Input wiring

- **`on_scroll`** — trackpad two-finger pan (`ScrollDelta::Pixels`)
  and mouse wheel (`ScrollDelta::Lines`) animate the pan signals
  via `Easing::EaseOut`. Trackpad momentum events from winit
  arrive as further `Pixels` deltas; the existing animation
  pipeline turns this into smooth inertial fling without a custom
  recognizer.
- **`on_pinch`** — a pinch (`PinchPhase::Changed`) feeds `scale` into
  the zoom signal and `rotation` into the rotation signal, anchored
  around the gesture center so the scene point under the user's
  fingers stays put. Two producers reach the one handler: the OS
  trackpad stream and the two-contact touch recognizer, and both
  satisfy the contract on
  `GestureEvent::PinchChanged`
  — `scale` is the factor **since the previous sample** (folded in with
  `zoom *= scale`) and `rotation` the **radian** delta since the previous
  sample (`rotation += delta`). Degrees never reach here: winit's unit is
  converted at the platform seam. `view::tests::touch_camera` holds both
  halves.
- **Reduced-motion** — at build time, captures
  `BuildContext::prefers_reduced_motion`.
  When set, scroll handlers `set` the signals directly instead of
  `animate_to`-ing them; pinch is already instantaneous.
- **Drag-to-move** for items carrying `IS_DRAGGABLE`; **marquee**
  selection on the empty viewport surface (or under
  `DragMode::ScrollHandDrag`, pan-on-drag).
- **Pan to scroll** — an interactive view declares a
  `PanClaim`, so a finger's
  pan reaches the same `on_scroll` handler as a
  `ScrollSource::TouchPan` sample and moves the camera with no tween (a
  finger is already the animation), hard-clamped on the coast that follows
  the lift, and declined at a bound so the gesture chains to whatever
  scrolls outside the view. Deliberately **not** routed through
  `teksilo_widgets::common::scrollable`: that models a surface as an offset
  in `[0, max]`, and a scene's pan is negated, bounded by a zoom-dependent
  rectangle, and centre-pinned where that rectangle is smaller than the
  viewport. With selection or magnetism on, the view's own drag recognizer
  is the competitor a finger meets first and it wins at the smaller drag
  slop — the marquee, not the camera.

#### Example

```rust
# use teksilo_scene::{Scene, SceneModel, SceneView, SceneSelectionMode, RectItem};
# use teksilo_canvas::{Point, Rect};
# use teksilo_tokens::Color;
// Build a shared model and add a lightweight rect item.
let model = SceneModel::new();
let local_bounds = Rect::new(0.0, 0.0, 120.0, 80.0);
let item_id = model.add_item(
    RectItem::new(local_bounds).fill(Color::from_rgb(0.2, 0.5, 0.8)),
    Point::new(50.0, 50.0), // local_pos in scene coords
);

// Create viewports backed by that model; each has its own camera.
let _view_a = SceneView::with_model(model.clone())
    .selection_mode(SceneSelectionMode::Single)
    .default_size(800.0, 600.0)
    .initial_zoom(1.5);

let _view_b = SceneView::with_model(model.clone())
    .interactive(false); // axis-chrome / overview pane

// Both views see the item; the model remembers its local_pos.
assert!(model.local_pos(item_id).is_some());
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

<a id="default_retention_margin"></a>

## `pub const DEFAULT_RETENTION_MARGIN`

**Import:** `use teksilo_scene::DEFAULT_RETENTION_MARGIN;` — add `teksilo-scene` to `[dependencies]`: teksilo does not re-export it.

Default `SceneView::retention_margin`, in screen pixels.

96 px is what a very fast fling covers in one frame at 120 Hz
(11 520 px/s), which is the quantity the margin has to beat: it exists so a
card is woken while it is still off screen rather than on the frame it
becomes visible.

```rust
pub const DEFAULT_RETENTION_MARGIN: f32 = 96.0;
```

<a id="debugoverlay"></a>

## `pub struct DebugOverlay`

**Import:** `use teksilo_scene::DebugOverlay;` — add `teksilo-scene` to `[dependencies]`: teksilo does not re-export it.

Visual debug overlays painted on top of normal scene rendering.

Every flag defaults to `false`. Use this to verify that culling /
hit-test / spatial-index / dragging are doing what you expect
while developing a scene-based feature; turn off before shipping.

Each flag adds a thin overlay paint with a distinct color so
multiple flags can be combined without visual confusion:

- `item_bounds`: green outline around every
  visible scene item's `bounds_in_scene`.
- `content_bounds`: blue outline around
  the scene's overall content extent (the union of all item
  bounds).
- `viewport`: red outline around the visible
  scene region (the cull rect — the inverse-projected viewport).
- `selection_bounds`: orange outline
  around every currently-selected item.

```rust
pub struct DebugOverlay { /* fields */ }
```

### Fields

- **`item_bounds`**: `bool`
- **`content_bounds`**: `bool`
- **`viewport`**: `bool`
- **`selection_bounds`**: `bool`

### Methods

<a id="debugoverlay-all"></a>

#### `pub const ALL: DebugOverlay = DebugOverlay { item_bounds: true, content_bounds: true, viewport: true, selection_bounds: true, };`

All overlays enabled. Useful to catch any anomaly visually.

<a id="debugoverlay-is_active"></a>

#### `pub fn is_active(&self) -> bool`

Whether at least one debug overlay is enabled.

<a id="focusdirection"></a>

## `pub enum FocusDirection`

**Import:** `use teksilo_scene::FocusDirection;` — add `teksilo-scene` to `[dependencies]`: teksilo does not re-export it.

Direction passed to a `SceneView::focus_order` callback when the
app wants to override the default Tab cycle.

`Forward` corresponds to Tab; `Backward` to Shift+Tab. The default
SceneView focus traversal is scene insertion order — apps that
need data-flow order (graph editor), story-order (corkboard with
Acts), chronological order (timeline), etc. install a callback
that receives the current focus and returns the next id.

```rust
pub enum FocusDirection { /* variants */ }
```

### Variants

- **`Forward`** — Advance to the next item — corresponds to the Tab key.
- **`Backward`** — Retreat to the previous item — corresponds to Shift+Tab.

<a id="sceneview"></a>

## `pub struct SceneView`

**Import:** `use teksilo_scene::SceneView;` — add `teksilo-scene` to `[dependencies]`: teksilo does not re-export it.

A pannable/zoomable viewport that renders a `Scene`'s items at scene
coordinates and routes user input (scroll, pinch, drag, keyboard) back into
the camera signals.

Construct with `SceneView::new` (single-view sugar: wraps a `Scene` in a
fresh `SceneModel`) or `SceneView::with_model` (multi-view: several
viewports share one `SceneModel` and each reconcile independently on every
mutation). Install a heavyweight builder for delegated items via
`delegate_typed`. Add to a `WidgetTree`
like any other widget; gestures and camera animations are wired automatically
during `build`.

See the `module-level documentation` for the full composition model
and `docs/teksilo-scene.md` for an end-to-end guide.

```rust
pub struct SceneView { /* fields */ }
```

### Methods

<a id="sceneview-veto_snapshot_scans"></a>

#### `pub fn veto_snapshot_scans(&self) -> u64`  *(hidden)*

How many times this view has walked its handler snapshot to answer the
above-a-card veto, since it was created.

The veto's memo is asked once per hit-tested child and answers from one
scan; this is the number that says so. A hit test over *n* overlapping
cards must advance it by **one**, not by *n* — the regression the
`veto_scaling_probe` test gates, and one that is invisible to every
other observable the crate publishes (the verdicts are identical either
way; only the cost moves).

<a id="sceneview-veto_key_probes"></a>

#### `pub fn veto_key_probes(&self) -> u64`  *(hidden)*

How many times the veto has looked a child's `PaintKey` up.

The companion to `veto_snapshot_scans`, and
necessary because the two halves of the veto's cost fail independently:
a memo keyed on something that differs per child makes the *scan* count
follow the card count, while evaluating the child's key before the
claimant query can decline it makes *this* one follow it. Each is
invisible to the other's test, and neither changes a verdict.

A scene with no press claimant under the pointer must leave this at
zero however many children the arena asks about.

<a id="sceneview-new"></a>

#### `pub fn new(scene: Scene) -> Self`

Wrap a `Scene` in a viewport (single-view sugar). The scene is moved
into a fresh `SceneModel`; for multi-view, build a `SceneModel`
yourself and use `with_model`.

<a id="sceneview-with_model"></a>

#### `pub fn with_model(model: SceneModel) -> Self`

Attach a viewport to a (possibly shared) `SceneModel`. Clone one
model into several `SceneView::with_model(model.clone())` to render the
same scene in multiple panes, each with its own camera and delegate.

<a id="sceneview-selection_mode"></a>

#### `pub fn selection_mode(mut self, mode: crate::selection::SceneSelectionMode) -> Self`

Configure selection behavior. Default
`SceneSelectionMode::None` —
click and marquee do nothing. Set to `Single` for
at-most-one selection (click replaces) or `Multi` for
multi-select with marquee box-select, Ctrl+click toggle,
and Ctrl+drag additive marquee.

<a id="sceneview-marquee_selection_mode"></a>

#### `pub fn marquee_selection_mode(mut self, mode: crate::shape::ItemSelectionMode) -> Self`

Which rule the rubber band picks items by.

Default `ItemSelectionMode::IntersectsItemShape` —
Qt's default, and the one that makes a rubber band agree with a click
about what an item is: a band that merely grazes a connector's bounding
box no longer selects the connector, it has to actually cross the
stroke. Pass
`IntersectsItemBoundingRect`
for the looser box test, which is what the marquee did before
`ItemShape` existed.

<a id="sceneview-selection"></a>

#### `pub fn selection(&self) -> &crate::selection::SceneSelection`

Borrow the SceneView's `SceneSelection`.
Use this from external code to bind to the selection signal,
query selected ids, or call `select_one` / `clear` /
`replace` programmatically.

<a id="sceneview-delegate"></a>

#### `pub fn delegate( mut self, f: impl Fn(&dyn std::any::Any, ItemId) -> Box<dyn Widget> + 'static, ) -> Self`

Install the per-view heavyweight builder for `Delegated` items
(those added via `SceneModel::add_widget_item`).
The closure receives the item's type-erased payload and its `ItemId`
and returns the widget to materialise in **this** view's arena.
Prefer the typed `delegate_typed` wrapper.

<a id="sceneview-delegate_typed"></a>

#### `pub fn delegate_typed<P: 'static>( mut self, f: impl Fn(&P, ItemId) -> Box<dyn Widget> + 'static, ) -> Self`

Typed convenience over `delegate`: downcasts the
payload to `P` before calling `f`. A downcast miss debug-asserts and
skips the item (no widget is materialised) in release.

<a id="sceneview-selection_model"></a>

#### `pub fn selection_model(mut self, selection: crate::selection::SceneSelection) -> Self`

Replace this view's selection with a (typically shared) one. Pass the
same `SceneSelection` clone to several views so
they select together; capture its `selection_signal()` in your delegate
to highlight selected items reactively (no rebuild). Distinct from the
`selection()` getter; supersedes any
`selection_mode` set earlier.

<a id="sceneview-model"></a>

#### `pub fn model(&self) -> SceneModel`

A clone of this view's `SceneModel` handle — for handler closures
that mutate the scene (every mutator is `&self`) or wire additional views.

<a id="sceneview-model_ref"></a>

#### `pub fn model_ref(&self) -> &SceneModel`

Borrow this view's `SceneModel` handle.

<a id="sceneview-flush_marquee_commit"></a>

#### `pub fn flush_marquee_commit(&self) -> bool`

Drain any pending marquee commit synchronously. Normal
per-frame use never needs this — `place_children` consumes
the pending commit at the start of every layout pass. Tests
that drive on_drag without a follow-up layout call this to
materialise the box-select result.

<a id="sceneview-flush_pending_item_move"></a>

#### `pub fn flush_pending_item_move(&mut self) -> bool`

Drain any pending drag-to-move commit by translating the
dragged item's `local_pos` by the queued delta. Descendants
follow automatically: their `local_pos` is unchanged but
their `scene_pos` derives from the parent's chain.

<a id="sceneview-flush_pending_transform"></a>

#### `pub fn flush_pending_transform(&mut self) -> bool`

Drain a committed transform, applying its delta through one
`Scene::apply_transform_delta`.

The same drain `build()` runs; the public form exists for the headless
driver, exactly like `flush_pending_item_move`
and `flush_marquee_commit`. Returns whether
anything was pending.

<a id="sceneview-interactive"></a>

#### `pub fn interactive(mut self, interactive: bool) -> Self`

Disable user-driven navigation: pinch and keyboard handlers are not
registered, the scroll handler's wheel arm returns early, and the
SceneView is not made focusable. The `on_scroll` slot itself stays
registered, because it also carries a descendant's `ScrollIntoView`
reveal — a focused caret inside an embedded card is entitled to be on
screen whether or not the *user* may drive the camera. Programmatic
`pan_to` /
`zoom_to` / `fit_to_content`
still work — this gates only user input.

Use this for **outer** SceneViews in nested chart-style
patterns: an outer locked SceneView holds axis chrome
(`TextItem`s reading the inner's pan/zoom signals via
`view_transform_signal`),
an inner interactive SceneView holds the data and accepts
pan/zoom from the user. Default: interactive (`true`).

<a id="sceneview-nested_a11y"></a>

#### `pub fn nested_a11y(mut self, nested: bool) -> Self`

Mark this SceneView as logically nested inside another
SceneView. Affects only the AT walker — the inner
SceneView reports `Role::Region` instead of the default
`Role::Pane`, so screen readers don't announce a
redundant top-level landmark for what's logically a sub-
region. Pair with `a11y_label` to give
the inner region a useful announce name.

Use case: chart-style nested scenes (outer SceneView holds
axis chrome, inner SceneView holds data) — the inner one
should announce as "Data area" or similar, not as another
"Pane" sibling to the outer.

Default `false`. Apps explicitly set this when they know
they're nesting; the framework doesn't introspect the
widget tree to detect nesting automatically (deliberately
kept declarative — the visual layout doesn't always match
logical nesting).

<a id="sceneview-a11y_label"></a>

#### `pub fn a11y_label(mut self, label: impl Into<LocalizedString>) -> Self`

Set the AT label announced as this SceneView's own name.
Particularly useful for nested SceneViews via
`nested_a11y`, where the inner
region should have a domain-specific name (e.g. "Chart
data area"). Default `None` — the SceneView has no
explicit AT name.

<a id="sceneview-is_nested"></a>

#### `pub fn is_nested(&self) -> bool`

Whether the SceneView is currently marked as logically
nested. Read-only accessor for tests / diagnostics.

<a id="sceneview-debug_overlay"></a>

#### `pub fn debug_overlay(mut self, overlay: DebugOverlay) -> Self`

Configure visual debug overlays. Default: all flags off.
Pass `DebugOverlay::ALL` to enable every overlay or
construct a custom config:

```
# use teksilo_scene::{Scene, SceneView, DebugOverlay};
# let scene = Scene::new();
let _view = SceneView::new(scene)
    .debug_overlay(DebugOverlay {
        item_bounds: true,
        viewport: true,
        ..Default::default()
    });
```

Intended for development only — overlay paint is cheap but
not free; ship with the default (off) config.

<a id="sceneview-current_debug_overlay"></a>

#### `pub fn current_debug_overlay(&self) -> DebugOverlay`

Read-only accessor for the active debug overlay config.

<a id="sceneview-focus_order"></a>

#### `pub fn focus_order<F>(mut self, callback: F) -> Self where F: Fn(&Scene, FocusDirection, Option<ItemId>) -> Option<ItemId> + 'static,`

Install a custom focus-order callback. When set,
`next_focus` /
`previous_focus` route through the
closure instead of falling back to scene insertion order.

Apps wire this to a Tab / Shift+Tab handler in their root
shortcut/action map. Typical implementations:

- **Graph editor:** walk outgoing-port connections from the
  current node, return the connected-node `ItemId`.
- **Corkboard with Acts:** walk a parallel `BTreeMap<ActId,
  Vec<CardId>>` declared by the app and Tab through cards in
  story order, not reading order.
- **Timeline:** sort items by `start_time`, return the next.

The callback receives the full `Scene` (read-only), the
requested `FocusDirection`, and the currently focused item
(`None` on the first Tab into the scene). Return `None` to
signal "no next item" (the framework can then advance focus
outside the SceneView).

Calling `next_focus` /
`previous_focus` without a callback
installed walks scene insertion order — adequate for simple
scenes; replace as needed.

<a id="sceneview-focus_in_direction"></a>

#### `pub fn focus_in_direction( &self, direction: FocusDirection, current: Option<ItemId>, ) -> Option<ItemId>`

Compute the next item the focus should advance to in the
given direction. If a `focus_order`
callback is installed, routes through it; otherwise falls
back to scene insertion order — `Forward` returns the item
after `current` (or the first if `current` is `None`),
`Backward` returns the previous (or the last if `current`
is `None`).

<a id="sceneview-next_focus"></a>

#### `pub fn next_focus(&self, current: Option<ItemId>) -> Option<ItemId>`

Convenience: forward-Tab traversal. See
`focus_in_direction`.

<a id="sceneview-previous_focus"></a>

#### `pub fn previous_focus(&self, current: Option<ItemId>) -> Option<ItemId>`

Convenience: backward-Tab (Shift+Tab) traversal. See
`focus_in_direction`.

<a id="sceneview-pan_x_signal"></a>

#### `pub fn pan_x_signal(&self) -> Signal<f32>`

Live `Signal<f32>` for the X pan offset. Use this from a
parent scene (or any reactive consumer) to derive values
that follow the SceneView's pan — typically axis-label
text in a chart-style outer SceneView.

<a id="sceneview-pan_y_signal"></a>

#### `pub fn pan_y_signal(&self) -> Signal<f32>`

Live `Signal<f32>` for the Y pan offset.

<a id="sceneview-zoom_signal"></a>

#### `pub fn zoom_signal(&self) -> Signal<f32>`

Live `Signal<f32>` for the zoom factor.

<a id="sceneview-rotation_signal"></a>

#### `pub fn rotation_signal(&self) -> Signal<f32>`

Live `Signal<f32>` for the rotation in radians.

<a id="sceneview-view_transform_signal"></a>

#### `pub fn view_transform_signal(&self) -> Signal<Transform2D>`

Live `Signal<Transform2D>` for the composed view transform
(pan + zoom + rotation + bounds-origin). Folds in the
`bounds.origin` contribution so reactive consumers see the
exact transform the renderer applies. Updated whenever any
of the underlying signals change. Use this when the
consumer needs the full matrix (e.g. converting a screen
point to scene coords from outside the SceneView).

<a id="sceneview-a11y_mode"></a>

#### `pub fn a11y_mode(mut self, mode: crate::a11y::A11yMode) -> Self`

Override the `A11yMode` for this
SceneView. Default is `Cooperative` — the visual scene
layout drives AT emission unless explicitly overridden via
`Scene::set_a11y_parent`.
Switch to `StrictlyParallel` when your app's AT shape is
fundamentally different from its visual layout: items
without a declared logical parent are then suppressed from
the AT tree, and the app declares every node it wants AT
users to reach.

<a id="sceneview-retention_margin"></a>

#### `pub fn retention_margin(mut self, screen_px: f32) -> Self`

How far past the viewport edge, in **screen** pixels, a heavyweight
card stays live. Default `DEFAULT_RETENTION_MARGIN` (96).

A card outside the *retention region* — the viewport grown by this
margin, unioned with whatever
`a11y_off_screen_mode` asks to be
listed — is parked dormant. It leaves paint, the layout recursion, the
AccessKit tree and the Tab ring, and keeps its focus, text and
animation state for when the camera brings it back.

**It is a lifecycle knob, not an accessibility one.** Raising it does
not offer assistive technology more of the scene: how much of it a
screen reader is enumerated is decided by
`a11y_off_screen_mode` alone. A card in
the margin band but outside that mode's region is alive and unlisted —
laid out, Tab-reachable, holding its state, absent from the AccessKit
tree. Landing focus on it publishes it, in the same pass.

**Screen pixels, not scene units**, converted through the current zoom
each pass, so the margin is a constant on-screen distance at every zoom
level. That is what it has to be: a pan covers screen distance, not
scene distance, and the margin's job is to have woken a card before the
camera reaches it. At zoom 0.1 the default is 960 scene units.

Three regions, not one, and they are layered: the *tight* viewport
decides who is laid out at full size versus collapsed to zero
(unchanged, zero margin), the accessibility region decides who is
enumerated, and this widest one decides who is dormant. Each contains
the one before it by construction — so a card being laid out is never
also parked, and the AT walk is never asked to describe a card the
arena has parked.

Raising this keeps more cards live — more Tab stops, more layout
recursion, more state held; lowering it parks closer to the edge. Zero
is legal and means "park at the viewport edge"; a card entering is
still woken and laid out in the same pass, so nothing is ever a frame
late, but every crossing then pays a wake.

<a id="sceneview-current_retention_margin"></a>

#### `pub fn current_retention_margin(&self) -> f32`

The current retention margin in screen pixels.

<a id="sceneview-a11y_off_screen_mode"></a>

#### `pub fn a11y_off_screen_mode(mut self, mode: crate::a11y::A11yOffScreenMode) -> Self`

Override the off-screen visibility policy for the AT walker — how much
of a scene that runs past the viewport a screen reader is offered.

Default: `ViewportPlusN { n: 1 }` — everything inside the viewport plus
a one-screen margin on each side. `AllItems` for small scenes where AT
users want a complete table of contents (nothing is ever parked in that
mode, so it costs what it promises); `ViewportOnly` for very large
scenes, where listing off-screen content would bury an AT client.

This governs **both** tiers — lightweight items and heavyweight cards —
and it is the only thing that does.
`retention_margin` can keep a card alive past
this region, for the camera's sake, without adding it here.

<a id="sceneview-default_size"></a>

#### `pub fn default_size(mut self, w: f32, h: f32) -> Self`

Override the size used when the parent doesn't propose one on
an axis. Defaults to 800×600 logical pixels.

<a id="sceneview-adopt_scene_size"></a>

#### `pub fn adopt_scene_size(mut self, on: bool) -> Self`

When set, the view's `layout_response` returns the scene's
`scene_rect_extent` as its own wanted size — the view sizes
itself to its scene rather than to `default_size`. Pairs
naturally with `Scene::pan_axes` / `Scene::zoomable`
to embed bounded, non-navigable scenes inline (mini diagrams,
fixed corkboards). Default `false`.

<a id="sceneview-drag_mode"></a>

#### `pub fn drag_mode(mut self, mode: impl Into<Prop<crate::item_handlers::DragMode>>) -> Self`

Configure how left-mouse drag-on-canvas behaves. Default
`DragMode::RubberBand` — drag-on-an-item
moves it (when `IS_DRAGGABLE`), drag-on-empty-space creates
a marquee. `DragMode::ScrollHandDrag` makes left-drag
pan the view unconditionally; `DragMode::NoDrag` disables
the on-drag handler entirely.

Accepts a static `DragMode` — which sets the current value on the
view's internal signal — or a `Signal<DragMode>` (via
`impl Into<Prop<DragMode>>`), which **replaces** the internal signal
with the app-owned one so a toolbar can hold the same handle and
toggle Hand vs Select vs NoDrag at runtime. To stop sharing, pass a
fresh `Signal::new(mode)`.

<a id="sceneview-view_state"></a>

#### `pub fn view_state( mut self, pan_x: Signal<f32>, pan_y: Signal<f32>, zoom: Signal<f32>, rotation: Signal<f32>, ) -> Self`

Replace the view's pan / zoom / rotation signals with app-owned ones.

The four view-state signals become the app's to hold, share, and
persist — so view state survives a *rebuild-from-state* (a wrapper that
reconstructs the `Scene` + `SceneView` keeps the same signals and the
viewport doesn't jump back to the origin), a "Reset View" button can
snap them, and two views could share one camera. The derived
`view_transform_signal` is recomposed
from the injected signals.

Must be called before the view is added to the tree (like the other
builder methods) — `build()` reads `view_transform_signal` once.

<a id="sceneview-initial_pan"></a>

#### `pub fn initial_pan(self, x: f32, y: f32) -> Self`

Seed the initial pan offset (logical pixels). The view keeps ownership
of the signals; for app-owned signals use `view_state`.

<a id="sceneview-initial_zoom"></a>

#### `pub fn initial_zoom(self, zoom: f32) -> Self`

Seed the initial zoom factor (clamped to the active zoom range).

<a id="sceneview-initial_rotation"></a>

#### `pub fn initial_rotation(self, radians: f32) -> Self`

Seed the initial rotation (radians).

<a id="sceneview-drag_mode_signal"></a>

#### `pub fn drag_mode_signal(&self) -> Signal<crate::item_handlers::DragMode>`

Reactive accessor for the drag mode. Useful for toolbars
that need to read the current mode (e.g. to highlight the
active tool button) and write to it.

<a id="sceneview-background"></a>

#### `pub fn background<F>(mut self, paint: F) -> Self where F: Fn(&mut teksilo_canvas::Canvas, &PaintContext, Rect) + 'static,`

Install a closure painted **before** the items walk. The
canvas already has the view-transform scope pushed, so the
closure paints in scene coords. The `Rect` argument is the
scene-coord visible region — useful for tiled backgrounds
(graph-paper grids, ruled lines, dot grids) so the closure
only emits geometry the user can actually see.

```ignore
SceneView::new(scene).background(|canvas, _ctx, region| {
    // Draw a 50-unit grid covering only the visible region.
    let step = 50.0;
    let x0 = (region.x / step).floor() * step;
    let mut x = x0;
    while x < region.x + region.width {
        canvas.draw_line(/* ... */);
        x += step;
    }
})
```

<a id="sceneview-foreground"></a>

#### `pub fn foreground<F>(mut self, paint: F) -> Self where F: Fn(&mut teksilo_canvas::Canvas, &PaintContext, Rect) + 'static,`

Install a closure painted **after** the items walk and the
marquee, but before any debug overlay. Same coordinate
conventions as `background`. Used for
scene-coord chrome that should ride over content (rulers,
snap-line indicators, drop hints).

<a id="sceneview-wet_layer"></a>

#### `pub fn wet_layer(mut self, layer: crate::WetLayer) -> Self`

Install a `WetLayer` — a surface for content being
authored right now, which repaints without taking the scene's item
bands with it.

Its node is always the **last** child of this view, so wet content sits
above every card and every
`Interleaved` item, and under the
`Over` band and the view's own chrome (marquee, magnet feedback,
transform frame, debug overlay).

Unlike `foreground`, which runs inside the view's
own `post_paint` and therefore cannot be invalidated without repainting
the whole `Under` band with it, this is a node of its own:
`WetLayer::request_repaint` marks it
and nothing else.

Paint-only, hidden from assistive technology and transparent to the
pointer — see `WetLayer`.

The same layer may be installed in **several** views: each mounts its
own node and paints the same painter, the way several views share one
`SceneModel`. Mounting it in a second view does not
unmount it from the first.

<a id="sceneview-magnetism"></a>

#### `pub fn magnetism(mut self, config: crate::magnet::MagnetismConfig) -> Self`

Enable magnetism on this view with the given
`MagnetismConfig`.

Once installed, this view's lightweight item drags snap their
magnets onto compatible magnets on other items, magnet handles
become grabbable for port-drag wires, the keyboard connect flow
(the config's connect key) is available while the view is
focused, magnet markers paint, and each enabled magnet is
emitted as a synthetic AT node. A view with no magnetism config
ignores magnets entirely.

<a id="sceneview-transform_controller"></a>

#### `pub fn transform_controller( mut self, config: crate::transform_session::TransformConfig, ) -> Self`

Install the **selection transform controller** on this view: a frame
with resize and rotate handles drawn around the selection, a group move,
a keyboard route, and accessibility nodes for all of it.

Nothing about the view changes until this is called — with no controller
the pointer rules are exactly what they were.

**Where the gesture may land** is not this controller's business: it is
the scene's, through
`SceneModel::set_geometry_constraint`,
which every gesture here consults on every sample. Snap-to-grid, axis
lock and page clamping live there, once for the document, rather than
once per view.

# What a gesture writes, and when

Nothing, until the release. The session is per-view and model-free: it
reads `SceneSelection`, paints a preview, and
then writes the scene **once**, through a single
`Scene::apply_transform_delta`.
So Esc is "drop the session" with no rollback to get wrong, a ten-item
selection emits no `ItemChange` at all until it lands, and an app that
wants the edit to be reversible has exactly one call to record — from
`TransformConfig::on_end`. The history itself is the data layer's, not
this crate's.

# Which handles appear

Exactly the operations that will be honoured, never more:

| Operation | Flag | Offered when |
| --- | --- | --- |
| Move | `IS_DRAGGABLE` | **any** selection root carries it (the others stay put) |
| Resize | `IS_RESIZABLE` | **every** selection root carries it |
| Rotate | `IS_ROTATABLE` | **every** selection root carries it *and* is lightweight |

Resize and rotate demand unanimity because a partial one would tear the
selection apart — the locked members would stand still while the rest
scaled, and the frame would stop describing what it is drawn around. A
move can be partial because that is what every editor does with a locked
object.

The flags are **honoured, not bypassed**: a default `ItemFlags` carries
`IS_SELECTABLE` and not `IS_DRAGGABLE`, so selecting a decorative item
does not quietly make it draggable.

# Reaching the chrome over a heavyweight card

The frame is drawn `padding` **outside** the
selection, so the band and every handle sit on pixels no card owns. That
matters because a card that claims its own press — one calling
`capture_pointer`, or carrying its own `on_drag`, which is every
`Splitter` handle, `SpinBox` step button and `TextInput` selection drag —
wins that press by design, and this view never sees the gesture. So
**body drag over a card is best-effort and the frame band is the route
that always works.** Over the lightweight tier, and over a card that
claims nothing, both work.

# Keyboard and assistive technology

`t` (see `TransformConfig::transform_key`) enters a handle-roving
mode: `Tab` / `Shift+Tab` move between the handles the frame is
offering, arrows drive the roved one (`Shift` ×10), `Enter` commits,
`Esc` cancels, `t` leaves. The frame and each handle are published as
accessibility nodes with `Increment` / `Decrement`, and both routes go
through the same session and the same commit — so the non-drag
alternative makes the same model change the drag makes, structurally.

```
# use teksilo_scene::{Scene, SceneView, SceneSelectionMode, TransformConfig};
let view = SceneView::new(Scene::new())
    .selection_mode(SceneSelectionMode::Multi)
    .transform_controller(TransformConfig::new().keep_ratio(true));
```

<a id="sceneview-transform_session_signal"></a>

#### `pub fn transform_session_signal( &self, ) -> Signal<Option<crate::transform_session::TransformSession>>`

The live transform session, for an app-owned inspector, status bar or
size readout. `None` between gestures. Bind at
`BindingLevel::RepaintOnly`.

Republished on every **input** sample — each pointer move, each keyboard
step, and at the start and end of a gesture. It is deliberately *not*
republished per frame: during an edge auto-pan the pointer is standing
still while the view slides under it, so the frame keeps changing with
no sample to hang a republish on, and a readout bound here shows the
last sampled numbers until the pointer moves again. What is committed is
always the live resolution, so this lags the frame rather than
disagreeing with the result.

<a id="sceneview-transform_enabled_signal"></a>

#### `pub fn transform_enabled_signal(&self) -> Option<Signal<bool>>`

The reactive enabled signal of the installed transform controller, if
any — for a toolbar to read or bind a "transform tools" toggle.

<a id="sceneview-magnetism_enabled_signal"></a>

#### `pub fn magnetism_enabled_signal(&self) -> Option<Signal<bool>>`

The reactive enabled signal of the installed magnetism config, if
any — for a toolbar to read or bind a magnetism on/off toggle.

<a id="sceneview-invalidate_item_cache"></a>

#### `pub fn invalidate_item_cache(&self, id: ItemId)`

Drop the cached paint output for `id`. Apps that mutate
item-internal state without going through a `Scene` mutator
(e.g. a custom item whose paint depends on a private
`Signal<Color>` that doesn't drive `local_bounds`) call this
to invalidate. The cache is otherwise dropped automatically
on `LocalBoundsChanged` / `ItemReplaced` / `AppearanceChanged` /
`MeasuredSizeChanged` / an `IS_ENABLED` `FlagsChanged` / `Removed` —
but never on opacity / transform / z / `local_pos`, which are applied
as wrapping scopes at replay and don't bake into a cached frame.

<a id="sceneview-item_cache_len"></a>

#### `pub fn item_cache_len(&self) -> usize`

Number of cached entries currently held. Diagnostic / test
hook — apps shouldn't normally need this.

<a id="sceneview-min_zoom"></a>

#### `pub fn min_zoom(self, v: f32) -> Self`

Minimum zoom factor (default 0.1×). Applied as a clamp to all
programmatic and gesture-driven zoom changes via the
view-level `zoom_range_override`.
Shim — updates the lower bound of the override range. The
effective clamp is the intersection of Scene-level
`Scene::set_zoom_range` and
this override (tightening-only — neither side can loosen).

<a id="sceneview-max_zoom"></a>

#### `pub fn max_zoom(self, v: f32) -> Self`

Maximum zoom factor (default 10×). Shim — updates the upper
bound of the override range. See `min_zoom`.

<a id="sceneview-zoom_range_override"></a>

#### `pub fn zoom_range_override(self, range: Option<std::ops::RangeInclusive<f32>>) -> Self`

Replace the view-level zoom-range override wholesale.
`None` clears the override so this view imposes no zoom
clamp of its own (Scene-level constraints still apply).
Tightening rule: the effective clamp is the intersection
with `Scene::current_zoom_range()` — neither can loosen.

<a id="sceneview-zoom_range_override_signal"></a>

#### `pub fn zoom_range_override_signal(&self) -> Signal<Option<std::ops::RangeInclusive<f32>>>`

Reactive accessor for the view-level zoom-range override.
Use this to mutate the override at runtime (e.g. from a
toolbar). Mutations take effect on the next gesture.

<a id="sceneview-pan_bounds_override"></a>

#### `pub fn pan_bounds_override(self, bounds: Option<Rect>) -> Self`

View-level *tightening* override on pan bounds, in scene
coords. The effective clamp at gesture-time is the rect
intersection with `Scene::current_pan_bounds()` — view
overrides cannot loosen what the `Scene` declares. `None`
(default) means no view-side clamp.

<a id="sceneview-pan_bounds_override_signal"></a>

#### `pub fn pan_bounds_override_signal(&self) -> Signal<Option<Rect>>`

Reactive accessor for the view-level pan-bounds override.
Use this to mutate the override at runtime (e.g. dynamically
shrinking the navigable area). Mutations take effect on the
next gesture.

<a id="sceneview-line_height"></a>

#### `pub fn line_height(mut self, px: f32) -> Self`

Logical pixels of pan applied per scroll-wheel line notch.
Defaults to 16 px (matches `ScrollArea`).

<a id="sceneview-overscroll_behavior"></a>

#### `pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self`

Whether a wheel the scene can't absorb (already clamped at its
`pan_bounds`) chains to an ancestor scrollable
(`OverscrollBehavior::Chain`, the default — matches the widget
scrollables) or is contained (`OverscrollBehavior::Contain`). Use
`Contain` for a tightly-bounded scene embedded in a scroll view that
should never steal the scene's wheel.

<a id="sceneview-with_scroll_bars"></a>

#### `pub fn with_scroll_bars(self) -> crate::SceneScrollView`

Wrap this view in a `SceneScrollView`, adding
draggable scroll bars with the widget-tier `ScrollArea`'s options (mode,
per-axis policy, thickness). The bars track the camera and drive panning;
native wheel / drag panning — and its smoothing — keeps working.

Configure the result with the `SceneScrollView` builder methods:

```no_run
# use teksilo_scene::{Scene, SceneView, ScrollBarMode, ScrollBarPolicy};
let scrollable = SceneView::new(Scene::new())
    .with_scroll_bars()
    .scroll_bar_mode(ScrollBarMode::Overlay)
    .vertical_policy(ScrollBarPolicy::AsNeeded);
# let _ = scrollable;
```

<a id="sceneview-scene"></a>

#### `pub fn scene(&self) -> std::cell::Ref<'_, Scene>`

Read access to the underlying scene, as a borrow guard.

Prefer the cloneable `model` handle for multi-view
wiring and scene mutation (its methods are `&self`); this guard is the
single-view escape hatch for ad-hoc reads.

<a id="sceneview-scene_mut"></a>

#### `pub fn scene_mut(&mut self) -> std::cell::RefMut<'_, Scene>`

Mutable access to the underlying scene, as a borrow guard.

Single-view escape hatch. For multi-view, mutate through the shared
`SceneModel` handle (`model`) — every
mutator is `&self`, so a handler holding a clone can drive the scene
directly (no `with_widget_mut` needed) and **all** views reconcile:

```
# use teksilo_scene::{Scene, SceneView};
# use teksilo_canvas::Rect;
# let view = SceneView::new(Scene::new());
# let card_data = "example payload";
# let rect = Rect::new(0.0, 0.0, 200.0, 120.0);
let model = view.model();          // cheap handle clone
model.add_widget_item(card_data, rect);   // every view rebuilds it
```

The view self-reconciles on every mutation: `add_widget_item` /
`add_item` materialise on the next rebuild, `remove` destroys the
orphaned arena widget and cleans its maps, `set_payload` rebuilds an
item's widget, and **both** the visual tree and the *separate* AccessKit
tree re-walk (geometry, reparents, and pure-a11y mutations all reach
assistive tech — `build()` requests an AT re-walk, since a relayout no
longer does so on its own).

<a id="sceneview-widget_id_for"></a>

#### `pub fn widget_id_for(&self, id: ItemId) -> Option<WidgetId>`

The `WidgetId` an item was materialised as, if known.

<a id="sceneview-pan"></a>

#### `pub fn pan(&self) -> Vec2`

Current pan offset (logical pixels).

<a id="sceneview-zoom"></a>

#### `pub fn zoom(&self) -> f32`

Current zoom factor.

<a id="sceneview-rotation"></a>

#### `pub fn rotation(&self) -> f32`

Current rotation in radians.

<a id="sceneview-pan_x_animation_target"></a>

#### `pub fn pan_x_animation_target(&self) -> Option<f32>`

In-flight animation target for the X pan signal, or `None`
if the signal is at rest. Useful for tests that want to
observe a tween before it lands without spinning the
scheduler.

<a id="sceneview-pan_y_animation_target"></a>

#### `pub fn pan_y_animation_target(&self) -> Option<f32>`

In-flight animation target for the Y pan signal.

<a id="sceneview-zoom_animation_target"></a>

#### `pub fn zoom_animation_target(&self) -> Option<f32>`

In-flight animation target for the zoom signal.

<a id="sceneview-view_transform"></a>

#### `pub fn view_transform(&self) -> Transform2D`

The composed view transform the render walker has on its
stack while painting this view's subtree. Includes the
`bounds.origin` offset captured during the last
`place_children` call, so this is the exact transform
applied to scene-coord points by the renderer.

<a id="sceneview-view_scale"></a>

#### `pub fn view_scale(&self) -> f32`

The view's uniform zoom — the scale of the view transform's linear
part, the same number pointer dispatch passes to
`ItemShape::contains`.

It reaches exactly one thing: a **cosmetic** (device-pixel) stroke
band, whose width in scene coordinates shrinks as the view zooms in.
Computed as the hypotenuse of the transform's first column so a pure
rotation reports `1.0`.

<a id="sceneview-map_to_scene"></a>

#### `pub fn map_to_scene(&self, view_pt: Point) -> Point`

Project a point in **view space** (screen-pixel coords —
the same frame pointer events arrive in) into scene
coordinates. Inverse of `map_from_scene`.
Returns the scene origin when the view transform is
degenerate (e.g. zoom = 0).

<a id="sceneview-map_from_scene"></a>

#### `pub fn map_from_scene(&self, scene_pt: Point) -> Point`

Project a point in **scene coords** to view space (screen
pixels). Inverse of `map_to_scene`.

<a id="sceneview-map_rect_to_scene"></a>

#### `pub fn map_rect_to_scene(&self, view_rect: Rect) -> Rect`

Project a rectangle in view space into scene coordinates.
Returns the AABB of the four projected corners under
rotation. Empty rect when the view transform is degenerate.

<a id="sceneview-map_rect_from_scene"></a>

#### `pub fn map_rect_from_scene(&self, scene_rect: Rect) -> Rect`

Project a rectangle in scene coords into view space.

<a id="sceneview-viewport_in_scene_signal"></a>

#### `pub fn viewport_in_scene_signal(&self) -> Signal<Rect>`

Reactive signal of the **visible scene region** — the
portion of scene space currently inside the SceneView's
viewport. Fires whenever pan / zoom / rotation /
bounds_origin / viewport-size changes.

Use to drive a minimap viewport indicator, lazy-load only
the visible scene region, or implement "scroll into view"
guards. The value is the AABB of the viewport rectangle
projected through `view_transform.inverse()`.

<a id="sceneview-viewport_size_signal"></a>

#### `pub fn viewport_size_signal(&self) -> Signal<Size>`

Reactive signal of the SceneView's resolved viewport size.
Fires whenever `layout_response` resolves a new size that
differs from the previous.

<a id="sceneview-pan_to"></a>

#### `pub fn pan_to(&self, target: Vec2, duration: Duration)`

Animate pan to `target` over `duration`. Bounded by
`Easing::EaseOut`. Honours `prefers-reduced-motion` only
indirectly: the scheduler pauses animation on window-inactive
and the test seam allows snapping. For an explicit snap, call
`SceneView::set_pan`.

<a id="sceneview-set_pan"></a>

#### `pub fn set_pan(&self, target: Vec2)`

Snap pan to `target` without animation. Gated by the scene's
`PanAxes` policy.

<a id="sceneview-zoom_to"></a>

#### `pub fn zoom_to(&self, target: f32, duration: Duration)`

Animate zoom to `target` over `duration`, clamped to
`[min_zoom, max_zoom]`. No-op when the scene declares
`Scene::zoomable(false)`.

<a id="sceneview-set_zoom"></a>

#### `pub fn set_zoom(&self, target: f32)`

Snap zoom to `target` without animation, clamped. No-op when
the scene declares zoom disabled.

<a id="sceneview-ensure_visible"></a>

#### `pub fn ensure_visible(&self, scene_rect: Rect, margin: f32)`

Pan (without changing zoom) so `scene_rect.expand(margin)`
fits inside the current visible scene region. If the
expanded target rect already fits, this is a no-op.

Animated, over the view's pan-animation duration. For an instant jump,
or to pin the target at a fraction of the viewport rather than merely
reveal it, use
`ensure_visible_aligned`.

# Who calls this

An app scrolling to a search hit or a newly created item calls it
directly. The framework calls it for every
`teksilo_core::event::WidgetEvent::ScrollIntoView`
that reaches the view — which is how a caret inside an embedded editor
moves the camera, since a `SceneView` clips its children and is
therefore on the reveal walk. (It is **not** called by focus traversal:
this doc used to claim it was, and nothing in the crate ever did.)

Pan is gated by `Scene::pan_axes`:
if a scene declares `PanAxes::None`, this is a no-op; if it
declares a single axis, only that axis pans. Items can't be
scrolled into view if the policy doesn't permit panning
toward them.

<a id="sceneview-ensure_visible_aligned"></a>

#### `pub fn ensure_visible_aligned( &self, scene_rect: Rect, margin: f32, align: ScrollAlign, motion: ScrollMotion, ) -> Vec2`

`ensure_visible` with an explicit alignment and
motion, reporting the shift it applied in **scene** coordinates.

* `ScrollAlign::Minimal` reveals the target and does nothing when it
  is already visible; `ScrollAlign::Fraction` *pins* its top `f` of
  the way down the viewport whether or not it was visible.
* `ScrollMotion::Instant` snaps. That is what a caret chase wants:
  animating a reveal that re-fires on every keystroke is what produces
  the screen-bouncing typewriter modes are complained about for.

The returned vector is `Vec2::ZERO` when nothing moved, and is what the
`ScrollIntoView`
`applied_scroll` back-channel carries: the shift the target appears to
make **relative to the viewport**, stated in the space the caller asked
in. The reveal walk subtracts it from the rectangle before projecting
that rectangle out to the next container, using the transform it read
*before* dispatching — so a screen-space pan delta there would be wrong
by a factor of the zoom, and under rotation by an angle.

<a id="sceneview-rotate_to"></a>

#### `pub fn rotate_to(&self, target_radians: f32, duration: Duration)`

Animate rotation to `target` over `duration` (radians).

<a id="sceneview-set_rotation"></a>

#### `pub fn set_rotation(&self, target_radians: f32)`

Snap rotation to `target` without animation.

<a id="sceneview-state"></a>

#### `pub fn state(&self) -> crate::SceneViewState`

Snapshot the current pan / zoom / rotation as a
`SceneViewState`. Designed for
persistence: store the snapshot in your settings layer on
app exit, restore it via `restore_state`
on next launch.

The snapshot reflects the *current* signal values — if a
pan/zoom animation is in flight, the captured values are
the in-flight tween position, not the eventual target.
Apps that want to capture the target should query
`pan_x_animation_target` /
friends manually.

<a id="sceneview-restore_state"></a>

#### `pub fn restore_state(&self, state: crate::SceneViewState)`

Restore a previously captured `SceneViewState`.
Snaps each signal to the saved value (no animation —
pan/zoom/rotation jump to the persisted state immediately).
Zoom is clamped to `[min_zoom, max_zoom]`.

<a id="sceneview-viewport_size"></a>

#### `pub fn viewport_size(&self) -> Size`

Latest viewport size observed during layout. Useful for
imperative `fit_*` calls.

<a id="sceneview-scene_content_bounds"></a>

#### `pub fn scene_content_bounds(&self) -> Option<Rect>`

Compute the bounding rectangle (in scene coords) that encloses
every item in the scene. Returns `None` for an empty scene.

<a id="sceneview-fit_to_content"></a>

#### `pub fn fit_to_content(&self)`

Animate pan + zoom so the scene's content bounding box fits
the current viewport with a small margin. No-op for an empty
scene. Resets rotation to 0.

<a id="sceneview-fit_to_items"></a>

#### `pub fn fit_to_items(&self, ids: &[ItemId])`

Animate pan + zoom so the union of the given items' bounds
fits the current viewport. Ids not currently in the scene
are skipped silently. No-op if `ids` is empty or all ids are
stale. Resets rotation to 0.

Use this for "zoom to selection" / "frame this subset" UX.

<a id="sceneview-fit_to_selection"></a>

#### `pub fn fit_to_selection(&self)`

Animate pan + zoom so the bounds of the currently selected
items fit the viewport. No-op when nothing is selected.
Convenience for the common "F to focus selection" hotkey.
