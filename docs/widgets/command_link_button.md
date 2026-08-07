<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# CommandLinkButton

CommandLinkButton — large two-line button with icon, title, and
subtitle. Used for wizard landing screens, onboarding choices, and
any "card-shaped CTA" pattern.

Modeled on Qt's `QCommandLinkButton`. Distinct from a regular
`Button` by its layout (`HStack(icon +
VStack(title + subtitle))`) and default visual variant (`Flat` —
Int UI convention — with an interactive surface tint on hover).

```ignore
CommandLinkButton::new(tr!(create_new_project()))
    .description(tr!(create_new_project_subtitle()))
    .icon(IconWidget::from_svg(NEW_PROJECT_ICON))
    .on_activate_fn(|ctx| ctx.send_intent(AppIntent::NewProject))
```

## Builder methods at a glance

`description`, `icon`, `enabled`, `on_activate_fn`, `title_style`, `description_style`, `title_color`, `description_color`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/command_link_button/index.html)

## `pub const COMMAND_LINK_BUTTON_ICON_SIZE`

CommandLinkButton design tokens. The widget is a group-4 composite
with no dedicated recipe module.

```rust
pub const COMMAND_LINK_BUTTON_ICON_SIZE: f32 = 28.0;
```

## `pub const COMMAND_LINK_BUTTON_ICON_TEXT_GAP`

```rust
pub const COMMAND_LINK_BUTTON_ICON_TEXT_GAP: f32 = 14.0;
```

## `pub const COMMAND_LINK_BUTTON_TITLE_DESCRIPTION_GAP`

```rust
pub const COMMAND_LINK_BUTTON_TITLE_DESCRIPTION_GAP: f32 = 4.0;
```

## `pub const COMMAND_LINK_BUTTON_PADDING_HORIZONTAL`

```rust
pub const COMMAND_LINK_BUTTON_PADDING_HORIZONTAL: f32 = 16.0;
```

## `pub const COMMAND_LINK_BUTTON_PADDING_VERTICAL`

```rust
pub const COMMAND_LINK_BUTTON_PADDING_VERTICAL: f32 = 14.0;
```

## `pub const COMMAND_LINK_BUTTON_MIN_HEIGHT`

```rust
pub const COMMAND_LINK_BUTTON_MIN_HEIGHT: f32 = 64.0;
```

## `pub struct CommandLinkButton`

A large two-line CTA button: icon + title + subtitle.

```rust
pub struct CommandLinkButton { /* fields */ }
```

### Methods

#### `pub fn new(title: impl Into<LocalizedString>) -> Self`

Create a `CommandLinkButton` with the given title text.
Chain `.description(...)` and `.icon(...)` to complete the card layout.

#### `pub fn description(mut self, text: impl Into<LocalizedString>) -> Self`

Optional descriptive subtitle rendered below the title.

#### `pub fn icon(mut self, icon: IconWidget) -> Self`

Leading icon — large enough to anchor the card visually
(rendered at 28 dp).

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Set the enabled state, statically or reactively. Forwarded to
the arena at build time.

#### `pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Closure invoked on activation. Use `ctx.send_intent(...)` to
route through the Action / Intent system.

#### `pub fn title_style( mut self, style: impl Into<teksilo_core::color_prop::TextStyleProp>, ) -> Self`

Override the title's text style (font, size, weight). Accepts a
`TextStyleRole`, a `TextStyle`, or a `Signal` of either. Default
(unset) is `TextStyleRole::BodyBold`.

#### `pub fn description_style( mut self, style: impl Into<teksilo_core::color_prop::TextStyleProp>, ) -> Self`

Override the description's text style. Default is `TextStyleRole::Body`.

#### `pub fn title_color(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the title's text color. Accepts `Color`, a role, or a
`Signal` of either. Default (unset) is `TextRole::Primary`.

#### `pub fn description_color( mut self, color: impl Into<teksilo_core::color_prop::ColorProp>, ) -> Self`

Override the description's text color. Default is `TextRole::Secondary`.

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain single-line tooltip shown after a hover delay.
Clears any previously set rich or composite tooltip.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip looked up by registry key.
Clears any previously set plain or composite tooltip.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip with inline content (no registry lookup).
Clears any previously set plain or composite tooltip.

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip hosting an arbitrary widget tree body.
Clears any previously set plain or rich tooltip.
