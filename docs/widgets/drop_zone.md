<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# DropZone

`DropZone` — a "drop files here" target for external (OS) drag-and-drop.

A bordered, tinted region that accepts files / text / URLs dragged in from
the operating system (Finder, Explorer, Nautilus) or another application.
It reacts to hover (accept / reject highlight) and fires typed callbacks on
drop. Because an OS drag cannot be initiated from the keyboard, the zone
also offers a keyboard-operable **Browse…** button (opening the native file
dialog) as the WCAG 2.1.1 equivalent.

```ignore
DropZone::new(tr!("drop_images_here"))
    .subtitle(tr!("png_or_jpeg"))
    .accept_extensions(["png", "jpg", "jpeg"])
    .allow_multiple(true)
    .on_files_dropped(|paths, _ctx| { /* import paths */ });
```

External drops are delivered through the framework's normal drag pipeline
(`on_drag_hover` / `on_drag_leave` / `on_drop`) once
[`install_external_dnd`](https://docs.rs/teksilo-app) is wired and a backend
is available; on platforms with no backend (e.g. X11) the Browse button
keeps the zone fully usable.

# Styling

The bordered, tinted chrome is a Tier-3 `DropZoneStyle`; the default
`RecipeDropZoneStyle` tracks the
interaction state. Override per-call with `DropZone::style` or theme-wide
via `theme.style_slots.drop_zone`.

# Accessibility

The zone is a `Role::Group` labelled by its prompt, with a `Live::Polite`
status line that announces hover ("Drop to add 3 files"), success
("3 files added"), and rejection. AccessKit models no drag/drop action and
ARIA's `aria-grabbed` / `aria-dropeffect` are deprecated, so live-region
announcements plus the Browse fallback are the supported pattern.

## Builder methods at a glance

`subtitle`, `accept_extensions`, `allow_multiple`, `show_browse_button`, `starting_dir`, `browse_label`, `icon`, `style`, `on_files_dropped`, `on_text_dropped`, `on_urls_dropped`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/drop_zone/index.html)

## `pub struct DropZone`

A drop target for external (OS) drag-and-drop. See the module docs.

```rust
pub struct DropZone { /* fields */ }
```

### Methods

#### `pub fn new(label: impl Into<LocalizedString>) -> Self`

Build a drop zone with the given prompt (e.g. `tr!("drop_files_here")`).
The label may come from `tr!(...)` (translated) or
`lit!(...)`; it is resolved eagerly at construction
and stored as a `String`. Locale changes rebuild the composite parent,
which re-creates the `DropZone` with a fresh translation — the same
model as `Button::new`.

#### `pub fn subtitle(mut self, text: impl Into<LocalizedString>) -> Self`

Secondary line under the prompt (e.g. `tr!("png_or_jpeg")`).

#### `pub fn accept_extensions<I, S>(mut self, extensions: I) -> Self where I: IntoIterator<Item = S>, S: Into<String>,`

Restrict accepted files to these extensions (without leading dots,
case-insensitive). Empty (the default) accepts any file. Text and URL
drops are unaffected.

#### `pub fn allow_multiple(mut self, allow: bool) -> Self`

Whether more than one file may be dropped at once. Default `true`.
When `false`, a multi-file drop is rejected.

#### `pub fn show_browse_button(mut self, show: bool) -> Self`

Show or hide the keyboard-operable Browse button. Default `true`.
Keeping it visible is strongly recommended — it is the only
keyboard-accessible path to the zone's action.

#### `pub fn starting_dir(mut self, path: impl Into<PathBuf>) -> Self`

Override the Browse button's label (e.g. `tr!("browse")`).
Directory the Browse button's dialog opens in. If unset, the OS default is
used.

The same builder `FilePickerField::starting_dir`
offers, and for the same reason: an app that remembers where its writer last
picked files has no way to say so otherwise, because this widget builds its own
`FileDialogRequest` internally rather than taking one.

#### `pub fn browse_label(mut self, label: impl Into<LocalizedString>) -> Self`

#### `pub fn icon(mut self, icon: impl Widget + 'static) -> Self`

An icon widget shown above the prompt (any widget — typically an
`IconWidget`).

#### `pub fn style(mut self, style: impl DropZoneStyle) -> Self`

Override the Tier-3 `DropZoneStyle` for this instance only.

#### `pub fn on_files_dropped( mut self, f: impl FnMut(Vec<PathBuf>, &mut EventContext) + 'static, ) -> Self`

Called with the dropped (or browsed) file paths. Files are only
accepted when this is set.

#### `pub fn on_text_dropped(mut self, f: impl FnMut(String, &mut EventContext) + 'static) -> Self`

Called with dropped plain text. Text drops are only accepted when set.

#### `pub fn on_urls_dropped( mut self, f: impl FnMut(Vec<String>, &mut EventContext) + 'static, ) -> Self`

Called with dropped non-file URLs. URL drops are only accepted when set.
