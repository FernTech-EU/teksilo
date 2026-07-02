<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TextInputField

`TextInputField` — editable single-line text surface primitive.

This is the raw editing primitive that powers the styled
`TextInput` composite and any
other widget that needs inline editable text — `SpinBox` being
the primary second consumer.

Unlike `TextInput`, `TextInputField` paints no frame, no
placeholder overlay, no validation border, and hosts no trailing
slots: it is the focusable text area only. Compose it yourself
with `RectWidget`, `Padding`, icons, clear buttons, etc. to
build a styled control. Focus indication is the composite's
responsibility — the Int UI convention is to thicken the
enclosing frame's border to `focus_ring_width` and recolor it
to the accent focus-ring color.

Features:
- Bound `Signal<String>` for two-way text binding.
- Full keyboard editing (arrow keys, Home/End, Backspace/Delete,
  Ctrl+X/C/V, Ctrl+A, Ctrl+Z/Y), IME commit, and pointer caret
  positioning and drag-select.
- Optional per-character input filter
  (`TextInputField::char_filter`), max-length cap
  (`TextInputField::max_length`), and read-only mode
  (`TextInputField::read_only`).
- Commit hooks: Enter fires
  `on_submit_fn` and focus loss
  fires `on_blur_fn`.
- Non-editable trailing
  `suffix`, rendered flush-right inside
  the field's bounds (Qt's `QSpinBox::suffix`). Caret cannot
  enter it; clicks past the text end clamp to the last
  character.
- Right-click context menu (Cut / Copy / Paste / Select All).
- AccessKit `Role::TextInput` with value, selection, and
  character/word boundary metadata.

# Example

```ignore
let text = ctx.signal(String::new());
ctx.add(
    TextInputField::new(text.clone())
        .placeholder("Enter a name…")
        .char_filter(|c| !c.is_ascii_digit())
        .on_submit_fn(|ctx| ctx.send_intent(MyIntent::Save)),
);
```


## Builder methods at a glance

`placeholder`, `enabled`, `read_only`, `max_length`, `on_submit_fn`, `on_blur_fn`, `char_filter`, `suffix`, `text_height`, `interaction_signal`, `input_mask`, `mask_placeholder`, `validator`, `secure`, `echo_char`, `revealed`, `at_reveal_policy`, `allow_copy`, `validation_feedback_signal`, `text`, `interaction`, `caret_position`, `caret_setter`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/primitives/text_input_field/index.html)

## `pub enum EchoMode`

How a secure (`TextInputField::secure`) field echoes typed
characters. Mirrors Qt's `QLineEdit::EchoMode`.

```rust
pub enum EchoMode { /* variants */ }
```

### Variants

- **`Masked`** — Replace every character with the echo glyph (default `'•'`). The plaintext stays in the bound `Signal<String>` but never reaches the text engine while masked.
- **`NoEcho`** — Show nothing at all — not even the length. The caret stays at the start. Qt's `NoEcho`.
- **`RevealWhileTyping`** — Show plaintext while the field is focused (being edited) and re-mask on blur. Qt's `PasswordEchoOnEdit`.

## `pub enum AtRevealPolicy`

How a *revealed* secure field reports to assistive technology.

```rust
pub enum AtRevealPolicy { /* variants */ }
```

### Variants

- **`SwapRole`** — When revealed, expose the field as a normal `Role::TextInput` carrying the plaintext value — matching what is visibly on screen and the web `type=password ↔ type=text` swap. When masked, it reverts to `Role::PasswordInput`. (Default.)
- **`AlwaysProtected`** — Always report `Role::PasswordInput` and never expose plaintext to assistive tech, even while visually revealed. Higher confidentiality at the cost of consistency with the screen.

## `pub struct TextInputField`

Editable single-line text surface primitive.

See the `module docs` for the full feature list and a
compositional example.

```rust
pub struct TextInputField { /* fields */ }
```

### Methods

#### `pub fn new(text: Signal<String>) -> Self`

Construct a new field bound to `text`.

#### `pub fn placeholder(mut self, text: impl Into<String>) -> Self`

Declarative placeholder string. The field itself paints
nothing for placeholder — that visual is the composite
parent's responsibility (`TextInput` overlays a
`TextWidget`). The string is still stored here and published
via AccessKit's `placeholder` property so screen readers
announce it.

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Set the enabled state, statically or reactively. Disabled blocks
input and AccessKit interaction. Forwarded to the arena at build
time.

#### `pub fn read_only(mut self, read_only: bool) -> Self`

Mark the field read-only. Caret and selection still work;
inserts, deletes, paste, undo/redo, and cut are all no-ops.

#### `pub fn max_length(mut self, max_length: usize) -> Self`

Hard cap on document length in `char`s (grapheme count is
approximated — each `char` counts as one unit, matching
`String::chars().count()`).

#### `pub fn on_submit_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Closure fired on `Enter`. Unlike `on_blur_fn`, this does
not move focus — the field stays focused and the caret
stays where it was.

#### `pub fn on_blur_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Closure fired once per focus-loss, after selection/scroll
have been reset. SpinBox-style callers parse and reformat
here; validators revalidate here.

#### `pub fn char_filter(mut self, f: impl Fn(char) -> bool + 'static) -> Self`

Per-character input-filter predicate. Applied uniformly to
keyboard input, IME commits, and clipboard paste so a filtered
field cannot receive disallowed characters through any path.
Composes with `max_length` and the built-in control/newline
strip (filter runs after the strip). Whole-string validity
(e.g. "at most one decimal point") is a commit-time concern
for `on_blur` / `on_submit`.

#### `pub fn suffix(mut self, text: impl Into<Prop<String>>) -> Self`

Static non-editable trailing string rendered flush-right
inside the field's bounds (Qt's `QSpinBox::suffix`). The
caret cannot enter the suffix; clicks past the text end
position the caret at the last editable character.

Accepts a static `String`/`&str` or a reactive `Signal<String>` /
`Prop<String>`; when bound, the field re-measures the suffix glyphs
and relayouts the editable text viewport each time the signal fires.
Typical use: a `SpinBox` with `special_value_text` binds an empty
string to the suffix whenever the value equals `min`, and the
configured unit string otherwise.

#### `pub fn text_height(mut self, height: f32) -> Self`

Override the intrinsic text-area height. The field is a
pure leaf with no theme lookup of its own; by default it
reports `DEFAULT_TEXT_HEIGHT`. A wrapping composite like
`TextInput` passes its theme's `text_field.height` minus
border + padding here so the visuals line up with the
rest of the form.

#### `pub fn interaction_signal(mut self, signal: Signal<InteractionState>) -> Self`

Bind an externally-owned `InteractionState` signal. The
field writes `Focused` on focus gain and `Idle` on loss;
other states (`Hovered`, `Pressed`, `Disabled`) are the
composite's responsibility. When unset, the field owns a
private signal that observers can still read via
`interaction`, but composites
that drive a focus ring or border color usually want to
push their own.

#### `pub fn input_mask(mut self, mask: impl AsRef<str>) -> Self`

Set an input mask (Qt grammar). Constrains accepted characters
per position, auto-derives the empty-state template
(`__/__/____` for `99/99/9999`), and routes typed chars
through the mask's class filter.

Composes with `char_filter`: a char must
pass *both* the mask's per-position class AND the user's
`char_filter` to be accepted.

On parse error (only the trailing-backslash case in practice),
the mask is silently dropped — the field falls back to its
no-mask behaviour rather than panicking.

#### `pub fn mask_placeholder(mut self, c: char) -> Self`

Override the visible character used for unfilled editable mask
positions. Default: the theme's
`text_field.mask_placeholder_char` (typically `_`).

#### `pub fn validator(mut self, f: impl Fn(&str) -> ValidationOutcome + 'static) -> Self`

Install a validator. The closure runs on every commit (Enter,
Tab-out, focus loss) and returns a `ValidationOutcome` that
drives `validation_feedback_signal`.

**Does not run per-keystroke** — that's `char_filter`'s
job. Mixing per-keystroke text rewriting with validation
produces caret-jump bugs and is explicitly out of scope.

#### `pub fn secure(mut self, echo_mode: EchoMode) -> Self`

Turn this into a secure (password) field with the given
`EchoMode`. Masking happens at the text-engine layer (one echo
glyph per source `char`), so the plaintext never reaches the
shaper or glyph atlas while masked, and caret / selection /
hit-test stay correct. Also defaults `allow_copy` to `false` and
opts the focused node out of OS IME composition. Pair with
`revealed` for a reveal toggle.

#### `pub fn echo_char(mut self, c: char) -> Self`

Override the masking glyph (default `'•'`, U+2022). Any
uniform-width character works; the engine emits exactly one per
source `char`.

#### `pub fn revealed(mut self, revealed: Signal<bool>) -> Self`

Bind the reveal toggle. When the signal is `true` the field
shows plaintext regardless of `EchoMode`; when `false` it
masks. Shared with the eye `IconButton::visibility_toggle`.


#### `pub fn at_reveal_policy(mut self, policy: AtRevealPolicy) -> Self`

How a *revealed* secure field reports to assistive tech. Default
`AtRevealPolicy::SwapRole`.

#### `pub fn allow_copy(mut self, allow: bool) -> Self`

Permit (or forbid) copy / cut. Plain fields default `true`;
`secure` flips the default to `false`. Even when
`false`, copy is allowed while the field is revealed.

#### `pub fn validation_feedback_signal(&self) -> Signal<ValidationFeedback>`

Reactive handle on the published `ValidationFeedback` state.
Composites bind to this to render the inline feedback strip
below the field. Always present; reads `Pristine` until the
first commit (or forever if no validator is installed).

#### `pub fn text(&self) -> Signal<String>`

The `Signal<String>` this field is bound to.

#### `pub fn interaction(&self) -> Signal<InteractionState>`

The interaction signal this field writes on focus changes.
Call before inserting the field into the tree.

#### `pub fn caret_position(&self) -> Signal<usize>`

Reactive caret position in the field's text (in `usize` char
offsets). Updates after every keyboard or pointer action that
moves the cursor. Used by composing widgets that need to know
where the caret is — e.g. `DateEdit` reads this to figure out
which date segment Up/Down should step.

#### `pub fn caret_setter(&self) -> std::rc::Rc<dyn Fn(usize)>`

Returns a callable that programmatically sets the caret
position (in char offsets) on the field. Capture this on the
builder BEFORE `ctx.add(...)` consumes the field; call it
after a programmatic text rewrite to restore the caret to the
right column instead of leaving it at the document end (the
default behaviour of `cursor.insert_text`).

The returned closure becomes a no-op until `build()` runs;
after build it walks the field's inner state and moves the
document cursor to `position`, clamped to the document
length. Used by `DateEdit` / `TimeEdit` segment-stepping to
keep the caret within its current segment after Up/Down.
