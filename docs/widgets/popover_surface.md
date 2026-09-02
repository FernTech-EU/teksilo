<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# PopoverSurface

`PopoverSurface` — the themed panel a popover's content sits in.

Style infrastructure, not a widget an app mounts: `RecipePopoverStyle` (and
any `PopoverStyle` replacing it) constructs one in `make_body`, and
`PopoverWidget` shows the result as its overlay. It lived in `popover.rs`
beside the standalone `Popover` widget until that type was removed; the two
were never related beyond sharing a file.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/popover_surface/index.html)

## `pub struct PopoverSurface`

```rust
pub struct PopoverSurface { /* fields */ }
```

### Methods

#### `pub fn new( content: PendingChild, placement: OverlayPlacement, show_caret: bool, caret_size: f32, name: String, content_padding: EdgeInsets, background: SurfaceRole, corner_radius: f32, presentational: bool, ) -> Self`
