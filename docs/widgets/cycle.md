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

Internally a [`Switcher`] whose
`Signal<usize>` index is incremented by a per-frame effect.
Children share a `ZStack` slot — at any given moment only the
selected child is visible (others are dormant).

## Reduced motion

Honours `prefers-reduced-motion`: pins on the first child and
does not install the timer driver. Subsequent children are still
built (so widget construction is identical) but are never shown.

## Builder methods at a glance

`period`, `child`, `child_boxed`, `children`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/animations/cycle/index.html)

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

#### `pub fn child_boxed(mut self, widget: Box<dyn Widget>) -> Self`

Append a pre-boxed child to the rotation.

#### `pub fn children(mut self, iter: impl IntoIterator<Item = impl Widget + 'static>) -> Self`

Append children from an iterator.
