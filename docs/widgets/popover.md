<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Popover

`Popover` — a button that opens a floating panel anchored to itself.

`Popover` is the legacy one-type-does-everything disclosure widget: it
pairs a labelled `Button` trigger (or any custom trigger supplied via
`.trigger(...)`) with a themed popover surface and the full overlay
wiring (dormant pre-build, `activate` on open, `show_overlay`, dismiss
callback). For the more ergonomic generic form that works with both
`Button` and `IconButton` triggers see
`PopoverWidget` /
`PopoverButton` /
`PopoverIconButton`.

## Accessibility

The trigger announces `HasPopup::Dialog` and tracks open/closed state
via `set_expanded`. The popover surface carries `Role::Dialog` named
after the trigger label.

```rust
# use teksilo_widgets::popover::Popover;
# use teksilo_widgets::primitives::TextWidget;
# use teksilo_i18n::lit;
let _w = Popover::new(lit!("Choose…"))
    .content(TextWidget::new(lit!("Pick an option")));
```

## Builder methods at a glance

`surface_variant`, `style`, `content`, `content_id`, `variant`, `enabled`, `placement`, `dismiss_behavior`, `trigger`, `trigger_id`, `caret`, `caret_size`, `focus_on_show`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/popover/index.html)

## `pub struct Popover`

Labelled button that opens a floating popover panel. See the `module docs`.

```rust
pub struct Popover { /* fields */ }
```

### Methods

#### `pub fn new(label: impl Into<LocalizedString>) -> Self`

Construct a popover with the given trigger-button label. Supply content via
`.content(...)` before mounting.

#### `pub fn surface_variant(mut self, variant: teksilo_core::styles::PopoverVariant) -> Self`

Pick the popover surface's design-language variant. Default
`Default`. The active `PopoverStyle` decides what each variant
means (the IntUI default ships one chrome shape across all
variants and lets the inner content distinguish them; custom
styles can branch on the variant for distinct surfaces).

#### `pub fn style(mut self, style: impl teksilo_core::styles::PopoverStyle) -> Self`

Per-call style override for the popover surface chrome.
Replaces the theme-wide default `PopoverStyle` for just this
Popover instance.

#### `pub fn content(mut self, content: impl Widget + 'static) -> Self`

Set the popover body widget (required). Built as a dormant subtree during
`build()` and woken when the trigger is activated.

#### `pub fn content_id(mut self, id: WidgetId) -> Self`

Set the popover body by pre-registered `WidgetId`.

#### `pub fn variant(mut self, variant: ButtonVariant) -> Self`

Set the `ButtonVariant` used for the built-in text trigger. Default `Plain`.

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Enable or disable the trigger button, statically or reactively.
Default `true`.

#### `pub fn placement(mut self, placement: OverlayPlacement) -> Self`

Set the `OverlayPlacement` of the popover surface. Default
`OverlayPlacement::BelowPreferred`.

#### `pub fn dismiss_behavior(mut self, dismiss: DismissBehavior) -> Self`

Override the dismiss gesture. Default
`DismissBehavior::EscapeOrClickOutside`.

#### `pub fn trigger(mut self, trigger: impl Widget + 'static) -> Self`

Replace the built-in text `Button` with a custom trigger widget. The
custom trigger is wrapped in overlay machinery (focusable, tap / key /
AT-click open the panel) via an internal `OverlayTrigger`.

#### `pub fn trigger_id(mut self, id: WidgetId) -> Self`

Set a custom trigger by pre-registered `WidgetId`.

#### `pub fn caret(mut self, show_caret: bool) -> Self`

Show or hide the pointing caret between the popover panel and the
trigger. Default `true`.

#### `pub fn caret_size(mut self, caret_size: f32) -> Self`

Override the caret size in logical pixels (clamped to `0`). Default `10`.

#### `pub fn focus_on_show(mut self, slot: Rc<Cell<Option<WidgetId>>>) -> Self`

Request focus on a specific widget immediately after the popover
opens. The slot is written by the content widget during `build()`
(same pattern as `ComboBox`'s search-input slot).

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain-text tooltip to the trigger button. Clears any
previously set rich or composite tooltip on this `Popover`.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip driven by a registry key. Clears any
previously set plain or composite tooltip on this `Popover`.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip from an inline `crate::tooltip::TooltipContent`
value. Clears any previously set plain or composite tooltip.

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip (arbitrary widget body) to the trigger
button. Clears any previously set plain or rich tooltip.
