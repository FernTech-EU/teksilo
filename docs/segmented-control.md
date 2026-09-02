<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SegmentedControl

A row of mutually exclusive segments — view mode, time period, document
view. Source: [crates/teksilo-widgets/src/segmented_control.rs](../crates/teksilo-widgets/src/segmented_control.rs).

Two things distinguish it from the rest of the radio family
([`RadioButton`](../crates/teksilo-widgets/src/radio_button.rs),
[`RadioTileGroup`](../crates/teksilo-widgets/src/radio_tile_group.rs)):
selection is **keyed**, not positional, and the control has a real
**width story** — segments that do not fit move into a chevron menu
rather than all of them compressing into ellipsised stubs.

```rust
const LIST: SegmentId = SegmentId::from_u64(1);
const GRID: SegmentId = SegmentId::from_u64(2);
const COLUMNS: SegmentId = SegmentId::from_u64(3);

let view = ctx.signal(Some(LIST));

SegmentedControl::new(view.clone())
    .label(tr!(view_mode()))
    .segment(Segment::new(tr!(list_view())).id(LIST).icon(|| IconWidget::list(14.0)))
    .segment(Segment::new(tr!(grid_view())).id(GRID).icon(|| IconWidget::grid(14.0)))
    .segment(Segment::new(tr!(columns())).id(COLUMNS))
```

---

## Identity

Selection is a `Signal<Option<SegmentId>>`.
[`SegmentId`](../crates/teksilo-widgets/src/segmented_control/id.rs)
mirrors [`TabId`](tab-widget.md): a `NonZeroU64` newtype with
`fresh()`, `from_raw()` / `raw()`, and a `const fn from_u64()` so an app
can declare its segments as constants.

`Segment::new(label)` allocates a fresh id, so a throwaway control needs
none. Declare them explicitly when the selection is **persisted**, or
when a segment can be **contributed** by another crate.

Why keyed at all? Because the positional alternative fails silently. Bind
a `Signal<usize>` to a control and a `Switcher`, let a plugin insert a
segment at position 1, and every index below it now points at the wrong
pane — with no error, no panic, and nothing in the type system to catch
it. `TabWidget` learned this already; this is the same fix.

Framework-allocated ids start at 2^48, so a small app constant —
`from_u64(1)`, the first thing anyone writes — can never collide with a
`fresh()` id.

### Pairing with a `Switcher`

`Switcher` is index-driven. `index_signal` is the adapter:

```rust
Switcher::new(segmented_control::index_signal(&view, &[LIST, GRID, COLUMNS]))
    .child(list_pane)
    .child(grid_pane)
    .child(columns_pane)
```

### When position really is the meaning

Some state is positional by construction: an enum discriminant over a
fixed `ALL` array, a settings choice, a preview knob. `indexed` binds a
`Signal<usize>` directly, mirrored both ways:

```rust
SegmentedControl::indexed(bucket_idx.clone())
    .segments([lit!("×2"), lit!("×4"), lit!("×8")])
```

Reach for it only when the segment list is **closed and local**. A
persisted selection, or segments another crate can contribute to, belong
on `new` — an index stops meaning the same thing the moment a segment is
inserted ahead of it, which is the whole reason selection is keyed.
Positions address the *declared* list, so hiding a segment does not
renumber the others.

---

## Width

By default ([`SegmentOverflow::Menu`]) segments that do not fit move into
a trailing chevron menu, and the rest keep a legible width.

Declaration order is stable, with exactly one exception: **the selected
segment is always visible.** If it would have been pushed into the menu
it takes the *last* slot, and it stays there until something else is
chosen from the menu — so the strip does not reshuffle under the pointer.
The promotion is forgotten once the control is wide enough for
everything, so a later unrelated narrowing starts from clean declaration
order instead of resurrecting a minutes-old pick.

```text
Declared: [A][B][C][D][E][F][G]   fits 4 + chevron

start, A selected     [A][B][C][D][v]   menu: E F G
pick F from menu      [A][B][C][F][v]   menu: D E G
click A (F stays)     [A][B][C][F][v]   menu: D E G
pick D from menu      [A][B][C][D][v]   menu: E F G
widen to full fit     [A][B][C][D][E][F][G]
```

This is deliberately *not* MRU. A bar whose items reorder by recency is
harder to use than one that does not — adaptive menus in Office are the
cautionary case. Only one slot ever moves, and only when you reach into
the menu.

`SegmentOverflow::Compress` opts out: every segment stays on the strip
and labels ellipsize, which is the right call for two or three short
segments that will never realistically overflow.

### Knobs

| Method | Effect |
| --- | --- |
| `.overflow(SegmentOverflow)` | `Menu` (default) or `Compress`. |
| `.sizing(SegmentSizing)` | `Uniform` (default — every visible segment the same width, measured against the widest) or `Fit` (each its own width, leftover shared). |
| `.display(SegmentDisplay)` | `Auto` (default) / `Text` / `Icon` / `IconText`. Icon-only fits far more segments, so it is worth reaching for *before* overflow engages; the label becomes the tooltip, and a segment with no icon falls back to its label so the mode is never a silent no-op. |
| `.fill_width(bool)` | `true` (default) claims the offered width; `false` hugs the segments and makes the control shrinkable, so an over-constrained stack compresses it instead of letting it spill. |

`is_overflowing() -> Signal<bool>` reports whether anything is currently
in the menu — republished from `place_children` behind an equality guard,
like [`Toolbar::is_overflowing`](widgets/toolbar.md). Safe for `RepaintOnly` /
`AccessibilityOnly` consumers, and for `Relayout` consumers that do not
feed back into this control's own width (a caption beside it is fine; a
container that resizes the control from it is not).

Widths come from real measurement
([`LayoutContext::measure_intrinsic`](../crates/teksilo-core/src/widget/layout_context.rs)),
including for segments currently in the menu — that is how the control
knows when they fit again. The height follows the measured content with
the 24 dp design constant as a **floor**, so a raised global text scale
grows the control rather than clipping it.

---

## Reactivity

| Method | Level |
| --- | --- |
| `.enabled(impl Into<Prop<bool>>)` | whole control |
| `Segment::disabled(impl Into<Prop<bool>>)` | per segment; read at event time, so a bound signal changes keyboard stepping with no rebuild |
| `Segment::visible(impl Into<Prop<bool>>)` | per segment; removes it from the strip, the menu, the keyboard order and the a11y tree |

*Hidden* and *overflowed* are different states: an overflowed segment is
still reachable from the chevron menu, a hidden one is not there at all.
Hiding is structural — it renumbers the live list — so it triggers a
rebuild; the keyed selection survives that, which is again why it is
keyed.

### `on_change`

```rust
.on_change(|id, ctx| ctx.set_locale(locale_for(id)))
```

Fires for user-driven changes — click, arrow key, assistive technology,
overflow menu — and hands over an `EventContext`, so the control can do
things a bare `Signal` write cannot. Programmatic writes to the bound
signal do not fire it: there is no event in flight to carry. Observe the
signal for those.

---

## Keyboard and accessibility

`Role::RadioGroup` on the control, with `active_descendant` pointing at
the selected segment and `Increment` / `Decrement` AT actions.
`Role::RadioButton` per segment, carrying "N of M" over the **whole**
segment list — segments in the overflow menu are still part of the set,
so the count deliberately exceeds the number of rendered radios on a
narrow control. `push_to_radio_group` lists only the segments actually on
the strip: a segment in the menu publishes no AccessKit node, and
referencing it would dangle.

| Key | Effect |
| --- | --- |
| ← / → | previous / next selectable segment, wrapping; RTL-swapped, resolved at event time so a locale flip needs no rebuild |
| Home / End | first / last selectable segment |

Disabled segments are skipped. Stepping onto a segment that is in the
overflow menu **promotes it into view**, so the keyboard reaches every
segment without opening the menu.

Name the group with `.label(...)`, matching `RadioGroup::label` /
`RadioTileGroup::label`. `.access_label(...)` also works — the control
itself is the semantic node.

### Tab stops

One while everything fits. Two while overflowing: the group, then the
chevron. An overflow menu no keyboard can reach is not an overflow menu,
and the chevron cannot join the arrow sequence because here arrows move
*selection*, not a roving focus (unlike [`Toolbar`](widgets/toolbar.md)).

Segments in the menu are dormant, so they are pruned from the
accessibility tree; their **menu rows** are their representation there,
rendered as real `Role::MenuItemRadio` rows. The open menu therefore
forms its own, smaller radio group with its own "N of M".

---

## Styling

Tier-3 [`SegmentedControlStyle`](../crates/teksilo-core/src/styles/segmented_control_style.rs),
via `.style(...)` per call or `theme.style_slots.segmented_control`
theme-wide. Default:
[`RecipeSegmentedControlStyle`](../crates/teksilo-widgets/src/styles/recipe_segmented_control_style.rs).

The chrome paints the frame, hover tint, selected-segment surface,
overflow divider and focus ring — never text or icons, which stay
composed widgets so they remain locale- and theme-reactive.

Because a control can overflow, the chrome cannot derive segment
rectangles by dividing its bounds by `n`. The widget publishes resolved
geometry each layout pass through `SegmentSlots`:

```rust
pub struct SegmentSlotGeometry {
    pub frame: Rect,
    pub segments: Vec<Rect>,   // one per visible slot, reading order
    pub order: Vec<usize>,     // order[slot] = live segment index
    pub overflow: Option<Rect>,
}
```

`order` is what maps a *segment* to a *slot*; the two coincide until a
segment is promoted. `overflow` is paint-only — the trigger is a real
widget whose bounds come from the layout pass, so never hit-test against
that rect.

---

## Testing

Anything asserting **structural** state — which segments are active,
node counts, geometry — needs two `layout()` calls. A `Signal::set` from
`place_children` dirties the binding registry, but `process_state_changes`
only turns that into dormancy transitions at the top of the *next*
layout. A real app never notices (the window manager re-lays out whenever
`needs_reconcile()`); a bare `WidgetTree` does. `Toolbar`'s suite has the
same requirement.

```rust
fn settle(tree: &mut WidgetTree, width: f32, height: f32) {
    tree.layout(SizeProposal::exact(width, height));
    tree.layout(SizeProposal::exact(width, height));
}
```

Note that `MockTextBackend` ignores the `TextStyle` it is handed (fixed
8 px per char, 16 px line height), so headless text never changes size —
a text-scale assertion there proves nothing about this widget.

Demo: `cargo run -p widget-catalog` (Inputs tab). The seven-segment
showcase sits in a slider-driven fixed-width box — the same shape as the
`collapsible_menu_bar` example's responsive bar — so the overflow
behaviour can be watched without resizing the window, with a caption
bound to `is_overflowing()` narrating the current state.
