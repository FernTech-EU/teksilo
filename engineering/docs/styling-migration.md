<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Styling migration record

## Migration status (as of this branch)

Every themable widget is on the Tier-3 trait + recipe-default +
slot lookup. No themable widget self-paints anymore. **47 widgets
across 42 style traits, spanning seven families** (a "trait" can cover
more than one widget — e.g. `ListContainerStyle` styles both
`ListView` and `TreeView`; `ChartStyle` styles `BarChart`, `LineChart`,
and `PieChart`):

**Controls**

| Widget | Trait | Default impl | Slot |
| --- | --- | --- | --- |
| `Toggle` | `ToggleStyle` | `RecipeToggleStyle` | `style_slots.toggle` |
| `Button` | `ButtonStyle` | `RecipeButtonStyle` | `style_slots.button` |
| `SplitButton` | `SplitButtonStyle` | `RecipeSplitButtonStyle` | `style_slots.split_button` |
| `Checkbox` | `CheckboxStyle` | `RecipeCheckboxStyle` | `style_slots.checkbox` |
| `RadioButton` | `RadioStyle` | `RecipeRadioStyle` | `style_slots.radio` |
| `RadioTile` | `RadioTileStyle` | `RecipeRadioTileStyle` | `style_slots.radio_tile` |
| `IconButton` | `IconButtonStyle` | `RecipeIconButtonStyle` | `style_slots.icon_button` |
| `Slider` | `SliderStyle` | `RecipeSliderStyle` | `style_slots.slider` |
| `SegmentedControl` | `SegmentedControlStyle` | `RecipeSegmentedControlStyle` | `style_slots.segmented_control` |
| `ProgressBar` | `ProgressBarStyle` | `RecipeProgressBarStyle` | `style_slots.progress_bar` |
| `Link` | `LinkStyle` | `RecipeLinkStyle` | `style_slots.link` |
| `Avatar` | `AvatarStyle` | `RecipeAvatarStyle` | `style_slots.avatar` |
| `Badge` | `BadgeStyle` | `RecipeBadgeStyle` | `style_slots.badge` |

**Inputs**

| Widget | Trait | Default impl | Slot |
| --- | --- | --- | --- |
| `TextInput` | `TextInputStyle` | `RecipeTextInputStyle` | `style_slots.text_input` |
| `SearchField` | `SearchFieldStyle` | `RecipeSearchFieldStyle` | `style_slots.search_field` |
| `ComboBox` | `ComboBoxStyle` | `RecipeComboBoxStyle` | `style_slots.combo_box` |
| `SpinBox` | `SpinBoxStyle` | `RecipeSpinBoxStyle` | `style_slots.spin_box` |
| `DateEdit` | `DateEditStyle` | `RecipeDateEditStyle` | `style_slots.date_edit` |
| `ColorPicker` | `ColorPickerStyle` | `RecipeColorPickerStyle` | `style_slots.color_picker` |
| `Calendar` | `CalendarStyle` ¹ | `RecipeCalendarStyle` | `style_slots.calendar` |
| `RichTextEditor` | `RichTextEditorStyle` | `RecipeRichTextEditorStyle` | `style_slots.rich_text_editor` |

**Containers**

| Widget | Trait | Default impl | Slot |
| --- | --- | --- | --- |
| `Panel` | `PanelStyle` | `RecipePanelStyle` | `style_slots.panel` |
| `Card` | `CardStyle` | `RecipeCardStyle` | `style_slots.card` |
| `TabBar` | `TabStyle` ¹ | `RecipeTabStyle` | `style_slots.tab` |
| `ListView` / `TreeView` (container chrome) | `ListContainerStyle` | `RecipeListContainerStyle` | `style_slots.list_container` |
| `TableView` / `TreeTableView` (header + sort + row chrome) | `TableStyle` ¹ | `RecipeTableStyle` | `style_slots.table` |
| `DropZone` | `DropZoneStyle` | `RecipeDropZoneStyle` | `style_slots.drop_zone` |
| `DropTarget` | `DropTargetStyle` | `RecipeDropTargetStyle` | `style_slots.drop_target` |
| `Splitter` (divider handles) | `SplitterStyle` | `RecipeSplitterStyle` | `style_slots.splitter` |
| `GridView` (focus ring, marquee, insertion bar, pinned header) | `GridViewStyle` ² | `RecipeGridViewStyle` | `style_slots.grid_view` |
| `WebView` (overlay chrome, `teksilo-webview`) | `WebViewStyle` | `RecipeWebViewStyle` (in `teksilo-webview`) | `style_slots.web_view` |

**Overlays**

| Widget | Trait | Default impl | Slot |
| --- | --- | --- | --- |
| `TooltipWidget` | `TooltipStyle` | `RecipeTooltipStyle` | `style_slots.tooltip` |
| `Popover` | `PopoverStyle` | `RecipePopoverStyle` | `style_slots.popover` |
| `Dialog` (in-tree modal) | `DialogStyle` ¹ | `RecipeDialogStyle` | `style_slots.dialog` |
| `Snackbar` | `SnackbarStyle` | `RecipeSnackbarStyle` | `style_slots.snackbar` |
| `Toast` | `ToastStyle` | `RecipeToastStyle` | `style_slots.toast` |
| `Banner` | `BannerStyle` | `RecipeBannerStyle` | `style_slots.banner` |

**Rows / Items**

| Widget | Trait | Default impl | Slot |
| --- | --- | --- | --- |
| `MenuItem` | `MenuItemStyle` ³ | `RecipeMenuItemStyle` | `style_slots.menu_item` |
| `StandardListItem` / `StandardTreeItem` | `StandardItemStyle` ³ | `RecipeStandardItemStyle` | `style_slots.standard_item` |

**Chrome**

| Widget | Trait | Default impl | Slot |
| --- | --- | --- | --- |
| `ScrollBar` | `ScrollBarStyle` | `RecipeScrollBarStyle` | `style_slots.scroll_bar` |
| Touch text-selection handles + magnifier | `TextSelectionStyle` ² | `RecipeTextSelectionStyle` | `style_slots.text_selection` |

**Data Visualization**

| Widget | Trait | Default impl | Slot |
| --- | --- | --- | --- |
| `BarChart` / `LineChart` / `PieChart` (`teksilo-charts`) | `ChartStyle` ² | `RecipeChartStyle` (in `teksilo-charts`, not `teksilo-widgets`) | `style_slots.chart` |

¹ Multi-method trait — see [Multi-method styles](#multi-method-styles)
below.

² All-recipe trait, no `make_*` methods (for `ChartStyle`, see
[Data-visualization styling](#data-visualization-styling) below).
`RecipeChartStyle` and `RecipeWebViewStyle` are the two entries in this
table whose `Recipe*Style` does **not** live under
`teksilo-widgets/src/styles/*` — `teksilo-charts` and `teksilo-webview`
deliberately have no dependency on `teksilo-widgets`, so each default
style has to live where its own dependencies already reach. See
[charts.md §11](charts.md) for the
full reference.

³ Carries **defaulted label hooks**:
`StandardItemStyle::selected_label_role`,
`MenuItemStyle::highlighted_label_role` and
`MenuItemStyle::pressed_label_role`, all `-> Option<TextRole>`, all
`None` by default. `StandardItemStyle` also carries
`selection_emphasis() -> SelectionEmphasis`, defaulted to
`ViewFocusedAndWindowActive`: what mutes a selected row, which a design
language that picks its palette per window (Qt, so KDE's Breeze) sets to
`WindowActive`.

A row builds its label *before* any style's `make_body` runs, so a
style cannot recolour the text it is about to paint behind. That is
fine for a design language whose selection is a pale wash — IntUI and
Fluent both keep `TextRole::Primary` on top of theirs — and impossible
for one whose selection is a **solid fill**: macOS's accent capsule
would leave `labelColor` at roughly 3.5:1. The hook lets the style
declare the role and the widget compose it into the label's colour
signal (and, for a menu row, its shortcut's), gated on the row actually
being emphasised so an unemphasised or window-inactive row keeps its
normal label.

Same shape as `ButtonStyle::label_text_role`, and defaulted for the
same reason: every existing style is unchanged.

```rust
impl StandardItemStyle for MyStyle {
    fn make_body(&self, cfg: &StandardItemStyleConfig, ctx: &mut BuildContext) -> WidgetId { … }

    // Only needed when `make_body` fills the selection with a colour
    // the default label cannot read on.
    fn selected_label_role(&self) -> Option<TextRole> {
        Some(TextRole::OnAccent)
    }
}
```

The legacy per-widget dimension structs are gone: the 17
old `teksilo-tokens::components::*Style` structs were deleted and their
IntUI constants folded into the matching
`teksilo-widgets/src/styles/recipe_*_style.rs` modules.
The `ComponentStyles` struct has been fully removed from `Theme`.
Migrated widgets read entirely from `theme.style_slots.*` plus their
`Recipe*Style` defaults. Dimension data for any remaining non-themable
widgets (toolbar, status bar, accordion, …) lives directly in their
`Recipe*Style` modules as `pub const` blocks.

The **`teksilo-theme-material3`** sibling preset is now a real Material 3
theme (baseline `#6750A4` scheme, M3 shape/typography, pill 40 dp
buttons with state-layer hover, the M3 switch, 12 dp cards) and the
proving ground for the recipe-vocabulary additions above. Its optional
`bundled-fonts` feature embeds Roboto. The framework primitives it
needed — `FillRecipe::StateLayer`, per-side `BorderRecipe` +
`BorderPosition`, gradient `PaintProp`, the configurable `FooRecipe`
sweep, the cross-design-language color roles
(`TextRole::OnError`, `SurfaceRole::{ErrorContainer, Container,
ContainerRaised, ContainerSunken}`), `Easing::CubicBezier`,
`ToggleStyleConfig::is_pressed`, and `TeksiloAppBuilder::register_fonts`
— are all in place, so the `-fluent` and `-macos` presets below and a
future GTK4-Adwaita one follow the same path.

The **`teksilo-theme-fluent`** sibling preset is a full Windows 11 /
WinUI 3 theme, transcribed from WinUI's own `Common_themeresources_any.xaml`
and the control theme-resource dictionaries: the light and dark colour
dictionaries (exposed in full through the `FluentPalette` theme
extension), the two-radius geometry (`ControlCornerRadius` 4 dp /
`OverlayCornerRadius` 8 dp), the WinUI type ramp at zero tracking, and
the four `Control*AnimationDuration` steps on
`ControlFastOutSlowInKeySpline`. It installs Tier-3 chrome for 25 style
slots: eight are real `impl FooStyle` blocks where the WinUI control is
structurally its own thing — the button's **elevation edge** (a heavier
stroke on the bottom edge in light, the top edge in dark, dropped on
press), the two-tone high-contrast focus ring, the `ToggleSwitch`'s
off-state outline and morphing knob, the filled unchecked checkbox and
radio, the field's **accent focus underline**, the slider's two-circle
thumb, the menu row's neutral hover, and the list row's **selection
pill** — while the rest are the shipped `Recipe*Style` constructed with
Fluent metrics. `light_with_accent` / `dark_with_accent` rebuild the
whole accent family around a caller-supplied seed, the substitution
Windows performs when the user picks an accent colour. Mica and Acrylic
resolve to the opaque fallbacks WinUI itself uses when the compositor
material is unavailable; Segoe UI Variable cannot be redistributed, so
the optional `system-fonts` feature names it for the text engine to
resolve rather than bundling it.

The **`teksilo-theme-macos`** sibling preset is a full macOS **Aqua /
Dark Aqua** theme. It is the one preset whose source publishes almost
nothing: Apple attaches a standing disclaimer to every colour value it
prints, and states no corner radii, no control heights, no focus-ring
geometry and exactly one animation duration. Every literal in the crate
is therefore tagged at its definition as `[HIG]` (published — the
13-hue system-colour table and the whole typography ramp), `[measured]`
(a capture of the private `NSColor` enumeration, or a screen
measurement) or `[derived]` (computed, with the rule given). AppKit's
wider vocabulary — four label grades, two independent selection
families, the control bezel, the eight System Settings accents — is
exposed through the `MacOsPalette` theme extension.

Geometry is 6 dp in-page / 10 dp floating (menus at their own measured
9 dp) on a **22 dp** control height, a third under Fluent's 32.
Typography is the published SF ramp — Body 13/16, Callout 12/15,
Subheadline 11/14 — carrying Apple's **signed** tracking: −0.08 pt at
13, exactly 0 at 12, +0.06 at 11. It is the only Teksilo preset that
tracks non-uniformly and the only one whose tracking changes sign.
Motion is Core Animation's default 0.25 s on
`kCAMediaTimingFunctionEaseInEaseOut` — `cubic-bezier(0.42, 0, 0.58, 1)`,
symmetric where Fluent's is decelerate-only.

It installs Tier-3 chrome for 28 style slots; eight are real
`impl FooStyle` blocks: the push button's **bezel** (shadow, face
gradient, hairline, Dark-Aqua catch-light — dropped on press, and
deliberately absent from the accent-filled *default* button), a focus
ring that **is the accent** rather than Fluent's neutral outline, the
`NSSwitch`'s 18 dp knob in a 22 dp track, the 14 dp bezelled checkbox
and radio, the field's **accent focus halo**, the slider's plain round
knob, the menu row's **accent fill with a white label**, and the list
row's **selection capsule**. `light_with_accent` / `dark_with_accent`
and the `SystemAccent` enum rebuild the accent family; `linkColor`
deliberately does not follow, as on macOS.

Four places deviate from Apple's own numbers to clear WCAG, each
documented at its assignment with the measurement that forced it and
each pinned by a test that also asserts the *premise* — so if Apple's
value ever starts passing, the deviation can be reverted rather than
inherited. Two framework additions came out of it: the defaulted
label-role hooks described above, without which a solid-accent
selection cannot recolour the text on top of it.

Known limitations are stated rather than deferred: the OS accent is not
read (Teksilo's platform layer returns only the light/dark preference
on macOS), vibrancy resolves to each material's opaque fallback, the
`TableView` / `GridView` selection band is an accent wash rather than
the capsule (those views paint the shared `surface_selected` token
behind app-supplied cells this preset cannot retint), and San Francisco
is named under the optional `system-fonts` feature rather than bundled.

The **`teksilo-theme-kde`** sibling preset is KDE Plasma's **Breeze** as
Plasma 6.8 ships it, and the first whose colours are not a palette written
in the crate. They are a KDE colour scheme, read and resolved by
`teksilo-kde-scheme` (a no-I/O crate, written from
`engineering/docs/kde-breeze-spec.md` and never from KDE or Qt source, with
a test that fails on upstream identifiers), so the stock Breeze Light and
Dark, any `.colors` file (`from_scheme`), Plasma's accent application
(`light_with_accent`, `PlasmaAccent`) and a whole configured desktop
(`from_desktop`: scheme, fonts, animation speed, `breezerc`) all come out of
one function. `KdePalette` keeps the scheme's colours in every state as a
theme extension; the colour tokens are those colours lifted to WCAG by
stated rules where the scheme falls short, and each lift is tested both to
pass and on its premise that the scheme's own colour fails. Every literal
is tagged at its assignment with where it comes from (`[M]` / `[S]` /
`[Hp]` Breeze's style code, `[qt]` / `[measured-qt]` Qt's behaviour,
`[derived]`, `[wcag]`, `[unpublished]`), and a pixel test compares the
controls with Plasma's own through the production renderer.

Real `impl FooStyle` blocks cover the buttons, the check box, radio button
and switch, the line edit, combo box, spin box and slider, the menu row, the
popup frame and tooltip, the list and tree rows, tables, tabs, the scroll
bar and the progress bar; the card, panel, dialog, snackbar and the like are
the shipped recipe styles built with Breeze's metrics, and the crate's slot
tests give a reason for every slot it leaves to the widget. Three framework
additions came out of it, all defaulted so no existing style changed: a
`PaintProjection` theme extension (a theme whose palette defines its own
inactive and high-contrast colours paints those, replacing the generic
desaturation chain; see `docs/window-activation.md`), the
`selection_emphasis` and `pressed_label_role` hooks of footnote ³, and the
recipe fields the Breeze metrics needed rendered (row, header and tab
heights, a minimum thumb length, a negative focus-ring offset; see
`density-projection-gaps.md`).

Known limitations are in the crate's documentation, not deferred: some
widgets keep the framework's chrome, Qt's linear-light-versus-sRGB blending
shows in translucent paints over a backdrop the style does not paint, and
the Noto Sans and Hack faces are named or embedded only under the crate's
own font features, which the umbrella does not forward. The widget shapes
are chosen when a tree is built, so switching to or from this preset at
runtime re-tints and keeps the built shapes; `ThemeMode::Native` with
`native_theme_kde()` builds a window in Breeze's shapes on a Breeze desktop
(`docs/native-theme.md`).

Still ahead on the styling roadmap: image-backed styles, the
`ImageTheme` TOML manifest loader, and a GTK4-Adwaita sibling preset
crate.

### Multi-method styles

Most style traits have a single `make_body(cfg, ctx) -> WidgetId`
method. Four widgets need finer granularity — the trait splits chrome
into multiple slots so a custom impl can replace one piece without
re-implementing the others:

- **`TabStyle`** — `make_body` themes a single tab header (accent
  indicator + focus ring + label slot composition); `make_bar` themes
  the whole strip (optional backdrop fill, content-pane separator,
  drag-reorder drop indicator). `TabStyleConfig` carries
  `indicator_position` (`TabIndicatorPosition::{OuterEdge, InnerEdge}`)
  so the active-tab highlight can hug either edge; the default
  `RecipeTabStyle` honours all four edges (outer/inner × horizontal/
  vertical, RTL-correct). Per-tab backgrounds, the bar backdrop, inter-tab
  dividers, and text-colour roles are widget-level `TabBar`/`TabWidget`
  builders rather than part of the trait — see
  [tab-widget.md](../../docs/tab-widget.md) "Appearance".
- **`DialogStyle`** — `make_panel` themes the modal surface (shadow +
  corner radius + padding + container chrome); `make_scrim` themes
  the full-viewport overlay backdrop (the click-outside-to-dismiss
  layer). Wired into the in-tree modal pipeline so the scrim is a
  proper child of the dialog overlay, not a hand-rolled rect.
- **`TableStyle`** — `make_header_cell` (column header chrome: hover
  tint, resize-handle band, raised background), `make_sort_indicator`
  (the up/down arrow), `make_row_background` (per-row surface, with
  selection + hover + zebra states). The body cell stays
  app-controlled — same delegate that produces the cell's content
  also owns its paint.
- **`CalendarStyle`** — `make_day_cell`, `make_zoom_cell` (month /
  year picker grid), `make_header` (month-year label + nav buttons).
  Calendar is unusually paint-heavy and the three slots match the
  three distinct visual modes (day grid, zoom grid, header).

For these traits, a custom `impl` must implement every method (no
default impls beyond the trait's own — the recipe defaults compose
the four slots into the IntUI look). Apps that only want to tweak
one slot typically forward the others to `Recipe*Style::default()`.

### Data-visualization styling

`ChartStyle` (`BarChart` / `LineChart` / `PieChart`, `teksilo-charts`)
is a third trait *shape*, distinct from both the single-method
`make_body` traits and the multi-method traits above:

```rust
pub trait ChartStyle: 'static {
    fn bar_fill(&self, cfg: &ChartFillContext) -> FillRecipe;
    fn area_fill(&self, cfg: &ChartFillContext, opacity: f32) -> FillRecipe;
    fn donut_fill(&self, cfg: &ChartFillContext) -> FillRecipe;
    fn gridline(&self, theme: &Theme) -> BorderRecipe;
}
```

Every method returns a Tier-2 recipe (`FillRecipe` / `BorderRecipe`)
directly — **none returns a `WidgetId`**. Charts paint through `Canvas`
calls inside their own `paint()` instead of composing a child widget
subtree, so there is no `make_*(cfg, ctx) -> WidgetId` step for a
custom impl to hook: the widget resolves the active `ChartStyle`,
asks it for a recipe, and paints that recipe's fill/stroke directly.
Where `TabStyle`/`DialogStyle`/`TableStyle`/`CalendarStyle` split
chrome into *named `WidgetId`-returning slots* because each slot is a
distinct sub-tree, `ChartStyle` splits into named *recipe-returning*
methods because each is a distinct paint operation (bar fill vs. area
fill vs. donut fill vs. gridline stroke) inside one widget's own paint
pass. Resolution precedence is identical to every other trait:
per-call `.style(impl ChartStyle)` > `theme.style_slots.chart` >
`RecipeChartStyle::default()`. Full reference:
[charts.md §11](charts.md).
