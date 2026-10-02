<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Styling System

Use theme tokens for application-wide values, variants for standard appearances,
and style traits for custom widget chrome.

```
Tier 0:  Tokens          (colors, shapes, motion, typography, layout)
Tier 1:  Variants         (per-widget closed enums: Filled / Plain / …)
Tier 2:  Recipes          (paint vocabulary, shape, fill, border, shadow)
Tier 3:  Style protocols  (`trait FooStyle { fn make_body(...) -> WidgetId }`)
```

The default implementations of Tier 3 (the `Recipe*Style` types
shipped in `teksilo-widgets/src/styles/`) read Tier 2 recipes; the
default recipes read Tier 0 tokens. So Tier 3 *contains* Tiers 0-2 for
the IntUI preset, but the trait protocol at Tier 3 is the escape
hatch that lets apps replace the entire chrome of any widget without
touching the widget source.

## Mental model: which tier do I use for X?

| Task | Tier | API |
| --- | --- | --- |
| Tweak a color across the whole app | 0 | `theme.colors.accent = …` |
| Make a single Button's label red | n/a | `Button::text_role(Color::RED)` (always-allowed prop override; a per-instance fill goes through `.style(...)`) |
| Pick "outlined" instead of "filled" on a Button | 1 | `Button::variant(ButtonVariant::Outlined)` |
| Make every Outlined Button thicker | 2 | Modify a `BorderRecipe` in the IntUI preset, OR ship a new preset |
| Replace Button chrome entirely (glassmorphism / brutalist / Material-3) | 3 | `impl ButtonStyle for MyGlassButton` then `theme.style_slots.button = Some(Rc::new(MyGlassButton))` |
| Reskin from designer-exported SVGs | 3 | An image-backed `impl ButtonStyle` (a built-in `ImageBackedButtonStyle` + manifest loader is planned, not shipped) |

Implement and install a style trait when token and variant changes are insufficient.

## Tier 0: Tokens

The five token groups (`ColorTokens`, `ShapeTokens`, `LayoutTokens`,
`TypographyTokens`, `MotionTokens`) live in
[`teksilo-tokens/src/`](../crates/teksilo-tokens/src/). They're pure data
structs with no widget knowledge.

`Theme` aggregates the five token groups plus an id, appearance, the
input/density tokens, and the typed style-slot bag:

```rust
pub struct Theme {
    pub id: ThemeId,                        // "intui.light", "custom", …
    pub appearance: ThemeAppearance,        // Light | Dark, required
    pub colors: ColorTokens,
    pub layout: LayoutTokens,
    pub typography: TypographyTokens,
    pub shape: ShapeTokens,
    pub motion: MotionTokens,
    pub input: InputTokens,                  // density, target sizes, gesture slop
    pub style_slots: ComponentStyleSlots,    // typed Rc<dyn FooStyle> slots
    pub extensions: ThemeExtensions,
}
```

There is no `Theme::default()`. Apps explicitly pick a preset:

```rust
use teksilo::prelude::intui;
let theme = intui::light();   // or intui::dark()
```

Other presets ship as opt-in Cargo features (Material 3, macOS,
Fluent), only IntUI is bundled by default.

**Reactive.** `Theme` lives behind a `Signal<Theme>` on
`WidgetTree`, `set_theme(...)` dirty-marks every widget for relayout
and repaint without rebuilding the tree. Focus, scroll offsets, and animation
state survive theme swaps. See
[`docs/reactive-theme.md`](reactive-theme.md).

**Extensions.** `theme.with_extension::<MyPalette>(...)` /
`theme.extension::<MyPalette>()` attach app-specific extras that don't
fit any of the five token groups. Cheap (TypeId lookup), arbitrary
type.

### How a widget goes grey when disabled

A widget is disabled when its own `enabled` prop is false **or any ancestor's
is**, a control inside a disabled form is disabled. Its chrome greys by one
of two routes, and there is a trap in each.

**Role-driven chrome dims for free.** `ColorProp::resolve(theme,
effective_enabled)`, which every role-driven leaf (`TextWidget`,
`IconWidget`, `RectWidget`) calls at paint time, substitutes the disabled
counterpart of a role in a disabled subtree: any `TextRole` →
`TextRole::Disabled`; the *accent* family (`SurfaceRole::Accent` /
`AccentHover` / `AccentPressed`, `BorderRole::Accent`) → their
`AccentDisabled` counterpart; and the *neutral interactive*
`SurfaceRole::Field` / `BorderRole::Field` → their `Disabled` counterpart.
This is why most recipes never mention `is_disabled`.

Note:️ The substitution only reaches roles. `ColorProp::Bound(Signal<Color>)`
resolves to `s.get()` and ignores `enabled` entirely, so a recipe that folds
per-state colours into a flat reactive colour (as `RecipeButtonStyle` does via
`PerStateRecipe` + `bind_fill`) gets **no** paint-time safety net, and must
select its `WidgetState::Disabled` from `cfg.is_disabled`.

**Neutral controls must opt into a `Field` role.** The substitution
deliberately leaves passive surfaces alone, a disabled `Panel` keeps its
surface. It has to: a text field's frame and a passive `Panel` both painted
`SurfaceRole::Content`, so the hook could not tell them apart, and dimmed
neither. That is what `Field` is for. It resolves **identically to `Content`
while enabled** and substitutes to `Disabled` when not, so a field dims and a
panel does not:

```rust
// A field's frame. No `is_disabled` needed for the resting case, the role
// dims itself at paint, from the live arena.
let bg = RectWidget::new().background(SurfaceRole::Field);

// The border still consults `is_disabled`, so disabled outranks *focus*.
let border_role = cfg.is_focused.zip(&cfg.is_disabled).map(|(f, d)| {
    if *d { BorderRole::Disabled }
    else if *f { BorderRole::Focused }
    else { BorderRole::Field }
});
```

`SurfaceRole::Disabled` / `BorderRole::Disabled` resolve to the neutral
`surface_disabled` / `border_disabled` tokens. Do **not** reach for
`AccentDisabled` here: it is a washed-out *accent* (pale cyan in IntUI),
right for an accent-filled Button and wrong for a grey field.

**Getting the signal.** `ctx.effective_enabled_signal(self_id).map(|on| !*on)`
ANDs the widget's own `enabled` prop with every ancestor's. It is a node-resident
signal that the framework refreshes from the live arena each state-change pass,
so it may be bound to a prop *and* passed to `ctx.effect`.

It is deliberately not a signal derived by walking ancestors at call time. A
widget's `parent` is still `None` while its own `build()` runs, `insert_widget`
inserts the node parentless and wires the parent only after `build()` returns,
so such a walk sees an empty chain and captures the widget's *own* `enabled` prop
as the whole answer, permanently. Prefer a `Field` role over the signal where you
can: the paint-time route reads the live tree and cannot go stale.

**Raw-colour widgets bypass all of this.** Anything shaping through a
`RichTextEngine` (e.g. `TextInputField`) hands GPU colours straight to the
engine, so no `ColorProp` is involved. Resolve against `ctx.effective_enabled`
in `paint` instead.

### `Color::mix` and non-finite factors

`darken`, `lighten`, `desaturated`, and `ColorTokens::for_inactive_window`
(the window-deactivation accent projection, see
[window-activation.md](window-activation.md)) all bottom out in
`Color::mix(other, t)`. `t` is clamped to `[0, 1]` before use, but a bare
`t.clamp(0.0, 1.0)` is not enough on its own: `f32::clamp` returns `NaN` for
a `NaN` input rather than saturating it, so a `NaN` factor used to poison
every channel and produce an unrenderable colour. `Color::mix` now maps a
`NaN` factor to `0.0` (returning `self` unchanged) before clamping, the
safest reading of an undefined mix. `±inf` needs no such special case: the
clamp already maps them to `1.0` / `0.0` correctly. A caller deriving `t`
from a ratio that can legitimately divide by zero no longer needs to guard
it before calling `mix`.

## Tier 1: Variants

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
AvatarShape      { Circle, RoundedSquare, Square }            // and AvatarSize, AvatarCorner, AvatarPresence
RadioTileVariant { Outlined, Elevated, Filled }
DropTargetVariant { Default, Prominent, Subtle, None }        // border weight
// SplitButton reuses ButtonVariant
```

`Card` defaults to `Elevated` (shadow + surface_main), the "just
works" Card that matches pre-refactor behaviour. Use
`.variant(CardVariant::Plain)` for a flat surface.

The remaining themable widgets are variant-free: `MenuItem`,
`StandardListItem` / `StandardTreeItem`, `TabBar`, `TooltipWidget`,
`Dialog`, `Snackbar`, `Banner`, `SegmentedControl`, `ProgressBar`,
`Link`, `Badge`, `SearchField`, `SpinBox`, `DateEdit`, `ColorPicker`,
`Calendar`, `RichTextEditor`, `ListView` / `TreeView` (via
`ListContainerStyle`), `TableView` / `TreeTableView` (via `TableStyle`).
Their style traits take a `*StyleConfig` with no `variant` field,
the design language has a single canonical shape, or the variant
distinction lives elsewhere (e.g. `ProgressBarKind` for determinate
vs indeterminate).

`Slider` and `ScrollBar` additionally carry an *orientation* enum
(`SliderOrientation`, `ScrollBarOrientation`) alongside the variant,
since orientation changes layout, not just paint.

Several widgets have multi-method style traits where chrome
decomposes into named slots (e.g. `TabStyle::make_body` +
`make_bar`). See [Multi-method styles](#tier-3-style-protocols) below
for the full list.

Set per-call: `Button::new(lit!("Save")).variant(ButtonVariant::Outlined)`.
Set per-app via a custom Tier-3 style that *defaults* a variant for
unspecified callers.

**IntUI variant policy.** Int UI is intentionally minimalist about
button styling, destructive actions live in confirmation dialogs
where the body carries the warning, not the button. So the IntUI
`RecipeButtonStyle` collapses several variants:
`Destructive` → Filled, `Tinted`/`Outlined` → Plain, `Link` → Ghost.
Other design languages (Material 3, Fluent, macOS) honour them
distinctly.

## Tier 2: Recipes

Recipes are pure data describing paint vocabulary. They live in
[`teksilo-core/src/styles/recipe.rs`](../crates/teksilo-core/src/styles/recipe.rs).
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
    pub style: BorderStyle,           // Solid | Dashed { dash, gap } | Dotted { gap }
    pub position: BorderPosition,     // Inside | Center | Outside (now honoured)
    pub sides: Option<BorderSides>,   // None = uniform; Some = per-side widths
}
// BorderSides { top, trailing, bottom, leading: f32 }, e.g.
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
`PerStateRecipe<T>` with an explicit fallback chain, Teksilo's
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
`ColorProp`), `Static | Surface(SurfaceRole) | Border(BorderRole)
| Text(TextRole)`, so the full theme cascade still applies but the
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

## Tier 3: Style protocols

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
`WidgetId` as its root child. Everything else, background, border,
focus ring, padding, min size, is the style's responsibility.

The trait is `'static` only (not `Send + Sync`) because all Teksilo
trees are single-threaded by construction; `Rc<dyn FooStyle>` is the
public alias (`SharedButtonStyle` and friends).

**Same shape across widgets.** The style traits live in
[`teksilo-core/src/styles/`](../crates/teksilo-core/src/styles/), one per
`ComponentStyleSlots` slot. All but three return `WidgetId` from their
`make_*` methods and take a `*StyleConfig` describing the inputs that
vary by widget; the exceptions are all-recipe traits that hand back
paint data instead, `ChartStyle` (`bar_fill` / `area_fill` /
`donut_fill` / `gridline`), `GridViewStyle` (`focus_ring` / `marquee` /
`insertion` / `pinned_header_surface`) and `TextSelectionStyle`
(`handle` / `magnifier`). The trait
is the public API; everything below it is implementation. The full
list lives in the [migration status table](https://github.com/ferntech-eu/teksilo/blob/main/engineering/docs/styling-migration.md).

### Worked example: a Material-3-flavoured Button

```rust
use std::rc::Rc;

use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::{ButtonStyle, ButtonStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, SurfaceRole};
use teksilo_widgets::primitives::{Padding, RectWidget, ZStack};

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
                .child(cfg.label),
        );

        ctx.add(ZStack::new().child(rect).child(padded_label))
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
[`teksilo-widgets/src/button.rs`](../crates/teksilo-widgets/src/button.rs)
under `theme_slot_supplies_button_style_when_no_override` /
`per_call_style_override_wins_over_theme_slot`.

## Built-in presets

| Preset | Where | Status |
| --- | --- | --- |
| `intui::light` / `intui::dark` | `teksilo_core::presets::intui` | shipped, the default look |
| `material3::light` / `material3::dark` | `teksilo-theme-material3` crate | shipped, Material 3 |
| `fluent::light` / `fluent::dark` | `teksilo-theme-fluent` crate | shipped, Windows 11 / WinUI 3 |
| `macos::light` / `macos::dark` | `teksilo-theme-macos` crate | shipped, macOS Aqua / Dark Aqua |
| Image-backed themes | `teksilo-image-theme` crate | not yet shipped |

Each preset is just a function returning `Theme`. Apps can write their
own without depending on any sibling crate:

```rust
pub fn brutalist_light() -> Theme {
    let mut theme = intui::light();
    theme.colors.accent     = Color::new(1.0, 0.0, 0.4, 1.0);   // hot pink
    theme.shape.radius_control = 0.0;                           // sharp control corners
    theme.style_slots.button   = Some(Rc::new(MyBrutalistButton));
    theme.style_slots.checkbox = Some(Rc::new(MyBrutalistCheckbox));
    theme
}
```


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

The trait pattern doesn't require buying into Teksilo's slot bag,
you can ship the trait + default impl and let users override via
`MyWidget::style(...)` per call. The slot bag is for theme-wide
installation; it's optional, but it's how the framework's themable
widgets get reskinned across an app.

## See also

- [docs/reactive-theme.md](reactive-theme.md), Signal-backed Theme,
  color signals, theme swaps without rebuild.
- [docs/widgets-overview.md](widgets-overview.md), per-widget
  variant + style trait references.
- [docs/accessibility-overrides.md](accessibility-overrides.md),
  style trait impls do **not** participate in accessibility; the
  widget owns its `accessibility(builder)` regardless of which style
  is installed.
