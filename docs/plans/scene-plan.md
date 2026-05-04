# Scene plan — `fern-scene`

A pannable/zoomable scene viewport sub-toolkit for FernUI: free-positioned
widgets and lightweight scene items under one view transform, fully
accessible, fully idle-compliant.

## 1. Context

FernUI's widget tree is layout-algorithm-driven: every container places its
children via `place_children` measurement logic. This is correct for
traditional UIs (forms, toolbars, dialogs) but leaves a gap for
**scene-based applications** — story corkboards, mind maps, node-graph
editors, timeline views, character-relationship graphs — where content is
**free-positioned**, **pan/zoom navigable**, potentially **dense**
(thousands of items, only a viewport-worth visible per frame), and
**mixed-weight** (some items deserve full widget machinery, most don't).

Qt addresses this with `QGraphicsView`/`QGraphicsScene`/`QGraphicsItem`
plus a `QGraphicsProxyWidget` bridge. FernUI's architecture lets us do
better: every scope-level primitive we need already exists (per-node
transform/clip/opacity scopes that compose, animated `Signal<f32>` with
idle gates, gesture arena with pinch already plumbed from winit, synthetic
AT NodeIds for virtualization). The result is one unified container —
`SceneView` — that hosts both **heavyweight widgets** (any `Widget`,
dropped in unchanged, fully interactive and accessible) and **lightweight
`SceneItem`s** (paint/bounds/hit-test/a11y, no arena overhead) under one
view transform with one spatial index.

Two non-negotiable constraints, both naturally satisfiable given the
existing primitives:

- **Accessibility-as-tools, not visual-derivative.** A scene's logical AT
  shape is *not predictable from its visual layout*. A node-graph
  editor's AT tree is "nodes connected by data-flow ports"; a Skribisto
  corkboard's is "Acts containing Scene cards"; a CAD canvas's is
  "layers of geometric components". Reading-order Tab over screen-
  projected bounds is wrong for all three. The framework therefore
  exposes a **parallel structural layer**: app authors declare logical
  parents, navigation order, group containers, relationships, rotor
  categories, landmarks, and live regions — independent of visual
  containment. The defaults exist (reading-order, GraphicsObject role)
  so a quick prototype is accessible out of the box, but they're 100%
  overridable.
- **Zero CPU/GPU drain when idle.** Pan/zoom/rotation/inertial-fling are
  all animated `Signal<f32>` with epsilon, automatically participating
  in the framework's 4-gate idle scheduler. No per-frame ticks anywhere
  in fern-scene.

## 2. Design targets

1. Two-tier content (heavy widgets + lightweight items) sharing one
   spatial index, one view transform, one a11y walker.
2. **A11y as tools, not visual-derivative.** Both tiers fully accessible
   by default (reading-order Tab, screen-projected bounds, default item
   roles), AND every default 100% overridable: app-declared logical AT
   tree (groups, parents), custom focus order, custom directional
   navigation, relations (described-by / labelled-by / controls /
   flow-to), live regions, landmarks, rotor categories. Default
   `OffScreenA11y::ViewportPlusN { n: 1 }` for the visual-default
   walker.
3. Zero idle drain — pan/zoom never schedule frames at rest.
4. Transform-aware hit-test (foundational fix in fern-core; benefits
   Scale/Rotate widgets too — landed Phase 0).
5. Spatial index (grid-hash MVP, R-tree pluggable behind a trait later).
6. Viewport culling — off-screen items don't lay out, don't paint, don't
   burden the walk.
7. OS gestures wired: pinch-zoom, two-finger pan, Ctrl-wheel
   zoom-about-pointer, modifier-aware scroll.
8. Inertial fling via `animate_to` (finite-duration, terminal-tick
   lands, reduced-motion snaps).
9. Keyboard navigation (Tab in reading order, arrows pan/nudge,
   Home/End, +/- zoom).
10. Marquee/lasso selection.
11. No fork of widget machinery — any `Widget` drops into a `WidgetItem`
    unchanged.
12. Two reference docs: **`docs/fern-scene.md`** (overview, model,
    layout, transforms, gestures, examples) and
    **`docs/fern-scene-a11y.md`** (logical AT tree, groups, relations,
    auto-graft, override knobs, custom focus order, worked examples).

## 3. Crate layout

`crates/fern-scene/` — same dependency tier as `fern-charts` (depends on
`fern-core`, `fern-canvas`, `fern-tokens`, `fern-data`; **not** on
`fern-widgets`; widgets only as dev-dep for tests/examples). No `mod.rs`
files, per project convention. Apps depend on `fern-scene` as a peer of
`fern-ui` (not re-exported through the umbrella) — same pattern as
`fern-charts`.

```text
crates/fern-scene/src/
  lib.rs              # public re-exports + crate doc
  view.rs             # SceneView container, gesture wiring, view transform
  scene.rs            # Scene model (items, signals, spatial-index integration)
  item.rs             # SceneItem trait + RectItem/PathItem/ImageItem/TextItem/GroupItem
  widget_item.rs      # WidgetItem (heavyweight Widget at a scene_rect)
  transform.rs        # Affine2 helpers, pan/zoom/rotate composition
  index.rs            # SpatialIndex trait + GridHashIndex
  gestures.rs         # PanGestureRecognizer, MarqueeRecognizer
  a11y.rs             # synthetic-NodeId emission + a11y debounce
  keyboard.rs         # SceneFocusOrder + arrow/Tab/Home/End/+/- handler
  tests/
    integration.rs
    a11y.rs
    gestures.rs
```

## 4. Public API surface (skeleton)

### Core types

```rust
// item id — opaque newtype over u64, generated by Scene
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemId(u64);

// the data model: items + spatial index + invalidation signal
pub struct Scene { /* … */ }

impl Scene {
    pub fn new() -> Self;
    pub fn with_index(index: Box<dyn SpatialIndex>) -> Self;   // Phase 3+
    pub fn add_widget<W: Widget + 'static>(&mut self, w: W, scene_rect: Rect) -> ItemId;
    pub fn add_item(&mut self, item: Box<dyn SceneItem>) -> ItemId;     // Phase 4
    pub fn move_item(&mut self, id: ItemId, new_bounds: Rect);
    pub fn remove(&mut self, id: ItemId);
    pub fn items_in_rect(&self, scene_rect: Rect) -> Vec<ItemId>;
    pub fn invalidate_signal(&self) -> Signal<u64>;
}

// the viewport widget
pub struct SceneView { /* fields: scene + 4 signals + config */ }

impl SceneView {
    pub fn new(scene: Scene) -> Self;

    // Phase 2+ configuration
    pub fn min_zoom(self, v: f32) -> Self;
    pub fn max_zoom(self, v: f32) -> Self;
    pub fn zoom_anchor(self, anchor: ZoomAnchor) -> Self;
    pub fn pan_modes(self, modes: PanModes) -> Self;
    pub fn show_grid(self, show: bool) -> Self;
    pub fn snap_to_grid(self, snap: bool) -> Self;
    pub fn grid_size(self, px: f32) -> Self;
    pub fn a11y_off_screen_mode(self, mode: A11yOffScreenMode) -> Self;
    pub fn reduced_motion_override(self, snap: bool) -> Self;

    // imperative animation (Phase 2+)
    pub fn pan_to(&self, target: Vec2, duration: Duration);
    pub fn zoom_to(&self, target: f32, duration: Duration);
    pub fn fit_to_content(&self);
    pub fn fit_to_selection(&self, ids: &[ItemId]);
    pub fn focus_item(&self, id: ItemId);

    pub fn pan(&self) -> Vec2;
    pub fn zoom(&self) -> f32;
    pub fn rotation(&self) -> f32;
    pub fn view_transform(&self) -> Transform2D;
    pub fn scene_mut(&mut self) -> &mut Scene;
}

#[derive(Default)]
pub struct PanModes {
    pub scroll: bool,
    pub two_finger_trackpad: bool,
    pub middle_drag: bool,
}

pub enum ZoomAnchor { Pointer, ViewportCenter, SceneCenter }

pub enum A11yOffScreenMode {
    AllItems,
    ViewportPlusN { n: u32 },    // default n=1
    ViewportOnly,
}
```

### Lightweight item trait (Phase 4+)

```rust
pub trait SceneItem: Send + 'static {
    fn bounds_in_scene(&self) -> Rect;
    fn paint(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext);
    fn hit_test(&self, scene_point: Point) -> bool {
        self.bounds_in_scene().contains(scene_point)
    }
    fn accessibility(&self, builder: &mut AccessNodeBuilder, ctx: &SceneItemA11yContext) {
        builder.set_role(accesskit::Role::GraphicsObject);
        if let Some(label) = self.label() { builder.set_label(label); }
    }
    fn label(&self) -> Option<&str> { None }
}

pub struct SceneItemPaintContext<'a> {
    pub view_transform: Transform2D,
    pub theme: &'a Theme,
    pub scene_dirty_rect: Option<Rect>,
}

pub struct SceneItemA11yContext {
    pub view_transform: Transform2D,
    pub screen_bounds: Rect,
    pub item_id: ItemId,
}
```

Built-in items (all Phase 4): `RectItem`, `PathItem`, `ImageItem`,
`TextItem`, `GroupItem`. Heavyweight bridge: `WidgetItem`.

### Spatial index trait (Phase 3+)

```rust
pub trait SpatialIndex: Send {
    fn insert(&mut self, id: ItemId, bounds: Rect);
    fn remove(&mut self, id: ItemId);
    fn query(&self, scene_rect: Rect) -> Vec<ItemId>;
}

pub struct GridHashIndex { /* … */ }
```

### Accessibility-shaping API (Phase 5b — the parallel structural layer)

Logical AT structure is declared independently of visual scene-rect
placement. Apps wire whatever shape makes sense for their domain.

```rust
pub struct A11yGroup { /* id, role, label, members, parent */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct A11yGroupId(u64);

impl Scene {
    pub fn add_a11y_group(&mut self, builder: A11yGroupBuilder) -> A11yGroupId;
    pub fn remove_a11y_group(&mut self, id: A11yGroupId);
    pub fn set_a11y_parent(&mut self, child: A11yNode, parent: Option<A11yNode>);
    pub fn add_a11y_relation(&mut self, from: A11yNode, kind: A11yRelation, to: A11yNode);
    pub fn set_a11y_live(&mut self, node: A11yNode, live: accesskit::Live);
    pub fn set_a11y_landmark(&mut self, node: A11yNode, role: accesskit::Role);
    pub fn set_a11y_categories(&mut self, node: A11yNode, categories: &[A11yCategory]);
}

#[derive(Debug, Clone, Copy)]
pub enum A11yNode {
    Item(ItemId),               // a lightweight SceneItem
    Group(A11yGroupId),         // a virtual A11yGroup
    Widget(WidgetId),           // a real interactive widget — addressable
                                // directly so apps can relocate it elsewhere
                                // in the logical tree
}

pub enum A11yRelation { Controls, LabelledBy, DescribedBy, FlowTo, MemberOf }

pub struct A11yCategory(pub Cow<'static, str>);

impl SceneView {
    pub fn focus_order(self, f: impl Fn(&Scene, FocusOrderContext) -> Vec<ItemId> + 'static) -> Self;
    pub fn directional_navigation(self, f: impl Fn(&Scene, NavCtx) -> Option<ItemId> + 'static) -> Self;
    pub fn a11y_bounds_space(self, space: A11yBoundsSpace) -> Self;
    pub fn disable_default_keyboard_nav(self) -> Self;
    pub fn default_item_role(self, role: accesskit::Role) -> Self;
}

pub enum A11yBoundsSpace { Screen, Scene }
```

Per-item builder chain, mirroring `WidgetBuilder::access_*`:

```rust
impl<I: SceneItem> SceneItemBuilder<I> {
    pub fn access_label(self, label: impl Into<String>) -> Self;
    pub fn access_description(self, desc: impl Into<String>) -> Self;
    pub fn access_role(self, role: accesskit::Role) -> Self;
    pub fn access_value(self, value: impl Into<String>) -> Self;
    pub fn access_disabled(self, disabled: bool) -> Self;
    pub fn access_hidden(self, hidden: bool) -> Self;
    pub fn access_action(self, action: accesskit::Action,
                         cb: impl Fn(SceneItemActionCtx) + 'static) -> Self;
    pub fn access_custom_action(self, name: impl Into<String>,
                                cb: impl Fn(SceneItemActionCtx) + 'static) -> Self;
    pub fn access_described_by(self, target: A11yNode) -> Self;
    pub fn access_labelled_by(self, target: A11yNode) -> Self;
    pub fn access_controls(self, target: A11yNode) -> Self;
    pub fn access_flow_to(self, target: A11yNode) -> Self;
    pub fn access_live(self, live: accesskit::Live) -> Self;
    pub fn access_categories(self, cats: &[A11yCategory]) -> Self;
    pub fn access_subtree(self, mode: AccessSubtreeMode) -> Self;
    pub fn access_customize(self, f: impl FnOnce(&mut AccessNodeBuilder) + 'static) -> Self;

    /// How interactive widget descendants of this item participate in
    /// the AT logical tree. Default `AutoGraft`. `Suppress`: no widget
    /// descendants emitted under this item. `Manual`: widgets only
    /// appear here if explicitly placed via
    /// `Scene::set_a11y_parent(A11yNode::Widget(id), this_item)`.
    pub fn a11y_descendants(self, mode: A11yDescendantsMode) -> Self;
}

pub enum A11yDescendantsMode { AutoGraft, Suppress, Manual }
```

Heavyweight `WidgetItem`s reuse the **existing** `WidgetBuilder::access_*`
surface — no parallel API. Logical groups and items support pure AT-only
behavior via the same `access_action` / `access_custom_action` chain —
the "minimal shadow" case for actions with no visual counterpart.

## 5. Mechanism design (load-bearing decisions)

**View transform.** Four `Signal<f32>` (`pan_x`, `pan_y`, `zoom`,
`rotation`) — separate so each can animate independently with the right
epsilon. A derived `view_transform: Signal<Transform2D>` zips them.
`SceneView::build` calls `ctx.set_transform(content_id, view_transform)`.
The framework's render walker composes `set_transform` scopes (see
`fern-render/src/renderer.rs` and `WidgetArena::effective_transform`),
so every item under SceneView automatically inherits the view transform
and per-item transforms compose on top.

**Free positioning.** `SceneView` overrides `place_children` to plant
each `WidgetItem` at its `scene_rect.origin`/`size` in parent-local
coords (no layout algorithm). In Phase 1 SceneView is also the visible
root; from Phase 2 it applies the view transform via `set_transform`.

**Hit-test (Phase 0, landed).** Fern-core's `hit_test_recursive_excluding`
inverse-transforms the input point at each `transform_prop` boundary,
threading the local-space point through children. `Transform2D::inverse`
and `Arena::effective_transform` were introduced in the same change.

**Spatial index (Phase 3+).** `GridHashIndex` with cell ≈ 1.5× median
item bounds. Items insert on `Scene::add_*`, re-bucket on `move_item`.
SceneView's `place_children` and paint walk both query
`index.query(viewport_in_scene)` instead of iterating all items.
Trait-based so an R-tree variant slots in later (Phase 7).

**Viewport culling.** `viewport_in_scene = view_transform.inverse() *
widget_bounds`. Spatial-index query gives candidates; only those enter
the layout/paint walk. Off-screen `WidgetItem`s don't get a layout
proposal (no bounds populated, no paint, no a11y emission). Combined
with the existing per-widget `paint_epoch` gate, a docked SceneView
with thousands of off-screen items costs essentially nothing.

**Pan/zoom gestures.**

- `WidgetEvent::Scroll { delta, modifiers }`: Ctrl-modifier →
  `zoom.animate_to` about pointer (zoom anchor logic adjusts pan to
  keep the pointed-at scene point fixed). Plain → `pan.animate_to`.
- `WidgetEvent::Gesture(GestureEvent::PinchChanged { center, scale,
  .. })`: already plumbed from winit at
  `crates/fern-platform/src/event_translation.rs:216`. Apply scale
  around `center`, animate.
- Two-finger trackpad pan = `ScrollDelta::Pixels` with no modifiers;
  same path as plain scroll.
- All updates flow through `Signal::animate_to(target, duration,
  easing, epsilon)`, naturally registered with the scheduler.
  Window-unfocused pause and reduced-motion snapping inherit from the
  scheduler.

**Inertial fling.** `PanGestureRecognizer` tracks pointer velocity. On
release with velocity above threshold (~50 px/s), schedule
`pan.animate_to(current + v * decay_t, decay_t, EaseOut)`. Finite
duration → terminal tick → loop sleeps. Reduced-motion path skips the
`animate_to` and lands instantly.

**Marquee selection.** `MarqueeRecognizer` (Alt-click-drag, or modal).
Held in SceneView state. While active, paints a semi-transparent
overlay rect; on release, converts screen-rect → scene-rect via
`view_transform.inverse()` and queries `Scene::items_in_rect`.

**Keyboard navigation.** SceneView injects a `SceneFocusOrder` overriding
the arena's natural Tab cycle. Reading order: items sorted by bounds-
center y (with row-tolerance ≈ 1.5× median item height to group rows),
then x. Arrow keys: `KeyboardMode::Pan` → animate pan by ~viewport/4;
`KeyboardMode::Select` → 4-way nearest-neighbor in scene coords.
`Home`/`End` jump to first/last by reading order. `+`/`-` zoom about
viewport center. Held-key repeat: `animate_to` replaces the in-flight
tween, so the signal smoothly chains.

**Accessibility — visual default vs logical override (Phase 5).** The AT
walker emits a tree in two passes: (1) **logical pass** — if the app
declared logical structure via `Scene::set_a11y_parent` /
`add_a11y_group`, walk that tree first and emit synthetic NodeIds for
groups + items in declared order. (2) **fallback pass** — items not
assigned a logical parent fall through to the default visual emission
(children of SceneView, ordered by reading order). Heavyweight items
participate in either pass identically.

**Accessibility — heavyweight items.** Bounds free: `arena.bounds(id)`
is post-layout/post-transform screen space. What's NOT free is *logical
placement* in the AT tree — by default a `WidgetItem`'s AT node is a
direct child of SceneView, but `Scene::set_a11y_parent` can reparent it.

**Accessibility — lightweight items.** Extend `SyntheticKind` (currently
Paragraph/TextRun/ImageRun/Link at
`crates/fern-core/src/accessibility.rs:46-56`) with
`SyntheticKind::SceneItem` and `SyntheticKind::SceneGroup`. SceneView's
`accessibility(&self, builder, ctx)` walks visible items per
`A11yOffScreenMode` policy:

- `node_id = synthetic_node_id(self_id, item_id.0, SyntheticKind::SceneItem)`
  — deterministic, stable across rebuilds.
- `screen_bounds = view_transform * item.bounds_in_scene()` (one
  matrix-mul per item, only at AT rebuild). When
  `a11y_bounds_space(Scene)` is configured, emit scene-coord bounds.
- `item.accessibility(builder, ctx)` populates role/label/value/
  actions; per-item builder chain (`access_*`) layered overrides
  apply on top.
- Logical groups (`A11yGroup`s) get their own synthetic NodeIds via
  `SyntheticKind::SceneGroup`; their children are appended in the
  order the app declared.
- Relations declared via `Scene::add_a11y_relation` are written into
  AccessKit relationship arrays.
- App-tagged categories (`A11yCategory`) are stored as a custom
  property on the AT node — surfaced to AT clients that support
  categorized navigation (VoiceOver rotor on macOS, NVDA quick-nav).

**Custom navigation hooks.** Default reading-order Tab cycle wraps a
callback: if the app installs `SceneView::focus_order(...)`, that
callback drives Tab/Shift+Tab. Same shape for
`directional_navigation(...)`.

**Interactive widgets inside the logical tree (the ComboBox-in-a-scene
case).** Three composable mechanisms:

1. **Auto-graft (default).** During the logical-pass walk, when emitting
   an item's AT subtree, the walker descends into that item's visual
   widget descendants and emits each interactive widget's AT node (real
   `WidgetId`, not synthetic) as a child of the logical item, in
   declaration order. The widget keeps its full machinery — focus
   trap when expanded, native keyboard, AccessKit role, action
   dispatch. From the AT user's view: navigating to "Scene B"
   announces "Scene B, group, contains: ComboBox, Character"; the
   ComboBox is the next Tab stop after Scene B; activating it works
   exactly as anywhere else.
2. **Explicit relocation via `set_a11y_parent(A11yNode::Widget(combo_id),
   parent)`.** For widgets visually nested in one place but logically
   belonging elsewhere (a global character picker visually nested in
   Scene B but logically under "Story Tools"). The widget is grafted
   at the declared parent instead of its visual ancestor.
3. **Per-item opt-out via `a11y_descendants(Suppress | Manual)`.**
   Items can decline auto-graft entirely (Suppress) or accept only
   explicitly-placed widgets (Manual). The "preview card" use case.

No "hop in / hop out" mode: the widget's existing focus semantics
already do the right thing. When Tab is pressed inside an active
ComboBox, the widget consumes it; when the ComboBox is closed and Tab
fires, the framework asks SceneView's `focus_order` for the next
logical item. The callback gets a helper
`FocusOrderContext::expand(item) -> impl Iterator<…>` that yields the
item then its grafted interactive descendants in DFS order.

No persistent shadow widgets: the *minimal* shadow case (AT-only actions
with no visual counterpart) is covered by `A11yGroup::access_action` /
`SceneItemBuilder::access_custom_action`, which register a callback
dispatched on AT-invoked action without any visual equivalent. No
state-sync, no proxy drift.

**Off-screen a11y policy.** Default `ViewportPlusN { n: 1 }` — items
intersecting `viewport ∪ (1× viewport-grown-rect)` appear in AT.
AT-focusing an item that's in-tree but visually off-screen triggers
`SceneView::focus_item(id)` which animates pan/zoom (300ms EaseInOut,
finite, idle-friendly).

**A11y debounce during pan/zoom.** While view-transform signals are
mid-tween, AT bounds are in flight. Skip AccessKit commits until 100ms
after the view transform reaches its terminal tick.

**AT actions on lightweight items.** `SceneView` keeps a
`HashMap<ItemId, HashMap<Action, Rc<dyn Fn>>>`. Incoming
`WidgetEvent::AccessAction { target_node_id, action }` for one of
SceneView's synthetic children → reverse-lookup `node_id → item_id` →
dispatch.

**Idle compliance — explicit checklist.**

- Pan/zoom/rotation are animated `Signal<f32>` registered via
  `register_animated_signal` → 4-gate scheduler applies.
- Epsilon: pan ε = 0.5 px (sub-pixel invisible); zoom ε = 0.001 in log2
  space (~0.07% multiplicative); rotation ε = 1e-3 rad (~0.057°).
- On SceneView destroy: `scheduler.cancel_by_widget(self_id)`.
- Window-unfocused pause and reduced-motion snap inherited from the
  scheduler.
- Looping animations on lightweight items register against SceneView's
  widget id (lightweight items have no widget ids); off-screen items
  inside a visible SceneView still tick — documented limitation.
  Mitigation: discourage looping animations on light items; provide a
  `pulse_once` helper for one-shots.
- No `tick`, no `frame_tick_requested`, no manual scheduler.poll
  anywhere in fern-scene.

## 6. Phased rollout

Each phase = one feature-aligned PR with tests, doc updates, and an
example commit. Mirrors the cadence of `docs/plans/charts-plan.md` and
`docs/plans/settings-plan.md`. The example app at
`examples/scene_corkboard/` evolves alongside the crate.

### Phase 0 — Transform-aware hit-test (fern-core, foundational) ✅

**Where:** `crates/fern-core/src/widget_tree/event_dispatch_impl.rs` +
`arena.rs` + `crates/fern-canvas/src/geometry.rs`.

**What:** added `Transform2D::inverse()` and `WidgetArena::
effective_transform(id)`. Rewrote `hit_test_recursive_excluding` to
inverse-transform the point at each `transform_prop` boundary.

**Outcome:** Existing widgets under `Scale` / `Rotate` (and any future
`set_transform` consumer) become clickable on their visually-displayed
area. Foundation for fern-scene's view transform.

### Phase 1 — Crate skeleton + Scene + free positioning + minimal SceneView

**What:** new `crates/fern-scene/` crate. `Scene` (no spatial index yet
— brute-force list). `SceneView` with **identity** view transform.
`WidgetItem` heavyweight wrapper. No pan/zoom. No lightweight items. No
a11y customization beyond defaults.

**Example v1:** `examples/scene_corkboard/` — 9 cards in a 3×3 grid at
fixed scene coords. Each card is a `Panel` with title + body
`TextWidget`. Click to focus.

**Docs:** create `docs/plans/scene-plan.md` (this doc); stub
`docs/fern-scene.md` covering Scene/SceneView/WidgetItem/free-positioning
+ tiny worked example.

**Tests:** `Scene::add_widget` round-trip; `SceneView` places at scene
coords (verify `arena.bounds(id)` matches expected origin).

### Phase 2 — View transform, pan, zoom, gestures, inertial fling

**What:** four signals (`pan_x/pan_y/zoom/rotation`) wired to
`set_transform`. `on_scroll` (Ctrl-wheel zoom about pointer; plain pan;
two-finger pan). `on_gesture` (pinch). `PanGestureRecognizer` with
velocity for fling. Reduced-motion path. Per-axis epsilon.
`SceneView::pan_to/zoom_to/fit_to_content`.

**Example v2:** same 9 cards, now pannable with two-finger trackpad /
Ctrl-wheel zoom / pinch on macOS. Inertial fling on release. "Fit to
content" button.

**Docs:** `docs/fern-scene.md` extended with view-transform model,
gesture wiring, idle compliance.

**Tests:** signal-driven pan tween mid-flight; idle-drain test asserts
`tree.needs_redraw() == false` after view at rest; reduced-motion path
snaps without animation.

### Phase 3 — Spatial index + viewport culling

**What:** `SpatialIndex` trait + `GridHashIndex` impl. `Scene`
integrates index on add/move/remove. `SceneView` queries index in
`place_children` and paint walk; off-screen items skip layout entirely.

**Example v3:** scale corkboard to 5,000 cards spread over a wide
scene. Pan/zoom remains 60 FPS; only viewport-worth laid out per frame.

**Docs:** `docs/fern-scene.md` extended with `SpatialIndex` trait,
performance characteristics, viewport-cull semantics.

**Tests:** `index.query(rect)` matches brute-force; viewport-cull test
asserts off-screen `WidgetItem` has no laid-out bounds; insert/move/
remove perf microbench (1k ops < 5ms).

### Phase 4 — Lightweight `SceneItem` + builtins

**What:** `SceneItem` trait. `RectItem`, `PathItem`, `ImageItem`,
`TextItem`, `GroupItem`. Items stored in same `Scene`, share spatial
index, painted in same walk (after widgets). Hit-test goes broad-phase
via index then narrow-phase via `item.hit_test`.

**Example v4:** corkboard adds connector `PathItem`s between related
cards + tiled `RectItem` background grid. 4k cards + 8k connectors at
60 FPS.

**Docs:** `docs/fern-scene.md` extended with `SceneItem` trait + each
builtin documented + custom-item authoring guide.

**Tests:** `RectItem`/`PathItem` paint correct; mixed scene
(heavy + light) hit-test resolves correctly; light-only stress at 10k
items.

### Phase 5a — Visual-default a11y + keyboard navigation

**What:** extend `SyntheticKind::SceneItem`. SceneView a11y walker emits
synthetic children with `view_transform`-projected screen bounds.
`A11yOffScreenMode` honored (default `ViewportPlusN { n: 1 }`).
`SceneItem::accessibility` default sets `Role::GraphicsObject`. Per-item
`access_*` override chain mirroring `WidgetBuilder`. AT-action dispatch
via reverse-lookup map. Default `SceneFocusOrder` (reading order).
Tab/Shift+Tab cycle items. Arrow keys (Pan vs Select mode).
Home/End/+/-. `focus_item(id)` auto-pan animation. AT-commit debounce
(100ms after terminal tick).

**Example v5a:** corkboard fully keyboard-navigable with the default
policy. NVDA/VoiceOver announce each card and connector. Tabbing to
off-screen card auto-pans it into view.

**Docs:** `docs/fern-scene.md` finalized for the visual-default a11y
story. Stub `docs/fern-scene-a11y.md` — covers defaults, off-screen
modes, the per-item `access_*` chain, with a "if these defaults are
enough for your app, stop here" pointer.

**Tests:** N visible items → N synthetic AT nodes; off-screen mode
boundary correctness; reading-order Tab cycle; per-item `access_label`
overrides walker default; AT action dispatched on synthetic node fires
lightweight item callback; auto-pan animation finite-duration.

### Phase 5b — A11y-shaping tools (the parallel structural layer)

**What:** the API surface from §4 — `Scene::add_a11y_group`,
`set_a11y_parent` (accepting `A11yNode::{Item, Group, Widget}`),
`add_a11y_relation`, `set_a11y_live`, `set_a11y_landmark`,
`set_a11y_categories`. `SyntheticKind::SceneGroup`. SceneView walker's
two-pass logical/fallback emission. **Auto-graft of interactive widget
descendants** into their containing logical item, with
`a11y_descendants(AutoGraft | Suppress | Manual)` per-item override.
`FocusOrderContext::expand(item)` helper for custom focus orders.
`SceneView::focus_order(...)` and `directional_navigation(...)`
callbacks override the defaults from 5a. AT-only `access_action` /
`access_custom_action` on items and groups (the "minimal shadow" path
for actions with no visual counterpart). `default_item_role(role)` and
`a11y_bounds_space(...)` configuration. Heavyweight items participate
uniformly in groups/relations.

**Example v5b:** corkboard cards grouped under "Act 1", "Act 2", "Act
3" `A11yGroup`s — screen reader announces "Act 2, Scene cards, 5 of
12, Scene: The Confrontation". Connector `PathItem`s declared as
`flow_to` relations between source/target cards — VoiceOver rotor
offers "next connector" navigation. Each Scene card contains a real
`ComboBox` for character selection; AT user Tabs from "Scene B" →
"Scene B's ComboBox" → "Scene C", activates ComboBox normally with
Enter / arrow keys, no mode switch. One global character picker placed
visually inside Scene B but explicitly relocated to a top-level "Story
Tools" group via `set_a11y_parent(A11yNode::Widget(picker_id),
tools_group)`. Each Act group exposes a custom_action "Reorder scenes
alphabetically" with no visible button. `focus_order` callback
overridden to Tab through cards in story-order using `expand()` to
descend into each card's grafted ComboBox.

**Docs:** `docs/fern-scene-a11y.md` brought to full draft — covers
logical tree (groups, parents, relations), auto-graft + override knobs,
`A11yNode::Widget` relocation, custom focus / directional callbacks,
AT-only actions, with three worked examples: corkboard, graph editor,
CAD canvas. Cross-link from `docs/fern-scene.md`.

**Tests:** logical AT tree matches declared structure; item without a
logical parent falls back to SceneView root; relation arrays written
into AccessKit nodes; category tags survive walker; custom
`focus_order` callback drives Tab cycle; **auto-graft test**: ComboBox
inside a Scene card appears as logical child of that card AND retains
its real `WidgetId`; **relocation test**:
`set_a11y_parent(Widget, ...)` moves the widget to the declared
parent; **suppress test**: `a11y_descendants(Suppress)` drops widget
descendants from the logical tree; **AT-only action test**: invoking
`access_custom_action` on a group fires its callback with no visual
side-effect.

### Phase 6 — Selection, marquee, drag-to-move

**What:** `SceneSelection` (likely `fern-data::SelectionModel<ItemId>`).
`MarqueeRecognizer` with overlay paint. Drag-to-move on items:
pointer-down on item → drag updates `Scene::move_item` → re-index.
Optional snap-to-grid. Multi-select group-move.

**Example v6:** drag cards to reposition. Alt-drag marquee to
multi-select. Snap-to-grid toggle. Group-move all selected.

**Tests:** marquee → expected `items_in_rect` result; drag-to-move
updates spatial index; group-move applies same delta to all.

### Phase 7 — Polish + 1.0

**What:** R-tree alternative for `SpatialIndex`; benchmark vs
grid-hash. Optional mini-map widget. `GridBackgroundItem` builtin.
Inertial-fling tuning. `fit_to_selection`. Final API review. Both
reference docs finalized: full API coverage, all worked examples,
performance/idle notes, troubleshooting, cross-links to
`docs/idle-and-animation.md` and `docs/accessibility-overrides.md`.

**Example v7:** mini-map slot added (optional). 10k-item stress demo.

## 7. The example app — `examples/scene_corkboard/`

Skribisto-relevant: a story corkboard with cards (act/scene/beat),
connectors between related beats, pannable/zoomable, fully keyboard
navigable, fully a11y. Evolves per phase as listed in §6. Single example
throughout — each phase commits a diff to it, so the example is also a
regression artifact.

## 8. Testing strategy

All headless via `WidgetTree`, no GPU/display.

- `crates/fern-core/src/widget_tree/event_dispatch_impl.rs` tests
  for Phase 0.
- `crates/fern-scene/tests/integration.rs` — Scene + SceneView
  end-to-end (placement, pan, zoom).
- `crates/fern-scene/tests/gestures.rs` — gesture recognizer state
  machines.
- `crates/fern-scene/tests/a11y.rs` — synthetic-node emission,
  off-screen modes, reading order, AT-action dispatch.
- Performance micro-benchmarks under `crates/fern-scene/benches/` for
  spatial index query and viewport-cull throughput at 1k/5k/10k items.

Idle-drain test pattern (recurring across phases):

```rust
let (mut tree, view_id) = setup_scene_view_with_n_items(100);
tree.layout(SizeProposal::exact(800.0, 600.0));
tree.advance_time(Duration::from_millis(500));
assert!(!tree.needs_redraw(), "scene at rest must not request frames");
```

## 9. Critical files

- **Modified (Phase 0, landed):**
  [crates/fern-canvas/src/geometry.rs](../../crates/fern-canvas/src/geometry.rs),
  [crates/fern-canvas/src/render_frame.rs](../../crates/fern-canvas/src/render_frame.rs),
  [crates/fern-core/src/arena.rs](../../crates/fern-core/src/arena.rs),
  [crates/fern-core/src/widget_tree/event_dispatch_impl.rs](../../crates/fern-core/src/widget_tree/event_dispatch_impl.rs).
- **Modified (later phases):**
  [crates/fern-core/src/accessibility.rs](../../crates/fern-core/src/accessibility.rs)
  (Phase 5 — `SyntheticKind::SceneItem`, `SyntheticKind::SceneGroup`),
  root [Cargo.toml](../../Cargo.toml) (Phase 1 — workspace member +
  shared dep entry).
- **New:** `crates/fern-scene/` (Phases 1–7),
  `examples/scene_corkboard/` (Phases 1–7), `docs/fern-scene.md` (stub
  Phase 1, finalized Phase 7), `docs/fern-scene-a11y.md` (stub Phase
  5a, full draft Phase 5b, finalized Phase 7).

## 10. Verification per phase

Each phase ships when:

1. `cargo test -p fern-scene` (and `-p fern-core` for Phase 0) green.
2. `cargo run -p scene_corkboard` matches the per-phase example state.
3. Idle-drain test passes (Phase 2+).
4. Manual keyboard walk-through with NVDA/VoiceOver/Orca — all items
   announced, Tab cycles correctly (Phase 5+).
5. Stress demo at the phase's item count holds 60 FPS in `--release`
   (Phase 3+).
6. No new clippy warnings; `cargo doc --no-deps -p fern-scene` builds
   cleanly.

## 11. Out of scope (explicitly)

3D/perspective; physics; animation timeline (use existing
`Signal::animate_to` + wrapper widgets); multi-touch beyond pinch+pan;
explicit z-order signals (insertion order until users complain); WASM
target; collaborative editing; SVG/PDF export (caller's job via Canvas
backends).

## 12. Open / deferred

- **R-tree vs grid-hash crossover.** Grid-hash is cache-friendly up to
  ~10k items; R-tree wins for non-uniform density. Phase 7 benchmarks
  decide.
- **Rotation as a first-class view-transform axis.** API supports it,
  but real apps may want rotation snapped to 0/90/180/270°. Defer to
  user feedback during Phase 2.
- **Looping animations on lightweight items.** Documented limitation;
  revisit if a real app needs cheap animated decorations off-screen.
- **Mini-map widget** (Phase 7 stretch). Possibly its own crate.
- **Snap-to-other-items** beyond grid (alignment guides). Phase 7+.
- **GPU instancing for lightweight items.** A profiled-but-deferred
  optimization for rendering tens of thousands of homogeneous
  `RectItem`s / `PathItem`s. The framework's existing 3-pipeline
  renderer already issues per-shape-kind batches via
  `RenderFrame::shapes` and `RenderFrame::draw_order`, so a single
  visible viewport at 5k items renders in well under a frame budget.
  Real wins from instancing would only show on stress demos beyond
  the spatial-index cull (10k+ visible items), where the bottleneck
  is typically memory bandwidth at the upload step rather than draw
  count. This is a **renderer-level concern**, not a fern-scene
  concern: the right fix is `fern-render` adopting an
  `InstanceBuffer` for `RectItem`-like quads, after which
  fern-scene benefits transparently. Revisit when a real app stress-
  tests beyond the current cull-bounded budget.
