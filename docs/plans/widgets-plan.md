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
RectWidget, Divider, Switcher, FocusRing.

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

1. **TableView / TableWidget** — `QTableView` + `QTableWidget`. FernUI has
   ListView and TreeView but no 2D grid view. Needed for any spreadsheet-like
   data, inspector panes, settings tables. Depends on a `TableModel` data
   source (analogous to the existing `ListModel` / `TreeModel`) and a new
   `HeaderView` widget for sortable, resizable, reorderable columns. **Large
   effort**, high value — this is the single biggest functional gap for
   data-driven apps.

2. **HeaderView** — `QHeaderView`. Column header strip with click-to-sort,
   drag-to-resize, drag-to-reorder, section visibility menu. Needed as
   soon as Table lands, also usable as a standalone column header on top of
   ListView when used in "details" (columns) mode.

3. **GroupBox** — `QGroupBox`. Titled frame around a cluster of controls,
   optionally checkable (the title contains a checkbox that enables/disables
   the whole group). Distinct from Panel: GroupBox draws the title
   notched into the top border line. Common in preference dialogs and forms.
   **Low effort.**

4. **FormLayout** — `QFormLayout`. Two-column layout for label/field rows,
   with configurable label alignment (right-aligned vs top, wrap or truncate)
   and consistent baseline alignment across rows. Currently expressible as a
   Grid + manual baseline tweaking, but a dedicated primitive encodes the
   desktop form convention. **Medium effort.**

5. **ButtonGroup** — `QButtonGroup`. Coordinator (not a widget) that
   enforces mutual exclusion across a set of otherwise independent
   RadioButtons living in different parts of the tree. The existing
   [RadioButton](crates/fern-widgets/src/radio_button.rs) uses a shared
   `Signal<T>` which handles the common case, but a cross-subtree coordinator
   makes the intent explicit and feeds a11y `set_member_of`.

6. **Calendar widget** — `QCalendarWidget`. Month grid with day cells, month
   navigation, week number column, today highlight. Standalone, not tied to a
   date field — so it can ship before text-input lands and be wired into a
   date picker later. **Medium effort.**

7. **ColorPicker (swatch + HSV canvas)** — Qt bundles this inside
   `QColorDialog`. The swatch grid and HSV triangle/wheel are standalone
   widgets; only the hex field depends on text input. Ship the swatch and
   HSV canvas now, wire up the hex cell later.

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

11. **DialogButtonBox** — `QDialogButtonBox`. Not Qt-only — every mature
    desktop toolkit has the same concept under different names: GTK
    `GtkDialog` action area, Cocoa `NSAlert` button arrangement, WinUI
    `ContentDialog` primary/secondary/close. The reason it exists: dialog
    button **ordering is platform-specific** (macOS puts the destructive
    action on the left, Windows/Linux on the right; the "default" button
    is highlighted differently on each platform; Tab order follows the
    visual order). Today the existing
    [Dialog](crates/fern-widgets/src/dialog.rs) probably ships an HStack
    action bar where the developer hard-codes the order — which silently
    looks wrong on macOS.

    DialogButtonBox is a small helper that takes buttons by **semantic
    role** (`Accept`, `Reject`, `Apply`, `Reset`, `Help`, `Save`, `Open`,
    `Cancel`, `Close`, `Yes`, `No`, `Discard`, `RestoreDefaults`,
    `Destructive`) rather than by position, and re-orders them per
    `Theme::platform_convention()` (or however FernUI exposes the host
    platform). It also wires the **default button** (Enter triggers
    `Accept`/`Save`/`Yes`) and the **cancel button** (Escape triggers
    `Reject`/`Cancel`/`Close`) automatically, plus the focus order. Not
    overkill — this is the kind of small helper that prevents a class of
    cross-platform polish bugs you don't notice until a macOS user
    complains. **Low effort**, ships alongside any Dialog improvements.

---

## Gap 3 — Int UI / Jewel additions (not in Qt)

1. **IconButton / ActionButton** — Borderless square icon-only button, flat
   until hover. Matches
   [IconButtonStyle](crates/fern-tokens/src/components.rs#L42-L61) which
   is already defined in tokens (sizes 22/24/30). A `ButtonStyle::Flat`
   Button with icon-only works but wastes MinSize/padding budget and carries
   label-layout overhead. **Low effort.**

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

6. **Rich Tooltip (with sticky-on-dwell)** — Extend the existing 104-line
   [Tooltip](crates/fern-widgets/src/tooltip.rs) (currently text-only,
   no tests) to support title + body + shortcut hint + optional
   link/action. The `tooltip_shortcut` token already exists at
   [theme.rs:83](crates/fern-tokens/src/theme.rs#L83).

   **Sticky-dwell behavior.** After the tooltip has been visible for 2s of
   continuous pointer dwell, it promotes itself to a "sticky" state:
   - While dwelling, a small radial/linear fill indicator animates from 0
     to 1 over the 2s window, visually counting down to the promotion.
     The indicator sits in a corner of the tooltip so it doesn't crowd the
     content. Uses `Signal<f32>::animate_to()` + `AnimationScheduler`.
   - Once promoted, the tooltip no longer auto-dismisses on pointer-out.
     It behaves like a Popover: `DismissBehavior::ClickOutside` takes over,
     and the user must click elsewhere (or press Escape) to close it.
   - Sticky tooltips can host a **"more" disclosure** — a trailing
     Accordion (or the Disclosure-triangle primitive from Gap 3 item 8)
     that expands to reveal a longer explanation, illustrations, or
     additional links. Collapsed by default so the initial reveal stays
     lightweight; the user opts in to the long form.
   - **Nested tooltips.** Links inside a sticky tooltip are themselves
     hoverable. Hovering a link inside a sticky tooltip opens a child
     tooltip anchored to that link, which can itself become sticky after
     its own 2s dwell, which can itself contain links that spawn further
     nested tooltips — a cascade. Implementation hook: the existing
     overlay system already supports parent→child overlay relationships
     with cascading dismissal; reuse that. Closing a parent sticky
     tooltip closes its entire nested chain. Escape closes the innermost
     open tooltip first (onion-peel), matching the MenuBar/ContextMenu
     cascade pattern.
   - Non-sticky tooltips keep current behavior (show on hover-in, hide on
     hover-out), so casual hovers don't get hijacked.
   - Accessibility: while sticky, the tooltip's a11y role flips from
     `Role::Tooltip` to `Role::Dialog` (non-modal) since it now behaves
     like a persistent panel. The "more" disclosure declares
     `set_expanded`.

   **Effort:** low for base rich content, medium once sticky-dwell + dwell
   indicator + dismiss coordination are wired in. State machine to design:
   `Hidden → Hovering(elapsed) → Sticky → Hidden` with timers and click
   dismissal.

7. **TabStrip** — Bare strip of tab headers decoupled from the content
   Switcher. Lets the header row drive arbitrary external state (e.g., an
   open-file list) without being welded to a single Switcher child.
   [TabWidget](crates/fern-widgets/src/tab_widget.rs) refactor: extract the
   header HStack as TabStrip, rebuild TabWidget as `TabStrip + Switcher`.

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

1. **FormLayout** — already listed in Gap 2 item 4 as `QFormLayout`. Two
   columns of label/field rows with consistent baseline alignment. The
   single most common form-building primitive in Qt-style apps.

2. **NavigationSplitView / TwoPane / ThreePane** — adaptive multi-pane
   shell layout (sidebar / content / detail). SwiftUI's
   `NavigationSplitView`. Differs from the existing
   [SplitView](crates/fern-widgets/src/split_view.rs) (which is a fixed
   2-pane resizable splitter) by being responsive: at narrow widths it
   collapses panes into a stack with back-navigation; at wide widths it
   shows all panes side-by-side.

3. **MasonryLayout** — variable-height grid that packs children into the
   shortest column (Pinterest-style). Useful for image galleries, card
   walls, snippet collections. Niche but cheap to implement once the
   measurement pass is generalized.

4. **AnchorLayout** — Qt's `QGraphicsAnchorLayout`-style anchored
   positioning ("right of A, vertically centered with B"). Powerful but
   conceptually heavy and overlaps with what HStack/VStack already cover.
   **Skip unless a real use case shows up** — listing here only for
   completeness against Qt.

The Border layout (north/south/east/west/center) from Java AWT is
adequately expressed today as `VStack(top, HStack(left, center, right),
bottom)` and doesn't need its own primitive.

---

## Gap 3.6 — Media and visualization

These don't fit cleanly under "widgets" or "layouts" and are easy to
overlook in a catalog.

1. **Image** — Display a static image (PNG, JPEG, WebP, optionally SVG
   via the existing path rasterizer). The architecture doc §27 mentions
   the rendering pipeline already exists (`Canvas::draw_image`,
   `ImageManager`), but **no public Image widget surfaces it**. Avatar
   (Gap 1) needs this; Banner can use it for hero icons; the existing
   [RichTextView](crates/fern-widgets/src/) / `RichTextEditor` (when
   they land) need it for inline images. Properties: source (path or
   embedded bytes), fit mode (`Fit::Contain` / `Cover` / `Fill` / `None`),
   alignment, optional corner radius and tint color. **Low effort** — the
   underlying texture pipeline exists; this is a thin Widget impl over it.

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

1. **Composable MenuList** — Today
   [MenuList](crates/fern-widgets/src/menu_list.rs) is documented as "a
   vertical container for MenuItem and MenuSeparator widgets" and its
   `.item()` method downcasts to `MenuItem`. Modern context menus mix
   menu items with arbitrary controls inline:
   - **Volume slider** in a system tray menu (macOS audio menu, Linux
     pavucontrol)
   - **Color swatch row** in a formatting context menu (Word, Pages,
     Apple Notes)
   - **Brightness slider, zoom row, font-size stepper** in editor menus
   - **Search field** at the top of long menus (IntelliJ Find Action,
     macOS Help menu)
   - **Recently used files row**, **avatar/user header**, **mini chart
     preview**, **inline checkbox group**

   Relax MenuList to accept `impl Widget + 'static` children directly
   (no `MenuItem` downcast), with a parallel `.item()` keeping the
   typed-MenuItem path for the common case. Keyboard navigation
   (Arrow Up/Down, Enter) needs to skip non-focusable children
   gracefully — the existing `KeyboardHighlightWrapper` only highlights
   `MenuItem` rows today; generalize it to "any child marked
   `focusable(true)`." Accessibility: arbitrary children keep their own
   role; the MenuList itself stays `Role::Menu`. **Medium effort** —
   the layout side is trivial, the keyboard/focus generalization is
   the real work. Unlocks Phase B widgets like in-menu Slider,
   ColorPicker, search field.

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

## Gap 4 — Postponed (text-input dependent)

These are blocked on the text input milestone. Listed here so the catalog is
exhaustive and nothing is forgotten:

### The text family itself

- **TextField** (Qt: `QLineEdit`) — single-line text input. Foundational.
- **TextArea** (Qt: `QPlainTextEdit`) — multi-line plain text input.
- **RichTextView** — §27.10.1. Read-only rich text with selection/copy, image
  and link click. `[rich-text]` feature-gated; depends on text-typeset.
- **RichTextEditor** — §27.10.1. Full editing surface with IME, formatting
  commands, undo/redo. The most architecturally distinctive widget in
  FernUI; detailed through §27.10.4.

### Widgets that embed a TextField

- **SpinBox / DoubleSpinBox** — `QSpinBox`, `QDoubleSpinBox`. **Shipped** as
  [SpinBox](crates/fern-widgets/src/spin_box.rs) (numeric stepper with
  up/down buttons and an editable text cell). Demo:
  [examples/spin_box](examples/spin_box/).
- **DateEdit / TimeEdit / DateTimeEdit** — `QDateEdit`, `QTimeEdit`,
  `QDateTimeEdit`. Spin-box-style editors for temporal values. The Calendar
  widget (Gap 2 item 6) can ship standalone and plug in as the popover here.
- **SearchField** — specialized TextField with magnifier icon, clear button,
  history dropdown, optional scoped-search chips. IntelliJ's is distinctive.
- **Path / FilePicker field** — TextField + browse button that opens a file
  dialog. Needed by the settings UI.
- **EditableComboBox** — ComboBox where the user can also type a freeform
  value. Qt has `QComboBox::setEditable(true)`; Int UI has it as a separate
  widget. Current [ComboBox](crates/fern-widgets/src/combo_box.rs) is
  selection-only.
- **FontComboBox** — `QFontComboBox`. ComboBox pre-populated with installed
  font families, each rendered in its own font. Selection-only variant could
  ship early; editable variant waits.
- **InputDialog** — `QInputDialog`. Modal with a single input field.
- **HexColorField** — the text input half of the ColorPicker (Gap 2 item 7).

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

**Phase A — Trivial / low-effort wins.** **Image**, GroupHeader,
Chip, IconButton, **SplitButton**, Disclosure triangle,
Banner, Rich Tooltip extension, CommandLinkButton.
(Spinner / CircularProgressIndicator has shipped — see Gap 3 item 2.
Avatar has shipped — see Gap 1.)
Each is days, not weeks, and each unlocks real visual polish.

**Phase B — Mid-effort desktop essentials.** GroupBox, FormLayout,
ButtonGroup, Calendar widget, ColorPicker (sans hex field), TabStrip,
Balloon notification, ToolBox (as Accordion variant), MasonryLayout.

**Phase C — Large structural work.** TableView + HeaderView + TableModel;
NavigationSplitView (adaptive multi-pane shell); DockWidget + dock-area
layout subsystem. (The **fern-charts crate** — BarChart, LineChart,
PieChart — has shipped; see Gap 3.6 item 2.) Each remaining item is a
milestone, not a widget, and needs its own design pass before
implementation.

**Phase D — Blocked on text input.** Entire Gap 4 list. Scheduled after the
text input milestone lands.

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
