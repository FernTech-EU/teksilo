# Reactive Theme Reference

> This doc covers the **reactive** layer — how `Theme` flows through
> `Signal`s, roles, and props so a theme swap repaints without a
> rebuild. For the broader styling picture (the four-tier ladder:
> tokens → variants → recipes → style protocols, `Theme` construction,
> per-widget `*Variant` enums and `*Style` traits), see
> [`styling-system.md`](styling-system.md).

Bastyde runs its theme through three layers of reactive primitives:

| Layer | Type | Lives on | Purpose |
|-------|------|----------|---------|
| **Root signal** | [`Signal<Theme>`](../crates/bastyde-core/src/signal.rs) | [`WidgetTree`](../crates/bastyde-core/src/widget_tree.rs) | Source of truth; `set_theme` fires this |
| **Role enums** | [`TextRole`](../crates/bastyde-tokens/src/roles.rs), `SurfaceRole`, `BorderRole`, `TextStyleRole` | `bastyde-tokens` | Name *what* a value represents, not *which* literal it is |
| **Props** | [`ColorProp`](../crates/bastyde-core/src/color_prop.rs), [`TextStyleProp`](../crates/bastyde-core/src/color_prop.rs) | `bastyde-core` | Unified input type accepted by widget builders |

The rules:

1. **`set_theme` never rebuilds.** It updates the signal and dirty-marks every node; the next layout/paint pass reads the new theme and repaints affected widgets. Focus, scroll offsets, expanded panels — all interaction state survives a theme switch.
2. **Roles resolve at paint/layout time.** A widget that stores `ColorProp::TextRole(TextRole::Primary)` looks up `ctx.theme.colors.text_primary` in its `paint` — never at build time.
3. **User code almost never needs to name `theme_signal`.** Builders accept roles directly; `Signal<Role>` covers the interaction-driven case.

---

## Quick reference

```rust
use bastyde::prelude::*;  // re-exports Color, Role enums, ColorProp, TextStyleProp

// Plain text — default role is TextRole::Primary.
TextWidget::new("Hello")

// Role-based color: resolved against current theme, reactive.
TextWidget::new("Error!").color(TextRole::Error)

// Role-based typography: same story, but the text style role.
TextWidget::new("Section").style(TextStyleRole::BodyBold)

// Static color: frozen literal.
TextWidget::new("Custom").color(Color::from_hex("#FF00FF"))

// Reactive signal (usually interaction state): repaints on signal change.
TextWidget::new("").bind_text(status).color(hover_color_signal)

// Panel with role-based surface and border.
Panel::new()
    .background(SurfaceRole::Raised)
    .border_color(BorderRole::Default)
    .corner_radius(8.0)

// Reactive SurfaceRole — the role itself depends on interaction state.
let bg_role = interaction.map(|s| match s {
    InteractionState::Hovered => SurfaceRole::Hover,
    InteractionState::Pressed => SurfaceRole::Pressed,
    _                          => SurfaceRole::Transparent,
});
RectWidget::new().background(bg_role)
```

---

## `WidgetTree::set_theme`

```rust
pub fn set_theme(&mut self, theme: Theme)
```

Declared at [`crates/bastyde-core/src/widget_tree.rs`](../crates/bastyde-core/src/widget_tree.rs) (around line 1120). Sequence:

1. `self.theme = theme.clone()` — the cached `&Theme` accessor still works.
2. `self.theme_signal.set(theme)` — fires observers (derived `Signal<Theme>`s, role-carrying `ColorProp`s via their bindings).
3. `self.arena.mark_all_dirty()` — every node needs layout and paint.

No `rebuild_built_widgets` call, no focus clearing. `set_locale` follows the same pattern on `locale_signal`.

For per-subtree overrides:

```rust
tree.set_theme_override(panel_id, |theme| {
    theme.colors.surface_main = Color::from_hex("#...");
});
```

This only marks the subtree dirty; layout/paint contexts resolve ancestor overrides via [`WidgetArena::resolve_theme`](../crates/bastyde-core/src/arena.rs).

---

## Constructing and loading themes

### Built-in presets

There is no `Theme::default()` / `Theme::*_default()`. `Theme` lives in
`bastyde-core::styles` and is built through a preset constructor:

```rust
use bastyde::prelude::intui;

let light = intui::light();   // bastyde_core::presets::intui::light
let dark  = intui::dark();
```

Both are neutral Int UI baselines — not visually distinctive, designed to be customized. Apps usually start from one of them and override the slots they care about. For the full styling picture (variants, recipes, style traits) see [`styling-system.md`](styling-system.md).

### Programmatic customization via struct spread

`Theme` (`bastyde-core::styles`) and the token structs `ColorTokens`, `TypographyTokens`, `ShapeTokens`, `LayoutTokens`, `MotionTokens` (`bastyde-tokens`) are plain structs. Override the fields you want and spread the rest from a preset base:

```rust
use bastyde_core::styles::Theme;
use bastyde_tokens::{ColorTokens, TypographyTokens, TextStyle, Color};
use bastyde::prelude::intui;

let editor_light = Theme {
    colors: ColorTokens {
        accent: Color::from_hex("#2E7D32"),
        accent_hover: Color::from_hex("#1B5E20"),
        text_on_accent: Color::WHITE,
        surface_main: Color::from_hex("#FAFAF5"),
        ..ColorTokens::light_default()
    },
    typography: TypographyTokens {
        body: TextStyle {
            family: "Literata".to_string(),
            size: 16.0,
            ..TextStyle::default()
        },
        ..TypographyTokens::default()
    },
    ..intui::light()
};

tree.set_theme(editor_light);
```

`ColorTokens::light_default()` and the other raw-token defaults still live in `bastyde-tokens` — only the `Theme`-level constructor moved.

The same pattern works for sub-trees via `set_theme_override(panel_id, |theme| { ... })` (see above).

### Loading from a file

`Theme` and every token struct derive `serde::Serialize` and `serde::Deserialize`, so themes round-trip through any serde format the app picks (TOML, JSON, RON, YAML). The `style_slots` and `extensions` fields are `#[serde(skip)]` — a deserialized `Theme` gets empty defaults for those, so style-trait overrides are re-installed in code, not loaded from the file. The runtime cost is one read + one deserialize + one `set_theme` call:

```rust
use std::fs;
use bastyde_core::styles::Theme;

let toml = fs::read_to_string("themes/editor-light.toml")?;
let theme: Theme = toml::from_str(&toml)?;
tree.set_theme(theme);
```

Authoring a theme file is the inverse — `toml::to_string(&intui::light())?` writes a complete starter file the user can edit.

### Partial files — current limitation

The token structs **do not** carry `#[serde(default)]` on their fields, so a file missing any field fails to deserialize. To accept hand-edited theme files that only specify a few overrides, the app needs to do the merge itself — typically by deserializing into an `Option`-wrapped or `serde_json::Value` shape, then folding non-null values onto a base produced by `intui::light()`. A future change can add `#[serde(default)]` so partial files merge automatically; until it lands, treat the file format as "all fields required."

---

## Role enums

All defined in [`crates/bastyde-tokens/src/roles.rs`](../crates/bastyde-tokens/src/roles.rs), exported from `bastyde_tokens::{TextRole, SurfaceRole, BorderRole, TextStyleRole}` and re-exported through `bastyde::prelude`.

### `TextRole`
Foreground text color.

`Primary` (default), `Secondary`, `Disabled`, `OnAccent`, `Accent`, `Error`, `Warning`, `Success`, `Link`, `LinkHover`, `TooltipText`, `TooltipShortcut`, `EditorFg`, `EditorGutterFg`.

### `SurfaceRole`
Filled-area color (panel backgrounds, button fills, selection highlights).

`Main` (default), `Content`, `Raised`, `Sunken`, `Hover`, `Pressed`, `Selected`, `SelectedInactive`, `Accent`, `AccentHover`, `AccentPressed`, `AccentDisabled`, `AccentSubtle`, `StatusInfo`, `StatusSuccess`, `StatusWarning`, `StatusError`, `TooltipBg`, `EditorBg`, `EditorCaret`, `EditorCurrentLineBg`, `EditorSelectionBg`, `Scrim`, **`Transparent`** (paints nothing — the "no surface" slot in interaction chains).

### `BorderRole`
Stroke color.

`Default` (default), `Strong`, `Focused`, `Error`, `Warning`, `Divider`, `DividerStrong`, `TooltipBorder`, **`Transparent`**.

### `TextStyleRole`
Typography role.

`Body` (default), `BodyBold`, `Small`, `SmallBold`, `Tiny`, `Mono`.

Every role has a `resolve(&ColorTokens)` (or `resolve(&TypographyTokens)` for `TextStyleRole`) method. Paint/layout code already calls those under the hood when reading a `ColorProp` / `TextStyleProp`.

**Adding a role.** Add the variant to the enum, extend `resolve(..)`, re-export from `bastyde::prelude`. Add a role only when more than one widget repeatedly wants the same token — otherwise a `.color(Color::..)` literal is fine.

---

## `ColorProp`

```rust
pub enum ColorProp {
    Static(Color),
    Bound(Signal<Color>),
    TextRole(TextRole),
    SurfaceRole(SurfaceRole),
    BorderRole(BorderRole),
    DynamicTextRole(Signal<TextRole>),
    DynamicSurfaceRole(Signal<SurfaceRole>),
    DynamicBorderRole(Signal<BorderRole>),
}
```

Widget color builders accept `impl Into<ColorProp>`. Every input shape above implements `From`, plus `From<Prop<Color>>` for migrating legacy callers. Call `cp.resolve(&theme)` in paint and `cp.register_if_bound(self_id, registry, level)` in build to hook signal-bearing variants into dirty-tracking.

Role variants need no binding registration — the tree-wide `mark_all_dirty` inside `set_theme` already forces a repaint.

### Which variant to use

| Case | Variant | How to construct |
|------|---------|------------------|
| "Normal" theme color | `TextRole` / `SurfaceRole` / `BorderRole` | `.color(TextRole::Primary)` |
| Interaction-dependent color (hover, focus, pressed) | `DynamicSurfaceRole` (etc.) | `.background(interaction.map(\|s\| match s { .. }))` |
| Brand / decoration color not in the theme | `Static` | `.color(Color::from_hex("#..."))` |
| Externally-provided signal (not tied to theme) | `Bound` | `.color(user_color_signal)` |
| Legacy code still using `Prop<Color>` | via `From` impl | pass-through; no changes required |

---

## `TextStyleProp`

```rust
pub enum TextStyleProp {
    Static(TextStyle),
    Role(TextStyleRole),
}
```

`TextWidget::style(...)` and the other style-accepting builders take `impl Into<TextStyleProp>`. Default is `TextStyleRole::Body`, so a bare `TextWidget::new("x")` follows the theme typography.

Resolved at paint/layout via `prop.resolve(&ctx.theme.typography)`. Changing `Theme::typography` on a running tree updates every `TextWidget` that uses a role; widgets that passed a raw `TextStyle` stay frozen (that's the user intent — custom fonts stay custom).

---

## Interaction-driven colors: the `Signal<Role>` pattern

For state-dependent colors (hover, pressed, focus, disabled), emit a `Signal<Role>` from the interaction signal and pass it as `ColorProp` directly — the paint layer handles the theme lookup. No explicit `theme_signal` zip.

### Template — `Button` (canonical example)

```rust
fn resolve_bg_role(style: ButtonVariant, state: InteractionState) -> SurfaceRole {
    match (style, state) {
        (ButtonVariant::Default, InteractionState::Hovered) => SurfaceRole::AccentHover,
        (ButtonVariant::Default, InteractionState::Pressed) => SurfaceRole::AccentPressed,
        (ButtonVariant::Default, _)                          => SurfaceRole::Accent,
        (ButtonVariant::Flat,    InteractionState::Hovered) => SurfaceRole::Hover,
        (ButtonVariant::Flat,    _)                          => SurfaceRole::Transparent,
        // ... Regular variant ...
    }
}

impl Widget for Button {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let interaction = ctx.signal(InteractionState::Idle);
        let style       = self.style;

        let bg_role     = interaction.map(move |s| resolve_bg_role(style, *s));
        let text_role   = interaction.map(move |s| resolve_text_role(style, *s));
        let border_role = interaction.map(move |s| resolve_border_role(style, *s));

        ctx.add(
            RectWidget::new()
                .background(bg_role)
                .border_color(border_role)
                // ...
        );
        // TextWidget inside picks up the text role the same way.
    }
}
```

The `interaction` signal is the *only* upstream root the role signals observe. When the user moves the mouse off the button, only `interaction` fires; when the theme changes, `mark_all_dirty` triggers the repaint and the paint-time `resolve(&theme)` picks up the new colors. Two separate triggers, same rendering path.

See [`crates/bastyde-widgets/src/button.rs`](../crates/bastyde-widgets/src/button.rs) for the full widget; [`menu_list::KeyboardHighlightWrapper`](../crates/bastyde-widgets/src/menu_list.rs) and [`combo_box::DropdownItem`](../crates/bastyde-widgets/src/combo_box/item.rs) are smaller walk-throughs.

### When `Signal<Role>` doesn't fit

Three cases keep an explicit `theme_signal`:

1. **Color transformations** (`token.with_alpha(0.2)` etc.) — no role represents "accent at 12 % alpha"; use `theme_signal.map(|t| t.colors.accent.with_alpha(0.12))` to get a `Signal<Color>`.
2. **Effects on external state** — the rich-text engine's per-frame palette, for instance. See [`primitives/text_input_field.rs`](../crates/bastyde-widgets/src/primitives/text_input_field.rs) for the `ctx.effect(&theme_signal, move |theme| { ... })` pattern.
3. **Layout snapshots** — `let shape = ctx.theme_signal().get().shape` at the top of `build()` captures corner radii; `let layout = ctx.theme_signal().get().layout` captures spacing values. These rarely differ across themes; the snapshot is fine.

---

## Dimension props (`Prop<f32>`)

Layout primitives accept `impl Into<Prop<f32>>` for dimensions that may come from a theme-derived signal:

| Primitive | Method | File |
|-----------|--------|------|
| `HStack` / `VStack` / `Wrap` | `.spacing(...)` | [primitives/hstack.rs](../crates/bastyde-widgets/src/primitives/hstack.rs), `vstack.rs`, `wrap.rs` |
| `Grid` | `.column_gap(...)` / `.row_gap(...)` | [primitives/grid.rs](../crates/bastyde-widgets/src/primitives/grid.rs) |
| `Padding` | `Padding::new / uniform / symmetric` | [primitives/padding.rs](../crates/bastyde-widgets/src/primitives/padding.rs) |
| `MinSize` / `MaxSize` / `FixedSize` | `.bind_width` / `.bind_height` / etc. | existing `.bind_*` builders |
| `RectWidget` | `.border_width(...)` / `.corner_radius(...)` | [primitives/rect_widget.rs](../crates/bastyde-widgets/src/primitives/rect_widget.rs) |

Pass a static `f32`, a `Signal<f32>`, or a `Prop<f32>`; the builder registers a `BindingLevel::Relayout` binding for signal variants so layout re-runs on theme-driven spacing changes.

---

## DX — what developers should write

**Good (most common paths):**

```rust
// "I want a normal label": zero color code.
TextWidget::new("Status")

// "I want an error-colored label": one role.
TextWidget::new(msg).color(TextRole::Error)

// "I want a raised panel": one role.
Panel::new().background(SurfaceRole::Raised).child(...)

// "I want a Bold Heading": one style role.
TextWidget::new("Settings").style(TextStyleRole::BodyBold)
```

**Good (custom / reactive):**

```rust
// Frozen brand color.
Panel::new().background(Color::from_hex("#e2007a"))

// Signal-driven non-theme color.
Panel::new().background(animated_banner_color)

// Interaction-driven role (the important pattern for new widgets).
let bg = interaction.map(|s| map_to_surface_role(*s));
RectWidget::new().background(bg)
```

**Avoid (legacy pattern — only keep when no role fits):**

```rust
// Don't write this for normal theme colors:
.bind_color(ctx.theme_signal().map(|t| t.colors.text_primary))

// Write this instead:
.color(TextRole::Primary)
```

---

## Migration cheat sheet

| Old form | New form |
|----------|----------|
| `.color(ctx.theme().colors.text_primary)` | `.color(TextRole::Primary)` (or drop — it's the default) |
| `.background(theme.colors.surface_raised)` | `.background(SurfaceRole::Raised)` |
| `.border_color(theme.colors.border)` | `.border_color(BorderRole::Default)` |
| `.style(theme.typography.body_bold.clone())` | `.style(TextStyleRole::BodyBold)` |
| `.bind_color(theme_signal.map(\|t\| t.colors.X))` | `.color(TextRole::X)` (if X has a role) |
| `.bind_background(theme_signal.map(\|t\| t.colors.X))` | `.background(SurfaceRole::X)` |
| `interaction.zip(&theme_signal).map(\|(s, t)\| resolve_bg(s, &t.colors))` | `interaction.map(\|s\| resolve_bg_role(s))` returning `Signal<SurfaceRole>` |

---

## Files to know

| File | Contents |
|------|----------|
| [`crates/bastyde-tokens/src/roles.rs`](../crates/bastyde-tokens/src/roles.rs) | Role enums + `resolve` |
| [`crates/bastyde-core/src/color_prop.rs`](../crates/bastyde-core/src/color_prop.rs) | `ColorProp`, `TextStyleProp`, `From` impls |
| [`crates/bastyde-core/src/widget_tree.rs`](../crates/bastyde-core/src/widget_tree.rs) | `set_theme`, `set_locale`, `theme_signal`, `locale_signal` |
| [`crates/bastyde-core/src/build_context.rs`](../crates/bastyde-core/src/build_context.rs) | `BuildContext::theme()`, `theme_signal()`, `locale_signal()` |
| [`crates/bastyde-widgets/src/button.rs`](../crates/bastyde-widgets/src/button.rs) | Canonical `Signal<Role>` pattern |
| [`crates/bastyde-widgets/src/primitives/text_widget.rs`](../crates/bastyde-widgets/src/primitives/text_widget.rs) | Default role usage, paint-time resolve |
| [`crates/bastyde-widgets/src/panel.rs`](../crates/bastyde-widgets/src/panel.rs) | `ColorProp` props + default fallbacks |
