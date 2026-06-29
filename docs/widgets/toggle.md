<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Toggle

Toggle — an animated on/off switch bound to a `Signal<bool>`.

Renders as a sliding-knob switch (IntUI default) or one of the alternate
`ToggleVariant` shapes. All visual chrome is delegated to a `ToggleStyle`
impl; the widget itself owns only event handling (tap, Space, AccessKit
`Click`). The IntUI recipe
(`crate::styles::RecipeToggleStyle`) ships out of the box; apps install a
custom look per-call with `.style(impl ToggleStyle)` or theme-wide via
`theme.style_slots.toggle = Some(Rc::new(…))`.

## Accessibility

Emits `Role::Switch` with `toggled` reflecting the signal value. Always pair
with `.label(…)` — the debug build asserts that a label is present, and
screen readers will announce "switch" with no context if it is absent.

## Example

```rust
# use bastyde_widgets::Toggle;
# use bastyde_core::signal::Signal;
# use bastyde_i18n::lit;
let dark_mode = Signal::new(false);
let _w = Toggle::new(dark_mode)
    .label(lit!("Dark mode"));
```

## Builder methods at a glance

`label`, `enabled`, `variant`, `style`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/toggle/index.html)

## `pub struct Toggle`

An animated toggle switch bound to a `Signal<bool>`.

```rust
pub struct Toggle { /* fields */ }
```

### Methods

#### `pub fn new(on: Signal<bool>) -> Self`

Create a toggle bound to `on`. The signal is both read (to paint the
current state) and written (flipped on each activation).

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Accessible label announced by AT and optionally displayed beside the switch.

#### `pub fn enabled(mut self, enabled: bool) -> Self`

Set the initial enabled state. Forwarded to the arena via
`ctx.enabled_when(self_id, false)` at build time. Reactive
enable/disable is supported via `ctx.enabled_when(id, signal)`.

#### `pub fn variant(mut self, variant: ToggleVariant) -> Self`

Pick a Tier-1 design-language variant
(`ToggleVariant::Switch` / `Pill` / `Square` / `Inset`). The
active `ToggleStyle` decides what to do with the hint —
IntUI's default impl honours all four; a custom impl might
ignore the variant entirely.

#### `pub fn style(mut self, style: impl ToggleStyle) -> Self`

Override the active `ToggleStyle` for this widget instance
only. Useful for one-off custom-painted toggles in a single
view.
