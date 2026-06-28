<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Divider

Divider — a themed separator line that visually partitions content.

`Divider` renders a single hairline stroke (`DIVIDER_THICKNESS` = 1 dp by
default) using the theme's divider color. It comes in two orientations:
horizontal (the default, spans the proposed width and has a fixed 1 dp
height) and vertical (spans the proposed height, 1 dp wide). Both the
thickness and the color can be overridden per-instance without a custom
style.

## Accessibility

The widget emits `Role::Splitter`, which matches the ARIA separator pattern
and signals a structural boundary to screen readers.

```rust
# use bastyde_widgets::primitives::Divider;
// Horizontal rule between two content sections
let _rule = Divider::new();

// Vertical rule inside a toolbar
let _vbar = Divider::vertical();
```

## Builder methods at a glance

`horizontal`, `vertical`, `thickness`, `color`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/primitives/divider/index.html)

## `pub struct Divider`

A themed separator line. Thickness defaults to `DividerStyle::thickness`
and the color defaults to `BorderRole::Divider`; both can be overridden.

```rust
pub struct Divider { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create a horizontal `Divider` with default theme thickness and color.

#### `pub fn horizontal() -> Self`

Create a horizontal `Divider` — alias for `Divider::new()`.

#### `pub fn vertical() -> Self`

Create a vertical `Divider` that spans the proposed height.

#### `pub fn thickness(mut self, thickness: f32) -> Self`

Override the stroke thickness in logical pixels; defaults to
[`DIVIDER_THICKNESS`] (1 dp).

#### `pub fn color(mut self, color: impl Into<ColorProp>) -> Self`

Override the line color. Accepts `Color`, a role (typically
`BorderRole`), or a `Signal<Color>`.

## `pub const DIVIDER_THICKNESS`

Default visual thickness of a `Divider` stroke. Divider has no
per-widget `Recipe*Style` module, so the constant lives alongside
the widget that reads it.

```rust
pub const DIVIDER_THICKNESS: f32 = 1.0;
```
