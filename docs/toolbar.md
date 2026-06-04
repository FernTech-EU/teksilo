# Toolbar Reference

`bastyde_widgets::toolbar` ships [`Toolbar`](../crates/bastyde-widgets/src/toolbar.rs)
— a command bar with **automatic overflow**: excess commands collapse into a
trailing chevron (`⌄`) that opens a drop-down menu, mirroring Qt's `QToolBar`
extension button, macOS `NSToolbar`'s overflow menu, and WinUI `CommandBar`.

Mental model in one line:

```
Toolbar::new().action(…).item(ToolbarItem::custom(…).overflow_widget(…)) → it fits itself to the available width
```

A `Toolbar` **fills the width it is offered** and decides, every layout pass,
which commands stay inline and which collapse into the chevron menu — so it
never spills outside its container and never truncates an action label (a
truncated *action* reads poorly; the desktop convention is to overflow excess
commands into a menu instead).

End-to-end demo: `cargo run -p over-constraint` (section 1). Source:
[examples/over_constraint/src/main.rs](../examples/over_constraint/src/main.rs).

---

## Quickstart

```rust
use bastyde::prelude::*;
use bastyde::widgets::{Toolbar, ToolbarAction};

Toolbar::new()
    .action(
        ToolbarAction::new(tr!(new_doc()))
            .icon(|| IconWidget::doc_add(16.0))
            .on_activate(|ctx| ctx.send_intent(AppIntent::NewDocument)),
    )
    .action(ToolbarAction::new(tr!(open())).on_activate(|ctx| ctx.send_intent(AppIntent::Open)))
    .action(ToolbarAction::new(tr!(save())).on_activate(|ctx| ctx.send_intent(AppIntent::Save)))
```

That's the 90% case: a row of [`ToolbarAction`]s. When the bar is too narrow to
show them all, the lowest-priority ones collapse into the chevron menu and
reappear as it widens.

---

## Items

A toolbar is a sequence of [`ToolbarItem`]s, added with `.item(...)` (or the
`.action(...)` / `.child(...)` sugar). There are four kinds:

| Item | Constructor | Collapses? | Renders inline as | Renders in the menu as |
|---|---|---|---|---|
| **Action** | `ToolbarItem::action(a)` / `.action(a)` | yes (by priority) | a `Button` | a `MenuItem` (with the action's icon) |
| **Pinned widget** | `ToolbarItem::custom(w)` / `.child(w)` | **no** | the widget itself | — (never collapses) |
| **Collapsible widget** | `ToolbarItem::custom(w).overflow_as(...)` / `.overflow_widget(...)` / `ToolbarItem::collapsible(w)` | yes | the widget itself | its declared overflow form |
| **Separator / flexible space** | `ToolbarItem::separator()` / `flexible_space()` | no | a `Divider` / a `Spacer` | — |

### Actions

[`ToolbarAction`] is a command with a label and an activation handler, plus
optional refinements:

```rust
ToolbarAction::new(tr!(bold()))
    .icon(|| IconWidget::bold(16.0))   // icon FACTORY — reused inline AND in the menu
    .tooltip(tr!(bold_tooltip()))      // also the accessible name in IconOnly mode
    .enabled(true)
    .toggle(is_bold)                   // checkable: pressed inline, checkmark in the menu
    .priority(10)                      // higher priority collapses LAST (NSToolbar semantics)
    .always_overflow()                 // WinUI secondary command — lives in the menu, never inline
    .on_activate(|ctx| ctx.send_intent(Editor::ToggleBold))
```

The icon is a factory (`Fn() -> IconWidget`) because `IconWidget` isn't `Clone`
and the toolbar may build it twice — once for the inline `Button`, once for the
menu `MenuItem`.

### Pinned widgets

`ToolbarItem::custom(widget)` (sugar: `.child(widget)`) embeds an arbitrary
widget — a search field, a `SegmentedControl`, a zoom `SpinBox` — that **never
collapses**. Use it for controls that must always stay reachable.

```rust
Toolbar::new()
    .child(SearchField::new(query))           // pinned: always visible
    .action(ToolbarAction::new(tr!(filter())).on_activate(…))
```

### Collapsible widgets — `overflow_as`, `overflow_widget`, `ToolbarOverflow`

A custom widget becomes collapsible by declaring its **overflow representation**
(NSToolbar `menuFormRepresentation` / Qt `QWidgetAction`). Pick the form that
reads best in a menu:

#### 1. `.overflow_as(action)` — a menu *row*

Best when the control's menu form is a single command. An icon-only inline
control reuses its icon as the menu item's leading glyph:

```rust
ToolbarItem::custom(IconButton::new(IconWidget::checkmark(16.0)).tooltip(tr!(confirm())))
    .overflow_as(
        ToolbarAction::new(tr!(confirm()))
            .icon(|| IconWidget::checkmark(16.0))   // shown as the MenuItem icon
            .on_activate(|ctx| ctx.send_intent(App::Confirm)),
    )
```

#### 2. `.overflow_widget(factory)` — a live *widget* in the menu

Best for **stateful inputs** (a combo box, a slider) that have no meaningful
single-command form. The factory rebuilds the control inside the menu, bound to
the **same signal** as the inline instance, so it stays fully usable while
collapsed — selecting in the menu copy updates the inline copy and vice-versa:

```rust
let view_mode = Signal::new(Some("List".to_string()));
let menu_mode = view_mode.clone();

ToolbarItem::custom(ComboBox::new(["List", "Grid", "Columns"], view_mode))
    .overflow_widget(move || {
        Box::new(ComboBox::new(["List", "Grid", "Columns"], menu_mode.clone()))
    })
```

When the bar is too narrow, the inline `ComboBox` is hidden and an equivalent,
live `ComboBox` appears in the chevron menu. State is shared through the cloned
`Signal`, so the two are never out of sync.

> A factory (`Fn() -> Box<dyn Widget>`) is required rather than a value because
> widgets aren't `Clone` and the menu builds its row lazily.

#### 3. `ToolbarOverflow` trait — a widget that knows its own menu form

When a *reusable* widget always overflows the same way, implement
[`ToolbarOverflow`] on it and add it with `ToolbarItem::collapsible(w)` — no
per-call `overflow_as`:

```rust
impl ToolbarOverflow for ZoomControl {
    fn toolbar_menu_form(&self) -> ToolbarAction {
        ToolbarAction::new(tr!(zoom())).on_activate(/* … */)
    }
}

Toolbar::new().item(ToolbarItem::collapsible(ZoomControl::new(zoom)))
```

### Separators & flexible space

`ToolbarItem::separator()` draws a `Divider` between groups.
`ToolbarItem::flexible_space()` inserts a `Spacer` that pushes the following
items to the trailing edge (NSToolbar `flexibleSpace`).

---

## How overflow is computed

Every layout pass, the toolbar measures each item's **intrinsic size** — even
the currently-collapsed ones, via
[`LayoutContext::measure_intrinsic`](../crates/bastyde-core/src/widget/layout_context.rs)
— so a collapsed command reappears at exactly the right width as the bar grows
(no stale-width glitch). It then runs a greedy priority algorithm:

1. If everything fits, nothing collapses and no chevron is shown.
2. Otherwise the chevron is reserved, and the **lowest-priority** inline
   commands collapse into the menu until the rest fit (ties: the later-declared
   one collapses first).
3. `always_overflow` commands start collapsed regardless of room.

Pinned widgets and separators reduce the room available to collapsible
commands but never collapse themselves.

`Toolbar::is_overflowing()` returns a `Signal<bool>` that is `true` whenever any
command is currently collapsed — useful for adaptive UI (WinUI
`IsOverflowOpen`-adjacent introspection).

---

## The overflow menu

The chevron's drop-down is a real
[`MenuList`](../crates/bastyde-widgets/src/menu_list.rs), not a bare list, so it:

- **sizes compactly** to the currently-collapsed rows (size-to-content, standard
  menu chrome — no fixed width/height);
- **takes focus when opened** (keyboard or pointer) and supports
  arrow / `Home` / `End` / `Enter` navigation, skipping the rows that aren't
  currently collapsed;
- hosts both ordinary menu rows (from `overflow_as` actions) and **live embedded
  widgets** (from `overflow_widget`).

It is driven by
[`MenuList::item_when`](../crates/bastyde-widgets/src/menu_list.rs) — a
conditionally-visible menu row that collapses to zero height (no gap) and is
skipped by keyboard navigation while hidden. That is the general primitive any
app can use for a menu whose rows come and go.

---

## Display mode & orientation

```rust
Toolbar::new()
    .display_mode(ToolbarDisplayMode::IconOnly)   // IconAndText (default) / IconOnly / TextOnly
    .orientation(ToolbarOrientation::Vertical)    // Horizontal (default) / Vertical
    .spacing(6.0)
```

In `IconOnly` mode the label becomes the control's accessible name + tooltip.
Vertical toolbars collapse along the **vertical** axis and the roving arrow keys
become Up/Down.

---

## Accessibility — the ARIA toolbar pattern

`Toolbar` implements the [WAI-ARIA toolbar
pattern](https://www.w3.org/WAI/ARIA/apg/patterns/toolbar/):

- It emits `Role::Toolbar` with its orientation and name
  (`.label(...)` overrides the default localized "Toolbar").
- It is a **single Tab stop with roving tab-index**: <kbd>Tab</kbd> enters the
  toolbar (landing on the last-focused control) and leaves it; the
  <kbd>←</kbd>/<kbd>→</kbd> (or <kbd>↑</kbd>/<kbd>↓</kbd> when vertical) arrow
  keys move focus among the visible controls, and <kbd>Home</kbd>/<kbd>End</kbd>
  jump to the ends. The roving suppression reaches *composite* controls (a
  `ComboBox`, an `IconButton`) correctly — Tab doesn't get stuck on one. Under
  **RTL** the horizontal arrows mirror (<kbd>←</kbd> advances, <kbd>→</kbd> steps
  back), resolved live so a locale change flips them.
- All localizable strings — the chevron's "More" tooltip and the accessible
  name — flow through the framework's Fluent bundle (`en-US` + `fr-FR` shipped),
  so they translate and update reactively on a locale change.
- The chevron announces `HasPopup::Menu` and its expanded state.
- Collapsed commands are **dormant** (absent from the accessibility tree) — they
  are represented by their menu rows instead, so no command is announced twice.
- Toggle actions carry `Toggled`.

---

## API surface

Pull the full, current signatures with:

```bash
python3 tools/extract_widget_api.py Toolbar
```

Key types ([crates/bastyde-widgets/src/toolbar.rs](../crates/bastyde-widgets/src/toolbar.rs)):

- [`Toolbar`] — `new`, `item`, `action`, `child`, `add_child`, `orientation`,
  `display_mode`, `spacing`, `label`, `is_overflowing`.
- [`ToolbarAction`] — `new`, `icon`, `tooltip`, `enabled`, `on_activate`,
  `toggle`, `priority`, `always_overflow`.
- [`ToolbarItem`] — `action`, `custom`, `custom_id`, `collapsible`,
  `overflow_as`, `overflow_widget`, `separator`, `flexible_space`.
- [`ToolbarOverflow`] — `toolbar_menu_form` (implement on a widget for
  `ToolbarItem::collapsible`).
- `ToolbarDisplayMode` { `IconAndText`, `IconOnly`, `TextOnly` },
  `ToolbarOrientation` { `Horizontal`, `Vertical` }.

[`Toolbar`]: ../crates/bastyde-widgets/src/toolbar.rs
[`ToolbarAction`]: ../crates/bastyde-widgets/src/toolbar.rs
[`ToolbarItem`]: ../crates/bastyde-widgets/src/toolbar.rs
[`ToolbarOverflow`]: ../crates/bastyde-widgets/src/toolbar.rs
