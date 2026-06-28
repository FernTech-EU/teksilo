<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Toolbar

`Toolbar` — a command bar with automatic **overflow**.

Excess actions collapse into a trailing chevron (`⌄`) that opens a popover
menu, mirroring Qt's `QToolBar` extension button, macOS `NSToolbar`'s
overflow menu, and WinUI `CommandBar`. Synthesized API:

- **Actions** ([`ToolbarAction`]) — a command with a label, optional icon,
  tooltip, enabled state, optional toggle (checkable), an **overflow
  priority** (NSToolbar: lowest priority collapses first), and an
  **`always_overflow`** flag (WinUI secondary commands). Each action has a
  toolbar form (a `Button`) and a menu form (a `MenuItem`), so it renders
  correctly whether inline or in the overflow menu.
- **Pinned widgets** ([`ToolbarItem::custom`]) — arbitrary widgets (a search
  field, a `SegmentedControl`) that never collapse.
- **Collapsible widgets** — an arbitrary widget that *does* overflow, by
  supplying an overflow representation (NSToolbar `menuFormRepresentation` /
  Qt `QWidgetAction`): a **menu row** ([`ToolbarAction`]) via
  `ToolbarItem::custom(w).overflow_as(action)`
  (or [`ToolbarOverflow`] + [`ToolbarItem::collapsible`]; an icon-only
  control reuses its icon as the menu glyph), or a **live embedded widget**
  via `ToolbarItem::custom(w).overflow_widget(f)`
  (the factory rebuilds the control — e.g. a `ComboBox` bound to the same
  signal — inside the menu so it stays usable while collapsed). When the bar
  is tight the inline widget is hidden and its overflow form appears in the
  menu.
- **Separators** and **flexible space** (NSToolbar `flexibleSpace`).
- **Display mode** (icon+text / icon-only / text-only) and **orientation**.

Overflow is computed every layout pass from each item's intrinsic size
(measured even while collapsed, via
`LayoutContext::measure_intrinsic`),
so items reappear correctly as the bar widens — no stale-width glitches.

The chevron's drop-down is a real [`MenuList`] whose rows are gated by
[`MenuList::item_when`],
so it sizes compactly to the currently-collapsed rows, carries standard
menu chrome, takes focus when opened, and supports arrow / `Home` / `End` /
`Enter` keyboard navigation (skipping the hidden rows).

**Accessibility (ARIA toolbar pattern).** The bar emits `Role::Toolbar`
with its orientation and name. It is a single Tab stop with **roving
tab-index**: arrow keys move focus among the visible controls (and the
chevron), `Home`/`End` jump to the ends. The chevron announces
`HasPopup::Menu` and its expanded state; overflowed actions are dormant
(absent from the AT tree), represented instead by their menu items — so no
action is announced twice. Toggle actions carry `Toggled`.

```ignore
// on_activate requires an EventContext — use ignore.
use bastyde_widgets::toolbar::{Toolbar, ToolbarAction, ToolbarItem};
use bastyde_i18n::lit;
let _bar = Toolbar::new()
    .action(ToolbarAction::new(lit!("Save")).on_activate(|ctx| { /* ... */ }))
    .action(ToolbarAction::new(lit!("Undo")).priority(-1))
    .item(ToolbarItem::flexible_space());
```

## Builder methods at a glance

`item`, `action`, `child`, `add_child`, `orientation`, `display_mode`, `spacing`, `label`, `is_overflowing`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/toolbar/index.html)

## `pub const TOOLBAR_HEIGHT_DEFAULT`

Toolbar design tokens.

```rust
pub const TOOLBAR_HEIGHT_DEFAULT: f32 = 40.0;
```

## `pub const TOOLBAR_SPACING`

```rust
pub const TOOLBAR_SPACING: f32 = 4.0;
```

## `pub enum ToolbarDisplayMode`

How toolbar actions render their label and icon.

```rust
pub enum ToolbarDisplayMode { /* variants */ }
```

### Variants

- **`IconAndText`** — Icon (if any) beside the label. The default.
- **`IconOnly`** — Icon only; the label becomes the accessible name + tooltip.
- **`TextOnly`** — Label only; the icon is dropped.

## `pub enum ToolbarOrientation`

Layout axis of the toolbar.

```rust
pub enum ToolbarOrientation { /* variants */ }
```

### Variants

- **`Horizontal`** — Items flow left-to-right (default).
- **`Vertical`** — Items flow top-to-bottom.

## `pub struct ToolbarAction`

A toolbar command: a label plus optional icon/tooltip/toggle, an activation
handler, an overflow priority, and an `always_overflow` flag. Renders as a
`Button` inline and as a `MenuItem` in the overflow menu.

```rust
pub struct ToolbarAction { /* fields */ }
```

### Methods

#### `pub fn new(label: impl Into<LocalizedString>) -> Self`

A new action with the given (translatable) label and a no-op handler.

#### `pub fn icon(mut self, factory: impl Fn() -> IconWidget + 'static) -> Self`

Icon factory — called to build the icon for both the inline button and
the overflow menu item (`IconWidget` isn't `Clone`).

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Tooltip / accessible-name supplement (also the AT name in `IconOnly`).

#### `pub fn enabled(mut self, enabled: bool) -> Self`

Initial enabled state.

#### `pub fn on_activate(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Activation handler (tap / Enter / Space / AT click / menu activate).

#### `pub fn toggle(mut self, state: Signal<bool>) -> Self`

Make this a checkable (toggle) action bound to `state`. Inline it reads
as a pressed toggle button; in overflow as a checkmark menu item.

#### `pub fn priority(mut self, priority: i32) -> Self`

Overflow priority — actions with the **lowest** priority collapse into
the menu first (NSToolbar semantics). Default `0`.

#### `pub fn always_overflow(mut self) -> Self`

Always live in the overflow menu, never inline (WinUI secondary command).

## `pub struct ToolbarItem`

One slot in a [`Toolbar`].

```rust
pub struct ToolbarItem { /* fields */ }
```

### Methods

#### `pub fn action(action: ToolbarAction) -> Self`

A collapsible command.

#### `pub fn custom(widget: impl Widget + 'static) -> Self`

A pinned arbitrary widget (never collapses) — e.g. a search field. Make
it collapsible with `overflow_as`.

#### `pub fn custom_id(id: WidgetId) -> Self`

A pinned arbitrary widget by pre-registered id.

#### `pub fn collapsible(widget: impl Widget + ToolbarOverflow + 'static) -> Self`

A collapsible widget that supplies its own menu form via
[`ToolbarOverflow`]. When the bar is too narrow, the widget is hidden
and its `toolbar_menu_form()` appears in the overflow menu.

#### `pub fn overflow_as(mut self, menu_form: ToolbarAction) -> Self`

Make a `custom` widget collapsible with an explicit menu
**row** — the [`ToolbarAction`] shown when it overflows (NSToolbar
`menuFormRepresentation`). Best for controls whose menu form is a
single command; an icon-only inline control reuses its icon here as the
menu item's leading glyph (set it via [`ToolbarAction::icon`]).

#### `pub fn overflow_widget(mut self, factory: impl Fn() -> Box<dyn Widget> + 'static) -> Self`

Make a `custom` widget collapsible by embedding a **live
widget** in the overflow menu — the factory rebuilds the control (e.g.
a `ComboBox` bound to the same signal) so it stays fully interactive
while collapsed, instead of degrading to a one-shot menu row. Best for
stateful inputs (combo boxes, sliders) that have no meaningful single
"command" representation.

#### `pub fn separator() -> Self`

A separator line between groups.

#### `pub fn flexible_space() -> Self`

Flexible space that pushes the following items to the trailing edge
(NSToolbar `flexibleSpace`). Collapses to nothing when over-constrained.

## `pub struct Toolbar`

A command bar with automatic overflow. See the `module docs`.

```rust
pub struct Toolbar { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create an empty toolbar with the default orientation (horizontal) and
`IconAndText` display mode. Add commands with `action` or
layout items with `item`.

#### `pub fn item(mut self, item: ToolbarItem) -> Self`

Add an item (action, pinned widget, separator, flexible space).

#### `pub fn action(self, action: ToolbarAction) -> Self`

Sugar for `.item(ToolbarItem::action(a))`.

#### `pub fn child(self, widget: impl Widget + 'static) -> Self`

Add a pinned inline child widget (sugar for
`.item(ToolbarItem::custom(widget))`). Pinned widgets never collapse
into the overflow menu — use `action` for collapsible
commands.

#### `pub fn add_child(self, id: WidgetId) -> Self`

Add a pinned inline child by pre-registered id (sugar for
`.item(ToolbarItem::custom_id(id))`).

#### `pub fn orientation(mut self, orientation: ToolbarOrientation) -> Self`

Set the layout axis (default [`ToolbarOrientation::Horizontal`]).

#### `pub fn display_mode(mut self, mode: ToolbarDisplayMode) -> Self`

Set how inline actions render their label and icon (default
[`ToolbarDisplayMode::IconAndText`]).

#### `pub fn spacing(mut self, spacing: f32) -> Self`

Gap between consecutive toolbar items in logical pixels (default
[`TOOLBAR_SPACING`]).

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the accessible name (default: the localized "Toolbar").

#### `pub fn is_overflowing(&self) -> Signal<bool>`

Reactive signal that is `true` whenever any action is collapsed into the
overflow menu (WinUI `IsOverflowOpen`-adjacent introspection).
