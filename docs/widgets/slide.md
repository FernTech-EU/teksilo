<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Slide

`Slide` — wraps a child and slides it in or out from a chosen
edge when an external `Signal<bool>` toggles. Common patterns:
drawers, snackbars, side panels, banner notifications.

```ignore
let visible = ctx.signal(false);
ctx.add(
    Slide::new(visible.clone())
        .from(SlideEdge::Bottom)
        .child(snackbar_content),
);
// ...elsewhere:
visible.set(true);   // slides in from below
```

## Layout semantics

`Slide`'s own slot stays in its laid-out position; the child is
*translated* within the slot via `place_children`. The wrapper
clips so a sliding-in child doesn't bleed past the slot edges.
The wrapper reports the child's full natural size at all
progress values — siblings don't reflow as the child slides.

For a "slide + fade" effect (notification snackbar), wrap the
child in `Fade` before passing it to `Slide`:

```rust
# use bastyde_widgets::animations::{Slide, Fade, SlideEdge};
# use bastyde_widgets::primitives::TextWidget;
# use bastyde_core::signal::Signal;
# use bastyde_i18n::lit;
# let visible = Signal::new(false);
# let snackbar_content = TextWidget::new(lit!("Changes saved"));
let _w = Slide::new(visible.clone())
    .from(SlideEdge::Bottom)
    .child(Fade::new(visible).child(snackbar_content));
```

## Reduced motion

Honours `prefers-reduced-motion`: snaps the child instantly into
or out of position instead of tweening.

## Builder methods at a glance

`from`, `child`, `child_id`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/animations/slide/index.html)

## `pub enum SlideEdge`

Which edge the child slides in from / out to.

`Leading` and `Trailing` honour layout direction (RTL flips them);
the resolution happens in `place_children` via the layout context.

```rust
pub enum SlideEdge { /* variants */ }
```

### Variants

- **`Leading`** — Slide from the leading edge (left in LTR, right in RTL). Suits drawers and side panels.
- **`Trailing`** — Slide from the trailing edge (right in LTR, left in RTL).
- **`Top`** — Slide from the top edge. Suits drop-down banners or navigation bars.
- **`Bottom`** — Slide from the bottom edge. Suits snackbars and bottom sheets.

## `pub struct Slide`

Wraps a child widget and translates it in or out from one edge of
its slot whenever `visible` flips.

```rust
pub struct Slide { /* fields */ }
```

### Methods

#### `pub fn new(visible: impl Into<Prop<bool>>) -> Self`

Create a slide wrapper bound to `visible`; accepts a static `bool`
or a reactive `Signal<bool>`. Defaults to [`SlideEdge::Bottom`] —
override with `.from(...)`.

#### `pub fn from(mut self, edge: SlideEdge) -> Self`

Edge the child slides in from (and out to).

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Inline child widget (deferred insertion).

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Pre-registered child by `WidgetId`.
