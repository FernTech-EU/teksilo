# Button Widget API Documentation

## Overview

The `Button` widget is a production-quality button implementation in the `fern-widgets` crate. It adheres to the V2 widget architecture, featuring signal-based reactivity, theme resolution at paint time, and non-generic design.

## Enums

### `ButtonVariant`

Defines the visual role of the button.

- **`Default`**: Primary action in a dialog or form. Filled with accent color, white label, no border.
- **`Regular`**: Non-primary action. Surface fill with a 1 dp border and primary text label.
- **`Flat`**: Borderless button for toolbars or inline contexts. Transparent at idle, light wash on hover.

### `InteractionState`

Internal interaction state of the button.

- **`Idle`**: Default state.
- **`Hovered`**: Mouse is hovering over the button.
- **`Pressed`**: Button is being pressed.
- **`Focused`**: Button has keyboard focus.
- **`Disabled`**: Button is disabled.

## Structs

### `Button`

A production-quality button widget.

#### Fields

- `label`: `String` — The text displayed on the button.
- `style`: `ButtonVariant` — Visual style of the button.
- `action`: `Option<CommandFactory>` — Closure to execute on activation.
- `enabled`: `bool` — Whether the button is enabled.
- `tooltip_text`: `Option<String>` — Tooltip text to display on hover.
- `interaction`: `Signal<InteractionState>` — Signal for interaction state.
- `root_child_id`: `Option<WidgetId>` — ID of the root child widget.

#### Methods

##### `new(label: impl Into<String>) -> Self`

Creates a new `Button` with the given label.

##### `style(mut self, style: ButtonVariant) -> Self`

Sets the visual style of the button.

##### `on_activate<C: AppCommand>(mut self, command: C) -> Self`

Sets the command to emit when the button is activated.

##### `on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Sets a custom closure to execute on activation.

##### `tooltip(mut self, text: impl Into<String>) -> Self`

Attaches a tooltip to the button.

##### `enabled(mut self, enabled: bool) -> Self`

Sets whether the button is enabled.

## Example Usage

```rust
Button::new("Save")
    .style(ButtonVariant::Default)
    .on_activate(AppCmd::Save)
```

## Implementation Details

- **Reactivity**: Uses signal-based reactivity for dynamic updates.
- **Theme Resolution**: Colors and styles are resolved at paint time.
- **Handlers**: Uses V2 attached handlers for event handling.
- **Accessibility**: Supports accessibility features like focus rings and keyboard navigation.