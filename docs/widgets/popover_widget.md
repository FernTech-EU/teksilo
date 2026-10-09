<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# PopoverWidget

`PopoverWidget<T>` — a generic trigger that opens a popover when
activated, plus the `PopoverButton` / `PopoverIconButton` aliases.

## Public types

| Kind | Name |
| ---: | :--- |
| `trait` | [`PopoverTrigger`](#popovertrigger) — A trigger widget usable with `PopoverWidget` |
| `type` | [`PopoverCustom`](#popovercustom) — A popover whose trigger is an arbitrary widget, wrapped in `OverlayTrigger` |
| `struct` | [`PopoverWidget`](#popoverwidget) — A trigger paired with a popover surface |
| `type` | [`PopoverButton`](#popoverbutton) — A `Button` that opens a popover when activated |
| `type` | [`PopoverIconButton`](#popovericonbutton) — An `IconButton` that opens a popover when activated |

## Public functions

### `PopoverWidget`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(trigger: T)`](#popoverwidget-new) |
| | **Builder methods** |
| `Self` | [`content(content: impl Widget + 'static)`](#popoverwidget-content) |
| `Self` | [`placement(p: OverlayPlacement)`](#popoverwidget-placement) |
| `Self` | [`dismiss_behavior(b: DismissBehavior)`](#popoverwidget-dismiss_behavior) |
| `Self` | [`fade_duration(d: Duration)`](#popoverwidget-fade_duration) |
| `Self` | [`has_popup_kind(k: HasPopup)`](#popoverwidget-has_popup_kind) |
| `Self` | [`show_disclosure_caret(on: bool)`](#popoverwidget-show_disclosure_caret) |
| `Self` | [`on_open(f: impl Fn() + 'static)`](#popoverwidget-on_open) |
| `Self` | [`on_close(f: impl Fn() + 'static)`](#popoverwidget-on_close) |
| `Self` | [`open_action(intent: &'static str)`](#popoverwidget-open_action) |
| `Self` | [`surface(variant: PopoverVariant)`](#popoverwidget-surface) |
| `Self` | [`bare()`](#popoverwidget-bare) |
| `Self` | [`surface_style(style: impl PopoverStyle)`](#popoverwidget-surface_style) |
| `Self` | [`surface_name(name: impl Into<String>)`](#popoverwidget-surface_name) |
| `Self` | [`tooltip(text: impl Into<teksilo_i18n::LocalizedString>)`](#popoverwidget-tooltip) |
| `Self` | [`rich_tooltip(key: impl Into<String>)`](#popoverwidget-rich_tooltip) |
| `Self` | [`rich_tooltip_content(content: crate::tooltip::TooltipContent)`](#popoverwidget-rich_tooltip_content) |
| `Self` | [`composite_tooltip(content: impl Widget + 'static)`](#popoverwidget-composite_tooltip) |
| | **Methods** |
| `Signal<bool>` | [`open_signal()`](#popoverwidget-open_signal) |

### `PopoverTrigger`

| Returns | Function |
| ---: | :--- |
| | **Builder methods** |
| `Self` | [`with_shared_interaction(signal: Signal<InteractionState>)`](#popovertrigger-with_shared_interaction) |
| `Self` | [`with_has_popup(kind: HasPopup)`](#popovertrigger-with_has_popup) |
| `Self` | [`with_expanded_when(open: Signal<bool>)`](#popovertrigger-with_expanded_when) |
| `Self` | [`with_on_activate(f: impl Fn(&mut EventContext) + 'static)`](#popovertrigger-with_on_activate) |
| | **Methods** |
| `bool { /* default implementation */ }` | [`suppress_caret()`](#popovertrigger-suppress_caret) |
| `Signal<TextRole>` | [`caret_role(interaction: &Signal<InteractionState>)`](#popovertrigger-caret_role) |
| `bool` | [`has_on_activate()`](#popovertrigger-has_on_activate) |
| | **Associated functions** |
| `HasPopup` | [`default_has_popup()`](#popovertrigger-default_has_popup) |
| `bool` | [`default_show_caret()`](#popovertrigger-default_show_caret) |

## Detailed description

Wraps a caller-built trigger (`T: PopoverTrigger`) with overlay
wiring: owns a `popover_open: Signal<bool>` toggled on activate /
dismiss, sets `has_popup` and `expanded_when` on the inner trigger so
AT announces the disclosure state, adds the popover content as a
dormant subtree whose panel is built the first time it is opened, and
shows / hides it via `OverlayRequest`. The
`set_dormant` + `activate` + `show_overlay` sequence and the
dismiss-callback shape match `DateEdit`
so behavior across the disclosure family stays consistent.

### Keyboard

Beyond whatever activates the trigger itself, `Alt+ArrowDown` opens the
popover and `Alt+ArrowUp` closes it — the platform disclosure chord
(Win32 / WinForms / WPF drop-downs, and the W3C ARIA combobox pattern).
Every consumer inherits it, so `PopoverButton`, `PopoverIconButton`
and `ColorEdit` share one
implementation. `F4` is deliberately *not* bound here: this generic also
backs toolbar chevrons and menu buttons, which carry no such
convention, so the drop-down fields bind it themselves.

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

### Trigger configuration overrides

`build()` configures the inner trigger by calling `has_popup`,
`expanded_when`, and `on_activate_fn` (and `share_interaction` when a
caret is shown). These **replace** any previous values the caller set
— in particular any `on_activate_fn` set before `::new` is discarded,
because the activate slot is owned by the popover wiring. Use
`on_open` / `on_close`, or observe `open_signal`, for side effects.

### Per-trigger differences (the `PopoverTrigger` trait)

`Button` and `IconButton` differ only in: the default `has_popup`
kind, whether the disclosure caret shows by default, whether the
caret is suppressed (IconButton at `Compact`), and how the caret's
color is derived. Those four points live behind `PopoverTrigger`;
everything else is shared by the generic.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/popover_widget/index.html)

<a id="popovertrigger"></a>

## `pub trait PopoverTrigger`

A trigger widget usable with `PopoverWidget`. Implemented for
`Button`, `IconButton` and `OverlayTrigger`. Captures the few
points where the triggers differ; everything else is handled by the
generic wrapper.

```rust
pub trait PopoverTrigger: Widget + Sized + 'static { /* associated items below */ }
```

### Associated items

<a id="popovertrigger-default_has_popup"></a>

#### `fn default_has_popup() -> HasPopup;`

The `has_popup` kind announced by AT when the caller doesn't
override it. `Button` → `HasPopup::Dialog`; `IconButton` →
`HasPopup::Menu`.

<a id="popovertrigger-default_show_caret"></a>

#### `fn default_show_caret() -> bool;`

Whether the disclosure caret is painted by default. `Button` →
`false` (text buttons advertise via an inline trailing chevron);
`IconButton` → `true` (icon-only triggers have no label slot).

<a id="popovertrigger-suppress_caret"></a>

#### `fn suppress_caret(&self) -> bool { /* default implementation */ }`

Whether the caret must be suppressed for this trigger regardless
of the flag (e.g. `IconButton` at `Compact` has no room).
Default: never suppressed.

<a id="popovertrigger-caret_role"></a>

#### `fn caret_role(&self, interaction: &Signal<InteractionState>) -> Signal<TextRole>;`

The `TextRole` the disclosure caret tints with, derived from the
shared interaction signal so the caret and trigger tint together
across hover / press / focus / disabled. Only called when a caret
is shown.

<a id="popovertrigger-with_shared_interaction"></a>

#### `fn with_shared_interaction(self, signal: Signal<InteractionState>) -> Self;`

Share an externally-allocated interaction signal so the caret colour
tracks the trigger's state (hover / press / focus / disabled) exactly.

<a id="popovertrigger-with_has_popup"></a>

#### `fn with_has_popup(self, kind: HasPopup) -> Self;`

Annotate the trigger with the given `has_popup` kind for AT.

<a id="popovertrigger-with_expanded_when"></a>

#### `fn with_expanded_when(self, open: Signal<bool>) -> Self;`

Bind the trigger's `set_expanded` disclosure state to `open`.

<a id="popovertrigger-with_on_activate"></a>

#### `fn with_on_activate(self, f: impl Fn(&mut EventContext) + 'static) -> Self;`

Install the popover's open/close handler as the trigger's activate callback.

<a id="popovertrigger-has_on_activate"></a>

#### `fn has_on_activate(&self) -> bool;`

Return `true` if the trigger already has an activate handler set by
the caller — the wrapper replaces it and will warn at build time.

<a id="popovercustom"></a>

## `pub type PopoverCustom`

A popover whose trigger is an arbitrary widget, wrapped in
`OverlayTrigger`.

The third stock shape beside `PopoverButton` and `PopoverIconButton`,
and what replaced the standalone `Popover` widget: that type existed only
because this generic could not take a non-button trigger.

```rust
pub type PopoverCustom = PopoverWidget<OverlayTrigger>;
```

<a id="popoverwidget"></a>

## `pub struct PopoverWidget`

A trigger paired with a popover surface. See the module docs for the
contract on which trigger properties get overridden during `build()`.
Use the `PopoverButton` / `PopoverIconButton` aliases for the
concrete trigger types.

```rust
pub struct PopoverWidget<T: PopoverTrigger> { /* fields */ }
```

### Methods

<a id="popoverwidget-new"></a>

#### `pub fn new(trigger: T) -> Self`

Wrap a pre-configured trigger. The popover content is set
separately via `Self::content` (required).

<a id="popoverwidget-content"></a>

#### `pub fn content(mut self, content: impl Widget + 'static) -> Self`

Set the popover content — added to the tree as a dormant subtree
during `build()`, woken via
`EventContext::activate`
when the trigger fires. Required.

<a id="popoverwidget-placement"></a>

#### `pub fn placement(mut self, p: OverlayPlacement) -> Self`

Override the popover's placement relative to the trigger.
Default: `OverlayPlacement::BelowPreferred`.

<a id="popoverwidget-dismiss_behavior"></a>

#### `pub fn dismiss_behavior(mut self, b: DismissBehavior) -> Self`

Override the dismiss behavior. Default:
`DismissBehavior::EscapeOrClickOutside`.

<a id="popoverwidget-fade_duration"></a>

#### `pub fn fade_duration(mut self, d: Duration) -> Self`

Animate the overlay in / out over the given duration. Default:
no fade. See `OverlayRequest::with_fade` for the mechanism.

<a id="popoverwidget-has_popup_kind"></a>

#### `pub fn has_popup_kind(mut self, k: HasPopup) -> Self`

Override the `has_popup` kind announced by AT. Defaults to the
trigger type's `PopoverTrigger::default_has_popup`.

<a id="popoverwidget-show_disclosure_caret"></a>

#### `pub fn show_disclosure_caret(mut self, on: bool) -> Self`

Whether to paint the disclosure triangle in the trigger's
bottom-right corner. Defaults to the trigger type's
`PopoverTrigger::default_show_caret`. The caret is
suppressed automatically when
`PopoverTrigger::suppress_caret` returns `true` (e.g.
`IconButton` at `Compact`) regardless of this flag. AT-hidden —
the popup is announced via `set_has_popup` + `set_expanded`.

<a id="popoverwidget-on_open"></a>

#### `pub fn on_open(mut self, f: impl Fn() + 'static) -> Self`

Notification fired on the rising edge of the popover (after the
overlay show request is dispatched). No `EventContext` — observe
`Self::open_signal` from your `build()` if you need
frame / dispatch context.

<a id="popoverwidget-on_close"></a>

#### `pub fn on_close(mut self, f: impl Fn() + 'static) -> Self`

Notification fired on the falling edge of the popover (when the
overlay's dismiss callback runs).

<a id="popoverwidget-open_signal"></a>

#### `pub fn open_signal(&self) -> Signal<bool>`

Observe-only handle to the popover-open state.

**Read-back only — writing this does not open the popover.** Presenting
an overlay needs an `EventContext` (`show_overlay` + `request_focus`),
which no signal observer has; this field is the mirror the trigger writes
after it has done that work. To open the popover from somewhere other
than its trigger, use `open_action`.

<a id="popoverwidget-open_action"></a>

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

<a id="popoverwidget-surface"></a>

#### `pub fn surface(mut self, variant: PopoverVariant) -> Self`

Choose which themed `PopoverVariant` surface wraps the content.
Default is `PopoverVariant::Default` (elevated panel with
padding + shadow). The surface is resolved from the active
`PopoverStyle` (`theme.style_slots.popover`), so it themes
app-wide.

<a id="popoverwidget-bare"></a>

#### `pub fn bare(mut self) -> Self`

Opt OUT of the themed surface: the content is added raw, with no
background / border / padding. Use when the content already
supplies its own chrome — a `MenuList` (which
routes through the Menu `PopoverStyle` itself) or a hand-rolled
surface `Panel`. Without this, such content would be
double-chromed.

<a id="popoverwidget-surface_style"></a>

#### `pub fn surface_style(mut self, style: impl PopoverStyle) -> Self`

Per-call `PopoverStyle` override for the surface (highest
precedence over the theme slot and the built-in default). Mirrors
the per-call override the standalone `Popover` used to offer. No effect under
`bare`.

<a id="popoverwidget-surface_name"></a>

#### `pub fn surface_name(mut self, name: impl Into<String>) -> Self`

Accessible name for the surface's `Role::Dialog` node. Without one
the surface is named by the trigger that opens it. No effect under
`bare` or for the Menu variant (which is
presentational).

<a id="popoverwidget-tooltip"></a>

#### `pub fn tooltip(mut self, text: impl Into<teksilo_i18n::LocalizedString>) -> Self`

Show a plain single-line tooltip on the trigger after a hover delay.
Mutually exclusive with `rich_tooltip`,
`rich_tooltip_content`, and
`composite_tooltip` — each setter clears
the other three so the last call wins. The tooltip anchors on the
trigger, not on the popover content.

<a id="popoverwidget-rich_tooltip"></a>

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Show a rich tooltip (looked up by registry key) on the trigger after
a hover delay. Mutually exclusive with the other tooltip setters —
the last call wins.

<a id="popoverwidget-rich_tooltip_content"></a>

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Show an inline rich tooltip (pre-built `TooltipContent`) on the
trigger after a hover delay. Mutually exclusive with the other tooltip
setters — the last call wins.


<a id="popoverwidget-composite_tooltip"></a>

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Show a composite tooltip (arbitrary widget tree) on the trigger after
a longer hover delay. Mutually exclusive with the other tooltip setters
— the last call wins.

<a id="popoverbutton"></a>

## `pub type PopoverButton`

A `Button` that opens a popover when activated. Alias for
`PopoverWidget<Button>` — `HasPopup::Dialog`, no caret by default.

```rust
pub type PopoverButton = PopoverWidget<Button>;
```

<a id="popovericonbutton"></a>

## `pub type PopoverIconButton`

An `IconButton` that opens a popover when activated. Alias for
`PopoverWidget<IconButton>` — `HasPopup::Menu`, corner caret on by
default (skipped at `Compact`).

```rust
pub type PopoverIconButton = PopoverWidget<IconButton>;
```
