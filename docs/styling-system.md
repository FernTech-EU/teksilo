<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Styling System

Bastyde's theming is a four-tier ladder. Each tier is independently
opt-in: an app that only needs dark mode never sees the higher tiers;
an app shipping a brutalist redesign uses every rung.

```
Tier 0:  Tokens          (colors, shapes, motion, typography, layout)
Tier 1:  Variants         (per-widget closed enums: Filled / Plain / …)
Tier 2:  Recipes          (paint vocabulary — shape, fill, border, shadow)
Tier 3:  Style protocols  (`trait FooStyle { fn make_body(...) -> WidgetId }`)
```

The default implementations of Tier 3 (the `Recipe*Style` types
shipped in `bastyde-widgets/src/styles/`) read Tier 2 recipes; the
default recipes read Tier 0 tokens. So Tier 3 *contains* Tiers 0-2 for
the IntUI preset — but the trait protocol at Tier 3 is the escape
hatch that lets apps replace the entire chrome of any widget without
touching the widget source.

> **Reference for designers** — image-backed themes (Figma / Penpot /
> Canva exports → 9-slice assets → reskinned app) get their own deep
> reference at [docs/image-themes.md](image-themes.md). The image-theme
> system is a parallel set of `impl FooStyle` blocks on top of the
> same Tier-3 surface — same widgets, different chrome.

## Mental model — which tier do I use for X?

| Task | Tier | API |
| --- | --- | --- |
| Tweak a color across the whole app | 0 | `theme.colors.accent = …` |
| Make a single Button red | n/a | `Button::color(Color::RED)` (always-allowed prop override) |
| Pick "outlined" instead of "filled" on a Button | 1 | `Button::variant(ButtonVariant::Outlined)` |
| Make every Outlined Button thicker | 2 | Modify a `BorderRecipe` in the IntUI preset, OR ship a new preset |
| Replace Button chrome entirely (glassmorphism / brutalist / Material-3) | 3 | `impl ButtonStyle for MyGlassButton` then `theme.style_slots.button = Some(Rc::new(MyGlassButton))` |
| Reskin from designer-exported SVGs | 3 | Ship an `ImageBackedButtonStyle` via the manifest loader |

The cardinal rule: **never edit widget source to change a look**. If
the existing API doesn't get you there, write an `impl FooStyle` block
and install it.

## Tier 0 — Tokens

The five token groups (`ColorTokens`, `ShapeTokens`, `LayoutTokens`,
`TypographyTokens`, `MotionTokens`) live in
[`bastyde-tokens/src/`](../crates/bastyde-tokens/src/). They're pure data
structs with no widget knowledge.

`Theme` aggregates the five token groups plus appearance, component
dimensions, and the typed style-slot bag:

```rust
pub struct Theme {
    pub appearance: ThemeAppearance,        // Light | Dark — required
    pub colors: ColorTokens,
    pub layout: LayoutTokens,
    pub typography: TypographyTokens,
    pub shape: ShapeTokens,
    pub motion: MotionTokens,
    pub style_slots: ComponentStyleSlots,    // typed Rc<dyn FooStyle> slots
    pub extensions: ThemeExtensions,
}
```

There is no `Theme::default()`. Apps explicitly pick a preset:

```rust
use bastyde::prelude::intui;
let theme = intui::light();   // or intui::dark()
```

Other presets ship as opt-in Cargo features (Material 3, macOS,
Fluent); until then only IntUI is bundled.

**Reactive.** `Theme` lives behind a `Signal<Theme>` on
`WidgetTree` — `set_theme(...)` dirty-marks every widget for repaint
without rebuilding the tree. Focus, scroll offsets, and animation
state survive theme swaps. See
[`docs/reactive-theme.md`](reactive-theme.md).

**Extensions.** `theme.with_extension::<MyPalette>(...)` /
`theme.extension::<MyPalette>()` attach app-specific extras that don't
fit any of the five token groups. Cheap (TypeId lookup), arbitrary
type.

### How a widget goes grey when disabled

Two mechanisms, and picking the wrong one is the classic way to ship a
control that stays fully lit after `.enabled(false)`.

**Accent-filled controls dim for free.** `ColorProp::resolve(theme,
effective_enabled)` — which every role-driven leaf (`TextWidget`,
`IconWidget`, `RectWidget`) calls at paint time — substitutes the
disabled counterpart of a role when the subtree is disabled: any
`TextRole` → `TextRole::Disabled`, and the *accent* family
(`SurfaceRole::Accent` / `AccentHover` / `AccentPressed`,
`BorderRole::Accent`) → their `AccentDisabled` counterpart. So a Filled
`Button`, a checked `Checkbox`, a `Toggle` and a `Slider` fill all grey
out with **no disabled-handling code at all**. That is why most recipes
never mention `is_disabled`.

**Neutral controls must opt in.** The substitution deliberately leaves
*non-accent* surfaces and borders alone — a disabled `Panel` keeps its
surface, and only interactive accent chrome dims. It has to: a text
field's frame and a passive `Panel` both paint `SurfaceRole::Content`,
so the hook cannot tell them apart. A neutral **interactive** control —
`TextInput`, `SpinBox`, `ComboBox`, `DateEdit` — therefore states it
explicitly, using the neutral disabled roles:

```rust
let bg_role = cfg.is_disabled.map(|d| {
    if *d { SurfaceRole::Disabled } else { SurfaceRole::Content }
});
let border_role = cfg.is_focused.zip(&cfg.is_disabled).map(|(f, d)| {
    if *d { BorderRole::Disabled }          // outranks focus
    else if *f { BorderRole::Focused }
    else { BorderRole::Default }
});
```

`SurfaceRole::Disabled` / `BorderRole::Disabled` resolve to the neutral
`surface_disabled` / `border_disabled` tokens. Do **not** reach for
`AccentDisabled` here: it is a washed-out *accent* (pale cyan in IntUI),
correct for an accent-filled Button and wrong for a grey field.

Get the `is_disabled` signal from
`ctx.effective_enabled_signal(self_id).map(|on| !*on)` — it ANDs the
widget's own `enabled` prop with every ancestor's, so a field inside a
disabled form dims too. Note it returns a **derived** signal, so it can
be bound to a prop but cannot be passed to `ctx.effect` (`Signal::observe`
panics on derived signals). A widget that paints raw colours rather than
roles — anything shaping through a `RichTextEngine`, e.g.
`TextInputField` — bypasses `ColorProp` entirely and must resolve against
`ctx.effective_enabled` in `paint` instead.

## Tier 1 — Variants

Each themable widget exposes a closed `*Variant` enum naming its
design-language presentations. The variant is **a hint**: the active
Tier-3 style decides what it means.

```rust
ButtonVariant    { Filled, Tinted, Outlined, Plain, Ghost, Link, Destructive }
ToggleVariant    { Switch, Pill, Square, Inset }
CheckboxVariant  { Square, Rounded, Circle }
RadioVariant     { Circle, Square, Rounded }
IconButtonSize   { Compact, Default, Toolbar, Large, Hero }  // size = variant for IconButton
CardVariant      { Plain, Elevated*, Outlined, Filled }       // * = #[default]
PanelVariant     { Plain, Sunken, Raised, Highlighted }
PopoverVariant   { Default, Menu, Tooltip }
SliderVariant    { Continuous, Discrete, Range }
TextInputVariant { Outlined, Filled, Underline, Bare }
ComboBoxVariant  { Outlined, Filled, Underline, Plain }
ScrollBarVariant { Permanent, Overlay, Thin }
AvatarShape      { Circle, Square, Rounded }                  // and AvatarSize, AvatarCorner, AvatarPresence
```

`Card` defaults to `Elevated` (shadow + surface_main) — the "just
works" Card that matches pre-refactor behaviour. Use
`.variant(CardVariant::Plain)` for a flat surface.

The remaining themable widgets are variant-free: `MenuItem`,
`StandardListItem` / `StandardTreeItem`, `TabBar`, `TooltipWidget`,
`Dialog`, `Snackbar`, `Banner`, `SegmentedControl`, `ProgressBar`,
`Link`, `Badge`, `SearchField`, `SpinBox`, `DateEdit`, `ColorPicker`,
`Calendar`, `RichTextEditor`, `ListView` / `TreeView` (via
`ListContainerStyle`), `TableView` / `TreeTableView` (via `TableStyle`).
Their style traits take a `*StyleConfig` with no `variant` field —
the design language has a single canonical shape, or the variant
distinction lives elsewhere (e.g. `ProgressBarKind` for determinate
vs indeterminate).

`Slider` and `ScrollBar` additionally carry an *orientation* enum
(`SliderOrientation`, `ScrollBarOrientation`) alongside the variant,
since orientation changes layout, not just paint.

Several widgets have multi-method style traits where chrome
decomposes into named slots (e.g. `TabStyle::make_body` +
`make_bar`). See [Multi-method styles](#multi-method-styles) below
for the full list.

Set per-call: `Button::new(lit!("Save")).variant(ButtonVariant::Outlined)`.
Set per-app via a custom Tier-3 style that *defaults* a variant for
unspecified callers.

**IntUI variant policy.** Int UI is intentionally minimalist about
button styling — destructive actions live in confirmation dialogs
where the body carries the warning, not the button. So the IntUI
`RecipeButtonStyle` collapses several variants:
`Destructive` → Filled, `Tinted`/`Outlined` → Plain, `Link` → Ghost.
Other design languages (Material 3 if/when it ships) honour them
distinctly.

## Tier 2 — Recipes

Recipes are pure data describing paint vocabulary. They live in
[`bastyde-core/src/styles/recipe.rs`](../crates/bastyde-core/src/styles/recipe.rs).
Primitive recipe types:

```rust
pub enum ShapeRecipe {
    Rect { corner_radius: CornerRadius },
    Pill,                          // corner = min(w,h)/2
    Circle,
}

pub enum FillRecipe {
    Solid(RecipeColor),
    // overlay composited over base at `alpha` → flat color. The M3 /
    // Fluent "state layer" (hover = 8 %, pressed = 12 % on-color).
    StateLayer { base: RecipeColor, overlay: RecipeColor, alpha: f32 },
    LinearGradient { stops: Vec<GradientStop>, angle_deg: f32 },
    RadialGradient { stops: Vec<GradientStop>, center: (f32, f32), radius: f32 },
    None,
}

pub struct BorderRecipe {
    pub width: f32,
    pub color: RecipeColor,
    pub style: BorderStyle,           // Solid | Dashed { dash, gap } | Dotted
    pub position: BorderPosition,     // Inside | Center | Outside (now honoured)
    pub sides: Option<BorderSides>,   // None = uniform; Some = per-side widths
}
// BorderSides { top, trailing, bottom, leading: f32 } — e.g.
// BorderRecipe::underline(w, color) for an M3/Fluent filled-field underline.

pub struct ShadowRecipe {
    pub offset: Vec2,
    pub blur: f32,
    pub spread: f32,
    pub color: RecipeColor,
}
```

**Per-state cascades.** Most widgets need different recipes for hover
/ pressed / focused / disabled. The answer is
`PerStateRecipe<T>` with an explicit fallback chain — Bastyde's
take on Flutter's `WidgetStateProperty<T>`:

```rust
pub struct PerStateRecipe<T> {
    pub idle:     T,
    pub hover:    Option<T>,    // falls back to idle
    pub pressed:  Option<T>,    // falls back to hover, then idle
    pub focused:  Option<T>,    // falls back to hover, then idle
    pub disabled: Option<T>,    // falls back to idle
}
```

`PerStateRecipe::resolve(WidgetState) -> &T` walks the chain. No
closures, fully Serde-serialisable, theme-file-friendly.

**Colors in recipes.** Recipes hold a `RecipeColor` enum (not
`ColorProp`) — `Static | Surface(SurfaceRole) | Border(BorderRole)
| Text(TextRole)` — so the full theme cascade still applies but the
recipe stays plain data (serializes cleanly for inspector JSON Export
and TOML image-theme manifests).

**Gradients are rendered.** `FillRecipe::LinearGradient` /
`RadialGradient` paint through the SDF gradient pipeline (via
`PaintProp`, the gradient-or-solid fill prop `RectWidget` accepts).
Anything `Into<ColorProp>` is also `Into<PaintProp>` as a solid, so
existing fills are unchanged.

**Configurable dimensions per widget.** Every themable widget now
surfaces a public `FooRecipe` dimension struct, and its
`RecipeFooStyle` carries `recipe: FooRecipe` with a
`RecipeFooStyle::new(recipe)` constructor. `Default` fills the recipe
from the IntUI `pub const` dimension block (kept as the default source),
so a theme can tweak *just the dimensions* without writing a new Tier-3
impl:

```rust
let toggle = RecipeToggleStyle::new(ToggleRecipe {
    track_width: 52.0, track_height: 32.0, thumb_diameter: 24.0, thumb_inset: 4.0,
});
theme.style_slots.toggle = Some(Rc::new(toggle));
```

The four multi-method widgets (Tab, Dialog, Table, Calendar) expose a
flat recipe each (`TabRecipe`, `DialogRecipe`, `TableRecipe`,
`CalendarRecipe`). A handful with no tunable dimensions (SpinBox,
SplitButton, GridView, ListContainer, RichTextEditor) stay unit structs.

## Tier 3 — Style protocols

The escape hatch. Each themable widget exposes a trait:

```rust
pub trait ButtonStyle: 'static {
    fn make_body(&self, cfg: &ButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub struct ButtonStyleConfig {
    pub label:       WidgetId,           // pre-built label subtree
    pub is_pressed:  Signal<bool>,
    pub is_hovered:  Signal<bool>,
    pub is_focused:  Signal<bool>,
    pub is_disabled: Signal<bool>,
    pub variant:     ButtonVariant,      // a hint; impl may ignore
}
```

The widget builds the parts (label, optional icon, four state
signals), hands the bag to the active style, and uses the returned
`WidgetId` as its root child. Everything else — background, border,
focus ring, padding, min size — is the style's responsibility.

The trait is `'static` only (not `Send + Sync`) because all Bastyde
trees are single-threaded by construction; `Rc<dyn FooStyle>` is the
public alias (`SharedButtonStyle` and friends).

**Same shape across widgets.** All 34 style traits live in
[`bastyde-core/src/styles/`](../crates/bastyde-core/src/styles/), all
return `WidgetId` from their `make_*` methods, all take a
`*StyleConfig` describing the inputs that vary by widget. The trait
is the public API; everything below it is implementation. The full
list lives in the [migration status table](#migration-status-as-of-this-branch).

### Worked example — a Material-3-flavoured Button

```rust
use std::rc::Rc;

use bastyde_core::build_context::BuildContext;
use bastyde_core::styles::{ButtonStyle, ButtonStyleConfig};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{Color, CornerRadius, SurfaceRole};
use bastyde_widgets::primitives::{Padding, RectWidget, ZStack};

struct MaterialFilledButton;

impl ButtonStyle for MaterialFilledButton {
    fn make_body(&self, cfg: &ButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Material 3 filled buttons are tall (40 dp), pill-shaped, with
        // a small elevation that lifts on hover. State-driven `Accent` /
        // `AccentHover` / `AccentPressed` cover the surface; disabled
        // collapses to a flat translucent grey.
        let bg = cfg.is_pressed
            .zip3(&cfg.is_hovered, &cfg.is_disabled)
            .map(|(pressed, hovered, disabled)| {
                if *disabled { SurfaceRole::AccentDisabled }
                else if *pressed { SurfaceRole::AccentPressed }
                else if *hovered { SurfaceRole::AccentHover }
                else { SurfaceRole::Accent }
            });

        let rect = ctx.add(
            RectWidget::new()
                .background(bg)
                .corner_radius(CornerRadius::uniform(20.0)),
        );

        let padded_label = ctx.add(
            Padding::symmetric(10.0, 24.0)     // M3 spec: 10×24
                .child_id(cfg.label),
        );

        ctx.add(ZStack::new().add_child(rect).add_child(padded_label))
    }
}
```

Install per-call: `Button::new(lit!("Save")).style(MaterialFilledButton)`.
Install theme-wide:

```rust
let mut theme = intui::light();
theme.style_slots.button = Some(Rc::new(MaterialFilledButton));
```

The widget honours this precedence at every `build()`:

```
per-call .style(...)  >  theme.style_slots.button  >  RecipeButtonStyle::default()
```

Tested end-to-end in
[`bastyde-widgets/src/button.rs`](../crates/bastyde-widgets/src/button.rs)
under `theme_slot_supplies_button_style_when_no_override` /
`per_call_style_override_wins_over_theme_slot`.

## Built-in presets

| Preset | Where | Status |
| --- | --- | --- |
| `intui::light` / `intui::dark` | `bastyde_core::presets::intui` | shipped — the default look |
| `material3::light` / `material3::dark` | `bastyde-theme-material3` crate | stub |
| `macos::light` / `macos::dark` | `bastyde-theme-macos` crate | stub |
| `fluent::light` / `fluent::dark` | `bastyde-theme-fluent` crate | stub |
| Image-backed themes | `bastyde-image-theme` crate | not yet shipped |

Each preset is just a function returning `Theme`. Apps can write their
own without depending on any sibling crate:

```rust
pub fn brutalist_light() -> Theme {
    let mut theme = intui::light();
    theme.colors.accent     = Color::new(1.0, 0.0, 0.4, 1.0);   // hot pink
    theme.shape.radius_md   = 0.0;                              // sharp corners everywhere
    theme.style_slots.button   = Some(Rc::new(MyBrutalistButton));
    theme.style_slots.checkbox = Some(Rc::new(MyBrutalistCheckbox));
    theme
}
```

## Migration status (as of this branch)

Every themable widget is on the Tier-3 trait + recipe-default +
slot lookup. No themable widget self-paints anymore. **43 widgets
across 38 style traits, spanning seven families** (a "trait" can cover
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
| `MenuItem` | `MenuItemStyle` | `RecipeMenuItemStyle` | `style_slots.menu_item` |
| `StandardListItem` / `StandardTreeItem` | `StandardItemStyle` | `RecipeStandardItemStyle` | `style_slots.standard_item` |

**Chrome**

| Widget | Trait | Default impl | Slot |
| --- | --- | --- | --- |
| `ScrollBar` | `ScrollBarStyle` | `RecipeScrollBarStyle` | `style_slots.scroll_bar` |

**Data Visualization**

| Widget | Trait | Default impl | Slot |
| --- | --- | --- | --- |
| `BarChart` / `LineChart` / `PieChart` (`bastyde-charts`) | `ChartStyle` ² | `RecipeChartStyle` (in `bastyde-charts`, not `bastyde-widgets`) | `style_slots.chart` |

¹ Multi-method trait — see [Multi-method styles](#multi-method-styles)
below.

² All-recipe trait, no `make_*` methods — see
[Data-visualization styling](#data-visualization-styling) below. Its
default impl is the one entry in this table whose `Recipe*Style` does
**not** live under `bastyde-widgets/src/styles/*` — `bastyde-charts`
deliberately has no dependency on `bastyde-widgets`, so its default
style has to live where its own dependencies already reach. See
[charts.md §11](charts.md) for the
full reference.

The legacy per-widget dimension structs are gone: the 17
old `bastyde-tokens::components::*Style` structs were deleted and their
IntUI constants folded into the matching
`bastyde-widgets/src/styles/recipe_*_style.rs` modules.
The `ComponentStyles` struct has been fully removed from `Theme`.
Migrated widgets read entirely from `theme.style_slots.*` plus their
`Recipe*Style` defaults. Dimension data for any remaining non-themable
widgets (toolbar, status bar, accordion, …) lives directly in their
`Recipe*Style` modules as `pub const` blocks.

The **`bastyde-theme-material3`** sibling preset is now a real Material 3
theme (baseline `#6750A4` scheme, M3 shape/typography, pill 40 dp
buttons with state-layer hover, the M3 switch, 12 dp cards) and the
proving ground for the recipe-vocabulary additions above. Its optional
`bundled-fonts` feature embeds Roboto. The framework primitives it
needed — `FillRecipe::StateLayer`, per-side `BorderRecipe` +
`BorderPosition`, gradient `PaintProp`, the configurable `FooRecipe`
sweep, the cross-design-language color roles
(`TextRole::OnError`, `SurfaceRole::{ErrorContainer, Container,
ContainerRaised, ContainerSunken}`), `Easing::CubicBezier`,
`ToggleStyleConfig::is_pressed`, and `BastydeAppBuilder::register_fonts`
— are all in place, so the `-macos` / `-fluent` / GTK4-Adwaita presets
can follow the same path.

Still ahead on the styling roadmap: image-backed styles, the
`ImageTheme` TOML manifest loader, and the `-macos` / `-fluent` /
GTK4-Adwaita sibling preset crates.

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
  [tab-widget.md](tab-widget.md) "Appearance".
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

`ChartStyle` (`BarChart` / `LineChart` / `PieChart`, `bastyde-charts`)
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

## Custom widgets and the styling system

Writing your own composing widget? Three steps to make it themable:

1. **Declare a closed `MyWidgetVariant` enum** for the design-language
   presentations users can pick (mirror `ButtonVariant`'s shape).
2. **Define a `MyWidgetStyle` trait** in your own crate with a
   `make_body(cfg, ctx) -> WidgetId` signature. The `cfg` struct
   exposes the inputs that vary by interaction state (`Signal<bool>`s
   for hover/pressed/etc.), the variant, and pre-built child subtrees.
3. **Ship a `RecipeMyWidgetStyle`** as the default impl. Add a slot to
   your own slot-bag struct (or attach via `theme.extensions` if you
   only need app-internal use).

The trait pattern doesn't require buying into Bastyde's slot bag —
you can ship the trait + default impl and let users override via
`MyWidget::style(...)` per call. The slot bag is for theme-wide
installation; it's optional, but it's how the framework's themable
widgets get reskinned across an app.

## See also

- [docs/reactive-theme.md](reactive-theme.md) — Signal-backed Theme,
  color signals, theme swaps without rebuild.
- [docs/image-themes.md](image-themes.md) — designer-workflow deep
  reference (Figma / Penpot / Canva → 9-slice manifest → theme).
  (Not yet shipped; design pending.)
- [docs/widgets-overview.md](widgets-overview.md) — per-widget
  variant + style trait references.
- [docs/accessibility-overrides.md](accessibility-overrides.md) —
  style trait impls do **not** participate in accessibility; the
  widget owns its `accessibility(builder)` regardless of which style
  is installed.
