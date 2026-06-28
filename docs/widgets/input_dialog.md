<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# InputDialog

InputDialog — a `QInputDialog`-style modal that prompts the user for
a single string. Built on the same `present_modal` infrastructure as
`MessageBox`, with a [`TextInput`]
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

## Builder methods at a glance

`prompt`, `placeholder`, `default_text`, `ok_label`, `cancel_label`, `on_result`, `present`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/input_dialog/index.html)

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

#### `pub fn present(self, ctx: &mut EventContext)`

Present the dialog as a modal on top of `ctx`'s tree. Consumes
`self`.
