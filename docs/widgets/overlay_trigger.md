<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# OverlayTrigger

`OverlayTrigger` — the shared "this widget opens that overlay" wrapper.

## Public functions

### `OverlayTrigger`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`around(widget: impl Widget + 'static)`](#overlaytrigger-around) |
| `Self` | [`around_id(id: WidgetId)`](#overlaytrigger-around_id) |
| | **Builder methods** |
| `Self` | [`named(name: impl Into<String>)`](#overlaytrigger-named) |
| `Self` | [`on_activate(f: impl Fn(&mut EventContext) + 'static)`](#overlaytrigger-on_activate) |
| | **Methods** |
| `bool` | [`has_on_activate()`](#overlaytrigger-has_on_activate) |

## Detailed description

Used by `Dialog`, `Snackbar`, `Wizard` and `PopoverWidget` whenever a
caller replaces the default `Button` trigger with a widget of their own.

#### One control, on whichever node takes focus

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

#### Pointer, touch and pen

The trigger has no geometry and no press visual of its own: it forwards the
caller's child, whose target and appearance are the child's. The opener is
an `on_tap`, attached beside the handlers already there (the external
bucket), so it happens on the release for every pointer kind and never
replaces what the child does itself. It goes on two nodes:

* **the wrapped widget**, so a tap anywhere on it opens, its margins and
  its non-interactive parts included;
* **the wrapped control**, when that is a node inside the widget rather
  than the widget itself. A control that handles taps of its own takes a
  tap before any ancestor sees it, so an opener left on the wrapper alone
  would answer Enter, Space and a reader's `Click` and never a click.

A tap is delivered to the innermost node that recognizes it, and nowhere
else, so a click on the control opens once, beside the control's own
response to it.

#### Opening on another button

The trigger opens on the buttons the wrapped widget accepts taps from,
`ButtonMask::PRIMARY` unless
it says otherwise. To open on a right click, give the wrapped widget
`.accept_tap_buttons(ButtonMask::SECONDARY)`; the inner control gets the
same mask for the opener, so a left click on it stays the control's own.
A second click with the same button on an open trigger closes it.

```ignore
PopoverWidget::new(OverlayTrigger::around(
    tag_chip.accept_tap_buttons(ButtonMask::SECONDARY),
))
.content(tag_menu)
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/overlay_trigger/index.html)

<a id="overlaytrigger"></a>

## `pub struct OverlayTrigger`

**Import:** `use teksilo::widgets::OverlayTrigger;`

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

<a id="overlaytrigger-around"></a>

#### `pub fn around(widget: impl Widget + 'static) -> Self`

Wrap any widget as a popover trigger.

<a id="overlaytrigger-around_id"></a>

#### `pub fn around_id(id: WidgetId) -> Self`

`around` for a widget already inserted by id.

<a id="overlaytrigger-named"></a>

#### `pub fn named(self, name: impl Into<String>) -> Self`

Set the trigger's accessible name.

Used when the wrapped widget takes no focus of its own. A wrapped
control keeps its own name.

<a id="overlaytrigger-has_on_activate"></a>

#### `pub fn has_on_activate(&self) -> bool`

Whether an activate handler is already installed.

<a id="overlaytrigger-on_activate"></a>

#### `pub fn on_activate(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Install the overlay's open/close handler. Routed onto the wrapped widget
as a pointer tap, and onto the node that takes focus as Enter/Space and
the AT `Click` action.
