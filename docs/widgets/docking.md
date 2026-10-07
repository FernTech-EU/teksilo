<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# DockingLayout

![DockingLayout preview](img/docking.png)

`DockingLayout` — a VS Code-style dockable layout: a fixed centre slot
(the app's main content) surrounded by four collapsible, splittable,
draggable side regions (leading / trailing / top / bottom), backed by a
cloneable, serializable `DockingModel`.

## Public types

| Kind | Name |
| ---: | :--- |
| `struct` | [`DockingLayout`](#dockinglayout) — The docking layout widget |
| `type` | [`DockRailSlot`](#dockrailslot) — Factory for a rail slot widget (rebuilt on each rail rebuild) |
| `struct` | [`DockActionId`](#dockactionid) — Stable identity for a `DockAction` |
| `enum` | [`DockActionPlacement`](#dockactionplacement) — Where a `DockAction` sits along the rail's column |
| `struct` | [`DockAction`](#dockaction) — A **dockless command button** in the activity rail: it looks and behaves like an activity item, but opens no panel — activating it just runs a closure |
| `struct` | [`DockRail`](#dockrail) — App-facing configuration for a side's activity rail (Rail presentation) |
| `enum` | [`DockSide`](#dockside) — One of the four dockable sides |
| `enum` | [`DockCorner`](#dockcorner) — One of the four corners of the container |
| `struct` | [`CornerOwners`](#cornerowners) — Which side owns each corner |
| `struct` | [`DockWidgetId`](#dockwidgetid) — Process-unique identity for a registered dock widget (the atomic unit) |
| `struct` | [`DockTabId`](#docktabid) — Process-unique identity for a dock tab (a tab of a side's TabWidget) |
| `enum` | [`TabPresentation`](#tabpresentation) — How a side surfaces its tabs: an in-side strip (the TabWidget's own bar) or an always-visible activity rail outboard of the collapsible content |
| `enum` | [`DockOpenMode`](#dockopenmode) — Placement mode for a programmatically-opened dock |
| `struct` | [`DockOpenLocation`](#dockopenlocation) — Target for `DockingModel::open_dock` / `DockingModel::move_dock` |
| `enum` | [`DockRailItemSize`](#dockrailitemsize) — Activity-bar item size for a side's rail (context-menu "Activity bar size") |
| `enum` | [`DockTabDisplay`](#docktabdisplay) — How a side's dock tabs render (context-menu "Tab size") |
| `struct` | [`DockPolicy`](#dockpolicy) — App-declared policy that **locks down end-user layout edits** on a `DockingLayout` |
| `struct` | [`DockingModel`](#dockingmodel) — The shared docking-layout model |
| `struct` | [`DockWidget`](#dockwidget) — App-facing declaration of a dock widget: identity, chrome metadata, and a lazy content factory |
| `struct` | [`DockLayoutState`](#docklayoutstate) — The full serializable snapshot of a `DockingModel` |

## Public functions

### `DockingLayout`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(model: DockingModel)`](#dockinglayout-new) |
| | **Builder methods** |
| `Self` | [`rail(rail: DockRail)`](#dockinglayout-rail) |
| `Self` | [`rails(rails: impl IntoIterator<Item = DockRail>)`](#dockinglayout-rails) |
| `Self` | [`center(widget: impl teksilo_core::IntoTeksiChild)`](#dockinglayout-center) |
| `Self` | [`policy(policy: DockPolicy)`](#dockinglayout-policy) |
| `Self` | [`disable_side(side: DockSide)`](#dockinglayout-disable_side) |
| `Self` | [`dock(dock: DockWidget)`](#dockinglayout-dock) |
| `Self` | [`docks(docks: impl IntoIterator<Item = DockWidget>)`](#dockinglayout-docks) |

### `DockActionId`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`named(name: &str)`](#dockactionid-named) |
| `Self` | [`from_raw(v: u64)`](#dockactionid-from_raw) |
| | **Methods** |
| `u64` | [`raw()`](#dockactionid-raw) |

### `DockAction`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(id: DockActionId, label: impl Into<LocalizedString>, icon: impl Fn() -> IconWidget + 'static, on_activate: impl Fn(&mut EventContext) + 'static)`](#dockaction-new) |
| | **Builder methods** |
| `Self` | [`placement(placement: DockActionPlacement)`](#dockaction-placement) |
| `Self` | [`tooltip(tooltip: impl Into<LocalizedString>)`](#dockaction-tooltip) |
| `Self` | [`enabled(enabled: impl Into<Prop<bool>>)`](#dockaction-enabled) |
| `Self` | [`toggled(state: Signal<bool>)`](#dockaction-toggled) |
| | **Methods** |
| `DockActionId` | [`id()`](#dockaction-id) |

### `DockRail`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(side: DockSide)`](#dockrail-new) |
| | **Builder methods** |
| `Self` | [`size(size: IconButtonSize)`](#dockrail-size) |
| `Self` | [`background(color: impl Into<ColorProp>)`](#dockrail-background) |
| `Self` | [`divider()`](#dockrail-divider) |
| `Self` | [`divider_color(color: impl Into<ColorProp>)`](#dockrail-divider_color) |
| `Self` | [`top_slot<W: Widget + 'static>(f: impl Fn() -> W + 'static)`](#dockrail-top_slot) |
| `Self` | [`bottom_slot<W: Widget + 'static>(f: impl Fn() -> W + 'static)`](#dockrail-bottom_slot) |
| `Self` | [`leading_slot<W: Widget + 'static>(f: impl Fn() -> W + 'static)`](#dockrail-leading_slot) |
| `Self` | [`trailing_slot<W: Widget + 'static>(f: impl Fn() -> W + 'static)`](#dockrail-trailing_slot) |
| `Self` | [`action(action: DockAction)`](#dockrail-action) |
| `Self` | [`actions(actions: impl IntoIterator<Item = DockAction>)`](#dockrail-actions) |
| `Self` | [`overflow_icon(f: impl Fn() -> IconWidget + 'static)`](#dockrail-overflow_icon) |

### `DockSide`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `bool` | [`is_horizontal_axis()`](#dockside-is_horizontal_axis) |
| | **Constants and types** |
| `[DockSide` | [`ALL`](#dockside-all) |

### `DockCorner`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `(DockSide, DockSide)` | [`adjacent_sides()`](#dockcorner-adjacent_sides) |
| | **Constants and types** |
| `[DockCorner` | [`ALL`](#dockcorner-all) |

### `CornerOwners`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `DockSide` | [`owner(corner: DockCorner)`](#cornerowners-owner) |
|  | [`set(corner: DockCorner, owner: DockSide)`](#cornerowners-set) |

### `DockWidgetId`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`fresh()`](#dockwidgetid-fresh) |
| `Self` | [`from_raw(v: u64)`](#dockwidgetid-from_raw) |
| | **Methods** |
| `u64` | [`raw()`](#dockwidgetid-raw) |

### `DockTabId`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`fresh()`](#docktabid-fresh) |
| `Self` | [`from_raw(v: u64)`](#docktabid-from_raw) |
| | **Methods** |
| `u64` | [`raw()`](#docktabid-raw) |

### `DockOpenLocation`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`side(side: DockSide)`](#dockopenlocation-side) |
| | **Builder methods** |
| `Self` | [`stack()`](#dockopenlocation-stack) |
| `Self` | [`new_tab()`](#dockopenlocation-new_tab) |

### `DockRailItemSize`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `bool` | [`shows_label()`](#dockrailitemsize-shows_label) |

### `DockTabDisplay`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `bool` | [`shows_icon()`](#docktabdisplay-shows_icon) |
| `bool` | [`shows_text()`](#docktabdisplay-shows_text) |

### `DockPolicy`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`locked()`](#dockpolicy-locked) |

### `DockingModel`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new()`](#dockingmodel-new) |
| | **Methods** |
| `Signal<u64>` | [`version()`](#dockingmodel-version) |
| `Signal<u64>` | [`geometry_version()`](#dockingmodel-geometry_version) |
| `bool` | [`consume_animate_flag()`](#dockingmodel-consume_animate_flag) |
| `bool` | [`is_registered(id: DockWidgetId)`](#dockingmodel-is_registered) |
|  | [`set_side_rail(side: DockSide, thickness: f32)`](#dockingmodel-set_side_rail) |
|  | [`set_side_size(side: DockSide, size: f32)`](#dockingmodel-set_side_size) |
|  | [`set_side_min_size(side: DockSide, min: f32)`](#dockingmodel-set_side_min_size) |
|  | [`set_policy(policy: DockPolicy)`](#dockingmodel-set_policy) |
| `DockPolicy` | [`policy()`](#dockingmodel-policy) |
|  | [`set_side_enabled(side: DockSide, enabled: bool)`](#dockingmodel-set_side_enabled) |
| `bool` | [`is_side_enabled(side: DockSide)`](#dockingmodel-is_side_enabled) |
|  | [`set_side_visible(side: DockSide, visible: bool)`](#dockingmodel-set_side_visible) |
|  | [`set_side_visible_immediate(side: DockSide, visible: bool)`](#dockingmodel-set_side_visible_immediate) |
|  | [`toggle_side_visible(side: DockSide)`](#dockingmodel-toggle_side_visible) |
|  | [`select_tab(side: DockSide, tab_idx: usize)`](#dockingmodel-select_tab) |
|  | [`select_tab_by_id(side: DockSide, tab_id: DockTabId)`](#dockingmodel-select_tab_by_id) |
|  | [`set_tab_hidden(tab_id: DockTabId, hidden: bool)`](#dockingmodel-set_tab_hidden) |
| `bool` | [`is_tab_hidden(tab_id: DockTabId)`](#dockingmodel-is_tab_hidden) |
| `DockRailItemSize` | [`side_rail_size(side: DockSide)`](#dockingmodel-side_rail_size) |
|  | [`set_side_rail_size(side: DockSide, size: DockRailItemSize)`](#dockingmodel-set_side_rail_size) |
| `Signal<DockRailItemSize>` | [`rail_size_mode_signal(side: DockSide)`](#dockingmodel-rail_size_mode_signal) |
| `DockTabDisplay` | [`side_tab_display(side: DockSide)`](#dockingmodel-side_tab_display) |
|  | [`set_side_tab_display(side: DockSide, display: DockTabDisplay)`](#dockingmodel-set_side_tab_display) |
|  | [`set_corner(corner: DockCorner, owner: DockSide)`](#dockingmodel-set_corner) |
|  | [`open_dock(id: DockWidgetId, loc: DockOpenLocation)`](#dockingmodel-open_dock) |
|  | [`promote_to_tab(id: DockWidgetId, side: DockSide, at_tab: usize)`](#dockingmodel-promote_to_tab) |
|  | [`split_into_tab(id: DockWidgetId, side: DockSide, tab_idx: usize, pane_idx: usize, before: bool)`](#dockingmodel-split_into_tab) |
|  | [`stack_into_tab(id: DockWidgetId, side: DockSide, tab_idx: usize)`](#dockingmodel-stack_into_tab) |
|  | [`move_dock(id: DockWidgetId, loc: DockOpenLocation)`](#dockingmodel-move_dock) |
|  | [`close_tab(tab_id: DockTabId)`](#dockingmodel-close_tab) |
|  | [`move_tab(tab_id: DockTabId, target_side: DockSide, at_tab: usize)`](#dockingmodel-move_tab) |
|  | [`close_dock(id: DockWidgetId)`](#dockingmodel-close_dock) |
|  | [`toggle_dock(id: DockWidgetId)`](#dockingmodel-toggle_dock) |
|  | [`reveal_dock(id: DockWidgetId)`](#dockingmodel-reveal_dock) |
| `bool` | [`is_dock_open(id: DockWidgetId)`](#dockingmodel-is_dock_open) |
| `Option<DockLoc>` | [`dock_location(id: DockWidgetId)`](#dockingmodel-dock_location) |
| `Signal<bool>` | [`dock_open_signal(id: DockWidgetId)`](#dockingmodel-dock_open_signal) |
| `bool` | [`is_side_visible(side: DockSide)`](#dockingmodel-is_side_visible) |
| `Signal<bool>` | [`side_visible_signal(side: DockSide)`](#dockingmodel-side_visible_signal) |
| `Signal<usize>` | [`side_selected_tab_signal(side: DockSide)`](#dockingmodel-side_selected_tab_signal) |
| `usize` | [`side_selected_tab(side: DockSide)`](#dockingmodel-side_selected_tab) |
| `TabPresentation` | [`side_presentation(side: DockSide)`](#dockingmodel-side_presentation) |
| `f32` | [`side_size(side: DockSide)`](#dockingmodel-side_size) |
| `f32` | [`side_min_size(side: DockSide)`](#dockingmodel-side_min_size) |
| `f32` | [`side_rail_thickness(side: DockSide)`](#dockingmodel-side_rail_thickness) |
| `bool` | [`side_has_rail(side: DockSide)`](#dockingmodel-side_has_rail) |
| `DockSide` | [`corner_owner(corner: DockCorner)`](#dockingmodel-corner_owner) |
| `usize` | [`tab_count(side: DockSide)`](#dockingmodel-tab_count) |
| `Option<DockTabId>` | [`tab_id_at(side: DockSide, idx: usize)`](#dockingmodel-tab_id_at) |
|  | [`set_tab_title(tab_id: DockTabId, title: Option<LocalizedString>)`](#dockingmodel-set_tab_title) |
| `Option<LocalizedString>` | [`tab_title(tab_id: DockTabId)`](#dockingmodel-tab_title) |
| `Option<DockTabId>` | [`activity_of(dock_id: DockWidgetId)`](#dockingmodel-activity_of) |
|  | [`set_dock_activity_title(dock_id: DockWidgetId, title: impl Into<LocalizedString>)`](#dockingmodel-set_dock_activity_title) |
| `Vec<DockSide>` | [`enabled_move_targets(from: DockSide)`](#dockingmodel-enabled_move_targets) |
| `super::state::DockLayoutState` | [`export_state()`](#dockingmodel-export_state) |
|  | [`import_state(state: &super::state::DockLayoutState)`](#dockingmodel-import_state) |

### `DockWidget`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new<W: Widget + 'static>(id: DockWidgetId, title: impl Into<LocalizedString>, factory: impl Fn(DockWidgetId) -> W + 'static)`](#dockwidget-new) |
| | **Builder methods** |
| `Self` | [`icon(f: impl Fn() -> IconWidget + 'static)`](#dockwidget-icon) |
| `Self` | [`header_actions(f: impl Fn(DockWidgetId) -> Vec<ToolbarItem> + 'static)`](#dockwidget-header_actions) |
| `Self` | [`show_header(show: bool)`](#dockwidget-show_header) |
| `Self` | [`default_location(loc: DockOpenLocation)`](#dockwidget-default_location) |

## Detailed description

The structure is four levels deep:

```text
DockingLayout
└── Centre + 4 Sides
    └── Side = [optional DockActivityBar rail] + collapsible content region
        └── content region holds ONE TabWidget (strip optional / replaced
            by the rail)
            └── Tab → DockArrangement (a Splitter of panes, each a single
                DockWidget or a ToolBox of DockWidgets)
                └── DockWidget — the atomic dockable unit
```

## Density

The picture above is the widget at `TargetDensity::Compact`, the mouse-and-keyboard ladder. Below is the same subject on the same canvas with only the ladder changed, so what moves is the density and nothing else — where the subject no longer fits, that is what the denser targets cost it at that size. See `docs/density-and-targets.md`.

**Touch**

![DockingLayout at Touch density](img/docking-touch.png)

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/docking/index.html)

<a id="dockinglayout"></a>

## `pub struct DockingLayout`

The docking layout widget. See the module docs.

```ignore
let model = DockingModel::new();
// …declare panels + an initial layout on `model`…
DockingLayout::new(model.clone())
    .center(editor)
    .dock(DockWidget::new(EXPLORER, lit!("Explorer"), |_| Explorer::new()))
```

```rust
pub struct DockingLayout { /* fields */ }
```

### Methods

<a id="dockinglayout-new"></a>

#### `pub fn new(model: DockingModel) -> Self`

Create a docking layout over a model.

<a id="dockinglayout-rail"></a>

#### `pub fn rail(mut self, rail: DockRail) -> Self`

Configure a side's activity rail (item size, top/bottom slots, overflow
trigger). The side still needs `DockingModel::set_side_rail` to put it
in Rail presentation; this only styles the rail. See `DockRail`.

<a id="dockinglayout-rails"></a>

#### `pub fn rails(self, rails: impl IntoIterator<Item = DockRail>) -> Self`

Configure several sides' activity rails from an iterator.

The loop form of `rail`. Each rail carries its own side, so
a later entry for a side already configured replaces it.

<a id="dockinglayout-center"></a>

#### `pub fn center(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self`

Set the always-present centre content (the app's main area).

<a id="dockinglayout-policy"></a>

#### `pub fn policy(self, policy: DockPolicy) -> Self`

Lock down end-user layout edits (sugar for `DockingModel::set_policy`).
See `DockPolicy`.

<a id="dockinglayout-disable_side"></a>

#### `pub fn disable_side(self, side: DockSide) -> Self`

Disable a side (sugar for `DockingModel::set_side_enabled``(side, false)`):
it renders nothing, reserves no space, and rejects docks.

<a id="dockinglayout-dock"></a>

#### `pub fn dock(self, dock: DockWidget) -> Self`

Declare a dock widget (its content factory + chrome metadata). The
dock is registered immediately, so the app may set the initial layout
on the model (`open_dock` / `import_state`) before mounting.

<a id="dockinglayout-docks"></a>

#### `pub fn docks(self, docks: impl IntoIterator<Item = DockWidget>) -> Self`

Declare several dock widgets from an iterator, in order.

The loop form of `dock`, and the usual one once an app has
more than a couple of panels to register.

<a id="dockrailslot"></a>

## `pub type DockRailSlot`

Factory for a rail slot widget (rebuilt on each rail rebuild).

A slot that wants to match the rail's current item size binds
`DockingModel::rail_size_mode_signal`
— the rail rebuilds its slots whenever the size mode changes, so reading the
signal in the factory is enough to keep the slot in step.

```rust
pub type DockRailSlot = Rc<dyn Fn() -> Box<dyn Widget>>;
```

<a id="dockactionid"></a>

## `pub struct DockActionId`

Stable identity for a `DockAction`.

**Not** used for persistence — a rail action carries no user-mutable state,
so nothing about it is serialized (see `DockLayoutState`'s
"app-config is reconstructed each run" rule). It exists so the accessibility
tree and the automation bridge can address a given action stably across
runs; a fresh-per-run id would make every script that clicks a rail action
flaky.

```rust
pub struct DockActionId(u64);
```

### Methods

<a id="dockactionid-named"></a>

#### `pub const fn named(name: &str) -> Self`

Derive a stable id from a caller-chosen name — identical across runs,
processes and machines. Prefer this over `from_raw`:
it removes the hand-picked-`u64`-literal collision hazard entirely.

`const` so ids can be declared as module-scope `const` items, the same
way apps already declare their `DockWidgetId`s.

```
# use teksilo_widgets::docking::DockActionId;
const SETTINGS: DockActionId = DockActionId::named("app.settings");
assert_eq!(SETTINGS, DockActionId::named("app.settings"));
assert_ne!(SETTINGS, DockActionId::named("app.about"));
```

<a id="dockactionid-from_raw"></a>

#### `pub const fn from_raw(v: u64) -> Self`

Wrap a raw value. Prefer `named`.

<a id="dockactionid-raw"></a>

#### `pub const fn raw(self) -> u64`

<a id="dockactionplacement"></a>

## `pub enum DockActionPlacement`

Where a `DockAction` sits along the rail's column.

```rust
pub enum DockActionPlacement { /* variants */ }
```

### Variants

- **`Start`** — Before the first activity item, in the flowing cluster.
- **`End`** — After the last activity item **and after the overflow trigger**, still in the flowing cluster — the group grows downward with the tabs.
- **`Pinned`** — Past the flexible spacer, anchored to the rail's far edge regardless of how many activities exist — VS Code's Accounts / Manage-gear cluster. Where a Settings gear belongs.

<a id="dockaction"></a>

## `pub struct DockAction`

A **dockless command button** in the activity rail: it looks and behaves
like an activity item, but opens no panel — activating it just runs a
closure.

Declared on `DockRail::action` rather than on
`DockingModel` because nothing about an action is
user-mutable, which is `state.rs`'s own test for what is not persisted —
so (like the rail's slots) it is per-view app config, reconstructed each
run. A rail action is deliberately **more
restricted** than a real activity: it is never draggable, never hidable, has
no "Move to" menu, and is never overflow-parked — it is reserved space. That
matches every surveyed precedent (VS Code's fixed Accounts / Manage cluster;
IntelliJ's stripe, whose only non-tool-window button is IDE-owned chrome).

```ignore
DockRail::new(DockSide::Leading).action(
    DockAction::new(
        DockActionId::named("app.settings"),
        lit!("Settings"),
        || IconWidget::gear(),
        |ctx| ctx.send_intent(Intent::new("app.settings")),
    )
    .placement(DockActionPlacement::Pinned),
)
```

```rust
pub struct DockAction { /* fields */ }
```

### Methods

<a id="dockaction-new"></a>

#### `pub fn new( id: DockActionId, label: impl Into<LocalizedString>, icon: impl Fn() -> IconWidget + 'static, on_activate: impl Fn(&mut EventContext) + 'static, ) -> Self`

Declare a rail action. Defaults to `DockActionPlacement::End`,
enabled, untoggled, with the label as its hover tooltip.

<a id="dockaction-placement"></a>

#### `pub fn placement(mut self, placement: DockActionPlacement) -> Self`

Where the action sits along the rail. See `DockActionPlacement`.

<a id="dockaction-tooltip"></a>

#### `pub fn tooltip(mut self, tooltip: impl Into<LocalizedString>) -> Self`

Override the hover tooltip (defaults to the label). Ignored in
`Icon + Label` rail mode, which paints the label inline instead.

<a id="dockaction-enabled"></a>

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Enable / disable the action. Accepts a `bool` or a `Signal<bool>`.

<a id="dockaction-toggled"></a>

#### `pub fn toggled(mut self, state: Signal<bool>) -> Self`

Paint the selected surface while `state` is `true` — the same
highlight an open activity gets. **Reflect-only**: activating the
action does not write `state`; `on_activate` must.

<a id="dockaction-id"></a>

#### `pub fn id(&self) -> DockActionId`

The action's id.

<a id="dockrail"></a>

## `pub struct DockRail`

App-facing configuration for a side's activity rail (Rail presentation).

Pass to `DockingLayout::rail`. All knobs are
optional; an unconfigured rail uses `IconButtonSize::Large` items, no
slots, and no overflow affordance (items just clip if the side is too
short).

```rust
pub struct DockRail { /* fields */ }
```

### Methods

<a id="dockrail-new"></a>

#### `pub fn new(side: DockSide) -> Self`

Configure the rail for `side`.

<a id="dockrail-size"></a>

#### `pub fn size(mut self, size: IconButtonSize) -> Self`

Pick one size for every rail item (`IconButtonSize::Compact` …
`Hero`). Default `IconButtonSize::Large`.

<a id="dockrail-background"></a>

#### `pub fn background(mut self, color: impl Into<ColorProp>) -> Self`

Override the rail strip's background. Accepts `Color`, a
`SurfaceRole`, or a `Signal<Color>`.
Default (unset) is `SurfaceRole::Sunken`.

<a id="dockrail-divider"></a>

#### `pub fn divider(mut self) -> Self`

Draw a 1 dp divider line between the rail and the side's content, on
the rail's content-facing edge (RTL-aware). Uses `BorderRole::Divider`.
Off by default. See `divider_color` for a custom
colour.

<a id="dockrail-divider_color"></a>

#### `pub fn divider_color(mut self, color: impl Into<ColorProp>) -> Self`

Like `divider`, but with an explicit colour. Accepts
`Color`, a `BorderRole`, or a
`Signal<Color>`.

<a id="dockrail-top_slot"></a>

#### `pub fn top_slot<W: Widget + 'static>(mut self, f: impl Fn() -> W + 'static) -> Self`

Widget pinned **above** the items (e.g. a logo / hamburger). To track the
rail's item size, bind
`DockingModel::rail_size_mode_signal`
inside the factory.

<a id="dockrail-bottom_slot"></a>

#### `pub fn bottom_slot<W: Widget + 'static>(mut self, f: impl Fn() -> W + 'static) -> Self`

Widget pinned at the **bottom** of the rail (e.g. settings / account). To
track the rail's item size, bind
`DockingModel::rail_size_mode_signal`
inside the factory.

<a id="dockrail-leading_slot"></a>

#### `pub fn leading_slot<W: Widget + 'static>(mut self, f: impl Fn() -> W + 'static) -> Self`

Widget pinned at the **start** of this side's **Strip**-presentation tab
bar (via `TabWidget::bar_leading_slot`).
The Rail-presentation counterpart is `top_slot`.

**Weaker contract than `top_slot`.** `top_slot`/`bottom_slot` sit on the
`DockActivityBar`, which is built whenever the side has a rail — they
survive the side being collapsed. `leading_slot`/`trailing_slot` sit
inside the side's `TabWidget`, which lives within the collapsing
`SideClipPane`, so they disappear with the content when the side is
hidden. If your content must survive a hidden side, use Rail
presentation, or host it outside the docking system.

<a id="dockrail-trailing_slot"></a>

#### `pub fn trailing_slot<W: Widget + 'static>(mut self, f: impl Fn() -> W + 'static) -> Self`

Widget pinned at the **end** of this side's **Strip**-presentation tab
bar. Composed *before* the side's own "hidden activities" hamburger when
both are present, so neither is dropped. See
`leading_slot` for the visibility contract.

<a id="dockrail-action"></a>

#### `pub fn action(mut self, action: DockAction) -> Self`

Append a **dockless command button** to this side's rail. Declaration
order is render order within a placement. See `DockAction`.

**Rail presentation only.** A side in
`TabPresentation::Strip` renders no
actions at all — and `set_side_rail`
can flip presentation at runtime, so a side that flips Rail → Strip drops
its whole action cluster. If that is reachable in your app, mirror the
cluster with `trailing_slot`, which the same
`DockRail` can carry alongside its actions.

<a id="dockrail-actions"></a>

#### `pub fn actions(self, actions: impl IntoIterator<Item = DockAction>) -> Self`

Append several dockless command buttons from an iterator, in order.

The loop form of `action`, for a rail whose command
cluster comes from data. The same duplicate-id `debug_assert!` applies to
every action in the iterator.

<a id="dockrail-overflow_icon"></a>

#### `pub fn overflow_icon(mut self, f: impl Fn() -> IconWidget + 'static) -> Self`

Choose the glyph for the overflow trigger — the item shown (in place of
the surplus items) when they don't all fit. Tapping it opens a popover
list of the overflowed entries.

<a id="dockside"></a>

## `pub enum DockSide`

One of the four dockable sides. `Leading`/`Trailing` are
writing-direction-relative (mirrored under RTL by the caller); `Top`/
`Bottom` never mirror.

```rust
pub enum DockSide { /* variants */ }
```

### Variants

- **`Leading`** — Left in LTR, right in RTL.
- **`Trailing`** — Right in LTR, left in RTL.
- **`Top`**
- **`Bottom`**

### Methods

<a id="dockside-all"></a>

#### `pub const ALL: [DockSide;`

All four sides, in a stable order.

<a id="dockside-is_horizontal_axis"></a>

#### `pub fn is_horizontal_axis(self) -> bool`

True for the vertical columns (leading / trailing), whose long axis
is vertical — they stack their dock content top-to-bottom.

<a id="dockcorner"></a>

## `pub enum DockCorner`

One of the four corners of the container. Each corner is owned by exactly
one of its two adjacent sides (Qt `setCorner`).

```rust
pub enum DockCorner { /* variants */ }
```

### Variants

- **`TopLeading`**
- **`TopTrailing`**
- **`BottomLeading`**
- **`BottomTrailing`**

### Methods

<a id="dockcorner-all"></a>

#### `pub const ALL: [DockCorner;`

All four corners.

<a id="dockcorner-adjacent_sides"></a>

#### `pub fn adjacent_sides(self) -> (DockSide, DockSide)`

The two sides adjacent to this corner: `(horizontal side, vertical
side)` — i.e. `(Leading|Trailing, Top|Bottom)`.

<a id="cornerowners"></a>

## `pub struct CornerOwners`

Which side owns each corner. Default = the classic IDE shell where the
top and bottom bars span the full width and the leading / trailing columns
occupy only the middle band.

```rust
pub struct CornerOwners { /* fields */ }
```

### Methods

<a id="cornerowners-owner"></a>

#### `pub fn owner(&self, corner: DockCorner) -> DockSide`

<a id="cornerowners-set"></a>

#### `pub fn set(&mut self, corner: DockCorner, owner: DockSide)`

<a id="dockwidgetid"></a>

## `pub struct DockWidgetId`

Process-unique identity for a registered dock widget (the atomic unit).

```rust
pub struct DockWidgetId(pub u64);
```

### Methods

<a id="dockwidgetid-fresh"></a>

#### `pub fn fresh() -> Self`

Mint a fresh, process-unique id.

<a id="dockwidgetid-from_raw"></a>

#### `pub fn from_raw(v: u64) -> Self`

<a id="dockwidgetid-raw"></a>

#### `pub fn raw(self) -> u64`

<a id="docktabid"></a>

## `pub struct DockTabId`

Process-unique identity for a dock tab (a tab of a side's TabWidget).

```rust
pub struct DockTabId(pub u64);
```

### Methods

<a id="docktabid-fresh"></a>

#### `pub fn fresh() -> Self`

<a id="docktabid-from_raw"></a>

#### `pub fn from_raw(v: u64) -> Self`

<a id="docktabid-raw"></a>

#### `pub fn raw(self) -> u64`

<a id="tabpresentation"></a>

## `pub enum TabPresentation`

How a side surfaces its tabs: an in-side strip (the TabWidget's own bar) or
an always-visible activity rail outboard of the collapsible content.

```rust
pub enum TabPresentation { /* variants */ }
```

### Variants

- **`Strip`** — In-side tab strip (always shown, even for a single tab).
- **`Rail`** — External always-visible activity rail; the in-side strip is suppressed.

<a id="dockopenmode"></a>

## `pub enum DockOpenMode`

Placement mode for a programmatically-opened dock.

```rust
pub enum DockOpenMode { /* variants */ }
```

### Variants

- **`Stack`** — Stack into the side's currently-selected tab (as an extra Splitter pane).
- **`NewTab`** — Create a brand-new tab holding just this dock.

<a id="dockopenlocation"></a>

## `pub struct DockOpenLocation`

Target for `DockingModel::open_dock` / `DockingModel::move_dock`.

```rust
pub struct DockOpenLocation { /* fields */ }
```

### Methods

<a id="dockopenlocation-side"></a>

#### `pub fn side(side: DockSide) -> Self`

Default placement on a side (stack into the active tab).

<a id="dockopenlocation-stack"></a>

#### `pub fn stack(mut self) -> Self`

Stack into the side's active tab.

<a id="dockopenlocation-new_tab"></a>

#### `pub fn new_tab(mut self) -> Self`

Open as a fresh tab.

<a id="dockrailitemsize"></a>

## `pub enum DockRailItemSize`

Activity-bar item size for a side's rail (context-menu "Activity bar size").

```rust
pub enum DockRailItemSize { /* variants */ }
```

### Variants

- **`Default`** — The rail's configured size (`DockRail::size`); icon only, title on hover.
- **`Compact`** — Compact items — the standard `IconButtonSize::Default` regardless of the rail's configured (larger) size; icon only, title on hover. Not the extra-small `Compact` button: a rail glyph is the activity's identifier and must stay legible.
- **`Labeled`** — Icon at the configured size **plus** a 90°-rotated title beneath it (the vertical-accordion look). The title shows inline, so no hover tooltip.

### Methods

<a id="dockrailitemsize-shows_label"></a>

#### `pub fn shows_label(self) -> bool`

Whether this mode paints the title inline (rotated) rather than only as a
hover tooltip.

<a id="docktabdisplay"></a>

## `pub enum DockTabDisplay`

How a side's dock tabs render (context-menu "Tab size").

```rust
pub enum DockTabDisplay { /* variants */ }
```

### Variants

- **`Text`** — Title text only (the default).
- **`Icon`** — The dock's icon only (falls back to the title initial if it has none).
- **`IconText`** — Icon + title.

### Methods

<a id="docktabdisplay-shows_icon"></a>

#### `pub fn shows_icon(self) -> bool`

Whether this mode shows the icon glyph.

<a id="docktabdisplay-shows_text"></a>

#### `pub fn shows_text(self) -> bool`

Whether this mode shows the title text.

<a id="dockpolicy"></a>

## `pub struct DockPolicy`

App-declared policy that **locks down end-user layout edits** on a
`DockingLayout`. Each flag removes a *user
affordance* only — the programmatic `DockingModel` API (a "Toggle panel"
button, `open_dock`, `set_tab_hidden`, …) keeps working regardless, so the
app can still drive the layout it has locked for the user.

App-declared each run (like `rail_thickness` / `min_size` / `DockRail`)
— **not** persisted in `DockLayoutState`. Set it with
`DockingModel::set_policy`. Default = everything allowed; `DockPolicy::locked`
= everything forbidden.

```rust
pub struct DockPolicy { /* fields */ }
```

### Methods

<a id="dockpolicy-locked"></a>

#### `pub fn locked() -> Self`

A fully **locked** layout — no user drag, no collapse, no activity hide.
The app's programmatic API still drives it.

<a id="dockingmodel"></a>

## `pub struct DockingModel`

The shared docking-layout model. `Clone` = share-by-handle.

```rust
pub struct DockingModel(Rc<RefCell<Inner>>);
```

### Methods

<a id="dockingmodel-new"></a>

#### `pub fn new() -> Self`

A fresh model: four empty, hidden sides and the default corner owners.

<a id="dockingmodel-version"></a>

#### `pub fn version(&self) -> Signal<u64>`

Structural version — bump on tab / pane / section / side add-remove.
The widget binds this at `BindingLevel::Rebuild`.

<a id="dockingmodel-geometry_version"></a>

#### `pub fn geometry_version(&self) -> Signal<u64>`

Geometry version — bump on side size / visibility / corner change.
The widget binds this at `BindingLevel::Relayout`.

<a id="dockingmodel-consume_animate_flag"></a>

#### `pub fn consume_animate_flag(&self) -> bool`

Read-and-reset the "animate the next side show/hide" latch.

<a id="dockingmodel-is_registered"></a>

#### `pub fn is_registered(&self, id: DockWidgetId) -> bool`

Whether a dock id is known (its content factory + meta are registered).

<a id="dockingmodel-set_side_rail"></a>

#### `pub fn set_side_rail(&self, side: DockSide, thickness: f32)`

Set a side's activity-rail thickness and presentation. A non-zero rail
switches the side to `TabPresentation::Rail`; the in-side strip is
then suppressed.

<a id="dockingmodel-set_side_size"></a>

#### `pub fn set_side_size(&self, side: DockSide, size: f32)`

Set a side's stored content size (px). Relayout only (no rebuild).

<a id="dockingmodel-set_side_min_size"></a>

#### `pub fn set_side_min_size(&self, side: DockSide, min: f32)`

Set a side's minimum content size (px).

<a id="dockingmodel-set_policy"></a>

#### `pub fn set_policy(&self, policy: DockPolicy)`

Set the app's `DockPolicy` — locks down end-user layout edits (the
programmatic API keeps working). Structural → rebuild.

<a id="dockingmodel-policy"></a>

#### `pub fn policy(&self) -> DockPolicy`

The app's current `DockPolicy` (cheap `Copy`; read by the widgets in
`build()` to gate their user affordances).

<a id="dockingmodel-set_side_enabled"></a>

#### `pub fn set_side_enabled(&self, side: DockSide, enabled: bool)`

Enable / disable a whole side. A disabled side renders nothing, reserves
no space, is not a drop target, and rejects placement / moves to it; its
docks stay in the model and reappear when re-enabled. Structural →
rebuild.

<a id="dockingmodel-is_side_enabled"></a>

#### `pub fn is_side_enabled(&self, side: DockSide) -> bool`

Whether a side is enabled (default `true`).

<a id="dockingmodel-set_side_visible"></a>

#### `pub fn set_side_visible(&self, side: DockSide, visible: bool)`

Show / hide a whole side (animated).

<a id="dockingmodel-set_side_visible_immediate"></a>

#### `pub fn set_side_visible_immediate(&self, side: DockSide, visible: bool)`

Show / hide a side immediately (no animation — drag-driven).

<a id="dockingmodel-toggle_side_visible"></a>

#### `pub fn toggle_side_visible(&self, side: DockSide)`

Toggle a side's visibility (animated).

<a id="dockingmodel-select_tab"></a>

#### `pub fn select_tab(&self, side: DockSide, tab_idx: usize)`

Select the active tab of a side. Repaint only (the Switcher swaps via
its bound `selected_tab` signal — no rebuild, no relayout).

<a id="dockingmodel-select_tab_by_id"></a>

#### `pub fn select_tab_by_id(&self, side: DockSide, tab_id: DockTabId)`

Select a side's active tab by id (position-independent — used by the
rail / strip, whose visible order may skip hidden tabs).

<a id="dockingmodel-set_tab_hidden"></a>

#### `pub fn set_tab_hidden(&self, tab_id: DockTabId, hidden: bool)`

Hide / show one activity (tab). A hidden activity stays registered (so
it remains listable + restorable) but is dropped from the rail and tab
strip. Hiding the selected tab moves the selection to the nearest still-
visible tab. Structural → rebuild.

<a id="dockingmodel-is_tab_hidden"></a>

#### `pub fn is_tab_hidden(&self, tab_id: DockTabId) -> bool`

Whether an activity (tab) is currently hidden.

<a id="dockingmodel-side_rail_size"></a>

#### `pub fn side_rail_size(&self, side: DockSide) -> DockRailItemSize`

Current activity-bar item size for a side.

<a id="dockingmodel-set_side_rail_size"></a>

#### `pub fn set_side_rail_size(&self, side: DockSide, size: DockRailItemSize)`

Set a side's activity-bar item size (reactive → the rail rebuilds).

<a id="dockingmodel-rail_size_mode_signal"></a>

#### `pub fn rail_size_mode_signal(&self, side: DockSide) -> Signal<DockRailItemSize>`

Reactive activity-bar **size mode** for a side — fires whenever the user
switches Default / Compact / Icon + Label (via the context menu or
`set_side_rail_size`). Bind it to adapt any
external widget — a rail's slotted controls, an app toolbar — to the
rail's current item size. (The rail rebuilds its slots on every change,
so a slot factory that reads this signal stays in step.)

<a id="dockingmodel-side_tab_display"></a>

#### `pub fn side_tab_display(&self, side: DockSide) -> DockTabDisplay`

Current dock-tab display mode for a side.

<a id="dockingmodel-set_side_tab_display"></a>

#### `pub fn set_side_tab_display(&self, side: DockSide, display: DockTabDisplay)`

Set a side's dock-tab display mode (reactive → the strip rebuilds).

<a id="dockingmodel-set_corner"></a>

#### `pub fn set_corner(&self, corner: DockCorner, owner: DockSide)`

Set the owner of a corner (must be one of its two adjacent sides).

<a id="dockingmodel-open_dock"></a>

#### `pub fn open_dock(&self, id: DockWidgetId, loc: DockOpenLocation)`

Open (or move) a dock onto a side. Already-open docks are relocated
(never duplicated).

<a id="dockingmodel-promote_to_tab"></a>

#### `pub fn promote_to_tab(&self, id: DockWidgetId, side: DockSide, at_tab: usize)`

Drag a dock out into its own new tab on `side`, inserted at `at_tab`.

<a id="dockingmodel-split_into_tab"></a>

#### `pub fn split_into_tab( &self, id: DockWidgetId, side: DockSide, tab_idx: usize, pane_idx: usize, before: bool, )`

Drop a dock into an existing tab's Splitter as a new `Single` pane,
before (`before = true`) or after the pane at `pane_idx`.

<a id="dockingmodel-stack_into_tab"></a>

#### `pub fn stack_into_tab(&self, id: DockWidgetId, side: DockSide, tab_idx: usize)`

Drop a dock into a tab as a new Splitter pane appended after its
existing panes (the "centre" drop — join this group without choosing a
split direction). Each pane is its own Accordion.

<a id="dockingmodel-move_dock"></a>

#### `pub fn move_dock(&self, id: DockWidgetId, loc: DockOpenLocation)`

Move a dock to another location (close + open in one notify).

<a id="dockingmodel-close_tab"></a>

#### `pub fn close_tab(&self, tab_id: DockTabId)`

Close a whole tab (and every dock it holds).

<a id="dockingmodel-move_tab"></a>

#### `pub fn move_tab(&self, tab_id: DockTabId, target_side: DockSide, at_tab: usize)`

Move a whole tab (its arrangement + every dock + selection) to another
side, re-deriving the Splitter orientation. Inserted at `at_tab`.

<a id="dockingmodel-close_dock"></a>

#### `pub fn close_dock(&self, id: DockWidgetId)`

Close (remove) a dock from the layout.

<a id="dockingmodel-toggle_dock"></a>

#### `pub fn toggle_dock(&self, id: DockWidgetId)`

Toggle a dock: close it if open, else open it on its default location.

<a id="dockingmodel-reveal_dock"></a>

#### `pub fn reveal_dock(&self, id: DockWidgetId)`

Reveal a dock: ensure it is open, show + select its side / tab.

<a id="dockingmodel-is_dock_open"></a>

#### `pub fn is_dock_open(&self, id: DockWidgetId) -> bool`

<a id="dockingmodel-dock_location"></a>

#### `pub fn dock_location(&self, id: DockWidgetId) -> Option<DockLoc>`

<a id="dockingmodel-dock_open_signal"></a>

#### `pub fn dock_open_signal(&self, id: DockWidgetId) -> Signal<bool>`

A reactive `true`-while-open signal for an external rail / toolbar.

<a id="dockingmodel-is_side_visible"></a>

#### `pub fn is_side_visible(&self, side: DockSide) -> bool`

<a id="dockingmodel-side_visible_signal"></a>

#### `pub fn side_visible_signal(&self, side: DockSide) -> Signal<bool>`

<a id="dockingmodel-side_selected_tab_signal"></a>

#### `pub fn side_selected_tab_signal(&self, side: DockSide) -> Signal<usize>`

<a id="dockingmodel-side_selected_tab"></a>

#### `pub fn side_selected_tab(&self, side: DockSide) -> usize`

<a id="dockingmodel-side_presentation"></a>

#### `pub fn side_presentation(&self, side: DockSide) -> TabPresentation`

<a id="dockingmodel-side_size"></a>

#### `pub fn side_size(&self, side: DockSide) -> f32`

<a id="dockingmodel-side_min_size"></a>

#### `pub fn side_min_size(&self, side: DockSide) -> f32`

<a id="dockingmodel-side_rail_thickness"></a>

#### `pub fn side_rail_thickness(&self, side: DockSide) -> f32`

<a id="dockingmodel-side_has_rail"></a>

#### `pub fn side_has_rail(&self, side: DockSide) -> bool`

<a id="dockingmodel-corner_owner"></a>

#### `pub fn corner_owner(&self, corner: DockCorner) -> DockSide`

<a id="dockingmodel-tab_count"></a>

#### `pub fn tab_count(&self, side: DockSide) -> usize`

<a id="dockingmodel-tab_id_at"></a>

#### `pub fn tab_id_at(&self, side: DockSide, idx: usize) -> Option<DockTabId>`

The id of the tab at `idx` in a side's full tab list. The live inverse
of `select_tab_by_id` — the strip's
index → id selection sync uses it so both directions resolve against the
*current* order and agree across a reorder (a build-time snapshot would
disagree and feed back unboundedly).

<a id="dockingmodel-set_tab_title"></a>

#### `pub fn set_tab_title(&self, tab_id: DockTabId, title: Option<LocalizedString>)`

Give an activity (tab) a stable, explicit name, independent of which dock
occupies pane 0 (e.g. a grouped "Source Control" activity holding a file
tree and a git pane). Pass `None` to clear it (the label then derives from
the primary dock again). App-config — reconstructed each run, like dock
titles; not persisted. Structural → rebuild.

<a id="dockingmodel-tab_title"></a>

#### `pub fn tab_title(&self, tab_id: DockTabId) -> Option<LocalizedString>`

The explicit title set on an activity (`None` when it derives from its
primary dock).

<a id="dockingmodel-activity_of"></a>

#### `pub fn activity_of(&self, dock_id: DockWidgetId) -> Option<DockTabId>`

The activity (tab) currently holding a dock — apps hold stable
`DockWidgetId`s, so this is the bridge to address the enclosing tab.

<a id="dockingmodel-set_dock_activity_title"></a>

#### `pub fn set_dock_activity_title( &self, dock_id: DockWidgetId, title: impl Into<LocalizedString>, )`

Sugar: name the activity that currently holds `dock_id`. The natural way
to title a grouped activity from app code that holds the dock id.

<a id="dockingmodel-enabled_move_targets"></a>

#### `pub fn enabled_move_targets(&self, from: DockSide) -> Vec<DockSide>`

The enabled sides a tab / dock on `from` can be relocated to (every side
except `from`, keeping only `is_side_enabled`).
The "Move to" menus iterate this so a disabled side is never offered as a
silently-rejected target.

<a id="dockingmodel-export_state"></a>

#### `pub fn export_state(&self) -> super::state::DockLayoutState`

Serialize the user-controllable layout state (sizes / visibility /
selections / arrangement structure / corners). App-config (rail
thickness, mins, content factories) is reconstructed each run.

<a id="dockingmodel-import_state"></a>

#### `pub fn import_state(&self, state: &super::state::DockLayoutState)`

Restore a previously-exported state. Unknown dock ids are dropped,
emptied panes / tabs pruned, selections clamped. Bumps `version`.

<a id="dockwidget"></a>

## `pub struct DockWidget`

App-facing declaration of a dock widget: identity, chrome metadata, and a
lazy content factory. Collect these on `DockingLayout::dock`.

```rust
pub struct DockWidget { /* fields */ }
```

### Methods

<a id="dockwidget-new"></a>

#### `pub fn new<W: Widget + 'static>( id: DockWidgetId, title: impl Into<LocalizedString>, factory: impl Fn(DockWidgetId) -> W + 'static, ) -> Self`

Declare a dock widget. `factory` builds its content the first time the
dock appears (and after it is closed and re-opened).

<a id="dockwidget-icon"></a>

#### `pub fn icon(mut self, f: impl Fn() -> IconWidget + 'static) -> Self`

Set the dock's tab / rail icon.

<a id="dockwidget-header_actions"></a>

#### `pub fn header_actions( mut self, f: impl Fn(DockWidgetId) -> Vec<ToolbarItem> + 'static, ) -> Self`

Attach a factory for the dock's **inline header actions** — a flat list
of `ToolbarAction`s shown before the `⋮` options button, the VS Code
"view actions" pattern ("New File", "Collapse All", …). Built on demand
each time the dock is placed into a header. The framework hosts them in a
`Toolbar`, so the actions gain **overflow** (when the header is tight,
the lowest-`priority` actions collapse into a
`⌄` menu) and the correct **axis** for free — a horizontal row on leading
/ trailing sides, a vertical column on the rotated top / bottom strip. The
actions appear in any header the dock has: the multi-pane `Accordion`
header always, and the sole-pane (bare) header when
`show_header(true)` is set.

Each item is a `ToolbarItem` — a collapsible
`ToolbarAction` via
`ToolbarItem::action`, or a pinned arbitrary widget (a `SplitButton`, a
search field, …) via `ToolbarItem::custom`.

```ignore
DockWidget::new(id, lit!("Explorer"), build).header_actions(|_| vec![
    ToolbarItem::action(ToolbarAction::new(lit!("New File"), new_icon).on_activate(..)),
    ToolbarItem::custom(CreateSplitButton::new(..)),
])
```

<a id="dockwidget-show_header"></a>

#### `pub fn show_header(mut self, show: bool) -> Self`

Give a **sole-pane** (bare) dock its own header bar (title + actions +
`⋮` options). Default `false`. The multi-pane Accordion header is always
present regardless; this only governs the bare case. Turn it on to get a
discoverable options button (and inline `header_actions`) on a dock that
is the only one on its side.

<a id="dockwidget-default_location"></a>

#### `pub fn default_location(mut self, loc: DockOpenLocation) -> Self`

The location used when the dock is opened via `toggle` / `reveal`
without an explicit target.

<a id="docklayoutstate"></a>

## `pub struct DockLayoutState`

The full serializable snapshot of a `DockingModel`.

```rust
pub struct DockLayoutState { /* fields */ }
```
