# `fern-scene` accessibility

The user-facing reference for shaping a scene's accessibility tree
without touching the framework AT walker. Pairs with the visual
reference at [`fern-scene.md`](fern-scene.md).

`SceneView` ships an accessible tree out of the box: every visible
heavyweight widget participates as a normal child, every visible
lightweight item gets a synthetic AT node with role +
screen-projected bounds, Tab cycles in scene-insertion order. This
document covers the levers that override that default when the AT
shape needs to *diverge* from the visual layout — the typical case
for story corkboards, node-graph editors, CAD canvases, anything
where "what the eye sees" and "what the ears need" aren't the same
tree.

---

## Two layers

The AT machinery has two cooperating layers:

1. **Visual default.** [`A11yOffScreenMode`](../crates/fern-scene/src/a11y.rs)
   decides which off-viewport entries the walker still emits. Pick
   `Cooperative` (default) when the visual layout *is* a sensible
   reading order; pick `StrictlyParallel` when AT shape diverges
   meaningfully from visual layout.
2. **Logical structural API.** [`A11yGroup`](../crates/fern-scene/src/a11y.rs),
   [`A11yNode`](../crates/fern-scene/src/a11y.rs), parents,
   relations, auto-graft, and a focus-order callback let apps
   declare an AT tree that has no visual counterpart. Cards live in
   *Acts*, nodes live in *Subgraphs*, components live in *Layers*.

---

## Off-screen mode

```rust
SceneView::new(scene)
    .off_screen_a11y_mode(A11yOffScreenMode::OnlyVisible)
    // OnlyVisible (default) | All | RegionMargin(units)
```

Decides which items the AT walker emits when the user pans / zooms.
`OnlyVisible` keeps the AT tree in lockstep with the viewport;
`All` always emits everything (good for tiny scenes); `RegionMargin`
emits items within `margin` of the visible region (good for "user
can Tab to the next off-screen item" patterns).

---

## A11y mode

```rust
SceneView::new(scene).a11y_mode(A11yMode::Cooperative)
// Cooperative | StrictlyParallel
```

`Cooperative` (default) — items / widgets without a declared logical
parent appear as direct children of the SceneView in the AT tree.
Pick this for charts, dashboards, simple maps where visual layout
*is* the reading order.

`StrictlyParallel` — only entries placed in the logical tree
(`set_a11y_parent`, `add_a11y_group`) are emitted. Items without an
explicit declaration are **suppressed**. Pick this for corkboards /
graph editors where AT shape should ignore visual layout entirely.

---

## Logical groups

A virtual AT container with no visual counterpart. Pure structure:
no hit-test, no paint.

```rust
let act_one = scene.add_a11y_group(
    A11yGroup::builder()
        .label(tr!("act_one"))
        .role(accesskit::Role::Region),
);
scene.set_a11y_parent(A11yNode::Item(scene_card), Some(A11yNode::Group(act_one)));
```

Groups can themselves nest under other groups via
`set_a11y_parent(A11yNode::Group(child_group), Some(A11yNode::Group(parent_group)))`.
Build arbitrary AT-only trees that have no relationship to the
visual layout.

---

## Reparenting

```rust
scene.set_a11y_parent(A11yNode::Item(child), Some(A11yNode::Item(parent)));
scene.set_a11y_parent(A11yNode::Item(card), None);   // back to root
scene.a11y_parent_of(A11yNode::Item(card)) -> Option<A11yNode>
```

`A11yNode` addresses any node in the parallel tree:

| `A11yNode::*` | Targets |
|---|---|
| `Item(ItemId)` | Any scene entry — lightweight item or heavyweight widget added via `Scene::add_widget` |
| `Group(A11yGroupId)` | A logical group declared via `add_a11y_group` |
| `Widget(WidgetId)` | A real interactive widget addressed by its arena id — typically a *descendant* of a heavyweight scene item that should logically belong elsewhere |

For widgets you added via `Scene::add_widget`, prefer
`A11yNode::Item(item_id)` — the walker handles the heavyweight
auto-graft for you (the real widget's `NodeId` lands under the
declared parent without you doing anything special).

---

## Relations

Cross-tree relationships independent of parenting.

```rust
scene.add_a11y_relation(A11yNode::Item(button), A11yRelation::Controls,    A11yNode::Item(menu));
scene.add_a11y_relation(A11yNode::Item(field),  A11yRelation::DescribedBy, A11yNode::Item(error_msg));
scene.add_a11y_relation(A11yNode::Item(node_a), A11yRelation::FlowTo,      A11yNode::Item(node_b));
```

`A11yRelation` variants:

- `Controls` — `from` controls `to` (button opening a menu).
- `DescribedBy` — `from` is described by `to` (cross-item annotation).
- `LabelledBy` — `from` is labelled by `to` (cross-item label).
- `FlowTo` — logical reading flow from `from` to `to`. Many node-graph
  editors use this so VoiceOver / NVDA "next item" follows
  data-flow order rather than scene-insertion order.

---

## Live regions

Mark a scene entry as a polite or assertive live region:

```rust
scene.set_a11y_live(A11yNode::Item(toast), accesskit::Live::Polite);
```

Updates to the entry's AT name / value are announced.

---

## Landmark roles

Promote a group to a landmark for screen-reader navigation:

```rust
scene.set_a11y_landmark(A11yNode::Group(toolbar_group), accesskit::Role::Toolbar);
```

---

## Categories (rotor / quick-nav)

App-defined tags surfaced to AT clients that support categorized
navigation (VoiceOver rotor on macOS, NVDA quick-nav on Windows).
Apps coin their own category names — `"node"`, `"connector"`,
`"comment"` — and bucket items into them:

```rust
scene.add_a11y_category(A11yNode::Item(node), A11yCategory::new("node"));
scene.add_a11y_category(A11yNode::Item(edge), A11yCategory::new("connector"));
```

---

## Subtree mode for items

Each `SceneItem` builder carries an
[`AccessSubtreeMode`](../crates/fern-scene/src/items.rs):

| Mode | Effect |
|---|---|
| `Inherit` (default) | Descendants emit AT nodes normally. |
| `Exclude` | Descendants are pruned from the AT tree. |
| `Merge` | Descendants' label / value / actions concatenate into this item's AT node and they're pruned individually. The subtree reads as one element. |

```rust
RectItem::new(rect)
    .label(tr!("card_idea_1"))
    .access_merge_subtree();    // card with rect + label + indicator dot reads as one
```

`Merge` is the right pattern for a card whose visual subparts
(background rect, label, status dot) are conceptually one element
for the AT user. `Exclude` is useful for animated decorations whose
emission would be noisy (a pulsing recording dot, a spinner).

---

## Override chain (`access_*` builders)

Every built-in item's builder, every custom item that invokes the
`item_a11y_builders!()` macro, and `A11yGroupBuilder` / `SceneView`
expose a parallel `.access_*` chain that mirrors `WidgetBuilder` on
the widget tier:

```rust
RectItem::new(rect)
    .access_label(tr!("save"))
    .access_description(tr!("save_explanation"))
    .access_role(accesskit::Role::Button)
    .access_subtree(AccessSubtreeMode::Merge);
```

`access_label_literal`, `access_description_literal` (and friends)
are `#[doc(hidden)]` twins for explicitly-untranslated strings.

---

## Custom focus order

Apps that need a focus traversal that diverges from scene-insertion
order install a callback. Common cases:

- Story corkboards — Tab follows Acts → Scene cards in story order.
- Node-graph editors — Tab follows data-flow order via `FlowTo`
  relations.
- CAD canvases — Tab follows depth-then-breadth tree order.
- Timelines — Tab follows chronological order.

```rust
SceneView::new(scene).focus_order(|scene, dir, current| {
    // dir: FocusDirection::{Forward, Backward}
    // current: Option<ItemId> — None on initial Tab
    match dir {
        FocusDirection::Forward => next_in_my_order(scene, current),
        FocusDirection::Backward => prev_in_my_order(scene, current),
    }
});
```

The callback is `Fn(&Scene, FocusDirection, Option<ItemId>) -> Option<ItemId>`.
Returning `None` ends the cycle (the focus exits the SceneView and
moves to the next focusable in the parent).

When the focused item is off-viewport, the SceneView calls
[`ensure_visible`](fern-scene.md#background--foreground-hooks)
automatically so the focus indicator stays on screen.

---

## SceneView own AT name + nesting

```rust
SceneView::new(scene)
    .a11y_label(tr!("graph_data_area"))
    .nested_a11y(true)              // emit Role::Region instead of Role::Pane
    .a11y_bounds_space(A11yBoundsSpace::Scene)
    // Screen (default, view-projected) | Scene (independent of pan / zoom)
```

`Pane` is the right role for a top-level scene; `Region` is for an
inner scene inside another (a chart's data area inside a chart's
chrome). Switch via `nested_a11y(true)`.

`a11y_bounds_space` controls the coordinate frame reported to AT for
items: `Screen` (view-projected, the framework default) is right for
most cases; `Scene` is right when AT users should be able to reason
about "where in the design" an item sits, independent of the current
pan / zoom (CAD canvases, blueprint editors).

---

## Worked example: story corkboard

Acts contain Scene cards. Acts are virtual groups; Scene cards are
heavyweight widgets. AT shape ignores visual layout entirely.

```rust
let mut scene = Scene::new();
let act1 = scene.add_a11y_group(A11yGroup::builder().label(tr!("act_1")));
let act2 = scene.add_a11y_group(A11yGroup::builder().label(tr!("act_2")));

let scene_card_1 = scene.add_widget(card_widget("Opening"), Rect::new(0.0, 0.0, 200.0, 120.0));
let scene_card_2 = scene.add_widget(card_widget("Climax"),  Rect::new(220.0, 0.0, 200.0, 120.0));

scene.set_a11y_parent(A11yNode::Item(scene_card_1), Some(A11yNode::Group(act1)));
scene.set_a11y_parent(A11yNode::Item(scene_card_2), Some(A11yNode::Group(act2)));

let view = SceneView::new(scene)
    .a11y_mode(A11yMode::StrictlyParallel)   // ignore visual layout entirely
    .focus_order(|scene, dir, current| story_order_traversal(scene, dir, current));
```

Screen-reader output: "Act 1, Region. Opening, Card. Act 2, Region.
Climax, Card." Tab cycles in story order regardless of where the
cards sit visually.

---

## Worked example: graph editor

Nodes contain Ports; connector lines declare data flow via `FlowTo`.

```rust
let mut scene = Scene::new();

let node_a = scene.add_widget(node_widget("A"), Rect::new(0.0, 0.0, 120.0, 80.0));
let node_b = scene.add_widget(node_widget("B"), Rect::new(300.0, 0.0, 120.0, 80.0));

// Connector lines are lightweight PathItems.
let edge = scene.add_item(
    PathItem::new(connector_path(), edge_aabb()).stroke(Color::BLACK, 2.0),
    Point::ZERO,
);
scene.add_a11y_category(A11yNode::Item(edge), A11yCategory::new("connector"));
scene.add_a11y_category(A11yNode::Item(node_a), A11yCategory::new("node"));
scene.add_a11y_category(A11yNode::Item(node_b), A11yCategory::new("node"));

// Logical flow: data flows A → B.
scene.add_a11y_relation(
    A11yNode::Item(node_a), A11yRelation::FlowTo, A11yNode::Item(node_b),
);

let view = SceneView::new(scene)
    .focus_order(|scene, dir, current| flow_order_traversal(scene, dir, current));
```

VoiceOver rotor offers "Nodes" and "Connectors" categories; "next
item" via the rotor follows the user's chosen category.

---

## Worked example: CAD canvas

Components belong to Layers. Layers are virtual groups. AT bounds
are reported in **scene** coordinates so AT users can reason about
"the gear is at (150, 200) in the design" regardless of pan / zoom.

```rust
let mut scene = Scene::new();
let layer_drive = scene.add_a11y_group(A11yGroup::builder().label(tr!("drive_layer")));
let layer_frame = scene.add_a11y_group(A11yGroup::builder().label(tr!("frame_layer")));

let gear = scene.add_widget(gear_widget(), Rect::new(150.0, 200.0, 60.0, 60.0));
scene.set_a11y_parent(A11yNode::Item(gear), Some(A11yNode::Group(layer_drive)));

let beam = scene.add_widget(beam_widget(), Rect::new(0.0, 280.0, 400.0, 20.0));
scene.set_a11y_parent(A11yNode::Item(beam), Some(A11yNode::Group(layer_frame)));

let view = SceneView::new(scene)
    .a11y_mode(A11yMode::StrictlyParallel)
    .a11y_bounds_space(A11yBoundsSpace::Scene)
    .nested_a11y(true)
    .a11y_label(tr!("design_canvas"));
```

---

## Reference

- Implementation: [`crates/fern-scene/src/a11y.rs`](../crates/fern-scene/src/a11y.rs),
  [`crates/fern-scene/src/scene.rs`](../crates/fern-scene/src/scene.rs)
  (the `Scene::add_a11y_*` / `set_a11y_*` API), and the AT walker in
  [`crates/fern-scene/src/view.rs`](../crates/fern-scene/src/view.rs).
- Widget-tier override surface: [`docs/accessibility-overrides.md`](accessibility-overrides.md).
- AccessKit reference: <https://accesskit.dev>.
