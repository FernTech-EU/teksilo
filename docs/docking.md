<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Docking — building a VS Code-style shell

[`DockingLayout`](../crates/teksilo-widgets/src/docking.rs) gives you the shape
every IDE, DAW and 3D tool has: one **centre** that is always there (your
editor, your canvas, your document), surrounded by four **sides** the user can
resize, collapse, re-tab and drag panels between.

You describe it twice and only twice: a [`DockingModel`](../crates/teksilo-widgets/src/docking/model.rs)
that owns the layout *state*, and a list of `DockWidget` declarations that say
what each panel is called and how to build its content. Everything else — the
tab strips, the activity rail, the splitters, the drag targets, the context
menus, the accessibility tree — the widget builds for you.

```rust
use teksilo::prelude::*;
use teksilo::widgets::{
    DockOpenLocation, DockSide, DockWidget, DockWidgetId, DockingLayout, DockingModel,
};

let model = DockingModel::new();
let explorer = DockWidgetId::fresh();

let shell = DockingLayout::new(model.clone())
    .center(my_editor())
    .dock(DockWidget::new(explorer, lit!("Explorer"), |_| file_tree()));

model.open_dock(explorer, DockOpenLocation::side(DockSide::Leading));
```

Those five lines are the whole layout: an editor filling the window, a 260 dp
Explorer panel on the leading edge, a draggable divider between them, and a
keyboard- and screen-reader-navigable structure you did not write.
[§1](#1-the-minimal-case-in-context) puts them in a widget that runs.

Verified against teksilo 0.13.1. Sources:
[`docking.rs`](../crates/teksilo-widgets/src/docking.rs),
[`docking/model.rs`](../crates/teksilo-widgets/src/docking/model.rs),
[`docking/panel.rs`](../crates/teksilo-widgets/src/docking/panel.rs),
[`docking/activity_bar.rs`](../crates/teksilo-widgets/src/docking/activity_bar.rs).
Exact public surface: [the generated catalog page](widgets/docking.md).
Runnable demo: [`examples/docking`](../examples/docking/src/main.rs) —
`cargo run -p docking`.

## 1. The minimal case, in context

Those calls are not free-floating. The model and the side configuration belong
to your root widget's constructor; the layout and the placement calls belong to
its `build()`. The shape that actually runs:

```rust
use teksilo::prelude::*;
use teksilo::widgets::{
    DockOpenLocation, DockSide, DockWidget, DockWidgetId, DockingLayout, DockingModel,
    Expand, VStack,
};

#[derive(Debug)]
struct Shell {
    model: DockingModel,
    explorer: DockWidgetId,
    root: Option<WidgetId>,
}

impl Shell {
    fn new() -> Self {
        // Model + side configuration: once, here. Neither is persisted.
        let model = DockingModel::new();
        model.set_side_rail(DockSide::Leading, 48.0);
        Self { model, explorer: DockWidgetId::fresh(), root: None }
    }
}

impl Widget for Shell {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let layout = DockingLayout::new(self.model.clone())
            .center(my_editor())
            .dock(DockWidget::new(self.explorer, lit!("Explorer"), |_| file_tree()));

        // `.dock(…)` registered the dock immediately, so placement comes after it.
        // Both calls are safe on every re-run of `build()` — see §3.
        self.model
            .open_dock(self.explorer, DockOpenLocation::side(DockSide::Leading));

        let root = ctx.add(
            VStack::new()
                .child(my_toolbar())
                .child(Expand::new().flex(1.0).child(layout)),
        );
        self.root = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

fn main() {
    TeksiloAppBuilder::new()
        .theme(intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Shell")
                .size(1280, 860)
                .root(|tree, _state| tree.add(Shell::new())),
        )
        .run();
}
```

Three rules about where the pieces live:

- **The model outlives the widget.** Keep the `DockingModel` on your root
  widget (or in app state) and hand `DockingLayout::new` a clone. It is an
  `Rc<RefCell<…>>` handle — `.clone()` shares, it does not copy. Every
  "toggle the sidebar" button in your app drives that same handle.
- **Dock ids are yours to hold.** `DockWidgetId::fresh()` mints one; store it
  in a struct field. Everything you will ever want to do to a panel later
  (open it, reveal it, ask whether it is open) is addressed by that id.
- **`DockingLayout` reports `flex = 0`.** In a `VStack` with a toolbar above
  it, you have to hand it the leftover space explicitly — hence the `Expand`
  above. Dropped, the layout collapses to its (small) intrinsic height.

`.center(…)` accepts either a widget value or a `WidgetId` you already
registered with `ctx.add`. If you never call it you get a blank surface, which
is fine while you are getting the sides right.

For more than a couple of panels, `.docks(…)` and `.rails(…)` take iterators:

```rust
DockingLayout::new(model.clone())
    .center(editor)
    .docks(vec![explorer_dock, search_dock, terminal_dock])
```

## 2. The four levels

Everything in this guide is one of four nested things, and knowing which one
you mean makes the API read itself:

| Level | What it is | Addressed by |
|---|---|---|
| **Side** | One of the four edges: `Leading`, `Trailing`, `Top`, `Bottom` | `DockSide` |
| **Activity** (tab) | One entry in the side's tab strip or activity rail | `DockTabId` |
| **Pane** | One slot of the activity's `Splitter` | `(side, tab_idx, pane_idx)` — a `DockLoc` |
| **Dock** | The panel itself: a title, an icon, a content factory | `DockWidgetId` |

Three of these types are exported from the module rather than the crate root,
so they need the longer path: `use teksilo::widgets::docking::{DockLoc,
DockSideState, DockTabState};`. Everything else in this guide is reachable as
`teksilo::widgets::<Name>`.

One dock per pane, always. An activity holding two docks is a two-pane
splitter; there is no third nesting level inside a pane. `Leading` and
`Trailing` are writing-direction-relative (left/right swap under RTL);
`Top` and `Bottom` never mirror.

A side starts **hidden and empty**. It becomes visible the first time a dock
lands on it — `open_dock` sets `visible = true` for you — so the initial layout
is just a few `open_dock` calls.

## 3. Placing docks: activities and panes

`DockOpenLocation` says where a dock goes, and its two modes are the difference
between "another tab" and "share this tab":

```rust
// A new activity of its own (a new entry in the rail / tab strip).
model.open_dock(terminal, DockOpenLocation::side(DockSide::Bottom).new_tab());

// Stack into the side's currently-selected activity, as another pane.
model.open_dock(problems, DockOpenLocation::side(DockSide::Bottom).stack());
```

`DockOpenLocation::side(s)` defaults to `.stack()`, and stacking into an empty
side creates the first activity — so the common "Explorer and Search grouped in
one Source activity" reads:

```rust
model.open_dock(explorer, DockOpenLocation::side(DockSide::Leading));
model.open_dock(search, DockOpenLocation::side(DockSide::Leading).stack());
model.set_dock_activity_title(explorer, lit!("Source"));
```

That last line matters more than it looks. An activity with no explicit title
borrows the title of the dock in its first **non-collapsed** pane — so a grouped
activity silently renames itself not only when the user reorders its panes, but
when they merely collapse the lead one's accordion. `set_dock_activity_title(id,
title)` pins it. `set_tab_title(tab_id, title)` is the same thing addressed by
`DockTabId`, and takes an `Option<LocalizedString>` rather than a bare title:
`Some(…)` pins, `None` clears the override and hands the label back to the
primary dock. Either way it is app config — set it each run, it is not
persisted.

`open_dock` is **idempotent per side** — re-opening a dock that is already on
its target side is a no-op with no version churn, which is what makes it safe
to call from a `build()` that re-runs. `move_dock` is a one-line alias for it
and inherits the same early return, so it is *not* the escape hatch: to
re-place a dock within the side it already occupies, call `promote_to_tab`,
`split_into_tab` or `stack_into_tab`.

Placing a dock you never registered is a `debug_assert!` in a debug build and a
**silent no-op** in release — the one ordering mistake that costs a beginner an
afternoon. `.dock(…)` registers immediately, so declaring before placing is
enough; `is_registered(id)` is the read-back when your dock list is
data-driven.

The other placement calls exist mostly so the drag-and-drop machinery has
something to call, but they are public and you can drive them yourself:

| Call | Effect |
|---|---|
| `promote_to_tab(id, side, at_tab)` | Pull a dock out into its own new activity at that index |
| `split_into_tab(id, side, tab_idx, pane_idx, before)` | Insert as a pane before/after an existing one |
| `stack_into_tab(id, side, tab_idx)` | Append as a pane of that activity |
| `move_dock(id, loc)` | Alias for `open_dock` — detaches and re-places only when the side differs |
| `close_dock(id)` / `close_tab(tab_id)` | Remove a dock, or a whole activity |
| `move_tab(tab_id, target_side, at_tab)` | Move an entire activity to another side |

## 4. Sides: size, minimum, visibility

Per side, in logical pixels:

```rust
model.set_side_size(DockSide::Leading, 320.0);      // stored content width
model.set_side_min_size(DockSide::Leading, 180.0);  // the drag floor
```

Defaults are 260 dp (min 120) for the leading/trailing columns and 200 dp
(min 80) for the top/bottom bands. Size and minimum are **app config**, not
user state, with one asymmetry: the size is persisted (the user dragged it),
the minimum is not (you declare it each run).

Visibility is animated by default:

```rust
model.toggle_side_visible(DockSide::Bottom);
model.set_side_visible(DockSide::Trailing, true);
model.set_side_visible_immediate(DockSide::Trailing, false); // no tween
```

A collapsing side does **not** reflow its content at a shrinking width — it
lays it out at full size and slides it out under a clip, so a collapse costs
nothing per frame. Fully collapsed, the content goes dormant: out of paint,
out of the focus ring, out of the accessibility tree.

The user resizes a side by dragging the 6 dp divider between it and the centre.
That divider is a real focusable control: arrows step it by 16 dp, `Home` hides
the side, `End` shows it, `Enter` toggles it, a double-click hides it, and
dragging 30 dp past the minimum snaps it shut. A finger gets a widened grab
band (24 dp compact, 44 dp touch) without the divider itself getting fatter.

**Reopening a hidden side** depends on its presentation, and this is the one
geometry rule worth internalising:

- A hidden **leading / trailing** side keeps its activity rail, if it has one.
  The rail *is* the reopen affordance — that is why it lives outboard of the
  collapsing region.
- A hidden **top / bottom** band collapses **completely**, rail included (a
  vertical rail cannot stand in a zero-depth band). You must give the user
  another way back: a toolbar button, a menu item, a shortcut, all calling
  `set_side_visible` or `reveal_dock`.

## 5. Tabs or an activity rail

Each side surfaces its activities one of two ways.

**Strip** (the default) is the side's own tab bar, drawn inside the collapsing
region. It is **always** drawn, single activity or not, so a lone panel reads as
a tab rather than as a bespoke title bar — which is also what lets a sole-pane
dock go headerless in [§6](#6-dock-headers-and-view-actions).

**Rail** is the VS Code activity bar — the
[`DockActivityBar`](../crates/teksilo-widgets/src/docking/activity_bar.rs) widget: a
vertical icon column outboard of the content, always visible. Clicking an inactive item selects it and shows the
side; clicking the *active* item hides the side again. Switch a side into it by
giving it a rail thickness:

```rust
model.set_side_rail(DockSide::Leading, 48.0);   // non-zero ⇒ Rail presentation
```

Then style it with a `DockRail`, passed to the layout:

```rust
use teksilo::widgets::{DockRail, IconButtonSize, IconWidget};

DockingLayout::new(model.clone())
    .rail(
        DockRail::new(DockSide::Leading)
            .size(IconButtonSize::Large)
            .divider()
            .top_slot(|| app_logo())
            .bottom_slot(|| settings_button())
            .overflow_icon(|| IconWidget::chevron_down(18.0)),
    )
```

`top_slot` / `bottom_slot` pin a widget above the items and at the far end.
`leading_slot` / `trailing_slot` are their Strip-presentation counterparts —
but with a **weaker contract**: they live inside the side's `TabWidget`, so
they vanish when the side is hidden. Anything that must survive a hidden side
belongs in a rail slot, or outside the docking system entirely.

`overflow_icon` earns its keep on short windows: when the items no longer fit,
the surplus are parked and reached through a popover behind that glyph. Without
it they simply clip.

`.background(color)` tints the rail strip and `.divider_color(color)` overrides
the line `.divider()` draws; both take the usual `impl Into<ColorProp>`, so a
theme role stays reactive and a bare `Color` pins it.

### Dockless buttons on the rail

A `DockAction` looks like an activity item but opens no panel — VS Code's
Accounts and Manage gears:

```rust
use teksilo::widgets::{DockAction, DockActionId, DockActionPlacement};

const SETTINGS: DockActionId = DockActionId::named("app.settings");

DockRail::new(DockSide::Leading).action(
    DockAction::new(SETTINGS, lit!("Settings"), || gear_icon(), |ctx| {
        ctx.send_intent(AppIntent::OpenSettings)
    })
    .placement(DockActionPlacement::Pinned),
)
```

`DockActionId::named` derives a stable id from a string, identical across runs
and machines — which is what keeps an automation script that clicks your
settings gear from going flaky. Placement is `Start` (before the activities),
`End` (after them, still flowing) or `Pinned` (past the spacer, anchored to the
far edge). Actions are deliberately restricted: never draggable, never hidable,
never overflow-parked, never persisted.

`.actions(iter)` is the loop form when the cluster comes from data. Beside
`.placement`, an action carries `.tooltip(text)` (defaults to the label, and is
ignored in `Icon + Label` rail mode, which paints the label inline) and
`.enabled(impl Into<Prop<bool>>)` — pass a `Signal<bool>` to grey a command out
live, with no rebuild.

Three things to know. Actions render in **Rail presentation only** — a side
flipped back to Strip drops the whole cluster, so mirror it in
`trailing_slot` if that transition is reachable in your app. `.toggled(sig)`
is reflect-only: it paints the selected surface while the signal is true, but
activating the action does not write it — your `on_activate` must. And ids must
be **unique per rail**: a duplicate (two actions declared with the same id, or
an FNV-1a collision between two `DockActionId::named` values) is a
`debug_assert!`, because in release both buttons still render and "click the
settings action" quietly becomes ambiguous.

### Item and tab display modes

Users switch these from the context menu; you can set the starting point:

```rust
use teksilo::widgets::{DockRailItemSize, DockTabDisplay};

model.set_side_rail_size(DockSide::Leading, DockRailItemSize::Labeled);
model.set_side_tab_display(DockSide::Bottom, DockTabDisplay::IconText);
```

`DockRailItemSize` is `Default` / `Compact` / `Labeled` (icon plus a 90°-rotated
title). `Compact` shrinks the rail to the standard `IconButtonSize::Default`,
whatever larger size `DockRail::size` asked for — it is deliberately **not** the
extra-small `IconButtonSize::Compact`, because a rail glyph is the activity's
only identifier and has to stay legible. `DockTabDisplay` is `Text` / `Icon` /
`IconText`. In any icon-bearing mode a dock with no icon falls back to its
title's initial, so the mode is never a silent no-op.

A rail slot that wants to match the current item size binds
`model.rail_size_mode_signal(side)` inside its factory — the rail rebuilds its
slots on every change, so reading the signal there is enough:

```rust
.bottom_slot({
    let mode = model.rail_size_mode_signal(DockSide::Leading);
    move || {
        // Compact == IconButtonSize::Default, not IconButtonSize::Compact —
        // match the items, don't undershoot them.
        let size = if mode.get() == DockRailItemSize::Compact {
            IconButtonSize::Default
        } else {
            IconButtonSize::Large
        };
        IconButton::new(gear_icon()).size(size).tooltip(lit!("Settings"))
    }
})
```

## 6. Dock headers and view actions

A dock that shares its activity with others gets an `Accordion` header
automatically: it titles the panel, drags it, and collapses it. A dock that is
the **sole pane of its activity** is bare by default — its tab or rail item is
already its header — and opts in with `.show_header(true)`. The predicate is the
pane count, not the dock count: a side holding three single-dock activities
renders all three bare.

Either header can carry inline **view actions**, the VS Code "New File /
Collapse All" pattern:

```rust
use teksilo::widgets::{ToolbarAction, ToolbarItem};

DockWidget::new(explorer, lit!("Explorer"), |_| file_tree())
    .icon(|| folder_icon(18.0))
    .show_header(true)
    .header_actions(|_id| vec![
        ToolbarItem::action(
            ToolbarAction::new(lit!("New File"), || plus_icon(14.0))
                .on_activate(|ctx| ctx.send_intent(AppIntent::NewFile)),
        ),
        ToolbarItem::action(
            ToolbarAction::new(lit!("Collapse All"), || chevron_icon(14.0))
                .priority(-1)
                .on_activate(|ctx| ctx.send_intent(AppIntent::CollapseAll)),
        ),
    ])
    .default_location(DockOpenLocation::side(DockSide::Leading))
```

You return a flat list and the framework does the rest: it hosts them in a
`Toolbar`, so they gain **overflow** (lowest `priority` collapses into a `⌄`
menu when the header is tight) and the **right axis** for free — a horizontal
row on a leading/trailing side, a vertical column in a rotated top/bottom strip.
Never write that orientation logic yourself. `ToolbarItem::custom(w)` pins an
arbitrary widget instead of a collapsible action.

Beside your actions sits a framework-owned **`⋮` options** menu, offering
"Move to new activity" / "Move to side" for a grouped dock, or "Hide" /
"Move to" for a sole-pane one. There is deliberately **no Close row**: the user
can only hide, because hiding is reversible from the activities checklist and
closing would leave no way back. (`close_dock` / `close_tab` exist for your own
code — see [§3](#3-placing-docks-activities-and-panes).)

`.default_location(loc)` is where `toggle_dock` and `reveal_dock` put a dock
when you call them without a target.

## 7. What the user can rearrange — and how to stop them

Out of the box the user can: drag an activity along the rail or tab strip to
reorder it, drag it onto another side's rail or strip to move it, drag a dock's
header onto another pane to split or stack, resize and collapse any side, and
hide an activity from its context menu.

**Drag-to-dock** works on a pane's five zones: drop on the **centre** to join
that activity as another pane; drop on an **edge fifth** to split before or
after the pane you are over. Those zones are a plain multi-zone `DropTarget` at
`zone_size_factor(0.2)` — strictly proportional, with no pixel floor. On a 60 dp
pane an edge zone is 12 dp and stays 12 dp, so a layout that can get that narrow
needs a minimum size that keeps the zones hittable.

**Right-click** an activity for: `Hide "<name>"`, `Move to ▸`, a checkable list
of every activity on that side, and `Activity bar size ▸` (rail) or
`Tab size ▸` (tab). Right-clicking the rail's empty background gives the
checklist and the size submenu, without the per-activity rows — that is the way
back when the user has hidden everything.
A Strip-presentation side grows a trailing hamburger for the same purpose.

To take affordances away, declare a `DockPolicy`:

```rust
use teksilo::widgets::DockPolicy;

DockingLayout::new(model.clone())
    .policy(DockPolicy { allow_dock_drag: false, ..DockPolicy::default() })
```

| Flag | Removes |
|---|---|
| `allow_activity_drag` | Rail/tab reordering and moving, plus every "Move to" menu |
| `allow_dock_drag` | Dragging a dock out of a split pane by its header |
| `allow_side_collapse` | Hiding a side by handle, double-click, `Home`, `Enter`, or rail click — resizing still works |
| `allow_activity_hide` | The context-menu Hide and the checklist |

`DockPolicy::locked()` clears all four. Every flag gates the **user affordance
only** — your own `toggle_side_visible`, `open_dock`, `set_tab_hidden` keep
working, so a locked layout is still a layout your app drives.

Policy lives on the **model**: `DockingLayout::policy(…)` is sugar for
`model.set_policy(p)`, and `model.policy()` reads it back. So it is not a
construction-time decision — a "Lock layout" checkbox in your View menu is one
`set_policy` call. It is app-declared each run and is *not* persisted.

To remove a whole edge, disable it:

```rust
DockingLayout::new(model.clone()).disable_side(DockSide::Top)
```

A disabled side renders nothing, reserves no space, refuses drops and rejects
placement calls. Its docks stay in the model and reappear when you re-enable it
with `model.set_side_enabled(side, true)` — `disable_side` is one-way builder
sugar, so the model setter (with `is_side_enabled` to read it back) is the only
route out.
Relocation menus iterate `enabled_move_targets(from)`, so a disabled side is
never offered as a target that would be silently refused.

## 8. Driving the layout from your own chrome

Your menu bar and toolbar talk to the model directly. The reads are reactive,
so a "View ▸ Explorer" checkmark tracks the panel without any wiring:

```rust
MenuItem::new(lit!("Explorer"))
    .reflect_checked(model.dock_open_signal(explorer))
    .on_activate_fn({
        let model = model.clone();
        move |_| model.toggle_dock(explorer)
    })
```

`reflect_checked` (rather than `checked`) is the right half of that pair: the
model owns the truth, the menu only mirrors it.

| Want | Call |
|---|---|
| Show a panel, opening its side and selecting its tab | `reveal_dock(id)` |
| Open/close a panel at its default location | `toggle_dock(id)` |
| Show/hide a whole edge | `set_side_visible(side, b)` / `toggle_side_visible(side)` |
| Is it open? | `is_dock_open(id)` → `bool`, `dock_open_signal(id)` → `Signal<bool>` |
| Is the edge showing? | `is_side_visible(side)`, `side_visible_signal(side)` |
| Which activity is selected? | `side_selected_tab(side)`, `side_selected_tab_signal(side)` |
| Select one | `select_tab(side, idx)` or `select_tab_by_id(side, tab_id)` |
| Where does a dock live? | `dock_location(id)` → `Option<DockLoc>` |
| Which activity holds it? | `activity_of(id)` → `Option<DockTabId>` |
| Hide/restore an activity | `set_tab_hidden(tab_id, b)`, `is_tab_hidden(tab_id)` |
| Is this dock declared yet? | `is_registered(id)` |
| How is the edge configured? | `side_size`, `side_min_size`, `side_presentation`, `side_rail_thickness`, `side_has_rail` |
| How many activities, and what title is pinned on one? | `tab_count(side)`, `tab_title(tab_id)` (the explicit override; `None` when the label derives from a dock) |
| Who owns a corner? | `corner_owner(corner)` |

Prefer `select_tab_by_id` over `select_tab` anywhere the index could be stale:
the rail and strip skip hidden activities, so their visible order is not the
model's. `tab_id_at(side, idx)` is the live inverse when you need to go the
other way.

Two `Signal<u64>`s report that something moved. `version()` bumps on
**structure** — a tab, pane or side added or removed — and the widget binds it
at `BindingLevel::Rebuild`. `geometry_version()` bumps on **size, visibility and
corner ownership**, bound at `Relayout`. Observe the first if you want to
autosave without polling; the split is also the concrete form of the
rebuild-versus-relayout rule [§12](#12-what-v1-does-not-do) states in prose.

## 9. Corners

A docking layout is a border layout with **configurable corners** — Qt's
`QMainWindow` model. By default the top and bottom bands span the full width
and the leading/trailing columns occupy the middle:

```rust
use teksilo::widgets::DockCorner;

// Let the sidebar run to the bottom of the window instead.
model.set_corner(DockCorner::BottomLeading, DockSide::Leading);
```

The owner must be one of the corner's two adjacent sides. Ownership degrades
gracefully when the owning side is hidden, and everything clamps non-negative,
so no window size — down to `0×0` — produces an overlapping rectangle.

## 10. Persisting the layout

`export_state()` returns a `DockLayoutState`: per-side size, visibility,
presentation, selection, the full activity → pane tree, hidden flags, rail and
tab display modes, and corner ownership. It is `Serialize + Deserialize +
Versioned`, so it drops straight into `teksilo-settings`:

```rust
use teksilo::settings::{Migrator, SettingsFile};
use teksilo::widgets::DockLayoutState;

let layout: SettingsFile<DockLayoutState> =
    SettingsFile::load(paths.config_file("layout.toml"), Migrator::new())?;

// At startup, after every `.dock(...)` is registered:
model.import_state(&layout.borrow());

// On quit, or whenever `version()` / `geometry_version()` tells you
// something moved (see §8):
layout.replace(model.export_state())?;
```

What is **not** in there, by design, is everything you declare in code: content
factories, header actions, icons, titles, minimum sizes, rail thickness,
`DockPolicy`, `DockRail` slots. That is Qt `saveState` parity — reconstruct the
app config each run, restore only what the user changed.

Four consequences:

- **Register before you import — and before you open.** `import_state` drops
  dock ids it does not know, so a dock declared after the import is simply
  missing from the restored layout, and `open_dock` on an unregistered id is a
  debug assert and a release no-op ([§3](#3-placing-docks-activities-and-panes)).
  Do all your `.dock(…)` calls first.
- **Ids must be stable across runs.** `DockWidgetId::fresh()` is fine as long
  as you mint them in a fixed order at startup, since the raw values are what
  the file stores. If that makes you nervous, the id is a plain
  `pub struct DockWidgetId(pub u64)`, so you can write your own constants:
  `const EXPLORER: DockWidgetId = DockWidgetId(1);`.
- **Presentation is user state; rail thickness is not**, and the pair does not
  round-trip. `import_state` restores a side's `presentation` verbatim but never
  touches its `rail_thickness`, and `side_rail_thickness` reports 0 for any side
  that is not in Rail presentation. So a file saved in Strip silently cancels
  the `set_side_rail(side, 48.0)` your constructor ran — taking the rail's whole
  `DockAction` cluster with it — while a file saved in Rail, in an app that
  never called `set_side_rail`, leaves a side with no rail *and* no tab strip:
  no selector at all. Call `set_side_rail` **after** `import_state`, not before.
- **Pane sizes survive only an intact pane count.** The per-activity `Splitter`
  state is restored only when every pane in the file is still known; one dropped
  id therefore discards that whole activity's sizing, not just that pane's.

Otherwise `import_state` is forgiving: unknown ids dropped, emptied panes and
activities pruned, selections clamped (including off a hidden tab). A file from
a newer build is refused rather than truncated.

## 11. Accessibility and keyboard

You get this without asking:

- Each side's content region is a named `Role::Complementary` landmark
  ("Leading panel", "Bottom panel", …).
- An activity rail is a `Role::TabList` with `Role::Tab` items and a `controls`
  relation pointing at the content region it governs; a rail's dockless actions
  form a separate `Role::Toolbar` cluster beside it, never inside the tab list,
  because ARIA forbids non-tab children there. Each composite is one Tab stop
  with its own roving arrow/`Home`/`End` cycle.
- Every resize divider is a `Role::Splitter` with arrows, `Home`, `End` and
  `Enter`, and AccessKit `Increment` / `Decrement` / `Collapse` / `Expand`.
- Collapsed and hidden content is dormant, so it is not in the AT tree and Tab
  skips it — there is no phantom focus stop behind a closed panel.

Your part is to give each dock a real title (it becomes the tab name, the rail
tooltip, the landmark name and the menu label) and an icon if you use any
icon-bearing display mode.

## 12. What v1 does not do

Discover these here rather than by hitting them:

- **No floating windows.** A dock cannot be torn off into its own window.
  There is no API for it.
- **No cross-window docking.** One `DockingLayout` per window; panels do not
  move between windows.
- **No recursive nesting.** An activity is a flat `Splitter` of panes, one dock
  each. You cannot nest a splitter inside a pane.
- **Content is rebuilt on structural moves.** Dragging a dock to another side
  re-runs its content factory; transient widget state inside it is lost.
  Resizing, showing/hiding a side and switching tabs do *not* rebuild — those
  are relayout and repaint. Keep anything worth preserving in a model your
  factory reads, not in the widget.
- **RTL divider drag is not mirrored.** The layout itself mirrors correctly
  under RTL, and the divider's *keyboard* direction is correct, but its drag
  math is not yet RTL-aware.
- **No `Close` offered to the user.** `close_dock` / `close_tab` are public and
  your code may call them; what the framework never puts in a menu is a Close,
  because hiding is reversible and closing would leave no way back — see
  [§6](#6-dock-headers-and-view-actions).
- **Hidden-activity state is not reactively observable from app code.**
  `is_tab_hidden` is a snapshot; there is no public signal for it (unlike
  `dock_open_signal` and `side_visible_signal`).
- **The rail's restore menu is pointer-only.** Once every activity on a Rail
  side is hidden, the right-click background menu is the only restore path and
  it is not keyboard-reachable; give keyboard users their own affordance.

## 13. Troubleshooting

**The centre went blank after a structural change.** You are probably rebuilding
your own root and handing `.center(…)` a fresh widget each time. `DockingLayout`
preserves the centre subtree across its *own* rebuilds; if your parent widget
rebuilds, it must preserve its children too — return `true` from
`Widget::preserves_children_on_rebuild()` and re-attach the cached `WidgetId`.

**A panel I opened is nowhere to be seen.** Three usual causes: the side is
disabled (`set_side_enabled(side, false)` makes every placement call a silent
no-op); the activity is hidden (`is_tab_hidden`); or the side is a hidden
top/bottom band, which takes its rail down with it — give the user an external
button (see [§4](#4-sides-size-minimum-visibility)).

**`open_dock` did nothing.** Either it is idempotent per side — a dock already
on that side stays where it is, panes and all — or the dock was never
registered, which is a `debug_assert!` in debug and a silent return in release.
Check `is_registered(id)`, and put every `.dock(…)` before the placement calls.
For the idempotent case, `move_dock` is an alias and will do exactly as little;
use `promote_to_tab` / `split_into_tab` / `stack_into_tab` to re-place a dock
within its own side.

**The activity renamed itself.** An untitled activity borrows the title of its
first **non-collapsed** pane, so this fires when a pane is reordered *and* when
the user simply collapses the lead one's accordion. Pin it with
`set_dock_activity_title(dock_id, title)`, or `set_tab_title(tab_id,
Some(title))` if you hold the `DockTabId`.

**My rail actions disappeared.** The side is in Strip presentation, and actions
are Rail-only. Two ways it gets there: `set_side_rail(side, 0.0)`, or — far more
often — `import_state` restoring a `Strip` presentation over the
`set_side_rail(side, 48.0)` your constructor ran, since presentation is
persisted and rail thickness is not. Call `set_side_rail` after the import
([§10](#10-persisting-the-layout)), and mirror the cluster in
`DockRail::trailing_slot` if the flip is genuinely reachable.

**The restored layout is missing half its panels.** `import_state` ran before
the `.dock(…)` declarations, so it dropped ids it had never seen. Register
first, import second. If the panels are there but the *sizes* inside one
activity reset, that is the same cause one level down: pane sizes are restored
only when every pane in the file survived.

**A restored side has no rail and no tab strip.** The file carried `Rail`
presentation into a run that never called `set_side_rail`, so there is no rail
to build and the strip is suppressed. Call `set_side_rail` after
`import_state` — see [§10](#10-persisting-the-layout).

**A rail slot widget stayed the wrong size after the user switched to Compact.**
The slot factory is not reading `rail_size_mode_signal`. The rail rebuilds its
slots on the change, but a factory that hardcodes a size cannot notice — see
[§5](#5-tabs-or-an-activity-rail).

**Everything is greyed out / nothing drags.** Check for a `DockPolicy` — a
`locked()` policy removes every user affordance while leaving your programmatic
calls working, which looks exactly like this.

## Reference

- [Generated catalog page](widgets/docking.md) — the exact public surface of
  every type named here.
- [`examples/docking`](../examples/docking/src/main.rs) — the full shell:
  activity rail, grouped activities, header actions, corner flip, lock toggle,
  export/restore.
- [Splitter](widgets/splitter.md) — the pane splitter a docking activity is
  built from, and the state type `DockLayoutState` embeds.
- [Tab widget](tab-widget.md) — the strip presentation's underlying widget.
- [Drag & drop](drag-and-drop.md) — the drop-target machinery behind
  drag-to-dock.
- [Settings & persistence](settings.md) — `SettingsFile`, `Migrator`,
  `AppPaths`.
- [A horizontal activity rail for top and bottom](docking-horizontal-rail.md) —
  a backlog note: why a hidden top/bottom band takes its rail with it, and what
  fixing that would cost. Written for a contributor, so it is a chapter of this
  book but is deliberately not in the retrieval corpus — `cargo teksilo show`
  will not find it.
