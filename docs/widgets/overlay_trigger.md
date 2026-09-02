<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# OverlayTrigger

## Builder methods at a glance

`around`, `around_id`, `named`, `has_on_activate`, `on_activate`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/overlay_trigger/index.html)

## `pub struct OverlayTrigger`

Wraps an arbitrary widget so it can drive a popover.

`PopoverButton` and `PopoverIconButton` cover the two stock triggers; this
is the third case — a trigger that is *not* a button, such as a table
header's filter glyph or a tag chip. It supplies what those two get from
`Button`/`IconButton`: an activate route (pointer, Enter/Space, and the
AT `Click` action), the `has_popup` / `expanded` disclosure annotations, and
the arena-level `enabled` gate.

```ignore
PopoverWidget::new(OverlayTrigger::around(my_glyph))
    .content(my_panel)
    .placement(OverlayPlacement::BelowPreferred)
```

```rust
pub struct OverlayTrigger { /* fields */ }
```

### Methods

#### `pub fn around(widget: impl Widget + 'static) -> Self`

Wrap any widget as a popover trigger.

#### `pub fn around_id(id: WidgetId) -> Self`

`around` for a widget already inserted by id.

#### `pub fn named(self, name: impl Into<String>) -> Self`

Set the trigger's accessible name.

#### `pub fn has_on_activate(&self) -> bool`

Whether an activate handler is already installed.

#### `pub fn on_activate( mut self, f: impl Fn(&mut teksilo_core::widget::EventContext) + 'static, ) -> Self`

Install the popover's open/close handler. Routed onto the wrapped widget
as pointer-tap, Enter/Space and the AT `Click` action.
