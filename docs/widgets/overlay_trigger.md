<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# OverlayTrigger

`OverlayTrigger` — the shared "this widget opens that overlay" wrapper.

Used by `Dialog`, `Snackbar`, `Wizard` and `PopoverWidget` whenever a
caller replaces the default `Button` trigger with a widget of their own.

## One control, on whichever node takes focus

To a screen reader a trigger is one control, and the node focus lands on is
that control: it carries the role, the name, the popup state, and it
answers Enter, Space and the AT `Click`. Which node that is depends on what
was wrapped, and is decided once, as it mounts:

* **A widget that takes no focus** (a panel, a glyph, a label): the
  trigger's own node is the button. It is the Tab stop and it carries
  everything above.
* **A control of its own** (a `Button`, an `IconButton`, anything that is or
  holds a focus stop, even one disabled as it mounts): that control is the
  button. Its role and its own text stand, the opening routes and the popup
  state are added to it, and the trigger's node is structure, which the
  tree walk leaves out. The name given to the trigger is not used there:
  the control's own text is what a sighted user reads on it, and a second
  button around the first would be one control heard as two.

## Touch and pen

The trigger has no geometry and no press visual of its own: it forwards the
caller's child, whose target and appearance are the child's, and routes the
pointer handler onto that child's external bucket so it fires beside the
child's own. The activation is an `on_tap`, so it happens on the release for
every pointer kind.

## Builder methods at a glance

`around`, `around_id`, `named`, `has_on_activate`, `on_activate`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/overlay_trigger/index.html)

## `pub struct OverlayTrigger`

Wraps an arbitrary widget so it can drive a popover.

`PopoverButton` and `PopoverIconButton` cover the two stock triggers; this
is the third case — a trigger that is *not* a button, such as a table
header's filter glyph or a tag chip. It supplies what those two get from
`Button`/`IconButton`: a Tab stop, an activate route (pointer, Enter/Space,
and the AT `Click` action), the `has_popup` / `expanded` disclosure
annotations, and the arena-level `enabled` gate. A wrapped widget that is a
control already keeps its own focus, role and name; see the module docs.

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

Used when the wrapped widget takes no focus of its own. A wrapped
control keeps its own name.

#### `pub fn has_on_activate(&self) -> bool`

Whether an activate handler is already installed.

#### `pub fn on_activate(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Install the overlay's open/close handler. Routed onto the wrapped widget
as a pointer tap, and onto the node that takes focus as Enter/Space and
the AT `Click` action.
