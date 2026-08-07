<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Card

Card — a surface container with optional header, content, and footer slots.

`Card` renders an opaque or tinted rounded-rectangle backdrop, an optional
drop shadow, and up to three stacked content slots (header / content /
footer). It is the standard building block for list-item cards, dashboard
tiles, onboarding panels, and any widget that needs a visually distinct
raised or outlined surface. Chrome (shadow, background, corner radius,
padding) is delegated to the active `CardStyle`
so the visual language can be changed per-call (`.style(...)`) or
theme-wide via `theme.style_slots.card`.

## When to use

- `CardVariant::Elevated` — a dashboard tile or list card that should
  "float" above the page surface.
- `CardVariant::Outlined` — a bordered grouping box without shadow.
- `CardVariant::Plain` — the content sits on the default surface; no
  visible chrome (useful for spacing only).

## Accessibility

Announces as `Role::Group`. The slots' own accessibility nodes are
included in the subtree; the card itself carries no additional AT name.

```rust
# use teksilo_widgets::Card;
# use teksilo_core::styles::CardVariant;
# use teksilo_widgets::primitives::TextWidget;
# use teksilo_i18n::lit;
let _card = Card::new()
    .variant(CardVariant::Elevated)
    .content(TextWidget::new(lit!("Hello, card!")));
```

## Builder methods at a glance

`header`, `header_id`, `content`, `content_id`, `footer`, `footer_id`, `shadow`, `background`, `corner_radius`, `padding`, `variant`, `style`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/card/index.html)

## `pub struct Card`

A card container with shadow, background, and optional header/content/footer.

```rust
pub struct Card { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Construct an empty card with no slots and the default `CardVariant::Plain`.

#### `pub fn header(mut self, widget: impl Widget + 'static) -> Self`

Set the header slot (topmost section) to an inline widget.

#### `pub fn header_id(mut self, id: WidgetId) -> Self`

Set the header slot to a pre-registered `WidgetId`.

#### `pub fn content(mut self, widget: impl Widget + 'static) -> Self`

Set the main content slot (middle section) to an inline widget.

#### `pub fn content_id(mut self, id: WidgetId) -> Self`

Set the main content slot to a pre-registered `WidgetId`.

#### `pub fn footer(mut self, widget: impl Widget + 'static) -> Self`

Set the footer slot (bottommost section) to an inline widget.

#### `pub fn footer_id(mut self, id: WidgetId) -> Self`

Set the footer slot to a pre-registered `WidgetId`.

#### `pub fn shadow(mut self, shadow: Shadow) -> Self`

Override the drop shadow. Accepts a `Shadow` token (see
`teksilo_tokens::Shadow`). The default shadow comes from the active
`CardStyle` for the chosen `CardVariant`.

#### `pub fn background(mut self, color: impl Into<ColorProp>) -> Self`

Override the background. Default (unset) is the variant's default
(`SurfaceRole::Main` for Plain/Outlined/Elevated, `SurfaceRole::Raised`
for Filled). Accepts `Color`, a role, or `Signal<Color>`.

#### `pub fn corner_radius(mut self, radius: impl Into<Prop<f32>>) -> Self`

Override the corner radius (default: theme `components.card.corner_radius`).
Accepts a static `f32` or a reactive `Signal<f32>`.

#### `pub fn padding(mut self, padding: impl Into<Prop<f32>>) -> Self`

Override the padding (default: theme `components.card.padding`).
Accepts a static `f32` or a reactive `Signal<f32>`.

#### `pub fn variant(mut self, variant: CardVariant) -> Self`

Pick the design-language variant. Default `Plain`. The active
`CardStyle` decides what each variant means visually (the IntUI
default maps Plain → no shadow + surface_main, Elevated →
shadow_md + surface_main, Outlined → border + surface_main,
Filled → shadow_md + surface_raised).

#### `pub fn style(mut self, style: impl teksilo_core::styles::CardStyle) -> Self`

Per-call style override. Replaces the theme-wide default
`CardStyle` for just this Card instance.
