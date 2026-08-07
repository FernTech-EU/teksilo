<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# IconButton

IconButton — a square, icon-only, flat-surface button.

Five sizes covering both **embedded** use (inside another widget's
trailing slot — TextInput's clear-X, ComboBox's chevron, SearchField's
magnifier) and **stand-alone** use (toolbars, rich menus, hero CTAs).
The `.embedded()` flag opts into the JetBrains "built-in" look —
dimmer icon at rest (Secondary), brightening on hover (Primary),
flashing accent on press — so an IconButton living inside a TextInput
doesn't compete visually with the field's text. Without the flag the
icon stays at full visual weight (Primary at rest), the right default
for stand-alone toolbar / menu rows.

```rust
# use teksilo_widgets::{IconButton};
# use teksilo_widgets::primitives::IconWidget;
# use teksilo_i18n::lit;
# use teksilo_core::Intent;
# const MY_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg'/>";
// Stand-alone toolbar use — full-weight icon.
let _w = IconButton::new(IconWidget::from_svg(MY_SVG))
    .toolbar()
    .tooltip(lit!("Save"))
    .on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.save")));

// Embedded inside a TextInput's trailing slot — dim until hover.
let _w = IconButton::clear()
    .embedded()
    .on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.clear")));
```

## Predefined constructors

Common roles ship with the appropriate icon and an i18n tooltip
(which doubles as the AT name). They are size- and mode-agnostic —
call `.embedded()`, `.toolbar()`, `.large()`, etc. to configure:

```rust
# use teksilo_widgets::IconButton;
# use teksilo_core::signal::Signal;
# let visible = Signal::new(false);
let _w = IconButton::browse().embedded();           // 24 dp, dim — TextInput trailing
let _w = IconButton::clear().embedded();            // 24 dp, dim — clear-X
let _w = IconButton::search().toolbar();            // 40 dp, full weight — toolbar
let _w = IconButton::visibility_toggle(visible);    // password-field eye toggle
```

## Bistate

Two distinct toggle modes:

- `IconButton::toggle` — surface-tint bistate: clicking flips the
  bound `Signal<bool>`; while `true`, the background reads as
  `SurfaceRole::Selected` ("on"). Same icon throughout. The
  pin-this-row / select-this-tool pattern.
- `IconButton::toggle_with_icon` — surface-tint **and** icon-swap
  bistate: same surface flip plus the icon glyph swaps to a second
  icon. The visibility-toggle pattern (eye ↔ eye-off).

## Slot convention

Host widgets that accept icon buttons follow the `trailing_slot`
convention established by `TabWidget`:

```rust
# use teksilo_widgets::{IconButton, TextInput};
# use teksilo_widgets::primitives::HStack;
# use teksilo_core::signal::Signal;
# use teksilo_core::Intent;
# let value = Signal::new(String::new());
let _w = TextInput::new(value)
    .trailing_slot(HStack::new().spacing(0.0)
        .child(IconButton::clear().embedded().on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.clear"))))
        .child(IconButton::browse().embedded().on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.browse"))))
    );
```

## Builder methods at a glance

`style`, `style_shared`, `size_variant`, `is_embedded`, `share_interaction`, `embedded`, `icon_role`, `focusable`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`, `composite_tooltip_boxed`, `enabled`, `size`, `toolbar`, `large`, `hero`, `on_activate_fn`, `toggle`, `toggle_with_icon`, `has_popup`, `expanded_when`, `browse`, `expand`, `search`, `copy`, `clear`, `add`, `bell`, `menu`, `more`, `visibility_toggle`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/icon_button/index.html)

## `pub struct IconButton`

A square, icon-only, flat-surface button. See module docs for
embedded vs stand-alone modes, the five sizes, and the two bistate
toggle modes.

```rust
pub struct IconButton { /* fields */ }
```

### Methods

#### `pub fn new(icon: IconWidget) -> Self`

Create an icon button from a custom icon. Defaults to
`IconButtonSize::Default` (24 dp) and stand-alone visual mode.
Apply `.embedded()` for the JetBrains "built-in" dim look,
and one of the size methods (`.large()` / `.toolbar()` /
`.hero()`) or `.size(...)` to pick a different size.

#### `pub fn style(mut self, style: impl teksilo_core::styles::IconButtonStyle) -> Self`

Per-call style override. Replaces the theme-wide default
`IconButtonStyle` for just this IconButton instance — same role
as `Button::style(...)`. The override fully owns the background +
border + size composition; icon coloring stays on the widget.

#### `pub fn style_shared(mut self, style: SharedIconButtonStyle) -> Self`

Per-call style override from an already-shared
`SharedIconButtonStyle` (`Rc<dyn IconButtonStyle>`). Same effect as
`style` but takes the erased handle directly, so a host
(e.g. a `Toolbar` applying one style to all its icon buttons) can share a
single `Rc` instead of cloning a concrete style per button.

#### `pub fn size_variant(&self) -> IconButtonSize`

Returns the configured size variant. Used by wrappers like
`PopoverIconButton`
that need to reason about the trigger's footprint at build time
(e.g. to skip a corner decoration that wouldn't fit at Compact).

#### `pub fn is_embedded(&self) -> bool`

Returns whether the button is in the JetBrains "built-in" /
embedded color profile (Secondary at rest). Mirror getter to
`size_variant` for wrappers that want to
derive their own chrome colors from the same icon role.

#### `pub fn share_interaction(mut self, signal: Signal<InteractionState>) -> Self`

Bind the button's internal interaction state to a caller-owned
`Signal<InteractionState>` instead of letting `build()` allocate
its own. Used by wrapper widgets like
`PopoverIconButton`
whose disclosure caret needs to match the icon's color across
hover / press / focus / disabled states.

The provided signal is reset to `Disabled` when `enabled == false`
during `build()` so the shared signal honors the button's
enabled state without the caller having to seed it.

#### `pub fn embedded(mut self) -> Self`

Opt into the **embedded** visual treatment — the JetBrains
"built-in button" look. Icon dims to `Secondary` at rest,
brightens to `Primary` on hover, flashes `Accent` on press —
designed to live inside another widget's trailing slot
(TextInput's clear-X, ComboBox's chevron) without competing
visually with the host's content. Default mode is stand-alone
(icon at full visual weight, `Primary` always).

#### `pub fn icon_role(mut self, role: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the icon's tint with a static `ColorProp`. When set,
the icon ignores `embedded` and the auto-derived idle/hover/press
role cascade — its color is bound directly to this prop instead.
Use for chrome whose host enforces a single text role across all
of its sub-widgets (e.g. tab-bar scroll arrows that must match
the tab strip's `idle_text_role` regardless of hover state).
Accepts `Color`, `TextRole`, `Signal<Color>`, or `Signal<TextRole>`.

It replaces the *interaction* cascade (idle / hover / press / focus),
**not** the disabled substitution: a role passed here still resolves to
`TextRole::Disabled` in a disabled subtree, like every other
role-derived color (see `ColorProp::resolve`).
That is what a disabled
control should look like. When the tint is semantic *state* that stays
true even though the button can't be pressed — a save/sync indicator, a
validation badge — wrap it: `.icon_role(ColorProp::undimmed(role))`.

#### `pub fn focusable(mut self, on: bool) -> Self`

Whether the button takes keyboard focus. Default `true` —
the button is focusable when enabled. Set to `false` for
embedded-control patterns where the parent owns focus and
keyboard interaction goes through the parent (e.g. the
close button inside a tab header — Tab moves between tabs,
not onto their close buttons).

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a tooltip that appears after a hover delay. Required —
the tooltip text doubles as the AT name for icon-only buttons.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip resolved from the app-wide tooltip
registry. See `Button::rich_tooltip`.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip driven by inline `TooltipContent`.

#### `pub fn composite_tooltip( mut self, content: impl teksilo_core::widget::Widget + 'static, ) -> Self`

Attach a composite tooltip — third tier, hosting an arbitrary
widget tree. See `Button::composite_tooltip`.

#### `pub fn composite_tooltip_boxed( mut self, content: Box<dyn teksilo_core::widget::Widget>, ) -> Self`

Attach a composite tooltip from an already-boxed widget — the boxed twin
of `composite_tooltip`, for hosts that build
the body via a `Fn() -> Box<dyn Widget>` factory (e.g. a `ToolbarAction`).

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Set the enabled state, statically or reactively. Disabled
buttons ignore input and dim their icon (handled by the
framework's `PaintContext::effective_enabled`). Forwarded into
the arena via `ctx.enabled_when(self_id, self.enabled.clone())`
at build time — a bound signal updates live as it changes.

For a reactive enabled state — e.g. a toolbar button that
enables only when the caret is inside a table — pass a
`Signal<bool>` here, or call `ctx.enabled_when(button_id,
my_signal)` from the composing widget's `build()` instead of
(or in addition to) this builder. Both routes write to the
same arena `enabled_state`; an external `enabled_when`
registered after this builder runs wins (last-write semantics)
and updates reactively from the signal.

#### `pub fn size(mut self, size: IconButtonSize) -> Self`

Set the size variant. Most callers prefer the named shortcuts
`large` / `toolbar` /
`hero`; use `.size(...)` for `Compact` or for
programmatic size selection.

#### `pub fn toolbar(mut self) -> Self`

Shortcut for `.size(IconButtonSize::Toolbar)` (30 dp) — the
IntelliJ side-toolbar density (left / right / top window edges).

#### `pub fn large(mut self) -> Self`

Shortcut for `.size(IconButtonSize::Large)` (40 dp) —
emphasized stand-alone buttons in rich menus and detail panes.

#### `pub fn hero(mut self) -> Self`

Shortcut for `.size(IconButtonSize::Hero)` (50 dp) — hero /
landing-screen CTAs.

#### `pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Closure invoked on activation. Fires after the toggle signal
(if any) is flipped, so apps observing the closure see the
post-flip state.

#### `pub fn toggle(mut self, state: Signal<bool>) -> Self`

Enable **surface-tint** bistate: clicking flips `state` and the
background reads as `SurfaceRole::Selected` while `state == true`.
The icon glyph is unchanged. Pin / select / lock-toggle pattern.
`on_activate_fn`, if any, still fires after the flip.

For the eye / eye-off pattern where the icon glyph also changes,
use `toggle_with_icon` instead.

#### `pub fn toggle_with_icon(mut self, state: Signal<bool>, toggled_icon: IconWidget) -> Self`

Enable **surface-tint plus icon-swap** bistate: clicking flips
`state`, the background flips to `Selected`, **and** the icon
swaps to `toggled_icon`. The visibility-toggle pattern (eye ↔
eye-off). For surface-only bistate (icon stays the same), use
`toggle`.

#### `pub fn has_popup(mut self, kind: teksilo_core::accesskit::HasPopup) -> Self`

Declare that this button is a disclosure trigger for a popup
(menu, dialog, listbox, …). Surfaced via `set_has_popup` in
the a11y node so screen readers announce it as opening the
named popup kind. Wired automatically by
`PopoverIconButton`.

#### `pub fn expanded_when(mut self, signal: impl Into<Prop<bool>>) -> Self`

Bind a signal reporting whether this button's popup is
currently visible. The popover wrapper owns the signal and
flips it on show / dismiss; IconButton reads it in
`accessibility()` to publish `set_expanded`. Only meaningful
alongside `has_popup`.

#### `pub fn browse() -> Self`

Browse button (ellipsis icon). Opens a file/directory chooser.

#### `pub fn expand() -> Self`

Expand button (diagonal resize arrows). Enlarges a constrained field.

#### `pub fn search() -> Self`

Search button (magnifier icon). Triggers a search.

#### `pub fn copy() -> Self`

Copy button (clipboard icon). Copies the field content.

#### `pub fn clear() -> Self`

Clear button (X icon). Clears the field content.

#### `pub fn add() -> Self`

Add button (plus icon). Adds a new entry.

#### `pub fn bell() -> Self`

Notification bell. Used by
`NotificationCenterButton`
— the bell-icon trigger that opens the notification log popover.

#### `pub fn menu() -> Self`

Menu / hamburger button (three horizontal bars). Used by the
collapsible `MenuBar` as the
collapsed representation that reveals the bar when activated.
Advertises `HasPopup::Menu` for assistive technology.

#### `pub fn more() -> Self`

"More actions" / overflow button — three **vertical** dots (the kebab
`⋮`). The conventional trigger for a per-item options menu (view-header
`…`, list-row overflow). Advertises `HasPopup::Menu` for assistive
technology. Pair with a `PopoverIconButton` + `MenuList` (use `.bare()`
so the menu isn't wrapped in a second popover surface).

#### `pub fn visibility_toggle(visible: Signal<bool>) -> Self`

Visibility toggle (eye / eye-off). Toggles password visibility.
Uses the icon-swap bistate mode internally — the icon advertises
the **expected action**, matching the prevailing password-field
convention (1Password, Bitwarden, KeePass, Chrome, GitHub):
`eye` (open) while the value is hidden, suggesting "click to
reveal"; `eye_off` (closed) once revealed, suggesting "click to
hide". `set_toggled` still reports the literal current state, so
AT readers are not misled.

For a current-state-instead semantics (icon shows what IS),
build your own with `toggle_with_icon`
and the eye glyphs in the opposite order.

The `visible` signal is flipped on each click. The host widget reads
it to decide whether to mask or show the text.

## `pub struct BuiltInIcons`

Icon factory set for predefined built-in buttons.

Each field is a function pointer that creates an `IconWidget`.
The default implementation uses SVG icons embedded in teksilo-widgets.

# Overriding

Call `BuiltInIcons::set_global` at app startup (before creating any
built-in buttons) to replace the default icon set:

```rust
# use teksilo_widgets::{BuiltInIcons};
# use teksilo_widgets::primitives::IconWidget;
# const MY_BROWSE_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg'/>";
# const MY_CLEAR_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg'/>";
BuiltInIcons::set_global(BuiltInIcons {
    browse: || IconWidget::from_svg(MY_BROWSE_SVG),
    clear: || IconWidget::from_svg(MY_CLEAR_SVG),
    ..BuiltInIcons::defaults()
});
```

```rust
pub struct BuiltInIcons { /* fields */ }
```

### Methods

#### `pub fn defaults() -> Self`

Return the default icon set (SVGs embedded in teksilo-widgets).

#### `pub fn set_global(icons: Self)`

Set the global icon set. Call at app startup before creating any
built-in buttons. Can only be set **once**: the global is a
process-wide `OnceLock`, so the first set wins and any later
call is ignored (and warns). It is also locked in the first time
`global()` reads it, so set it before any built-in
button is created. Use `defaults()` with struct
update syntax to override only specific icons.
