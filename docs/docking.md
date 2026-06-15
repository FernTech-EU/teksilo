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
It hugs each side's **leading edge**: for the leading / trailing columns that's
the outer (window) edge; for the **top / bottom** bands the vertical rail is a
**column on the leading cross-edge** (left in LTR, right in RTL) with the dock
content inboard to its side — so a top/bottom rail reads like a leading rail
rather than a thin horizontal strip. A hidden **leading / trailing** side keeps
its rail visible (the reopen affordance); a hidden **top / bottom** band
collapses completely (rail included — a vertical rail can't stand in a
zero-depth band), so reveal it again from an external control (a toolbar
"toggle panel" button, `set_side_visible(side, true)`, or `reveal_dock`). Style
it with `DockingLayout::rail(DockRail::new(side)…)`:

- `.size(IconButtonSize)` — one size for every item (Compact … Hero).
- `.top_slot(|| …)` / `.bottom_slot(|| …)` — fixed widgets pinned above the items
  and at the very bottom (a logo on top, settings/account at the bottom — the VS
  Code convention). To make a slotted control track the rail's item size, bind
  `model.rail_size_mode_signal(side)` inside the factory and map it to an
  `IconButton::size`; the rail rebuilds its slots whenever the size mode changes,
  so reading the signal keeps the slot in step (the factory stays a plain
  `Fn() -> impl Widget`, like every other slot).
- `.overflow_icon(|| IconWidget…)` — when the items don't all fit, the surplus
  are parked **dormant** and reached through this caller-chosen trigger, which
  opens a popover list of the overflowed entries.

**The rail width follows the size mode.** Switching Default / Compact / Icon +
Label resizes the whole strip (the rail thickness is derived from the effective
item size), not just the items. `set_side_rail(side, thickness)` enables the
rail; the rendered width tracks the mode. Any external widget can react to the
switch by binding `model.rail_size_mode_signal(side) -> Signal<DockRailItemSize>`
(the same signal a rail slot reads to resize itself).

**The rail is a drop target, like a `TabWidget` that reorders + accepts
external tabs.** While a dock tab (a rail item or a tab-strip header from any
side) or a single dock (a split-pane header) is dragged over the rail, it paints
an **insertion line** between items, and on drop relocates the activity to that
position: dragging one of the rail's *own* items reorders the side's tabs
(`move_tab`, same source/target side), a tab from *another* side moves here
(`move_tab`), and a single dock becomes a new activity at the drop position
(`promote_to_tab`). Dropping on a hidden side's rail reveals it. An empty
Rail-presentation side accepts the first drop this way too (its content area is
otherwise blank).

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
Activity bar size    ▸  Default / Compact / Icon + Label   (rail item)
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
- **Activity bar size** (`DockRailItemSize::{Default, Compact, Labeled}`) and
  **Tab size** (`DockTabDisplay::{Text, Icon, IconText}`) are per-side, reactive,
  and persisted. The rail / strip rebind and re-render when they change.

**Icons, titles, and tooltips.** Every dock declares a title (`DockWidget::new`)
and, optionally, an icon (`DockWidget::icon`). Both the rail and the tab strip
use them per the size / display mode:

- **Rail** — `Default` / `Compact` show the icon alone (the title is a hover
  **tooltip**); `Labeled` adds a 90°-rotated title beneath the icon (the
  vertical-accordion look — no tooltip, the title is on screen). A dock with no
  icon falls back to its title's initial letter as the glyph.
- **Strip** — the side's `DockTabDisplay` maps straight onto the `TabWidget`'s
  [`TabDisplayMode`](tab-widget.md#tab-display-mode--icon--text--icon--text):
  `Icon` shows the icon alone (title → tooltip) and the tab **sizes to its
  icon**, `Text` the title, `IconText` both (the tab grows to fit the icon). An
  **icon-less** dock in `Icon` mode falls back to its title's initial letter
  (the full title stays in the tooltip + the content panel's AT name), so the
  mode is never a silent no-op.

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
**activity-rail item** — to move (or reorder) the whole tab; dropping it on a
pane splits/stacks there, on another side's **tab bar** inserts it at the drop
position (the bar paints the insertion line — this works for a rail item too,
via the tab bar's `on_external_drop`), on an **activity rail** inserts it at the
line the rail paints (reordering within the side, or accepting the tab from
another side — see *Activity rail* above), and on any other non-pane chrome
relocates it to the end of that side. The Splitter re-derives orientation for the
destination side. Programmatic relocation: `move_dock` / `promote_to_tab` /
`move_tab`.

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

The model gives you the two halves directly:

```rust
let state: DockLayoutState = model.export_state();   // serde + Versioned
model.import_state(&state);                          // restore (also reset-to-default)
```

Only **user-controllable** state is serialized (per-side size / visibility /
presentation / selection and the full tab → arrangement tree, plus corner
owners). App-config — rail thickness, minimums, content factories, closable — is
declared each run and reconstructed (Qt `saveState` parity). On import, unknown
dock ids are dropped, emptied panes/tabs pruned, selections clamped.

### Saving / restoring with `bastyde-settings`

`DockLayoutState` is `Versioned + Serialize + Deserialize + Default + Clone`,
which is exactly what [`SettingsFile<T>`](settings.md) needs — so the disk side
is a debounced, atomic, corrupt-file-quarantining projection of the model.

**1. Load once at startup** (missing file → `default()`; corrupt file →
quarantined to `<path>.broken-<ts>` + `default()`):

```rust
use bastyde::settings::{AppPaths, SettingsFile};
use bastyde::widgets::DockLayoutState;
use bastyde_settings::Migrator;
use std::time::Duration;

let paths = AppPaths::new("com", "FernTech", "Bastyde").expect("config dir");
let dock_file = SettingsFile::<DockLayoutState>::load(
    paths.config_file("docking.toml"),
    Duration::from_millis(500),   // write debounce
    &Migrator::new(),             // v1: no migration steps yet
).expect("load docking layout");
```

**2. Restore *after* the docks are registered.** `import_state` drops unknown
dock ids, so register the panels first (the `.dock(..)` builder registers
eagerly), then import:

```rust
let layout = DockingLayout::new(model.clone())
    .center(editor)
    .dock(DockWidget::new(explorer, lit!("Explorer"), |_| ExplorerPanel::new()))
    .dock(DockWidget::new(terminal, lit!("Terminal"), |_| TerminalPanel::new()));
// docks are now registered → safe to restore:
model.import_state(&dock_file.snapshot());
// `import_state(&DockLayoutState::default())` is also the reset-to-default path.
```

**3. Auto-save on change.** Bind one effect (in the root widget's `build()`) to
the model's two version signals — `version()` (structural: open / close / move /
split) and `geometry_version()` (size / visibility / corners / presentation).
`SettingsFile` debounces, so bursts coalesce into a single write:

```rust
let combined = model.version().zip(&model.geometry_version());
let file = dock_file.clone();
let m = model.clone();
ctx.effect(&combined, move |_| {
    let _ = file.replace(m.export_state());   // schedules a debounced atomic write
});
```

A **selection-only** change (`select_tab`) bumps neither version — it's captured
on the next structural/geometry change, or call `dock_file.flush_now()` on window
close. Bind the per-side `model.side_selected_tab_signal(side)` too if you want
selection persisted live.

**Compose, don't sprinkle files.** Prefer **one workspace file** over one per
dock layout / splitter. Since `SplitterState` and `DockLayoutState` are both
`Versioned` serde DTOs, wrap them and restore each piece via its own
`import_state`:

```rust
#[derive(Default, Clone, PartialEq, Serialize, Deserialize)]
struct WorkspaceLayout {
    version: u32,
    docking: DockLayoutState,
    sidebar_split: SplitterState,
}
impl Versioned for WorkspaceLayout {
    const CURRENT_VERSION: u32 = 1;
    fn version(&self) -> u32 { self.version }
    fn set_version(&mut self, v: u32) { self.version = v; }
}
// one SettingsFile<WorkspaceLayout>.
```

The dock layout is the **content** state; *window* geometry (position / size) is
separate and handled automatically by `WindowConfig::id(..)` +
`SettingsBundle::with_window_state(true)` — see [settings.md](settings.md).

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
