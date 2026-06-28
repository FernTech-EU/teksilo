<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SegmentedControl

SegmentedControl — mutually exclusive segments in a horizontal row.

Each segment is a real composed widget — a centered icon + label with
a reactive tint — built from a [`Segment`] descriptor. The control
binds a `Signal<usize>` index: reading or writing the signal selects
the corresponding segment without rebuilding the tree. Per-segment
disabling, optional leading icons, and optional hover tooltips are all
first-class; the chrome (rounded frame, hover tint, selected-segment
surface) is delegated to the active `SegmentedControlStyle`.

## When to use

- Use a `SegmentedControl` when there are 2–5 mutually exclusive modes
  that fit in a compact horizontal strip (e.g. view mode, time period).
- Prefer a `ComboBox` when there are more than five options or labels
  are long.
- Prefer `RadioButton` when the options need more vertical space or
  detailed descriptions.

## Accessibility

`Role::RadioGroup` on the control, `Role::RadioButton` per segment.
Arrow keys cycle selection, skipping disabled segments; the entire
control is a single tab stop. `Increment`/`Decrement` AT actions
mirror arrow-key behavior for switch-access users.

```ignore
SegmentedControl::new(selected)
    .segment(Segment::new(tr!(list_view())).icon(|| IconWidget::list(14.0)))
    .segment(Segment::new(tr!(grid_view())).icon(|| IconWidget::grid(14.0)).tooltip(tr!(grid_hint())))
    .segment(Segment::new(tr!(columns())).disabled(true))
```

## Builder methods at a glance

`segment`, `segments`, `enabled`, `style`, `text_style`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/segmented_control/index.html)

## `pub struct Segment`

One segment descriptor: a localized label with an optional leading
icon, hover tooltip, and disabled flag.

```rust
pub struct Segment { /* fields */ }
```

### Methods

#### `pub fn new(label: impl Into<LocalizedString>) -> Self`

A text segment. The label may come from `tr!(...)` (translated —
follows a live locale switch) or `lit!(...)` (untranslated).

#### `pub fn icon(mut self, factory: impl Fn() -> IconWidget + 'static) -> Self`

Add a leading icon. The factory is invoked at build time (and on
rebuild); the icon's tint is bound reactively to the segment's
selected / focus / enabled state so it matches the label.

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Hover tooltip — most useful for icon-only segments.

#### `pub fn disabled(mut self, disabled: bool) -> Self`

Disable this segment: not selectable via click or keyboard,
dimmed, and announced disabled to assistive tech.

## `pub struct SegmentedControl`

A segmented control that binds a `Signal<usize>` index to a row of
mutually exclusive segments. Build the segment list with
`segment` or `segments`.

```rust
pub struct SegmentedControl { /* fields */ }
```

### Methods

#### `pub fn new(selected: Signal<usize>) -> Self`

Create an empty segmented control bound to `selected`. Add segments
with `segment` or `segments`.

#### `pub fn segment(mut self, segment: impl Into<Segment>) -> Self`

Append one segment. Accepts a [`Segment`] or, via
`From<LocalizedString>`, a bare `tr!(...)` / `lit!(...)` label.

#### `pub fn segments(mut self, segments: impl IntoIterator<Item = impl Into<Segment>>) -> Self`

Append several segments. Label-only:
`.segments([tr!(day()), tr!(week())])`; rich:
`.segments([Segment::new(...).icon(...), ...])`.

#### `pub fn enabled(mut self, enabled: bool) -> Self`

Set the initial enabled state. Forwarded to the arena at build
time. For reactive enable/disable use
`ctx.enabled_when(segmented_control_id, signal)`.

#### `pub fn style(mut self, style: impl bastyde_core::styles::SegmentedControlStyle) -> Self`

Per-call override for the segmented-control chrome.

#### `pub fn text_style(mut self, style: impl Into<bastyde_core::color_prop::TextStyleProp>) -> Self`

Override every segment's label text style (font, size, weight).
Accepts a `TextStyleRole`, a `TextStyle`, or a `Signal` of either.
Default (unset) is `TextStyleRole::Small`. Text color stays
state-driven and is intentionally not overridable here.
