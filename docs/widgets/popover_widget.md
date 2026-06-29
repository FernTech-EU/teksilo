<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# PopoverWidget

`PopoverWidget<T>` — a generic trigger that opens a popover when
activated, plus the `PopoverButton` / `PopoverIconButton` aliases.

Wraps a caller-built trigger (`T: PopoverTrigger`) with overlay
wiring: owns a `popover_open: Signal<bool>` toggled on activate /
dismiss, sets `has_popup` and `expanded_when` on the inner trigger so
AT announces the disclosure state, pre-builds the popover content as a
dormant subtree, and shows / hides it via `OverlayRequest`. The
`set_dormant` + `activate` + `show_overlay` sequence and the
dismiss-callback shape match `DateEdit`
so behavior across the disclosure family stays consistent.

```rust
# use bastyde_widgets::{Button, ButtonVariant, IconButton, MenuList, MenuItem, PopoverButton, PopoverIconButton};
# use bastyde_widgets::primitives::TextWidget;
# use bastyde_i18n::lit;
// Text trigger (HasPopup::Dialog by default, no caret):
let _w = PopoverButton::new(Button::new(lit!("Choose…")).variant(ButtonVariant::Plain))
    .content(TextWidget::new(lit!("Pick")));

// Icon trigger (HasPopup::Menu by default, corner caret on):
let _w = PopoverIconButton::new(IconButton::add().toolbar())
    .content(MenuList::new().item(MenuItem::new(lit!("New file"))));
```

# Trigger configuration overrides

`build()` configures the inner trigger by calling `has_popup`,
`expanded_when`, and `on_activate_fn` (and `share_interaction` when a
caret is shown). These **replace** any previous values the caller set
— in particular any `on_activate_fn` set before `::new` is discarded,
because the activate slot is owned by the popover wiring. Use
`on_open` / `on_close`, or observe `open_signal`, for side effects.

# Per-trigger differences (the `PopoverTrigger` trait)

`Button` and `IconButton` differ only in: the default `has_popup`
kind, whether the disclosure caret shows by default, whether the
caret is suppressed (IconButton at `Compact`), and how the caret's
color is derived. Those four points live behind `PopoverTrigger`;
everything else is shared by the generic.

## Builder methods at a glance

`content`, `placement`, `dismiss_behavior`, `fade_duration`, `has_popup_kind`, `show_disclosure_caret`, `on_open`, `on_close`, `open_signal`, `surface`, `bare`, `surface_style`, `surface_name`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/popover_widget/index.html)

## `pub struct PopoverWidget`

A trigger paired with a popover surface. See the module docs for the
contract on which trigger properties get overridden during `build()`.
Use the `PopoverButton` / `PopoverIconButton` aliases for the
concrete trigger types.

```rust
pub struct PopoverWidget<T: PopoverTrigger> { /* fields */ }
```

### Methods

#### `pub fn new(trigger: T) -> Self`

Wrap a pre-configured trigger. The popover content is set
separately via `Self::content` (required).

#### `pub fn content(mut self, content: impl Widget + 'static) -> Self`

Set the popover content — added to the tree as a dormant subtree
during `build()`, woken via
`EventContext::activate`
when the trigger fires. Required.

#### `pub fn placement(mut self, p: OverlayPlacement) -> Self`

Override the popover's placement relative to the trigger.
Default: `OverlayPlacement::BelowPreferred`.

#### `pub fn dismiss_behavior(mut self, b: DismissBehavior) -> Self`

Override the dismiss behavior. Default:
`DismissBehavior::EscapeOrClickOutside`.

#### `pub fn fade_duration(mut self, d: Duration) -> Self`

Animate the overlay in / out over the given duration. Default:
no fade. See `OverlayRequest::with_fade` for the mechanism.

#### `pub fn has_popup_kind(mut self, k: HasPopup) -> Self`

Override the `has_popup` kind announced by AT. Defaults to the
trigger type's `PopoverTrigger::default_has_popup`.

#### `pub fn show_disclosure_caret(mut self, on: bool) -> Self`

Whether to paint the disclosure triangle in the trigger's
bottom-right corner. Defaults to the trigger type's
`PopoverTrigger::default_show_caret`. The caret is
suppressed automatically when
`PopoverTrigger::suppress_caret` returns `true` (e.g.
`IconButton` at `Compact`) regardless of this flag. AT-hidden —
the popup is announced via `set_has_popup` + `set_expanded`.

#### `pub fn on_open(mut self, f: impl Fn() + 'static) -> Self`

Notification fired on the rising edge of the popover (after the
overlay show request is dispatched). No `EventContext` — observe
`Self::open_signal` from your `build()` if you need
frame / dispatch context.

#### `pub fn on_close(mut self, f: impl Fn() + 'static) -> Self`

Notification fired on the falling edge of the popover (when the
overlay's dismiss callback runs).

#### `pub fn open_signal(&self) -> Signal<bool>`

Observe-only handle to the popover-open state. Apps can
`ctx.effect(&pb.open_signal(), ...)` from their composite to react
with full `EventContext` — `on_open` / `on_close` are
notification-only (no ctx).

#### `pub fn surface(mut self, variant: PopoverVariant) -> Self`

Choose which themed `PopoverVariant` surface wraps the content.
Default is `PopoverVariant::Default` (elevated panel with
padding + shadow). The surface is resolved from the active
`PopoverStyle` (`theme.style_slots.popover`), so it themes
app-wide.

#### `pub fn bare(mut self) -> Self`

Opt OUT of the themed surface: the content is added raw, with no
background / border / padding. Use when the content already
supplies its own chrome — a `MenuList` (which
routes through the Menu `PopoverStyle` itself) or a hand-rolled
surface `Panel`. Without this, such content would be
double-chromed.

#### `pub fn surface_style(mut self, style: impl PopoverStyle) -> Self`

Per-call `PopoverStyle` override for the surface (highest
precedence over the theme slot and the built-in default). Mirrors
`Popover::style`. No effect under
`bare`.

#### `pub fn surface_name(mut self, name: impl Into<String>) -> Self`

Accessible name for the surface's `Role::Dialog` node. Defaults
to empty (the wrapped content usually carries its own role and
name). No effect under `bare` or for the Menu
variant (which is presentational).

## `pub type PopoverButton`

A `Button` that opens a popover when activated. Alias for
`PopoverWidget<Button>` — `HasPopup::Dialog`, no caret by default.

```rust
pub type PopoverButton = PopoverWidget<Button>;
```

## `pub type PopoverIconButton`

An `IconButton` that opens a popover when activated. Alias for
`PopoverWidget<IconButton>` — `HasPopup::Menu`, corner caret on by
default (skipped at `Compact`).

```rust
pub type PopoverIconButton = PopoverWidget<IconButton>;
```
