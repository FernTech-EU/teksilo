<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# FormLayout

![FormLayout preview](img/form_layout.png)

FormLayout — a two-column settings or preferences form layout.

Children are added as label/field pairs via `FormLayout::line` (inline
widgets) or `FormLayout::line` (pre-registered IDs). Full-width rows
that span both columns — section headers, `Divider`s, or banners — are
added via `FormLayout::full_width` / `FormLayout::full_width`. The
label column auto-sizes to the widest label across all pairs so all field
inputs are left-aligned. RTL layouts are handled automatically: the label
column migrates to the trailing side and the field column moves to the
leading side. Dormant rows are excluded from both measurement and
placement.

When an accessible name is provided via `FormLayout::label`, the widget
emits `Role::Form` so screen-reader users can navigate directly to the
form. Without a name it demotes to a presentational `GenericContainer`.

```rust
# use teksilo_widgets::primitives::{FormLayout, TextWidget, RectWidget};
# use teksilo_i18n::lit;
let _form = FormLayout::new()
    .label_gap(8.0)
    .row_spacing(6.0)
    .line(TextWidget::new(lit!("Name:")),  RectWidget::new())
    .line(TextWidget::new(lit!("Email:")), RectWidget::new());
```

## Density

The picture above is the widget at `TargetDensity::Compact`, the mouse-and-keyboard ladder. Below is the same subject on the same canvas with only the ladder changed, so what moves is the density and nothing else — where the subject no longer fits, that is what the denser targets cost it at that size. See `docs/density-and-targets.md`.

**Touch**

![FormLayout at Touch density](img/form_layout-touch.png)

## Builder methods at a glance

`label_gap`, `row_spacing`, `label`, `line`, `lines`, `line_ids`, `full_width`, `full_width_rows`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/form_layout/index.html)

## `pub struct FormLayout`

A two-column form layout with auto-sized label column.

Children are added as label/field pairs via `line()` or as
full-width rows via `full_width()`. The label column
auto-sizes to the widest label; the field column takes the remaining
space.

```text
┌─ label col ─┐ gap ┌── field col ──────────────┐
│ Name:       │     │ [___________________]      │
│ Email:      │     │ [___________________]      │
├─────────────┴─────┴────────────────────────────┤
│ ── Advanced ──────────────────────────────────  │  ← full_width
├─ label col ─┐ gap ┌── field col ──────────────┐
│ Port:       │     │ [____]                     │
└─────────────┘     └────────────────────────────┘
```

```rust
pub struct FormLayout { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create an empty `FormLayout` with zero label gap and zero row spacing.

#### `pub fn label_gap(mut self, gap: f32) -> Self`

Horizontal gap between the label column and the field column.

#### `pub fn row_spacing(mut self, spacing: f32) -> Self`

Vertical gap between rows.

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Set an accessible name for this form. When set, the widget emits
the `Role::Form` landmark so assistive-technology users can
navigate directly to it and distinguish it from other forms on
the page. When unset, the widget demotes to a presentational
`GenericContainer` — an unnamed landmark is worse than no
landmark for AT users.

#### `pub fn line( mut self, label: impl teksilo_core::IntoTeksiChild, field: impl teksilo_core::IntoTeksiChild, ) -> Self`

Add a label/field pair row.

#### `pub fn lines<L, F>(self, rows: impl IntoIterator<Item = (L, F)>) -> Self where L: teksilo_core::IntoTeksiChild, F: teksilo_core::IntoTeksiChild,`

Add several label/field pair rows from an iterator of `(label, field)`
pairs, in order.

The loop form of `line`, and the usual one once the form is
generated from a settings schema rather than written row by row.

#### `pub fn line_ids(self, rows: impl IntoIterator<Item = (WidgetId, WidgetId)>) -> Self`

Add several label/field pair rows from an iterator of
`(label_id, field_id)` pairs, in order.

`lines` accepts ids in both columns too, so this is the
spelling that states the id types outright rather than a capability the
other method lacks. Reach for it when a loop has already registered both
columns and naming the type reads better than inferring it.

#### `pub fn full_width(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self`

Add a full-width row spanning both columns.

#### `pub fn full_width_rows( self, iter: impl IntoIterator<Item = impl teksilo_core::IntoTeksiChild>, ) -> Self`

Add several full-width rows from an iterator, in order.

The loop form of `full_width`, for a run of banners
or section headers that comes from data.
