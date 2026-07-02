<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Splitter

N-pane split container with draggable, collapsible dividers.

`Splitter` arranges `N ≥ 2` panes along one axis (per `Orientation`)
with `N − 1` grabbable handles between them — the Qt `QSplitter`
model. All layout state (per-pane size / min / max / stretch /
collapsed) lives in a shared, cloneable `SplitterModel`; the app
holds a clone to read, mutate, persist, and import/export, while the
widget renders it and reacts to the model's `version` signal.

Strengths carried over from the old two-pane `SplitView`: anti-jump
drag, keyboard resize, `Role::Splitter` accessibility, per-pane content
clipping, RTL-correct horizontal layout. New: N panes, per-pane
stretch (container-resize policy), animated collapse with four triggers
(programmatic / double-click / drag-past-min snap / keyboard), a Tier-3
`SplitterStyle`, and serializable import/export. Intended as the
building block for a future `DockingLayout`.

```ignore
let model = SplitterModel::from_panes(vec![
    PaneDescriptor::new().size(220.0).min_size(160.0).stretch(0.0).collapsible(true),
    PaneDescriptor::new().stretch(1.0).min_size(320.0),
    PaneDescriptor::new().size(280.0).stretch(0.0).collapsible(true),
], Orientation::Horizontal);

Splitter::new(model.clone())
    .pane(sidebar).pane(editor).pane(inspector)
    .pane_label(0, tr!(sidebar()));
```

## Builder methods at a glance

`pane`, `pane_id`, `child`, `pane_label`, `style`, `enabled`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/splitter/index.html)

## `pub struct Splitter`

An N-pane resizable split container driven by a `SplitterModel`.

See the `module-level documentation` for a usage overview and
constructor patterns.

```rust
pub struct Splitter { /* fields */ }
```

### Methods

#### `pub fn new(model: SplitterModel) -> Self`

Create a `Splitter` bound to the given model. Panes must be appended
with `pane` in model order.

#### `pub fn pane(mut self, widget: impl Widget + 'static) -> Self`

Append a content pane (model order). Call once per pane; the count
must match `model.pane_count()`.

#### `pub fn pane_id(mut self, id: WidgetId) -> Self`

Append a pre-registered content pane by id.

#### `pub fn child(self, widget: impl Widget + 'static) -> Self`

`bati!` ergonomic alias for `pane`: a bare child in a
`Splitter { ... }` block lowers to `.child(...)`.

#### `pub fn pane_label(mut self, index: usize, label: impl Into<Prop<String>>) -> Self`

Set an accessible region name for pane `index` (locale-reactive).
Labeled panes become a named `Role::Group`; unlabeled panes stay
AT-transparent (their content represents itself).

#### `pub fn style(mut self, style: impl SplitterStyle) -> Self`

Override the active `SplitterStyle` for this instance only.

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Enable or disable handle dragging, statically or reactively. When
`false`, divider handles are rendered inert — the pane layout is
still valid but the user cannot resize panes.
