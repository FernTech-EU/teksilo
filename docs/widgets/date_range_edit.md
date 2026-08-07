<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# DateRangeEdit

`DateRangeEdit` — single unified control for picking a `DateRange`.

Visually one widget: a single bordered frame containing two
`TextInputField` halves separated by a painted arrow glyph, with
a trailing built-in calendar button that opens a shared
`Calendar::range` popover. Backed by `Signal<Option<DateRange>>`.

```text
┌──────────────────────────────────────┐
│ 05/12/2026   →   05/19/2026   │ 📅  │
└──────────────────────────────────────┘
```

# Why one frame?

Two adjacent `DateEdit`s (one frame each) visually read as two
separate fields that happen to be next to each other. A single
frame says "this is one range". Same affordance the user is used
to from booking sites and analytics dashboards.

# Behaviour

- **Two text halves** — each masked from the resolved date pattern,
  each with its own validator + segment-stepping (Up/Down on the
  focused segment matches `DateEdit`).
- **Painted arrow separator** — a thin chevron-right glyph, no text.
  Visual only; AT users see the wrapper's `Role::DateInput`.
- **One trailing calendar button** — Int UI `IconButton::embedded()` with
  the calendar glyph. Opens a single popover hosting
  `Calendar::range` bound to the outer signal. The two-anchor
  click model (start-then-end) commits the range and closes the
  popover. No per-half calendar buttons — there's only one
  calendar, anchored to the wrapper.
- **One frame** — focus-aware border (`BorderRole::Focused` while
  any half holds focus, otherwise `Default`), validation-aware
  border (`Error` for `Invalid`, `Focused` for `Corrected`).
- **One validation strip** below the frame — composed feedback
  from both halves (worse of the two wins).

# Accessibility

- Container — `Role::DateInput` with `set_value` formatted as
  `YYYY-MM-DD/YYYY-MM-DD` (ISO range).
- Each `TextInputField` keeps its own `Role::TextInput` AT node;
  the wrapper's `Role::DateInput` provides the range semantics.

```ignore
// Requires ctx.signal() — shown as ignore per convention.
use teksilo_widgets::date_range_edit::DateRangeEdit;
use jiff::civil::Weekday;

let range = ctx.signal(None);
let _w = DateRangeEdit::new(range.clone())
    .first_day_of_week(Weekday::Monday)
    .on_value_changed(|r, _ctx| println!("{r:?}"));
```

## Builder methods at a glance

`style`, `min_date`, `max_date`, `format_pattern`, `placeholder_start`, `placeholder_end`, `first_day_of_week`, `enabled`, `read_only`, `label`, `validation_behavior`, `end_width_policy`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`, `validation_feedback_signal`, `on_value_changed`, `value`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/date_range_edit/index.html)

## `pub struct DateRangeEdit`

Two-handle date picker over `Signal<Option<DateRange>>`. See the
`module docs` for the visual layout and behaviour.

```rust
pub struct DateRangeEdit { /* fields */ }
```

### Methods

#### `pub fn new(value: Signal<Option<DateRange>>) -> Self`

Create a date-range picker bound to `value`.

#### `pub fn style(mut self, style: impl teksilo_core::styles::DateEditStyle) -> Self`

Per-call DateEditStyle override (shared with DateEdit family).

#### `pub fn min_date(mut self, d: Date) -> Self`

Restrict the selectable start and end dates to those on or after `d`.

#### `pub fn max_date(mut self, d: Date) -> Self`

Restrict the selectable start and end dates to those on or before `d`.

#### `pub fn format_pattern(mut self, p: impl Into<String>) -> Self`

Override the strftime-subset format pattern for both halves
(e.g. `"%d/%m/%Y"`). Defaults to the locale-derived pattern.

#### `pub fn placeholder_start(mut self, text: impl Into<LocalizedString>) -> Self`

Placeholder shown in the start half when no date is set.

#### `pub fn placeholder_end(mut self, text: impl Into<LocalizedString>) -> Self`

Placeholder shown in the end half when no date is set.

#### `pub fn first_day_of_week(mut self, w: Weekday) -> Self`

Override which weekday appears in the first column of the calendar popup.

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Set the enabled state, statically or reactively. Forwarded to the
arena at build time.

#### `pub fn read_only(mut self, read_only: bool) -> Self`

Make both halves read-only; the calendar button is also disabled.

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Accessible label for the wrapper `Role::DateInput` node. When not set,
falls back to the localized `date-range-edit-name` message.

#### `pub fn validation_behavior(mut self, behavior: ValidationBehavior) -> Self`

How both halves handle invalid or out-of-range text on blur / Enter.
Defaults to `ValidationBehavior::AutoCorrect`.

#### `pub fn end_width_policy(mut self, policy: crate::date_edit::WidthPolicy) -> Self`

How the trailing (end) half claims horizontal space. The
leading (start) half always sizes to its natural mask width;
the end half follows this policy. Default
`WidthPolicy::Default` (natural width); pass
`WidthPolicy::Fill` to make the end half absorb extra
space the parent offers.

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Show a plain single-line tooltip on hover. Mutually exclusive with the
rich / composite tooltip slots — this setter clears the other two so the
last call wins.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Show a rich tooltip sourced from the registry by `key`. Mutually
exclusive with the plain / composite tooltip slots — this setter clears
the other two so the last call wins.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Show a rich tooltip from an inline `TooltipContent` value. Mutually
exclusive with the plain / registry-key tooltip slots — this setter
clears the other two so the last call wins.

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Show a composite tooltip whose body is an arbitrary widget tree. Mutually
exclusive with the plain / rich tooltip slots — this setter clears the
other two so the last call wins.

#### `pub fn validation_feedback_signal(&self) -> Signal<ValidationFeedback>`

Reactive handle on the composed validation feedback (worse of the two
halves — `Invalid > Corrected > Valid > Pristine`).

#### `pub fn on_value_changed( mut self, f: impl Fn(Option<DateRange>, &mut EventContext) + 'static, ) -> Self`

Callback invoked whenever the range changes (including when one half
clears its value). Receives the new `Option<DateRange>` and an
`EventContext` for dispatching intents or side effects.

#### `pub fn value(&self) -> Signal<Option<DateRange>>`

Clone the underlying `Signal<Option<DateRange>>` for external binding.
