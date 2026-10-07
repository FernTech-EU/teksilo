<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TabBar

`TabBar<T>` — header strip driven by a data source.

## Public types

| Kind | Name |
| ---: | :--- |
| `const` | [`DEFAULT_MIN_TAB_WIDTH`](#default_min_tab_width) — Default min width for an unpinned tab |
| `fn` | [`default_min_tab_width`](#default_min_tab_width-2) — `DEFAULT_MIN_TAB_WIDTH` raised to the density's `target_size` (24 / 32 / 44 dp) |
| `const` | [`DEFAULT_MAX_TAB_WIDTH`](#default_max_tab_width) — Default max width for an unpinned tab |
| `const` | [`DEFAULT_TAB_SPACING`](#default_tab_spacing) — Default spacing between tab headers in the row |
| `fn` | [`default_tab_spacing`](#default_tab_spacing-2) — `DEFAULT_TAB_SPACING` scaled by the density's `spacing_factor` (1.00 / 1.15 / 1.30) |
| `const` | [`DEFAULT_BAR_SLOT_SPACING`](#default_bar_slot_spacing) — Default spacing between the bar's leading slot, scroll area, and trailing slot |
| `fn` | [`default_bar_slot_spacing`](#default_bar_slot_spacing-2) — `DEFAULT_BAR_SLOT_SPACING` scaled by the density's `spacing_factor` (1.00 / 1.15 / 1.30) |
| `const` | [`DEFAULT_PINNED_TAB_WIDTH`](#default_pinned_tab_width) — Default width (in dp) of a pinned tab — icon-only squares |
| `fn` | [`default_pinned_tab_width`](#default_pinned_tab_width-2) — `DEFAULT_PINNED_TAB_WIDTH` raised to the density's `target_size` (24 / 32 / 44 dp) |
| `struct` | [`TabBarDragData`](#tabbardragdata) — Drag payload published by a tab header when the user starts dragging it |
| `struct` | [`TabBar`](#tabbar) — A reactive header strip that pulls its tab list from a data source and writes the active tab into a shared `Signal<Option<TabId>>` |

## Public functions

### `TabBar`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`horizontal(model: ListModel<T>, delegate: TabDelegate<T>, selected_id: Signal<Option<TabId>>, id_of: impl Fn(usize, &T) -> TabId + 'static)`](#tabbar-horizontal) |
| `Self` | [`horizontal_from_source<S: ListDataSource<Item = T>>(source: S, delegate: TabDelegate<T>, selected_id: Signal<Option<TabId>>, id_of: impl Fn(usize, &T) -> TabId + 'static)`](#tabbar-horizontal_from_source) |
| `Self` | [`vertical(model: ListModel<T>, delegate: TabDelegate<T>, selected_id: Signal<Option<TabId>>, id_of: impl Fn(usize, &T) -> TabId + 'static)`](#tabbar-vertical) |
| `Self` | [`vertical_from_source<S: ListDataSource<Item = T>>(source: S, delegate: TabDelegate<T>, selected_id: Signal<Option<TabId>>, id_of: impl Fn(usize, &T) -> TabId + 'static)`](#tabbar-vertical_from_source) |
| | **Builder methods** |
| `Self` | [`tab_sizing(mode: TabSizing)`](#tabbar-tab_sizing) |
| `Self` | [`tab_display(mode: TabDisplayMode)`](#tabbar-tab_display) |
| `Self` | [`min_tab_width(dp: f32)`](#tabbar-min_tab_width) |
| `Self` | [`tab_bar_height(dp: f32)`](#tabbar-tab_bar_height) |
| `Self` | [`max_tab_width(dp: f32)`](#tabbar-max_tab_width) |
| `Self` | [`tab_spacing(dp: f32)`](#tabbar-tab_spacing) |
| `Self` | [`pinned_tab_width(dp: f32)`](#tabbar-pinned_tab_width) |
| `Self` | [`tab_background(color: impl Into<teksilo_core::color_prop::ColorProp>)`](#tabbar-tab_background) |
| `Self` | [`selected_tab_background(color: impl Into<teksilo_core::color_prop::ColorProp>)`](#tabbar-selected_tab_background) |
| `Self` | [`hover_tab_background(color: impl Into<teksilo_core::color_prop::ColorProp>)`](#tabbar-hover_tab_background) |
| `Self` | [`idle_tab_background(color: impl Into<teksilo_core::color_prop::ColorProp>)`](#tabbar-idle_tab_background) |
| `Self` | [`bar_background(color: impl Into<teksilo_core::color_prop::ColorProp>)`](#tabbar-bar_background) |
| `Self` | [`tab_dividers()`](#tabbar-tab_dividers) |
| `Self` | [`tab_divider_color(color: impl Into<teksilo_core::color_prop::ColorProp>)`](#tabbar-tab_divider_color) |
| `Self` | [`active_indicator(position: teksilo_core::styles::TabIndicatorPosition)`](#tabbar-active_indicator) |
| `Self` | [`selected_text_role(role: TextRole)`](#tabbar-selected_text_role) |
| `Self` | [`idle_text_role(role: TextRole)`](#tabbar-idle_text_role) |
| `Self` | [`style(style: impl teksilo_core::styles::TabStyle)`](#tabbar-style) |
| `Self` | [`on_pin_toggle(f: impl Fn(usize, bool, &mut EventContext) + 'static)`](#tabbar-on_pin_toggle) |
| `Self` | [`bar_leading_slot(w: impl teksilo_core::IntoTeksiChild)`](#tabbar-bar_leading_slot) |
| `Self` | [`bar_trailing_slot(w: impl teksilo_core::IntoTeksiChild)`](#tabbar-bar_trailing_slot) |
| `Self` | [`separator(on: bool)`](#tabbar-separator) |
| `Self` | [`show_scroll_arrows(on: bool)`](#tabbar-show_scroll_arrows) |
| `Self` | [`overflow_button(mode: TabOverflowButton)`](#tabbar-overflow_button) |
| `Self` | [`show_overflow_dropdown(on: bool)`](#tabbar-show_overflow_dropdown) |
| `Self` | [`vertical_wheel_scrolls_horizontally(on: bool)`](#tabbar-vertical_wheel_scrolls_horizontally) |
| `Self` | [`shift_wheel_scrolls_horizontally(on: bool)`](#tabbar-shift_wheel_scrolls_horizontally) |
| `Self` | [`on_close(f: impl Fn(usize, &mut EventContext) + 'static)`](#tabbar-on_close) |
| `Self` | [`reorderable(on: bool)`](#tabbar-reorderable) |
| `Self` | [`on_reorder(f: impl Fn(usize, usize, &mut EventContext) + 'static)`](#tabbar-on_reorder) |
| `Self` | [`accept_external_tabs(on: bool)`](#tabbar-accept_external_tabs) |
| `Self` | [`on_tab_received(f: impl Fn(T, usize, &mut EventContext) + 'static)`](#tabbar-on_tab_received) |
| `Self` | [`on_transfer_out(f: impl Fn(TabId, &mut EventContext) + 'static)`](#tabbar-on_transfer_out) |
| `Self` | [`on_external_drop(f: impl Fn(&DragPayload, usize, &mut EventContext) -> bool + 'static)`](#tabbar-on_external_drop) |

## Detailed description

Horizontal and vertical orientations, with shared / independent
sizing. Bar-leading and bar-trailing slots are wired. Overflow is
handled by a `ScrollArea` around the headers row, plus optional
scroll arrows and a "show all tabs" overflow dropdown (both on by
default); whichever tab is activated is scrolled back into view (see
`RevealState`). Closable tabs (with middle-click close),
drag-to-reorder with edge auto-scroll, and a leading icon-only
pinned-tab strip are all supported. Multi-line (multi-row) wrapping
is the one layout mode not yet implemented.

The data source is consumed via the `pub(crate)` `ListSource`
abstraction so callers can pass either a `ListModel<T>` (clonable,
mutable) or any external `ListDataSource<Item = T>` (a database
cursor, a virtual list, …) without TabBar having to carry a generic
source parameter.

#### Accessibility

The bar emits `Role::TabList` with an `aria-orientation`
reflecting whether it was built with `TabBar::horizontal` or
`TabBar::vertical`. When a page hosts more than one tab list,
give each one an accessible name via
`.access_label(tr!(tab_list_name()))`
so screen readers can distinguish them (ARIA APG recommendation).

```ignore
use teksilo_widgets::tab_widget::{TabBar, TabDelegate, TabId};
use teksilo_data::ListModel;
use teksilo_core::signal::Signal;

#[derive(Clone)]
struct Tab { id: TabId, title: String }

let model: ListModel<Tab> = ListModel::new();
let selected: Signal<Option<TabId>> = Signal::new(None);
let delegate = TabDelegate::new(|_i, t: &Tab| teksilo_i18n::lit!(t.title.clone()));
let _bar = TabBar::horizontal(model, delegate, selected, |_i, t| t.id)
    .reorderable(true)
    .tab_dividers();
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/tab_widget/index.html)

<a id="default_min_tab_width"></a>

## `pub const DEFAULT_MIN_TAB_WIDTH`

Default min width for an unpinned tab.

```rust
pub const DEFAULT_MIN_TAB_WIDTH: f32 = 96.0;
```

<a id="default_min_tab_width-2"></a>

## `pub fn default_min_tab_width(...)`

`DEFAULT_MIN_TAB_WIDTH` raised to the density's `target_size`
(24 / 32 / 44 dp). The identity at Compact.

```rust
pub fn default_min_tab_width(tokens: &InputTokens) -> f32;
```

<a id="default_max_tab_width"></a>

## `pub const DEFAULT_MAX_TAB_WIDTH`

Default max width for an unpinned tab.

```rust
pub const DEFAULT_MAX_TAB_WIDTH: f32 = 240.0;
```

<a id="default_tab_spacing"></a>

## `pub const DEFAULT_TAB_SPACING`

Default spacing between tab headers in the row. `0.0` so tabs sit
flush against each other (Firefox / Chrome convention) — adjacent
tab boundaries are visually separated by the per-tab borders, not
by an empty gap.

```rust
pub const DEFAULT_TAB_SPACING: f32 = 0.0;
```

<a id="default_tab_spacing-2"></a>

## `pub fn default_tab_spacing(...)`

`DEFAULT_TAB_SPACING` scaled by the density's `spacing_factor`
(1.00 / 1.15 / 1.30).

```rust
pub fn default_tab_spacing(tokens: &InputTokens) -> f32;
```

<a id="default_bar_slot_spacing"></a>

## `pub const DEFAULT_BAR_SLOT_SPACING`

Default spacing between the bar's leading slot, scroll area, and
trailing slot.

```rust
pub const DEFAULT_BAR_SLOT_SPACING: f32 = 8.0;
```

<a id="default_bar_slot_spacing-2"></a>

## `pub fn default_bar_slot_spacing(...)`

`DEFAULT_BAR_SLOT_SPACING` scaled by the density's `spacing_factor`
(1.00 / 1.15 / 1.30).

```rust
pub fn default_bar_slot_spacing(tokens: &InputTokens) -> f32;
```

<a id="default_pinned_tab_width"></a>

## `pub const DEFAULT_PINNED_TAB_WIDTH`

Default width (in dp) of a pinned tab — icon-only squares.

```rust
pub const DEFAULT_PINNED_TAB_WIDTH: f32 = 32.0;
```

<a id="default_pinned_tab_width-2"></a>

## `pub fn default_pinned_tab_width(...)`

`DEFAULT_PINNED_TAB_WIDTH` raised to the density's `target_size`
(24 / 32 / 44 dp). The identity at Compact.

```rust
pub fn default_pinned_tab_width(tokens: &InputTokens) -> f32;
```

<a id="tabbardragdata"></a>

## `pub struct TabBarDragData`

Drag payload published by a tab header when the user starts
dragging it.

Generic over the bar's item type `T` so a `TabBar<T>` only ever
downcasts (`get_typed::<TabBarDragData<T>>()`) a drag started by
another `TabBar<T>` — a drag from a `TabBar<OtherT>` simply never
matches, giving cross-bar transfer type-safety for free.

Two consumers:
- **Intra-bar reorder**: the bar's own `on_drop` matches
  `source_bar_id == self_id` and uses `source_index` to drive
  `move_item`. `item` is unused on this path (and may be `None`).
- **Cross-bar transfer**: a *different* bar that opted in via
  `accept_external_tabs` takes
  `item` by value and hands it to its
  `on_tab_received` callback. `item` is
  `Some` only when the source bar opted in *and* the per-tab
  transferable predicate allows it (static tabs are excluded).

```rust
pub struct TabBarDragData<T: 'static> { /* fields */ }
```

<a id="tabbar"></a>

## `pub struct TabBar`

A reactive header strip that pulls its tab list from a data source
and writes the active tab into a shared `Signal<Option<TabId>>`.

Selection is **id-based**: the bar holds a stable `TabId` per
item (extracted via the `id_of` closure passed to the constructor)
and the public `selected_id` signal is the source of truth across
reorders / removals / locale changes. Internal index-based work
(keyboard nav, scroll-to-active, click activation) reads a
**private** `selected_index` signal that the bar keeps in
bidirectional sync with `selected_id` at build time.

```rust
pub struct TabBar<T: 'static> { /* fields */ }
```

### Methods

<a id="tabbar-horizontal"></a>

#### `pub fn horizontal( model: ListModel<T>, delegate: TabDelegate<T>, selected_id: Signal<Option<TabId>>, id_of: impl Fn(usize, &T) -> TabId + 'static, ) -> Self`

Construct a horizontal tab bar from a `ListModel<T>`.
Default sizing is `TabSizing::Shared`.

`selected_id` is the id-based selection signal — written by
the bar on click / keyboard / drag-drop and observable by
callers. `id_of(index, &item)` extracts the stable `TabId`
from each model item.

<a id="tabbar-horizontal_from_source"></a>

#### `pub fn horizontal_from_source<S: ListDataSource<Item = T>>( source: S, delegate: TabDelegate<T>, selected_id: Signal<Option<TabId>>, id_of: impl Fn(usize, &T) -> TabId + 'static, ) -> Self`

Construct a horizontal tab bar from any `ListDataSource`.
Default sizing is `TabSizing::Shared`.

<a id="tabbar-vertical"></a>

#### `pub fn vertical( model: ListModel<T>, delegate: TabDelegate<T>, selected_id: Signal<Option<TabId>>, id_of: impl Fn(usize, &T) -> TabId + 'static, ) -> Self`

Construct a vertical tab bar from a `ListModel<T>`. Tabs
stack top-to-bottom as horizontal pills (icon + label + close
button arranged left-to-right within each pill). Default
sizing is `TabSizing::Shared` — uniform pill heights.

<a id="tabbar-vertical_from_source"></a>

#### `pub fn vertical_from_source<S: ListDataSource<Item = T>>( source: S, delegate: TabDelegate<T>, selected_id: Signal<Option<TabId>>, id_of: impl Fn(usize, &T) -> TabId + 'static, ) -> Self`

Construct a vertical tab bar from any `ListDataSource`.

<a id="tabbar-tab_sizing"></a>

#### `pub fn tab_sizing(mut self, mode: TabSizing) -> Self`

Override the per-tab sizing strategy. See `TabSizing`.

<a id="tabbar-tab_display"></a>

#### `pub fn tab_display(mut self, mode: TabDisplayMode) -> Self`

Choose what every tab shows — icon, label, or both. See
`TabDisplayMode`. Default `TabDisplayMode::Auto` (render each tab as
its `TabInfo` declares).

<a id="tabbar-min_tab_width"></a>

#### `pub fn min_tab_width(mut self, dp: f32) -> Self`

Minimum width (in dp) any unpinned tab will be drawn at.
Default: `DEFAULT_MIN_TAB_WIDTH`.

In **horizontal** orientation this clamps the **per-tab** width.
In **vertical** orientation every tab is forced to the bar's
cross-axis width, so the same knob defines the bar's minimum
width — the sidebar adapts to the widest piece of bar content
(tab labels or a slot widget) and never shrinks below this floor.
Vertical pill heights stay at the tab style's `editor_tab_height`
regardless of this knob.

Under `TabSizing::Fill` a **vertical** bar takes the width it is
offered outright, so this floor no longer applies to it; in a
**horizontal** `Fill` bar it still does (the tabs overflow into
scroll rather than squeeze below it).

<a id="tabbar-tab_bar_height"></a>

#### `pub fn tab_bar_height(mut self, dp: f32) -> Self`

Override the tab-strip cross-axis extent (the strip height for a
horizontal bar; the per-tab pill height for a vertical one). `None`
keeps the style's `editor_tab_height`. Use for a compact bar.

<a id="tabbar-max_tab_width"></a>

#### `pub fn max_tab_width(mut self, dp: f32) -> Self`

Maximum width (in dp) any unpinned tab will be drawn at — long
labels truncate with an ellipsis at this width.
Default: `DEFAULT_MAX_TAB_WIDTH`.

In **horizontal** orientation this clamps the **per-tab** width.
In **vertical** orientation it caps the whole sidebar's width —
see `min_tab_width` for the symmetric
adapt-to-content rule.

`TabSizing::Fill` ignores this cap in both orientations — filling
the bar is the point, and a cap would leave exactly the slack the
mode exists to remove.

<a id="tabbar-tab_spacing"></a>

#### `pub fn tab_spacing(mut self, dp: f32) -> Self`

Override the spacing (in dp) between adjacent tab headers in
the row. Default: `DEFAULT_TAB_SPACING`.

<a id="tabbar-pinned_tab_width"></a>

#### `pub fn pinned_tab_width(mut self, dp: f32) -> Self`

Width (in dp) of an icon-only pinned tab.
Default: `DEFAULT_PINNED_TAB_WIDTH`.

<a id="tabbar-tab_background"></a>

#### `pub fn tab_background(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

All-states shorthand for the per-tab background — every tab
(selected, idle, hovered) paints this unless a per-state override
below is set. Accepts any `Color`, `SurfaceRole`, or `Signal<Color>`
(via `ColorProp`).
Default `None` = transparent. To tint the bar's backdrop instead,
use `bar_background`.

<a id="tabbar-selected_tab_background"></a>

#### `pub fn selected_tab_background( mut self, color: impl Into<teksilo_core::color_prop::ColorProp>, ) -> Self`

Background for the **selected** tab. Falls back to
`tab_background`, then transparent.

<a id="tabbar-hover_tab_background"></a>

#### `pub fn hover_tab_background( mut self, color: impl Into<teksilo_core::color_prop::ColorProp>, ) -> Self`

Background for the **hovered** (non-selected) tab. Falls back to
`tab_background`, then transparent.

<a id="tabbar-idle_tab_background"></a>

#### `pub fn idle_tab_background( mut self, color: impl Into<teksilo_core::color_prop::ColorProp>, ) -> Self`

Background for **idle** tabs (not selected, not hovered). Falls back
to `tab_background`, then transparent.

<a id="tabbar-bar_background"></a>

#### `pub fn bar_background(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Set the backdrop fill spanning the whole bar strip (behind the
headers, slots, and scroll arrows). Independent of the per-tab
backgrounds. Accepts any `Color`, `SurfaceRole`, or `Signal<Color>`.
Default `None` = transparent.

<a id="tabbar-tab_dividers"></a>

#### `pub fn tab_dividers(mut self) -> Self`

Draw a 1 dp divider between consecutive tabs (scrollable and pinned
strips). Off by default. See `tab_divider_color`.

<a id="tabbar-tab_divider_color"></a>

#### `pub fn tab_divider_color( mut self, color: impl Into<teksilo_core::color_prop::ColorProp>, ) -> Self`

Like `tab_dividers`, but with an explicit
colour. Accepts any `Color`, `BorderRole`,
or `Signal<Color>`. Implies `tab_dividers()`.

<a id="tabbar-active_indicator"></a>

#### `pub fn active_indicator( mut self, position: teksilo_core::styles::TabIndicatorPosition, ) -> Self`

Choose which edge the active-tab highlight indicator hugs. Default
`TabIndicatorPosition::OuterEdge`
(top for horizontal / leading for vertical);
`InnerEdge`
puts it below the label (horizontal) / on the trailing edge (vertical).
Honoured by the default `RecipeTabStyle`; a custom
`TabStyle` may interpret it freely.

<a id="tabbar-selected_text_role"></a>

#### `pub fn selected_text_role(mut self, role: TextRole) -> Self`

Set the text role used for the label (and matching icon tint)
on the **selected** tab. Default: `TextRole::Primary` — the
Int UI editor-strip convention. Override to e.g.
`TextRole::Accent` when the strip sits over a tinted surface.

<a id="tabbar-idle_text_role"></a>

#### `pub fn idle_text_role(mut self, role: TextRole) -> Self`

Set the text role used for the label (and matching icon tint)
on **idle** tabs (not selected, not disabled). Default:
`TextRole::Secondary`. Disabled tabs always read as
`TextRole::Disabled` regardless of this setting.

<a id="tabbar-style"></a>

#### `pub fn style(mut self, style: impl teksilo_core::styles::TabStyle) -> Self`

Override the active `TabStyle`
for every header in this bar. The widget keeps responsibility
for the label / icon / close button composition, the
optional per-state tab backgrounds, and all input handling;
the style only paints the accent indicator and focus ring
chrome via `make_body`. Per-call override > theme slot >
built-in `RecipeTabStyle` default.

<a id="tabbar-on_pin_toggle"></a>

#### `pub fn on_pin_toggle(mut self, f: impl Fn(usize, bool, &mut EventContext) + 'static) -> Self`

Install a pin-toggle handler called whenever the user crosses
a pinned tab over the unpinned region or vice-versa during a
drag. Receives `(model_index, new_pinned_flag, ctx)`. The
firing `EventContext` lets the handler confirm the
transition via a dialog or route it through an intent before
mutating the item; apps decide whether to actually flip the
pinned state.

<a id="tabbar-bar_leading_slot"></a>

#### `pub fn bar_leading_slot(mut self, w: impl teksilo_core::IntoTeksiChild) -> Self`

Bar-level leading slot — a widget rendered before the headers
row, and before the pinned-tab strip.

<a id="tabbar-bar_trailing_slot"></a>

#### `pub fn bar_trailing_slot(mut self, w: impl teksilo_core::IntoTeksiChild) -> Self`

Bar-level trailing slot — a widget rendered after the headers
row, and after the overflow dropdown.

<a id="tabbar-separator"></a>

#### `pub fn separator(mut self, on: bool) -> Self`

Toggle the 1 dp bottom separator the bar paints under the
headers. Default: on.

<a id="tabbar-show_scroll_arrows"></a>

#### `pub fn show_scroll_arrows(mut self, on: bool) -> Self`

Toggle the leading + trailing scroll-arrow buttons. They
auto-show when the headers row overflows the bar's viewport,
and click animates the scroll position by one tab-width.
Default: on.

<a id="tabbar-overflow_button"></a>

#### `pub fn overflow_button(mut self, mode: TabOverflowButton) -> Self`

When the trailing "show all tabs" overflow dropdown appears — a
`Popover` with a `MenuList` of every tab. Default:
`TabOverflowButton::Auto` (shown only when the headers overflow the
viewport). See `TabOverflowButton` for `Always` / `Never`.

<a id="tabbar-show_overflow_dropdown"></a>

#### `pub fn show_overflow_dropdown(mut self, on: bool) -> Self`

Convenience over `overflow_button`: `true` maps
to `TabOverflowButton::Always`, `false` to `TabOverflowButton::Never`.
Prefer `overflow_button(TabOverflowButton::Auto)` for the default
"only when overflowing" behaviour.

<a id="tabbar-vertical_wheel_scrolls_horizontally"></a>

#### `pub fn vertical_wheel_scrolls_horizontally(mut self, on: bool) -> Self`

On a horizontal bar, treat a plain vertical-wheel event as a
horizontal scroll (Firefox / Chrome convention). Has no
effect on vertical or multi-line bars (those still scroll
vertically). Default: on.

<a id="tabbar-shift_wheel_scrolls_horizontally"></a>

#### `pub fn shift_wheel_scrolls_horizontally(mut self, on: bool) -> Self`

`Shift` + vertical wheel forces a horizontal scroll regardless
of orientation. Default: on.

<a id="tabbar-on_close"></a>

#### `pub fn on_close(mut self, f: impl Fn(usize, &mut EventContext) + 'static) -> Self`

Install a close-tab handler called whenever the user clicks a
closable tab's close button, middle-clicks the tab header, or
presses `Delete` on a focused tab. The handler receives the
firing `EventContext` so it can open a confirmation dialog
(`ctx.present_modal(MessageBox::confirm(...))`), dispatch an
intent, or otherwise route the close request through the
framework. To veto the close, do nothing in the handler; to
confirm-then-close, run the confirmation flow and only mutate
the underlying model on accept.

If unset and the bar is backed by a `ListModel<T>`, the
default behavior is to remove the item at the given index
from the model (no confirmation, no ctx needed for that path).

<a id="tabbar-reorderable"></a>

#### `pub fn reorderable(mut self, on: bool) -> Self`

Enable drag-to-reorder. Each tab header becomes a drag source
and the bar accepts drops anywhere along the headers row,
painting an insertion-line indicator at the would-be
position. On drop the bar calls `on_reorder`
— falling back to `ListModel::move_item` when the bar is
backed by a `ListModel<T>` and no explicit handler is set.
Default: off.

<a id="tabbar-on_reorder"></a>

#### `pub fn on_reorder(mut self, f: impl Fn(usize, usize, &mut EventContext) + 'static) -> Self`

Install a reorder handler called whenever the user drag-drops
a tab to a new position. Receives `(from, to, ctx)` —
`from`/`to` are model indices and `ctx` is the firing
`EventContext` so the handler can open a confirmation
dialog or dispatch an intent before persisting the move.
Implies `reorderable(true)`.

<a id="tabbar-accept_external_tabs"></a>

#### `pub fn accept_external_tabs(mut self, on: bool) -> Self where T: Clone,`

Opt into cross-bar tab transfer. When enabled, this bar's
headers become transfer drag sources (their drag payload
carries a clone of the dragged item) **and** the bar accepts
tabs dragged from *other* `TabBar<T>`s, painting the same
insertion-line indicator as an intra-bar reorder.

Requires `T: Clone` — the dragged item is cloned into the
payload (cheap for handle-like `T` whose heavy state lives
behind an `Rc`). Default: off.

Pair with `on_tab_received` (this bar,
as a drop target — insert the item into your model) and
`on_transfer_out` (the source bar —
remove the tab from your model).

<a id="tabbar-on_tab_received"></a>

#### `pub fn on_tab_received(mut self, f: impl Fn(T, usize, &mut EventContext) + 'static) -> Self where T: Clone,`

Install the target-side callback fired when a foreign tab is
dropped onto this bar. Receives `(item, insertion_index, ctx)`
— the moved item (taken by value from the drag payload), the
model index in *this* bar where it should land, and the firing
context. The app inserts the item into its own model. Implies
`accept_external_tabs(true)`.

<a id="tabbar-on_transfer_out"></a>

#### `pub fn on_transfer_out(mut self, f: impl Fn(TabId, &mut EventContext) + 'static) -> Self where T: Clone,`

Install the source-side callback fired after one of this bar's
tabs has been accepted by a *different* bar. Receives the
transferred tab's `TabId`; the app removes it from its own
model. Not fired for intra-bar reorders (those go through
`on_reorder`) or rejected / cancelled
drags. Implies `accept_external_tabs(true)`.

<a id="tabbar-on_external_drop"></a>

#### `pub fn on_external_drop( mut self, f: impl Fn(&DragPayload, usize, &mut EventContext) -> bool + 'static, ) -> Self`

Accept **non-tab** drops onto the bar — an in-app foreign drag
(e.g. a file dragged from a `TreeView`, carrying app data) or an
OS file/text/URL drop. The bar paints the same insertion-line
indicator while such a payload hovers, and on drop calls `f`
with the raw `DragPayload`, the model insertion index, and the
firing context. Return `true` if accepted — the app inspects the
payload (`get_typed::<T>()` / `files()` / `text()` / `uris()`)
and mints whatever it needs (e.g. opens a tab).

Independent of `accept_external_tabs`:
a bar can accept foreign tabs, non-tab payloads, both, or
neither. OS drops additionally require the app to have called
`TeksiloAppBuilder::install_external_dnd()`.

Note: the hover indicator is *optimistic* — it shows for any
non-tab payload while this handler is installed; `f`'s return
value is authoritative at drop time.
