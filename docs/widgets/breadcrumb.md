<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Breadcrumb

Breadcrumb — a navigational trail with automatic overflow into a `…` menu.

`Breadcrumb` renders a horizontal row of labelled segments separated by
chevron glyphs, representing a hierarchical path (file system, settings
hierarchy, wizard steps, etc.). When the trail is too wide to fit its
container, middle segments are automatically collapsed into a `…` popover
menu — the root and the current (last) segment always stay visible,
matching Windows Explorer, macOS path bar, and web breadcrumb conventions.

## Building a trail

```rust
# use teksilo_widgets::{Breadcrumb, BreadcrumbItem};
# use teksilo_core::Intent;
# use teksilo_i18n::lit;
let _bc = Breadcrumb::new()
    .item(BreadcrumbItem::new(lit!("Home"))
        .on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.nav.home"))))
    .item(BreadcrumbItem::new(lit!("Projects"))
        .on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.nav.projects"))))
    .item(BreadcrumbItem::current(lit!("Teksilo")));
```

## Accessibility

The container uses `Role::Navigation`; each segment uses `Role::Link`.
The current crumb sets `aria-current="page"`. The decorative separator
chevrons are hidden from the AT tree. The `…` overflow button declares
`HasPopup::Menu`.

## Builder methods at a glance

`label`, `item`, `item_id`, `trailing_slot`, `trailing_slot_id`, `is_overflowing`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/breadcrumb/index.html)

## `pub const BREADCRUMB_ITEM_HEIGHT`

Minimum height of a single breadcrumb segment in logical pixels.

```rust
pub const BREADCRUMB_ITEM_HEIGHT: f32 = 20.0;
```

## `pub const BREADCRUMB_ITEM_PADDING_HORIZONTAL`

Horizontal inner padding of each segment pill in logical pixels.

```rust
pub const BREADCRUMB_ITEM_PADDING_HORIZONTAL: f32 = 6.0;
```

## `pub const BREADCRUMB_SEPARATOR_GAP`

Gap reserved for the chevron separator between adjacent segments.

```rust
pub const BREADCRUMB_SEPARATOR_GAP: f32 = 4.0;
```

## `pub const BREADCRUMB_CORNER_RADIUS`

Corner radius of the interactive segment hover/focus rectangle.

```rust
pub const BREADCRUMB_CORNER_RADIUS: f32 = 4.0;
```

## `pub struct BreadcrumbItem`

A single breadcrumb segment definition.

```rust
pub struct BreadcrumbItem { /* fields */ }
```

### Methods

#### `pub fn new(label: impl Into<LocalizedString>) -> Self`

Construct a non-current (navigable) breadcrumb segment.

#### `pub fn current(label: impl Into<LocalizedString>) -> Self`

Construct the current (last) breadcrumb segment, announced
with `aria-current="page"`. Current segments are never
collapsed into the overflow `…` menu.

#### `pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Closure invoked on activation.

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain single-line tooltip to this breadcrumb segment, shown
after a hover delay. Clears any previously set rich or composite tooltip.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip to this breadcrumb segment, looked up by registry
key. Clears any previously set plain or composite tooltip.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip to this breadcrumb segment from inline content.
Clears any previously set plain or composite tooltip.

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip (arbitrary widget tree) to this breadcrumb
segment. Clears any previously set plain or rich tooltip.

## `pub struct Breadcrumb`

A breadcrumb navigation row with **automatic overflow**: when the trail is
too wide, the middle crumbs collapse into a trailing-of-root `…` menu while
the root and the current (last) crumb stay visible — the standard breadcrumb
collapse (Windows Explorer / web breadcrumbs / macOS path bar).

```rust
pub struct Breadcrumb { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Construct an empty breadcrumb trail. Add segments with
`item` and `item_id`.

#### `pub fn label(mut self, text: impl Into<LocalizedString>) -> Self`

Accessible name for the `Navigation` landmark — distinguishes
this breadcrumb from other nav landmarks on the page
(e.g. "Files", "Settings"). Screen readers announce it as the
name of the landmark when it gains focus or is summoned.

#### `pub fn item(mut self, item: BreadcrumbItem) -> Self`

Append a `BreadcrumbItem` segment to the trail. Items are rendered
in insertion order, separated by chevron glyphs. Middle items (neither
root nor current) may be collapsed into the `…` overflow menu.

#### `pub fn item_id(mut self, id: WidgetId) -> Self`

Insert a pre-registered widget as a breadcrumb segment slot.
The caller is responsible for the segment's visual + interaction.
Note: a pre-registered crumb never collapses into the overflow menu
(the breadcrumb has no label/action to synthesize a menu row from) —
it is treated like the root/current crumbs as always-visible.

#### `pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self`

Append a trailing widget after all segments, pushed to the far edge
by an intervening `Spacer`. Common uses: a search icon, refresh button,
or current-path copy button. When a trailing slot is set, the breadcrumb
spans the full proposed width.

#### `pub fn trailing_slot_id(mut self, id: WidgetId) -> Self`

Same as `trailing_slot` but accepts a
pre-registered `WidgetId` instead of an inline widget.

#### `pub fn is_overflowing(&self) -> Signal<bool>`

Reactive signal that is `true` whenever any crumb is collapsed into the
overflow `…` menu — for adaptive chrome.
