<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# DockingLayout

`DockingLayout` — a VS Code-style dockable layout: a fixed centre slot
(the app's main content) surrounded by four collapsible, splittable,
draggable side regions (leading / trailing / top / bottom), backed by a
cloneable, serializable [`DockingModel`].

See `docs/docking.md` for the full reference. The structure is four
levels deep:

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

## Builder methods at a glance

`rail`, `center`, `policy`, `disable_side`, `center_id`, `dock`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/docking/index.html)

## `pub struct DockingLayout`

The docking layout widget. See the module docs and `docs/docking.md`.

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

#### `pub fn new(model: DockingModel) -> Self`

Create a docking layout over a model.

#### `pub fn rail(mut self, rail: DockRail) -> Self`

Configure a side's activity rail (item size, top/bottom slots, overflow
trigger). The side still needs [`DockingModel::set_side_rail`] to put it
in Rail presentation; this only styles the rail. See [`DockRail`].

#### `pub fn center(mut self, widget: impl Widget + 'static) -> Self`

Set the always-present centre content (the app's main area).

#### `pub fn policy(self, policy: DockPolicy) -> Self`

Lock down end-user layout edits (sugar for [`DockingModel::set_policy`]).
See [`DockPolicy`].

#### `pub fn disable_side(self, side: DockSide) -> Self`

Disable a side (sugar for [`DockingModel::set_side_enabled`]`(side, false)`):
it renders nothing, reserves no space, and rejects docks.

#### `pub fn center_id(mut self, id: WidgetId) -> Self`

Set the centre content by a pre-registered id.

#### `pub fn dock(self, dock: DockWidget) -> Self`

Declare a dock widget (its content factory + chrome metadata). The
dock is registered immediately, so the app may set the initial layout
on the model (`open_dock` / `import_state`) before mounting.
