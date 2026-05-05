# Widget Catalog — Gap Analysis

## Context

Goal: identify every widget/layout missing from FernUI so the catalog can be
driven to completeness. Sources cross-referenced:

1. Authoritative design: **Section 27** of
   [docs/fern-ui-architecture.md](docs/fern-ui-architecture.md#L2721).
2. Qt 6 widget catalog: <https://doc.qt.io/qt-6/widget-classes.html> — the
   reference desktop toolkit.
3. JetBrains Int UI / Jewel: FernUI's visual target, with a handful of
   distinctive components not found in Qt.

This is a survey, not an implementation plan. Deliverable = a prioritized list
the user can pick from.

---

## Already implemented (for disambiguation)

A few entries the user asked about are already in the tree — confirming them
so they don't appear as gaps:

- **SegmentedButton** — shipped as
  [SegmentedControl](crates/fern-widgets/src/segmented_control.rs) (same
  widget, Int UI calls it SegmentedControl, Material calls it SegmentedButton).
  515 lines, 7 tests, `Signal<usize>` driven, `RadioGroup` a11y role.
- **TabWidget / TabBar** — shipped as
  [tab_widget.rs](crates/fern-widgets/src/tab_widget.rs) (tab headers + Switcher).
- **TreeView, ListView, ScrollArea, ScrollBar, SplitView, Dialog, Popover,
  Snackbar, Wizard, MenuBar, MenuItem, MenuList, Breadcrumb, TitleBar, Link,
  Accordion, Badge, ProgressBar, StatusBar, Toolbar, Card, Panel, Repeater,
  Avatar** — all present. ScrollArea/ScrollBar were missing from an earlier
  inventory pass; they do exist. Avatar (Gap 1) shipped after this document
  was originally written; details remain in Gap 1 below.

Full primitive set ([primitives/](crates/fern-widgets/src/primitives/)):
HStack, VStack, ZStack, Grid, Wrap, Center, Expand, AspectRatio, MinSize,
MaxSize, FixedSize, Padding, Spacer, TrackSize, TextWidget, IconWidget,
RectWidget, Divider, Switcher, FocusRing, FormLayout, MasonryLayout,
ImageWidget, ImageMask, ValidationStrip, TextInputField.

Bonus widgets shipped that were not in the original gap list (so they
don't get re-requested):
[ValidationStrip](crates/fern-widgets/src/primitives/validation_strip.rs),
[PopoverButton](crates/fern-widgets/src/popover_button.rs),
[PrivacySettings](crates/fern-widgets/src/privacy_settings.rs),
[ShortcutSettings](crates/fern-widgets/src/shortcut_settings.rs),
[DateRangeEdit](crates/fern-widgets/src/date_range_edit.rs),
[BuiltInButton](crates/fern-widgets/src/built_in_button.rs) (internal-use
icon button embedded inside other widgets like TextInput's clear-X), and
the full
[animations/](crates/fern-widgets/src/animations/) wrapper family (Blur,
Collapse, Crossfade, Cycle, Fade, Pulse, Rotate, Scale, Shake, Slide,
SmoothSize).

---

## Gap 1 — Missing from Section 27

- **Avatar** (§27.5) — **Shipped** as
  [Avatar](crates/fern-widgets/src/avatar.rs). Circular / rounded-square /
  square user-identity widget. Image source path uses CPU-side
  anti-aliased alpha masking at construction time (4× super-sampling)
  so circular images compose with the existing rect-only `Canvas::set_clip`
  — no canvas-API change required. Initials path uses the theme's
  Okabe-Ito `chart_palette` for hash-derived tints (FNV-1a 64), with
  WCAG-luminance auto-contrast for the text colour. Builder surface
  covers `with_initials` / `with_name` (auto-derives `"Jane Doe" → JD`,
  `"jane.doe@x.com" → JD`, Unicode-safe) / `with_image` /
  `from_raw_image`; four discrete sizes (24/32/48/64) plus
  `Custom(px)`; three shapes (Circle / RoundedSquare / Square); four
  presence states (Online / Offline / Away / Busy) plus `Custom` with
  an a11y label, positionable to any of the four corners; optional
  outer ring; `image_visible` reactive prop swaps in the initials
  fallback without a layout shift; and `on_activate_fn` promotes the
  a11y role to `Role::Button` with `Action::Click`/`Focus` and a
  `Pointer` cursor. New `AvatarStyle` slot in
  [components.rs](crates/fern-tokens/src/components.rs). Mask helper
  factored into [avatar/mask.rs](crates/fern-widgets/src/avatar/mask.rs)
  for testability. WidgetCatalog entry registered for the previewer
  (12 named variants). Showcase added to the
  [widget_catalog](examples/widget_catalog/src/main.rs) display section
  (sizes × shapes × presence + hash-tint variety + clickable + ring).

Everything else in §27.1–27.9 ships. §27.10 (text editing) is postponed.

---

## Gap 2 — Standard desktop widgets missing (Qt-aligned)

The big ones — these are things users reasonably expect from a desktop toolkit
and Qt offers them as `QFoo`.

1. **TableView / TableWidget** — `QTableView` + `QTableWidget`. **Shipped** as
   [TableView](crates/fern-widgets/src/table_view.rs) (multi-column,
   virtualized, sort/filter via `SortFilterListModel`, drag-resize +
   drag-reorder of columns, pinned Leading/Trailing, cell-level + row-level
   selection, full keyboard nav, edit hooks, row drag-drop reorder, full a11y
   tree). [TreeTable](crates/fern-widgets/src/tree_table.rs) ships the
   hierarchical variant.

2. **HeaderView** — Absorbed into TableView's column model
   ([table_view/column.rs](crates/fern-widgets/src/table_view/column.rs) +
   [header.rs](crates/fern-widgets/src/table_view/header.rs)); not shipped as
   a standalone widget and not planned to be — TableView and TreeTable cover
   every real use case.

3. **GroupBox** — **Shipped** as
   [GroupBox](crates/fern-widgets/src/group_box.rs).

4. **FormLayout** — **Shipped** as
   [FormLayout](crates/fern-widgets/src/primitives/form_layout.rs).

5. **ButtonGroup** — `QButtonGroup`. Coordinator (not a widget) that
   enforces mutual exclusion across a set of otherwise independent
   RadioButtons living in different parts of the tree. The existing
   [RadioButton](crates/fern-widgets/src/radio_button.rs) +
   [RadioGroup](crates/fern-widgets/src/radio_group.rs) handle the
   same-parent case via a shared `Signal<T>`; a cross-subtree coordinator
   that feeds a11y `set_member_of` is still missing. **Low effort**, only
   ship if a real use case appears.

6. **Calendar widget** — `QCalendarWidget`. **Shipped** as
   [Calendar](crates/fern-widgets/src/calendar.rs). Month grid with
   navigation header, weekday row, 6×7 day cells, today ring, optional
   ISO week-number column, optional Today footer button. Single mode
   (`Calendar::single(Signal<Option<Date>>)`) and Range mode
   (`Calendar::range(Signal<Option<DateRange>>)`). Keyboard navigation
   matches the WAI-ARIA grid pattern (arrows, Home/End, Ctrl+Home/End,
   PageUp/Down, Shift+PageUp/Down for ±year, Enter/Space to commit,
   T to jump to today). AccessKit `Role::Grid` container with
   `Role::ColumnHeader` weekday cells and `Role::GridCell` per day,
   `Live::Polite` for month-change announcements, `aria_current=Date`
   on the focused cell. Locale-derived first day of week + format
   pattern (overridable via builder); month/weekday names via Fluent
   keys (`calendar-*` in `crates/fern-widgets/locales/`). Backed by
   `jiff` civil types via the `common::datetime` shared module. Demo:
   [datetime_pickers](examples/datetime_pickers/).

7. **ColorPicker (swatch + HSV canvas)** — **Shipped** as
   [ColorPicker](crates/fern-widgets/src/color_picker.rs) (with
   `ColorPickerLayout`), [ColorEdit](crates/fern-widgets/src/color_edit.rs),
   and [HexColorInput](crates/fern-widgets/src/hex_color_input.rs) — the hex
   field landed alongside the rest. Gated behind the `rich-text` feature.

8. **DockWidget + dock area** — `QDockWidget` + `QMainWindow`'s dock area.
   A panel that can be docked to any window edge, floated as a top-level
   window, tabbed with sibling docks, and saved/restored as a layout.
   IntelliJ's tool-window system is the same idea. **Large effort** — this is
   a layout subsystem (drag-to-dock regions, stripe buttons, layout
   persistence), not a single widget.

9. **CommandLinkButton** — `QCommandLinkButton`. Large button with icon,
   primary title, and descriptive subtitle. Used for wizard landing choices
   ("Create a new project" / "Open existing…"). Existing
   [Wizard](crates/fern-widgets/src/wizard.rs) would benefit.

10. **ToolBox** — `QToolBox`. Vertically stacked collapsible pages, exactly
    one expanded at a time. **Shipped** as a dedicated widget at
    [crates/fern-widgets/src/tool_box.rs](crates/fern-widgets/src/tool_box.rs)
    (not an Accordion flag — the state-ownership and Int UI visual
    differences ruled that out; see the tool_box plan file for the reasoning).

11. **DialogButtonBox** — Subsumed by
    [MessageBox](crates/fern-widgets/src/message_box.rs), which exposes
    `ButtonRole`, `StandardButton`, `MessageBoxButton`, `MessageBoxButtons`,
    and `MessageBoxResult` — semantic-role buttons with platform-aware
    ordering and default/cancel wiring. If plain
    [Dialog](crates/fern-widgets/src/dialog.rs) needs the same convenience
    for non-message use cases, lift the helper out of MessageBox; otherwise
    consider this gap closed.

---

## Gap 3 — Int UI / Jewel additions (not in Qt)

1. **IconButton / ActionButton** — **Covered by Button.**
   [Button](crates/fern-widgets/src/button.rs) supports
   `ButtonVariant::Flat` (borderless, transparent at idle, `surface_hover`
   on hover) and `IconLocation::IconOnly` (icon with no label, no
   label-layout overhead). The combination
   `Button::new("").style(Flat).icon(icon, IconLocation::IconOnly)` is the
   Int UI IconButton; the toolbar / inline pattern is already supported.
   A separate widget is only worth shipping if profiling shows the unused
   label slot is a real cost — currently no evidence it is. The
   [BuiltInButton](crates/fern-widgets/src/built_in_button.rs) widget is an
   internal-use icon button for embedding inside other widgets (TextInput's
   clear-X, search affordances, etc.) — not a public top-level IconButton.
   **Closed unless evidence forces a split.**

2. **CircularProgressIndicator / Spinner** — **Shipped** as
   [Spinner](crates/fern-widgets/src/spinner.rs). Animated arc backed by
   the shader-driven `AnimatedQuadKind::SpinnerArc` pipeline (~one
   uniform write + one `draw_indexed` per frame, no `paint()` re-runs).
   Honours `prefers-reduced-motion` with a static three-quarter arc
   fallback. Distinct from linear ProgressBar; for unknown-duration
   async work.

3. **Banner / inline callout** — Persistent inline info/warning/error strip
   (not a toast). Status color + icon + message + optional action buttons.
   Tokens exist (`status_*_bg`, `status_*_fg`). Distinct from
   [Snackbar](crates/fern-widgets/src/snackbar.rs) (transient, corner) and
   Dialog (modal). **Low effort.**

4. **GroupHeader** — Horizontal section header: label + trailing rule line.
   Int UI uses it to segment settings pages and forms. Trivial composite
   (HStack → TextWidget + Expand + Divider). **Trivial.**

5. **Chip** — Interactive pill, distinct from the existing
   [Badge](crates/fern-widgets/src/badge.rs) (which is passive/display-only).
   Selectable, toggleable, or dismissable with trailing close icon. Used
   for tag lists, filter bars, multi-select display. **Low effort.**

6. **Rich Tooltip (with sticky-on-dwell)** — **Shipped.** The
   [tooltip system](crates/fern-widgets/src/tooltip.rs) supports rich
   content (title + body + inline markup + shortcut hint + "more"
   disclosure), a `TooltipRegistry` for app-wide reusable tooltip entries,
   inline `TooltipContent` for one-offs, sticky-dwell promotion (2 s of
   continuous hover flips the overlay from `Role::Tooltip` to non-modal
   `Role::Dialog` with click-outside / Escape dismissal), nested tooltips
   over links inside sticky tooltips with cascading dismissal, and the
   prefers-reduced-motion fallback. Surfaced on widgets via
   `Button::rich_tooltip(key)` / `rich_tooltip_content(content)` and the
   matching builder methods on other widgets.

7. **TabStrip** — **Shipped** as
   [`TabBar<T>`](crates/fern-widgets/src/tab_widget/bar.rs) — the public
   `ListModel<T>` + `TabDelegate<T>`-driven header strip, usable
   stand-alone when the content lives in a different panel or window.
   `TabWidget` is the all-in-one `TabBar + Switcher` composition over
   the same selection signal.

8. **Disclosure triangle** — Standalone chevron toggle bound to `Signal<bool>`,
   with animated rotation. Accordion and TreeView each re-implement this
   inside themselves; exposing it as a primitive lets custom widgets reuse
   the rotation and a11y.

9. **Balloon notification** — Stackable, sticky, action-rich notification —
   the "upgrade path" from Snackbar. Snackbar is single, auto-dismissing,
   message-only; Balloon is a queue of richer entries. Could share
   `NotificationStyle` tokens.

10. **SplitButton** — IntelliJ
    [Split Button](https://plugins.jetbrains.com/docs/intellij/split-button.html).
    A button visually divided into two adjacent regions: the left side is
    the **default action** (acts like a regular Button), and the right side
    is a small chevron region that opens a dropdown **menu of related
    actions**. Picking an action from the dropdown can also promote it to
    become the new default for the session (Int UI's "remember last used"
    convention) — a `Signal<usize>` for the selected action index makes
    this trivial. Composition: `HStack(MainButton + Divider + ChevronTrigger)`
    inside a single border frame so it reads as one control. The chevron
    region opens a [MenuList](crates/fern-widgets/src/menu_list.rs) via the
    overlay system. Distinct from ComboBox (which is selection-only) and
    from a regular Button + adjacent Menu (no shared frame, no default-action
    semantics). Accessibility: main region is `Role::Button`; chevron is
    `Role::Button` with `HasPopup::Menu`; the two share a group label.
    A new `SplitButtonStyle` slot in
    [components.rs](crates/fern-tokens/src/components.rs) is the only token
    addition. **Low effort.**

---

## Gap 3.5 — Layouts

The current primitive set is unusually complete for a young framework —
HStack/VStack/ZStack, Grid, Wrap (FlowLayout), Center/Expand, AspectRatio,
the size constraint family (MinSize/MaxSize/FixedSize), Padding/Spacer,
TrackSize, plus Switcher for stacked content. A few standard layout
primitives are still missing:

1. **FormLayout** — **Shipped.** See Gap 2 item 4.

2. **NavigationSplitView / TwoPane / ThreePane** — adaptive multi-pane
   shell layout (sidebar / content / detail). SwiftUI's
   `NavigationSplitView`. Differs from the existing
   [SplitView](crates/fern-widgets/src/split_view.rs) (which is a fixed
   2-pane resizable splitter) by being responsive: at narrow widths it
   collapses panes into a stack with back-navigation; at wide widths it
   shows all panes side-by-side.

3. **MasonryLayout** — **Shipped** as
   [MasonryLayout](crates/fern-widgets/src/primitives/masonry.rs).
   Variable-height grid packing into the shortest column (Pinterest-style).

The Border layout (north/south/east/west/center) from Java AWT is
adequately expressed today as `VStack(top, HStack(left, center, right),
bottom)` and doesn't need its own primitive. **AnchorLayout**
(Qt `QGraphicsAnchorLayout`) is intentionally out of scope — overlaps
with HStack/VStack and adds conceptual weight no real use case has
required.

---

## Gap 3.6 — Media and visualization

These don't fit cleanly under "widgets" or "layouts" and are easy to
overlook in a catalog.

1. **Image** — **Shipped** as
   [ImageWidget](crates/fern-widgets/src/primitives/image_widget.rs) (with
   `ImageFit`) and the related
   [ImageMask](crates/fern-widgets/src/primitives/image_mask.rs) primitive
   (`ImageMaskShape`) used by Avatar and other masked-image patterns.

2. **Charts (2D)** — **Shipped** as a dedicated
   [fern-charts](crates/fern-charts/src/) crate (sits at the same tier as
   fern-widgets, no dep on widgets). Current catalog:
   - [BarChart](crates/fern-charts/src/bar_chart.rs) — vertical or
     horizontal bars, single or grouped series, optional value labels,
     axis labels, and grid lines.
   - [LineChart](crates/fern-charts/src/line_chart.rs) — points
     connected by polylines, single or multiple series, optional area
     fill, axis labels, grid lines, hover tooltips on data points.
   - [PieChart](crates/fern-charts/src/pie_chart.rs) — pie + donut
     variants with a center slot (donut hole content).

   Rendered via the existing `Path` + `fill_path`/`stroke_path` Canvas
   API. Shared infrastructure: [series.rs](crates/fern-charts/src/series.rs)
   (`ChartSeries<T>`, Signal-bound), [axis.rs](crates/fern-charts/src/axis.rs)
   (tick generators), [legend.rs](crates/fern-charts/src/legend.rs),
   [palette.rs](crates/fern-charts/src/palette.rs),
   [layout.rs](crates/fern-charts/src/layout.rs). Design + roadmap:
   [docs/plans/charts-plan.md](docs/plans/charts-plan.md).

   Explicitly **not** in scope: scatter, heatmap, candlestick, radar,
   sankey, sunburst, treemap. The focused catalog (bar/line/pie) avoids
   the "tiny matplotlib" trap.

---

## Gap 3.7 — Refinements to existing widgets

These are not new widgets but loosen constraints on existing ones to enable
modern composable patterns.

1. **Composable MenuList** — Layout side **shipped**:
   [MenuList](crates/fern-widgets/src/menu_list.rs)'s `.item(...)`
   already takes `impl Widget + 'static`, so arbitrary controls
   (sliders, swatch rows, search fields, recent-file rows) compose
   directly. **Still open**: keyboard / focus generalization for non-
   `MenuItem` children — the `KeyboardHighlightWrapper` was originally
   wired to `MenuItem` only; verify it skips non-focusable children
   gracefully and that "any child marked `focusable(true)`" is
   highlightable via Arrow Up/Down navigation. Accessibility: arbitrary
   children keep their own role; MenuList itself stays `Role::Menu`.
   **Low-to-medium effort** — layout is done, only the keyboard/focus
   generalization remains, and it's a verification pass plus possibly a
   small fix.

2. **(Note on Popover)** — No change needed.
   [Popover](crates/fern-widgets/src/popover.rs) already accepts arbitrary
   `impl Widget + 'static` content via `Popover::new(label, content)`,
   supports a custom trigger via `.trigger(widget)`, configurable
   placement, dismissal behavior, and optional caret/arrow. Three
   "big-button-opens-a-File-menu" sizes map to existing tools, no widget
   gap:

   - **Small** — classic File menu (Open / Save / Save As / Recent ▸ /
     Exit): use [MenuList](crates/fern-widgets/src/menu_list.rs), via
     MenuBar or directly via Popover-with-MenuList-content.
   - **Medium** — rich anchored panel with sections, thumbnails, search,
     or master/detail-on-hover behavior (the Office 2007 round-button
     menu is the canonical example): use **Popover with custom
     content**. See the composition example at the end of this file.
   - **Large** — Office Backstage / IntelliJ Welcome screen (fullscreen
     panel that takes over the window, sidebar of action categories on
     the left, content pane on the right): **not a Popover** — express
     it as a [Switcher](crates/fern-widgets/src/primitives/) at the root
     of the window swapping between "main view" and "backstage view."

---

## Gap 4 — Text-input dependent

The text input milestone has landed. Most of this gap is closed; the
remaining items are listed under "Still open" below.

### The text family itself

- **TextField** (`QLineEdit`) — **Shipped** as
  [TextInput](crates/fern-widgets/src/text_input.rs) (with `ValidationState`)
  on top of the
  [TextInputField](crates/fern-widgets/src/primitives/text_input_field.rs)
  primitive. Gated behind the `rich-text` feature.
- **TextArea** (`QPlainTextEdit`) — Covered by
  [RichTextEditor](crates/fern-widgets/src/rich_text.rs) running in plain
  mode (`ScrollPolicy` configurable). No separate `TextArea` widget.
- **RichTextView** — Covered by
  [RichTextEditor](crates/fern-widgets/src/rich_text.rs) configured
  read-only (selection/copy, image and link click). `rich-text` feature-gated.
- **RichTextEditor** — **Shipped** as
  [RichTextEditor](crates/fern-widgets/src/rich_text.rs). Full editing
  surface with IME, formatting commands, undo/redo, intrinsic-mode sizing
  via `.min_lines(n)` / `.max_lines(n)`.

### Widgets that embed a TextField

- **SpinBox / DoubleSpinBox** — **Shipped** as
  [SpinBox](crates/fern-widgets/src/spin_box.rs) (with `WrapMode`,
  `StepType`, `ButtonLayout`, `WheelMode`, `WidthPolicy`). Demo:
  [examples/spin_box](examples/spin_box/).
- **DateEdit / TimeEdit / DateTimeEdit** — **Shipped** as
  [DateEdit](crates/fern-widgets/src/date_edit.rs),
  [TimeEdit](crates/fern-widgets/src/time_edit.rs), and
  [DateTimeEdit](crates/fern-widgets/src/date_time_edit.rs). Bonus:
  [DateRangeEdit](crates/fern-widgets/src/date_range_edit.rs) for two-date
  ranges. Each binds to a nullable signal with a `::required(...)`
  constructor; DateEdit ships with a trailing calendar-icon trigger that
  opens the Calendar widget as a popover. Locale-derived format patterns,
  preview-pass arrow keys, dedicated AccessKit roles. Demo:
  [datetime_pickers](examples/datetime_pickers/).
- **HexColorField** — **Shipped** as
  [HexColorInput](crates/fern-widgets/src/hex_color_input.rs); part of the
  ColorPicker family (Gap 2 item 7).

### Still open

- **SearchField** — specialized TextField with magnifier icon, clear button,
  history dropdown, optional scoped-search chips. Int UI's is distinctive.
- **Path / FilePicker field** — TextField + browse button that opens a file
  dialog. Native file dialog backend already lands via
  `EventContextFileDialogExt`; this is the matching field widget.
- **EditableComboBox** — ComboBox with freeform typing. Current
  [ComboBox](crates/fern-widgets/src/combo_box.rs) is selection-only.
- **FontComboBox** — ComboBox pre-populated with installed font families,
  each rendered in its own font.
- **InputDialog** — `QInputDialog` equivalent. Modal with a single input
  field. [MessageBox](crates/fern-widgets/src/message_box.rs) covers
  buttons-only modals; this is the input variant.

---

## Gap 5 — Explicit non-goals

Listed so they're consciously out of scope, not accidentally forgotten:

- **LCDNumber** (`QLCDNumber`) — seven-segment display. Niche.
- **MDI area** (`QMdiArea`) — MDI child-window workspace. Out of scope;
  DockWidget covers the real use case.
- **GraphicsView / QGraphicsScene** — a whole interactive drawing canvas
  subsystem, not a widget. Out of scope for FernUI's retained widget model.
- **SizeGrip** (`QSizeGrip`) — native platform resize is handled by winit;
  the existing [TitleBar](crates/fern-widgets/src/title_bar/) system owns
  client-area resize.
- **FocusFrame** (`QFocusFrame`) — already covered by the
  [FocusRing](crates/fern-widgets/src/primitives/) primitive.
- **StackedWidget** (`QStackedWidget`) — already covered by Switcher.

---

## Recommended phasing

**Phase A — Remaining low-effort wins.** Chip, Banner, Disclosure triangle,
CommandLinkButton. Each is days, not weeks, and each unlocks real visual
polish. (Image, GroupHeader, IconButton-as-Button, SplitButton, Spinner,
Avatar, and the Rich Tooltip extension have shipped.)

**Phase B — Remaining mid-effort essentials.** Balloon notification,
ButtonGroup cross-subtree coordinator, composable-MenuList keyboard /
focus generalization for non-`MenuItem` children. (TabStrip-as-`TabBar`,
GroupBox, FormLayout, Calendar, ColorPicker, ToolBox, MasonryLayout have
all shipped.)

**Phase C — Remaining large structural work.** NavigationSplitView
(adaptive multi-pane shell); DockWidget + dock-area layout subsystem.
(TableView/TreeTable and the **fern-charts crate** have shipped; see
Gap 2 item 1 and Gap 3.6 item 2.) Each remaining item is a milestone,
not a widget, and needs its own design pass before implementation.

**Phase D — Remaining text-input-dependent.** SearchField, Path/FilePicker
field, EditableComboBox, FontComboBox, InputDialog. (TextInput,
RichTextEditor, SpinBox, DateEdit/TimeEdit/DateTimeEdit/DateRangeEdit,
HexColorInput have all shipped.)

---

## Critical files (reference, read-only)

- [docs/fern-ui-architecture.md §27](docs/fern-ui-architecture.md#L2721)
- [crates/fern-widgets/src/lib.rs](crates/fern-widgets/src/lib.rs)
- [crates/fern-widgets/src/primitives.rs](crates/fern-widgets/src/primitives.rs)
- [crates/fern-tokens/src/theme.rs](crates/fern-tokens/src/theme.rs)
- [crates/fern-tokens/src/components.rs](crates/fern-tokens/src/components.rs)
- [crates/fern-widgets/src/segmented_control.rs](crates/fern-widgets/src/segmented_control.rs)
  (so SegmentedButton isn't re-requested)

## Composition examples (no new widgets)

These are illustrative usage patterns showing how existing pieces compose
into recognizable UI shapes. They are not gaps and not widgets to
implement — they're examples to keep handy when scoping the catalog,
to confirm that a given recognizable pattern is already buildable.

### Office 2007 round-button File menu

The canonical "click an orb in the top-left, get a rich anchored panel
with file commands left, hover-driven context pane right, footer
actions": pure composition over Popover + Switcher + Signal-bound
hover. No new widget needed.

```text
Popover(trigger: round button, content:
  VStack(
    HStack(
      VStack of hoverable action rows  ← left column
            (each binds .on_hover to a hovered_index Signal:
             New, Open, Save, Save As ▸, Print ▸, Send ▸, Close),
      Switcher(driven by hovered_index) ← right pane
            with one child per left action: Recent Documents
            (default), Save-As format list, Print preview shortcuts,
            Send-to targets, …
    ),
    Divider,
    HStack(Spacer, Button("Word Options"), Button("Exit Word"))
                                             ← footer
  )
)
```

Every part exists today: Popover (anchored overlay with arbitrary
content + custom trigger), VStack/HStack, Switcher, Signal-bound hover,
Divider, Button. The left-column rows are slightly larger than plain
MenuItems; reuse MenuItem with `.on_hover()` attached, or a small
custom row widget similar to a sized-down CommandLinkButton (Gap 2 item
9). If this exact pattern shows up enough in real apps a thin ~100-line
convenience wrapper could bundle it later, but that's a deferred
ergonomics decision, not a catalog gap.

---

## Verification

Survey deliverable — verification = user review, pruning, and selection of
phase(s) to pursue. Each approved widget then gets its own implementation
plan. No code or test changes are proposed here.
