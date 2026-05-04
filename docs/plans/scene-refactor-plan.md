# `fern-scene` Qt-alignment refactor — plan

## 1. Context

The Phase 0–7 rollout shipped a working pannable/zoomable scene viewport: two-tier model (heavyweight widgets + lightweight items), one spatial index, one view transform, parallel a11y tree, idle-correct animations. The showcase exercises every shipped capability.

A side-by-side review against Qt's `QGraphicsScene` / `QGraphicsView` / `QGraphicsItem` (with the actual Qt 6 docs in hand) surfaced three classes of gaps that, taken together, make the crate feel "half-baked" for serious scene-graph use cases (node-graph editors, CAD canvases, mind maps with rotated subtrees):

- **C — semantic divergences from Qt.** Absolute scene coords instead of parent-relative; no per-item transformations; no per-item events. Apps writing a node editor today must re-implement hit-test dispatch by hand on `SceneView::on_pointer_event`, and cannot rotate/scale subtrees at all.
- **D — missing functionality.** No `ItemFlags`, no per-item visibility / opacity / enabled, no cache modes, no `itemChange`, no collision API, no `map_to_scene` helpers, no `ensure_visible`, no background/foreground hooks, no `ScrollHandDrag`, no `sceneRect` clamping.
- **E — concrete code-level issues.** `items_in_rect` narrow-phase is O(V·N); `Scene::remove` orphans children; `PathItem::set_bounds` is translate-only and silently mis-sizes on resize; `set_bounds` default is silent-desync; per-item `_literal` i18n twins missing; `view_transform_signal` fires four times per animation tick; `scene_mut` semantics under-documented; no middle-click pan / right-click context menu; per-item AT subtree-merge missing; no opt-in dynamic-bounds path.

The user's directive: fix every point in C, D, E. F is just a priority recommendation list — its priorities are folded into the phasing below, but no "phase F."

**Hard break, no backward compatibility.** No deprecation shims, no compatibility wrappers, no "old call sites still work." Every API that changes shape changes shape; downstream call sites are updated in lockstep. The only thing that survives is `ItemId` as an opaque newtype.

**Cleanup is in scope and explicit:**
- `view.rs` (5733 LoC) and `items.rs` (1047 LoC) both split into submodules (no `mod.rs`, per project convention).
- **Inline `///` doc-comments are cleaned up across the entire crate** — every phase that touches a file rewrites the doc-comments on the items it touches: tighten, remove obsolete cross-references, drop "Phase N" history language, surface the actual contract first. By the end of R8, no doc-comment in `crates/fern-scene/` mentions phase numbers, "Phase 6 territory," "TODO," or "we'll add later." Each public item has a one-paragraph contract; private helpers carry one line of why.

**Two new policy axes added at user request (folded into R2):**
- `Scene` declares whether it permits view-level pan and zoom. `Scene::pan_axes(PanAxes::{None|Horizontal|Vertical|Both})` and `Scene::zoomable(bool)`. SceneView reads these at build time and gates its gesture handlers accordingly.
- `SceneView` gains an "adopt scene size" mode: when set, the view's `layout_response` returns the scene's content extent as its own size — the scene is unmovable inside the view because the view *is* the scene. Useful for embedding bounded scenes inline (mini diagrams in docs, fixed corkboards) instead of full-viewport scenes.

Work happens on the existing `fern-scene2` branch in worktree `../fern-ui.worktrees/fern-scene`. Each phase is one focused PR with tests + showcase update + doc update.

## 2. Patterns to reuse (no new infrastructure)

The fern-core / fern-widgets toolkit already has every primitive this refactor needs. The plan rides on them rather than inventing parallels.

| Need | Existing pattern (file:line) | Reuse strategy |
|---|---|---|
| Per-node transform with chain composition | [`BuildContext::set_transform`](crates/fern-core/src/build_context.rs#L243), [`SetTransform` semantics](crates/fern-canvas/src/render_frame.rs#L386-L401) — composes with stack-top | Wrap each item's paint in `canvas.translate/scale/rotate` for lightweight tier; emit `set_transform` scope on heavyweight `WidgetItem` nodes. |
| Per-node opacity / clip / blur | [`set_opacity`](crates/fern-core/src/build_context.rs#L230), [`set_blur`](crates/fern-core/src/build_context.rs#L261), `clips_children` on HandlerSet | D.2 opacity rides `canvas` paint-scope wrap; clip uses canvas `push_clip`. |
| Builder + handler chain | [`HandlerSet` + `WidgetBuilder` blanket impl](crates/fern-core/src/widget_builder.rs#L1237-L1586), [`EventHandlers`](crates/fern-core/src/event_handlers.rs#L18-L88) | Build a thinner `SceneItemHandlerSet` (subset: tap/hover/drag/scroll/key/pointer/cursor/tooltip/accept_drops). Reuse `fern_core::gesture` recognizers verbatim. |
| Per-node a11y overrides + Merge subtree | [`AccessibilityOverrides`](crates/fern-core/src/widget_builder.rs#L59-L141), [`AccessSubtreeMode::{Inherit,Exclude,Merge}`](crates/fern-core/src/widget_builder.rs#L27-L40), [`merge_descendants_into`](crates/fern-core/src/widget_tree/accessibility_impl.rs#L228) | Extend `ItemA11yOverrides` with `subtree_mode`. Walker logic for items mirrors widget walker; merge concatenates first-non-empty label/value/actions across descendant items. |
| Reactive observer / itemChange | [`Signal<T>::observe`](crates/fern-core/src/signal.rs) returning `ObserverHandle` (RAII) | `Scene` exposes `Signal<ItemChangeEvent>` fired by every mutator; consumers `.observe(...)` for snap-to-grid / clamping / side-effects. |
| Per-widget scopes survive rebuild | Existing pattern in widget tree; `preserves_children_on_rebuild` already on SceneView | Per-item handler closures stored on `SceneEntry` survive Scene mutation. |

## 3. Target API shape (after all phases)

The end-state surface — for orientation; details in each phase below.

```rust
// --- core types ---------------------------------------------------------

pub struct ItemId(u64);

bitflags::bitflags! {
    pub struct ItemFlags: u32 {
        const IS_VISIBLE                 = 1 << 0;   // default on
        const IS_ENABLED                 = 1 << 1;   // default on
        const IS_DRAGGABLE               = 1 << 2;
        const IS_SELECTABLE              = 1 << 3;
        const IS_FOCUSABLE               = 1 << 4;
        const ACCEPTS_HOVER              = 1 << 5;
        const CLIPS_TO_SHAPE             = 1 << 6;
        const CLIPS_CHILDREN_TO_SHAPE    = 1 << 7;
        const IGNORES_TRANSFORMATIONS    = 1 << 8;   // pin at fixed pixel size
        const HAS_NO_CONTENTS            = 1 << 9;   // logical-only, skip paint
        const NEGATIVE_Z_BEHIND_PARENT   = 1 << 10;
    }
}

pub enum CacheMode { None, ItemCoordinate }

#[derive(Clone)]
pub enum ItemChange {
    GeometryChanged { id: ItemId, old: Rect, new: Rect },
    LocalPosChanged { id: ItemId, old: Point, new: Point },
    TransformChanged { id: ItemId },
    VisibilityChanged { id: ItemId, visible: bool },
    OpacityChanged { id: ItemId, opacity: f32 },
    ZChanged { id: ItemId, z: f32 },
    ParentChanged { id: ItemId, old: Option<ItemId>, new: Option<ItemId> },
    Removed { id: ItemId },
}

// --- SceneItem trait (post-refactor) ------------------------------------

pub trait SceneItem: std::fmt::Debug + 'static {
    /// Bounds in **local item coordinates** (origin at item's anchor).
    fn local_bounds(&self) -> Rect;

    /// Optional exact-shape hit-test in local coords. Default: AABB containment.
    fn shape_contains(&self, local_pt: Point) -> bool { self.local_bounds().contains(local_pt) }

    fn paint(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext);

    fn set_local_bounds(&mut self, bounds: Rect);   // required (was `set_bounds`, default no-op — fixed)

    fn cache_mode(&self) -> CacheMode { CacheMode::None }
    fn label(&self) -> Option<String> { None }
    fn register_bindings(&self, _ctx: &mut BuildContext, _view_id: WidgetId) {}
    fn accessibility(&self, b: &mut AccessNodeBuilder, ctx: &SceneItemA11yContext) { /* default role */ }
}

// --- Scene mutators (post-refactor) -------------------------------------

impl Scene {
    // Construction
    pub fn add_item<I: SceneItem>(&mut self, item: I, local_pos: Point) -> ItemId;
    pub fn add_widget<W: Widget>(&mut self, w: W, local_rect: Rect) -> ItemId;

    // Geometry — parent-relative
    pub fn local_pos(&self, id: ItemId) -> Option<Point>;
    pub fn set_local_pos(&mut self, id: ItemId, pos: Point);
    pub fn transform(&self, id: ItemId) -> Option<Transform2D>;          // local-to-parent
    pub fn set_transform(&mut self, id: ItemId, t: Transform2D);
    pub fn scene_pos(&self, id: ItemId) -> Option<Point>;                 // computed via parent chain
    pub fn scene_rect(&self, id: ItemId) -> Option<Rect>;                  // local_bounds × scene_transform
    pub fn scene_transform(&self, id: ItemId) -> Transform2D;              // composed up the chain
    pub fn map_to_scene(&self, id: ItemId, local_pt: Point) -> Point;
    pub fn map_from_scene(&self, id: ItemId, scene_pt: Point) -> Point;
    pub fn map_to_parent(&self, id: ItemId, local_pt: Point) -> Point;

    // Per-item state
    pub fn flags(&self, id: ItemId) -> ItemFlags;
    pub fn set_flags(&mut self, id: ItemId, f: ItemFlags);
    pub fn set_flag(&mut self, id: ItemId, f: ItemFlags, on: bool);
    pub fn set_visible(&mut self, id: ItemId, v: bool);
    pub fn set_opacity(&mut self, id: ItemId, o: f32);
    pub fn opacity(&self, id: ItemId) -> f32;
    pub fn effective_opacity(&self, id: ItemId) -> f32;                    // composed up chain
    pub fn set_z(&mut self, id: ItemId, z: f32);

    // Parenting (Qt-style — children deleted with parent by default)
    pub fn set_item_parent(&mut self, child: ItemId, parent: Option<ItemId>);
    pub fn parent_of(&self, id: ItemId) -> Option<ItemId>;
    pub fn child_items(&self, id: ItemId) -> Vec<ItemId>;
    pub fn ancestors(&self, id: ItemId) -> impl Iterator<Item = ItemId>;
    pub fn remove(&mut self, id: ItemId);                                  // recursively removes children
    pub fn detach(&mut self, id: ItemId) -> Option<DetachedSubtree>;       // remove without deleting

    // Queries
    pub fn item_at(&self, scene_pt: Point) -> Option<ItemId>;              // topmost
    pub fn items_at(&self, scene_pt: Point) -> Vec<ItemId>;
    pub fn items_in_rect(&self, scene_rect: Rect) -> Vec<ItemId>;
    pub fn colliding_items(&self, id: ItemId) -> Vec<ItemId>;
    pub fn items_along_path(&self, path: &Path) -> Vec<ItemId>;

    // Scene rect (Qt setSceneRect)
    pub fn set_scene_rect(&mut self, rect: Option<Rect>);                  // None = auto from items
    pub fn scene_rect_extent(&self) -> Rect;                                // user-set or computed

    // Interaction policy (user-added)
    pub fn pan_axes(&mut self, axes: PanAxes);                             // None|Horizontal|Vertical|Both
    pub fn zoomable(&mut self, on: bool);
    pub fn current_pan_axes(&self) -> PanAxes;
    pub fn is_zoomable(&self) -> bool;

    // itemChange notifications
    pub fn item_change_signal(&self) -> Signal<ItemChange>;
}

pub enum PanAxes { None, Horizontal, Vertical, Both }

// --- SceneView additions ------------------------------------------------

impl SceneView {
    pub fn drag_mode(self, mode: DragMode) -> Self;                        // NoDrag|ScrollHandDrag|RubberBand
    pub fn adopt_scene_size(self, on: bool) -> Self;                        // view sizes to scene extent (user-added)
    pub fn background<F: Fn(&mut Canvas, &PaintContext, Rect) + 'static>(self, f: F) -> Self;
    pub fn foreground<F: Fn(&mut Canvas, &PaintContext, Rect) + 'static>(self, f: F) -> Self;

    pub fn map_to_scene(&self, view_pt: Point) -> Point;
    pub fn map_from_scene(&self, scene_pt: Point) -> Point;
    pub fn map_rect_to_scene(&self, view_rect: Rect) -> Rect;
    pub fn map_rect_from_scene(&self, scene_rect: Rect) -> Rect;

    pub fn ensure_visible(&self, scene_rect: Rect, margin: f32);           // pan only, no zoom change
}

// --- per-item handler chain (mirrors WidgetBuilder) ---------------------

impl<I: SceneItem> ItemBuilder<I> {
    pub fn flags(self, f: ItemFlags) -> Self;
    pub fn flag(self, f: ItemFlags, on: bool) -> Self;
    pub fn local_pos(self, p: Point) -> Self;
    pub fn z(self, z: f32) -> Self;
    pub fn opacity(self, o: f32) -> Self;
    pub fn transform(self, t: Transform2D) -> Self;

    pub fn on_tap(self, f: impl Fn(Point, &mut EventContext) + 'static) -> Self;
    pub fn on_double_tap(self, f: impl Fn(Point, &mut EventContext) + 'static) -> Self;
    pub fn on_hover(self, f: impl Fn(bool, &mut EventContext) + 'static) -> Self;
    pub fn on_drag(self, f: impl Fn(DragPhase, &mut EventContext) + 'static) -> Self;
    pub fn on_pointer_event(self, f: impl Fn(&PointerEvent, &mut EventContext) + 'static) -> Self;
    pub fn on_scroll(self, f: impl Fn(ScrollDelta, &mut EventContext) + 'static) -> Self;
    pub fn on_key(self, f: impl Fn(&KeyEvent, &mut EventContext) -> EventResponse + 'static) -> Self;
    pub fn on_context_menu(self, f: impl Fn(Point, &mut EventContext) + 'static) -> Self;

    pub fn cursor(self, c: CursorIcon) -> Self;
    pub fn tooltip(self, t: impl Into<String>) -> Self;
    pub fn tooltip_literal(self, t: impl Into<String>) -> Self;             // E.5 — i18n twin
    pub fn accept_drops(self, accept: bool) -> Self;

    // a11y — extends existing ItemA11yOverrides
    pub fn access_label(self, l: impl Into<String>) -> Self;
    pub fn access_label_literal(self, l: impl Into<String>) -> Self;        // E.5
    pub fn access_subtree(self, mode: AccessSubtreeMode) -> Self;           // E.9 — Merge support
    pub fn access_merge_subtree(self) -> Self;
    pub fn access_exclude_subtree(self) -> Self;
}
```

## 4. Phasing — eight focused PRs

### Phase R1 — Coordinate-space refactor (C.1, C.2, D.6, E.7) — **the big one**

The foundation. Every other phase rides on this.

**Storage change.** `SceneEntry`'s `scene_rect: Rect` becomes:
- `local_pos: Point` — origin in **parent** coordinates (or scene if `parent == None`)
- `local_bounds: Rect` — rect at origin in **item-local** coords (read from `item.local_bounds()`)
- `transform: Transform2D` — local→parent (default identity)

**Computed accessors.** `Scene::scene_pos(id)` walks the parent chain composing transforms; `Scene::scene_rect(id)` returns `scene_transform × local_bounds`. Spatial index buckets the **scene-space AABB** (computed from local_bounds × accumulated chain transform — recomputed when any ancestor's transform/local_pos changes).

**Mutators.** `Scene::set_local_pos`, `Scene::set_transform`, `Scene::set_local_bounds`. The old `move_item(id, scene_rect)` is **deleted outright** — no shim, no compat wrapper. Every existing call site is rewritten to the explicit local-pos / transform setters. Drag-end commit (in `view.rs`'s `pending_item_move` drain) calls `set_local_pos` on the dragged parent only — descendants follow automatically (their local_pos is unchanged but their scene_pos derives from the parent's chain).

**Per-item paint (lightweight tier).** `paint(canvas, ctx)` is called with the item's **scene transform pushed onto the canvas**: SceneView's paint walk emits `canvas.push_transform(scene_transform)` around each `item.paint(...)` call. Items paint in **local coords** — `RectItem::paint` becomes `canvas.fill_rect(self.local_bounds, ...)`. The `bounds` field on every built-in is renamed `local_bounds` and its origin is normalized to (0, 0). `move_item` and the compatibility shim are gone.

**Per-item paint (heavyweight tier).** `WidgetItem` materialization in SceneView's `build()` emits `ctx.set_transform(child_id, scene_transform_signal)` per heavyweight, layered with the existing view-transform scope on SceneView's root. Composition is automatic via render_frame's stack-top semantics.

**Hit-test.** `Scene::item_at(scene_pt)` and `items_at` first do broad-phase via spatial index, then per-candidate inverse-transform `scene_pt` to local coords and call `shape_contains(local_pt)` (renamed from `hit_test(scene_point)`).

**`ItemFlag::IGNORES_TRANSFORMATIONS`** — when set, the item's paint and hit-test skip the parent-chain composition and apply only translation, freezing the item at fixed pixel size regardless of view zoom. Implementation: in the paint walk, special-case items with this flag — push a *cancellation* transform that undoes the view's scale + rotation around the item's anchor.

**Coord helpers (D.6).** Add `SceneView::map_to_scene` / `map_from_scene` / `map_rect_to_scene` / `map_rect_from_scene`. Trivial wrappers around `view_transform().inverse().apply_*`.

**Drag-cascade rewrite.** Today drag end applies one delta to parent + every descendant's `scene_rect`. After this phase, drag end updates **only the parent's `local_pos`**; descendants automatically follow because their local_pos is unchanged but their scene_pos derives from the parent's. Cleaner, removes the descendant walk in the move drain.

**Files modified.**
- [`crates/fern-scene/src/scene.rs`](crates/fern-scene/src/scene.rs) — `SceneEntry` field changes; new accessors; rewrite spatial-index bucketing to use computed scene AABB; rewrite `move_item` as compatibility shim.
- [`crates/fern-scene/src/item.rs`](crates/fern-scene/src/item.rs) — rename `bounds_in_scene` → `local_bounds`; rename `set_bounds` → `set_local_bounds` (still default-no-op for now; E.4 fixes that); rename `hit_test` → `shape_contains` taking local coords.
- [`crates/fern-scene/src/items.rs`](crates/fern-scene/src/items.rs) — every built-in's `bounds: Rect` becomes `local_bounds: Rect` rooted at origin; paint methods use local coords; rename one method per built-in. `PathItem::set_local_bounds` no longer translates path geometry — the path itself is in local coords once and for all (E.3 absorbed: resize is impossible because there's no semantic for "resize a path").
- [`crates/fern-scene/src/transform.rs`](crates/fern-scene/src/transform.rs) — add `compose_chain(items: &[Transform2D]) -> Transform2D` helper.
- [`crates/fern-scene/src/view.rs`](crates/fern-scene/src/view.rs) — paint walk wraps each lightweight `item.paint()` in `canvas.push_transform/pop_transform`; heavyweight materialization adds per-widget `set_transform` scope; hit-test pipelines pivot to local-coord `shape_contains`; new `map_to_scene` / `map_from_scene` / `map_rect_*` methods; document `scene_mut` semantics on the method itself (E.7).
- [`examples/scene_showcase/src/main.rs`](examples/scene_showcase/src/main.rs) — add Section 9 demonstrating per-item rotation/scale (a card that rotates 15° while remaining clickable on its visual area) and `IGNORES_TRANSFORMATIONS` (a fixed-size badge that stays 12px regardless of zoom).
- [`docs/fern-scene.md`](docs/fern-scene.md) — coordinate-system section rewrite.

**Tests.**
- `parent_relative_position_composes_through_chain` — three-deep parent chain, set_local_pos on root, all descendants' scene_pos reflects update.
- `per_item_transform_paints_rotated_clickable_area` — a rotated rect, click on the rotated visual area hits, click on the un-rotated AABB area outside the rotated shape misses.
- `ignores_transformations_pins_at_fixed_pixel_size` — zoom 4×, item with `IGNORES_TRANSFORMATIONS` paints at the same pixel size.
- `map_to_scene_round_trips` — `map_from_scene(map_to_scene(p)) == p` under arbitrary pan/zoom/rotation.
- `set_local_pos_propagates_to_descendants_scene_pos` — drag-end's single call to `set_local_pos` on the dragged parent moves all descendants' computed `scene_pos`.
- Existing snap-back, parent-cascade, freeze-after-drag tests pass after the rewrite.

**Verification.** `cargo test --workspace` green. `cargo run -p scene-showcase` shows new Section 9 (rotation + ignore-transformations).

---

### Phase R2 — Item flags + visibility + opacity + sceneRect + Scene/View interaction policy (D.1, D.2, D.10 + user-added requirements)

**`ItemFlags` bitflags type.** New module [`crates/fern-scene/src/flags.rs`](crates/fern-scene/src/flags.rs). Stored as a `u32` field on `SceneEntry`. Default: `IS_VISIBLE | IS_ENABLED | IS_SELECTABLE`. `is_draggable: bool` field on built-in items disappears — replaced by `flag(ItemFlags::IS_DRAGGABLE, true)` on the builder.

**Scene-level pan/zoom policy (user-added).** Two new fields on `Scene`:

```rust
pub enum PanAxes { None, Horizontal, Vertical, Both }   // default: Both

impl Scene {
    pub fn pan_axes(&mut self, axes: PanAxes);                  // policy: which axes the view may pan along
    pub fn current_pan_axes(&self) -> PanAxes;
    pub fn zoomable(&mut self, on: bool);                        // default: true
    pub fn is_zoomable(&self) -> bool;
}
```

`SceneView` reads these at build time and gates its gesture handlers:
- `PanAxes::None` — `on_scroll` and pinch-pan ignore the `pan_*` deltas; `pan_to` / `set_pan` become no-ops; ScrollHandDrag (R3) is disabled.
- `PanAxes::Horizontal` / `Vertical` — pan signal updates only on the permitted axis; the orthogonal scroll-wheel direction passes through to ancestor scrollables (proper event propagation).
- `zoomable(false)` — Ctrl+wheel and pinch are inert; `zoom_to` / `set_zoom` no-op; `+`/`-` keyboard shortcuts unbound.

These are policy on the *Scene* (not the *View*) because a given scene model often makes sense at one zoom level and orientation only — a fixed-extent diagram, a horizontally-flowing timeline. Different views of the same scene inherit the same constraints.

**View "adopt scene size" mode (user-added).** New `SceneView` builder method:

```rust
impl SceneView {
    pub fn adopt_scene_size(self, on: bool) -> Self;            // default: false
}
```

When set, `SceneView::layout_response` returns the scene's `scene_rect_extent()` as its **own** wanted size (instead of `default_size`). The view becomes "as big as the scene"; user pan does nothing because the entire scene is already on-screen. Useful for embedding bounded scenes inline (a fixed corkboard widget inside a settings panel, mini diagrams in docs). Implies `pan_axes(None)` semantically — explicit pan calls still no-op even if `pan_axes` is `Both`.

Interaction with viewport-cull: when `adopt_scene_size` is on, the visible region is the entire scene, so spatial-index culling becomes a no-op — every item paints. Documented; not a perf concern for the bounded-scene use case.

**Visibility.** `IS_VISIBLE` short-circuits paint walk (skip the item) and hit-test (skip narrow phase). Toggling fires `ItemChange::VisibilityChanged` (R4). When parent is invisible, all descendants are effectively invisible — composed via `Scene::is_effectively_visible(id)`.

**Opacity.** `Scene::set_opacity(id, f32)` stored on `SceneEntry`. Per-item paint applies `canvas.push_opacity(opacity)` before `item.paint(...)`. Composes through chain — `effective_opacity` walks parents multiplicatively. Default 1.0; clamped to `[0.0, 1.0]`.

**Enabled.** `IS_ENABLED` flag. Disabled items paint with reduced contrast (use `canvas.push_opacity(0.5)` is too coarse; instead expose `SceneItemPaintContext::is_enabled` and let items honor it). Disabled items don't dispatch pointer events, don't take focus.

**Scene rect (D.10).** `Scene::set_scene_rect(Option<Rect>)` — None = auto-compute from item bounds (current behavior); Some = user-declared extent. `SceneView` clamps pan in `set_pan` / pan animations to `scene_rect ± viewport_size` when set. `scene_rect_extent()` is the resolved value.

**`HAS_NO_CONTENTS` flag.** Pure-logical groups (used as parent containers only) skip paint entirely. The current `GroupItem` with `is_visual() == false` becomes any item with this flag set.

**Files modified.**
- new [`crates/fern-scene/src/flags.rs`](crates/fern-scene/src/flags.rs) — `ItemFlags` bitflags.
- [`crates/fern-scene/src/scene.rs`](crates/fern-scene/src/scene.rs) — `SceneEntry::flags`, `opacity`; `PanAxes` enum; `pan_axes` / `zoomable` policy fields + accessors; chain accessors for visibility/opacity.
- [`crates/fern-scene/src/items.rs`](crates/fern-scene/src/items.rs) — drop `draggable: bool` and `show_label: bool` (latter migrates to `IS_VISIBLE` on a child label item). Builder methods `.draggable(true)` become `.flag(ItemFlags::IS_DRAGGABLE, true)`; provide convenience `.draggable()` that delegates.
- [`crates/fern-scene/src/view.rs`](crates/fern-scene/src/view.rs) — paint walk consults flags; pan clamping when scene_rect set; gesture gating reads scene's `pan_axes`/`zoomable`; new `adopt_scene_size` builder; `layout_response` uses scene extent when adopt mode set; bounds_snapshot filter pivots from `is_draggable()` to `flags.contains(IS_DRAGGABLE)`.
- [`Cargo.toml`](crates/fern-scene/Cargo.toml) — add `bitflags` workspace dep.

**Tests.**
- `invisible_item_does_not_paint_or_hit_test` — visibility flag short-circuits both passes.
- `effective_opacity_composes_through_chain` — parent at 0.5, child at 0.5, effective is 0.25.
- `disabled_item_ignores_pointer_events` — clicks pass through to next layer.
- `scene_rect_clamps_pan` — set_pan past extent clamps to bounds; animated pan to past-extent target stops at boundary.
- `has_no_contents_skips_paint_walk` — paint count drops when flag set.
- `pan_axes_horizontal_blocks_vertical_scroll` — vertical scroll deltas pass through to ancestor scrollable.
- `pan_axes_none_makes_pan_to_a_noop`.
- `zoomable_false_blocks_ctrl_wheel_and_pinch`.
- `adopt_scene_size_returns_scene_extent_from_layout_response` — `tree.bounds(view_id).size == scene.scene_rect_extent().size`.
- `adopt_scene_size_implies_no_user_pan` — drag attempts inside the view don't change `pan` signal.

---

### Phase R3 — Per-item events (C.3, D.9, E.8)

The biggest functional gap. Today every event is on `SceneView`. After this phase, items get a parallel event chain mirroring `WidgetBuilder`.

**`SceneItemHandlerSet`.** New module [`crates/fern-scene/src/item_handlers.rs`](crates/fern-scene/src/item_handlers.rs). Mirrors a subset of [`HandlerSet`](crates/fern-core/src/widget_builder.rs#L253) — gesture handlers + cursor + tooltip + accept_drops, but NOT the framework-coupled fields (focusable wires through `IS_FOCUSABLE` flag instead, focus_within / hover_within not applicable). Stored on `SceneEntry::handlers: Option<Box<SceneItemHandlerSet>>`.

**`ItemBuilder<I>` wrapper.** Mirrors `WidgetWithHandlers<W>` — wraps the inner item type and accumulates a handler set + flag overrides + initial local_pos / z / opacity / transform. Trait `ItemBuilder` blanket impl exposes the chain; `Scene::add_item` accepts `impl Into<BuiltItem>` so authors can pass either a raw `RectItem` or an `ItemBuilder<RectItem>`.

**Pointer dispatch.** `SceneView::on_pointer_event` (currently just cursor-tracking) becomes the **scene-graph router**:
1. Project screen point to scene coords.
2. Spatial-index broad-phase + narrow-phase via `Scene::items_at(scene_pt)`.
3. For the topmost hit item with `IS_ENABLED`, build a preview chain (root → ancestors → item) and dispatch through each item's handler set, respecting consumed/ignored.
4. Reuse `fern_core::gesture` recognizers verbatim — instantiate per-item arenas keyed by item id.

**Drag mode (D.9).** New enum `DragMode { NoDrag, ScrollHandDrag, RubberBand }`. Current behavior (item drag → move; empty area → marquee) stays as `RubberBand`. `ScrollHandDrag` ignores item handlers and pans the view; left-click anywhere drags the canvas. `NoDrag` disables both.

**Middle-click pan + right-click context menu (E.8).** Middle button drag → pan unconditionally (independent of `DragMode`). Right-click on item with `on_context_menu` handler fires the handler.

**Cursor / tooltip per-item.** Item handlers' `cursor` overrides SceneView's default cursor when hovering over the item. `tooltip` shows after the framework's hover-delay (reuse the widget-tier tooltip mechanism — overlay manager).

**Files modified.**
- new [`crates/fern-scene/src/item_handlers.rs`](crates/fern-scene/src/item_handlers.rs).
- new [`crates/fern-scene/src/item_builder.rs`](crates/fern-scene/src/item_builder.rs) — `ItemBuilder<I>` wrapper trait + blanket impl.
- [`crates/fern-scene/src/scene.rs`](crates/fern-scene/src/scene.rs) — `SceneEntry::handlers`; `add_item` accepts builders.
- [`crates/fern-scene/src/view.rs`](crates/fern-scene/src/view.rs) — pointer dispatch rewrite; new `drag_mode` builder method; middle-click pan; right-click routing.
- [`crates/fern-scene/src/items.rs`](crates/fern-scene/src/items.rs) — every built-in's `.draggable(true)` etc. now goes through `ItemBuilder` chain.
- [`examples/scene_showcase/src/main.rs`](examples/scene_showcase/src/main.rs) — Section 5 cards get `.on_tap(println!)` to demonstrate per-item events; Section 4 cards get `.on_context_menu(...)` for a card-level menu; new minimap-style section showing `.cursor(CursorIcon::Pointer)` on hover.

**Tests.**
- `on_tap_fires_when_item_clicked` — hit a RectItem with `.on_tap(...)`, callback fires once.
- `on_hover_enter_leave_fires` — hover over item, then off — callback fires twice with correct `entered: bool`.
- `on_context_menu_fires_on_right_click`.
- `on_drag_per_item_overrides_marquee` — RubberBand mode + item with `.on_drag(...)`: dragging the item dispatches to its handler, not marquee.
- `scroll_hand_drag_pans_view_ignoring_item_handlers`.
- `disabled_item_does_not_dispatch` — `IS_ENABLED` cleared blocks all event dispatch to the item.
- `cursor_over_item_uses_item_cursor`.

---

### Phase R4 — itemChange notifications + collision API + AccessSubtreeMode::Merge (D.4, D.5, E.9)

**`Scene::item_change_signal()`.** New `Signal<ItemChange>` field on `Scene`; every mutator (set_local_pos, set_transform, set_visible, set_opacity, set_z, set_item_parent, remove) fires `Signal::set(ItemChange::Variant { ... })`. Apps observe via the standard signal mechanism.

**Snap-to-grid pattern.** Documented in `docs/fern-scene.md`: `scene.item_change_signal().observe(|change| if let GeometryChanged { id, new, .. } = change { snap(id, new) })`.

**Collision API (D.5).**
- `Scene::colliding_items(id)` — items whose scene_rect intersects this item's scene_rect (using spatial index).
- `Scene::items_along_path(&Path)` — broad-phase via path AABB, narrow-phase via per-segment-vs-item-AABB distance. Useful for "items under this connector."
- `Scene::item_at(scene_pt) -> Option<ItemId>` — topmost; `Scene::items_at(scene_pt) -> Vec<ItemId>` — all under point.

**AccessSubtreeMode::Merge for items (E.9).** Extend `ItemA11yOverrides` with `subtree_mode: Option<AccessSubtreeMode>`. The a11y walker honors `Merge`: descendants' label / value / actions concatenate into the parent (mirroring [`merge_descendants_into`](crates/fern-core/src/widget_tree/accessibility_impl.rs#L228) for the widget tier). The "card containing rect + label + indicator dot reads as one AT element" pattern.

**Files modified.**
- [`crates/fern-scene/src/scene.rs`](crates/fern-scene/src/scene.rs) — `item_change_signal`, fire on every mutator; `item_at` / `items_at` / `colliding_items` / `items_along_path`.
- [`crates/fern-scene/src/items.rs`](crates/fern-scene/src/items.rs) — `ItemA11yOverrides::subtree_mode`; new builder methods `.access_subtree(mode)`, `.access_merge_subtree()`, `.access_exclude_subtree()`.
- [`crates/fern-scene/src/view.rs`](crates/fern-scene/src/view.rs) — a11y walker honors Merge for items.

**Tests.**
- `set_local_pos_fires_geometry_changed_signal`.
- `colliding_items_returns_correct_set`.
- `items_along_path_finds_items_near_polyline`.
- `merged_subtree_reads_as_one_at_element` — parent rect with two child labels + Merge produces one AT node whose label concatenates the descendants.

---

### Phase R5 — Background / foreground + ensure_visible + cache modes (D.3, D.7, D.8)

**Background / foreground (D.8).** `SceneView::background(impl Fn(&mut Canvas, &PaintContext, Rect))` and `.foreground(...)`. The closure runs with the **scene-coord visible region** as the rect argument and the canvas at the view-transform scope, so a "grid every 50 units" closure is one line. Paint order: background closure → items → foreground closure.

**ensure_visible (D.7).** `SceneView::ensure_visible(scene_rect, margin)` — pans (no zoom change) so `scene_rect.expand(margin)` fits in the viewport. If already inside, no-op. Pairs with `focus_item(id)` when an off-viewport item gains focus.

**Cache modes (D.3).** Add `SceneItem::cache_mode() -> CacheMode { None | ItemCoordinate }` (defer `DeviceCoordinate` to a later phase). `ItemCoordinate` keeps a per-item bitmap-or-display-list cache keyed by `local_bounds + last_paint_epoch`. SceneView's paint walk consults the cache; items return `Cached(handle)` from a new `paint_or_cached(canvas, ctx) -> CachePaint` method (default delegates to `paint`).

Cache invalidation: `item_change_signal` consumers in SceneView dirty the relevant cache entry on geometry / opacity changes. Items that mutate their internal state without going through Scene mutators must call `ctx.invalidate_cache(self_id)` from `register_bindings` (signal observer).

**Files modified.**
- [`crates/fern-scene/src/view.rs`](crates/fern-scene/src/view.rs) — `background` / `foreground` builders; paint walk integrates them; `ensure_visible`.
- [`crates/fern-scene/src/item.rs`](crates/fern-scene/src/item.rs) — `cache_mode` trait method.
- new [`crates/fern-scene/src/cache.rs`](crates/fern-scene/src/cache.rs) — per-SceneView item-coordinate cache (HashMap<ItemId, CachedFrame>).
- [`examples/scene_showcase/src/main.rs`](examples/scene_showcase/src/main.rs) — Section 1 background closure paints a zoom-aware grid.

**Tests.**
- `background_runs_before_items` — z-order proof via paint counter.
- `foreground_runs_after_items`.
- `ensure_visible_pans_only` — zoom unchanged; pan moves item into viewport.
- `cache_mode_item_coordinate_avoids_repeat_paints` — paint-call counter on a cached item drops to 1 across N idle frames.

---

### Phase R6 — Code-level fixes (E.1, E.2, E.4, E.6, E.10)

The cleanup pass. Each fix is small and mechanical; bundling them avoids cross-cutting churn during the larger phases above.

**E.1 — `items_in_rect` narrow-phase O(1).** Replace `self.entries.iter().find(|e| e.id == *id)` with `self.entry_index.get(id).map(|&pos| &self.entries[pos])`. Total query becomes O(visible) instead of O(visible × N).

**E.2 — `Scene::remove` recursive.** Today removes only the named entry; child entries keep `parent: Some(removed_id)` (orphaned). Qt's default is recursive. Add `Scene::remove(id)` walks `collect_descendants(id)` first and removes them all in dependency order. Also add `Scene::detach(id) -> DetachedSubtree` for the rare case the app wants to remove without deleting children.

**E.4 — `set_local_bounds` no-default.** Make required (no default). Catches custom items at compile time. Every built-in already overrides; existing users updating to the new API get a clear error.

**E.6 — `view_transform_signal` coalescing.** Today it's a derived `zip4` that fires on every upstream tick; during a 4-axis animation that's 4 fires per frame. Add a `Signal::map_coalesced` (or `zip_coalesced`) variant that only fires once per frame (caches and emits at end of layout pass). Place this in `fern-core`; consume from `view.rs`.

**E.10 — `add_item_dynamic`.** Add `Scene::add_item_dynamic<I: SceneItem>(item: I, local_pos: Point) -> ItemId` for items whose `local_bounds` is signal-driven. Re-bucketing happens each layout pass via `register_bindings`-installed observer on the bounds source. The static `add_item` keeps its snapshot semantics for the common case.

**Files modified.**
- [`crates/fern-scene/src/scene.rs`](crates/fern-scene/src/scene.rs) — E.1, E.2, E.10.
- [`crates/fern-scene/src/item.rs`](crates/fern-scene/src/item.rs) — E.4 (remove default).
- [`crates/fern-core/src/signal.rs`](crates/fern-core/src/signal.rs) — `Signal::map_coalesced`.
- [`crates/fern-scene/src/view.rs`](crates/fern-scene/src/view.rs) — adopt `map_coalesced` for `view_transform_signal`.

**Tests.**
- `items_in_rect_narrow_phase_is_o1_per_candidate` — micro-bench: 10k-item scene, query 10 visible, compare to 1k-item baseline.
- `scene_remove_recursively_removes_descendants` — remove parent → all children gone.
- `view_transform_signal_fires_once_per_frame_under_4_axis_animation` — install observer, drive pan_x + pan_y + zoom + rotation simultaneously, observer count is one-per-tick not four.
- `add_item_dynamic_rebuckets_on_signal_change`.

---

### Phase R7 — `_literal` i18n twins for items (E.5)

Mechanical addition: every item-level user-visible-string method gets a `_literal` twin per the CLAUDE.md convention. `RectItem::label` → also `RectItem::label_literal`; `access_label` → `access_label_literal`; `tooltip` → `tooltip_literal`; etc. The `_literal` variants take `impl Into<String>` directly (no `LocalizedString` resolution); the regular variants take `impl Into<LocalizedString>` via the `i18n` feature.

**Files modified.**
- [`crates/fern-scene/src/items.rs`](crates/fern-scene/src/items.rs) — `_literal` twins on every text-bearing builder method, applied via the `item_a11y_builders!` macro.
- [`crates/fern-scene/src/item_handlers.rs`](crates/fern-scene/src/item_handlers.rs) — `tooltip_literal`.

**Tests.**
- `access_label_literal_skips_i18n_resolution` — sanity-check the same pattern applied at widget tier.

---

### Phase R8 — Module split + docs (cleanup)

**Split `view.rs` (5733 LoC).** Per project convention (no `mod.rs`):
```
crates/fern-scene/src/
  view.rs                  # SceneView struct + Widget trait impl + top-level builders (~1000 LoC)
  view_paint.rs            # paint walk + debug overlays
  view_a11y.rs             # accessibility walker (synthetic node emission, logical-tree DFS)
  view_input.rs            # on_drag / on_pointer_event / on_scroll / on_key handlers
  view_focus.rs            # focus_order callback dispatch + focus traversal
```

**Split `items.rs` (1047 LoC).**
```
crates/fern-scene/src/
  items.rs                 # ItemA11yOverrides + macro + re-exports (~150 LoC)
  items/rect.rs            # RectItem
  items/path.rs            # PathItem (incl. per-segment hit-test helper)
  items/image.rs           # ImageItem
  items/text.rs            # TextItem (+ TextSource enum)
  items/group.rs           # GroupItem
```

Note: per CLAUDE.md, no `mod.rs`. The `items/` directory contains the per-type modules; `items.rs` declares them via `pub mod items::rect;` (2018-style — `items/rect.rs` works).

**Documentation — full rewrite, no traces of the previous version.** Per user directive: `docs/fern-scene.md` and `docs/fern-scene-a11y.md` are **deleted and rewritten from scratch**. No "previously…", no "this used to be…", no migration table referencing old API names. Each doc reads as if the post-refactor surface is the only one that ever existed.

- New `docs/fern-scene.md` covers, in order: the two-tier model, parent-relative coordinate system + per-item transforms, item flags, per-item events / cursor / tooltip, signals + reactivity (pan/zoom/rotation, item_change_signal), cache modes, Scene's interaction policy (`pan_axes` / `zoomable`) and SceneView's `adopt_scene_size`, drag modes, background/foreground hooks, scene_rect + clamping, worked examples (corkboard, simple node-graph, embedded fixed diagram).
- New `docs/fern-scene-a11y.md` covers logical AT tree, groups, parents, relations, auto-graft of widget descendants, custom focus / directional callbacks, AT-only actions, `AccessSubtreeMode::{Inherit,Exclude,Merge}` at item level, with three worked examples (corkboard, graph editor, CAD canvas).

**Plan-doc parity.** This plan's content moves into `docs/plans/scene-refactor-plan.md` as the in-tree mirror (like `docs/plans/charts-plan.md`).

**Files modified.**
- All view.rs / items.rs splits.
- [`docs/fern-scene.md`](docs/fern-scene.md) — **delete and rewrite from scratch**, no traces of the previous version.
- [`docs/fern-scene-a11y.md`](docs/fern-scene-a11y.md) — **delete and rewrite from scratch**, no traces of the previous version. Includes item-level `AccessSubtreeMode::Merge` from R4.
- new [`docs/plans/scene-refactor-plan.md`](docs/plans/scene-refactor-plan.md) — in-tree mirror of this plan.
- [`crates/fern-scene/src/lib.rs`](crates/fern-scene/src/lib.rs) — crate-level `//!` doc rewritten too; re-exports updated for the new module layout.
- **Inline `///` doc-comment audit:** every `pub` item across the crate gets its doc comment rewritten or pruned. Final state: no mention of "Phase N", no "Phase 6 territory", no "we'll add later", no historical asides. Each public item's doc comment leads with the contract (what it does + invariants) and adds context only when the contract isn't obvious from the signature.

**Verification.** `cargo doc --no-deps -p fern-scene` builds clean. `cargo test --workspace` green. `cargo run -p scene-showcase` covers every new feature.

## 5. Cross-cutting concerns

**No backwards compatibility.** Per user directive: hard break. No shims, no compat wrappers, no migration table. Every old call site (showcase, tests, internal helpers) is updated in lockstep within each phase's PR. The post-refactor docs read as if the previous API never existed.

**Inline-doc cleanup discipline.** Every phase that touches a file also rewrites the `///` comments on items it touches. By R8 the final crate has zero "Phase N" references, zero "TODO: …" comments, zero "previously this used to…" asides. Each public item's doc leads with its contract; private helpers carry one line of why-not-what.

**Test discipline.** Every phase ends with `cargo test --workspace` green. Each phase adds at least one regression test that would have caught the gap it closes. The Qt-comparison points in C/D/E become test names where reasonable (e.g. `parent_relative_position_composes_through_chain`, `colliding_items_returns_correct_set`).

**No idle-drain regression.** R3 (per-item events) installs per-item gesture arenas. Verify the existing `idle_drain_zero_frames_at_rest` test still passes — gesture arenas are state-only, no ticking.

**Showcase as regression artifact.** Each phase commits a diff to `examples/scene_showcase`. Final showcase exercises every new capability: rotation, ignore-transformations, per-item events, context menu, opacity, visibility toggle, scene_rect clamping, ensure_visible, cache mode, background grid, scroll-hand-drag.

## 6. Critical files (modified across the refactor)

- [`crates/fern-scene/src/scene.rs`](crates/fern-scene/src/scene.rs) — coord-space refactor (R1), flags + scene_rect (R2), itemChange (R4), code fixes (R6).
- [`crates/fern-scene/src/item.rs`](crates/fern-scene/src/item.rs) — trait surface rewrite (R1), cache_mode (R5), no-default `set_local_bounds` (R6).
- [`crates/fern-scene/src/items.rs`](crates/fern-scene/src/items.rs) — built-ins rewrite (R1), flag migration (R2), AccessSubtreeMode (R4), `_literal` twins (R7), split (R8).
- [`crates/fern-scene/src/view.rs`](crates/fern-scene/src/view.rs) — paint walk + transforms (R1), pan clamping (R2), pointer dispatch + drag mode (R3), background/foreground (R5), split (R8).
- new [`crates/fern-scene/src/flags.rs`](crates/fern-scene/src/flags.rs) — `ItemFlags` (R2).
- new [`crates/fern-scene/src/item_handlers.rs`](crates/fern-scene/src/item_handlers.rs) — `SceneItemHandlerSet` (R3).
- new [`crates/fern-scene/src/item_builder.rs`](crates/fern-scene/src/item_builder.rs) — `ItemBuilder<I>` (R3).
- new [`crates/fern-scene/src/cache.rs`](crates/fern-scene/src/cache.rs) — item-coord cache (R5).
- [`crates/fern-core/src/signal.rs`](crates/fern-core/src/signal.rs) — `map_coalesced` (R6).
- [`examples/scene_showcase/src/main.rs`](examples/scene_showcase/src/main.rs) — incremental updates per phase.
- [`docs/fern-scene.md`](docs/fern-scene.md) — rewrite (R8).
- new [`docs/plans/scene-refactor-plan.md`](docs/plans/scene-refactor-plan.md) — in-tree mirror (R8).

## 7. End-to-end verification

After all phases:

1. `cargo test --workspace` — 2150+ tests including the new regressions for every C/D/E gap.
2. `cargo run -p scene-showcase` — every new capability demonstrable.
3. Manual NVDA / Orca walk of the showcase — Section 5 cards merged via `access_merge_subtree`, AT user can context-menu via right-click, focus order respects `IS_FOCUSABLE` flag.
4. Stress: 10k-item scene with mixed transformed subtrees, cached items, hovers and drags — 60 FPS in `--release`.
5. `cargo doc --no-deps -p fern-scene` clean; doctests run.
6. Visual regression: drag-and-drop scenes (parent-cascade, no snap-back, looping animations after drag-end) all still work.

## 8. Quality bar checklist (must hold at end of R8)

- [ ] No mention of "Phase N" / "Phase 4" / "Phase 6 territory" anywhere in `crates/fern-scene/src/` `///` comments.
- [ ] No `TODO`, `FIXME`, `XXX` comments left in the crate.
- [ ] No `_old`, `_legacy`, `_v2` types or methods.
- [ ] `docs/fern-scene.md` and `docs/fern-scene-a11y.md` reference only the post-refactor API.
- [ ] `cargo doc --no-deps -p fern-scene` builds with zero broken intra-doc links.
- [ ] `cargo test --workspace` green.
- [ ] `cargo run -p scene-showcase` exercises every new capability (rotation, ignore-transformations, per-item events, context menu, opacity, visibility, scene_rect clamping, ensure_visible, cache mode, background grid, scroll-hand-drag, pan_axes, zoomable=false, adopt_scene_size).
- [ ] `crates/fern-scene/src/view.rs` is under 1500 LoC after the split.
- [ ] `crates/fern-scene/src/items.rs` is under 200 LoC after the split.

## 9. Out of scope

- `DeviceCoordinateCache` — defer until profiling justifies. R5 ships `ItemCoordinateCache` only.
- `QGraphicsEffect`-style per-item effect chain (drop shadow, blur, colorize). The engine has `set_blur` per-node; per-item shadows can land later.
- 3D / perspective transforms.
- Item groups as Qt's `QGraphicsItemGroup` (treat-as-one selection / transform). `set_a11y_parent` covers AT; visual-and-interaction grouping is a thinner layer on top of R1/R2/R3 if and when an app needs it.
- Touch / IME at item level — heavyweight tier handles these via real Widget machinery; lightweight tier defers indefinitely.
