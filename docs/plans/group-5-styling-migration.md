# Group-5 styling migration — finishing the four-tier ladder

## Context

FernUI's four-tier styling ladder (tokens → variants → recipes → style
protocols, see [docs/styling-system.md](../styling-system.md)) is
**complete for 16 widgets** — every one of them composes its chrome
through a Tier-3 `*Style` trait, with a default `Recipe*Style` impl and
a typed slot in `theme.style_slots`. No themable widget in that set
self-paints.

That leaves a tail — informally "group 5" — of widgets that still
either call `paint()` directly or read dimension data out of the
legacy `fern_tokens::ComponentStyles` struct. They were deliberately
left for last: some are genuinely specialized, some are popup
containers, some are heavy data widgets. This plan migrates **all of
them**, deletes `ComponentStyles` entirely, and reduces `fern-tokens`
to a pure-data leaf crate.

There is **no deferral in this plan**. Every widget named here gets a
concrete destination. Where a widget turns out *not* to need a style
trait, that is stated as a finding with a reason — not a TODO.

## Precise scope

An audit of all 101 widget files in `crates/fern-widgets/src` against
two signals — presence of a `Widget::paint` method, and reads of
`theme.components.*` — produces the following classification. The
"informal group 5" from the styling review breaks down further once
each widget is actually inspected:

### In scope — gets a Tier-3 style trait

| Widget | Today | Destination |
| --- | --- | --- |
| `MenuList` | `MenuList::paint` draws the drop shadow; bg/border is a child `RectWidget` | routed through `PopoverStyle` (new `PopoverVariant::Menu`) |
| `Dialog` (`ModalContainer`) | `ModalContainer::paint` draws panel surface + border; reads `components.dialog` | `DialogStyle` |
| `Snackbar` (`SnackbarSurface`) | `SnackbarSurface::paint` draws the surface; reads `components.notification` | `SnackbarStyle` |
| `Banner` | `SeverityGlyph::paint` draws the severity icon; body is composition; reads `components.banner` | `BannerStyle` |
| `Avatar` | `Avatar::paint` + `paint_border` + `paint_focus_ring` + `InitialsLeaf::paint`; reads `components.avatar` | `AvatarStyle` |
| `Badge` | no `paint()`; composition; reads `components.badge` | `BadgeStyle` |
| `ProgressBar` | `ProgressBar::paint` draws track + fill; reads `components.progress_bar` | `ProgressBarStyle` |
| `SegmentedControl` | `SegmentedControl::paint` draws selected segment + dividers + focus; reads `components.segmented_control` | `SegmentedControlStyle` |
| `Link` | no `paint()`; composition; reads `components.link` | `LinkStyle` |
| `Calendar` | no `paint()` anywhere — fully composed; reads `components.calendar` | `CalendarStyle` (multi-method) |
| `ColorPicker` | container is composition; `HsvCanvas` / `HueStrip` / `AlphaStrip` / `ColorSwatch` are functional renderers; reads `components.color_picker` | `ColorPickerStyle` (container chrome only — see Hard cases) |
| `SpinBox` | no `paint()`; composes `TextInput` + `StepButton` | `SpinBoxStyle` |
| `DateEdit` / `TimeEdit` / `DateRangeEdit` / `DateTimeEdit` | no `paint()`; compose `TextInput`; read `components.date_edit` / `components.time_edit` | one shared `DateEditStyle` |
| `SearchField` | no `paint()`; composes `TextInput`; `SuggestionPanel` is a popup container; reads `components.search_field` | `SearchFieldStyle` + suggestion panel routed through `PopoverStyle::Menu` |
| `RichTextEditor` | `RichTextEditor::paint` renders text runs / caret / selection | `RichTextEditorStyle` (frame chrome only — see Hard cases) |
| `TableView` | `TableView::paint` + `HeaderCell::paint` + `SortIndicator::paint` + `HeaderRow::paint` + `BodyRow::paint`; reads `components.table` | `TableStyle` (multi-method) |
| `TreeTable` | `TreeTable::paint`; reuses table_view header modules; reads `components.table` | shares `TableStyle` |
| `ListView` | `ListView::paint` draws the drag-insertion line | `ListContainerStyle` (shared) |
| `TreeView` | `TreeView::paint` draws the drag-insertion line | shares `ListContainerStyle` |

### In scope — confirmed *no trait needed* (themed through composition)

These were in the informal group-5 bucket but inspection shows they own
no chrome of their own. They are handled by **deleting** their stale
dim-struct reads and folding any residual layout constants into a
`pub const` block in their own module (the Step-7 pattern). They are
not deferred — they are *finished* by Stage G.

| Widget | Finding |
| --- | --- |
| `MessageBox` | `MessageBox::paint` is empty `{}`. Pure `Dialog` composite — themed automatically once `DialogStyle` lands. |
| `InputDialog` | No `paint()`. Pure `Dialog` composite. |
| `ScrollArea` | `ScrollArea::paint` is empty (`_bounds, _canvas, _ctx`). Its only chrome is the `ScrollBar`, already on `ScrollBarStyle`. No trait. |
| `FilePickerField` | No `paint()`, no `components.*` read. Pure composite of themed `TextInput` + `IconButton`. No trait. |
| `ShortcutSettings` | No `paint()`, no `components.*` read. Pure composite of themed `Button` / `Panel` / `TextInput`. **Not group 5** — mis-bucketed in the review. No trait. |
| `PrivacySettings` | Same as `ShortcutSettings`. No trait. |
| `MenuSeparator` | A 1 px line. Stays a primitive like `Divider`; its colour already resolves from a role. No trait. |

### Explicitly out of scope

| Widget | Reason |
| --- | --- |
| `TitleBar` / `WindowFrame` / `DragRegion` / `ResizeStrip` / `WindowControls` | OS-integration chrome bound to `fern-platform`'s per-OS `PlatformTitleBarHost`. Most of its `paint()` methods are already empty; the live ones (`TitleBar::paint` background, `WindowFrame::paint` resize border) are platform-decoration concerns, not app-theming surface. A `TitleBarStyle` is a separate, later effort with platform constraints — it is **not** part of finishing the four-tier ladder and is excluded here by design, not deferred-with-a-TODO. This plan does, however, fold `TitleBar`'s residual layout constants into a `pub const` block in Stage G so `ComponentStyles` can still be deleted. |

After this plan, **every widget that owns chrome is on a Tier-3 style
trait, and `fern_tokens::ComponentStyles` no longer exists.**

## Design principles

Carried verbatim from the completed 16-widget migration — no new
conventions are introduced:

1. **Trait shape.** For widget `Foo`, a trait file lives at
   `crates/fern-core/src/styles/foo_style.rs`:
   ```rust
   pub trait FooStyle: 'static {
       fn make_body(&self, cfg: &FooStyleConfig, ctx: &mut BuildContext) -> WidgetId;
   }
   pub type SharedFooStyle = Rc<dyn FooStyle>;
   ```
   Multi-surface widgets (`Calendar`, `TableView`) carry more than one
   method, exactly as `TabStyle` carries `make_body` + `make_bar`.

2. **Config shape.** `FooStyleConfig` carries *pre-built sub-widget
   `WidgetId`s* (the style arranges chrome around them, never builds
   them), the live interaction state as `Signal<bool>`s, and any
   domain enum the style needs as a hint. The widget builds the parts;
   the style returns the root.

3. **No invented variants.** A `*Variant` enum is added only when there
   is a real design-language axis. Group-5 widgets that already carry a
   domain enum (`AvatarShape`, `BannerSeverity`, `CalendarMode`,
   `ColorPickerLayout`) pass that enum through the config instead — the
   same way `Slider` passes `SliderOrientation`. The only genuinely new
   variant enum in this plan is none: every group-5 widget is either
   variant-free or reuses an existing domain enum.

4. **Recipes are the default impl, not a parallel store.** Each trait
   ships a `RecipeFooStyle` default in
   `crates/fern-widgets/src/styles/recipe_foo_style.rs`. IntUI
   dimension constants move into that module as `pub const` — there is
   no `theme.components.foo` left to read after the widget migrates.

5. **Resolution precedence is fixed.** Every migrated widget resolves
   its style as `self.style_override → theme.style_slots.<slot> →
   RecipeFooStyle::default()`.

6. **Functional renderers are not chrome.** A widget-internal painter
   that renders *domain data* — the HSV gradient of a colour picker,
   the glyph runs of a rich-text editor — is not "look" and does not
   move behind a style trait. The style trait themes the *frame* around
   it. A custom `impl` is still free to replace the whole subtree
   (the trait returns a `WidgetId`), but the default `Recipe*Style`
   keeps composing the built-in functional renderer. This boundary is
   stated per-widget under Hard cases; it is a deliberate design line,
   not a shortcut.

7. **Accessibility never moves.** Style impls decorate; the widget
   keeps its `accessibility(builder)` and all `.access_*` overrides.

## Per-widget specifications

### Stage A — popup-container unification

The styling review left an interim `DropdownShadow` leaf inside
`ComboBox`'s panel because `PopoverStyle` could not yet express a
"menu-flavoured" surface (`PopoverSurface` hard-codes 16 px content
padding and a `surface_main` background). Stage A fixes the root cause
and unifies **all** popup containers under one trait.

**A1 — `PopoverVariant::Menu` becomes real.**
`RecipePopoverStyle::make_body` currently ignores `cfg.variant`.
Parameterize `PopoverSurface` (`crates/fern-widgets/src/popover.rs`)
with three new fields — `content_padding: EdgeInsets`, `background:
SurfaceRole`, `corner_radius: f32` — defaulted from the recipe constants
per variant:
- `Default` / `Tooltip` → `POPOVER_PADDING` (16), `surface_main`,
  `POPOVER_CORNER_RADIUS`.
- `Menu` → `EdgeInsets::ZERO`, `SurfaceRole::Raised`,
  `MENU_POPUP_CORNER_RADIUS` — so menu rows reach the surface edge.
The `attached_shadow_side` / caret logic is shared unchanged.

**A2 — route `MenuList` through `PopoverStyle`.**
`MenuList::build` already creates the bg `RectWidget` child and
`MenuList::paint` only draws the shadow. Replace both: resolve the
popover style, call `make_body` with `PopoverStyleConfig { content:
items_column, variant: PopoverVariant::Menu, placement:
self.attached_side-derived, show_caret: false, .. }`, use the returned
id as the root. Delete `MenuList::paint` and the build-time bg rect.

**A3 — route `ComboBox`'s `DropdownPanel` through `PopoverStyle`.**
Delete the interim `DropdownShadow` leaf added by the styling review
and the build-time bg `RectWidget`; call `PopoverStyle::make_body` with
`variant: Menu` instead. This supersedes that interim commit cleanly.

**A4 — route `SearchField`'s `SuggestionPanel` through `PopoverStyle`.**
`SuggestionPanel` / `SuggestionListBox` are the same popup-container
shape. Same treatment as A3.

**Outcome of Stage A:** one trait (`PopoverStyle`) themes every floating
panel in the framework. `MenuList::paint`, the `DropdownShadow` leaf,
and three hand-rolled bg rects are gone. No new slot — they all use
`theme.style_slots.popover`.

### Stage B — notification surfaces

**B1 — `DialogStyle`.**
```rust
pub struct DialogStyleConfig {
    pub content: WidgetId,        // the DialogContent subtree
    pub has_scrim: bool,          // ModalContainer always true today
}
pub trait DialogStyle: 'static {
    /// The modal panel surface that wraps `content`.
    fn make_panel(&self, cfg: &DialogStyleConfig, ctx: &mut BuildContext) -> WidgetId;
    /// The full-window scrim behind the panel.
    fn make_scrim(&self, ctx: &mut BuildContext) -> WidgetId;
}
```
`ModalContainer::paint` (rounded surface + `border_strong` stroke) and
the scrim become `RecipeDialogStyle`. The `ModalRequest` /
`ModalPresentation` pipeline is untouched — it still mounts a
`ModalContainer`; only `ModalContainer`'s chrome moves. `MessageBox` and
`InputDialog` inherit the new chrome for free (they build `Dialog`
content). `components.dialog` constants move to
`recipe_dialog_style.rs`.

**B2 — `SnackbarStyle`.**
```rust
pub struct SnackbarStyleConfig {
    pub content: WidgetId,        // message + optional action subtree
}
pub trait SnackbarStyle: 'static {
    fn make_body(&self, cfg: &SnackbarStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}
```
`SnackbarSurface::paint` → `RecipeSnackbarStyle`. `components.notification`
constants move to `recipe_snackbar_style.rs`.

**B3 — `BannerStyle`.**
```rust
pub struct BannerStyleConfig {
    pub severity: BannerSeverity,   // existing domain enum, passed as hint
    pub content: WidgetId,          // message + actions
    pub leading_glyph: WidgetId,    // the SeverityGlyph subtree, pre-built
}
pub trait BannerStyle: 'static {
    fn make_body(&self, cfg: &BannerStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}
```
`SeverityGlyph::paint` stays as a small functional glyph painter (it
draws an info/warn/error mark — domain data, principle 6), but its
*colour mapping per severity* is recipe data the style owns. The Banner
strip background, border, and per-severity tint move to
`RecipeBannerStyle`. `components.banner` constants relocate.

New slots: `dialog`, `snackbar`, `banner`.

### Stage C — indicator & control widgets

**C1 — `ProgressBarStyle`.**
```rust
pub struct ProgressBarStyleConfig {
    pub orientation: Orientation,
    pub progress: ProgressKind,     // Determinate(Signal<f32>) | Indeterminate
}
pub trait ProgressBarStyle: 'static {
    fn make_body(&self, cfg: &ProgressBarStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}
```
`RecipeProgressBarStyle` builds the track `RectWidget` and the
determinate fill `RectWidget` (bound to the value signal). **The
shader-quad indeterminate sweep (`AnimatedQuadRegistry`) stays
widget-owned** — registering an animated quad is motion infrastructure,
not chrome (principle 6); the style supplies the sweep's colour recipe,
the widget owns the `draw_animated_quad` call. This split is documented
in `recipe_progress_bar_style.rs`. `components.progress_bar` constants
relocate.

**C2 — `SegmentedControlStyle`.**
```rust
pub struct SegmentedControlStyleConfig {
    pub segments: Vec<WidgetId>,    // pre-built segment label/icon subtrees
    pub selected: Signal<usize>,
    pub is_focused: Signal<bool>,
}
pub trait SegmentedControlStyle: 'static {
    fn make_body(&self, cfg: &SegmentedControlStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}
```
`SegmentedControl::paint` (track, selected-segment surface, inter-segment
dividers, focus ring) → `RecipeSegmentedControlStyle`. `SegmentButton`'s
empty `paint` is deleted. `components.segmented_control` relocates.

**C3 — `BadgeStyle`.** Badge is already pure composition; this is a
fold-the-dims migration. `BadgeStyleConfig { content: WidgetId }`,
`make_body` composes the pill `RectWidget` + padding. `components.badge`
relocates.

**C4 — `LinkStyle`.**
```rust
pub struct LinkStyleConfig {
    pub label: WidgetId,
    pub is_hovered: Signal<bool>,
    pub is_pressed: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_visited: Signal<bool>,
}
pub trait LinkStyle: 'static {
    fn make_body(&self, cfg: &LinkStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}
```
The underline policy and per-state text colour are recipe data.
`components.link` relocates.

**C5 — `AvatarStyle`.**
```rust
pub struct AvatarStyleConfig {
    pub shape: AvatarShape,          // existing domain enum
    pub size: AvatarSize,            // existing domain enum
    pub content: WidgetId,           // image or InitialsLeaf, pre-built
    pub presence: Option<AvatarPresence>,
    pub is_focused: Signal<bool>,
}
pub trait AvatarStyle: 'static {
    fn make_body(&self, cfg: &AvatarStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}
```
`Avatar::paint` (bg shape fill, border ring, presence dot) + the
`paint_border` / `paint_focus_ring` free functions → `RecipeAvatarStyle`.
`InitialsLeaf` stays a functional renderer (it lays out and draws
initials text — principle 6) and is passed in as `content`. The
hash-derived background-colour palette is recipe data the style owns.
`components.avatar` relocates.

New slots: `progress_bar`, `segmented_control`, `badge`, `link`,
`avatar`.

### Stage D — calendar & colour picker

**D1 — `CalendarStyle` (multi-method).**
`Calendar` is fully composed already (no `paint()` anywhere) but its
sub-cells — `DayCell`, `ZoomCell`, `WeekdayHeaderCell`,
`CalendarHeader`, `NavArrow` — bake in IntUI surface/role choices and
read `components.calendar` dims. The trait drives the *cell* chrome:
```rust
pub struct CalendarDayConfig {
    pub label: WidgetId,
    pub state: CalendarDayState,   // Today | Selected | InRange | RangeEnd
                                   // | OutOfMonth | Disabled | Normal
    pub is_hovered: Signal<bool>,
    pub is_focused: Signal<bool>,
}
pub trait CalendarStyle: 'static {
    fn make_day_cell(&self, cfg: &CalendarDayConfig, ctx: &mut BuildContext) -> WidgetId;
    fn make_zoom_cell(&self, cfg: &CalendarDayConfig, ctx: &mut BuildContext) -> WidgetId;
    fn make_header(&self, cfg: &CalendarHeaderConfig, ctx: &mut BuildContext) -> WidgetId;
}
```
`RecipeCalendarStyle` ports the current IntUI look. `components.calendar`
constants (`row_height`, `indent`, grid dims) relocate to
`recipe_calendar_style.rs`. The month/year grid *layout* stays in the
widget; only per-cell chrome moves.

**D2 — `ColorPickerStyle` (container chrome only).**
The four functional renderers — `HsvCanvas` (2-D saturation/value
gradient), `HueStrip`, `AlphaStrip`, `ColorSwatch` — **stay
widget-internal**. They render the colour space itself; that is domain
data, not chrome (principle 6). They remain `pub(crate)` widgets.
```rust
pub struct ColorPickerStyleConfig {
    pub layout: ColorPickerLayout,    // existing domain enum
    pub canvas: WidgetId,             // HsvCanvas, pre-built
    pub hue_strip: WidgetId,
    pub alpha_strip: Option<WidgetId>,
    pub swatches: WidgetId,           // SwatchGrid, pre-built
    pub inputs: WidgetId,             // RGB/hex input row, pre-built
}
pub trait ColorPickerStyle: 'static {
    fn make_body(&self, cfg: &ColorPickerStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}
```
`RecipeColorPickerStyle` arranges the panel surface, section spacing,
and swatch sizing around the pre-built functional parts. A custom
`impl ColorPickerStyle` *can* return a completely different subtree
(even its own canvas) — the built-in renderers being `pub(crate)` is
the only limit, and that is acceptable: replacing the colour-space
renderer is a fork-worthy change, not a theming change.
`components.color_picker` constants relocate.

New slots: `calendar`, `color_picker`.

### Stage E — field-family widgets

All four field-family widgets already compose the themed `TextInput`,
so they are *mostly* themed-through-composition. What is left is each
widget's own affordances.

**E1 — `SpinBoxStyle`.** The only SpinBox-specific chrome is the
step-button pair. `StepButton` has no `paint()` — it composes an
`IconButton`. The trait carries the step-button layout policy
(stacked vs side-by-side, the `ButtonLayout` domain enum) and the
field-plus-buttons arrangement:
```rust
pub struct SpinBoxStyleConfig {
    pub field: WidgetId,             // the themed TextInput subtree
    pub step_up: WidgetId,
    pub step_down: WidgetId,
    pub layout: ButtonLayout,        // existing domain enum
}
pub trait SpinBoxStyle: 'static {
    fn make_body(&self, cfg: &SpinBoxStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}
```

**E2 — `DateEditStyle` (shared by `DateEdit` / `TimeEdit` /
`DateRangeEdit` / `DateTimeEdit`).** These four are variations on
"segmented field over a `TextInput`". One trait themes the segment
separators, the validation-strip styling, and the optional
calendar-popover trigger button arrangement:
```rust
pub struct DateEditStyleConfig {
    pub field: WidgetId,             // themed TextInput / segment row
    pub trigger: Option<WidgetId>,   // the calendar/clock popover button
    pub validation: TextInputValidationLevel,  // reuse the existing enum
}
pub trait DateEditStyle: 'static {
    fn make_body(&self, cfg: &DateEditStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}
```
`components.date_edit` and `components.time_edit` constants merge into
`recipe_date_edit_style.rs`.

**E3 — `SearchFieldStyle`.** The field chrome (themed `TextInput` with
a leading search glyph + trailing clear button) gets a thin trait; the
`SuggestionPanel` was already routed through `PopoverStyle::Menu` in
Stage A.
```rust
pub struct SearchFieldStyleConfig {
    pub field: WidgetId,
    pub leading_glyph: WidgetId,
    pub trailing_clear: Option<WidgetId>,
}
pub trait SearchFieldStyle: 'static {
    fn make_body(&self, cfg: &SearchFieldStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}
```
`components.search_field` relocates.

**E4 — `RichTextEditorStyle` (frame chrome only).**
`RichTextEditor::paint` renders glyph runs, caret, and selection — that
is the editor's domain output and **stays widget-owned** (principle 6).
The trait themes only the *frame*: border, padding, focus ring,
background — the same surface a `TextInput` has.
```rust
pub struct RichTextEditorStyleConfig {
    pub viewport: WidgetId,          // the scrolling text viewport, pre-built
    pub is_focused: Signal<bool>,
    pub is_read_only: bool,
}
pub trait RichTextEditorStyle: 'static {
    fn make_body(&self, cfg: &RichTextEditorStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}
```

New slots: `spin_box`, `date_edit`, `search_field`, `rich_text_editor`.

### Stage F — data-driven containers

**F1 — `TableStyle` (multi-method, shared by `TableView` + `TreeTable`).**
This is the heaviest migration. `TableView::paint` draws alt-row
backgrounds, row-selection highlights, and grid lines;
`HeaderCell::paint` / `SortIndicator::paint` / `HeaderRow::paint` draw
the header chrome; `BodyRow::paint` draws per-cell separators. All of it
reads `components.table`.
```rust
pub struct TableHeaderCellConfig {
    pub label: WidgetId,
    pub sort: Option<SortDirection>,
    pub is_hovered: Signal<bool>,
    pub is_resizing: Signal<bool>,
}
pub struct TableRowConfig {
    pub index: usize,
    pub is_selected: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_alt: bool,
}
pub trait TableStyle: 'static {
    fn make_header_cell(&self, cfg: &TableHeaderCellConfig, ctx: &mut BuildContext) -> WidgetId;
    fn make_sort_indicator(&self, dir: SortDirection, ctx: &mut BuildContext) -> WidgetId;
    /// Row-band chrome (selection / hover / alt) — composed *behind* the cells.
    fn make_row_background(&self, cfg: &TableRowConfig, ctx: &mut BuildContext) -> WidgetId;
    /// Grid-line + frozen-column-shadow recipe, applied by the table's own paint pass.
    fn grid(&self) -> TableGridRecipe;
}
```
The row-band backgrounds and header cells move to composed widgets;
grid lines and the frozen-column shadow stay a *recipe* (`TableGridRecipe`
— line thickness, colour role, shadow spec) consumed by a thin
`TableView::paint` that draws only those (grid lines genuinely need a
single batched paint pass over the virtualized viewport — composing one
`RectWidget` per line would defeat virtualization). This is the same
"recipe describes, widget paints the batched case" split the original
styling plan anticipated for specialty widgets. `components.table`
constants relocate to `recipe_table_style.rs`. `TreeTable` reuses
`TableStyle` unchanged — it already shares table_view's header modules.
`TableView::as_any` (used by the inspector) is preserved.

**F2 — `ListContainerStyle` (shared by `ListView` + `TreeView`).**
Both widgets' rows are already themed (`StandardItemStyle`). The only
container chrome is the drag-insertion line and the optional
selection-band behind rows. A thin shared trait:
```rust
pub struct ListInsertionConfig {
    pub axis_offset: f32,    // y for lists, in container-local coords
    pub width: f32,
}
pub trait ListContainerStyle: 'static {
    fn make_insertion_indicator(&self, cfg: &ListInsertionConfig, ctx: &mut BuildContext) -> WidgetId;
}
```
`ListView::paint` / `TreeView::paint` (which today draw only the
insertion line) are replaced by a composed indicator leaf bound to the
drop-feedback signal — the same pattern Stage A's review used for
`TabBar`'s drop indicator. The hard-coded
`Color::from_rgba(0.2, 0.4, 0.9, 0.8)` insertion-line colour becomes a
role in the recipe.

New slots: `table`, `list_container`.

### Stage G — `ComponentStyles` teardown

After Stages A–F, every `theme.components.*` field that a *themable*
widget read is gone. What remains in `ComponentStyles` is dimension
data for **group-4 composites and primitives** that own no chrome:
`text_area`, `toolbar`, `status_bar`, `tree_list`, `group_box`,
`accordion`, `tool_box`, `split_button`, `breadcrumb`, `wizard`,
`divider`, `split_view`, `chart`, `command_link_button`, plus
`TitleBar`'s residuals.

Stage G finishes the job:
1. For each remaining struct, fold its constants into a `pub const`
   block in the owning widget's module (the Step-7 pattern). These
   widgets compose themed children; their residual numbers are layout
   constants, not paint recipes.
2. Delete `ComponentStyles` and the `ComponentTokens` aggregator from
   `crates/fern-tokens/src/components.rs`; delete the file if empty.
3. Remove the `components: ComponentStyles` field from `Theme`
   (`crates/fern-core/src/styles/theme.rs`) and from
   `fern_core::presets::intui::{light, dark}`.
4. Drop `ComponentStyles` / `ComponentTokens` from
   `crates/fern-tokens/src/lib.rs` re-exports. `fern-tokens` is now a
   pure-data leaf: `ColorTokens`, `ShapeTokens`, `LayoutTokens`,
   `TypographyTokens`, `MotionTokens`, role enums, `ThemeAppearance`,
   raw token presets — and nothing widget-shaped.
5. `crates/fern-inspector/src/tabs/theme.rs`: the Theme tab no longer
   has a `ComponentStyles` table to render (it was already emptied to
   `&[]` in Step 7); remove the dead scaffolding.

`cargo build --workspace` is the proof: any surviving `theme.components`
reference is a compile error.

## Migration order & commit structure

One branch off `main`, executed as the stages above, **in order** —
each stage depends only on completed earlier stages:

```
Stage A  popup unification        4 commits (A1 PopoverVariant::Menu, A2 MenuList,
                                              A3 ComboBox, A4 SearchField panel)
Stage B  notification surfaces    3 commits (Dialog, Snackbar, Banner)
Stage C  indicators & controls    5 commits (ProgressBar, SegmentedControl,
                                              Badge, Link, Avatar)
Stage D  calendar & colour picker 2 commits (Calendar, ColorPicker)
Stage E  field family             4 commits (SpinBox, DateEdit-family,
                                              SearchField, RichTextEditor)
Stage F  data containers          2 commits (TableStyle, ListContainerStyle)
Stage G  ComponentStyles teardown 3 commits (G-fold constants, G-delete
                                              ComponentStyles, G-inspector cleanup)
```

23 commits. Each commit:
- adds/edits exactly one trait file + one recipe module + the widget,
  or one teardown step;
- leaves `cargo build --workspace` and `cargo test --workspace` green;
- is independently bisectable.

Within a stage, the trait file (`fern-core`) lands in the same commit
as its first consumer so `fern-core` never ships an unused public
trait.

## Crate-structure changes

**New trait files** in `crates/fern-core/src/styles/`:
`dialog_style.rs`, `snackbar_style.rs`, `banner_style.rs`,
`progress_bar_style.rs`, `segmented_control_style.rs`, `badge_style.rs`,
`link_style.rs`, `avatar_style.rs`, `calendar_style.rs`,
`color_picker_style.rs`, `spin_box_style.rs`, `date_edit_style.rs`,
`search_field_style.rs`, `rich_text_editor_style.rs`, `table_style.rs`,
`list_container_style.rs`. Each re-exported from `styles/mod.rs`.
(No `menu_list_style.rs` — MenuList uses `PopoverStyle`.)

**New recipe modules** in `crates/fern-widgets/src/styles/`: one
`recipe_<widget>_style.rs` per trait above, each holding the relocated
`pub const` IntUI dimensions and the `Recipe*Style` default impl.

**`ComponentStyleSlots`** (`crates/fern-core/src/styles/component_style_slots.rs`)
grows 16 new fields — `dialog`, `snackbar`, `banner`, `progress_bar`,
`segmented_control`, `badge`, `link`, `avatar`, `calendar`,
`color_picker`, `spin_box`, `date_edit`, `search_field`,
`rich_text_editor`, `table`, `list_container` — taking the slot bag
from 16 to 32. Its hand-rolled `Debug` and `PartialEq` impls extend in
lockstep.

**`PopoverSurface`** (`crates/fern-widgets/src/popover.rs`) gains the
`content_padding` / `background` / `corner_radius` parameters from A1.

**Deleted:** `crates/fern-tokens/src/components.rs` (Stage G);
`MenuList::paint`, `ComboBox`'s `DropdownShadow` leaf, `Avatar::paint` +
`paint_border` + `paint_focus_ring`, `ProgressBar::paint`,
`SegmentedControl::paint` + `SegmentButton::paint`, `SnackbarSurface::paint`,
`ModalContainer::paint`, `ListView::paint`, `TreeView::paint`,
`TableView::paint` (replaced by the thin grid-only pass), the
`HeaderCell` / `SortIndicator` / `HeaderRow` / `BodyRow` paint methods.

## Testing & verification

Per stage, before the stage's commits are considered done:

1. `cargo build --workspace` — clean. Any stale `theme.components`
   reference is a compile error (the enforcement mechanism).
2. `cargo test --workspace` — all suites green. The widget crates carry
   layout-integration tests; each migrated widget's tests must pass
   unchanged **or** be updated only for tree-structure shifts (the
   `bar_content`-helper situation from the `TabBar` migration is the
   precedent — a structural wrapper changing a test's navigation path
   is expected; a behavioural change is not).
3. `cargo clippy --workspace --all-targets` and `cargo fmt --check` —
   clean. Run at the tip of each stage.
4. **Visual smoke test** — run the affected examples 3 s in debug and
   confirm no panic, then eyeball parity against the pre-migration
   look:
   - Stage A: `menus_and_dropdowns`, `data_collections` (combo),
     `widget_catalog`.
   - Stage B: `dialogs_and_popovers`, `widget_catalog`.
   - Stage C: `widget_catalog`, `animations_kit` (progress bar),
     `new_widgets_kit`.
   - Stage D: `datetime_pickers`, `color_picker`.
   - Stage E: `spin_box`, `datetime_pickers`, `rich_text_editor`,
     `widget_catalog`.
   - Stage F: `data_grid`, `tree_table`, `data_collections`.
   - Stage G: full example sweep — every example 3 s in debug.
5. **Custom-style integration test** per stage: for at least one widget
   in the stage, write a throwaway `impl FooStyle` in `widget_catalog`'s
   styling tab that paints a visibly different chrome, confirm it
   renders without forking the widget. This proves the trait is a real
   escape hatch, not a rename.

## Hard cases & risks

- **Functional-renderer boundary (ColorPicker, RichTextEditor, Banner
  glyph, Avatar initials, ProgressBar indeterminate quad).** Principle 6
  draws the line: the style themes the *frame*, the widget keeps the
  *domain renderer*. The risk is scope-creep arguments ("but the HSV
  canvas *is* a look"). The plan's position is firm: a renderer that
  draws domain data stays in the widget; the trait still returns a
  `WidgetId` so a determined app can replace the whole subtree, but the
  default `Recipe*Style` is not obligated to make the renderer itself
  swappable. Each affected recipe module documents this explicitly.

- **`TableView` grid lines vs virtualization.** Composing one
  `RectWidget` per grid line would materialize a widget per visible
  row and defeat the table's virtualization. `TableStyle::grid()`
  therefore returns a *recipe* (not a subtree) and a deliberately thin
  `TableView::paint` draws the batched grid pass. This is the one place
  a themable widget keeps a `paint()` — and it is principled: it paints
  *from a recipe the style supplies*, it does not *decide* the look.
  Documented in `recipe_table_style.rs`.

- **`Dialog` and the modal pipeline.** `ModalContainer` is mounted by
  `ModalRequest` / `ModalPresentation` in `fern-core`. `DialogStyle`
  must only change `ModalContainer`'s chrome, never its mounting or
  dismissal. The `make_scrim` / `make_panel` split keeps the modal
  machinery's hooks (focus trap, dismiss-on-scrim-click) on the widget,
  not the style.

- **`ProgressBar` shader quad.** The `AnimatedQuadRegistry` slot is
  owned by the widget; the style supplies the sweep colour recipe only.
  Getting this split wrong would either move motion infra into a style
  (inconsistent with the framework) or freeze the indeterminate
  animation. Verification is explicit, not just a panic check: an
  indeterminate horizontal `ProgressBar` must still emit exactly one
  `draw_animated_quad` per frame and **must not** re-run `paint()` —
  confirmed via the inspector's frame counter / the existing idle-fps
  test alongside the `animations_kit` smoke test.

- **Stage G blast radius.** Removing the `theme.components` field
  touches every remaining consumer in one commit — the same shape as
  the original plan's Step 1d. Mitigation: Stages A–F already removed
  every *themable-widget* consumer, so by Stage G only group-4
  composites reference it, and the `pub const` fold (G commit 1) lands
  before the field removal (G commit 2) so each commit builds.

- **Slot-bag growth.** 32 slots on `ComponentStyleSlots` is large but
  flat and `Option`-defaulted — `Theme::clone` stays cheap (`Rc::clone`
  per populated slot). The hand-rolled `Debug` / `PartialEq` are
  mechanical to extend; a unit test asserts every field appears in
  both.

## Documentation work

Folded into the relevant stages, not a trailing milestone:

- **[docs/styling-system.md](../styling-system.md)** — the migration-status
  table grows to cover all of group 5; the "variant-free widgets" note
  extends; a short subsection documents the functional-renderer
  boundary (principle 6) since it is now load-bearing for five widgets.
- **[docs/widgets-overview.md](../widgets-overview.md)** — the Styling-status
  table gains every group-5 row; each migrated widget's bullet notes its
  style trait.
- **[.claude/CLAUDE.md](../../.claude/CLAUDE.md)** — the Theming section's
  migration-status line moves from "16 themable widgets" to "all
  chrome-owning widgets"; the `ComponentStyles` mention is removed (the
  struct no longer exists); `fern-tokens` is re-described as a pure-data
  leaf.
- **[docs/fern-ui-architecture.md](../fern-ui-architecture.md)** §19 — the
  one-paragraph theming summary drops the `ComponentStyles` reference.
- **[docs/table-view.md](../table-view.md)** — gains a "Theming" section
  documenting `TableStyle` and the grid-recipe split.
- **[docs/inspector.md](../inspector.md)** — the Theme-tab description
  loses its `ComponentStyles` line (already stale since Step 7).

## Definition of done

- All 19 in-scope widgets compose chrome through a Tier-3 `*Style`
  trait (or, for the popup containers, through `PopoverStyle`).
- The 7 confirmed-no-trait widgets read no `theme.components.*` and own
  no `paint()` chrome.
- `fern_tokens::ComponentStyles` and `ComponentTokens` are deleted;
  `theme.components` no longer exists; `cargo build --workspace` proves
  it.
- `fern-tokens` exports only pure data.
- Every example runs 3 s in debug without panic and at visual parity
  with the pre-migration IntUI look.
- A custom `impl` for at least one widget per stage is demonstrated in
  the `widget_catalog` styling tab.
- The four-tier styling ladder is, with the single documented exception
  of `TitleBar`, **complete**: no chrome-owning widget self-styles.
