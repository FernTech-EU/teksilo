<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# MinSize

MinSize — a layout modifier that ensures a child reaches a minimum width and/or height.

The child's reported size is clamped upward so it never falls below the
configured minimum on each constrained axis. The minimum is also forwarded
as part of the clamped proposal so that wrap-aware children (e.g. a
multi-line `TextWidget`) measure against the constraint they will actually
be placed into. Axes with no minimum set are passed through unchanged.

`MinSize` propagates the child's `flex` and `shrink` weights so that a
`Spacer` or `Expand` inside `MinSize` still participates in stack
slack-distribution; the child's own compression floor is composed with
the `MinSize` floor.

For the inverse operation (capping a maximum size) see `MaxSize`.

# Hit targets: take the floor from the theme

`MinSize` is the mechanism for a minimum hit box, but the *number* belongs to
the active density rather than to the call site. Route it through
`density_min_size` (or
`dp(.., TargetRole::Target, ..)`), which raises the named axes to
`theme.input.target_size` — 24 dp at `Compact`, 32 at `Comfortable`, 44 at
`Touch`, and 48 under the Material 3 preset. That is what every shipped
recipe does; see `docs/density-and-targets.md`.

```rust
# use teksilo_widgets::primitives::{MinSize, icon_widget::IconWidget};
# use teksilo_core::styles::density::density_min_size;
# use teksilo_tokens::{InputTokens, TargetAxes, TargetDensity};
# let tokens = InputTokens::for_density(TargetDensity::Touch);
# let base = teksilo_canvas::Size::new(20.0, 20.0);
// The density decides the floor; the call site only says "both axes".
let min = density_min_size(base, TargetAxes::BOTH, &tokens);
let _tap_target = MinSize::new(min.width, min.height)
    .child(IconWidget::checkmark(20.0));
```

A hard-coded literal is right only where the number is a *design* dimension
that must not move with density. Where it is a target, name its bar: 24 dp is
WCAG 2.2 SC 2.5.8 *Target Size (Minimum)*, level AA — the floor
`min_target_conformance` holds at every density; 44 dp is Apple's HIG minimum
and SC 2.5.5 *Target Size (Enhanced)*, level AAA, which is what the `Touch`
ladder aims for. 44 dp is never the AA figure.

## Builder methods at a glance

`width`, `height`, `min_width`, `min_height`, `child_id`, `child`, `child_opt`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/min_size/index.html)

## `pub struct MinSize`

Layout modifier that enforces a minimum width and/or height on a single child widget.

Constraints can be static or bound to a reactive `Signal<f32>` for dynamic resizing.

```rust
pub struct MinSize { /* fields */ }
```

### Methods

#### `pub fn new(width: f32, height: f32) -> Self`

Enforce a minimum on both axes: the child's width will be at least `width` and its height at least `height`.

#### `pub fn width(width: f32) -> Self`

Enforce a minimum only on the width axis; the height axis is unconstrained by this modifier.

#### `pub fn height(height: f32) -> Self`

Enforce a minimum only on the height axis; the width axis is unconstrained by this modifier.

#### `pub fn min_width(mut self, state: impl Into<Prop<f32>>) -> Self`

Bind min width to a reactive state.

#### `pub fn min_height(mut self, state: impl Into<Prop<f32>>) -> Self`

Bind min height to a reactive state.

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Set child by pre-registered ID.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Set an inline child widget (deferred insertion).

#### `pub fn child_opt(self, widget: Option<impl Widget + 'static>) -> Self`

Attach `widget` when it is `Some`, and do nothing when it is `None`.

The conditional-child form. `teksu!`'s `if` without an `else` lowers to
this, and it is what `cond.then(|| w)` is for in a builder chain. `None`
adds no arena node, so nothing is laid out, painted, or published to the
accessibility tree, and a stack applies no spacing around it.
