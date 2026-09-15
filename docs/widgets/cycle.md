<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Cycle

`Cycle` — show one of N children at a time, advancing on a fixed
period. The "rotating loading tip" / status display pattern.

```ignore
ctx.add(
    Cycle::new()
        .period(Duration::from_secs(3))
        .child(TextWidget::new(lit!("Tip: press Cmd-K to search")))
        .child(TextWidget::new(lit!("Tip: hold Shift to multi-select")))
        .child(TextWidget::new(lit!("Tip: drag the divider to resize"))),
);
```

Internally a `Switcher` whose
`Signal<usize>` index is incremented by a per-frame effect.
Children share a `ZStack` slot — at any given moment only the
selected child is visible (others are dormant).

## Reduced motion

Honours `prefers-reduced-motion`: pins on the first child and
does not install the timer driver. Subsequent children are still
built (so widget construction is identical) but are never shown.

## Builder methods at a glance

`period`, `child`, `child_opt`, `child_boxed`, `children_boxed`, `children`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/animations/cycle/index.html)

## `pub struct Cycle`

A wrapper that cycles through its children on a fixed period.

```rust
pub struct Cycle { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

New cycle with default 3 s period.

#### `pub fn period(mut self, period: Duration) -> Self`

Step interval — how long each child is visible before
advancing to the next. Default 3 s.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Append a child to the rotation.

#### `pub fn child_opt(self, widget: Option<impl Widget + 'static>) -> Self`

Attach `widget` when it is `Some`, and do nothing when it is `None`.

The conditional-child form. `teksu!`'s `if` without an `else` lowers to
this, and it is what `cond.then(|| w)` is for in a builder chain. `None`
adds no arena node, so nothing is laid out, painted, or published to the
accessibility tree, and a stack applies no spacing around it.

#### `pub fn child_boxed(mut self, widget: Box<dyn Widget>) -> Self`

Append a pre-boxed child to the rotation.

#### `pub fn children_boxed(self, iter: impl IntoIterator<Item = Box<dyn Widget>>) -> Self`

Append several pre-boxed children to the rotation.

The heterogeneous twin of `children`: `children` needs
every child to be the same concrete type, this one does not.

#### `pub fn children(mut self, iter: impl IntoIterator<Item = impl Widget + 'static>) -> Self`

Append children from an iterator.
