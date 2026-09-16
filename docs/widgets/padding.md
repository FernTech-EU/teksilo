<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Padding

![Padding preview](img/padding.png)

Padding — a single-child layout container that adds insets around its child.

`Padding` shrink-wraps a child widget and enlarges it by configurable insets
on each of the four sides. Horizontal insets are **leading/trailing**
(logical), not left/right (physical), so they flip automatically in RTL
locales. Each inset accepts a static `f32` or a reactive `Signal<f32>`; a
bound inset schedules a relayout whenever the signal fires, so theme-derived
spacing values take effect without rebuilding the widget tree.

The grow weight, shrink weight, and compression floor reported by the child
are forwarded through the padding so a flexible or shrinkable child inside a
`Padding` stays flexible or shrinkable from the parent's perspective.

## When to use

- Adding whitespace around a widget without wrapping it in a stack.
- Applying asymmetric insets (e.g. extra leading inset for a list item).
- Reacting to a `Signal`-driven spacing token.

Use `Padding::uniform` when all four sides are equal, and
`Padding::symmetric` when horizontal and vertical insets differ.

```rust
# use teksilo_widgets::primitives::{Padding, TextWidget};
# use teksilo_i18n::lit;
// 12 dp padding on every side:
let _w = Padding::uniform(12.0)
    .child(TextWidget::new(lit!("Hello")));
```

## Density

The picture above is the widget at `TargetDensity::Compact`, the mouse-and-keyboard ladder. Below is the same subject on the same canvas with only the ladder changed, so what moves is the density and nothing else — where the subject no longer fits, that is what the denser targets cost it at that size. See `docs/density-and-targets.md`.

**Touch**

![Padding at Touch density](img/padding-touch.png)

## Builder methods at a glance

`uniform`, `symmetric`, `child`, `child_opt`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/padding/index.html)

## `pub struct Padding`

A layout container that adds padding (insets) around a single child.

See the `module documentation` for the full feature description and
an example. Construct with `Padding::new`, `Padding::uniform`, or
`Padding::symmetric`; attach a child with `.child(widget)` or
`.child(id)`.

```rust
pub struct Padding { /* fields */ }
```

### Methods

#### `pub fn new( top: impl Into<Prop<f32>>, trailing: impl Into<Prop<f32>>, bottom: impl Into<Prop<f32>>, leading: impl Into<Prop<f32>>, ) -> Self`

Create a padding with explicit per-side insets.

Argument order mirrors CSS shorthand: `(top, trailing, bottom, leading)`.
`trailing` and `leading` are **logical** — they map to physical right and
left in LTR and are swapped in RTL.

#### `pub fn uniform(amount: impl Into<Prop<f32>>) -> Self`

Create a padding with the same inset on all four sides.

#### `pub fn symmetric(vertical: impl Into<Prop<f32>>, horizontal: impl Into<Prop<f32>>) -> Self`

Create a padding with equal top/bottom insets and equal leading/trailing insets.

`vertical` applies to both top and bottom; `horizontal` applies to both
leading and trailing sides (logical, RTL-aware).

#### `pub fn child(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self`

Set an inline child widget (deferred insertion).

#### `pub fn child_opt(self, widget: Option<impl teksilo_core::IntoTeksiChild>) -> Self`

Attach `widget` when it is `Some`, and do nothing when it is `None`.

The conditional-child form. `teksu!`'s `if` without an `else` lowers to
this, and it is what `cond.then(|| w)` is for in a builder chain. `None`
adds no arena node, so nothing is laid out, painted, or published to the
accessibility tree, and a stack applies no spacing around it.
