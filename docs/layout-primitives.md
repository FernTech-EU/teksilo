# Layout Primitives

**Companion to:** [architecture.md](architecture.md) §2 (Layout Model)
**Scope:** Reference for the layout primitives in [crates/bastyde-widgets/src/primitives/](../crates/bastyde-widgets/src/primitives/) — the containers and size wrappers every other widget composes against.

This document is a working reference: each primitive comes with a one-line summary, the public surface as you'd actually call it, the rule the layout engine applies, and at least one runnable example. Where two primitives can express the same intent, the trade-off is called out explicitly.

---

## 1. Mental model

Bastyde layout is a SwiftUI-style two-phase negotiation, recursive over the widget tree:

1. The parent calls `child.layout_response(proposal, ctx)`. The child returns a `LayoutResponse { size: Size, flex: f32 }` — the size it wants (a floor) plus a flex weight for slack distribution.
2. The parent calls `child.place_children(bounds, …)` once it has decided how much space each child gets and where to put it.

`SizeProposal { width: Option<f32>, height: Option<f32> }` is the parent's offer. `Some(_)` means *use this exact value*; `None` means *measure yourself, this axis is open*. Stacks pass `None` on their main axis to let children declare their wanted size, and `Some(bounds.cross)` on the cross axis to let children fill it.

**Three rules underlie every primitive in this document:**

- **Honest sizing.** A widget that knows its size returns it. A widget that wants slack returns `flex > 0`. The parent makes the placement decision; the child does not place itself.
- **Slack is a single rule.** In an `HStack`/`VStack`, `slack = bounds.main − Σ wanted − Σ spacing`; each child's final size is `wanted + (flex / Σ flex) × slack`. There is no special "spacer" or "expand" branch in the engine — `Spacer` and `Expand` are ordinary widgets that report `flex > 0`.
- **Logical pixels, Leading / Trailing.** All values in `f32` logical px; the renderer multiplies by scale factor at the boundary. `Leading` / `Trailing` flip with `LayoutDirection::RightToLeft`.

Everything in the rest of this document follows from those three rules.

```text
Stacks (HStack/VStack)              ZStack                  Grid
┌─ HStack ───────────────────────┐  ┌─ ZStack ─────────┐    ┌─ Grid ────────┐
│ A │ B │     slack     │ C │    │  │ ┌───────┐         │    │ A │ B │ C │
│   │   │  ←→ via flex  │   │    │  │ │  bg   │ ┌─fg─┐ │    ├───┼───┼───┤
└─  └─  └─────────────  └─  └─  │  │ └───────┘ └────┘ │    │ D │ E │ F │
                                   │   align=center      │    └───┴───┴───┘
                                   └────────────────────┘
```

---

## 2. The stack containers

Three containers cover almost everything: `VStack`, `HStack`, `ZStack`. They share the *deferred children* idiom — `.child(widget)` queues an inline child, `.add_child(id)` references a pre-registered `WidgetId`, `.children(iter)` adds many at once, `.child_opt(opt)` is a no-op when `None`. Pick whichever fits the call site; you can mix them on one builder.

### 2.1 `VStack` — vertical stack

[crates/bastyde-widgets/src/primitives/vstack.rs](../crates/bastyde-widgets/src/primitives/vstack.rs)

Lays children top-to-bottom. Cross-axis (horizontal) alignment is `HAlignment` — default `Leading`. Spacing accepts a static `f32` or a `Signal<f32>`.

```rust
use bastyde::prelude::*;

VStack::new()
    .spacing(8.0)
    .alignment(HAlignment::Center)
    .child(TextWidget::new_literal("Title").style(TextStyleRole::BodyBold))
    .child(TextWidget::new_literal("Subtitle"))
    .child(Button::new_literal("Save"))
```

**Sizing rule:** wants `Σ heights + spacing` on the main axis, `max(width)` on the cross axis. If any child reports `flex > 0` and the parent bounds the height, the VStack greedily claims the offered height so slack exists.

**Cross-axis floor.** Every child receives the VStack's full width as its `proposal.width`. A `TextWidget` in `TextOverflow::Wrap` will measure-and-wrap against that width; an `HStack` child fills that width.

### 2.2 `HStack` — horizontal stack

[crates/bastyde-widgets/src/primitives/hstack.rs](../crates/bastyde-widgets/src/primitives/hstack.rs)

Mirror of `VStack`. Cross-axis (vertical) alignment is `VAlignment` — default `Center`. **RTL-aware:** in `LayoutDirection::RightToLeft`, children are placed right-to-left automatically. There is no manual mirroring.

```rust
// Inside build(): bind spacing reactively to a theme token.
let gap = ctx.theme_signal().map(|t| t.layout.control_gap);

HStack::new()
    .spacing(gap)
    .alignment(VAlignment::Center)
    .child(IconWidget::checkmark(16.0))
    .child(TextWidget::new_literal("Save"))
    .child(Spacer::new())
    .child(Button::new_literal("Cancel"))           // pushed to trailing edge
```

### 2.3 `ZStack` — overlay stack

[crates/bastyde-widgets/src/primitives/zstack.rs](../crates/bastyde-widgets/src/primitives/zstack.rs)

Children overlap; later children paint on top. Size is the max of children's intrinsic sizes; the proposal is *only* used as a fallback when no child has a queryable size. Container-level alignment is a full `Alignment` (both axes); per-child override via `tree.set_alignment(id, …)`.

```rust
ZStack::new()
    .alignment(Alignment::TOP_TRAILING)
    .child(image_view)                              // the background
    .child(                                          // close button in the corner
        Button::new_literal("×")
            .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Close)),
    )
```

A common pattern: full-bleed background + foreground. Background widgets that report `0×0` for an unspecified proposal (e.g. `RectWidget::new()`) **do not** inflate the stack — only children with non-zero intrinsic size do. The `place_children` call then proposes the full ZStack bounds to every child, so an unsized background fills it.

### 2.4 Per-child alignment override

Container-level alignment applies uniformly. To diverge for one child, call `tree.set_alignment(child_id, Alignment::BOTTOM_TRAILING)`. The override always takes a full two-axis `Alignment`; an `HStack` reads only the vertical axis, a `VStack` reads only the horizontal axis, a `ZStack` reads both. The override lives on the arena node, so it survives reactive theme switches, language flips, and reordering.

---

## 3. Slack and flex

Slack is the leftover space inside a stack after every child's wanted size and the inter-child spacing have been honored. It's distributed proportionally to each child's `flex` weight. **Default flex is 0** (rigid). Two primitives ship `flex > 0`:

### 3.1 `Spacer` — fills available space

[crates/bastyde-widgets/src/primitives/spacer.rs](../crates/bastyde-widgets/src/primitives/spacer.rs)

Returns `LayoutResponse::flexible(Size::new(min, min), 1.0)`. The min-length is a floor on the main axis (default 0); the parent stack adds slack share on top.

```rust
HStack::new()
    .child(label)
    .child(Spacer::new())
    .child(button)                                  // pushed to trailing

HStack::new()
    .child(Spacer::new())
    .child(label)
    .child(Spacer::new())                           // centers `label`

HStack::new()
    .child(a)
    .child(Spacer::new().min_length(20.0))           // ≥ 20 px gap, more if available
    .child(b)
```

### 3.2 `Expand` — claim space and fill a child

[crates/bastyde-widgets/src/primitives/expand.rs](../crates/bastyde-widgets/src/primitives/expand.rs)

`Expand` is the workhorse. It returns flex (default `1.0`) and stretches its single child to its allocated bounds. Unlike `Spacer`, it has a child.

```rust
// Single panel filling the rest of the row:
HStack::new()
    .child(sidebar)
    .child(Expand::new().child(main_panel))

// Ratio splits — Category-A flex layouts:
HStack::new()
    .child(Expand::new().flex(1.0).child(left))     // 1/3 of slack
    .child(Expand::new().flex(2.0).child(right))    // 2/3 of slack

// Single-axis variants — name the axis you compete for slack on:
VStack::new()
    .child(header)                                   // intrinsic height
    .child(Expand::vertical().child(content))        // takes remaining vertical
    .child(footer)

// Opt out of fill — align the child at its natural size in claimed space:
Expand::new()
    .align_child(Alignment::CENTER)                  // == Center::new()
    .child(label)
```

#### Zero-basis vs auto-basis (CSS analog)

By default `Expand` reports `wanted = 0` on its flex axes. That's CSS `flex-basis: 0` — slack divides cleanly by weight, regardless of the child's natural size. `[Expand::flex(1).child(60), Expand::flex(2).child(40)]` in 300 px splits exactly **100 / 200**.

Switch with `.respect_intrinsic()` (CSS `flex-basis: auto`) when the parent is unconstrained on the flex axis. The child's natural size acts as a floor and slack is added on top. Use this inside an outer `VStack` with `height = None`, where zero-basis would let the child overflow because the parent has no bound to share.

**Trade-off (called out at [expand.rs:130](../crates/bastyde-widgets/src/primitives/expand.rs#L130)):** with `respect_intrinsic`, exact ratios bend by content. The same `[1, 2]` split inside a 300 px parent now gives `60 + 66 = 126` and `40 + 133 = 173` rather than `100 / 200`. Keep zero-basis for ratio layouts and reach for `respect_intrinsic` only when you actually need the floor.

#### `horizontal()` / `vertical()` semantics

The named axis is the one the wrapper *competes for slack on*. Cross-axis behavior depends on whether the parent bound that axis:

- `Expand::vertical()` inside a `VStack` (parent binds width, distributes height) — fills the VStack's full width AND distributes vertical slack.
- `Expand::horizontal()` inside a `VStack` — claims the VStack's full width, but reports `flex = 0` on the open vertical axis. It does **not** steal vertical slack from siblings — height stays at child intrinsic.

Symmetric for HStack. The behavior is documented and tested at [expand.rs:25-41](../crates/bastyde-widgets/src/primitives/expand.rs#L25-L41).

### 3.3 `Center` — sugar for centered fill

[crates/bastyde-widgets/src/primitives/center.rs](../crates/bastyde-widgets/src/primitives/center.rs)

`Center::new().child(w)` is exactly `Expand::new().align_child(Alignment::CENTER).child(w)`. Reach for it when that's all you mean — the name is clearer.

```rust
Center::new().child(spinner)
```

Claims all offered space; the child sits at its natural size, centered.

---

## 4. Size wrappers

Five primitives constrain what their child can be:

| Wrapper | Rule | When to use |
| --- | --- | --- |
| `FixedSize` | Child reports `bound.width` / `bound.height` (or its natural size on unbound axes); parent proposal is ignored on bound axes. | Dialog widths from settings, animated panel widths. |
| `MinSize` | Child's wanted size is clamped *upward* on each constrained axis. | Touch targets (`MinSize::new(48.0, 48.0)`), readable column widths. |
| `MaxSize` | Child's wanted size is clamped *downward*. Sets `clips_children: true` so overflow is scissored. | Reading-width caps (`MaxSize::width(640.0)`), modal max-height. |
| `AspectRatio` | Wanted size fits within proposal at a fixed `width / height`. | Image previews, video tiles, square avatars. |
| `Padding` | Wraps a child with insets; child receives `proposal − insets`, parent reports `child + insets`. | Inner spacing inside cards, dialogs, list rows. |

### 4.1 `FixedSize`

[crates/bastyde-widgets/src/primitives/fixed_size.rs](../crates/bastyde-widgets/src/primitives/fixed_size.rs)

```rust
// Static width, child decides height:
FixedSize::new().bind_width(280.0).child(content)

// Reactive — animated sidebar:
let sidebar_width = ctx.animated_signal(280.0);
let sidebar = FixedSize::new()
    .bind_width(sidebar_width.clone())
    .child(sidebar_content);

// Later, on toggle:
ctx.animate().normal().standard().to_or_snap(&sidebar_width, 0.0);
```

Both `bind_width` and `bind_height` accept `impl Into<Prop<f32>>` — pass an `f32` for static, a `Signal<f32>` for reactive. The bound proposal is **forwarded to the child**, so wrap-aware children (TextWidget in `TextOverflow::Wrap`, ScrollArea, etc.) measure against the right constraint.

**Without** any binding, `FixedSize` just reports the child's natural size and ignores the parent proposal. That's how you opt a widget out of stretching inside an `HStack` where siblings expand.

### 4.2 `MinSize`

[crates/bastyde-widgets/src/primitives/min_size.rs](../crates/bastyde-widgets/src/primitives/min_size.rs)

```rust
// 48×48 minimum touch target — the Button composite uses this internally:
MinSize::new(48.0, 48.0).child(content)

// Single axis:
MinSize::width(120.0).child(label)
MinSize::height(36.0).child(row)

// Reactive:
MinSize::width(0.0).bind_min_width(min_w_signal).child(text)
```

The proposal forwarded to the child is **clamped upward** to the minimum. A wrapping `TextWidget` inside `MinSize::width(100)` measures against `width >= 100`, so its wrapped height reflects the minimum width — not the unconstrained natural width. Tested at [min_size.rs:230-258](../crates/bastyde-widgets/src/primitives/min_size.rs#L230-L258).

### 4.3 `MaxSize`

[crates/bastyde-widgets/src/primitives/max_size.rs](../crates/bastyde-widgets/src/primitives/max_size.rs)

```rust
// Reading-width cap on a long article:
MaxSize::width(640.0).child(article_text)

// Both axes — modal content with hard ceiling:
MaxSize::new(800.0, 600.0).child(dialog_content)

// Reactive — user-resizable panel:
MaxSize::width(9999.0).bind_max_width(panel_width).child(content)
```

Symmetric to `MinSize`: proposal clamped *downward*, wanted size clamped downward. **Sets `clips_children: true`** when any constraint is active — content that exceeds the cap is scissored, not bled. Hidden from the accessibility tree (`builder.set_hidden()`).

### 4.4 `AspectRatio`

[crates/bastyde-widgets/src/primitives/aspect_ratio.rs](../crates/bastyde-widgets/src/primitives/aspect_ratio.rs)

```rust
AspectRatio::widescreen().child(video_thumbnail)     // 16:9
AspectRatio::square().child(avatar)                  // 1:1
AspectRatio::new(4.0 / 3.0).child(legacy_photo)
```

Picks the largest size matching the ratio that fits the proposal. Given `width = Some(w)`, height is `w / ratio`. Given `height = Some(h)`, width is `h × ratio`. Given both, fits within both. Given neither, returns `0×0` — always wrap an unconstrained `AspectRatio` in a parent that bounds at least one axis.

The child fills the resolved bounds.

### 4.5 `Padding`

[crates/bastyde-widgets/src/primitives/padding.rs](../crates/bastyde-widgets/src/primitives/padding.rs)

```rust
// All four insets:
Padding::new(16.0, 24.0, 16.0, 24.0).child(content)  // top, right, bottom, left

// Symmetric — vertical and horizontal pairs:
Padding::symmetric(12.0, 16.0).child(content)

// Uniform:
Padding::uniform(16.0).child(content)

// Reactive — track a theme-derived inset:
let pad = ctx.theme_signal().map(|t| t.layout.section_gap);
Padding::uniform(pad).child(content)
```

All four arguments accept `impl Into<Prop<f32>>` — static or reactive. The child is proposed `parent − insets`; the wrapper reports `child + insets`. No alignment — the child is anchored to the inner top-leading corner and stretched to fill the inner rect.

---

## 5. The grid and flow containers

For tables of mixed-size content, multi-column flow, and form-style label/field pairs.

### 5.1 `Grid` — explicit row and column tracks

[crates/bastyde-widgets/src/primitives/grid.rs](../crates/bastyde-widgets/src/primitives/grid.rs)

Children are placed in **row-major order** — child *i* goes to row `i / cols`, column `i % cols`. Tracks come in three sizing modes:

```rust
use bastyde::widgets::{Grid, TrackSize};

// 3 columns: [auto | 1fr | 80px], 2 rows of intrinsic height
Grid::new()
    .columns(vec![
        TrackSize::Auto,
        TrackSize::Fractional(1.0),
        TrackSize::Fixed(80.0),
    ])
    .rows(vec![TrackSize::Auto, TrackSize::Auto])
    .column_gap(8.0)
    .row_gap(4.0)
    .child(label_a)
    .child(field_a)
    .child(unit_a)
    .child(label_b)
    .child(field_b)
    .child(unit_b)
```

- **`Fixed(px)`** — exactly that many logical pixels.
- **`Auto`** — sized to the largest child intrinsic size in that track.
- **`Fractional(weight)`** — splits remaining space (after Fixed and Auto are claimed) by weight.

**Two-pass layout.** Auto tracks are resolved against children's unspecified-proposal width. Fractional tracks then take the remainder. Children that landed in Fractional columns *narrower* than their intrinsic single-line width are re-measured at the resolved column width — wrapping content reports its actual wrapped height instead of bleeding outside its cell. See [grid.rs:159-223](../crates/bastyde-widgets/src/primitives/grid.rs#L159-L223) for the reasoning.

Both `column_gap` and `row_gap` accept `impl Into<Prop<f32>>`.

### 5.2 `Wrap` — line-breaking flow

[crates/bastyde-widgets/src/primitives/wrap.rs](../crates/bastyde-widgets/src/primitives/wrap.rs)

A horizontal flow that wraps to the next line when a child won't fit. Each child keeps its intrinsic size; lines are packed greedily.

```rust
Wrap::new()
    .spacing(8.0)                  // between items on a line
    .line_spacing(4.0)              // between lines
    .children(tag_strings.iter().map(|t| Badge::new_literal(t.clone())))
```

Reports total height = `Σ line heights + line gaps`, where each line's height is the max child height on that line. Width reports the longest line (so an unconstrained `Wrap` collapses to its widest single-line case — wrap it in something that bounds width to actually trigger wrapping).

Use cases: tag clouds, toolbar overflow, chip lists, breadcrumb segments that fold to a second line on narrow windows.

### 5.3 `MasonryLayout` — Pinterest-style packing

[crates/bastyde-widgets/src/primitives/masonry.rs](../crates/bastyde-widgets/src/primitives/masonry.rs)

Variable-height grid where each child slots into the **shortest column** at the time. Column count is fixed; column width is `(available_width − gaps) / columns`. RTL-aware (column 0 is the rightmost in RTL).

```rust
MasonryLayout::new(3)               // 3 columns
    .column_spacing(12.0)
    .item_spacing(8.0)
    .children(photos.iter().map(|p| PhotoCard::new(p.clone())))
```

Each child is queried at column-width to get its real height, then placed under the shortest column. Ties break leftmost-first. Used for heterogeneous-height cards where you want dense packing without the rigid row breaks of a grid.

**When to choose:** masonry over grid when item heights vary a lot and you don't mind that the visual row alignment is broken; grid over masonry when columns must align horizontally.

### 5.4 `FormLayout` — two-column label / field

[crates/bastyde-widgets/src/primitives/form_layout.rs](../crates/bastyde-widgets/src/primitives/form_layout.rs)

A specialized two-column layout: label column auto-sizes to the widest label, field column takes the rest. Supports full-width rows for separators or wide inputs.

```rust
// host, port, timeout are Signal<String> / Signal<u16> / Signal<u32>.
FormLayout::new()
    .label_gap(12.0)
    .row_spacing(8.0)
    .label(tr!("connection_settings"))           // emits Role::Form landmark
    .line(TextWidget::new(tr!("host")), TextInput::new(host))
    .line(TextWidget::new(tr!("port")), SpinBox::new(port, 0u16, 65535u16))
    .full_width(Divider::new())
    .full_width(GroupHeader::new(tr!("advanced")))
    .line(TextWidget::new(tr!("timeout_ms")), TextInput::new(timeout))
```

- `.line(label, field)` adds a paired row.
- `.full_width(widget)` adds a row spanning both columns — sections, dividers, full-width inputs.
- `.label(LocalizedString)` opts in to the `Role::Form` accessibility landmark with that name. **Without** a label, the layout demotes to `GenericContainer` — an unnamed landmark hurts AT users more than it helps. Pass `tr!(…)` directly.

Row height is `max(label.height, field.height)`. The label column width is the widest label intrinsic — every row's label cell is sized to that uniform width, so the field columns line up vertically across all rows.

### 5.5 `Switcher` — show one child at a time

[crates/bastyde-widgets/src/primitives/switcher.rs](../crates/bastyde-widgets/src/primitives/switcher.rs)

Internally a `ZStack` where each child has a `visible_when` binding derived from `selected.map(|i| i == index)`. Layout is the size of the active child.

```rust
let page = Signal::new(0_usize);

Switcher::new(page.clone())
    .child(welcome_view)
    .child(settings_view)
    .child(about_view)

// Elsewhere: page.set(2);   // jumps to about_view
```

Use for tab content, wizard pages, or any "one of N visible" pattern. Hidden from the accessibility tree itself — the visible child supplies the AT presentation. `Switcher::capture_child_ids_into(rc)` exposes child IDs to callers that need to wire AT relationships (TabWidget does this for the `Tab → TabPanel` `controls` link).

---

## 6. Spacers and visual separators

### 6.1 `Divider` — themed separator line

[crates/bastyde-widgets/src/primitives/divider.rs](../crates/bastyde-widgets/src/primitives/divider.rs)

A 1 px (theme-tokenable) line. Horizontal by default, fills the proposal's main axis, claims `thickness` on the cross axis.

```rust
VStack::new()
    .child(header)
    .child(Divider::new())                          // full-width horizontal rule
    .child(body)

HStack::new()
    .child(left_pane)
    .child(Divider::vertical().thickness(2.0).color(BorderRole::Strong))
    .child(right_pane)
```

`color()` accepts the full `ColorProp` range — `Color`, a role (typically `BorderRole`), or `Signal<Color>`. Defaults to `BorderRole::Divider`. Emits `Role::Splitter` to AT.

Note: `Divider` is a *visual* separator, not a draggable splitter — for drag-to-resize panes, use `SplitView` from bastyde-widgets.

### 6.2 Spacing summary

| Need | Use |
| --- | --- |
| Push siblings to the edges | `Spacer::new()` |
| Hard gap with grow-if-available | `Spacer::new().min_length(n)` |
| Static gap between siblings | `HStack::new().spacing(n)` / `VStack::new().spacing(n)` |
| Visual divider line | `Divider::new()` |
| Inset around a child | `Padding::uniform(n)` / `Padding::symmetric(v, h)` / `Padding::new(t, r, b, l)` |

---

## 7. When to use which

| Goal | Reach for |
| --- | --- |
| Vertical column of widgets | `VStack` |
| Horizontal row, RTL-safe | `HStack` |
| Background + foreground on the same area | `ZStack` |
| Push to one edge | `Spacer` in a stack |
| Equal split (1:1, 1:2, …) | `Expand::flex(n)` pairs in a stack |
| One panel takes the rest | `Expand::new().child(panel)` |
| Center one child | `Center::new().child(w)` |
| Force a minimum touch area | `MinSize::new(48.0, 48.0)` |
| Cap reading width | `MaxSize::width(640.0)` |
| Dialog with a fixed width | `FixedSize::new().bind_width(w)` |
| Animated panel width | `FixedSize::bind_width(animated_signal)` |
| Locked aspect ratio (image, video) | `AspectRatio::new(w/h)` |
| Inner spacing | `Padding` |
| Tabular data with mixed track sizes | `Grid` |
| Tag cloud / toolbar overflow | `Wrap` |
| Pinterest-style heterogeneous cards | `MasonryLayout` |
| Settings forms | `FormLayout` |
| Tab pages / wizard steps | `Switcher` |

When two primitives could express the same thing, prefer the more specific one — the name is a hint to the next reader. `Center::new()` instead of `Expand::new().align_child(CENTER)`. `Spacer::new()` instead of `Expand::new()` when you mean "empty pushable region." `MinSize::new(48, 48)` instead of `FixedSize::bind_width(48.0).bind_height(48.0)` when you mean "at least," not "exactly."

---

## 8. Reactive sizing

Every size constraint that takes an `impl Into<Prop<f32>>` is reactive. Pass an `f32` for a static value, a `Signal<f32>` for reactive, or use `BuildContext::animated_signal(value)` for an animatable one.

Whenever a bound size value changes, the framework dirty-marks the wrapper for **relayout** (not just repaint). The relayout starts at the highest dirty ancestor and runs `layout_response` + `place_children` for each dirty subtree; clean subtrees are skipped. This is the same incremental-layout model browsers and Qt use.

```rust
// Animated drawer:
let drawer_w = ctx.animated_signal(0.0);
let drawer = FixedSize::new()
    .bind_width(drawer_w.clone())
    .child(drawer_content);

let toggle = Button::new_literal("Open")
    .on_activate_fn({
        let drawer_w = drawer_w.clone();
        move |ctx| {
            let target = if drawer_w.get() > 0.0 { 0.0 } else { 280.0 };
            ctx.animate().normal().standard().to_or_snap(&drawer_w, target);
        }
    });
```

Behavior under `prefers-reduced-motion`: `to_or_snap` snaps the value instead of tweening. The relayout still fires, just once instead of per-frame.

---

## 9. Composing your own

A custom layout container is an ordinary `Widget` that returns children from `build()`, picks a wanted size in `layout_response`, and places its children in `place_children`. The layout engine doesn't care whether a widget is shipped in bastyde-widgets or written in your app crate.

```rust
use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

#[derive(Debug)]
struct StaggeredColumn {
    child_ids: Vec<WidgetId>,
    offset: f32,
}

impl Widget for StaggeredColumn {
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let child_proposal = SizeProposal { width: proposal.width, height: None };
        let mut total_h = 0.0;
        let mut max_w = 0.0_f32;
        for &id in &self.child_ids {
            if let Some(s) = ctx.child_size(id, child_proposal) {
                total_h += s.height;
                max_w = max_w.max(s.width + self.offset);
            }
        }
        Size::new(proposal.width.unwrap_or(max_w), total_h).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let mut y = bounds.y;
        for (i, child) in children.iter_mut().enumerate() {
            let s = ctx.child_size(child.id, SizeProposal::with_width(bounds.width))
                .unwrap_or(Size::ZERO);
            let dx = self.offset * (i as f32);
            child.origin = Point::new(bounds.x + dx, y);
            child.size = Size::new(s.width.min(bounds.width - dx), s.height);
            y += s.height;
        }
    }

    fn children(&self) -> Vec<WidgetId> { self.child_ids.clone() }
}
```

Three things to remember:

- **Don't close over `bounds.width` for the child proposal in `layout_response`.** That measures children against the parent's offered width, not the wrapper's bounds. Use `proposal.width`.
- **Match `place_children`'s child query to your sizing policy.** If `layout_response` queried with `SizeProposal::with_width(w)`, query the same way in `place_children` — otherwise wrapping children measure twice with different results.
- **Honor flex.** If your layout wants stacks-style slack distribution, sum `child_layout_response(...).flex` and apply the standard rule. If your layout doesn't distribute slack, ignore flex; that's fine.

For testing, [crates/bastyde-core/src/test_widgets.rs](../crates/bastyde-core/src/test_widgets.rs) ships `FillWidget` and `StackWidget` (pub(crate)); for end-to-end layout tests use `WidgetTree` directly with `tree.layout(SizeProposal::exact(w, h))` and assert `tree.bounds(id)`.

---

## 10. References

- Architecture: [architecture.md §2 Layout Model](architecture.md)
- Reactive layer: [reactive-theme.md](reactive-theme.md), `Signal<T>` / `Prop<T>` in [crates/bastyde-core/src/signal.rs](../crates/bastyde-core/src/signal.rs)
- Animation tied to layout: [animation.md](animation.md)
- Custom widget patterns: [`Widget` trait](../crates/bastyde-core/src/widget.rs), [BuildContext](../crates/bastyde-core/src/build_context.rs)
- Visual tour: `cargo run -p widget-catalog`, `cargo run -p text-and-layout`, `cargo run -p data-grid`
