# DockingLayout

`DockingLayout` is a VS Code-style **dockable layout**: a fixed **centre** slot
(the app's main content — the "editor") surrounded by four collapsible,
splittable, draggable side regions — **leading / trailing / top / bottom**. It
is a *layout like any other* (not a window shell à la Qt `QMainWindow`), backed
by a cloneable, serializable [`DockingModel`]. **No floating docks.**

- Widget + orchestrator: [crates/bastyde-widgets/src/docking.rs](../crates/bastyde-widgets/src/docking.rs)
- Geometry engine: [crates/bastyde-widgets/src/docking/geometry.rs](../crates/bastyde-widgets/src/docking/geometry.rs)
- Model + state: [model.rs](../crates/bastyde-widgets/src/docking/model.rs), [state.rs](../crates/bastyde-widgets/src/docking/state.rs)
- Panels / drag / rail / handle: [panel.rs](../crates/bastyde-widgets/src/docking/panel.rs), [drag.rs](../crates/bastyde-widgets/src/docking/drag.rs), [activity_bar.rs](../crates/bastyde-widgets/src/docking/activity_bar.rs), [resize_handle.rs](../crates/bastyde-widgets/src/docking/resize_handle.rs)
- Demo: `cargo run -p docking`

## The structure — four levels

```
DockingLayout
└── Centre (one app widget, always present) + 4 Sides
    └── Side = [optional always-visible DockActivityBar rail] + collapsible content region
        └── content region holds ONE tab stack (in-side strip optional, or
            replaced by the rail)
            └── Tab → a Splitter of panes, one DockWidget per pane
                └── pane = a DockWidget. A **sole** pane renders bare (the
                    tab / rail is its header); a **split** pane (one of several)
                    is wrapped in a single-item ToolBox whose draggable header
                    titles the dock and is its drag handle.
```

There is **no multi-section pane** (no QToolBox-style accordion): stacking two
DockWidgets side-by-side adds a **Splitter pane** (each its own single-item
ToolBox), separated by the Splitter. A **DockWidget** can be dragged out to
**become its own tab**, dropped onto a pane's **edge** to **split** it, or
dropped onto a pane's **centre** to **stack** it (append a Splitter pane to that
tab). A whole **Side** is shown/hidden (animated); the activity rail (when on)
stays visible and is the reopen affordance.

## Quick start

```rust
use bastyde::widgets::{DockingLayout, DockingModel, DockWidget, DockWidgetId, DockSide, DockOpenLocation};

let model = DockingModel::new();
let explorer = DockWidgetId::fresh();
let terminal = DockWidgetId::fresh();

// Leading side as a VS Code activity rail:
model.set_side_rail(DockSide::Leading, 48.0);

let layout = DockingLayout::new(model.clone())
    .center(editor_widget)
    .dock(DockWidget::new(explorer, lit!("Explorer"), |_| ExplorerPanel::new())
        .default_location(DockOpenLocation::side(DockSide::Leading)))
    .dock(DockWidget::new(terminal, lit!("Terminal"), |_| TerminalPanel::new())
        .default_location(DockOpenLocation::side(DockSide::Bottom)));

// Initial layout (panels are registered by `.dock(..)` above, so this is valid):
model.open_dock(explorer, DockOpenLocation::side(DockSide::Leading));
model.open_dock(terminal, DockOpenLocation::side(DockSide::Bottom));
```

`DockWidget::new(id, title, factory)` declares a panel; `factory(id)` builds its
content lazily. `.icon(..)`, `.closable(..)`, `.default_location(..)` configure
chrome. `DockingLayout::new(model).center(w).dock(dw)…` assembles the widget;
`.dock(..)` registers the panel **eagerly**, so the initial layout can be set on
the model before mounting.

## Sides, corners, and geometry

The five region rectangles are computed **directly** (a border-layout with
configurable corners — Qt `QMainWindow::setCorner`). A nested-`Splitter` tree
genuinely cannot express per-corner ownership (in any splitter nesting the
corners always belong to the outer axis), so `DockingLayout` runs a small pure
[`geometry::compute_rects`](../crates/bastyde-widgets/src/docking/geometry.rs)
engine in `place_children`.

- Each side contributes, along the axis toward the centre: an always-visible
  **rail** strip (when in Rail presentation), a resizable/collapsible
  **content** rect, and a **resize handle**.
- **Per-corner ownership** (`model.set_corner(DockCorner::BottomLeading, DockSide::Bottom | DockSide::Leading)`)
  decides whether the bottom bar spans under the leading column or vice-versa.
  The default has top/bottom spanning full-width.
- **Corner degradation**: if a corner's owner side is hidden, the corner falls
  to the other adjacent side, else the centre.
- All extents are clamped non-negative — no container size (down to 0×0 or
  smaller than the sum of minimums) produces a negative or overlapping rect; the
  centre shrinks to zero first.
- RTL mirrors leading/trailing; top/bottom never mirror.

## Resizing, hide/show

Each side has a `DockResizeHandle` (`Role::Splitter`) between its content and the
centre: drag to resize (window-absolute anti-jump math), arrows / `Home` / `End`
to resize / hide / show from the keyboard, double-click to hide, or drag past the
minimum to snap it hidden. A side is **one shown/hidden concept** (the user
equates "collapsible = hideable"); a hidden side keeps its rail and is reopened
from the rail, the keyboard, or the programmatic API (VS Code Cmd+B). Show/hide
is **animated** (reduced-motion aware) and is a *relayout*, not a rebuild, so
content is preserved across it.

## Tabs, stacking, splitting (within a side)

A side's content is a stack of **tabs** (each tab sized to its own content,
`TabSizing::Independent`). A tab's content is a **Splitter** of panes, **one
DockWidget per pane**. A sole pane renders bare (the tab / rail is its header);
a split pane is wrapped in an **Accordion** whose draggable header titles the
dock and **collapses on click** — header-only (taps/drags inside the content are
absorbed, so clicking the panel body never collapses or moves it). Collapsing
**folds the Splitter pane down to the header** (its siblings grow to take the
space) and expanding **restores it to the same size** — the accordion drives
`SplitterModel::set_collapsed`, and the pane's `collapsed_size` is the header
height (a non-zero `collapsed_size` keeps the collapsed pane's header visible
rather than folding it to nothing). Orientation follows the side: leading/trailing use a vertical Splitter +
vertical Accordion headers; top/bottom use a **horizontal** Splitter +
**horizontal** Accordion (rotated-90° vertical header strip). The in-side tab
strip shows for the `Strip` presentation (a denser 38 dp `compact_bar`); the
`Rail` presentation replaces it with the activity rail. The content-vs-centre
resize divider (`DockResizeHandle`) renders with the active `SplitterStyle`, so
it looks and behaves exactly like a Splitter divider.

## Activity rail (`DockRail`)

Set `model.set_side_rail(side, thickness)` to put a side in **Rail**
presentation: an always-visible `DockActivityBar` (a `Role::TabList`) replaces
the in-side strip. Clicking an inactive item selects + shows the side; clicking
the **active** item hides the side. The rail stays visible while the side is
hidden — it is the reopen affordance.

The rail is a **vertical** column of one icon per tab, **pushed to the top**.
Style it with `DockingLayout::rail(DockRail::new(side)…)`:

- `.size(IconButtonSize)` — one size for every item (Compact … Hero).
- `.top_slot(|| …)` / `.bottom_slot(|| …)` — fixed widgets pinned above the items
  and at the very bottom (a logo on top, settings/account at the bottom — the VS
  Code convention).
- `.overflow_icon(|| IconWidget…)` — when the items don't all fit, the surplus
  are parked **dormant** and reached through this caller-chosen trigger, which
  opens a popover list of the overflowed entries.

## Context menus

Right-click a **rail item** or a **dock tab** for the per-activity menu (wired
automatically — no app code):

```text
Hide "<activity>"
──────────────
Move to              ▸  <the other sides>
──────────────
☑ <activity>            (one checkable row per activity in this side)
☑ <activity>
──────────────
Activity bar size    ▸  Default / Compact       (rail item)
  – or –
Tab size             ▸  Text / Icon / Icon + Text   (dock tab)
```

- **Hide** drops the activity from the rail / strip but keeps it in the model so
  it stays **listable + restorable** — it is *not* closed. The selected tab
  hands off to the nearest visible one.
- **Move to** relocates the whole tab to another side, shows that side, and
  selects it (`move_tab`).
- The **checkable list** toggles each activity's visibility (`set_tab_hidden`).
- **Restoring when every activity is hidden** (no tab/rail item to right-click):
  in **Rail** presentation, right-click the empty rail (the `DockActivityBar`
  always shows) → the list + size submenu; in **Strip** presentation, the tab bar
  keeps a trailing **hamburger** (`☰`) that opens the same menu. The menu is
  placed with `BelowPreferred`, so it flips above / clamps to stay on-screen even
  for a bottom-docked bar. The menu lives **only** on tabs, rail items, and the
  `DockActivityBar` — never on panes, accordions, or dock content.
- **Activity bar size** (`DockRailItemSize::{Default, Compact}`) and **Tab size**
  (`DockTabDisplay::{Text, Icon, IconText}`) are per-side, reactive, and
  persisted. The rail / strip rebind and re-render when they change.

Drive any of it from outside the menu too: `model.set_tab_hidden(tab, ..)`,
`model.set_side_rail_size(side, ..)`, `model.set_side_tab_display(side, ..)`,
`model.select_tab_by_id(side, tab)`. Per-tab context menus on a `TabWidget` are
available generally via `TabInfo::context_menu(..)`.

## Drag-to-dock

Drag a split pane's **ToolBox header** — a five-zone overlay appears on each
pane: drop on the **centre** to stack (append a Splitter pane to that tab), on an
**edge fifth** (capped at 48 px so the centre stays reachable) to split before /
after the target pane. Foreign / non-dock payloads are ignored. The drop routes
to `model.stack_into_tab` / `split_into_tab`. Drag a **tab-strip header** — or an
**activity-rail button** — to move (or reorder) the whole tab; dropping it on a
pane splits/stacks there, while dropping it on another side's **tab bar** (or any
non-pane chrome) relocates the tab to the end of that side. The Splitter
re-derives orientation for the destination side. Programmatic relocation:
`move_dock` / `promote_to_tab` / `move_tab`.

## Programmatic open-from-outside

The model is the single source of truth, so panels open from anywhere (a side
toolbar, a command, a menu):

```rust
model.reveal_dock(id);        // ensure open + show its side + select its tab
model.toggle_dock(id);        // open on default location / close
model.open_dock(id, DockOpenLocation::side(DockSide::Trailing).new_tab());
model.set_side_visible(DockSide::Bottom, false);

// Reactive bindings for an external rail / toolbar:
let is_open = model.dock_open_signal(id);            // Signal<bool>
let active  = model.side_selected_tab_signal(side);  // Signal<usize>
```

## Accessibility

- Container `Role::GenericContainer`; each side region `Role::Complementary`
  with a localized landmark name ("Leading panel" …).
- Activity rail `Role::TabList` > `Role::Tab` (selected / click), persists in the
  AT tree while the side is hidden.
- In-side tab strip headers `Role::Tab`; resize handles `Role::Splitter` (value /
  expanded / Increment / Decrement / Collapse / Expand); split-pane ToolBox
  headers carry their own roles + the draggable affordance.
- Structural mutations call `request_accessibility_update()`.

## Persistence

```rust
let state: DockLayoutState = model.export_state();   // serde + Versioned
model.import_state(&state);                          // restore (also reset-to-default)
```

Only **user-controllable** state is serialized (per-side size / visibility /
presentation / selection and the full tab → arrangement tree, plus corner
owners). App-config — rail thickness, minimums, content factories, closable — is
declared each run and reconstructed (Qt `saveState` parity). On import, unknown
dock ids are dropped, emptied panes/tabs pruned, selections clamped. Persist via
`SettingsFile<DockLayoutState>` (compose every dock layout + splitter into one
workspace struct rather than one file each).

## Scope & non-goals (v1)

- **In**: 4 sides + centre, per-corner ownership, Splitter arrangement (one dock
  per pane, both orientations), draggable DockWidgets (promote / split / stack /
  move-side), whole-tab drag across sides, hide/show sides, activity rail,
  programmatic open, serde export/import + reset-to-default, landmark/role a11y.
- **Out / known v1 limitations**: floating/tear-off docks (explicit constraint);
  cross-window dock moves (content factories are per-layout); recursive split
  nesting (flat: one Splitter of single-dock panes per tab); "maximize a dock"
  and hover-flyout auto-hide. **Collapsed-dock a11y**: a split-pane dock collapses
  to its Accordion header, and the header sliver + its content stay *live and
  clipped* during the fold so the collapse animates smoothly — meaning a fully
  collapsed dock's body is still reachable by Tab / screen-reader navigation
  (clipped to zero) rather than parked dormant. Parking it dormant only *after*
  the fold completes (so the animation still plays) is a follow-up.
  **Content preservation**: a
  *structural* change (open / close / move / split) rebuilds the open panels'
  content from their factories — transient widget state (scroll position, unsaved
  edits) is preserved across resize / show-hide / tab-switch (those are relayout/
  repaint, not rebuild) but not yet across structural moves; the
  `version`/`geometry_version` split keeps the common interactions rebuild-free.
  Drop-routing for the keyboard-only "Move to side" tab menu and the RTL resize-
  handle direction are likewise follow-ups.
