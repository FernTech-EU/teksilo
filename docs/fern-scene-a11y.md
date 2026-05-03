# `fern-scene` accessibility

`fern-scene` is built on the principle that **AT structure is not
predictable from visual layout**. A node-graph editor's logical AT
shape is "nodes connected by data-flow ports"; a Skribisto
corkboard's is "Acts containing Scene cards"; a CAD canvas's is
"layers of geometric components". Reading-order Tab over screen-
projected bounds is wrong for all three.

The framework therefore ships two cooperating layers:

1. **Visual-default layer (Phase 5a — this doc).** A quick
   prototype is accessible out of the box: heavyweight widgets
   participate in the AT walker as normal direct children of
   `SceneView`; lightweight `SceneItem`s become synthetic AT nodes
   with role + screen-projected bounds; Tab cycles in reading
   order; arrow keys pan; `+` / `-` zoom. **If these defaults are
   enough for your app, stop reading here.**
2. **Parallel structural layer (Phase 5b — see below once it
   ships).** The same primitives plus app-declared logical groups,
   custom focus / directional callbacks, AT-only actions, auto-graft
   of interactive widget descendants. This is what real apps wire
   when their AT shape diverges from their visual shape.

## Status

| Phase | What | Status |
|-------|------|--------|
| 5a | Visual-default a11y + keyboard navigation | ✅ this doc |
| 5b | A11y-shaping tools (groups / parents / relations / auto-graft / callbacks) | not yet |

## What lands by default (Phase 5a)

### Heavyweight widgets

Each `Scene::add_widget(widget, scene_rect)` call produces a real
`Widget` in the arena. The AT walker emits it as a normal child of
`SceneView`, with its own role / label / actions / bounds set by
the widget's own `accessibility(builder)` impl. Pan / zoom doesn't
change this — the bounds AccessKit sees come from
`arena.bounds(id)`, which the framework keeps post-layout/post-
transform-correct via Phase 0's transform-aware hit-test.

In short: a `Button`, `TextInput`, `ComboBox`, etc. dropped into a
`Scene` works exactly as it would anywhere else, with no extra
wiring.

### Lightweight items

Each `Scene::add_item(...)` produces a `SceneItem` that lives
outside the arena. `SceneView::accessibility` walks the visible
items per the off-screen-mode policy and emits **one synthetic AT
node per item**, with:

- **Role.** Item-shape-derived default — `Role::GraphicsObject` for
  `RectItem` / `PathItem`, `Role::Image` for `ImageItem`,
  `Role::Label` for `TextItem`, `Role::Group` for `GroupItem`.
  Override per-item with `.access_role(...)`.
- **Name.** The item's `label()` (which falls back to `text` for
  `TextItem`). Override with `.access_label(...)`.
- **Description.** Optional. Set with `.access_description(...)`.
- **Bounds.** Screen-projected via the current `view_transform` —
  AccessKit gets coordinates the user can actually point at.
- **Hidden flag.** `.access_hidden(true)` excludes the item from
  the AT tree entirely (useful for purely-decorative shapes).

Each visible item's synthetic NodeId is
`synthetic_node_id(scene_view_id, item_id.as_u64(), SceneItem)`,
deterministic and stable across rebuilds — AT focus survives
re-layouts.

### Off-screen visibility policy

[`A11yOffScreenMode`](../crates/fern-scene/src/a11y.rs) governs
which items the walker emits per AT-rebuild:

- **`ViewportPlusN { n: 1 }` (default)** — items inside the
  viewport plus a one-screen margin. Gives screen-reader users a
  one-screen "lookahead" without forcing pan animation on every
  navigation step.
- **`AllItems`** — every item, regardless of viewport. Heaviest
  mode; appropriate for small scenes (< ~500 items) where AT
  users want a complete table of contents.
- **`ViewportOnly`** — strict; only items intersecting the
  viewport.

```rust
SceneView::new(scene)
    .a11y_off_screen_mode(A11yOffScreenMode::AllItems)
```

### Per-item override chain

Mirrors the widget-level `.access_*` chain documented in
`docs/accessibility-overrides.md`. Available on every built-in
`SceneItem` (`RectItem` / `PathItem` / `ImageItem` / `TextItem` /
`GroupItem`):

```rust
RectItem::new(rect)
    .fill(Color::RED)
    .access_label("Critical alert")
    .access_role(Role::AlertDialog)
    .access_description("Confirm before continuing")
    .access_hidden(false);
```

Custom `SceneItem` impls override `accessibility(builder, ctx)`
directly:

```rust
impl SceneItem for MyConnector {
    fn accessibility(&self, builder: &mut AccessNodeBuilder, ctx: &SceneItemA11yContext) {
        builder.set_role(Role::GraphicsObject);
        builder.set_name(format!("connector from {} to {}", self.from, self.to));
        // ctx.screen_bounds is the projected rect (set on the node by SceneView later);
        // ctx.view_transform exposes the current matrix if you need to draw a custom
        // glyph-relative position; ctx.item_id routes AT actions back to this item.
    }
}
```

The framework writes `set_bounds` on the synthetic node *after*
your `accessibility` impl runs — using the screen-projected bounds
from `ctx.screen_bounds`. Custom items don't need to set bounds
themselves.

## Keyboard navigation

The default scheme on `SceneView` (Phase 5a):

| Key | Action |
|-----|--------|
| `→` | Pan view leftward (reveals scene content to the right) |
| `←` | Pan view rightward |
| `↓` | Pan upward |
| `↑` | Pan downward |
| `+` / `=` | Zoom in 1.25× about viewport center |
| `-` | Zoom out 0.8× about viewport center |
| `0` | Reset zoom to 1.0 about viewport center |

Pan step = ¼ of the smaller viewport axis (capped to ≥ 64 px).
Held-key repeat naturally chains tweens via `Signal::animate_to`.

The handler is `on_key` on the SceneView itself — it only fires
when the SceneView holds focus, **not** when a heavyweight child
(like a `TextInput` inside a card) is focused. Typing letters
into a card never pans the scene. Tab cycle uses the arena's
natural focusable-walk order (Phase 5a). Apps that want a
domain-specific cycle (data-flow order, story order, etc.)
override via the `focus_order(...)` callback that lands in Phase
5b.

App-wide pan/zoom shortcuts (`Ctrl+Plus` for zoom, etc.) should
be wired through the `Shortcut` / `Action` / `Intent` pipeline so
they fire regardless of focus.

## Idle compliance

Pan and zoom remain animated `Signal<f32>`s with the four idle
gates intact. The default keyboard pan ticks once per key-down and
the resulting `animate_to` lands in finite time. AT bounds settle
with the view transform — Phase 5a doesn't yet debounce AT commits
during pan/zoom (planned for Phase 5b once a real screen-reader
test surfaces motion-blur announcements as a problem).

## Worked examples

For now, the corkboard demo at
[`examples/scene_corkboard/`](../examples/scene_corkboard/src/main.rs)
exercises the visual-default surface — every connector and tile is
AT-discoverable, every card is AT-focusable. Phase 5b will extend
the demo with logical Acts → Scene cards groups, `flow_to`
relations between connector-source and -target cards, and the
auto-graft path for the character-picker `ComboBox`.

## See also

- [`docs/fern-scene.md`](fern-scene.md) — the user-facing reference.
- [`docs/accessibility-overrides.md`](accessibility-overrides.md) —
  the widget-level `.access_*` chain that fern-scene's per-item
  chain mirrors.
- [`docs/idle-and-animation.md`](idle-and-animation.md) — the
  scheduler the keyboard pan/zoom plugs into.
- [`docs/plans/scene-plan.md`](plans/scene-plan.md) — the full
  design and Phase 5b shape.
