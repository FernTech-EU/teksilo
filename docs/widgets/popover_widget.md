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
# use teksilo_widgets::{Button, ButtonVariant, IconButton, MenuList, MenuItem, PopoverButton, PopoverIconButton};
# use teksilo_widgets::primitives::TextWidget;
# use teksilo_i18n::lit;
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

`content`, `placement`, `dismiss_behavior`, `fade_duration`, `has_popup_kind`, `show_disclosure_caret`, `on_open`, `on_close`, `open_signal`, `open_action`, `surface`, `bare`, `surface_style`, `surface_name`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/popover_widget/index.html)

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

Observe-only handle to the popover-open state.

**Read-back only — writing this does not open the popover.** Presenting
an overlay needs an `EventContext` (`show_overlay` + `request_focus`),
which no signal observer has; this field is the mirror the trigger writes
after it has done that work. To open the popover from somewhere other
than its trigger, use `open_action`.

#### `pub fn open_action(mut self, intent: &'static str) -> Self`

Register a **named global action** that toggles this popover, so a menu
entry, a global shortcut or `ctx.send_intent(...)` can open it — not only
a click on its own trigger.

Without this a popover is reachable by pointer alone. `on_open` /
`on_close` are notification-only and `open_signal` is a read-back mirror
(see its doc), so an app that wanted "Go to… ⌘G" next to its button had
no way to wire the second half. Action handlers are the one place that
*does* get an `EventContext`, which is exactly what presenting an overlay
requires — so the action runs the identical toggle the trigger runs, and
the two can never drift.

Registered with `register_action_global`, deliberately: intents walk
source-widget → root, and a menu renders in an **overlay** that is a
sibling of the popover's own subtree, so a plain `register_action` would
never be reached from a menu item. Pair it with
`register_shortcut_global` in the app for the keystroke.

```ignore
PopoverButton::new(Button::new(tr!(go_to())))
    .content(palette)
    .open_action("go.to")
// elsewhere: MenuEntry::new(tr!(go_to())).intent("go.to").shortcut("go.to")
```

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

#### `pub fn tooltip(mut self, text: impl Into<teksilo_i18n::LocalizedString>) -> Self`

Show a plain single-line tooltip on the trigger after a hover delay.
Mutually exclusive with `rich_tooltip`,
`rich_tooltip_content`, and
`composite_tooltip` — each setter clears
the other three so the last call wins. The tooltip anchors on the
trigger, not on the popover content.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Show a rich tooltip (looked up by registry key) on the trigger after
a hover delay. Mutually exclusive with the other tooltip setters —
the last call wins.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Show an inline rich tooltip (pre-built `TooltipContent`) on the
trigger after a hover delay. Mutually exclusive with the other tooltip
setters — the last call wins.


#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Show a composite tooltip (arbitrary widget tree) on the trigger after
a longer hover delay. Mutually exclusive with the other tooltip setters
— the last call wins.

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
