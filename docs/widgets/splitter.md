<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Splitter

![Splitter preview](img/splitter.png)

N-pane split container with draggable, collapsible dividers.

## Public types

| Kind | Name |
| ---: | :--- |
| `struct` | [`Splitter`](#splitter) — An N-pane resizable split container driven by a `SplitterModel` |
| `struct` | [`PaneDescriptor`](#panedescriptor) — Per-pane configuration passed to `SplitterModel::from_panes` / `SplitterModel::insert_pane` |
| `struct` | [`PaneSnapshot`](#panesnapshot) — Immutable per-pane view handed to the pure `distribute` sizing function (the internal `splitter::distribute` engine) |
| `struct` | [`PaneState`](#panestate) — Persistable per-pane layout state |
| `struct` | [`SplitterState`](#splitterstate) — Full serializable snapshot of a `SplitterModel`'s sizes + collapsed flags |
| `struct` | [`SplitterModel`](#splittermodel) — A shared, cloneable handle to a splitter's layout state |

## Public functions

### `Splitter`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(model: SplitterModel)`](#splitter-new) |
| | **Builder methods** |
| `Self` | [`pane(widget: impl teksilo_core::IntoTeksiChild)`](#splitter-pane) |
| `Self` | [`panes(iter: impl IntoIterator<Item = impl teksilo_core::IntoTeksiChild>)`](#splitter-panes) |
| `Self` | [`child(widget: impl teksilo_core::IntoTeksiChild)`](#splitter-child) |
| `Self` | [`children(iter: impl IntoIterator<Item = impl teksilo_core::IntoTeksiChild>)`](#splitter-children) |
| `Self` | [`child_opt(widget: Option<impl teksilo_core::IntoTeksiChild>)`](#splitter-child_opt) |
| `Self` | [`pane_label(index: usize, label: impl Into<Prop<String>>)`](#splitter-pane_label) |
| `Self` | [`style(style: impl SplitterStyle)`](#splitter-style) |
| `Self` | [`enabled(enabled: impl Into<Prop<bool>>)`](#splitter-enabled) |

### `PaneDescriptor`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new()`](#panedescriptor-new) |
| | **Builder methods** |
| `Self` | [`size(size: f32)`](#panedescriptor-size) |
| `Self` | [`min_size(min: f32)`](#panedescriptor-min_size) |
| `Self` | [`max_size(max: f32)`](#panedescriptor-max_size) |
| `Self` | [`stretch(stretch: f32)`](#panedescriptor-stretch) |
| `Self` | [`collapsible(collapsible: bool)`](#panedescriptor-collapsible) |
| `Self` | [`collapsed(collapsed: bool)`](#panedescriptor-collapsed) |
| `Self` | [`collapsed_size(px: f32)`](#panedescriptor-collapsed_size) |
| `Self` | [`visible(visible: bool)`](#panedescriptor-visible) |

### `SplitterModel`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(n: usize, orientation: Orientation)`](#splittermodel-new) |
| `Self` | [`from_panes(panes: Vec<PaneDescriptor>, orientation: Orientation)`](#splittermodel-from_panes) |
| | **Methods** |
| `usize` | [`handle_count()`](#splittermodel-handle_count) |
|  | [`set_stored_size(index: usize, size: f32)`](#splittermodel-set_stored_size) |
|  | [`set_stored_size_silent(index: usize, size: f32)`](#splittermodel-set_stored_size_silent) |
|  | [`set_pair_sizes(index: usize, size_a: f32, size_b: f32)`](#splittermodel-set_pair_sizes) |
|  | [`set_min_size(index: usize, min: f32)`](#splittermodel-set_min_size) |
|  | [`set_max_size(index: usize, max: Option<f32>)`](#splittermodel-set_max_size) |
|  | [`set_stretch(index: usize, stretch: f32)`](#splittermodel-set_stretch) |
|  | [`set_collapsible(index: usize, collapsible: bool)`](#splittermodel-set_collapsible) |
|  | [`set_collapsed(index: usize, collapsed: bool)`](#splittermodel-set_collapsed) |
|  | [`set_collapsed_immediate(index: usize, collapsed: bool)`](#splittermodel-set_collapsed_immediate) |
|  | [`toggle_collapsed(index: usize)`](#splittermodel-toggle_collapsed) |
|  | [`set_collapsed_size(index: usize, px: f32)`](#splittermodel-set_collapsed_size) |
|  | [`set_pane_visible(index: usize, visible: bool)`](#splittermodel-set_pane_visible) |
| `bool` | [`is_pane_visible(index: usize)`](#splittermodel-is_pane_visible) |
| `bool` | [`consume_animate_flag()`](#splittermodel-consume_animate_flag) |
|  | [`insert_pane(index: usize, desc: PaneDescriptor)`](#splittermodel-insert_pane) |
|  | [`remove_pane(index: usize)`](#splittermodel-remove_pane) |
|  | [`replace_pane_desc(index: usize, desc: PaneDescriptor)`](#splittermodel-replace_pane_desc) |
|  | [`set_gutter_thickness(thickness: f32)`](#splittermodel-set_gutter_thickness) |
|  | [`set_snap_offset(offset: f32)`](#splittermodel-set_snap_offset) |
|  | [`set_keyboard_step_px(step: f32)`](#splittermodel-set_keyboard_step_px) |
|  | [`set_orientation(orientation: Orientation)`](#splittermodel-set_orientation) |
| `usize` | [`pane_count()`](#splittermodel-pane_count) |
| `f32` | [`stored_size(index: usize)`](#splittermodel-stored_size) |
| `f32` | [`min_size(index: usize)`](#splittermodel-min_size) |
| `Option<f32>` | [`max_size(index: usize)`](#splittermodel-max_size) |
| `f32` | [`stretch(index: usize)`](#splittermodel-stretch) |
| `bool` | [`is_collapsible(index: usize)`](#splittermodel-is_collapsible) |
| `f32` | [`collapsed_size(index: usize)`](#splittermodel-collapsed_size) |
| `bool` | [`is_collapsed(index: usize)`](#splittermodel-is_collapsed) |
| `Orientation` | [`orientation()`](#splittermodel-orientation) |
| `f32` | [`gutter_thickness()`](#splittermodel-gutter_thickness) |
| `f32` | [`snap_offset()`](#splittermodel-snap_offset) |
| `f32` | [`keyboard_step_px()`](#splittermodel-keyboard_step_px) |
| `Signal<u64>` | [`version()`](#splittermodel-version) |
| `Vec<PaneSnapshot>` | [`pane_snapshots()`](#splittermodel-pane_snapshots) |
| `SplitterState` | [`export_state()`](#splittermodel-export_state) |
| `bool` | [`import_state(state: &SplitterState)`](#splittermodel-import_state) |

## Detailed description

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
`SplitterStyle`, and serializable import/export. It is the building
block `DockingLayout` is built from.

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

## Density

The picture above is the widget at `TargetDensity::Compact`, the mouse-and-keyboard ladder. Below is the same subject on the same canvas with only the ladder changed, so what moves is the density and nothing else — where the subject no longer fits, that is what the denser targets cost it at that size. See `docs/density-and-targets.md`.

**Touch**

![Splitter at Touch density](img/splitter-touch.png)

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/splitter/index.html)

<a id="splitter"></a>

## `pub struct Splitter`

An N-pane resizable split container driven by a `SplitterModel`.

See the `module-level documentation` for a usage overview and
constructor patterns.

```rust
pub struct Splitter { /* fields */ }
```

### Methods

<a id="splitter-new"></a>

#### `pub fn new(model: SplitterModel) -> Self`

Create a `Splitter` bound to the given model. Panes must be appended
with `pane` in model order.

<a id="splitter-pane"></a>

#### `pub fn pane(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self`

Append a content pane (model order). Call once per pane; the count
must match `model.pane_count()`.

<a id="splitter-panes"></a>

#### `pub fn panes(self, iter: impl IntoIterator<Item = impl teksilo_core::IntoTeksiChild>) -> Self`

Append several content panes from an iterator, in model order.

The loop form of `pane`. The total pane count still has to
match `model.pane_count()`.

<a id="splitter-child"></a>

#### `pub fn child(self, widget: impl teksilo_core::IntoTeksiChild) -> Self`

`teksu!` ergonomic alias for `pane`: a bare child in a
`Splitter { ... }` block lowers to `.child(...)`.

<a id="splitter-children"></a>

#### `pub fn children( self, iter: impl IntoIterator<Item = impl teksilo_core::IntoTeksiChild>, ) -> Self`

`teksu!` ergonomic alias for `panes`: a `for` loop in a
`Splitter { ... }` block lowers to `.children(...)`.

<a id="splitter-child_opt"></a>

#### `pub fn child_opt(self, widget: Option<impl teksilo_core::IntoTeksiChild>) -> Self`

Attach `widget` when it is `Some`, and do nothing when it is `None`.

The conditional-child form. `teksu!`'s `if` without an `else` lowers to
this, and it is what `cond.then(|| w)` is for in a builder chain. `None`
adds no arena node, so nothing is laid out, painted, or published to the
accessibility tree, and a stack applies no spacing around it.

<a id="splitter-pane_label"></a>

#### `pub fn pane_label(mut self, index: usize, label: impl Into<Prop<String>>) -> Self`

Set an accessible region name for pane `index` (locale-reactive).
Labeled panes become a named `Role::Group`; unlabeled panes stay
AT-transparent (their content represents itself).

<a id="splitter-style"></a>

#### `pub fn style(mut self, style: impl SplitterStyle) -> Self`

Override the active `SplitterStyle` for this instance only.

<a id="splitter-enabled"></a>

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Enable or disable handle dragging. When `false`, divider handles are
rendered inert — the pane layout is still valid but the user cannot
resize panes. Read once at build time: a bound `Signal` is
snapshotted, not tracked.

<a id="panedescriptor"></a>

## `pub struct PaneDescriptor`

Per-pane configuration passed to `SplitterModel::from_panes` /
`SplitterModel::insert_pane`. Public fields + `Default` so it can
be built with struct-literal `..Default::default()` syntax, or via the
fluent setters.

```rust
pub struct PaneDescriptor { /* fields */ }
```

### Methods

<a id="panedescriptor-new"></a>

#### `pub fn new() -> Self`

<a id="panedescriptor-size"></a>

#### `pub fn size(mut self, size: f32) -> Self`

<a id="panedescriptor-min_size"></a>

#### `pub fn min_size(mut self, min: f32) -> Self`

<a id="panedescriptor-max_size"></a>

#### `pub fn max_size(mut self, max: f32) -> Self`

<a id="panedescriptor-stretch"></a>

#### `pub fn stretch(mut self, stretch: f32) -> Self`

<a id="panedescriptor-collapsible"></a>

#### `pub fn collapsible(mut self, collapsible: bool) -> Self`

<a id="panedescriptor-collapsed"></a>

#### `pub fn collapsed(mut self, collapsed: bool) -> Self`

<a id="panedescriptor-collapsed_size"></a>

#### `pub fn collapsed_size(mut self, px: f32) -> Self`

Size a collapsed pane folds down to (default `0`). See
`collapsed_size`.

<a id="panedescriptor-visible"></a>

#### `pub fn visible(mut self, visible: bool) -> Self`

<a id="panesnapshot"></a>

## `pub struct PaneSnapshot`

Immutable per-pane view handed to the pure `distribute` sizing
function (the internal `splitter::distribute` engine).

```rust
pub struct PaneSnapshot { /* fields */ }
```

<a id="panestate"></a>

## `pub struct PaneState`

Persistable per-pane layout state. Captures the user-controllable
values (size + collapsed); structural config (min/max/stretch/
collapsible) is app-declared and not serialized — Qt `saveState`
parity.

```rust
pub struct PaneState { /* fields */ }
```

<a id="splitterstate"></a>

## `pub struct SplitterState`

Full serializable snapshot of a `SplitterModel`'s sizes + collapsed
flags. Round-trips through `SplitterModel::export_state` /
`import_state` and implements
`Versioned` so apps persist it through
`SettingsFile<SplitterState>` + `Migrator` (TOML).

```rust
pub struct SplitterState { /* fields */ }
```

<a id="splittermodel"></a>

## `pub struct SplitterModel`

A shared, cloneable handle to a splitter's layout state. `Clone` =
share-by-handle (cheap `Rc` bump).

```rust
pub struct SplitterModel(Rc<RefCell<SplitterModelInner>>);
```

### Methods

<a id="splittermodel-new"></a>

#### `pub fn new(n: usize, orientation: Orientation) -> Self`

`n` equal-share panes (each `stretch = 1`, `min = SPLITTER_MIN_PANE_SIZE`).

<a id="splittermodel-from_panes"></a>

#### `pub fn from_panes(panes: Vec<PaneDescriptor>, orientation: Orientation) -> Self`

Build from explicit per-pane descriptors.

<a id="splittermodel-handle_count"></a>

#### `pub fn handle_count(&self) -> usize`

Number of distinct handles to this model (1 = unshared).

<a id="splittermodel-set_stored_size"></a>

#### `pub fn set_stored_size(&self, index: usize, size: f32)`

<a id="splittermodel-set_stored_size_silent"></a>

#### `pub fn set_stored_size_silent(&self, index: usize, size: f32)`

Like `set_stored_size` but **without** a version
bump — for writes made from inside a layout/effect pass that is already
relaying out (e.g. capturing the displayed size as the collapse
reference), where a bump would re-enter the effect.

<a id="splittermodel-set_pair_sizes"></a>

#### `pub fn set_pair_sizes(&self, index: usize, size_a: f32, size_b: f32)`

Set both sides of handle `index` (panes `index` and `index+1`) in
one mutation — a single version bump, so a drag produces exactly
one relayout per move.

<a id="splittermodel-set_min_size"></a>

#### `pub fn set_min_size(&self, index: usize, min: f32)`

<a id="splittermodel-set_max_size"></a>

#### `pub fn set_max_size(&self, index: usize, max: Option<f32>)`

<a id="splittermodel-set_stretch"></a>

#### `pub fn set_stretch(&self, index: usize, stretch: f32)`

<a id="splittermodel-set_collapsible"></a>

#### `pub fn set_collapsible(&self, index: usize, collapsible: bool)`

<a id="splittermodel-set_collapsed"></a>

#### `pub fn set_collapsed(&self, index: usize, collapsed: bool)`

Programmatically collapse/expand pane `index`, *animated*. Ignores
the `collapsible` flag (that flag only gates interactive triggers).

<a id="splittermodel-set_collapsed_immediate"></a>

#### `pub fn set_collapsed_immediate(&self, index: usize, collapsed: bool)`

Collapse/expand pane `index` *instantly* (no tween). Used by the
drag handlers — the pointer is already the motion.

<a id="splittermodel-toggle_collapsed"></a>

#### `pub fn toggle_collapsed(&self, index: usize)`

Toggle pane `index`'s collapsed state, animated.

<a id="splittermodel-set_collapsed_size"></a>

#### `pub fn set_collapsed_size(&self, index: usize, px: f32)`

Set the size pane `index` folds down to when collapsed (default `0`).
See `PaneDescriptor::collapsed_size`. No version bump on its own — it
only affects the next collapse.

<a id="splittermodel-set_pane_visible"></a>

#### `pub fn set_pane_visible(&self, index: usize, visible: bool)`

Show or hide pane `index` (animated). A hidden pane removes both the
pane and an adjacent gutter from the layout — it reads as absent,
unlike a collapsed pane (which keeps its grabbable gutter). The pane
must be pre-mounted in the `Splitter`; this is the reactive "add /
remove a pane from a fixed set" trick (no rebuild).

<a id="splittermodel-is_pane_visible"></a>

#### `pub fn is_pane_visible(&self, index: usize) -> bool`

<a id="splittermodel-consume_animate_flag"></a>

#### `pub fn consume_animate_flag(&self) -> bool`

Read-and-reset the "animate the next collapse change?" latch. The
widget's collapse effect calls this once per version bump; it
resets to `true` so the default (programmatic) path animates.

<a id="splittermodel-insert_pane"></a>

#### `pub fn insert_pane(&self, index: usize, desc: PaneDescriptor)`

Insert a pane at `index` (clamped to `[0, len]`). A `None`
`initial_size` takes the average of the existing panes' sizes; the
next layout rebalances. The app must rebuild the `Splitter` widget
to supply the new pane's content (retained-mode: changing a
container's child *set* is a rebuild; the model keeps the
persistent size/collapse state across it).

<a id="splittermodel-remove_pane"></a>

#### `pub fn remove_pane(&self, index: usize)`

Remove the pane at `index` (no-op if out of range). The app must
rebuild the `Splitter` widget to drop the corresponding content.

<a id="splittermodel-replace_pane_desc"></a>

#### `pub fn replace_pane_desc(&self, index: usize, desc: PaneDescriptor)`

Replace the metadata of pane `index` (keeps its current size unless
the descriptor specifies one).

<a id="splittermodel-set_gutter_thickness"></a>

#### `pub fn set_gutter_thickness(&self, thickness: f32)`

<a id="splittermodel-set_snap_offset"></a>

#### `pub fn set_snap_offset(&self, offset: f32)`

<a id="splittermodel-set_keyboard_step_px"></a>

#### `pub fn set_keyboard_step_px(&self, step: f32)`

<a id="splittermodel-set_orientation"></a>

#### `pub fn set_orientation(&self, orientation: Orientation)`

<a id="splittermodel-pane_count"></a>

#### `pub fn pane_count(&self) -> usize`

<a id="splittermodel-stored_size"></a>

#### `pub fn stored_size(&self, index: usize) -> f32`

<a id="splittermodel-min_size"></a>

#### `pub fn min_size(&self, index: usize) -> f32`

<a id="splittermodel-max_size"></a>

#### `pub fn max_size(&self, index: usize) -> Option<f32>`

<a id="splittermodel-stretch"></a>

#### `pub fn stretch(&self, index: usize) -> f32`

<a id="splittermodel-is_collapsible"></a>

#### `pub fn is_collapsible(&self, index: usize) -> bool`

<a id="splittermodel-collapsed_size"></a>

#### `pub fn collapsed_size(&self, index: usize) -> f32`

The size pane `index` folds to when collapsed (default `0`). See
`PaneDescriptor::collapsed_size`.

<a id="splittermodel-is_collapsed"></a>

#### `pub fn is_collapsed(&self, index: usize) -> bool`

<a id="splittermodel-orientation"></a>

#### `pub fn orientation(&self) -> Orientation`

<a id="splittermodel-gutter_thickness"></a>

#### `pub fn gutter_thickness(&self) -> f32`

<a id="splittermodel-snap_offset"></a>

#### `pub fn snap_offset(&self) -> f32`

<a id="splittermodel-keyboard_step_px"></a>

#### `pub fn keyboard_step_px(&self) -> f32`

<a id="splittermodel-version"></a>

#### `pub fn version(&self) -> Signal<u64>`

The reactive version signal. The `Splitter` widget binds this at
`BindingLevel::Relayout`.

<a id="splittermodel-pane_snapshots"></a>

#### `pub fn pane_snapshots(&self) -> Vec<PaneSnapshot>`

Immutable per-pane snapshot for the pure sizing engine.

<a id="splittermodel-export_state"></a>

#### `pub fn export_state(&self) -> SplitterState`

Snapshot the per-pane sizes + collapsed flags into a serializable
`SplitterState`.

<a id="splittermodel-import_state"></a>

#### `pub fn import_state(&self, state: &SplitterState) -> bool`

Restore sizes + collapsed flags from a `SplitterState`. Returns
`false` (and changes nothing) if the pane count doesn't match — the
structural config must be reconstructed first. Restoration is
instant (collapsed panes don't animate open on load).
