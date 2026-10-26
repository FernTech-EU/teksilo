<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# The application menu and the native menu bar

An application has **one application menu**, declared once, in the
application tree. On macOS it is the global menu bar at the top of the screen
(`NSApplication.mainMenu`) for the life of the process: while no window is
open, and whenever the key window has no menu of its own. On Windows and Linux
it is drawn in each window that asks for it, and in no other: nothing is
inserted into a window by itself. A window can show a menu of its own instead.

The design is a **single declarative model** consumed by two renderers:

- the in-window [`MenuBar`](../crates/teksilo-widgets/src/menu_bar.rs) widget, and
- a platform [`NativeMenuBackend`](../crates/teksilo-platform/src/native_menu.rs)
  (real `NSMenu` on macOS; a no-op everywhere else).

```
application tree                        a window
ctx.set_application_menu(model) ──┬─►  MenuBar::application()     (in-window dropdowns)
                                  └─►  NativeMenuSnapshot ─► NSMenu (macOS global bar)
                                       (plain data, crosses into teksilo-platform)
a window's own menu:  MenuBar::from_model(m) [.native_on_macos(..)]
```

## Quick start

```rust
use teksilo::prelude::*;   // brings ApplicationMenuExt
use teksilo::widgets::{MenuBar, MenuEntry, MenuModel, StandardMenu};

TeksiloAppBuilder::new()
    .install_native_menu()                         // the macOS menu bar
    .app_root(|tree| tree.add(AppRoot::new()))     // the application tree
    .initial_window(WindowConfig::new().root(|tree, _| tree.add(Document::new())))
    .run();

struct AppRoot {
    menu: MenuModel,                               // built once, kept
}

impl AppRoot {
    fn new() -> Self {
        let menu = MenuModel::new()
            .standard_menu(StandardMenu::app().title(tr!(app_name())).quit_intent("app.quit"))
            .menu(tr!(file()), |m| m
                .item(MenuEntry::new(tr!(new())).intent("app.new").shortcut("app.new")))
            .menu(tr!(edit()), |m| m
                .item(MenuEntry::new(tr!(copy())).intent("edit.copy").shortcut("edit.copy")))
            .standard_menu(StandardMenu::window());
        Self { menu }
    }
}

impl Widget for AppRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // The application's commands and their shortcuts.
        ctx.register_shortcut_global(
            Shortcut::new("app.new").primary(KeyStroke::ctrl(Key::N)).build(),
        );
        ctx.register_action_global(Action::new("app.new").on_invoke(|_, ctx| {
            ctx.open_window(document_window());
        }));
        ctx.register_action_global(Action::new("app.quit").on_invoke(|_, ctx| ctx.quit()));

        // The same handle at every build: declaring it again changes nothing.
        ctx.set_application_menu(self.menu.clone());
        vec![]
    }
    // layout_response: a zero size; the tree is never drawn.
}

// In a window's build: the application menu, in the window.
let bar = ctx.add(MenuBar::application());
```

`set_application_menu` is on `BuildContext` and `EventContext` (the
`ApplicationMenuExt` trait, in the prelude). Call it from the application tree;
called from a window's tree it does nothing, and a debug build panics. Calling
it again with another model replaces the menu everywhere: every bar showing it
rebuilds and the macOS menu is installed again. So build the model once, keep
the handle (a field of the application root, as above) and declare a clone of
it: the same handle declared again changes nothing. A model built inside
`build` is a new one at every rebuild of the root, and a debug build says so
when that keeps happening. Change the declared menu with its mutators
(`push_item`, `remove`, `modify`) or its entries' signals. `MenuBar::application()` with no
application menu declared shows no menu, and a debug build says so once.

Demo: `cargo run -p native-menu`.

## Where a row's command goes

Item activations route through the usual `Intent`/`Action` pipeline (with
`IntentSource::Menu`), so the **same** `Action::new("app.new")` fires whether the
item was chosen from the native menu, the in-window menu, or its keyboard
shortcut.

- **A row's intent** is sent from the focused widget of the window the user is
  working in: the window whose in-window bar was clicked, or for the native bar
  the window with the OS focus. An action on that widget or one of its
  ancestors takes it first (Edit ▸ Copy copies in the focused field), then the
  window's root and global actions, then the **application tree**, whose
  actions are the application's action scope (nothing else registers it). With
  no window focused, the native bar sends an application menu row's intent in
  the application tree, and a row of a window's own menu in that window.
  A click on the in-window menu bar leaves focus in the widget the user was
  working in (its titles take no focus from a click), and a menu opened from
  the keyboard (F10, Alt) hands focus back to that widget once a command is
  chosen, so the intent starts there.
- **A native bar row is input, as a key is.** While a dialog presented in the
  key window (a modal overlay: an in-window dialog, the command palette) is
  open, a native row's intent stops at the dialog, as its shortcut's would: the
  dialog's own actions and the global actions registered inside it take it,
  and what they leave goes to the application tree, never to the page behind
  the dialog. A button of the dialog that sends the same intent does walk on to
  the page; a row of the in-window menu bar cannot be chosen while the dialog
  is up. A native dialog window has a tree of its own, which the row's intent
  starts in. The App menu's Quit still asks the application to quit when
  nothing it can reach takes its intent.
- **A row's `on_activate` closure** runs where the state it captured lives: for
  the application menu, in the application tree, then the row's intent goes
  from the window's focus; for a window's own menu, in that window, anchored
  at its root.
- **A shortcut a row names** works in every window. Keys are resolved in the
  window they are pressed in, so each window gets a copy of the application
  tree's declaration of every shortcut the application menu names, unless the
  window registers that id itself, owned or shared (the commands its editors
  share, say); the window's menu rows and its
  `ShortcutSettings` show it. The copy sends the shortcut's intent by name (a
  shortcut built with `on_activate` sends the plain intent it is named after),
  and the intent takes the route above.

## Greyed-out rows

A row with an `.intent` and no `.on_activate` is **enabled only while something
on its route takes the intent**: an enabled action between the focused widget
and the window's root, a global action of the window, or one of the
application tree's, that has something to act on. Its own `.enabled` applies
on top. A row with a closure
always does something, and so does a two-way `.checkable`, `.tri_checkable` or
`.radio` row, which writes its state when chosen: only `.enabled` decides for
those, in the window as in the macOS menu bar.

- In a window, the row follows its route while its menu is open: focus, the
  actions and the application's change it in place. After F10 the menu answers
  for the widget focused before the bar, where its command goes, not for the
  bar's title.
- In the macOS menu bar the rows are brought up to date after every turn of the
  event loop, with the route a chosen row's intent would take, so a menu is
  current when the user opens it. That route starts at the key window's focused
  widget as it is: the menu bar is outside the window, so a field focused in an
  open popover is where Edit ▸ Copy goes, and what the row asks. The route is
  walked once a turn, asking the actions on it whether they pass a command on
  and whether they have anything to act on, so those checks run on every turn
  of the event loop
  (every frame while something animates) and must only read state; the rows
  are evaluated only when what it takes changed or a menu was installed since;
  where the backend shows no bar (every platform but macOS) nothing is
  refreshed. With no window open, a command that only
  windows implement is greyed out, and File ▸ New, which the application tree
  implements, is live. While a dialog presented in the key window is open, the
  route stops at the dialog (see "Where a row's command goes"): a row only the
  page behind it takes is greyed out, as AppKit disables a document's items
  while a modal panel is up, and a row the dialog or the application tree
  takes stays live.
- The text widgets say when an edit command has nothing to act on, as
  AppKit's text views do: in a `TextInputField` (and the fields built on it),
  a `RichTextEditor`, a `CodeEditor` or a `LogView` with focus, Edit ▸ Copy is
  greyed out while nothing is selected (a masked password counts as nothing),
  Cut also in a read-only one, Paste and Paste Unformatted in a read-only one,
  Select All in an empty one, Undo and Redo while there is nothing to undo or
  redo. A code editor and a log view copy the caret's line with nothing
  selected, so there Copy and Cut need text only. Paste does not look at the
  clipboard. This only greys rows: the shortcut still reaches the widget,
  which does what it did before (Ctrl+C with nothing selected copies nothing
  and goes no further).
- Any other action that may decline when it runs (`on_invoke_with_response`)
  counts as taking the intent, so its row stays enabled.

## A window's own menu

`MenuBar::from_model(m)` shows `m` in the window instead of the application
menu. With `.native_on_macos(mode)` it also replaces the application menu in
the macOS menu bar while that window is key:

- `Off` (default), in-window bar only; the native bar is untouched.
- `Suppress`, on macOS, mirror to the OS bar **and** hide the in-window strip
  (only its leading/trailing slots render). The native-looking choice.
- `Coexist`, mirror to the OS bar **and** keep the in-window strip too.

On non-macOS targets the flag is ignored and the in-window bar always renders
(the native backend is a no-op there). For `MenuBar::application()` the mode
only decides whether the in-window bar shows on macOS, where the application
menu is the global bar already: `Suppress`, its default, hides it when the app
installed a native menu, and the other two keep it.

The architecture is platform-neutral, so a Windows `HMENU` / Linux DBus
app-menu backend can be added later without touching the model or the widget.

## The model

`MenuModel` (in `teksilo-widgets`) is a cloneable handle (`Rc` inside) holding a
tree of `MenuNode`s with a `version: Signal<u64>`:

- `MenuModel::menu(title, |m| …)`, a top-level menu.
- `MenuModel::standard(role)`, a platform-standard menu (`StandardMenuRole::App`
  / `Window` / `Help`); rendered natively, ignored in-window.
- inside a menu: `MenuItems::item(MenuEntry)`, `.separator()`, `.submenu(title, |m| …)`.

`MenuEntry` is the leaf builder:

| method | effect |
| --- | --- |
| `.intent("app.x")` | fire intent by name on activation, from the focused widget; with no closure and no state to write (a two-way check or radio row writes one), the row greys out while choosing it would do nothing (nothing takes it, or what it would reach has nothing to act on) |
| `.on_activate(\|ctx\| …)` | run a closure, before the intent is dispatched (in the application tree for the application menu) |
| `.shortcut("app.x")` | display + bind the `ShortcutRegistry` chord |
| `.enabled(prop)` | static or `Signal<bool>`, greys out **reactively** (both surfaces) |
| `.visible(prop)` | static or `Signal<bool>`, hides **reactively** (both surfaces) |
| `.checkable(Signal<bool>)` | two-state check item, activation flips the signal (two-way) |
| `.checked(Signal<bool>)` | two-state check item, **reflect-only**, the checkmark mirrors the signal but activation does not write it; pair with `.intent`/`.on_activate` |
| `.tri_checkable(Signal<CheckState>)` | tri-state check item, activation moves it between unchecked and checked |
| `.radio(value, Signal<usize>)` | radio item within a group, activation writes `value` (two-way) |
| `.reflect_radio(value, Signal<usize>)` | radio item, **reflect-only**, the dot mirrors the signal but activation does not write it; pair with `.intent`/`.on_activate` |

A two-way item writes its signal when chosen, in the window and in the native
bar alike, before its `.on_activate` runs; the intent is dispatched after both.
Use a reflect-only item when the state belongs to something else that changes
it, a model or a setter that only acts on a real change: a two-way item would
write the signal first and leave that owner nothing to do.

Each `MenuEntry` is assigned a process-unique
[`MenuItemId`](../crates/teksilo-core/src/menu_item_id.rs), the token the native
backend round-trips on activation.

## Driving the menu from anywhere in the app

You never reach into the menu widget. Two channels reach it from any handler:

**Trigger a command**, fire the intent (no handle needed):

```rust
ctx.send_intent(Intent::new("app.save"));   // runs Action "app.save"
```

**Change per-item state**, bind a `Signal` to the entry and `.set()` it from
anywhere (keep the signal in `app_state` or a captured clone):

```rust
let can_save = Signal::new(false);
MenuEntry::new(lit!("&Save")).intent("app.save").enabled(can_save.clone());
// deep in the app:
can_save.set(true);     // greys in/out live, in-window AND native
```

`enabled` / `checkable` / `checked` / `tri_checkable` / `radio` /
`reflect_radio` / `visible` are all signal-driven and update without a rebuild (the native path observes the signal
and calls `update_item`; the in-window path binds it directly).

A **derived** signal works here too, `enabled(unsaved.and(&backup_mode.not()))`
is a normal binding, not a special case: `Signal::observe` registers on the
mutable roots a derived signal was built from. The one shape it cannot follow is
`flat_map`, whose inner signal is re-selected as it is read; such a row keeps the
state it was installed with in the *global* bar and does not follow later
changes. The in-window menu binds it directly and stays live either way.

## Dynamic structure (add / remove items at runtime)

`MenuModel` is a cloneable handle with `&self` mutators. Each bumps `version`;
a `from_model` or `application` bar binds `version` at `Rebuild` level, so the
in-window dropdowns re-derive, and the native menu re-installs automatically.
Keep a clone of the application menu's model to change it from a window's
handler with its mutators; declaring another model is done with
`set_application_menu` from the application tree (a window's call does
nothing).

```rust
// Pre-allocate an id so you can address a submenu later:
let recent = MenuItemId::next();
let model = MenuModel::new()
    .menu(tr!(file()), |m| m.submenu_with_id(recent, tr!(open_recent()), |s| s));

// ...anywhere later (hold a clone of `model`):
let entry = MenuEntry::new(lit!("doc.txt")).on_activate(|_| open());
let id = entry.id();                           // MenuEntry::new allocates the id
model.push_item(recent, entry);                // returns true if `recent` was found
model.remove(id);                              // remove any item/submenu by id
let edit = model.push_menu(tr!(edit()), |m| m.item(...));  // add a top-level menu
model.modify(|nodes| { /* full control: reorder, retitle, … */ });
```

| method | effect |
| --- | --- |
| `push_item(into, entry) -> bool` | append an item to the submenu with id `into`; `false` if not found |
| `push_separator(into) -> bool` | append a separator; `false` if not found |
| `push_menu(title, \|m\| …) -> MenuItemId` | add a top-level menu |
| `remove(id) -> bool` | remove an item or submenu anywhere |
| `modify(\|&mut Vec<MenuNode>\| …)` | arbitrary structural edit (escape hatch) |

`menu_with_id` / `submenu_with_id` let you assign ids up front so submenus are
addressable.

## Reactivity summary

- **Per-item** `enabled` / `visible` / check / radio, live, no rebuild
  (native: `update_item`; in-window: direct signal binding / `item_when`).
- **Structural** add/remove, `version` bump → automatic rebuild + native
  re-install.
- **A user's rebind** of a shortcut the application menu names applies to the
  application: every window's in-window rows and the native application menu
  show the new chord after the turn it was made in (see
  [Shortcuts and key equivalents](#shortcuts-and-key-equivalents)).
- **Locale**: native titles follow a locale change, and a reload of the
  translations. A row's title is updated in place; the menus' titles and the
  App menu's labels, which have nothing to update in place, by installing the
  menu again: the application menu at the end of the turn, a window's own
  `from_model` menu as its bar is rebuilt for the change.
- **A rebind of a key equivalent** in a window's own `from_model` menu
  applies at once: the native bar installs again when the user rebinds a
  shortcut one of its rows names (the in-window bar reflects it
  immediately too).

## Shortcuts and key equivalents

`.shortcut("id")` resolves the chord from the `ShortcutRegistry`. On the native
menu it becomes an `NSMenuItem` key equivalent, so **AppKit fires the item
directly**, the keystroke never reaches the widget tree, so there is no
double-fire with the in-app shortcut dispatcher.

Modifier mapping follows the cross-platform convention (as in Qt's `Qt::CTRL`):
a declared `Ctrl` is the primary accelerator modifier and resolves to ⌘ on
macOS, so `KeyStroke::ctrl(Key::S)` shows as ⌘S. `Alt`→⌥, `Shift`→⇧.

The rewriting happens once, in the registry, see
[Shortcuts, Intents and Actions](shortcut-intent-action.md#ctrl-means--on-macos)
,  so the menu row and the in-app dispatcher advertise and fire the *same* chord.
A shortcut declared `literal_modifiers()`, or a chord the user rebound to
physical ⌃, arrives here untouched and gets ⌃ as its key equivalent.

**A user's rebind applies to the application.** A shortcut the application menu
names is the application tree's, and each window holds a copy of it; the
window's `ShortcutSettings` lists the copy and rebinds it in the window's
registry. After that turn `teksilo-app` makes the same rebind (or reset) in the
application tree's registry, gives every window's copy the application tree's
binding, and installs the native application menu again with the new key
equivalent. An app that persists the user's bindings
(`ShortcutRegistry::export_overrides` / `import_overrides`) saves and loads
those of the application menu's shortcuts on the application tree's registry;
the windows receive them from it. A shortcut a window registers itself, owned
or shared, keeps its rebind in that window.

**A window's own registration of the id replaces the copy in the whole
window**, even one scoped to a widget (`register_shortcut`): outside that
widget the chord then does nothing in that window. Register the window's
command as a global shortcut, or leave the id to the application tree and take
the intent with an action, which the window's focus reaches first.

## What macOS shows

There is one global menu bar on macOS. `teksilo-app` installs the application
menu from the application tree (`NativeMenuHandle::set_app_menu`), again
whenever it changes, and calls `activate_window` on `WindowEvent::Focused`:

| The key window | The menu bar shows |
| --- | --- |
| has a menu of its own (`MenuBar::from_model(..).native_on_macos(Suppress or Coexist)`) | that menu |
| a modal dialog with no menu of its own | the menu of the window it blocks, or of that window's owner, up the chain, as AppKit keeps the bar for a sheet or a panel |
| shows the application menu (`MenuBar::application()`), or has no menu bar and no owner with one (a secondary window) | the application menu; with none declared, the bar shown before stays, until one is declared, which then shows at once |
| none: no window is open (`QuitPolicy::Explicit`) | the application menu, whole: File ▸ New and Open live, window commands greyed out |

A window's own menu never replaces the application menu itself: what the bar
shows with no window open does not depend on which window was open last. A
window's model that declares no App menu borrows the application menu's, and
one that declares no Window or Help menu borrows the application menu's
standard Window and Help menus, so Minimize, Zoom and the window list are in
every window's bar, as in any Mac app. Only those standard menus are
borrowed: rows of the application's own under the same title (an ordinary
`.menu` titled Window) are not, since their actions run in the application
tree. The window's menu follows what it borrows: it is installed again
whenever the application menu is declared again or changes in place. An
application that declares no application menu still gets an App menu, whose
Quit asks the application to quit.

An item of a window's own menu acts in two places. Its `.on_activate` closure,
and the state write a check or radio entry makes, run in the window whose menu
it is, where the state they captured lives, anchored at that window's root (an
overlay it shows belongs to the window, not to a popover that happens to hold
focus). Its intent is then sent from the focused widget of the window that has
the OS focus, as the item's shortcut would be. An item of the application menu
runs its closure in the application tree and sends its intent from the focused
window's focus, or in the application tree when no window has it.

## Standard macOS menus

The App / Window / Help menus carry **system selectors** (About, Services,
Hide, Hide Others, Show All; Minimize, Zoom, the live window list) and the
routed Quit and Settings rows, but their **labels go through i18n** like every
other widget, the platform layer never hardcodes English. Declare them with
localized strings:

```rust
use teksilo::widgets::StandardMenu;

MenuModel::new()
    .standard_menu(StandardMenu::app()
        .title(tr!(app_name()))       // bold app-name submenu
        .about(tr!(about()))
        .settings(tr!(settings()))    // "Settings…", needs settings_intent too
        .settings_intent("app.settings")
        .services(tr!(services()))
        .hide(tr!(hide()))            // "Hide <App>"
        .hide_others(tr!(hide_others()))
        .show_all(tr!(show_all()))
        .quit(tr!(quit())))           // e.g. "Quitter" on a French system
    .menu(tr!(file()), |m| …)
    .standard_menu(StandardMenu::window());   // Minimize / Zoom + window list
```

- `StandardMenu::{app, window, help}` give English `lit!` defaults; pass `tr!`
  to localize. `.standard(role)` is sugar for the all-default menu.
- A default **App** menu is auto-injected as the leading menu if the model
  declares none (so ⌘Q always works), labels resolved through the widget layer,
  not the platform crate. A window's own model borrows the application menu's
  App menu instead, when there is one, and its standard Window and Help menus
  when it declares none of its own.
- The **App** menu is first in the macOS bar wherever the model declares it.
  AppKit shows the bar's first menu as the application menu, under the
  application's name, whatever it holds; declared after File, it would have
  put File's rows there. The in-window bar ignores standard menus, so the
  declared order is the order of the other menus on both surfaces. A bar has
  one App menu: a second one in the same model is left out, and a debug build
  says so once, on every platform, when the model is declared or shown.
- **App** has AppKit's rows in AppKit's order: About; Settings…, in a group
  of its own, when routed (below); the Services submenu, which macOS fills
  with the services that apply to the selection; Hide (⌘H, `hide:`), Hide
  Others (⌥⌘H, `hideOtherApplications:`) and Show All
  (`unhideAllApplications:`); Quit (below).
- **Window** adds Minimize (⌘M, `performMiniaturize:`) + Zoom (`performZoom:`)
  and registers the menu with AppKit so the live window list appears (see
  below). macOS adds rows of its own for arranging windows (Fill, Center,
  Move & Resize and the like). It adds the tab rows (Show Previous Tab, Merge
  All Windows and the rest) only when the application lets it merge windows
  into tabs, `TeksiloAppBuilder::window_tabbing(true)`; that is off by
  default.
- **Help** is a localized titled submenu registered as the help menu.
- **Your own Window or Help rows** go in an ordinary `.menu(...)` with the same
  title as the standard menu. On macOS its rows are shown inside the standard
  menu, below Zoom in Window, instead of as a second menu of that name; the
  in-window bar shows it as an ordinary menu. The titles are compared without
  their `&` marker, so `.menu(tr!(window_menu()), …)` folds into
  `StandardMenu::window().title(tr!(window_menu()))`.

### The Window and Dock menus list the windows

AppKit keeps the list of the application's windows in one menu, the one
registered as `NSApp.windowsMenu`, and the Dock icon's menu lists the same
windows. Whenever a bar becomes the menu bar, the backend registers its
**Window** menu (the first `StandardMenu::window()` it declares), or, when it
declares none, a hidden one it adds to the bar, as Qt does, so the Dock icon's
menu still lists the windows. It then has AppKit list every open window there,
the window opened at startup included, on screen or minimised to the Dock; a
window the app hid stays out.

Every titled window is listed, except a modal dialog and a window opened with
`WindowConfig::skip_taskbar` (a tool palette): like AppKit's sheets and panels,
a dialog comes forward with the window it blocks, and a palette is not a window
the user switches to, so neither has a row of its own.

### Settings…

macOS keeps app settings in the application menu, under About, on ⌘,, and
neither the placement nor the chord is reachable from an ordinary `MenuEntry`,
since the App menu is filled in by the platform. `StandardMenu::settings_intent`
puts the row there:

```rust
StandardMenu::app()
    .settings(tr!(settings()))            // "Settings…" (macOS 13+; "Preferences…" before)
    .settings_intent("app.settings")      // same intent the in-window command fires
    .settings_shortcut("app.settings")    // …and the same registered chord
```

Unlike Quit there is no system fallback, no platform opens an arbitrary app's
settings on its own, so leaving `settings_intent` unset omits the row rather
than rendering one that does nothing. Route it to the same intent your in-window
Settings command uses and the two stay one command.

### Quit

The App menu's **Quit** asks the application to quit, as
`EventContext::quit` does: the application's quit guard
(`TeksiloAppBuilder::on_quit_requested`) is asked first, then every window's
close guard in turn, any of which can refuse or defer its answer
(`CloseResponse::Defer`), and the application ends once no window is left.
See "Quitting the application" in the [multi-window guide](multi-window.md).
The row is the application's own command, not AppKit's `terminate:`, so it
can carry the app's quit intent (below). A `terminate:` sent from elsewhere
(the Dock menu's Quit, AppleScript's `quit`, a logout) asks the same quit:
AppKit asks the application before it ends, and the quit's outcome is its
answer (see "When the operating system asks" in the multi-window guide).

An in-app ⌘Q shortcut does **not** replace it. AppKit dispatches main-menu key
equivalents *before* the responder chain, so the App menu's item wins and the
app's own shortcut never sees the keystroke. The same is true of a Quit row the
app puts in its own File menu.

An app that must do more than its guards do before exiting (a session to
flush, a job to stop) routes the item to its own command:

```rust
MenuModel::new().standard_menu(
    StandardMenu::app()
        .title(tr!(app_name()))
        .quit(tr!(quit()))
        .quit_intent("app.quit")      // the app's own guarded action
        .quit_shortcut("app.quit"),   // …advertised at the chord the registry holds
);
```

Quit then sends that intent, same ⌘Q, same `Intent`/`Action` pipeline as every
other menu item, `IntentSource::Menu`, and **the action that takes it owns the
exit**: nothing quits on its behalf. When nothing takes the intent (no window is
open and only windows handle it, say), Quit asks the application to quit as
above, so ⌘Q never goes dead.

**Register an app-wide Quit, or New, in the application tree.** The intent goes
from the focused window's focus, then to the application tree, never to another
window. A quit action only some windows register runs when one of them is key;
with another window key (a palette, a document window that lacks it), Quit
finds nothing on its route and asks the application to quit directly, past
that action. Registered in the application tree with `register_action_global`,
it runs whichever window is key, and with none open.

### Chords on the two routed rows

Quit and Settings are the only rows the platform places for you, so they are the
only ones that cannot carry a `MenuEntry::shortcut`. Name the registered
shortcut with `quit_shortcut` / `settings_shortcut` instead, and the chord is
resolved from the `ShortcutRegistry` exactly as every other row's is, same
primary-accelerator convention. In the application menu that is the
application tree's registry, which holds a user's rebind made in any window;
in a window's own menu, that window's.

Unset, the row falls back to the platform convention (⌘Q, ⌘,), right for an app
that registered no such shortcut, and wrong the moment one exists: a hardcoded
⌘Q stays live after the user moves Quit elsewhere *and* shadows wherever they
moved it to, because the platform dispatches a main-menu key equivalent before
the responder chain. Naming a shortcut that currently resolves to nothing leaves
the row with no key equivalent rather than resurrecting the convention, the app
said where the chord comes from, and it says none right now.

## Architecture

| layer | type | crate |
| --- | --- | --- |
| id token | `MenuItemId` | `teksilo-core` |
| rich model | `MenuModel` / `MenuEntry` / `MenuItemState` | `teksilo-widgets` |
| application menu holder | `ApplicationMenuExt::set_application_menu` (the holder itself is internal app-state) | `teksilo-widgets` |
| model → native bridge | `menu::native::install` / `install_application` (+ reactive observers) | `teksilo-widgets` |
| boundary data | `NativeMenuSnapshot` / `NativeMenuNode` / `MenuItemDelta` | `teksilo-platform` |
| trait + handle | `NativeMenuBackend` / `NativeMenuHandle` | `teksilo-platform` |
| macOS impl | `NSMenu` builder + `TeksiloMenuTarget` | `teksilo-platform/native_menu/macos.rs` |
| app wiring | `install_native_menu`, the application menu's sync (native menu, the windows' shortcut copies), payload router, Quit fallback, availability refresh | `teksilo-app` |

The platform boundary type is plain, already-resolved data, `teksilo-platform`
never sees the widgets model. The macOS item callback posts a
`NativeMenuEventPayload` (a window's own menu) or a `NativeAppMenuEventPayload`
(the application menu) through `AppEventPoster::post_external`, routed back into
the tree whose menu it is (the action) and the focused window (the intent),
like the file-dialog / external-DnD paths.
