<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# HexColorInput

![HexColorInput preview](img/hex_color_input.png)

`HexColorInput` — single-line `#RRGGBB[AA]` color editor.

## Public functions

### `HexColorInput`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(value: Signal<Color>)`](#hexcolorinput-new) |
| `Self` | [`nullable(value: Signal<Option<Color>>)`](#hexcolorinput-nullable) |
| | **Builder methods** |
| `Self` | [`alpha_enabled(enabled: bool)`](#hexcolorinput-alpha_enabled) |
| `Self` | [`short_form_enabled(enabled: bool)`](#hexcolorinput-short_form_enabled) |
| `Self` | [`require_hash(required: bool)`](#hexcolorinput-require_hash) |
| `Self` | [`uppercase(upper: bool)`](#hexcolorinput-uppercase) |
| `Self` | [`label(label: impl Into<LocalizedString>)`](#hexcolorinput-label) |
| `Self` | [`placeholder(placeholder: impl Into<LocalizedString>)`](#hexcolorinput-placeholder) |
| `Self` | [`enabled(enabled: impl Into<Prop<bool>>)`](#hexcolorinput-enabled) |
| `Self` | [`read_only(read_only: bool)`](#hexcolorinput-read_only) |
| `Self` | [`width(width: f32)`](#hexcolorinput-width) |
| `Self` | [`on_value_changed(f: impl Fn(Option<Color>, &mut teksilo_core::widget::EventContext) + 'static)`](#hexcolorinput-on_value_changed) |
| `Self` | [`on_invalid(f: impl Fn(&str, &mut teksilo_core::widget::EventContext) + 'static)`](#hexcolorinput-on_invalid) |
| `Self` | [`tooltip(text: impl Into<LocalizedString>)`](#hexcolorinput-tooltip) |
| `Self` | [`rich_tooltip(key: impl Into<String>)`](#hexcolorinput-rich_tooltip) |
| `Self` | [`rich_tooltip_content(content: crate::tooltip::TooltipContent)`](#hexcolorinput-rich_tooltip_content) |
| `Self` | [`composite_tooltip(content: impl Widget + 'static)`](#hexcolorinput-composite_tooltip) |
| | **Methods** |
| `Signal<ValidationFeedback>` | [`validation_feedback_signal()`](#hexcolorinput-validation_feedback_signal) |

## Detailed description

A specialization of `TextInput` that wires an input mask, a
hex-digit character filter, and a strict commit-time validator on top
of the standard text-editing surface. Bound to a `Signal<Color>`
(required) or `Signal<Option<Color>>` (nullable). External writes to
the bound signal reformat the field text — but only when the field
is unfocused, so a user typing "FF" in the middle of a long color
code isn't clobbered by a sibling widget tweaking the value.

### Behaviour

- **Parsing**: `#RRGGBB` (case-insensitive); `#RRGGBBAA` if
  `alpha_enabled`; `#RGB` short-form expands to `#RRGGBB` if
  `short_form_enabled`. Each accepted form may be normalized to
  uppercase on commit (configurable).
- **Char filter**: only `[0-9a-fA-F#]` admitted while typing.
- **Mask**: `\\#hhhhhh` (or `\\#hhhhhhhh` with alpha) — the
  `TextInputField` mask grammar (`h` = hex digit slot, `\\` literal
  escape).
- **Validation**: commits on Enter / Tab-out / blur.  Returns
  `ValidationOutcome::Valid` / `ValidationOutcome::Corrected` /
  `ValidationOutcome::Invalid` which the inner field maps to a
  visible inline strip via the standard
  `validation_feedback` bridge.
- **Nullable**: empty (after trim) commits `None`; non-empty
  parses normally and commits `Some(color)`.

### Example

```ignore
let color = ctx.signal(Color::from_hex("#3584E4"));
ctx.add(
    HexColorInput::new(color)
        .alpha_enabled(true)
        .label("Background"),
);
```

## Density

The picture above is the widget at `TargetDensity::Compact`, the mouse-and-keyboard ladder. Below is the same subject on the same canvas with only the ladder changed, so what moves is the density and nothing else — where the subject no longer fits, that is what the denser targets cost it at that size. See `docs/density-and-targets.md`.

**Touch**

![HexColorInput at Touch density](img/hex_color_input-touch.png)

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/hex_color_input/index.html)

<a id="hexcolorinput"></a>

## `pub struct HexColorInput`

Single-line hex color editor.

```rust
pub struct HexColorInput { /* fields */ }
```

### Methods

<a id="hexcolorinput-new"></a>

#### `pub fn new(value: Signal<Color>) -> Self`

Bind to a non-nullable color signal. Empty / invalid input
surfaces an error and keeps the previous value. Commits on
Enter or blur.

<a id="hexcolorinput-nullable"></a>

#### `pub fn nullable(value: Signal<Option<Color>>) -> Self`

Bind to a nullable color signal. Empty input commits `None`;
invalid input surfaces an error and keeps the previous value.
Commits on Enter or blur.

<a id="hexcolorinput-alpha_enabled"></a>

#### `pub fn alpha_enabled(mut self, enabled: bool) -> Self`

Enable or disable the alpha channel (`#RRGGBBAA` form). Default `false`
(`#RRGGBB` only). When enabled, the input mask and parser both switch
to the 8-digit form; existing values are immediately reformatted.

<a id="hexcolorinput-short_form_enabled"></a>

#### `pub fn short_form_enabled(mut self, enabled: bool) -> Self`

Allow CSS `#RGB` short-form input (each digit doubles: `#F0A` →
`#FF00AA`). Default `true`. When committed, the short form is expanded
and a `Corrected` feedback is shown to the user.

<a id="hexcolorinput-require_hash"></a>

#### `pub fn require_hash(mut self, required: bool) -> Self`

Require the `#` prefix during input. Default `true`. Set to `false`
to accept bare `RRGGBB` hex digits (e.g. CSS custom property editors).

<a id="hexcolorinput-uppercase"></a>

#### `pub fn uppercase(mut self, upper: bool) -> Self`

Normalize committed values to uppercase hex digits. Default `true`
(`#FF0000`). Set to `false` for lowercase (`#ff0000`). Existing
values are reformatted immediately.

<a id="hexcolorinput-label"></a>

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Attach a visible label above the field and use it as the AT name.

<a id="hexcolorinput-placeholder"></a>

#### `pub fn placeholder(mut self, placeholder: impl Into<LocalizedString>) -> Self`

Placeholder text shown when the field is empty. Defaults to the
framework's locale-specific `#RRGGBB` / `#RRGGBBAA` hint.

<a id="hexcolorinput-enabled"></a>

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Set the enabled state, statically or reactively. Forwarded to the
arena at build time.

<a id="hexcolorinput-read_only"></a>

#### `pub fn read_only(mut self, read_only: bool) -> Self`

Put the field in read-only mode; the value is displayed but cannot be
edited. Forwarded to the inner `TextInput`.

<a id="hexcolorinput-width"></a>

#### `pub fn width(mut self, width: f32) -> Self`

Set a minimum intrinsic width for the field in logical pixels.

<a id="hexcolorinput-on_value_changed"></a>

#### `pub fn on_value_changed( mut self, f: impl Fn(Option<Color>, &mut teksilo_core::widget::EventContext) + 'static, ) -> Self`

Called after a successful commit with the new color value (`None` on a
nullable binding when the field is cleared). Not called when the previous
and new values are identical.

<a id="hexcolorinput-on_invalid"></a>

#### `pub fn on_invalid( mut self, f: impl Fn(&str, &mut teksilo_core::widget::EventContext) + 'static, ) -> Self`

Called after a commit attempt when the input is invalid, with the raw
typed string. The field is left as-is so the user can correct the value.

<a id="hexcolorinput-tooltip"></a>

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain single-line tooltip shown after the standard hover delay.

Mutually exclusive with `Self::rich_tooltip`, `Self::rich_tooltip_content`,
and `Self::composite_tooltip` — each setter clears the other three so
the last call wins.

<a id="hexcolorinput-rich_tooltip"></a>

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip driven by a registry key.

Mutually exclusive with `Self::tooltip`, `Self::rich_tooltip_content`,
and `Self::composite_tooltip` — each setter clears the other three so
the last call wins.

<a id="hexcolorinput-rich_tooltip_content"></a>

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip from inline `crate::tooltip::TooltipContent`.

Mutually exclusive with `Self::tooltip`, `Self::rich_tooltip`,
and `Self::composite_tooltip` — each setter clears the other three so
the last call wins.

<a id="hexcolorinput-composite_tooltip"></a>

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip whose body is an arbitrary widget tree.

Mutually exclusive with `Self::tooltip`, `Self::rich_tooltip`,
and `Self::rich_tooltip_content` — each setter clears the other three so
the last call wins.

<a id="hexcolorinput-validation_feedback_signal"></a>

#### `pub fn validation_feedback_signal(&self) -> Signal<ValidationFeedback>`

Reactive handle on the inner TextInput's published validation
feedback. Mirrors the inner field's signal after `build()`.
