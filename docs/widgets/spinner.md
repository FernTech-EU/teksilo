<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Spinner

![Spinner preview](img/spinner.png)

`Spinner` — a shader-driven circular-arc loading indicator.

Uses the same per-slot uniform-buffer pipeline as
`ProgressBar::indeterminate` (an
`AnimatedQuadKind` variant), so per-frame cost is one
`queue.write_buffer(64 B) + draw_indexed` — the widget's `paint()`
does not re-run between frames and there's no signal-dirty-mark
cascade.

```rust
# use teksilo_widgets::Spinner;
# use teksilo_tokens::TextRole;
# use teksilo_i18n::lit;
let _s = Spinner::new(24.0)
    .color(TextRole::Secondary)
    .label(lit!("Loading"));
```

Defaults match the typical CSS spinner: a quarter-circle (90°)
arc rotating clockwise from the top, completing one full
rotation every 900 ms.

Honours `prefers-reduced-motion`: registers no animated quad and
falls back to a static three-quarter arc — the indicator is still
visible (so the user can tell the surface is busy) but doesn't
rotate.

## Builder methods at a glance

`period`, `arc_fraction`, `stroke_fraction`, `color`, `label`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/spinner/index.html)

## `pub struct Spinner`

A circular-arc loading indicator driven by a GPU shader quad.

Decorative — pair with `.label` to give screen readers
context. Honours `prefers-reduced-motion` by falling back to a static
three-quarter arc.

```rust
pub struct Spinner { /* fields */ }
```

### Methods

#### `pub fn new(size: f32) -> Self`

Construct a spinner of the given square edge length (logical
pixels). Use small sizes (16–24) for inline spinners and
larger (32–64) for full-content placeholders.

#### `pub fn period(mut self, period: Duration) -> Self`

Override the rotation period. Default: 900 ms (one full
rotation per period).

#### `pub fn arc_fraction(mut self, arc_fraction: f32) -> Self`

Override the arc length as a fraction of the full circle.
Default: 0.25 (a quarter-circle "comet tail" arc).

#### `pub fn stroke_fraction(mut self, stroke_fraction: f32) -> Self`

Override the stroke thickness as a fraction of the spinner's
edge length. Default: 0.12 (so a 24-px spinner has a ~3-px
stroke).

#### `pub fn color(mut self, color: impl Into<ColorProp>) -> Self`

Override the arc colour. Default: `TextRole::Secondary` so the
spinner picks up theme-aware text-tier styling.

#### `pub fn label(mut self, text: impl Into<LocalizedString>) -> Self`

Accessible name (e.g. "Loading", "Uploading file"). Without
this, screen readers announce a bare "progress indicator"
with no context.
