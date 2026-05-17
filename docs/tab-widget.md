# TabWidget and TabBar

Two cooperating widgets for tabbed content in FernUI: a header-only
[`TabBar<T>`](../crates/fern-widgets/src/tab_widget/bar.rs) driven by any
[`ListDataSource<Item = T>`](../crates/fern-data/src/list_data_source.rs)
and a [`TabDelegate<T>`](../crates/fern-widgets/src/tab_widget/delegate.rs),
and an all-in-one [`TabWidget`](../crates/fern-widgets/src/tab_widget.rs)
that pairs a `TabBar` with a `Switcher` of content panes — sharing one
`Signal<Option<TabId>>` selection.

`TabBar<T>` is the primitive: use it on its own when the header strip
lives in one panel and the content lives somewhere else (a separate
window, a different splitter pane, or a flat document area below).
`TabWidget` is the convenience composition for the common "header above
content" pattern.

This page is the reference for the public surface and the contracts you
can rely on.

---

## At a glance

```rust
use fern_ui::data::ListModel;
use fern_ui::prelude::*;
use fern_ui::widgets::{
    TabBarOrientation, TabHandle, TabId, TabInfo, TabSizing, TabWidget,
    TextWidget, VStack,
};

#[derive(Debug)]
struct DocState {
    title: String,
    edits: Signal<usize>,
}

let selected: Signal<Option<TabId>> = Signal::new(None);
let model: ListModel<TabHandle> = ListModel::from_vec(vec![
    TabHandle::dynamic(
        TabId::fresh(),
        "doc",
        TabInfo::new()
            .title(LocalizedString::literal("Doc 1"))
            .closable(true),
        DocState { title: "Doc 1".into(), edits: Signal::new(0) },
    ),
]);

let tw = TabWidget::new(selected.clone())
    .static_tab(
        TabInfo::new()
            .title(LocalizedString::literal("Welcome"))
            .pinned(true),
        TextWidget::new_literal("Welcome page"),
    )
    .dynamic_tab::<DocState>("doc", |_handle, state| {
        Box::new(VStack::new()
            .child(TextWidget::new_literal(state.title.clone()))
            .child(TextWidget::new_literal("…"))) as Box<dyn Widget>
    })
    .dynamic_model(model.clone())
    .reorderable(true)
    .tab_sizing(TabSizing::Shared);
```

Stand-alone `TabBar<T>` looks the same minus the content-side machinery:

```rust
use fern_ui::widgets::{TabBar, TabDelegate};

let bar = TabBar::horizontal(
    model,
    TabDelegate::new(|_, h: &TabHandle| h.info_title_or_empty())
        .closable(|_, h| h.info.closable)
        .pinned(|_, h| h.info.pinned)
        .icon(|_, h| h.info.icon.as_ref().map(|f| f())),
    selected,
    |_, h: &TabHandle| h.id,
)
.tab_sizing(TabSizing::Shared)
.reorderable(true);
```

---

## TabHandle / TabInfo / TabId

A tab's runtime identity is split across three types, each with one job:

- [`TabId`](../crates/fern-widgets/src/tab_widget/id.rs) — stable identity.
  A `NonZeroU64` wrapper. Allocate fresh ids with `TabId::fresh()` (a
  monotonic counter), or wrap an external key with
  `TabId::from_raw(NonZeroU64)` when the identity comes from app-side
  storage (document UUID, file-path hash, …) — fresh ids would re-allocate
  every restart and break session-restore round-trips.
- [`TabInfo`](../crates/fern-widgets/src/tab_widget/info.rs) — presentation
  metadata: `title`, `icon`, `tooltip`, `closable`, `pinned`, `enabled`.
  Title and tooltip are `LocalizedString` (accept `tr!(...)`); the icon is
  a factory closure (no `IconWidget: Clone` requirement) called each
  build, so it picks up theme/state changes naturally.
- [`TabHandle`](../crates/fern-widgets/src/tab_widget/handle.rs) — the
  thing that lives in the data source. Carries `id`, `info`, a `kind`
  discriminator, and an `Rc<dyn Any>` payload. Heavy state (the document,
  the image, the page) lives on `payload` — **not** on the content
  widget. Reorders, sort/filter rebuilds, and pin-toggle rebuilds destroy
  and recreate widgets freely; the handle's payload is stable and the
  registered factory produces a fresh view over it whenever the framework
  needs one.

`TabHandle::clone()` is cheap: `TabInfo` is shallow (the icon is an
`Rc<dyn Fn() -> IconWidget>` factory) and `payload` is an `Rc<dyn Any>`.

---

## Static vs dynamic tabs

`TabWidget` accepts both shapes side by side. Static tabs always render
first, in declaration order; dynamic tabs follow.

**Static tabs** are fixed for the widget's lifetime. The content is built
once and **memoized** — subsequent rebuilds (caused by adjacent
dynamic-model mutations, locale changes, theme flips) reuse the same pane
`WidgetId`, so per-pane state (focus, scroll, animation progress) is
preserved.

| Builder                                           | Content shape                              | Notes                                                                       |
|---------------------------------------------------|--------------------------------------------|-----------------------------------------------------------------------------|
| `static_tab(info, content)`                       | `impl Widget + 'static`                    | One-shot ownership; consumed on first build.                                |
| `static_tab_factory(info, fn(&TabHandle) -> Box)` | factory closure                            | Called once on first build.                                                 |
| `static_tab_id(info, WidgetId)`                   | pre-registered `WidgetId`                  | For the `fern!` DSL — wraps the id in an alias on first build.              |
| `static_tab_with_id(id, info, content)`           | `impl Widget + 'static` + caller-chosen id | Use when external code (deep links, session restore) flips selection by id. |
| `static_tab_factory_with_id(id, info, factory)`   | factory closure + caller-chosen id         | Factory variant of the above.                                               |

**Dynamic tabs** are produced from a `ListModel<TabHandle>` (or any
`ListDataSource<Item = TabHandle>`). One factory is registered per
`kind`:

```rust
.dynamic_tab::<DocState>("doc", |handle, state: &DocState| {
    Box::new(DocPane::new(state)) as Box<dyn Widget>
})
.dynamic_model(model)
```

The `<S>` type parameter pins the payload type. The framework downcasts
`handle.payload` to `S` before calling the factory and panics with a
clear "tab kind X was registered for Y but payload has different type"
message on mismatch — `Any` never leaks into app code. The `kind`
`"__static__"` is reserved for static tabs and panics at registration.

Dynamic panes are **also memoized**, keyed by `TabId`. The memo map is
pruned every build to drop entries whose tab is no longer in the model;
their widgets become unreachable and the arena reaps them.

### When to use which

- **Always-present features** that ship with the app (Welcome, Settings,
  a Console pane in an IDE, the editor's main perspective list) → static.
- **User-opened items** that come and go at runtime (open documents, open
  images, open chat threads) → dynamic.
- **Session-restored items**: dynamic, with `TabId::from_raw(...)`
  rehydrated from storage so deep links keep working.

Cross-boundary reorders (drag a dynamic tab past a static tab in the
unified ordering) are silently rejected by the **default** reorder
handler — the framework warns once per process and keeps the move from
happening. Install an explicit `on_reorder(...)` to interleave them.

---

## TabDelegate&lt;T&gt; — the per-item resolver

`TabBar<T>` is generic over the data source's item type, so the bar
needs a closure-of-closures to extract per-tab presentation. That's
`TabDelegate<T>`:

```rust
pub struct TabDelegate<T: 'static> { /* … */ }

TabDelegate::new(|i, item: &T| label_for(i, item))   // required
    .icon(|i, item|         item.icon().map(IconWidget::from))
    .leading(|i, item|      None::<Box<dyn Widget>>)
    .trailing(|i, item|     None::<Box<dyn Widget>>)
    .context_menu(|i, item| factory_for(i, item))
    .closable(|i, item|     item.is_closable())
    .pinned(|i, item|       item.is_pinned())
    .enabled(|i, item|      !item.is_locked())
    .tooltip(|i, item|      item.tooltip());
```

Closures run **at build time**, every build. Mutating an item through
`ListModel::set(i, …)` fires `DataChange::ItemUpdated` which rebuilds
the bar — closures re-run, labels and icons re-resolve. Locale changes
propagate the same way because `LocalizedString` already carries
reactive resolution semantics. There is no eager `resolve_now()`.

`TabWidget` has its own delegate-free shape (`static_tab(...)` /
`dynamic_tab::<S>(...)`) and constructs a `TabDelegate<TabHandle>`
internally that reads from `handle.info`. You only touch
`TabDelegate<T>` directly when you build a stand-alone `TabBar<T>` over
a custom `T`.

---

## TabBar vs TabWidget

The split is data flow, not features. `TabBar` owns:

- the header strip layout (axis-aware: horizontal row / vertical column)
- pinned-tab partition (leading icon-only strip)
- scroll viewport with arrows + wheel remap
- the "show all tabs" overflow dropdown
- per-tab close button (suppressed on pinned tabs)
- drag-to-reorder + insertion-line drop indicator + edge auto-scroll
- per-tab tooltip via `WidgetBuilder::tooltip`
- bar-leading and bar-trailing slots
- accessibility for the header tree (`Tab` role + `controls()` relation)

`TabWidget` adds:

- the `Switcher` of content panes
- static + dynamic tab registration
- pane memoization across rebuilds
- the unified ordering (static-then-dynamic) over the bar's index space
- callback translation: bar speaks indices, app callbacks speak `TabId`

Either widget works in the `fern!` DSL; both publish their selection
through `Signal<Option<TabId>>`.

---

## Selection — `Signal<Option<TabId>>`

Selection is **id-based**. The bar holds a stable `TabId` per item
(extracted by the `id_of` closure passed to the constructor) and the
public `selected_id` signal is the source of truth across reorders /
removals / locale changes. Internal index-based code (keyboard nav,
scroll, click) reads a private `selected_index` signal that the bar
keeps in bidirectional sync with `selected_id` at build time.

What this guarantees:

- **Reorder preserves selection.** Drag a tab from position 2 to
  position 0 with that tab selected → it is still selected after the
  move. The id matches; the index re-resolves.
- **Out-of-range writes are absorbed.** `selected_id.set(Some(id))` for
  an id not in the model leaves the visible state alone (no panic, no
  blank content).
- **External code drives it cleanly.** A "Go to tab" command sets
  `selected_id`; the bar follows. A toolbar's "open Settings" button
  sets `selected_id.set(Some(self.settings_id))` and the framework does
  the rest.

The framework's stale-id fallback: when the active tab is closed, the
bar selects the **next neighbour** (browser convention) — the index of
the tab that took the closed tab's slot, or the new last tab if the
closed tab was at the end.

---

## Orientation — reactive

[`TabBarOrientation`](../crates/fern-widgets/src/tab_widget/delegate.rs)
is `Horizontal` (default) or `Vertical`. On `TabWidget`:

```rust
TabWidget::new(selected)
    .horizontal()                              // default
    .vertical()                                // sidebar / IDE-perspective convention

// or — reactive, driven by an external signal:
let orient = Signal::new(TabBarOrientation::Horizontal);
TabWidget::new(selected).orientation_signal(orient.clone());
// later:
orient.set(TabBarOrientation::Vertical);  // bar flips, panes preserved
```

`TabWidget` binds the orientation signal at `BindingLevel::Rebuild` so
flipping it from a toolbar button rebuilds the outer layout (HStack ↔
VStack) and re-creates the inner `TabBar` with the new orientation.
**Memoized panes survive the rebuild** — focus, scroll, and per-document
state are preserved.

`TabBar<T>` chooses orientation through its constructor only:
`TabBar::horizontal(...)` / `TabBar::vertical(...)`. Switching at
runtime means rebuilding the bar — which is what the `TabWidget`
wrapper does for you.

Vertical bars use **upright** text (single-line, ellipsis-truncated),
not rotated glyphs. Rotated text breaks hit-testing and focus-ring
math, and FernUI's `text-typeset` integration doesn't yet support
per-glyph layout rotation. This matches VS Code's activity-bar style.

---

## Tab sizing — Shared vs Independent

```rust
pub enum TabSizing {
    /// All non-pinned tabs share the same extent on the layout axis —
    /// width in horizontal, height in vertical. Available region
    /// divided equally, clamped to [min_tab_extent, max_tab_extent].
    Shared,
    /// Each tab sizes to its content (icon + label + slots), clamped
    /// to [min_tab_extent, max_tab_extent]. Truncation via ellipsis
    /// when content hits max.
    Independent,
}
```

| Orientation  | "Layout axis" | Default       | Meaning                                                           |
|--------------|---------------|---------------|-------------------------------------------------------------------|
| `Horizontal` | width         | `Shared`      | Uniform tab widths (Firefox / Chrome convention).                 |
| `Vertical`   | height        | `Shared`      | Uniform pill heights — fixed at `editor_tab_height`.              |

Pinned tabs are **always fixed-extent** (`pinned_tab_width`) regardless
of `TabSizing` — that's what "pinned" means visually.

The two orientations apply Shared sizing differently:

- **Horizontal** divides the viewport width across tabs (Firefox /
  Chrome convention) and clamps by the `min_tab_width` /
  `max_tab_width` knobs:

  ```text
  available = scroll_region_width
  n         = unpinned_count
  ideal     = available / n
  target    = clamp(ideal, min_tab_width, max_tab_width)
  ```

  If `target * n < available`, slack is left as trailing empty space
  inside the scroll region (tabs do **not** stretch past `max`). If
  `target * n > available`, content overflows into scroll (arrows,
  wheel remap, dropdown engage normally).

- **Vertical does NOT divide the viewport.** Sidebar pills stay at
  the intrinsic per-tab height (`theme.components.tab.editor_tab_height`,
  default 50 dp) regardless of how tall the bar is. A 800 dp bar with
  4 tabs gives 4 pills of 50 dp at the top, not 4 × 200 dp bands.
  This matches VS Code, IntelliJ tool-window tabs, and the user
  expectation of sidebar tabs being short pills. The `min_tab_width`
  / `max_tab_width` knobs are width-defaulted (96 / 240) and
  intentionally **don't apply to vertical's height axis** — they'd
  force pills unreasonably tall.

Reactive: `TabWidget::sizing_signal(Signal<TabSizing>)` rebinds at
`BindingLevel::Rebuild` so toggling Shared ↔ Independent is a one-line
operation from a toolbar button.

---

## Pinned tabs

Tabs with `info.pinned = true` render in a **leading non-scrolling
strip** at fixed `pinned_tab_width` (default 32 dp), icon-only, with no
close button. This is the Firefox / Chrome convention.

Critical contract: **the model does not need to keep pinned items
contiguous.** At render time the bar partitions the source:

```text
items         = source.iter()
pinned_view   = items.filter(|(i, it)| delegate.pinned(i, it))
unpinned_view = items.filter(|(i, it)| !delegate.pinned(i, it))
```

Indices in callbacks (`on_close(i)`, `selected.set(i)`,
`on_reorder(from, to)`) remain **model indices**, not view positions.

When the title is `None` and the tab is pinned, the framework promotes
`info.title` (if any) to the tooltip — pinned tabs render icon-only and
otherwise have no way for the user to identify them on hover.

DnD across the pinned/unpinned boundary fires
`on_pin_toggle(model_index, new_pinned_flag)`. The app decides whether
to actually mutate `info.pinned`; the bar reports the desired
transition without applying it itself (pinning is app semantics).

---

## Close, reorder, pin handlers

```rust
TabWidget::new(selected)
    // …
    .on_close(|id: TabId| {
        // default behavior: remove from dynamic_model.
        // static tabs are not auto-closable.
    })
    .on_reorder(|moved_id: TabId, dest_index: usize| {
        // default behavior: ListModel::move_item within the dynamic region only.
        // implies .reorderable(true).
    })
    .on_pin_toggle(|id: TabId, new_pinned: bool| {
        // no default — pinning is app semantics.
    });
```

Note the indirection: `TabWidget` callbacks speak `TabId`, but inside,
the bar receives indices. The wrapper translates at the boundary using
the `index_to_id` table captured at build time. On stand-alone
`TabBar<T>` the callbacks are `Fn(usize)` / `Fn(usize, usize)` — the
caller is closer to the data source and may prefer indices.

`on_reorder(...)` implicitly sets `reorderable(true)`. The default
reorder handler refuses cross-boundary moves (dynamic past static) and
prints a one-shot stderr warning pointing at the install-explicit-handler
fix; high-frequency drag events do not spam the log.

Middle-click on a closable tab fires `on_close` (Firefox convention).
Pinned tabs suppress the close button regardless of `closable`.

---

## Drag & drop

Drag-reorder follows the same pattern `ListView` uses. Each tab header
is a drag source; the bar is the drop target.

- **Payload.** `TabBarDragData { source_index, source_bar_id }`. The
  `source_bar_id` field is checked on drop — drops from a different
  `TabBar` are rejected so a tab from one bar can't be dropped into
  another's content area as a reorder.
- **Insertion math.** `on_drag_hover` computes the insertion boundary
  from pointer position relative to tab boundaries (per-axis: x for
  horizontal, y for vertical when wired). The boundary is published
  through a shared `Cell<Option<f32>>` that the bar's `paint()` reads.
- **Drop indicator.** A 2 dp accent-color line at the insertion
  boundary. Vertical line for horizontal bar, horizontal line for
  vertical bar — both the paint and the hover-to-insertion-boundary
  math are axis-aware.
- **Edge auto-scroll.** `on_drag_tick` ramps scroll velocity inside a
  32 dp edge zone, capped at 12 dp/frame — same constants as `ListView`.
- **Pinned/unpinned model index translation.** Insertion is computed in
  unpinned-view space; the bar maintains an `unpinned_to_model` map and
  converts before applying the post-removal `-1` adjustment (`from <
  to_model → to_model - 1`) and calling `on_reorder(from_model,
  adjusted_to)`.
- **Cross-pane drops** (drop a non-pinned tab into the pinned strip, or
  vice versa) fire `on_pin_toggle` instead of `on_reorder`.

Drag-reorder is fully wired in both orientations.

---

## Overflow chrome

When the headers row doesn't fit the viewport, three affordances engage
(all toggleable):

### Scroll arrows

Two `IconButton`s (chevron-leading, chevron-trailing, embedded mode) flank the
scrollable region. **Visibility is dynamic**: leading visible iff
`scroll_x > 0`, trailing visible iff `scroll_x < max_scroll_x`. Click
animates `scroll_x` by ~one tab-width via `Signal::animate_to` with
`MotionTokens::duration_normal`.

```rust
.show_scroll_arrows(true)   // default
```

### Mouse wheel mapping

On a horizontal bar, vertical-only wheel deltas remap to horizontal
scroll (Firefox / Chrome convention). Shift+wheel always remaps,
regardless of orientation — useful on touchpads where two-finger scroll
is ambiguous. Diagonal trackpad gestures pass through.

```rust
.vertical_wheel_scrolls_horizontally(true)   // default
.shift_wheel_scrolls_horizontally(true)       // default
```

Wheel "lines" are converted to pixels at 64 dp/line (≈ one tab-width
per notch) so a single notch scrolls one full tab into view.

### "Show all tabs" overflow dropdown

A single trailing `PopoverButton` with a chevron icon. Clicking it
opens a `Popover` containing a `ListView` of every tab (pinned
included). Activating an item sets `selected_id` and dismisses the
popover.

```rust
.show_overflow_dropdown(true)   // default
```

The dropdown is rendered whenever `show_overflow_dropdown(true)`, not
only on overflow — Firefox does the same, since the fast-jump is useful
even with a few tabs. The popover's surface is a `Panel` with
`SurfaceRole::Raised` and bounded height (max 320 dp, 28 dp per row),
scrolling internally on long lists.

The dropdown advertises `HasPopup::Menu` to AccessKit so screen readers
announce it as a popup trigger.

### Keyboard `ScrollIntoView`

Tab keyboard nav into an off-screen tab is handled by the framework's
existing `WidgetEvent::ScrollIntoView` path on `ScrollArea` — when a
tab header gains focus and lies outside the viewport, ScrollArea
auto-scrolls to bring it on-screen. No tab-specific code is needed.

---

## Bar slots

Two stable widget positions for app chrome that should travel with the
bar:

```rust
.bar_leading_slot(small_breadcrumb_or_logo)    // before the pinned strip
.bar_trailing_slot(new_tab_button_toolbar)     // after the dropdown
```

Both accept `impl Widget + 'static`. `_id` variants take a
pre-registered `WidgetId` for the `fern!` DSL. The slot widget is
registered once on first build and **memoized** — subsequent rebuilds
reuse the same id, so a slot's internal state (button hover, tooltip
visibility, focus) survives bar rebuilds.

Slots scroll **with** the bar's outer chrome, not with the headers row
— a "+" button in the trailing slot stays visible regardless of
horizontal scroll position.

---

## Keyboard

| Key                       | Effect                                                   |
|---------------------------|----------------------------------------------------------|
| `ArrowLeft` / `ArrowUp`   | move selection to previous enabled tab                   |
| `ArrowRight` / `ArrowDown`| move selection to next enabled tab                       |
| `Home`                    | jump to first enabled tab                                |
| `End`                     | jump to last enabled tab                                 |
| `Enter` / `Space`         | activate the focused tab (sets `selected_id`)            |
| `Ctrl+W`                  | close the focused tab if `closable`                      |
| `Middle-click`            | close the clicked tab if `closable` (mouse, not keyboard)|

Disabled tabs are **skipped** by all keyboard navigation. Out-of-range
selection writes are absorbed harmlessly. Focus moves with selection;
`ScrollArea` scrolls the bar to keep the focused tab visible via the
existing `ScrollIntoView` event.

The framework dispatches both ArrowLeft/Up and ArrowRight/Down to the
"prev/next" handlers regardless of orientation — the same key map works
for horizontal and vertical bars without re-mapping.

---

## Accessibility

- **TabBar root**: `Role::TabList` with `orientation = Horizontal |
  Vertical`.
- **Each tab header**: `Role::Tab`, with `selected = bool` reflecting
  the active tab. The `controls()` relation points at the tab's
  content-panel `WidgetId` when the bar is composed inside `TabWidget`.
- **Each content pane**: `Role::TabPanel`, named after the tab's
  resolved title.
- **Pinned tabs**: include `access_description("Pinned tab")` so screen
  readers distinguish them.
- **Closable tabs**: advertise `accesskit::Action::Default` plus a
  custom action with i18n name "Close" wired to `on_close`.
- **Reorderable tabs**: advertise custom actions "Move Left" and
  "Move Right" (or "Move Up" / "Move Down" on vertical bars), invoking
  the same reorder path drag-drop uses. AT users can't drag, so this is
  the supported reorder affordance.
- **Overflow dropdown**: `HasPopup::Menu` + `controls(menu_list_id)`.
- **Scroll arrows**: `Role::Button` with i18n labels "Scroll tabs
  left" / "Scroll tabs right".

The full `TabList → Tab → TabPanel` hierarchy is what AT software
expects from a tabbed container, and matches what Firefox and Chrome
publish for their own browser tabs.

---

## Theme tokens

| Surface                     | Role                                    |
|-----------------------------|-----------------------------------------|
| bar backdrop + tab fills    | `tab_surface_role` (settable)           |
| label text — selected       | `selected_text_role` (settable)         |
| label text — idle           | `idle_text_role` (settable)             |
| label text — disabled       | `TextRole::Disabled` (always)           |
| accent indicator (selected) | `theme.components.tab.underline_active` |
| bar bottom separator        | `BorderRole::DividerStrong`             |
| close button hover          | `SurfaceRole::Hover`                    |
| drop indicator line         | `TextRole::Accent`                      |
| overflow popover surface    | `SurfaceRole::Raised`                   |
| overflow popover border     | `BorderRole::Default`                   |

`tab_surface_role` defaults to transparent and accepts any `Color`,
`SurfaceRole`, or `Signal<Color>` (via [`ColorProp`]). When set, the
bar paints it as a uniform backdrop covering the whole strip — leading
slot, pinned strip, scroll arrows, headers row, overflow dropdown, and
trailing slot all share the surface, so the bar reads as a single
plane regardless of how the chrome is composed.

`selected_text_role` defaults to `TextRole::Primary` (the Int UI
editor-strip convention); `idle_text_role` defaults to
`TextRole::Secondary`. Override either to e.g. `TextRole::Accent` /
`TextRole::Tertiary` when the strip sits over a tinted surface and
the default cascade reads with insufficient contrast. Disabled tabs
always render at `TextRole::Disabled`.

Static numbers come from `theme.components.tab`
([`TabStyle`](../crates/fern-tokens/src/components.rs)):

- `editor_tab_height` (default 50 dp) — height of horizontal bar tabs.
- `tool_window_tab_height` (default 28 dp) — reserved for future
  tool-window tab variant; not currently consumed by vertical bars.
- `underline_active` — accent color for the selection indicator.

The accent indicator paints at the **top edge** in horizontal bars and
the **leading edge** in vertical bars. Tabs use a uniform surface
across all states (`tab_surface_role`); selection is conveyed by the
accent indicator and the label-color shift only — Int UI editor-strip
convention.

```rust
TabWidget::new(selected)
    .tab_surface_role(SurfaceRole::Content)        // role-driven, theme-aware
    .selected_text_role(TextRole::Primary)         // override the selected label color
    .idle_text_role(TextRole::Secondary);          // override the idle label color
```

---

## What is and isn't shipped

**Shipped:**

- horizontal + vertical orientations, both reactive
- shared / independent sizing, both reactive
- static + dynamic tabs in one widget, with pane memoization across
  rebuilds (focus, scroll, animation, rich-text editor history all
  survive)
- closable tabs (button + middle-click), with selection re-anchoring
- pinned tabs (icon-only fixed-width leading strip, no close button,
  tooltip-promoted title)
- drag-to-reorder with insertion-line indicator, edge auto-scroll, and
  pinned/unpinned cross-boundary `on_pin_toggle` semantics
- horizontal scroll with leading + trailing arrow buttons and dynamic
  visibility
- mouse-wheel-to-horizontal mapping (configurable: vertical-only,
  shift-only, both, neither)
- "show all tabs" overflow dropdown via `PopoverButton` + `ListView`
- keyboard navigation: arrow keys, Home/End, Enter/Space, Ctrl+W
- accessibility: `TabList` / `Tab` / `TabPanel` roles; "Move Left/Right"
  custom actions for AT-driven reorder; named close action; `HasPopup`
  on the dropdown
- `Signal<Option<TabId>>` selection that survives reorders, removals,
  locale and theme changes

**Intentionally not shipped:**

- multi-line / wrapping horizontal bar (was prototyped via
  `Wrap::max_lines(...)`; dropped — lots of layout machinery for a
  feature most desktop apps don't use, and the overflow dropdown covers
  the same fast-jump need)
- touchscreen flick momentum on the scroll viewport (desktop trackpads
  hit the existing `ScrollDelta::Pixels` path with `Easing::EaseOut`
  animation; touch flicks would need `ScrollArea` ↔ `SwipeRecognizer`
  wiring, ~150 LOC, separate task)
- `tool_window_tab_height` (28 dp) is reserved on `TabStyle` but not
  yet consumed by vertical bars — they currently pick up
  `editor_tab_height` like horizontal bars

---

## Migration from the legacy `TabWidget::new(...).tab(...)` API

The pre-rewrite `.tab(label, content)` chain is preserved as a thin
shim — existing code keeps compiling. New work should prefer the
data-source-driven shape:

| Old                                     | New                                                    |
|-----------------------------------------|--------------------------------------------------------|
| `TabWidget::new(idx).tab(label, w)`     | `TabWidget::new(selected_id).static_tab(info, w)`      |
| `TabWidget::new(idx).tab_id(label, id)` | `TabWidget::new(selected_id).static_tab_id(info, id)`  |
| `selected: Signal<usize>`               | `selected: Signal<Option<TabId>>`                      |
| `.trailing_slot(w)`                     | `.bar_trailing_slot(w)`                                |

The new shape unlocks add/remove/reorder at runtime, locale-reactive
labels, drag-drop, pinning, and the overflow dropdown — none of which
the index-based shim could express.

---

## Demos

- `cargo run -p tab-widget` — full showcase: static tabs (pinned,
  disabled, default), three dynamic tabs from a `ListModel<TabHandle>`,
  registered `dynamic_tab::<DocState>` factory, "+ New tab" trailing
  button, theme / orientation / sizing toggle buttons, drag-reorder,
  overflow dropdown, pinned-tab tooltip promotion, status bar showing
  the resolved selection.
- `cargo run -p widget-catalog` — TabWidget appears in the catalog with
  the legacy shim demo for visual regression checks.
