# Styling System

FernUI's theming is a four-tier ladder. Each tier is independently
opt-in: an app that only needs dark mode never sees the higher tiers;
an app shipping a brutalist redesign uses every rung.

```
Tier 0:  Tokens          (colors, shapes, motion, typography, layout)
Tier 1:  Variants         (per-widget closed enums: Filled / Plain / …)
Tier 2:  Recipes          (paint vocabulary — shape, fill, border, shadow)
Tier 3:  Style protocols  (`trait FooStyle { fn make_body(...) -> WidgetId }`)
```

The default implementations of Tier 3 (the `Recipe*Style` types
shipped in `fern-widgets/src/styles/`) read Tier 2 recipes; the
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
[`fern-tokens/src/`](../crates/fern-tokens/src/). They're pure data
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
    pub components: ComponentStyles,         // dim structs for the non-themable widgets only
    pub style_slots: ComponentStyleSlots,    // typed Rc<dyn FooStyle> slots
    pub extensions: ThemeExtensions,
}
```

There is no `Theme::default()`. Apps explicitly pick a preset:

```rust
use fern_ui::prelude::intui;
let theme = intui::light();   // or intui::dark()
```

Other presets ship as opt-in Cargo features (Material 3, macOS,
Fluent) once Step 11 of the styling refactor lands; until then only
IntUI is bundled.

**Reactive.** `Theme` lives behind a `Signal<Theme>` on
`WidgetTree` — `set_theme(...)` dirty-marks every widget for repaint
without rebuilding the tree. Focus, scroll offsets, and animation
state survive theme swaps. See
[`docs/reactive-theme.md`](reactive-theme.md).

**Extensions.** `theme.with_extension::<MyPalette>(...)` /
`theme.extension::<MyPalette>()` attach app-specific extras that don't
fit any of the five token groups. Cheap (TypeId lookup), arbitrary
type.

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
`ListContainerStyle`), `TableView` / `TreeTable` (via `TableStyle`).
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

Set per-call: `Button::new("Save").variant(ButtonVariant::Outlined)`.
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
[`fern-core/src/styles/recipe.rs`](../crates/fern-core/src/styles/recipe.rs).
Primitive recipe types:

```rust
pub enum ShapeRecipe {
    Rect { corner_radius: Prop<CornerRadius> },
    Pill,                          // corner = min(w,h)/2
    Circle,
    CustomPath(Arc<dyn Fn(Rect) -> Path + Send + Sync>),
}

pub enum FillRecipe {
    Solid(RecipeColor),
    LinearGradient { stops: Vec<(f32, RecipeColor)>, angle_deg: f32 },
    RadialGradient { stops: Vec<(f32, RecipeColor)>, center: (f32, f32), radius: f32 },
    None,
}

pub struct BorderRecipe {
    pub width: Prop<f32>,
    pub color: RecipeColor,
    pub style: BorderStyle,         // Solid | Dashed { dash, gap } | Dotted
    pub position: BorderPosition,   // Inside | Center | Outside
}

pub struct ShadowRecipe {
    pub offset: Prop<Vec2>,
    pub blur: Prop<f32>,
    pub spread: Prop<f32>,
    pub color: RecipeColor,
}
```

**Per-state cascades.** Most widgets need different recipes for hover
/ pressed / focused / disabled. The plan's answer is
`PerStateRecipe<T>` with an explicit fallback chain — FernUI's
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

As of this branch every themable widget holds its recipe-equivalent
data inside its `Recipe*Style` default. The IntUI dimension constants
that used to live in `fern-tokens::components` per-widget structs were
deleted (Step 7) and folded directly into the matching
`fern-widgets/src/styles/recipe_*_style.rs` module as `pub const`
blocks — the recipe *is* the dimension data now, with no parallel
store. `ButtonRecipe` is the one standalone Tier-2 struct surfaced so
far; a future commit will surface `ToggleRecipe`, `CardRecipe`, etc.
so apps can construct custom-dimensioned `RecipeFooStyle::new(recipe)`
without writing a new Tier-3 impl.

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

The trait is `'static` only (not `Send + Sync`) because all FernUI
trees are single-threaded by construction; `Rc<dyn FooStyle>` is the
public alias (`SharedButtonStyle` and friends).

**Same shape across widgets.** All 32 style traits live in
[`fern-core/src/styles/`](../crates/fern-core/src/styles/), all
return `WidgetId` from their `make_*` methods, all take a
`*StyleConfig` describing the inputs that vary by widget. The trait
is the public API; everything below it is implementation. The full
list lives in the [migration status table](#migration-status-as-of-this-branch).

### Worked example — a Material-3-flavoured Button

```rust
use std::rc::Rc;

use fern_core::build_context::BuildContext;
use fern_core::styles::{ButtonStyle, ButtonStyleConfig};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius, SurfaceRole};
use fern_widgets::primitives::{Padding, RectWidget, ZStack};

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
                if *disabled { SurfaceRole::SurfaceDimmed }
                else if *pressed { SurfaceRole::AccentPressed }
                else if *hovered { SurfaceRole::AccentHover }
                else { SurfaceRole::Accent }
            });

        let rect = ctx.add(
            RectWidget::new()
                .bind_background(bg)
                .corner_radius(CornerRadius::uniform(20.0)),
        );

        let padded_label = ctx.add(
            Padding::new()
                .symmetric(10.0, 24.0)         // M3 spec: 10×24
                .child_id(cfg.label),
        );

        ctx.add(ZStack::new().add_child(rect).add_child(padded_label))
    }
}
```

Install per-call: `Button::new("Save").style(MaterialFilledButton)`.
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
[`fern-widgets/src/button.rs`](../crates/fern-widgets/src/button.rs)
under `theme_slot_supplies_button_style_when_no_override` /
`per_call_style_override_wins_over_theme_slot`.

## Built-in presets

| Preset | Where | Status |
| --- | --- | --- |
| `intui::light` / `intui::dark` | `fern_core::presets::intui` | shipped — the default look |
| `material3::light` / `material3::dark` | `fern-theme-material3` crate | stub (Step 11) |
| `macos::light` / `macos::dark` | `fern-theme-macos` crate | stub (Step 11) |
| `fluent::light` / `fluent::dark` | `fern-theme-fluent` crate | stub (Step 11) |
| Image-backed themes | `fern-image-theme` crate | not yet shipped (Step 10) |

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
slot lookup. No themable widget self-paints anymore. **32 widgets**
across six families:

**Controls**

| Widget | Trait | Default impl | Slot |
| --- | --- | --- | --- |
| `Toggle` | `ToggleStyle` | `RecipeToggleStyle` | `style_slots.toggle` |
| `Button` | `ButtonStyle` | `RecipeButtonStyle` | `style_slots.button` |
| `Checkbox` | `CheckboxStyle` | `RecipeCheckboxStyle` | `style_slots.checkbox` |
| `RadioButton` | `RadioStyle` | `RecipeRadioStyle` | `style_slots.radio` |
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
| `TableView` / `TreeTable` (header + sort + row chrome) | `TableStyle` ¹ | `RecipeTableStyle` | `style_slots.table` |

**Overlays**

| Widget | Trait | Default impl | Slot |
| --- | --- | --- | --- |
| `TooltipWidget` | `TooltipStyle` | `RecipeTooltipStyle` | `style_slots.tooltip` |
| `Popover` | `PopoverStyle` | `RecipePopoverStyle` | `style_slots.popover` |
| `Dialog` (in-tree modal) | `DialogStyle` ¹ | `RecipeDialogStyle` | `style_slots.dialog` |
| `Snackbar` | `SnackbarStyle` | `RecipeSnackbarStyle` | `style_slots.snackbar` |
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

¹ Multi-method trait — see [Multi-method styles](#multi-method-styles)
below.

Step 7 (delete legacy per-widget dimension structs) is done: the 17
old `fern-tokens::components::*Style` structs were deleted and their
IntUI constants folded into the matching
`fern-widgets/src/styles/recipe_*_style.rs` modules.
`theme.components` (`ComponentStyles`) still exists but now carries
dimension data only for the *non-themable* widgets (toolbar, status
bar, accordion, …) — anything that isn't yet on a style trait.
Migrated widgets read entirely from `theme.style_slots.*` plus their
`Recipe*Style` defaults.

Still ahead on the styling roadmap: image-backed styles (Step 9), the
`ImageTheme` TOML manifest loader (Step 10), and the sibling preset
crates `fern-theme-material3` / `-macos` / `-fluent` (Step 11).

### Multi-method styles

Most style traits have a single `make_body(cfg, ctx) -> WidgetId`
method. Four widgets need finer granularity — the trait splits chrome
into multiple slots so a custom impl can replace one piece without
re-implementing the others:

- **`TabStyle`** — `make_body` themes a single tab header (accent
  indicator + focus ring + label slot composition); `make_bar` themes
  the whole strip (optional backdrop fill, content-pane separator,
  drag-reorder drop indicator).
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

The trait pattern doesn't require buying into FernUI's slot bag —
you can ship the trait + default impl and let users override via
`MyWidget::style(...)` per call. The slot bag is for theme-wide
installation; it's optional, but it's how the framework's themable
widgets get reskinned across an app.

## See also

- [docs/reactive-theme.md](reactive-theme.md) — Signal-backed Theme,
  color signals, theme swaps without rebuild.
- [docs/image-themes.md](image-themes.md) — designer-workflow deep
  reference (Figma / Penpot / Canva → 9-slice manifest → theme).
  (Not yet shipped; design pending Step 10.)
- [docs/widgets-overview.md](widgets-overview.md) — per-widget
  variant + style trait references.
- [docs/accessibility-overrides.md](accessibility-overrides.md) —
  style trait impls do **not** participate in accessibility; the
  widget owns its `accessibility(builder)` regardless of which style
  is installed.
