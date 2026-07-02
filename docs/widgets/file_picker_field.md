<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# FilePickerField

`FilePickerField` — a text-input preset for path entry with a Browse button.

Combines a `TextInput` with a trailing `IconButton` (the folder/browse glyph)
that opens a native file dialog and writes the chosen path back into the bound
`Signal<String>`. The three `FilePickerKind` variants map to the three
single-result dialog modes: open a file, pick a folder, or save a file.
Multi-file selection does not fit the "one editable line" pattern; use the
file-dialog API directly for that.

```ignore
// Requires ctx.signal() — shown as ignore per convention.
let path = ctx.signal(String::new());
let _f = FilePickerField::new(path.clone())
    .kind(FilePickerKind::OpenFile)
    .add_filter("Images", &["png", "jpg"])
    .placeholder(lit!("Choose a file…"));
```

## Builder methods at a glance

`kind`, `dialog_title`, `starting_dir`, `default_file_name`, `add_filter`, `on_pick`, `placeholder`, `label`, `validation`, `enabled`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/file_picker_field/index.html)

## `pub enum FilePickerKind`

Which file-dialog kind the trailing button opens.

```rust
pub enum FilePickerKind { /* variants */ }
```

### Variants

- **`OpenFile`** — Open an existing file. Default.
- **`PickFolder`** — Pick an existing folder.
- **`SaveFile`** — Pick a new or existing file location for saving.

## `pub struct FilePickerField`

A single-line path entry field with a trailing Browse button that invokes the
native file dialog and writes the chosen path back into the bound `Signal<String>`.

```rust
pub struct FilePickerField { /* fields */ }
```

### Methods

#### `pub fn new(text: Signal<String>) -> Self`

Construct a `FilePickerField` bound to `text`. The visible string
is updated on a successful pick; existing content is shown as-is.

#### `pub fn kind(mut self, kind: FilePickerKind) -> Self`

Pick the dialog kind opened by the Browse button.

#### `pub fn dialog_title(mut self, title: impl Into<LocalizedString>) -> Self`

Title shown in the file-dialog window caption.

#### `pub fn starting_dir(mut self, path: impl Into<PathBuf>) -> Self`

Directory the dialog opens in. If not set, the OS default is used.

#### `pub fn default_file_name(mut self, name: impl Into<String>) -> Self`

Pre-filled file name for the `FilePickerKind::SaveFile` dialog.
No-op for `OpenFile` / `PickFolder`.

#### `pub fn add_filter(mut self, label: impl Into<String>, extensions: &[&str]) -> Self`

Append an extension filter (label + extensions without leading dots).
Repeat to add multiple rows.

#### `pub fn on_pick(mut self, f: impl Fn(&FileDialogResult, &mut EventContext) + 'static) -> Self`

Hook invoked with the raw `FileDialogResult` after the dialog
closes — useful when the caller needs to react to cancellation
or backend errors. The bound text signal is already updated by
the time this fires (on success).

#### `pub fn placeholder(mut self, text: impl Into<LocalizedString>) -> Self`

Placeholder text shown when the field is empty.

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Accessible name for the path field.

#### `pub fn validation(mut self, validation: impl Into<Prop<ValidationState>>) -> Self`

Bind an external `ValidationState` signal — shown as the same inline
error/warning strip and border tint the inner `TextInput` renders (e.g.
"the chosen folder does not exist / is not writable").

#### `pub fn enabled(mut self, on: impl Into<Prop<bool>>) -> Self`

Set the initial enabled state for the text field and Browse button.
Forwarded to the arena at build time.

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain single-line tooltip shown after the hover delay.
Clears any previously set rich or composite tooltip (last call wins).

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip by registry key.
Clears any previously set plain or composite tooltip (last call wins).

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip from inline `crate::tooltip::TooltipContent`.
Clears any previously set plain or composite tooltip (last call wins).

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip whose body is an arbitrary widget tree.
Clears any previously set plain or rich tooltip (last call wins).
