<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# MenuBar

MenuBar — a horizontal application menu bar with keyboard-driven dropdowns.

`MenuBar` renders a row of labelled trigger buttons; activating one opens a
dropdown `MenuList` as an overlay. Menus can be added via the fluent
`.menu(label, factory)` API or built from a declarative `MenuModel`
(the single source of truth shared with the native macOS menu bar via
`from_model` + `native_on_macos`). Leading and trailing slots accept
arbitrary widget content (an app icon or a search field, for example).

**Keyboard.** F10 and bare-Alt-tap focus the first trigger without opening
a menu; Alt+letter opens the menu whose label carries a matching mnemonic
marker (`&File` → Alt+F). On macOS the Alt+letter branch is suppressed
because the OS rewrites Option+letter for accented character composition —
F10 and bare-Alt-tap continue to work. Once a dropdown is open, ArrowLeft
and ArrowRight cycle between top-level menus, and Escape closes the active
one and returns focus to the trigger.

**Hamburger / collapsible mode.** Call `.collapsible()` to let the bar
collapse to a single hamburger `IconButton` when its intrinsic width
exceeds the allotted space (`CollapsePolicy::Responsive`). `.collapse_policy(Always)`
forces the hamburger regardless of width.

## Accessibility

The bar carries `Role::MenuBar`; each trigger is `Role::MenuItem` with
`set_has_popup(Menu)` and `set_expanded` tracking the open dropdown.
Mnemonic letters are announced via `set_access_key` for Windows Narrator.

```rust
# use teksilo_widgets::{MenuBar, MenuList, MenuItem};
# use teksilo_i18n::lit;
# use teksilo_core::Intent;
let _w = MenuBar::new()
    .menu(lit!("File"), || Box::new(
        MenuList::new()
            .item(MenuItem::new(lit!("New")).on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.new"))))
            .separator()
            .item(MenuItem::new(lit!("Quit")).on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.quit"))))
    ))
    .menu(lit!("Edit"), || Box::new(
        MenuList::new()
            .item(MenuItem::new(lit!("Cut")).on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.cut"))))
    ));
```

## Builder methods at a glance

`from_model`, `native_on_macos`, `collapsible`, `collapsed_signal`, `collapse_policy`, `hamburger_size`, `is_collapsed`, `no_dispatcher_install`, `menu`, `leading_slot`, `trailing_slot`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/menu_bar/index.html)

## `pub enum CollapsePolicy`

Controls when a collapsible `MenuBar` switches from the full inline bar
to the hamburger `IconButton` representation.

```rust
pub enum CollapsePolicy { /* variants */ }
```

### Variants

- **`Responsive`** — Collapse to a hamburger only when the bar's intrinsic width exceeds the width it is allotted; otherwise show the full inline bar. Mirrors the responsive `Toolbar` overflow behaviour.
- **`Always`** — Always show the hamburger, regardless of available width. The "force hamburger" / compact mode.

## `pub struct MenuBar`

A horizontal application menu bar with labelled trigger buttons and dropdown menus.

Each top-level entry becomes a focusable trigger; activating it opens a
floating `MenuList` overlay. See the module documentation for the full
keyboard, mnemonic, and collapsible-mode details.

```rust
pub struct MenuBar { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create an empty menu bar with no menus, slots, or collapse policy.

#### `pub fn from_model(model: crate::menu::MenuModel) -> Self`

Build a menu bar from a declarative `MenuModel`
— the single source of truth shared with the native OS menu bar. Each
top-level menu in the model becomes an in-window dropdown; combine with
`native_on_macos` to also mirror it into the
macOS system menu bar.

#### `pub fn native_on_macos(mut self, mode: crate::menu::NativeMenuMode) -> Self`

Choose how this bar behaves on macOS, where the convention is a global
menu bar at the top of the screen. Requires the bar to have been built
with `from_model` and the app to have called
`install_native_menu()`. No effect on other platforms (the in-window bar
renders there regardless).

#### `pub fn collapsible(mut self) -> Self`

Enable the optional **hamburger** representation. When there
isn't room for the full inline bar, it collapses to a single
hamburger (☰) `IconButton`; activating it (click, `Alt`+
mnemonic, `F10`, or bare-`Alt`-tap) reveals the full bar as a
floating overlay over content. Clicking outside the bar or
pressing `Escape` hides it again.

Uses `CollapsePolicy::Responsive`. Observe the collapsed state
via `is_collapsed`, or bind your own signal
with `collapsed_signal`.

#### `pub fn collapsed_signal(mut self, collapsed: Signal<bool>) -> Self`

Like `collapsible`, but uses the supplied
signal as the collapsed-state source so the application can
observe (and react to) collapse transitions. The responsive
decision **writes** this signal (it is not a plain read-only
input) — kept as a `Signal<bool>` rather than `Prop<bool>` since a
static value would have nowhere to receive those writes.

#### `pub fn collapse_policy(mut self, policy: CollapsePolicy) -> Self`

Set the collapse policy (and enable collapsible mode).
`CollapsePolicy::Always` forces the hamburger regardless of
available width — i.e. **collapsed by default**.

#### `pub fn hamburger_size(mut self, size: IconButtonSize) -> Self`

Set the size variant of the collapsed-mode hamburger
`IconButton`. Mirrors `IconButton::size` — pick
`IconButtonSize::Toolbar`, `IconButtonSize::Large`,
`IconButtonSize::Hero`, etc. so the hamburger matches the
surrounding chrome. Defaults to `IconButtonSize::Default`.

#### `pub fn is_collapsed(&self) -> Signal<bool>`

A clone of the collapsed-state signal (`true` while the
hamburger is shown). Call after `collapsible`.

#### `pub fn no_dispatcher_install(mut self) -> Self`

Skip the window-state dispatcher install. The MenuBar still
renders, intercepts mouse clicks, and supports keyboard
navigation when its triggers have focus — only F10 /
Alt+letter / Alt-tap routing through the window-level slot is
disabled. Use this for demo / showcase MenuBars that share a
window with a primary functional MenuBar — the slot is
single-occupancy and a second install would `debug_assert!`.

#### `pub fn menu( mut self, label: impl Into<LocalizedString>, factory: impl Fn() -> Box<dyn Widget> + 'static, ) -> Self`

Add a top-level menu entry. `label` is the trigger text (supports `&`
mnemonic markers, e.g. `"&File"`); `factory` is called each build to
produce the dropdown content — typically a `MenuList`.

#### `pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self`

Add content before the menu buttons (e.g. an app icon). Call more than
once to stack several.

Takes the widget by value, like every other widget's slot. MenuBar
builds it once and reuses it across rebuilds (it
`preserves_children_on_rebuild`),
so the slot — and any state it holds — survives a theme / locale /
model-version rebuild.

#### `pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self`

Add content after the menu buttons (e.g. a search box or avatar).
Like `leading_slot`, taken by value and preserved
across rebuilds.
