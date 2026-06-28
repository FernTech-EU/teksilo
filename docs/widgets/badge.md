<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Badge

Badge — a pill-shaped label for tags, status indicators, and counts.

`Badge` renders a short piece of text inside a rounded-pill container.
Common uses include tag chips on list items, unread-count bubbles in
navigation rails, and severity labels in alert rows. The pill chrome
(corner radius, padding, surface tint) is driven by the active
`BadgeStyle`; callers may swap it per-instance (`.style(...)`) or
theme-wide via `theme.style_slots.badge`.

## When to use

- Inline chip that annotates another widget (version tag, "NEW" label).
- Standalone count indicator; pair with `SeverityBadge` for icon-backed
  status glyphs.

## Accessibility

Announces as `Role::Label` with its resolved text as the AT name.
The inner `TextWidget` is hidden from AT to avoid double-announcement.

```rust
# use bastyde_widgets::Badge;
# use bastyde_i18n::lit;
# use bastyde_tokens::Color;
let _badge = Badge::new(lit!("NEW"))
    .background(Color::new(0.2, 0.6, 1.0, 1.0));
```

## Builder methods at a glance

`style`, `background`, `text_role`, `text_style`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/badge/index.html)

## `pub struct Badge`

A pill-shaped label for displaying tags, counts, or status.

```rust
pub struct Badge { /* fields */ }
```

### Methods

#### `pub fn new(label: impl Into<LocalizedString>) -> Self`

Construct a badge with the given label text.

#### `pub fn style(mut self, style: impl bastyde_core::styles::BadgeStyle) -> Self`

Per-call style override for the badge pill chrome. Replaces the
theme-wide default `BadgeStyle` for just this instance.

#### `pub fn background(mut self, color: impl Into<ColorProp>) -> Self`

Override the badge background. Accepts `Color`, a
`SurfaceRole` / `TextRole`,
or a `Signal<Color>`. Default (unset) is `SurfaceRole::AccentSubtle`.

#### `pub fn text_role(mut self, color: impl Into<ColorProp>) -> Self`

Override the badge text color. Accepts `Color`, a role, or a signal.
Default (unset) is the theme's `status_info_fg`.

#### `pub fn text_style(mut self, style: impl Into<bastyde_core::color_prop::TextStyleProp>) -> Self`

Override the label's text style (font, size, weight). Accepts a
`TextStyleRole`, a `TextStyle`, or a `Signal` of either. Default
(unset) is `TextStyleRole::Tiny`.
