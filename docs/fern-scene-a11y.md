# `fern-scene` accessibility

`fern-scene` is built on the principle that **AT structure is not
predictable from visual layout**. A node-graph editor's logical AT
shape is "nodes connected by data-flow ports"; a Skribisto
corkboard's is "Acts containing Scene cards"; a CAD canvas's is
"layers of geometric components". Reading-order Tab over screen-
projected bounds is wrong for all three.

The framework therefore ships two cooperating layers:

1. **Visual-default layer (Phase 5a).** A quick prototype is
   accessible out of the box: heavyweight widgets participate in
   the AT walker as normal direct children of `SceneView`;
   lightweight `SceneItem`s become synthetic AT nodes with role +
   screen-projected bounds; Tab cycles in reading order; arrow
   keys pan; `+` / `-` zoom. **If these defaults are enough for
   your app, jump to *Visual default* below and stop there.**
2. **Parallel structural layer (Phase 5b).** App-declared logical
   groups, parents, relations, live regions, landmarks, and
   category tags. The same primitives plus apps' freedom to wire
   *whatever* AT shape makes sense for their domain — independent
   of visual layout.

One piece remains deferred to a later sub-phase:

- **Custom focus-order / directional-navigation callbacks** on
  `SceneView`. Default reading-order Tab cycle is the only mode
  today. Apps that need data-flow-order Tab in a graph editor
  must wait for the dedicated callback API.

Auto-graft of interactive widget descendants into their declared
logical parent **is supported** — see *A11yMode + auto-graft*
below.

## Status

| Phase | What | Status |
|-------|------|--------|
| 5a | Visual-default a11y + keyboard navigation | ✅ landed |
| 5b core | Logical tree (groups, parents, relations, live, landmarks, categories) | ✅ landed |
| 5b auto-graft | Widget descendants routed through declared logical parents | ✅ landed |
| 5b modes | `A11yMode::Cooperative` / `StrictlyParallel` | ✅ landed |
| 5b callbacks | `focus_order(...)` / `directional_navigation(...)` | not yet |

---

## Visual default (Phase 5a)

### Heavyweight widgets

Each `Scene::add_widget(widget, scene_rect)` call produces a real
`Widget` in the arena. The AT walker emits it as a normal child of
`SceneView`, with its own role / label / actions / bounds set by
the widget's own `accessibility(builder)` impl. Pan / zoom doesn't
change this — the bounds AccessKit sees come from
`arena.bounds(id)`, post-layout/post-transform-correct via Phase
0's transform-aware hit-test.

A `Button`, `TextInput`, `ComboBox`, etc. dropped into a `Scene`
works exactly as it would anywhere else, with no extra wiring.

### Lightweight items

Each `Scene::add_item(...)` produces a `SceneItem` that lives
outside the arena. `SceneView::accessibility` walks the visible
items per the off-screen-mode policy and emits **one synthetic AT
node per item**, with:

- **Role.** Item-shape-derived default — `Role::GraphicsObject`
  for `RectItem` / `PathItem`, `Role::Image` for `ImageItem`,
  `Role::Label` for `TextItem`, `Role::Group` for `GroupItem`.
  Override per-item with `.access_role(...)`.
- **Name.** The item's `label()` (which falls back to `text` for
  `TextItem`). Override with `.access_label(...)`.
- **Description.** Optional. Set with `.access_description(...)`.
- **Bounds.** Screen-projected via the current `view_transform`.
- **Hidden flag.** `.access_hidden(true)` excludes the item.

Each visible item's synthetic NodeId is
`synthetic_node_id(scene_view_id, item_id.as_u64(), SceneItem)`,
deterministic and stable across rebuilds.

### Off-screen visibility policy

`A11yOffScreenMode` governs which items the walker emits:

- **`ViewportPlusN { n: 1 }` (default)** — items inside the
  viewport plus a one-screen margin.
- **`AllItems`** — every item, regardless of viewport.
- **`ViewportOnly`** — strict; viewport-intersecting only.

```rust
SceneView::new(scene)
    .a11y_off_screen_mode(A11yOffScreenMode::AllItems)
```

### Per-item override chain

Mirrors widget-level `.access_*`. Available on every built-in:

```rust
RectItem::new(rect)
    .fill(Color::RED)
    .access_label("Critical alert")
    .access_role(Role::AlertDialog)
    .access_description("Confirm before continuing")
    .access_hidden(false);
```

Custom `SceneItem` impls override `accessibility(builder, ctx)`
directly. The framework writes `set_bounds` on the synthetic node
*after* your impl runs.

### Keyboard navigation

| Key | Action |
|-----|--------|
| `→` / `←` | Pan view horizontally |
| `↓` / `↑` | Pan vertically |
| `+` / `=` | Zoom in 1.25× about viewport center |
| `-` | Zoom out 0.8× about viewport center |
| `0` | Reset zoom |

Pan step = ¼ of the smaller viewport axis (≥ 64 px). Held-key
repeat chains tweens via `Signal::animate_to`. The handler is
`on_key` on the SceneView itself — typing letters into a focused
`TextInput` inside a card never pans the scene.

App-wide shortcuts (`Ctrl+Plus` etc.) route through the standard
`Shortcut` / `Action` / `Intent` pipeline.

---

## A11yMode + auto-graft

Two design decisions shape how scene contents land in the AT tree:

### `A11yMode`

```rust
SceneView::new(scene)                                     // default: Cooperative
SceneView::new(scene).a11y_mode(A11yMode::StrictlyParallel)
```

- **`Cooperative`** *(default)*. Visual is the AT structure
  unless overridden. Lightweight items inside the off-screen-mode
  policy emit as direct AT children of `SceneView`; heavyweight
  widgets emit through the arena walker as natural descendants.
  Apps selectively override with `set_a11y_parent` for parts of
  the scene where AT diverges. Right for charts, dashboards,
  simple maps — apps where the visual layout *is* a sensible AT
  structure for most nodes.

- **`StrictlyParallel`**. AT structure is purely declared.
  Lightweight items are emitted **only** if the app placed them
  in the logical tree via `set_a11y_parent`; items without a
  declared parent are suppressed. Heavyweight widgets still
  emit (they own focus / interaction state the AT layer can't
  suppress) but their AT-tree parent is the declared logical
  parent if any, else `SceneView` root. Right for corkboards,
  graph editors, CAD canvases — apps where AT shape is
  fundamentally different from visual layout, and apps would
  override the default for every node anyway.

### Auto-graft

Heavyweight widgets are added via `Scene::add_widget(widget,
scene_rect)` and get an `ItemId` back. Apps declare their AT
parent the same way they declare it for lightweight items:

```rust
let card = scene.add_widget(my_card_widget(), card_rect);
scene.set_a11y_parent(A11yNode::Item(card), Some(A11yNode::Group(act_one)));
```

Behind the scenes, the framework AT walker calls a hook
(`Widget::a11y_redirect_descendant`) on `SceneView` for every
arena-child it's about to emit. SceneView's hook impl checks for
a declared parent on the descendant's `ItemId`; if found, it
returns the synthetic NodeId of the declared parent, telling the
walker to skip its default push. SceneView's own `accessibility()`
emission has already attached the widget's NodeId to the parent's
children list — so the widget appears exactly once, under its
logical parent, with all its real focus / keyboard / action
machinery intact.

`A11yNode::Widget(widget_id)` is reserved for relocating an
*arbitrary* widget. For widgets you added via `Scene::add_widget`,
prefer `A11yNode::Item(item_id)` — same ergonomics, and
auto-graft handles them.

> **Limitation.** The `Widget::a11y_redirect_descendant` hook is
> consulted only on the **direct** arena parent of a child during
> AT walk. So `A11yNode::Widget(widget_id)` only takes effect
> when `widget_id` is a direct child of `SceneView`. Relocating
> a *deeply nested* widget (a `ComboBox` two arena-levels inside
> a Card) would require ancestor-chain queries from the walker,
> which is a planned extension. Apps that need this today should
> structure the widget so the relocation target is a direct
> SceneView child, or expose the inner widget's id and add it to
> the Scene as its own `add_widget` entry.

## Parallel structural layer (Phase 5b)

The visual-default layer is correct out of the box but rigid:
items appear as flat children of `SceneView`, in the order their
`bounds_in_scene` was queried by the spatial index. For real apps
the AT shape needs to diverge from that — either to add structure
(Acts containing scenes; Subgraphs containing nodes) or to fix
order (story chronology vs. left-to-right reading order; data-flow
direction in a node graph).

### `A11yNode` — the universal address

```rust
pub enum A11yNode {
    Item(ItemId),         // a lightweight SceneItem
    Group(A11yGroupId),   // a virtual logical group
    Widget(WidgetId),     // a real interactive widget in the arena
}
```

Every Phase 5b API targets `A11yNode`. Apps mix item / group /
widget handles uniformly when declaring parents and relations.

> **Note.** For widgets you added via `Scene::add_widget`, prefer
> `A11yNode::Item(item_id)` (the address you got back from
> `add_widget`). The walker auto-grafts it. Reserve
> `A11yNode::Widget(widget_id)` for relocating *non-add_widget*
> widgets — typically a descendant of a heavyweight scene item
> that should logically belong elsewhere.

### Logical groups

`A11yGroup` is a virtual AT node with no visual counterpart. Apps
declare them to introduce structure that doesn't follow scene-
coordinate placement.

```rust
let act_one = scene.add_a11y_group(
    A11yGroup::builder()
        .label("Act 1")
        .role(accesskit::Role::Group)
);
```

### Parent declarations

Detach an item / group / widget from its visual position in the
AT tree and reparent it under a logical container. Independent of
scene coordinates.

```rust
scene.set_a11y_parent(
    A11yNode::Item(scene_card),
    Some(A11yNode::Group(act_one)),
);
```

`parent = None` clears the declaration → the node falls back to
the SceneView root in the next AT-rebuild. The two-pass walker
emits roots first (groups + unparented items), then descends DFS
through declared children.

A malformed cycle (`A → B → A`) is handled by a per-rebuild visit-
set: each node is emitted at most once, on its first appearance
in DFS order. The result is well-defined but not what the user
intended; apps should avoid declaring cycles.

### Relations

Cross-tree relationships, mapped to AccessKit's relationship
arrays.

```rust
scene.add_a11y_relation(
    A11yNode::Item(node_a),
    A11yRelation::FlowTo,           // data-flow direction
    A11yNode::Item(node_b),
);
scene.add_a11y_relation(
    A11yNode::Item(error_message),
    A11yRelation::DescribedBy,
    A11yNode::Item(input_field),
);
```

Variants: `Controls`, `DescribedBy`, `LabelledBy`, `FlowTo`. Multiple
relations between the same pair are kept (an edge can be both
`LabelledBy` and `DescribedBy`). The walker writes them onto
`AccessKit::Node` after the hierarchy emit, so resolution sees
the final synthetic NodeIds.

### Live regions

```rust
scene.set_a11y_live(
    A11yNode::Group(status_panel),
    accesskit::Live::Polite,
);
```

AT clients announce changes to the targeted node without focus.
`Live::Off` clears the declaration.

### Landmarks

```rust
scene.set_a11y_landmark(
    A11yNode::Group(navigation_panel),
    accesskit::Role::Navigation,
);
```

Overrides the node's role with one of the `Region`/`Main`/
`Navigation`/`Search` etc. variants. AT clients with "jump to
landmark" navigation surface them as targets. `Role::Unknown`
clears the declaration.

### Category tags

```rust
scene.set_a11y_categories(
    A11yNode::Item(node),
    &[A11yCategory::new("graph-node"), A11yCategory::new("draggable")],
);
```

App-defined tags surfaced to AT clients that support categorized
navigation (VoiceOver rotor, NVDA quick-nav). Phase 5b stores them
on the Scene; surfacing into AccessKit as a custom property is
deferred to Phase 7.

### Removal

`Scene::remove_a11y_group(id)` drops the group, every parent
declaration that points at it, and every relation / live / landmark
/ category targeting it. Children that declared this group as
their parent fall back to the SceneView root.

---

## Worked examples

### Story corkboard (Skribisto-style)

```rust
let mut scene = Scene::new();

let act1 = scene.add_a11y_group(A11yGroup::builder().label("Act I — Setup"));
let act2 = scene.add_a11y_group(A11yGroup::builder().label("Act II — Confrontation"));
let act3 = scene.add_a11y_group(A11yGroup::builder().label("Act III — Resolution"));

for (i, (title, body)) in cards.iter().enumerate() {
    let card_rect = grid_rect(i);
    let card = scene.add_widget(card_widget(title, body), card_rect);
    let act = match i / 3 { 0 => act1, 1 => act2, _ => act3 };
    // Auto-graft: card's WidgetId lands under its act in the AT
    // tree, with full focus / keyboard machinery intact.
    scene.set_a11y_parent(A11yNode::Item(card), Some(A11yNode::Group(act)));
}

// Connectors AT-grouped under their source act.
for (i, pair) in card_rects.windows(2).enumerate() {
    let path = step_path(pair[0], pair[1]);
    let id = scene.add_item(
        PathItem::new(path, bounds_of(pair[0], pair[1]))
            .stroke(connector_color, 2.0)
            .access_label(format!("connector {} → {}", i + 1, i + 2)),
    );
    let act = match i / 3 { 0 => act1, 1 => act2, _ => act3 };
    scene.set_a11y_parent(A11yNode::Item(id), Some(A11yNode::Group(act)));
}

let view = SceneView::new(scene);
```

### Node-graph editor

The interesting part: the AT shape is "Subgraph → nodes →
out-ports", and connections are `FlowTo` relations. Tab follows
data-flow rather than scene reading-order (deferred:
`focus_order(...)` callback).

```rust
let subgraph_a = scene.add_a11y_group(A11yGroup::builder().label("Decoder"));
for node in subgraph_a_nodes {
    let id = scene.add_item(node_item(...));
    scene.set_a11y_parent(A11yNode::Item(id), Some(A11yNode::Group(subgraph_a)));
}
for connection in connections {
    scene.add_a11y_relation(
        A11yNode::Item(connection.source),
        A11yRelation::FlowTo,
        A11yNode::Item(connection.target),
    );
}
```

### CAD canvas

Layers as logical groups; geometry items beneath. A "Locked" badge
inside a layer is announced as a live region so users hear when
state changes.

```rust
let layer_floor = scene.add_a11y_group(
    A11yGroup::builder().label("Floor plan").role(Role::Group)
);
for component in floor_components {
    let id = scene.add_item(...);
    scene.set_a11y_parent(A11yNode::Item(id), Some(A11yNode::Group(layer_floor)));
}
let lock_badge = scene.add_item(...);
scene.set_a11y_live(A11yNode::Item(lock_badge), accesskit::Live::Polite);
```

---

## Idle compliance

Pan and zoom remain animated `Signal<f32>`s with the four idle
gates intact. The default keyboard pan ticks once per key-down and
the resulting `animate_to` lands in finite time. AT bounds settle
with the view transform — Phase 5 doesn't yet debounce AT commits
during pan/zoom (planned for Phase 7 once a real screen-reader
test surfaces motion-blur announcements as a problem).

## See also

- [`docs/fern-scene.md`](fern-scene.md) — the user-facing reference.
- [`docs/accessibility-overrides.md`](accessibility-overrides.md) —
  the widget-level `.access_*` chain that the per-item chain mirrors.
- [`docs/idle-and-animation.md`](idle-and-animation.md) — the
  scheduler the keyboard pan/zoom plugs into.
- [`docs/plans/scene-plan.md`](plans/scene-plan.md) — the full
  design and remaining-work shape.
