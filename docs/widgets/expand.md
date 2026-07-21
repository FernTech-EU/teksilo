<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Expand

Expand — a layout modifier that claims slack space in a stack and
stretches its child to fill the allocated bounds.

Inside an `HStack` or
`VStack`, `Expand` participates in the flex
distribution pass by reporting a non-zero `flex` weight (default `1.0`).
The parent stack distributes leftover space proportionally to each child's
flex weight. `Expand::new()` competes on **both axes**;
`Expand::horizontal()` and `Expand::vertical()` restrict competition to
the named axis so they do not accidentally steal slack from orthogonal
siblings. By default the wrapped child is stretched to the full allocated
rectangle; call `.align_child(alignment)` to keep the child at its natural
size and align it within the slot instead.

The default flex basis is **zero** (CSS `flex-basis: 0`), giving exact
proportional ratios. Call `.respect_intrinsic()` to switch to **auto**
basis where the child's natural size acts as a floor before flex slack is
added.

```rust
# use bastyde_widgets::primitives::{HStack, Expand, RectWidget};
// Two panels sharing horizontal space in a 1:2 ratio
let _row = HStack::new()
    .child(Expand::new().flex(1.0).child(RectWidget::new()))
    .child(Expand::new().flex(2.0).child(RectWidget::new()));
```

## Builder methods at a glance

`horizontal`, `vertical`, `flex`, `align_child`, `respect_intrinsic`, `child_id`, `child`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/primitives/expand/index.html)

## `pub struct Expand`

Layout modifier that claims space along one or both axes from its parent
and stretches its child to fill it.

In an `HStack` / `VStack`, `Expand` participates in flex slack
distribution: it returns a `LayoutResponse` with `flex` (default `1.0`),
so the parent stack hands it a share of the leftover space proportional
to flex. Default basis is **zero** — the wrapped child's natural size
does NOT count in the rigid pool, which gives clean ratio layouts. Call
`Expand::respect_intrinsic` to switch to **auto** basis (CSS
flex-basis: auto), where the child's natural size acts as a floor and
flex adds slack on top.

`Expand::new()` is the common case: claim space, fill the child.
Use `.flex(n)` to change the ratio (e.g. 1:2 by pairing `flex(1)` with
`flex(2)`). Use `.align_child(...)` to opt out of fill and align the
child at its natural size within the claimed bounds.

**`horizontal()` / `vertical()` semantics.** The named axis is the one
the wrapper *competes for slack on*. Both sizing and flex behavior
follow from that:

- **Sizing:** when the parent binds an axis (`proposal.{axis} = Some`),
  the wrapper claims that axis regardless of its name. So
  `Expand::vertical(child)` inside a `VStack` (which binds width and
  leaves height open) fills the VStack's full width AND distributes
  vertical slack via flex. Cross-axis collapse to child intrinsic only
  happens when the parent left that axis open too.

- **Flex contribution:** the wrapper reports its `flex` weight only on
  axes the parent is distributing (i.e. left open). `Expand::horizontal()`
  inside a `VStack` reports `flex = 0` on the open vertical axis, so it
  does NOT compete for vertical slack with siblings — it just claims
  the cross-axis width and sits at its child's intrinsic height. Symmetric
  for `Expand::vertical()` inside an `HStack`.

```rust
pub struct Expand { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Expand on both axes. Default `flex(1)`, child fills bounds.

#### `pub fn horizontal() -> Self`

Compete for slack on the horizontal axis only. Inside an `HStack`,
distributes flex on width while claiming bound height as-is. Inside
a `VStack` (which binds width and distributes height), claims the
VStack's full width but reports `flex = 0` so it doesn't steal
vertical slack from siblings — height stays at child intrinsic.

#### `pub fn vertical() -> Self`

Compete for slack on the vertical axis only. Inside a `VStack`,
distributes flex on height while claiming bound width as-is. Inside
an `HStack` (which binds height and distributes width), claims the
HStack's full height but reports `flex = 0` so it doesn't steal
horizontal slack from siblings — width stays at child intrinsic.

#### `pub fn flex(mut self, flex: f32) -> Self`

Override the flex weight reported to a parent stack. `flex(0)` opts
out of slack distribution (the wrapper still claims any offered
proposal, useful inside non-stack containers). Default: `1.0`.

#### `pub fn align_child(mut self, alignment: Alignment) -> Self`

Opt out of stretching the child. The child is laid out at its
natural size and positioned within the Expand's bounds according
to `alignment`.

#### `pub fn respect_intrinsic(mut self) -> Self`

Switch to **auto** flex basis — the wrapped child's natural size
acts as a floor on each flex axis, and the parent stack adds slack
on top via the flex weight. Useful when the wrapper sits inside an
unconstrained parent (e.g. an outer `VStack` with `height = None`),
where the default zero-basis would let the child overflow because
the parent has no bound to share.

Trade-off: with `respect_intrinsic`, exact ratios bend by content
width — `[Expand::flex(1).child(60), Expand::flex(2).child(40)]` in
300 px gives `60 + 66 = 126` and `40 + 133 = 173` rather than
`100 / 200`. Without it (the default), the same layout splits
exactly `100 / 200`.

# Do not use this inside a *bounded* parent

The floor is a hard one: `Expand` reports `shrink = 0`, so if the
child's natural size exceeds what the parent can offer, the resulting
over-constraint deficit **cannot be absorbed** and later siblings are
pushed outside the bounds.

This bites hardest with children whose natural size is large and
content-driven. A vertical `TabBar`
answers an unbounded height query with its *stacked* height — every tab,
one below another. So:

```ignore
// 21 tabs => the bar's natural height is ~1050 dp.
VStack::new()
    .child(Expand::vertical().respect_intrinsic().child(tab_widget))
    .child(status_bar)
```

makes the `VStack` want `1050 + status_bar`, at *every* window size. The
status bar is placed at y=1050 and stays below the fold until the window
is grown past it — the bar never scrolls, because it was never asked to
fit. Dropping `respect_intrinsic()` fixes it: the bar takes the slack
left after the status bar and scrolls its tabs internally.

Rule of thumb: reach for this only when the parent genuinely has no
bound to share (`height = None`). When the parent is bounded — a window
root, a sized pane — the default zero basis is what you want.

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Set child by pre-registered ID.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Set an inline child widget (deferred insertion).
