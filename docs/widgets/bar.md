<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TabBar

`TabBar<T>` — header strip driven by a data source.

Horizontal and vertical orientations, with shared / independent
sizing. Bar-leading and bar-trailing slots are wired. Overflow is
handled by a `ScrollArea` around the headers row, plus optional
scroll arrows and a "show all tabs" overflow dropdown (both on by
default). Closable tabs (with middle-click close), drag-to-reorder
with edge auto-scroll, and a leading icon-only pinned-tab strip are
all supported. Multi-line (multi-row) wrapping is the one layout
mode not yet implemented.

The data source is consumed via the `pub(crate)` `ListSource`
abstraction so callers can pass either a `ListModel<T>` (clonable,
mutable) or any external `ListDataSource<Item = T>` (a database
cursor, a virtual list, …) without TabBar having to carry a generic
source parameter.

## Accessibility

The bar emits `Role::TabList` with an `aria-orientation`
reflecting whether it was built with `TabBar::horizontal` or
`TabBar::vertical`. When a page hosts more than one tab list,
give each one an accessible name via
`.access_label(tr!(tab_list_name()))`
so screen readers can distinguish them (ARIA APG recommendation).

```ignore
use bastyde_widgets::tab_widget::{TabBar, TabDelegate, TabId};
use bastyde_data::ListModel;
use bastyde_core::signal::Signal;

#[derive(Clone)]
struct Tab { id: TabId, title: String }

let model: ListModel<Tab> = ListModel::new();
let selected: Signal<Option<TabId>> = Signal::new(None);
let delegate = TabDelegate::new(|_i, t: &Tab| bastyde_i18n::lit!(t.title.clone()));
let _bar = TabBar::horizontal(model, delegate, selected, |_i, t| t.id)
    .reorderable(true)
    .tab_dividers();
```

## Builder methods at a glance

`horizontal`, `horizontal_from_source`, `vertical`, `vertical_from_source`, `tab_sizing`, `tab_display`, `min_tab_width`, `tab_bar_height`, `max_tab_width`, `tab_spacing`, `pinned_tab_width`, `tab_background`, `selected_tab_background`, `hover_tab_background`, `idle_tab_background`, `bar_background`, `tab_dividers`, `tab_divider_color`, `active_indicator`, `selected_text_role`, `idle_text_role`, `style`, `on_pin_toggle`, `bar_leading_slot`, `bar_leading_slot_id`, `bar_trailing_slot`, `bar_trailing_slot_id`, `separator`, `show_scroll_arrows`, `show_overflow_dropdown`, `vertical_wheel_scrolls_horizontally`, `shift_wheel_scrolls_horizontally`, `on_close`, `reorderable`, `on_reorder`, `accept_external_tabs`, `on_tab_received`, `on_transfer_out`, `on_external_drop`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/tab_widget/index.html)

## `pub const DEFAULT_MIN_TAB_WIDTH`

Default min width for an unpinned tab.

```rust
pub const DEFAULT_MIN_TAB_WIDTH: f32 = 96.0;
```

## `pub const DEFAULT_MAX_TAB_WIDTH`

Default max width for an unpinned tab.

```rust
pub const DEFAULT_MAX_TAB_WIDTH: f32 = 240.0;
```

## `pub const DEFAULT_TAB_SPACING`

Default spacing between tab headers in the row. `0.0` so tabs sit
flush against each other (Firefox / Chrome convention) — adjacent
tab boundaries are visually separated by the per-tab borders, not
by an empty gap.

```rust
pub const DEFAULT_TAB_SPACING: f32 = 0.0;
```

## `pub const DEFAULT_BAR_SLOT_SPACING`

Default spacing between the bar's leading slot, scroll area, and
trailing slot.

```rust
pub const DEFAULT_BAR_SLOT_SPACING: f32 = 8.0;
```

## `pub const DEFAULT_PINNED_TAB_WIDTH`

Default width (in dp) of a pinned tab — icon-only squares.

```rust
pub const DEFAULT_PINNED_TAB_WIDTH: f32 = 32.0;
```

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

#### `pub fn horizontal( model: ListModel<T>, delegate: TabDelegate<T>, selected_id: Signal<Option<TabId>>, id_of: impl Fn(usize, &T) -> TabId + 'static, ) -> Self`

Construct a horizontal tab bar from a `ListModel<T>`.
Default sizing is `TabSizing::Shared`.

`selected_id` is the id-based selection signal — written by
the bar on click / keyboard / drag-drop and observable by
callers. `id_of(index, &item)` extracts the stable `TabId`
from each model item.

#### `pub fn horizontal_from_source<S: ListDataSource<Item = T>>( source: S, delegate: TabDelegate<T>, selected_id: Signal<Option<TabId>>, id_of: impl Fn(usize, &T) -> TabId + 'static, ) -> Self`

Construct a horizontal tab bar from any `ListDataSource`.
Default sizing is `TabSizing::Shared`.

#### `pub fn vertical( model: ListModel<T>, delegate: TabDelegate<T>, selected_id: Signal<Option<TabId>>, id_of: impl Fn(usize, &T) -> TabId + 'static, ) -> Self`

Construct a vertical tab bar from a `ListModel<T>`. Tabs
stack top-to-bottom as horizontal pills (icon + label + close
button arranged left-to-right within each pill). Default
sizing is `TabSizing::Shared` — uniform pill heights.

#### `pub fn vertical_from_source<S: ListDataSource<Item = T>>( source: S, delegate: TabDelegate<T>, selected_id: Signal<Option<TabId>>, id_of: impl Fn(usize, &T) -> TabId + 'static, ) -> Self`

Construct a vertical tab bar from any `ListDataSource`.

#### `pub fn tab_sizing(mut self, mode: TabSizing) -> Self`

Override the per-tab sizing strategy. See `TabSizing`.

#### `pub fn tab_display(mut self, mode: TabDisplayMode) -> Self`

Choose what every tab shows — icon, label, or both. See
`TabDisplayMode`. Default `TabDisplayMode::Auto` (render each tab as
its `TabInfo` declares).

#### `pub fn min_tab_width(mut self, dp: f32) -> Self`

Minimum width (in dp) any unpinned tab will be drawn at.
Default: `DEFAULT_MIN_TAB_WIDTH`.

In **horizontal** orientation this clamps the **per-tab** width.
In **vertical** orientation every tab is forced to the bar's
cross-axis width, so the same knob defines the bar's minimum
width — the sidebar adapts to the widest piece of bar content
(tab labels or a slot widget) and never shrinks below this floor.
Vertical pill heights stay at `theme.components.tab.editor_tab_height`
regardless of this knob.

#### `pub fn tab_bar_height(mut self, dp: f32) -> Self`

Override the tab-strip cross-axis extent (the strip height for a
horizontal bar; the per-tab pill height for a vertical one). `None`
keeps the style's `editor_tab_height`. Use for a compact bar.

#### `pub fn max_tab_width(mut self, dp: f32) -> Self`

Maximum width (in dp) any unpinned tab will be drawn at — long
labels truncate with an ellipsis at this width.
Default: `DEFAULT_MAX_TAB_WIDTH`.

In **horizontal** orientation this clamps the **per-tab** width.
In **vertical** orientation it caps the whole sidebar's width —
see `min_tab_width` for the symmetric
adapt-to-content rule.

#### `pub fn tab_spacing(mut self, dp: f32) -> Self`

Override the spacing (in dp) between adjacent tab headers in
the row. Default: `DEFAULT_TAB_SPACING`.

#### `pub fn pinned_tab_width(mut self, dp: f32) -> Self`

Width (in dp) of an icon-only pinned tab.
Default: `DEFAULT_PINNED_TAB_WIDTH`.

#### `pub fn tab_background(mut self, color: impl Into<bastyde_core::color_prop::ColorProp>) -> Self`

All-states shorthand for the per-tab background — every tab
(selected, idle, hovered) paints this unless a per-state override
below is set. Accepts any `Color`, `SurfaceRole`, or `Signal<Color>`
(via `ColorProp`).
Default `None` = transparent. To tint the bar's backdrop instead,
use `bar_background`.

#### `pub fn selected_tab_background( mut self, color: impl Into<bastyde_core::color_prop::ColorProp>, ) -> Self`

Background for the **selected** tab. Falls back to
`tab_background`, then transparent.

#### `pub fn hover_tab_background( mut self, color: impl Into<bastyde_core::color_prop::ColorProp>, ) -> Self`

Background for the **hovered** (non-selected) tab. Falls back to
`tab_background`, then transparent.

#### `pub fn idle_tab_background( mut self, color: impl Into<bastyde_core::color_prop::ColorProp>, ) -> Self`

Background for **idle** tabs (not selected, not hovered). Falls back
to `tab_background`, then transparent.

#### `pub fn bar_background(mut self, color: impl Into<bastyde_core::color_prop::ColorProp>) -> Self`

Set the backdrop fill spanning the whole bar strip (behind the
headers, slots, and scroll arrows). Independent of the per-tab
backgrounds. Accepts any `Color`, `SurfaceRole`, or `Signal<Color>`.
Default `None` = transparent.

#### `pub fn tab_dividers(mut self) -> Self`

Draw a 1 dp divider between consecutive tabs (scrollable and pinned
strips). Off by default. See `tab_divider_color`.

#### `pub fn tab_divider_color( mut self, color: impl Into<bastyde_core::color_prop::ColorProp>, ) -> Self`

Like `tab_dividers`, but with an explicit
colour. Accepts any `Color`, `BorderRole`,
or `Signal<Color>`. Implies `tab_dividers()`.

#### `pub fn active_indicator( mut self, position: bastyde_core::styles::TabIndicatorPosition, ) -> Self`

Choose which edge the active-tab highlight indicator hugs. Default
`TabIndicatorPosition::OuterEdge`
(top for horizontal / leading for vertical);
`InnerEdge`
puts it below the label (horizontal) / on the trailing edge (vertical).
Honoured by the default `RecipeTabStyle`; a custom
`TabStyle` may interpret it freely.

#### `pub fn selected_text_role(mut self, role: TextRole) -> Self`

Set the text role used for the label (and matching icon tint)
on the **selected** tab. Default: `TextRole::Primary` — the
Int UI editor-strip convention. Override to e.g.
`TextRole::Accent` when the strip sits over a tinted surface.

#### `pub fn idle_text_role(mut self, role: TextRole) -> Self`

Set the text role used for the label (and matching icon tint)
on **idle** tabs (not selected, not disabled). Default:
`TextRole::Secondary`. Disabled tabs always read as
`TextRole::Disabled` regardless of this setting.

#### `pub fn style(mut self, style: impl bastyde_core::styles::TabStyle) -> Self`

Override the active `TabStyle`
for every header in this bar. The widget keeps responsibility
for the label / icon / close button composition, the
optional per-state tab backgrounds, and all input handling;
the style only paints the accent indicator and focus ring
chrome via `make_body`. Per-call override > theme slot >
built-in `RecipeTabStyle` default.

#### `pub fn on_pin_toggle(mut self, f: impl Fn(usize, bool, &mut EventContext) + 'static) -> Self`

Install a pin-toggle handler called whenever the user crosses
a pinned tab over the unpinned region or vice-versa during a
drag. Receives `(model_index, new_pinned_flag, ctx)`. The
firing `EventContext` lets the handler confirm the
transition via a dialog or route it through an intent before
mutating the item; apps decide whether to actually flip the
pinned state.

#### `pub fn bar_leading_slot(mut self, w: impl Widget + 'static) -> Self`

Bar-level leading slot — a widget rendered before the headers
row (and before any pinned region in later phases).

#### `pub fn bar_leading_slot_id(mut self, id: WidgetId) -> Self`

Bar-level leading slot accepting a pre-registered widget id.

#### `pub fn bar_trailing_slot(mut self, w: impl Widget + 'static) -> Self`

Bar-level trailing slot — a widget rendered after the headers
row (and after any overflow dropdown in later phases).

#### `pub fn bar_trailing_slot_id(mut self, id: WidgetId) -> Self`

Bar-level trailing slot accepting a pre-registered widget id.

#### `pub fn separator(mut self, on: bool) -> Self`

Toggle the 1 dp bottom separator the bar paints under the
headers. Default: on.

#### `pub fn show_scroll_arrows(mut self, on: bool) -> Self`

Toggle the leading + trailing scroll-arrow buttons. They
auto-show when the headers row overflows the bar's viewport,
and click animates the scroll position by one tab-width.
Default: on.

#### `pub fn show_overflow_dropdown(mut self, on: bool) -> Self`

Toggle the trailing "show all tabs" overflow dropdown — a
`Popover` with a `MenuList` of every tab. Default: on.

#### `pub fn vertical_wheel_scrolls_horizontally(mut self, on: bool) -> Self`

On a horizontal bar, treat a plain vertical-wheel event as a
horizontal scroll (Firefox / Chrome convention). Has no
effect on vertical or multi-line bars (those still scroll
vertically). Default: on.

#### `pub fn shift_wheel_scrolls_horizontally(mut self, on: bool) -> Self`

`Shift` + vertical wheel forces a horizontal scroll regardless
of orientation. Default: on.

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

#### `pub fn reorderable(mut self, on: bool) -> Self`

Enable drag-to-reorder. Each tab header becomes a drag source
and the bar accepts drops anywhere along the headers row,
painting an insertion-line indicator at the would-be
position. On drop the bar calls `on_reorder`
— falling back to `ListModel::move_item` when the bar is
backed by a `ListModel<T>` and no explicit handler is set.
Default: off.

#### `pub fn on_reorder(mut self, f: impl Fn(usize, usize, &mut EventContext) + 'static) -> Self`

Install a reorder handler called whenever the user drag-drops
a tab to a new position. Receives `(from, to, ctx)` —
`from`/`to` are model indices and `ctx` is the firing
`EventContext` so the handler can open a confirmation
dialog or dispatch an intent before persisting the move.
Implies `reorderable(true)`.

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

#### `pub fn on_tab_received(mut self, f: impl Fn(T, usize, &mut EventContext) + 'static) -> Self where T: Clone,`

Install the target-side callback fired when a foreign tab is
dropped onto this bar. Receives `(item, insertion_index, ctx)`
— the moved item (taken by value from the drag payload), the
model index in *this* bar where it should land, and the firing
context. The app inserts the item into its own model. Implies
`accept_external_tabs(true)`.

#### `pub fn on_transfer_out(mut self, f: impl Fn(TabId, &mut EventContext) + 'static) -> Self where T: Clone,`

Install the source-side callback fired after one of this bar's
tabs has been accepted by a *different* bar. Receives the
transferred tab's `TabId`; the app removes it from its own
model. Not fired for intra-bar reorders (those go through
`on_reorder`) or rejected / cancelled
drags. Implies `accept_external_tabs(true)`.

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
`BastydeAppBuilder::install_external_dnd()`.

Note: the hover indicator is *optimistic* — it shows for any
non-tab payload while this handler is installed; `f`'s return
value is authoritative at drop time.
