<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TabWidget

Tabbed-container widgets.

Two public entry points:

- `TabBar<T>` — a header strip driven by a `ListModel<T>` /
  `ListDataSource` and a
  `TabDelegate<T>`. Use it stand-alone when you want only the
  tab strip (e.g., a document tab strip whose content lives in a
  different panel or window).

- `TabWidget` — the all-in-one composition: bar above, content
  `Switcher` below, sharing one selection signal. Two
  construction flavors:
    - `static_tab(info, content)` —
      fixed tabs accumulated at construction.
    - `dynamic_tab::<S>(kind, factory)` +
      `dynamic_model(model)` — apps
      register a content factory per tab `kind` (`"plain-text-doc"`,
      `"image"`, …); the live tab list is a mutable
      `ListModel<TabHandle>` mutated at runtime (open / close /
      reorder).

Static tabs always render first, in declaration order; dynamic
tabs follow. Selection is by stable `TabId` — drag-reorder and
model mutations never silently send the active selection to a
different tab.

## Activating a tab scrolls it into view

When more tabs are open than the strip can show, activating one
always reveals it — including when the activation is programmatic
(writing the selection signal, the "show all tabs" overflow
dropdown, an assistive-technology click). Pointer and keyboard
activation move focus and would be revealed by the framework's focus
follow anyway; the other paths move no focus, so the bar scrolls the
header in itself, by the minimum needed to bring it fully inside the
viewport.

The reveal is edge-triggered on the selection changing, not an
invariant re-asserted every layout pass: once the reader has scrolled
away from the active tab by hand, a rebuild for an unrelated reason —
a retitled tab, a locale change, a tab opened elsewhere in the strip —
leaves the viewport where they left it.

## Accessibility

Both `TabWidget` and `TabBar` emit `Role::TabList` on the bar
and `Role::Tab` on each header. ARIA APG ([tabs
pattern](https://www.w3.org/WAI/ARIA/apg/patterns/tabs/))
recommends providing an accessible name for the tab list
whenever a page hosts more than one — call
`.access_label(tr!(editor_tabs()))`
on the widget so screen readers can distinguish "editor tabs"
from "tool tabs":

```ignore
TabWidget::new(selected)
    .static_tab(TabInfo::new().title(tr!(welcome())), welcome_panel)
    // ...
    .access_label(tr!(editor_tabs()))
```

Panels with no focusable descendants (a static text-only "About"
tab, a chart-only metrics tab) are unreachable by Tab key unless
opted in via `TabInfo::focusable_panel(true)`.

## Builder methods at a glance

`enabled`, `bar_visibility`, `tab_bar_height`, `compact_bar`, `vertical`, `horizontal`, `orientation`, `static_tab`, `tab`, `tab_id`, `static_tab_factory`, `static_tab_id`, `static_tab_with_id`, `static_tab_factory_with_id`, `dynamic_tab`, `dynamic_model`, `tab_sizing`, `sizing`, `tab_display`, `tab_background`, `selected_tab_background`, `hover_tab_background`, `idle_tab_background`, `bar_background`, `tab_dividers`, `tab_divider_color`, `active_indicator`, `selected_text_role`, `idle_text_role`, `min_tab_width`, `max_tab_width`, `pinned_tab_width`, `show_scroll_arrows`, `overflow_button`, `show_overflow_dropdown`, `reorderable`, `on_close`, `on_reorder`, `on_pin_toggle`, `accept_external_tabs`, `on_tab_received`, `on_transfer_out`, `on_external_drop`, `bar_leading_slot`, `bar_trailing_slot`, `bar_leading_slot_id`, `bar_trailing_slot_id`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/tab_widget/index.html)

## `pub type StaticContentFactory`

Closure that builds a static tab's content widget. Called once
per static tab — on the `TabWidget`'s first build that includes
it. The resulting pane is then memoized: rebuilds caused by
adjacent dynamic-model mutations reuse the same pane WidgetId, so
internal state (focus, scroll, animation progress, …) survives.

```rust
pub type StaticContentFactory = Rc<dyn Fn(&TabHandle) -> Box<dyn Widget>>;
```

## `pub struct TabWidget`

All-in-one tabbed container. Builds a `TabBar` above a
`Switcher` of content panes, sharing one selection signal.

```rust
pub struct TabWidget { /* fields */ }
```

### Methods

#### `pub fn new(selected: Signal<Option<TabId>>) -> Self`

Construct an empty `TabWidget`. Selection is `None` until
the first `static_tab(...)` / `dynamic_model(...)` adds a
tab and the framework activates it.

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Enable or disable the whole widget. A disabled `TabWidget` greys out
and stops accepting focus / selection / keyboard input
(arena-gated). Distinct from per-tab `TabInfo::enabled`.

#### `pub fn bar_visibility(mut self, visibility: TabBarVisibility) -> Self`

Set the tab-strip visibility policy (default
`TabBarVisibility::Always`). Use `TabBarVisibility::WhenMultiple`
to hide the strip while a single tab is present, or
`TabBarVisibility::Never` when an external selector (e.g. a
docking activity rail) drives selection.

#### `pub fn tab_bar_height(mut self, dp: f32) -> Self`

Override the tab-strip height (its cross-axis extent). `None` /
unset keeps the style's `editor_tab_height` (50 dp). Use for a denser
strip — e.g. dock side panels.

#### `pub fn compact_bar(self) -> Self`

Shorthand for a **compact** (38 dp) tab strip — denser than the standard
50 dp editor strip. Equivalent to `self.tab_bar_height(38.0)`.

#### `pub fn vertical(self) -> Self`

Configure the bar to render vertically — pills stacked
top-to-bottom on the leading edge, content fills the trailing
area (sidebar / IDE-perspective convention). Equivalent to
`self.orientation(TabBarOrientation::Vertical)`.

#### `pub fn horizontal(self) -> Self`

Configure the bar to render horizontally — pills laid out
left-to-right above the content (browser tab convention).
This is the default.

#### `pub fn orientation(mut self, orientation: impl Into<Prop<TabBarOrientation>>) -> Self`

Set the bar orientation, statically or reactively. Passing a
`Signal<TabBarOrientation>` replaces the internal orientation
signal with the external one — lets a parent widget toggle
orientation reactively (e.g. a "View → Vertical Tabs" toolbar
button) without recreating the `TabWidget`.

#### `pub fn static_tab(mut self, info: TabInfo, content: impl Widget + 'static) -> Self`

Add a static tab — fixed for the widget's lifetime, with a
pre-built content widget. The content is registered in the
arena on the `TabWidget`'s first build and **memoized** —
subsequent rebuilds (caused by adjacent dynamic-model
mutations) reuse the same pane WidgetId, preserving any
internal state the content owns.

#### `pub fn tab(self, label: impl Into<LocalizedString>, content: impl Widget + 'static) -> Self`

Ergonomic shorthand for a title-only static tab:
`tab(label, content)` is `static_tab(TabInfo::new().title(label),
content)`. `label` accepts `tr!(...)` (translated) or `lit!(...)`.
This is the method the `teksu!` `tab:` slot lowers to
(`tab: lit!("Overview"), Card { … }`).

#### `pub fn tab_id(self, label: impl Into<LocalizedString>, id: WidgetId) -> Self`

`WidgetId` twin of `tab` — `tab_id(label, id)` is
`static_tab_id(TabInfo::new().title(label), id)`. This is what the
`teksu!` `tab:` slot lowers to when its content is an id binding
(`#{…}` / `name = Element`).

#### `pub fn static_tab_factory( mut self, info: TabInfo, factory: impl Fn(&TabHandle) -> Box<dyn Widget> + 'static, ) -> Self`

Add a static tab whose content is constructed by a factory
closure. The factory is called once — on the slot's first
build — and the resulting pane is memoized just like
`static_tab`.

#### `pub fn static_tab_id(mut self, info: TabInfo, content_id: WidgetId) -> Self`

Element-valued slot variant for the `teksu!` DSL — accepts a
pre-registered widget id rather than a `Box<dyn Widget>`.
Equivalent to `static_tab` with an
already-built child; the id is wrapped in a tab pane on
first build and the pane id is memoized thereafter.

#### `pub fn static_tab_with_id( mut self, id: TabId, info: TabInfo, content: impl Widget + 'static, ) -> Self`

Add a static tab with a caller-provided `TabId` — useful
when external code (an app-event handler, a session-restore
path, a deep link) needs to flip selection to this tab by id.
The pane is memoized like `static_tab`.

#### `pub fn static_tab_factory_with_id( mut self, id: TabId, info: TabInfo, factory: impl Fn(&TabHandle) -> Box<dyn Widget> + 'static, ) -> Self`

Factory variant of `static_tab_with_id`.

#### `pub fn dynamic_tab<S: Any + 'static>( mut self, kind: &'static str, factory: impl Fn(&TabHandle, &S) -> Box<dyn Widget> + 'static, ) -> Self`

Register a dynamic-tab content factory keyed by `kind`. The
`<S>` type parameter pins the payload type — the framework
downcasts `handle.payload` to `S` before calling the
factory and panics with a clear message on kind/payload
mismatch, so `Any` never leaks into app code.

#### `pub fn dynamic_model(mut self, model: ListModel<TabHandle>) -> Self`

Connect the dynamic-tab data source. Mutations rebuild the
dynamic-tab subtree; static tabs are unaffected.

#### `pub fn tab_sizing(mut self, mode: TabSizing) -> Self`

Set the per-tab sizing strategy as a static value. Internally
stores it as a `Signal<TabSizing>` so the widget can be
retrofitted to reactive control via `Self::sizing`
without breaking existing call sites.

#### `pub fn sizing(mut self, sizing: impl Into<Prop<TabSizing>>) -> Self`

Bind the per-tab sizing strategy, statically or reactively —
flipping a bound signal swaps between Shared / Independent / Fill
live, with no rebuild on the parent's part. The signal is bound at
`BindingLevel::Rebuild` inside `build`;
memoized panes survive the rebuild so per-tab state is
preserved.

#### `pub fn tab_display(mut self, mode: impl Into<Prop<TabDisplayMode>>) -> Self`

Choose what every tab shows — icon, label, or both
(`TabDisplayMode`), statically or reactively. A bound signal can be
flipped to swap icon / text / icon+text live (the bar rebuilds,
memoized panes survive), with no rebuild on the parent's part. Bound
at `BindingLevel::Rebuild`.

#### `pub fn tab_background(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

All-states shorthand for the per-tab background — every tab
(selected, idle, hovered) paints this unless a per-state override
is set. Accepts any `Color`, `SurfaceRole`, or `Signal<Color>` (via
`ColorProp`). Default is
transparent. To tint the bar's backdrop instead, use
`bar_background`.

#### `pub fn selected_tab_background( mut self, color: impl Into<teksilo_core::color_prop::ColorProp>, ) -> Self`

Background for the **selected** tab. Falls back to
`tab_background`, then transparent.

#### `pub fn hover_tab_background( mut self, color: impl Into<teksilo_core::color_prop::ColorProp>, ) -> Self`

Background for the **hovered** (non-selected) tab. Falls back to
`tab_background`, then transparent.

#### `pub fn idle_tab_background( mut self, color: impl Into<teksilo_core::color_prop::ColorProp>, ) -> Self`

Background for **idle** tabs (not selected, not hovered). Falls back
to `tab_background`, then transparent.

#### `pub fn bar_background(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Set the bar-strip backdrop fill (behind headers, slots, arrows),
independent of the per-tab backgrounds. Default transparent.

#### `pub fn tab_dividers(mut self) -> Self`

Draw a 1 dp divider between consecutive tabs. Off by default.

#### `pub fn tab_divider_color( mut self, color: impl Into<teksilo_core::color_prop::ColorProp>, ) -> Self`

Like `tab_dividers` with an explicit colour
(`Color`, `BorderRole`, or
`Signal<Color>`). Implies `tab_dividers()`.

#### `pub fn active_indicator( mut self, position: teksilo_core::styles::TabIndicatorPosition, ) -> Self`

Choose which edge the active-tab highlight indicator hugs. Default
`TabIndicatorPosition::OuterEdge`;
`InnerEdge`
puts it below the label (horizontal) / trailing edge (vertical).

#### `pub fn selected_text_role(mut self, role: teksilo_tokens::TextRole) -> Self`

Set the text role used for the label (and matching icon tint)
on the **selected** tab. Default: `teksilo_tokens::TextRole::Primary`
— the Int UI editor-strip convention. Override to e.g.
`teksilo_tokens::TextRole::Accent` when the strip sits over a
tinted surface.

#### `pub fn idle_text_role(mut self, role: teksilo_tokens::TextRole) -> Self`

Set the text role used for the label (and matching icon tint)
on **idle** tabs (not selected, not disabled). Default:
`teksilo_tokens::TextRole::Secondary`. Disabled tabs always read
as `teksilo_tokens::TextRole::Disabled` regardless of this
setting.

#### `pub fn min_tab_width(mut self, dp: f32) -> Self`

Minimum scrollable-tab width in logical pixels. Default
`DEFAULT_MIN_TAB_WIDTH`.

#### `pub fn max_tab_width(mut self, dp: f32) -> Self`

Maximum scrollable-tab width in logical pixels. Default
`DEFAULT_MAX_TAB_WIDTH`.

#### `pub fn pinned_tab_width(mut self, dp: f32) -> Self`

Fixed width for pinned (icon-only) tabs in logical pixels. Default
`DEFAULT_PINNED_TAB_WIDTH`.

#### `pub fn show_scroll_arrows(mut self, on: bool) -> Self`

Show or hide the leading/trailing scroll-arrow buttons when tabs overflow.
Default (unset) uses the style's preference.

#### `pub fn overflow_button(mut self, mode: TabOverflowButton) -> Self`

When the trailing "show all tabs" overflow dropdown appears. Default
(unset) is `TabOverflowButton::Auto` — shown only when the tab headers
overflow the bar's viewport. See `TabOverflowButton` for
`Always` / `Never`.

#### `pub fn show_overflow_dropdown(mut self, on: bool) -> Self`

Convenience over `overflow_button`: `true` maps
to `TabOverflowButton::Always`, `false` to `TabOverflowButton::Never`.

#### `pub fn reorderable(mut self, on: bool) -> Self`

Allow drag-to-reorder of tabs within the bar. Default `false`.
Setting `on_reorder` implies `reorderable(true)`.

#### `pub fn on_close(mut self, f: impl Fn(TabId, &mut EventContext) + 'static) -> Self`

Install a close-tab handler. Receives the `TabId` of the
closed tab (not its index — indices are presentation-only)
and the firing `EventContext`. The latter lets the handler
open a confirmation dialog
(`ctx.present_modal(MessageBox::confirm(...))`), dispatch an
intent, or otherwise route the close request before mutating
the underlying model. To veto, do nothing in the handler; to
confirm-then-close, only call the model mutator on accept.

If unset, the default behavior is to remove the tab from
`dynamic_model` without a prompt
(static tabs cannot be closed by default).

#### `pub fn on_reorder(mut self, f: impl Fn(TabId, usize, &mut EventContext) + 'static) -> Self`

Install a reorder handler. Receives `(moved_tab_id,
destination_index, ctx)` in the unified static-then-dynamic
ordering. The firing `EventContext` lets the handler
confirm or dispatch the reorder via a dialog / intent
before mutating the model. If unset, the default behavior
is to reorder within the dynamic region of
`dynamic_model`. Implies
`reorderable(true)`.

#### `pub fn on_pin_toggle(mut self, f: impl Fn(TabId, bool, &mut EventContext) + 'static) -> Self`

Install a pin-toggle handler — receives `(tab_id,
new_pinned_flag, ctx)` when the user drags a tab across the
pinned ↔ unpinned boundary. The firing `EventContext`
lets the handler confirm or dispatch the transition via a
dialog / intent. Apps decide whether to actually mutate the
tab's `info.pinned`.

#### `pub fn accept_external_tabs(mut self, on: bool) -> Self`

Opt into cross-`TabWidget` tab transfer (app-internal
drag-and-drop between two tabbed containers). When enabled,
this widget's **dynamic** tabs can be dragged out to any other
accepting `TabWidget`, and it accepts tabs dragged in from one,
painting an insertion-line indicator between its tabs.

The dragged `TabHandle` moves intact — its `Rc<dyn Any>`
payload (the heavy per-tab state) is preserved, not rebuilt —
so the receiving widget must register a content factory for the
tab's `kind` via `dynamic_tab`.

**Static tabs are excluded**: they have no factory on a
receiving widget, so they can never be transferred out (they
still reorder in place if `reorderable`).

By default, accepting a tab inserts it into this widget's
`dynamic_model` and transferring one out
removes it from this widget's model. Override either side with
`on_tab_received` /
`on_transfer_out`. Default: off.

#### `pub fn on_tab_received( mut self, f: impl Fn(TabHandle, usize, &mut EventContext) + 'static, ) -> Self`

Override the target-side behaviour when a foreign tab is
dropped onto this widget. Receives `(handle, insertion_index,
ctx)` where `insertion_index` is within the **dynamic** tab
region. The app inserts the handle into its own model. Implies
`accept_external_tabs(true)`.

If unset, the default inserts the handle into
`dynamic_model` at the drop position.

#### `pub fn on_transfer_out(mut self, f: impl Fn(TabId, &mut EventContext) + 'static) -> Self`

Override the source-side behaviour after one of this widget's
tabs has been accepted by another `TabWidget`. Receives the
transferred `TabId`; the app removes it from its own model.
Implies `accept_external_tabs(true)`.

If unset, the default removes the tab from
`dynamic_model`.

#### `pub fn on_external_drop( mut self, f: impl Fn(&DragPayload, usize, &mut EventContext) -> bool + 'static, ) -> Self`

Accept **non-tab** drops onto the tab bar — an in-app foreign
drag (e.g. a file dragged from a `TreeView`, carrying app data)
or an OS file/text/URL drop. The bar shows an insertion-line
indicator while such a payload hovers; on drop, `f` runs with
the raw `DragPayload`, the insertion index *within the dynamic
region*, and the firing context. Inspect the payload
(`get_typed::<T>()` / `files()` / `text()` / `uris()`) and, e.g.,
push a new `TabHandle` into your `dynamic_model`;
return `true` if accepted.

This is the "open a dropped file as a tab" hook (VS Code style).
Independent of `accept_external_tabs`.
OS drops also require `TeksiloAppBuilder::install_external_dnd()`.

#### `pub fn bar_leading_slot(mut self, w: impl Widget + 'static) -> Self`

Place a widget on the leading edge of the tab strip (before the first
tab). Memoized: registered once on first build, reused on rebuilds.

#### `pub fn bar_trailing_slot(mut self, w: impl Widget + 'static) -> Self`

Place a widget on the trailing edge of the tab strip (after the last
tab and overflow button). Memoized like
`bar_leading_slot`.

#### `pub fn bar_leading_slot_id(mut self, id: WidgetId) -> Self`

Element-valued variant of
`bar_leading_slot` accepting a
pre-registered `WidgetId` (for the `teksu!` DSL).

#### `pub fn bar_trailing_slot_id(mut self, id: WidgetId) -> Self`

Element-valued variant of
`bar_trailing_slot`.

## `pub enum TabBarVisibility`

Controls whether a `TabWidget`'s tab strip is shown.

The default is `Always` — fully
back-compatible with the historical behaviour. `WhenMultiple` hides the strip while a single tab
is present (the content fills the whole area) and shows it again
once a second tab appears; the evaluation is reactive because a
dynamic-model mutation already rebuilds the `TabWidget`.
`Never` always hides the strip (the
selector lives elsewhere — e.g. a docking activity rail).

```rust
pub enum TabBarVisibility { /* variants */ }
```

### Variants

- **`Always`** — Always render the tab strip (historical default).
- **`WhenMultiple`** — Show the strip only when two or more tabs are present.
- **`Never`** — Never render the strip; the content fills the whole area.
