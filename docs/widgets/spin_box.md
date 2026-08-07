<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SpinBox

`SpinBox` — numeric input with increment/decrement buttons.

A generic composite over `SpinValue`
(integer and floating-point primitives), pairing the
`TextInputField` editing
primitive with a stacked pair of up/down step buttons. Semantics
are a synthesis of Qt's `QSpinBox` / `QDoubleSpinBox`, WinUI 3's
`NumberBox`, GTK's `GtkSpinButton`, and the W3C ARIA
`spinbutton` role.

# Behaviour

- **Value binding**: a `Signal<T>` is the single source of truth.
  Typing and stepping update it; external writes re-format the
  editable text.
- **Commit model**: the user can type freely (subject to the
  per-character input filter). The value is *committed* on
  `Enter` or on focus loss —
  at commit time the text is parsed, clamped into `[min, max]`
  (or wrapped, per `WrapMode`), and reformatted. Invalid input
  reverts to the last known good value.
- **Keyboard**:
  - `Up` / `Down` → ±`single_step`
  - `PageUp` / `PageDown` → ±`page_step`
    (default: `10 × single_step`)
  - `Enter` → commit (stays focused)
  - `Home` / `End` stay bound to the text cursor (Qt-compatible).
- **Mouse wheel**: adjusts by `single_step`; gated by
  `wheel_mode` (default: only when
  focused, to avoid accidental scroll changes).
- **Buttons**: up/down buttons stack to the right of the field
  by default; can be hidden with
  `button_layout`.
- **Special value text**: when the current value equals `min`
  and `special_value_text` is
  set, the field shows that string instead of the formatted
  number — Qt's "Auto" / "None" / "Unlimited" affordance.
- **Adaptive step**: with
  `StepType::Adaptive`, the effective step
  tracks the decimal magnitude of the current value (Qt's
  `AdaptiveDecimalStepType`). Useful for values that span many
  orders of magnitude in the same control.
- **Custom formatter / parser**: full override via
  `text_from_value` and
  `value_from_text`; together they
  let you implement currency, percentages with stored fraction,
  hex, duration, anything.

# Accessibility

The composite exposes itself as
`Role::SpinButton`
with numeric value, min, max, step, and jump properties set on
the AccessKit node; the AT receives
`Increment`,
`Decrement`,
`SetValue`, and
`Focus` actions. The
step buttons are structurally part of the SpinBox and publish
no separate a11y nodes.

# Example

```ignore
use teksilo::widgets::{SpinBox, WrapMode};

let font_size = ctx.signal(12_i32);
ctx.add(
    SpinBox::new(font_size, 4, 72)
        .single_step(1)
        .page_step(10)
        .suffix(" pt"),
);

let gain_db = ctx.signal(0.0_f32);
ctx.add(
    SpinBox::new(gain_db, -60.0, 12.0)
        .single_step(0.5)
        .decimals(1)
        .suffix(" dB")
        .wrap_mode(WrapMode::Clamp),
);
```

## Builder methods at a glance

`style`, `single_step`, `page_step`, `decimals`, `suffix`, `special_value_text`, `wrap_mode`, `step_type`, `button_layout`, `show_buttons`, `wheel_mode`, `width`, `width_chars`, `fill_width`, `label`, `placeholder`, `enabled`, `read_only`, `text_from_value`, `value_from_text`, `on_value_changed`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`, `value`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/spin_box/index.html)

## `pub enum WrapMode`

Out-of-range behavior when stepping past `min` or `max`.

Set via `SpinBox::wrap_mode`.

```rust
pub enum WrapMode { /* variants */ }
```

### Variants

- **`Clamp`** — Clamp to `min` / `max` (default).
- **`Wrap`** — Wrap around: past `max` jumps to `min`, past `min` jumps to `max`. Matches Qt's `QAbstractSpinBox::wrapping`.

## `pub enum StepType`

Step-size policy for each key/button press.

Set via `SpinBox::step_type`.

```rust
pub enum StepType { /* variants */ }
```

### Variants

- **`Fixed`** — Always step by `single_step` (default).
- **`Adaptive`** — Step by the decimal power-of-ten immediately below the current value's magnitude — e.g. values 1–9 step by 1, 10–99 by 10, 100–999 by 100. Matches Qt's `AdaptiveDecimalStepType`. Integer types honor the same rule using the magnitude of the absolute value.

## `pub enum WheelMode`

When the mouse wheel is allowed to adjust the value.

Set via `SpinBox::wheel_mode`.

```rust
pub enum WheelMode { /* variants */ }
```

### Variants

- **`Focused`** — Wheel adjusts only when the field is focused. Default — prevents accidental changes when the user is scrolling a larger surrounding view.
- **`Hover`** — Wheel adjusts whenever the pointer is over the widget.
- **`Disabled`** — Wheel never adjusts the value; events bubble to the surrounding scroll container.

## `pub enum WidthPolicy`

How the SpinBox decides its horizontal size envelope.

Chosen via the `width`,
`width_chars`, and
`fill_width` builder methods — the enum
itself is the storage, not a separate public configuration
API.

```rust
pub enum WidthPolicy { /* variants */ }
```

### Variants

- **`Pixels`** — Cap the widget at a fixed logical-pixel width. Default is `DEFAULT_PREFERRED_WIDTH` (120 dp), matching Qt's `QSpinBox` sizeHint.
- **`Chars`** — Size the widget to fit this many reference digits (`'0'`) plus the configured suffix, padding, and step buttons. Measurement uses the theme font at build time.
- **`Fill`** — Let the widget expand horizontally to fill whatever space the parent offers. Equivalent to an infinite pixel cap.

## `pub struct SpinBox`

Numeric input with step buttons. Generic over
`SpinValue` — pre-implemented for `i32`, `i64`, `u32`, `u64`,
`usize`, `f32`, and `f64`.

```rust
pub struct SpinBox<T: SpinValue> { /* fields */ }
```

### Methods

#### `pub fn new(value: Signal<T>, min: T, max: T) -> Self`

Construct a new SpinBox bound to `value` with the given
inclusive range. `min` must be ≤ `max`.

#### `pub fn style(mut self, style: impl teksilo_core::styles::SpinBoxStyle) -> Self`

Per-call style override. Higher precedence than the theme-wide
`style_slots.spin_box` slot.

#### `pub fn single_step(mut self, step: T) -> Self`

Set the step size for `Up` / `Down` / single wheel tick /
button tap.

#### `pub fn page_step(mut self, step: T) -> Self`

Set the step size for `PageUp` / `PageDown`. When unset,
defaults to `10 × single_step` at build time.

#### `pub fn decimals(mut self, decimals: u8) -> Self`

Number of decimal places shown for floating-point types.
Ignored for integer types.

#### `pub fn suffix(mut self, text: impl Into<String>) -> Self`

Qt-style non-editable trailing unit (e.g. `" %"`, `" px"`,
`" dB"`). Rendered flush-right inside the field's border;
the caret cannot enter it.

#### `pub fn special_value_text(mut self, text: impl Into<LocalizedString>) -> Self`

Text shown in place of the formatted value when the current
value equals `min`. Use for "Auto", "None", "Off",
"Unlimited" affordances where the minimum has special
semantics. When the field is focused the real number is
shown instead so the user can type.

#### `pub fn wrap_mode(mut self, mode: WrapMode) -> Self`

Set the out-of-range behavior when stepping past `min` or `max`
(default: `Clamp`).

#### `pub fn step_type(mut self, step_type: StepType) -> Self`

Set the step-size policy (default: `Fixed`). Use
`StepType::Adaptive` for values that span many orders of magnitude.

#### `pub fn button_layout(mut self, layout: ButtonLayout) -> Self`

Override the step-button layout (default: `Stacked` — stacked
up/down buttons to the right of the field).

#### `pub fn show_buttons(mut self, show: bool) -> Self`

Convenience wrapper over `button_layout`:
`true` → `ButtonLayout::Stacked`, `false` → `ButtonLayout::Hidden`.
Matches the Int UI guideline that SpinBoxes in dense forms
often hide the step buttons to reduce visual noise and let
keyboard / wheel carry the affordance — pass
`.show_buttons(false)` on those call sites.

#### `pub fn wheel_mode(mut self, mode: WheelMode) -> Self`

Set when the mouse wheel adjusts the value (default: `Focused` —
only when the inner field holds focus).

#### `pub fn width(mut self, width: f32) -> Self`

Cap the widget's horizontal size at a fixed logical-pixel
width. If the parent offers less, the SpinBox shrinks (down
to the internal 72 dp / 48 dp floor that keeps the buttons
and field from overlapping). Default: 120 dp, matching Qt
`QSpinBox` sizeHint and Int UI form density.

```rust
# use teksilo_widgets::SpinBox;
# use teksilo_core::signal::Signal;
# let v = Signal::new(0_i32);
let _w = SpinBox::new(v.clone(), 0, 9999).width(80.0);        // narrow
let _w = SpinBox::new(v.clone(), 0, 9999).width(200.0);       // wider
let _w = SpinBox::new(v.clone(), 0, 9999).fill_width();       // stretch to parent
let _w = SpinBox::new(v.clone(), 0, 9999).width_chars(5);     // "fits 5 digits"
```

#### `pub fn width_chars(mut self, chars: u32) -> Self`

Size the widget to fit exactly `chars` reference digits plus
the configured suffix, padding, and step buttons. The
measurement uses the actual theme font at build time (same
`SharedTypesetter` the field draws with), so values stay
right under runtime theme switches and HiDPI scale changes.

```rust
# use teksilo_widgets::SpinBox;
# use teksilo_core::signal::Signal;
# let port = Signal::new(8080_i32);
# let pct = Signal::new(0_i32);
let _w = SpinBox::new(port, 0, 65_535).width_chars(5);           // 5 digits
let _w = SpinBox::new(pct, 0, 100).suffix(" %").width_chars(3);  // 3 + " %"
```

#### `pub fn fill_width(mut self) -> Self`

Let the widget expand to fill the horizontal space offered
by its parent, instead of capping at `width`.
Use inside toolbars, inspector panels, or an
`Expand::horizontal` column that should stretch with the
surrounding layout.

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Set the accessible name announced by screen readers as the
control's label. ARIA requires spin buttons to have a label;
when none is set here the caller is responsible for labelling
via a wrapping element or `access_label`.

#### `pub fn placeholder(mut self, text: impl Into<LocalizedString>) -> Self`

Set the placeholder text shown in the field when it is empty.

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Set the enabled state, statically or reactively. Forwarded to
the arena at build time via
`ctx.enabled_when(spinbox_id, self.enabled.clone())`.

#### `pub fn read_only(mut self, read_only: bool) -> Self`

Prevent the user from typing in the field while still allowing
keyboard and button stepping.

#### `pub fn text_from_value(mut self, f: impl Fn(T) -> LocalizedString + 'static) -> Self`

Override the value → display-string conversion. Receives the
raw value; returns whatever string should appear in the
field. Suffix and `special_value_text` still apply on top of
the returned string.

#### `pub fn value_from_text(mut self, f: impl Fn(&str) -> Option<T> + 'static) -> Self`

Override the parse step. Receives the field's raw text
(without the suffix, which is never part of the editable
content); returns `Some(value)` to accept or `None` to
reject. Invalid input reverts to the last good value on
commit.

#### `pub fn on_value_changed(mut self, f: impl Fn(T, &mut EventContext) + 'static) -> Self`

Closure fired each time the value is committed (keyboard
step, button tap, wheel tick, Enter, blur). Bound observers
on the value signal also see every change; use this hook
when the caller needs an `EventContext` (e.g. to fire an
intent).

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain single-line tooltip shown after a hover delay.

Mutually exclusive with `rich_tooltip`,
`rich_tooltip_content`, and
`composite_tooltip` — each setter
clears the other two so the last call wins.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip looked up by registry key.

The key must match a `TooltipContent`
registered in the application's tooltip registry. Mutually
exclusive with `tooltip`,
`rich_tooltip_content`, and
`composite_tooltip`.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip with inline content (no registry key
required). Mutually exclusive with `tooltip`,
`rich_tooltip`, and
`composite_tooltip`.

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip whose body is an arbitrary widget
tree. Mutually exclusive with `tooltip`,
`rich_tooltip`, and
`rich_tooltip_content`.

#### `pub fn value(&self) -> Signal<T>`

The bound numeric value signal.
