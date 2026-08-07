<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# InputDialog

InputDialog — a `QInputDialog`-style modal that prompts the user for
a single string. Built on the same `present_modal` infrastructure as
`MessageBox`, with a `TextInput`
body between the prompt and the Ok / Cancel buttons.

Use `MessageBox` when the dialog
conveys information without requiring data; use `InputDialog` when
the modal needs to capture exactly one short string. Forms longer
than a single field belong in a custom `Dialog`.

```ignore
InputDialog::new(tr!(rename_title()))
    .prompt(tr!(rename_prompt()))
    .default_text(current_name)
    .placeholder("New name")
    .on_result(|result, _ctx| {
        if let Some(name) = result {
            rename(name);
        }
    })
    .present(ctx);
```

## Live validation

`validate` runs on every keystroke and both **disables OK**
and shows its message under the field, so a value the caller cannot accept can never
be submitted:

```ignore
InputDialog::new(tr!(save_as_template_title()))
    .validate(move |name| {
        if name.trim().is_empty() {
            Err(None)                                  // block, say nothing
        } else if let Some(clash) = taken(name) {
            Err(Some(tr!(duplicate(name = clash))))    // block, and explain
        } else {
            Ok(())
        }
    })
    .on_result(|result, _| { /* only ever called with a valid value */ })
    .present(ctx);
```

`Err(None)` is the "not yet" case — it disables OK without printing anything, which
is what an *untouched* empty field wants: shouting at someone before they have typed
is noise, and the greyed button already says the dialog is not ready. A message is
withheld until the field has been edited for the same reason, so a caller can return
`Err(Some(..))` for the empty case without it flashing on open.

## Builder methods at a glance

`prompt`, `placeholder`, `default_text`, `ok_label`, `cancel_label`, `on_result`, `validate`, `present`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/input_dialog/index.html)

## `pub type ValidateResult`

Verdict from an `InputDialog::validate` callback.

`Ok(())` accepts. `Err(None)` blocks silently; `Err(Some(msg))` blocks and shows
`msg` beneath the field once it has been edited.

```rust
pub type ValidateResult = Result<(), Option<LocalizedString>>;
```

## `pub struct InputDialog`

A single-field input modal.

```rust
pub struct InputDialog { /* fields */ }
```

### Methods

#### `pub fn new(title: impl Into<LocalizedString>) -> Self`

Construct a new input dialog with the given title.

#### `pub fn prompt(mut self, text: impl Into<LocalizedString>) -> Self`

Prompt rendered above the input field. Optional but recommended.

#### `pub fn placeholder(mut self, text: impl Into<LocalizedString>) -> Self`

Placeholder shown when the field is empty.

#### `pub fn default_text(mut self, text: impl Into<String>) -> Self`

Initial value pre-filled into the field.

#### `pub fn ok_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the OK button label (defaults to the framework's
translated "OK" string).

#### `pub fn cancel_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the Cancel button label (defaults to the framework's
translated "Cancel" string).

#### `pub fn on_result(mut self, f: impl Fn(Option<String>, &mut EventContext) + 'static) -> Self`

Result callback. Invoked exactly once when the user accepts
(`Some(value)`) or cancels (`None`).

#### `pub fn validate(mut self, f: impl Fn(&str) -> ValidateResult + 'static) -> Self`

Install a **live** validator, run on every keystroke.

While it returns `Err`, the OK button is disabled and Enter does nothing, so
`on_result` is only ever called with a value the validator
accepted (or with `None`, for Cancel). `Err(Some(msg))` shows `msg` under the
field; `Err(None)` blocks without saying anything.

The message is withheld until the field has been edited, so a validator that
rejects the empty string does not greet the writer with an error on a dialog they
have not yet typed into. The disabled OK is what communicates "not yet" there.

Distinct from `TextInput::validator`,
which fires on *commit* and cannot gate a dialog's accept path.

#### `pub fn present(self, ctx: &mut EventContext)`

Present the dialog as a modal on top of `ctx`'s tree. Consumes
`self`.
