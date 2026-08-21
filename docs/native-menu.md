<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Native (OS) Menu Bar

Teksilo can mirror a menu into the platform's **native** menu surface — the
global menu bar at the top of the screen on macOS (`NSApplication.mainMenu`). A
serious desktop app is expected to present its menus this way on macOS; an
in-window strip alone reads as non-native.

The design is a **single declarative model** consumed by two renderers:

- the in-window [`MenuBar`](../crates/teksilo-widgets/src/menu_bar.rs) widget, and
- a platform [`NativeMenuBackend`](../crates/teksilo-platform/src/native_menu.rs)
  (real `NSMenu` on macOS; a no-op everywhere else).

```
MenuModel  ──┬─►  MenuBar::from_model(..)              (in-window dropdowns)
 (widgets)   └─►  NativeMenuSnapshot ─► NSMenu          (macOS global bar)
                  (plain data, crosses into teksilo-platform)
```

## Quick start

```rust
use teksilo::widgets::{MenuBar, MenuModel, MenuEntry, NativeMenuMode};

// 1. Install the native-menu service on the app.
TeksiloAppBuilder::new()
    .install_native_menu()
    .initial_window(WindowConfig::new().root(|tree, _| tree.add(Root::new())))
    .run();

// 2. Build one model, render it both ways.
let model = MenuModel::new()
    .menu(tr!(file()), |m| m
        .item(MenuEntry::new(tr!(new())).intent("app.new").shortcut("app.new"))
        .separator()
        .item(MenuEntry::new(tr!(quit())).intent("app.quit").shortcut("app.quit")))
    .menu(tr!(view()), |m| m
        .item(MenuEntry::new(tr!(show_grid())).checkable(grid_visible.clone())));

let bar = ctx.add(
    MenuBar::from_model(model).native_on_macos(NativeMenuMode::Suppress),
);
```

Item activations route through the usual `Intent`/`Action` pipeline (with
`IntentSource::Menu`), so the **same** `Action::new("app.new")` fires whether the
item was chosen from the native menu, the in-window menu, or its keyboard
shortcut.

Demo: `cargo run -p native-menu`.

## The model

`MenuModel` (in `teksilo-widgets`) is a cloneable handle (`Rc` inside) holding a
tree of `MenuNode`s with a `version: Signal<u64>`:

- `MenuModel::menu(title, |m| …)` — a top-level menu.
- `MenuModel::standard(role)` — a platform-standard menu (`StandardMenuRole::App`
  / `Window` / `Help`); rendered natively, ignored in-window.
- inside a menu: `MenuItems::item(MenuEntry)`, `.separator()`, `.submenu(title, |m| …)`.

`MenuEntry` is the leaf builder:

| method | effect |
| --- | --- |
| `.intent("app.x")` | fire intent by name on activation |
| `.on_activate(\|ctx\| …)` | run a closure (after the intent) |
| `.shortcut("app.x")` | display + bind the `ShortcutRegistry` chord |
| `.enabled(prop)` | static or `Signal<bool>` — greys out **reactively** (both surfaces) |
| `.visible(prop)` | static or `Signal<bool>` — hide reactively in-window (native: omitted at build) |
| `.checkable(Signal<bool>)` | two-state check item |
| `.tri_checkable(Signal<CheckState>)` | tri-state check item |
| `.radio(value, Signal<usize>)` | radio item within a group |

Each `MenuEntry` is assigned a process-unique
[`MenuItemId`](../crates/teksilo-core/src/menu_item_id.rs) — the token the native
backend round-trips on activation.

## `NativeMenuMode` (the macOS flag)

`MenuBar::native_on_macos(mode)`:

- `Off` (default) — in-window bar only; the native bar is untouched.
- `Suppress` — on macOS, mirror to the OS bar **and** hide the in-window strip
  (only its leading/trailing slots render). The native-looking choice.
- `Coexist` — mirror to the OS bar **and** keep the in-window strip too.

On non-macOS targets the flag is ignored and the in-window bar always renders
(the native backend is a no-op there). The architecture is platform-neutral, so
a Windows `HMENU` / Linux DBus app-menu backend can be added later without
touching the model or the widget.

## Driving the menu from anywhere in the app

You never reach into the menu widget. Two channels reach it from any handler:

**Trigger a command** — fire the intent (no handle needed):

```rust
ctx.send_intent(Intent::new("app.save"));   // runs Action "app.save"
```

**Change per-item state** — bind a `Signal` to the entry and `.set()` it from
anywhere (keep the signal in `app_state` or a captured clone):

```rust
let can_save = Signal::new(false);
MenuEntry::new(lit!("&Save")).intent("app.save").enabled(can_save.clone());
// deep in the app:
can_save.set(true);     // greys in/out live, in-window AND native
```

`enabled` / `checkable` / `tri_checkable` / `radio` / `visible` are all
signal-driven and update without a rebuild (the native path observes the signal
and calls `update_item`; the in-window path binds it directly).

## Dynamic structure (add / remove items at runtime)

`MenuModel` is a cloneable handle with `&self` mutators. Each bumps `version`;
a `from_model` bar binds `version` at `Rebuild` level, so the in-window dropdowns
re-derive and the native menu re-installs automatically.

```rust
// Pre-allocate an id so you can address a submenu later:
let recent = MenuItemId::next();
let model = MenuModel::new()
    .menu(tr!(file()), |m| m.submenu_with_id(recent, tr!(open_recent()), |s| s));

// ...anywhere later (hold a clone of `model`):
let id = model.push_item(recent, MenuEntry::new(lit!("doc.txt")).on_activate(|_| open()));
model.remove(id);                              // remove any item/submenu by id
let edit = model.push_menu(tr!(edit()), |m| m.item(...));  // add a top-level menu
model.modify(|nodes| { /* full control: reorder, retitle, … */ });
```

| method | effect |
| --- | --- |
| `push_item(into, entry)` | append an item to the submenu with id `into` |
| `push_separator(into)` | append a separator |
| `push_menu(title, \|m\| …) -> MenuItemId` | add a top-level menu |
| `remove(id) -> bool` | remove an item or submenu anywhere |
| `modify(\|&mut Vec<MenuNode>\| …)` | arbitrary structural edit (escape hatch) |

`menu_with_id` / `submenu_with_id` let you assign ids up front so submenus are
addressable.

## Reactivity summary

- **Per-item** `enabled` / `visible` / check / radio — live, no rebuild
  (native: `update_item`; in-window: direct signal binding / `item_when`).
- **Structural** add/remove — `version` bump → automatic rebuild + native
  re-install.
- **Locale / shortcut-rebind** of native *titles / key equivalents* — re-resolved
  on the next rebuild or window re-focus (the in-window bar reflects them
  immediately). Trigger a refresh sooner by touching the model (e.g. any mutator)
  if needed.

## Shortcuts and key equivalents

`.shortcut("id")` resolves the chord from the `ShortcutRegistry`. On the native
menu it becomes an `NSMenuItem` key equivalent, so **AppKit fires the item
directly** — the keystroke never reaches the widget tree, so there is no
double-fire with the in-app shortcut dispatcher.

Modifier mapping follows the cross-platform convention (as in Qt's `Qt::CTRL`):
the primary accelerator modifier — `Ctrl` *or* `Super` in a Teksilo shortcut —
maps to ⌘ on macOS. So `KeyStroke::ctrl(Key::S)` shows as ⌘S. `Alt`→⌥, `Shift`→⇧.

## Multi-window

There is one global menu bar on macOS; it follows the **focused** window. Each
window installs its own snapshot (`NativeMenuHandle::set_window_menu`), and
`teksilo-app` calls `activate_window` on `WindowEvent::Focused` so the focused
window's menu becomes `mainMenu`. The menu + its activation map are dropped when
the window closes.

## Standard macOS menus

The App / Window / Help menus carry **system selectors** (About / Hide / Quit,
Minimize / Zoom, the live window list) but their **labels go through i18n** like
every other widget — the platform layer never hardcodes English. Declare them
with localized strings:

```rust
use teksilo::widgets::StandardMenu;

MenuModel::new()
    .standard_menu(StandardMenu::app()
        .title(tr!(app_name()))       // bold app-name submenu
        .about(tr!(about()))
        .hide(tr!(hide()))
        .quit(tr!(quit())))           // e.g. "Quitter" on a French system
    .menu(tr!(file()), |m| …)
    .standard_menu(StandardMenu::window());   // Minimize / Zoom + window list
```

- `StandardMenu::{app, window, help}` give English `lit!` defaults; pass `tr!`
  to localize. `.standard(role)` is sugar for the all-default menu.
- A default **App** menu is auto-injected as the leading menu if the model
  declares none (so ⌘Q always works) — labels resolved through the widget layer,
  not the platform crate.
- **Window** adds Minimize (⌘M, `performMiniaturize:`) + Zoom (`performZoom:`)
  and registers the menu with AppKit so the live window list appears.
- **Help** is a localized titled submenu registered as the help menu. Custom Help
  items beyond that are best declared as a normal `.menu(...)`.

### ⚠ Quit, and apps with something to lose

The App menu's **Quit** is bound to AppKit's `terminate:` by default. That is
what makes ⌘Q work with no wiring at all — but `terminate:` exits the process
directly: it does not run winit's exit path, so no `LoopExiting` hook, no
close guard, nothing the app registered.

An in-app ⌘Q shortcut does **not** save you. AppKit dispatches main-menu key
equivalents *before* the responder chain, so the App menu's item wins and the
app's own shortcut never sees the keystroke — the app looks wired up and is not.
The same is true of a Quit row the app puts in its own File menu.

So an app that must ask before exiting — unsaved work to confirm, a session to
flush, a job to stop — routes the item instead:

```rust
MenuModel::new().standard_menu(
    StandardMenu::app()
        .title(tr!(app_name()))
        .quit(tr!(quit()))
        .quit_intent("app.quit"),   // the app's own guarded action
);
```

Quit then becomes an ordinary routed item — same ⌘Q, same
`Intent`/`Action` pipeline as every other menu item, `IntentSource::Menu` — and
**the app owns the exit from that point on**: nothing terminates on its behalf.
Leave `quit_intent` unset and the platform behaviour is unchanged, which is also
what the auto-injected default App menu uses (a model that declares no App menu
has declared no quit handler to route to either).

## Architecture

| layer | type | crate |
| --- | --- | --- |
| id token | `MenuItemId` | `teksilo-core` |
| rich model | `MenuModel` / `MenuEntry` / `MenuItemState` | `teksilo-widgets` |
| model → native bridge | `menu::native::install` (+ reactive observers) | `teksilo-widgets` |
| boundary data | `NativeMenuSnapshot` / `NativeMenuNode` / `MenuItemDelta` | `teksilo-platform` |
| trait + handle | `NativeMenuBackend` / `NativeMenuHandle` | `teksilo-platform` |
| macOS impl | `NSMenu` builder + `TeksiloMenuTarget` | `teksilo-platform/native_menu/macos.rs` |
| app wiring | `install_native_menu`, payload router, focus arbitration | `teksilo-app` |

The platform boundary type is plain, already-resolved data — `teksilo-platform`
never sees the widgets model. The macOS item callback posts a
`NativeMenuEventPayload` through `AppEventPoster::post_external`, routed back into
the originating window's tree exactly like the file-dialog / external-DnD paths.
