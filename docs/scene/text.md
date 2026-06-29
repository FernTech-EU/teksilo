<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TextItem

`TextItem` — unstyled text in a local-coord rectangle.

`TextItem` renders text that wraps within a caller-specified rectangle in
local item coordinates. Text can be a static localized string (constructed
via `TextItem::new`) or a live `Signal<String>` (constructed via
`TextItem::with_signal_text`). Signal-bound and locale-reactive text both
register bindings at `RepaintOnly` so changes dirty the `SceneView`'s
paint pass without triggering a full rebuild.

Text scale: the global accessibility "grow all text" setting is **off** by
default for scene text, since a scene has its own pan/zoom. Opt in via
`.follow_text_scale(true)` for labels that should track the app-wide
setting instead.

## When to use

Use `TextItem` for card labels, node titles, annotation text, or any text
decoration in the lightweight tier. For editable text or text that needs
focus, selection, and full accessibility, embed a `RichTextEditor` or
`TextInput` as a heavyweight scene widget instead.

## Example

```ignore
use bastyde_scene::{SceneModel, TextItem};
use bastyde_canvas::{Point, Rect};
use bastyde_tokens::Color;
use bastyde_i18n::lit;

let model = SceneModel::new();

let item = TextItem::new(lit!("Scene node"), Rect::new(0.0, 0.0, 120.0, 30.0))
    .color(Color::new(0.1, 0.1, 0.1, 1.0));

model.add_item(item, Point::new(40.0, 40.0));
```

## Builder methods at a glance

`with_signal_text`, `draggable`, `color`, `follow_text_scale`, `label`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_scene/index.html)

## `pub struct TextItem`

Unstyled text in a local-coord rectangle.

Text wraps within the `local_bounds` rectangle; the caller is responsible
for sizing the rect so all text is visible. Content is either a static
localized string (see `TextItem::new`) or a reactive `Signal<String>`
(see `TextItem::with_signal_text`). Both sources trigger a repaint on
change without rebuilding the scene.

```rust
pub struct TextItem { /* fields */ }
```

### Methods

#### `pub fn new(text: impl Into<LocalizedString>, local_bounds: Rect) -> Self`

A static-text item in local coordinates. The `text` is
resolved eagerly via `LocalizedString::resolve_now` at
construction; locale changes rebuild the composite parent,
which re-creates this `TextItem` with a fresh translation.

#### `pub fn with_signal_text(text: Signal<String>, local_bounds: Rect) -> Self`

A text item whose content is driven by a `Signal<String>`.
`register_bindings` ties the signal to the SceneView at
`BindingLevel::RepaintOnly` so changes dirty paint and the
next walk reads the current value.

#### `pub fn draggable(mut self, draggable: bool) -> Self`

Opt the text into drag-to-move.

#### `pub fn color(mut self, color: Color) -> Self`

Override the foreground color.

#### `pub fn follow_text_scale(mut self, follow: bool) -> Self`

Opt this text into the global accessibility text scale, so it grows with
the app-wide "grow all text" setting. Off by default — the scene's own
pan/zoom usually governs scene text size.

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the AT label (defaults to the current text content).
