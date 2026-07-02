<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ProgressBar

ProgressBar — a bar showing progress from 0.0 to 1.0.

Supports determinate (fixed or reactive value), indeterminate (animated
sweep), horizontal, and vertical orientations. The stationary chrome (track
and determinate fill) is delegated to `ProgressBarStyle`; the indeterminate
sweep is widget-owned (motion infrastructure is not chrome). Three paint
paths exist internally:

- **Horizontal indeterminate** uses the shader-driven animated-quad
  pipeline. `ProgressBar::build` registers an `AnimatedQuadHandle`
  and mounts a single `IndeterminateSweepLeaf` whose `paint()`
  issues one `draw_animated_quad` per frame; the shader composes
  the track + moving fill in a procedural draw. The recipe frame
  is NOT mounted in this case (the shader self-paints both).
- **Vertical indeterminate** keeps the signal-based path. The
  recipe frame paints the track; an `IndeterminateSweepLeaf` in
  signal mode paints a moving fill rect on top driven by a
  `Signal<f32>::animate_looping`.
- **Determinate** mounts the recipe frame only; the frame paints
  the track plus a proportional fill rect.

```rust
# use bastyde_widgets::ProgressBar;
# use bastyde_core::signal::Signal;
// Static determinate bar at 70 %:
let _bar = ProgressBar::new(0.7).thickness(6.0);

// Reactive determinate bar:
let progress = Signal::new(0.0_f32);
let _bar = ProgressBar::new(0.0).value(progress);

// Indeterminate (animated sweep):
let _spinner_bar = ProgressBar::indeterminate();
```

## Builder methods at a glance

`indeterminate`, `value`, `orientation`, `thickness`, `track_color`, `fill_color`, `style`, `label`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/progress_bar/index.html)

## `pub struct ProgressBar`

A progress bar — determinate or indeterminate, horizontal or vertical.

```rust
pub struct ProgressBar { /* fields */ }
```

### Methods

#### `pub fn new(value: f32) -> Self`

Create a determinate progress bar with a static value (0.0–1.0).

#### `pub fn indeterminate() -> Self`

Create an indeterminate progress bar (animated sweep).

#### `pub fn value(mut self, state: impl Into<Prop<f32>>) -> Self`

Bind the progress value to a reactive state.

#### `pub fn orientation(mut self, orientation: Orientation) -> Self`

Set the bar's orientation. Default is `Orientation::Horizontal`.
Vertical bars use the shader-driven animation path only for horizontal;
vertical indeterminate bars use the signal-driven path instead.

#### `pub fn thickness(mut self, thickness: f32) -> Self`

Set the bar's narrow dimension in logical pixels. For horizontal bars
this is the height; for vertical bars this is the width. Default is 4.0.

#### `pub fn track_color(mut self, color: impl Into<ColorProp>) -> Self`

Override the track background. Default (unset) is `SurfaceRole::Sunken`.
Accepts `Color`, roles, or `Signal<Color>`.

#### `pub fn fill_color(mut self, color: impl Into<ColorProp>) -> Self`

Override the fill / sweep color. Default (unset) is `SurfaceRole::Accent`.
Accepts `Color`, roles, or `Signal<Color>`.

#### `pub fn style(mut self, style: impl bastyde_core::styles::ProgressBarStyle) -> Self`

Per-call style override for the stationary chrome (track +
determinate fill). The indeterminate sweep is widget-owned and
always uses the shader-quad / signal-driven path described in
the module doc; the style supplies the sweep's *colour*
recipe via `fill_color_override` / `track_color_override`.

#### `pub fn label(mut self, text: impl Into<LocalizedString>) -> Self`

Accessible name for the progress bar.
