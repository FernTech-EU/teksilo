<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TwistArrow

TwistArrow — a small chevron that indicates and toggles a tree node's expansion.

Renders a right-pointing arrow when collapsed and a down-pointing arrow when
expanded; a leaf node (where `has_children` is false) paints nothing but
reserves its slot so the indent column stays aligned across all rows.
The glyph flips direction under right-to-left layout.
Accessibility-decorative: the chevron hides itself from the AT tree and
the parent row's node owns `set_expanded`.

```ignore
// TwistArrow is typically instantiated by TreeView row delegates and requires
// an EventContext to wire the tap callback. The snippet below shows the
// construction pattern used inside a custom tree-row build().
let arrow = TwistArrow::new(16.0, true, false)
    .on_click(|ctx| ctx.send_intent(bastyde_core::Intent::new("tree.toggle")));
```

## Builder methods at a glance

`on_click`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/primitives/twist_arrow/index.html)

## `pub struct TwistArrow`

Small interactive chevron rendered in the leading indent column of a tree row.

```rust
pub struct TwistArrow { /* fields */ }
```

### Methods

#### `pub fn new(size: f32, has_children: bool, expanded: bool) -> Self`

Construct a chevron. `size` is the square side length in logical pixels;
`has_children` determines whether the glyph is painted; `expanded`
determines the glyph direction (down = expanded, right/left = collapsed).

#### `pub fn on_click(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Install a tap handler. Receives the firing `EventContext`
so consumers can dispatch intents (e.g. lazy-load children on
expand) or open dialogs from the chevron toggle.
