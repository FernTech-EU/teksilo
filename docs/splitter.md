<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Splitter

`Splitter` is an N-pane split container with draggable, collapsible
dividers — the Qt `QSplitter` model. It replaces the old two-pane
`SplitView` (no backward compatibility) and is the building block for the
future `DockingLayout`.

- Widget: [crates/bastyde-widgets/src/splitter.rs](../crates/bastyde-widgets/src/splitter.rs)
- Model: [crates/bastyde-widgets/src/splitter/model.rs](../crates/bastyde-widgets/src/splitter/model.rs)
- Sizing engine: [crates/bastyde-widgets/src/splitter/distribute.rs](../crates/bastyde-widgets/src/splitter/distribute.rs)
- Handle: [crates/bastyde-widgets/src/splitter/handle.rs](../crates/bastyde-widgets/src/splitter/handle.rs)
- Tier-3 style: [crates/bastyde-core/src/styles/splitter_style.rs](../crates/bastyde-core/src/styles/splitter_style.rs) + [recipe](../crates/bastyde-widgets/src/styles/recipe_splitter_style.rs)
- Demo: `cargo run -p splitter`

## Model + widget

All layout state lives in a shared, cloneable **`SplitterModel`**
(`Rc<RefCell<…>>`, the `SceneModel`/`ListModel` handle pattern). The app
holds a clone to read / mutate / persist; the widget renders it and reacts
to the model's `version` signal at `BindingLevel::Relayout` — so any
external change reflows the panes with **no rebuild**.

```rust
use bastyde_widgets::{Splitter, SplitterModel, PaneDescriptor, Orientation};

let model = SplitterModel::from_panes(vec![
    PaneDescriptor::new().size(220.0).min_size(160.0).stretch(0.0).collapsible(true), // sidebar
    PaneDescriptor::new().min_size(320.0).stretch(1.0),                               // editor
    PaneDescriptor::new().size(280.0).min_size(200.0).stretch(0.0).collapsible(true), // inspector
], Orientation::Horizontal);

Splitter::new(model.clone())
    .pane(sidebar).pane(editor).pane(inspector)   // N content panes, model order
    .pane_label(0, tr!(sidebar()));               // optional a11y region name
```

`Splitter` builder: `new(model)`, `.pane(impl Widget)` / `.pane_id(WidgetId)`
(repeated; count must match `model.pane_count()`), `.child(...)` (a `bati!`
alias for `.pane`), `.pane_label(i, impl Into<Prop<String>>)`,
`.style(impl SplitterStyle)`, `.enabled(bool)`.

Orientation, sizes, min/max, stretch, gutter, snap, and collapse all live on
the **model** (single serializable source of truth, shared with a
`DockingLayout`). Each content pane is wrapped in an internal clip so
overflow can't bleed into a gutter or sibling.

## Sizing

Pixel sizes are the source of truth (Qt). Each layout pass projects the
model's stored sizes onto the current bounds via the pure
[`distribute`](../crates/bastyde-widgets/src/splitter/distribute.rs)
function; **a container resize never writes back**, so drag positions
survive resizes. Stored sizes change only on drag, programmatic mutation,
or structural insert/remove.

- **Stretch** (`PaneDescriptor::stretch`, Qt `setStretchFactor`): positive
  container slack is distributed to `stretch > 0` panes proportional to
  weight; `stretch = 0` panes keep their pixel size. If no pane stretches,
  the surplus goes to the last pane.
- **Min/max**: a deficit (container smaller than the sum of sizes) shrinks
  panes proportional to their room above `min`, never below it. `max`
  clamps growth.
- Equal-size panes: `SplitterModel::new(n, orientation)` (each `stretch = 1`,
  no initial size) yields equal shares.

`Splitter` reports its own `min` as `Σ min[i] + (N−1)·gutter`, so a
min-respecting parent never forces overflow.

## Collapse

Panes marked `.collapsible(true)` can fold to zero width/height, **animated**
(reduced-motion aware — snaps under `prefers-reduced-motion`). A collapsed
pane's divider stays visible and draggable (it's how you restore it). Four
triggers:

- **Programmatic** — `model.set_collapsed(i, bool)` / `toggle_collapsed(i)`
  (animated). Ignores the `collapsible` flag (that flag only gates *user*
  interaction, like Qt `childrenCollapsible`).
- **Double-click** a divider — toggles the adjacent collapsible pane.
- **Drag-past-min snap** — drag a pane below `min − snap_offset` to snap it
  collapsed; drag the divider back out to restore (instant, the pointer is
  the motion).
- **Keyboard** — focus a divider (Tab) and press **Enter**.

## Dynamic panes (hide / show, add / remove)

Three distinct mechanisms, by how much they change:

| | Pane | Its gutter/handle | Content | Reactive (no rebuild)? |
|---|---|---|---|---|
| **Collapse** | size → 0, animated | **stays** (grab it to restore) | dormant | yes |
| **Hide** | size → 0, animated | **removed** — reads as absent | dormant | yes (pane pre-mounted) |
| **Add / remove** (new content) | created / destroyed | created / destroyed | brand-new | no — rebuild |

**Hide / show** a whole pane *and* its gutter via a per-pane `visible` flag —
the reactive "add / remove a pane from a fixed set" trick (the panes are
pre-mounted; toggling `visible` makes one appear/disappear with its divider,
animated, no rebuild):

```rust
let model = SplitterModel::from_panes(vec![
    PaneDescriptor::new().size(220.0).collapsible(true),       // sidebar
    PaneDescriptor::new().stretch(1.0),                        // editor
    PaneDescriptor::new().size(280.0).visible(false),          // inspector — starts hidden
], Orientation::Horizontal);

model.set_pane_visible(2, true);   // inspector + its gutter grow in (animated)
model.set_pane_visible(2, false);  // …and vanish; content goes dormant
```

A hidden pane's content is parked dormant and its gutter's handle is disabled
(Tab-skipped, event-gated) and removed from the AT tree. Use this for toggling
a whole sidebar / inspector / terminal, or a fixed-max split. **Caveat:** two
visible panes separated *only* by hidden panes have no divider between them
(you can't resize across a hidden middle pane until you show one) — for that,
use add/remove below.

**Add / remove with new content** (e.g. VS Code drag-a-tab-to-split, arbitrary
content) is a structural change → rebuild the `Splitter` with the new pane
list (`insert_pane`/`remove_pane` carry sizes across the rebuild). The
*seamless* feel comes from the collapse machinery — insert collapsed then
expand to **grow in**, or collapse then remove to **shrink out**:

```rust
// Grow a new pane in:
model.insert_pane(idx, PaneDescriptor::new().collapsed(true).collapsible(true));
// …rebuild the Splitter with the new content list, then:
model.set_collapsed(idx, false);   // animates 0 → full

// Shrink one out, then drop it (on the tween's end):
model.set_collapsed(idx, true);    // animates full → 0
// …after the tween: model.remove_pane(idx) + rebuild without that content.
```

The full drag-tab-to-split orchestration (split tree + drop zones + rebuild)
is the future **`DockingLayout`**'s job; `Splitter` is its building block and
provides the animated grow-in / shrink-out.

## Accessibility

Each divider is a `Role::Splitter` node: localized name, `numeric_value` /
min / max / value (`"42%"`), `numeric_value_step`, bar-axis orientation,
`set_expanded` of the adjacent collapsible pane, and `controls` relations to
the two panes it resizes. Actions: `Focus`, `Increment`, `Decrement`, and
`Collapse`/`Expand` when a neighbor is collapsible. Resize: arrows /
`Home` / `End` (and AccessKit `Increment`/`Decrement`). The focus indicator
shows on keyboard focus only (`FocusOrigin`). Labeled panes
(`.pane_label`) become named `Role::Group` regions; unlabeled panes stay
transparent (their content represents itself).

## Save / restore (persistence)

The model exposes a serde DTO. Only user-controllable values (per-pane
`stored_size` + `collapsed`) are serialized; structural config
(min/max/stretch/collapsible) is app-declared and reconstructed each run
(Qt `saveState` parity).

```rust
let state: SplitterState = model.export_state();   // serde + Versioned
let ok: bool             = model.import_state(&state); // false if pane count differs
```

`SplitterState` implements `bastyde_settings::Versioned`, so it drops into
the framework's persistence layer. **Don't use one `SettingsFile` per
splitter** — compose every splitter's state into one app/workspace struct
and persist that as a single file:

```rust
#[derive(Serialize, Deserialize, Default, Clone)]
struct WorkspaceLayout { version: u32, main: SplitterState, bottom: SplitterState }
impl Versioned for WorkspaceLayout { /* ... */ }

let file: SettingsFile<WorkspaceLayout> = SettingsFile::load(path, debounce, &migrator)?;
main_model.import_state(&file.snapshot().main);                 // restore on launch

let f = file.clone(); let m = main_model.clone();
let _obs = main_model.version().observe(move |_| {             // debounced auto-save
    let _ = f.mutate(|w| w.main = m.export_state());
});
```

`import_state` bumps the model's `version`, so restoring reflows
immediately; collapsed panes come back collapsed instantly (no open
animation on load). A pane-count mismatch is handled gracefully (restore is
skipped, returns `false`).

## Runtime structure changes

`insert_pane` / `remove_pane` / `replace_pane_desc` mutate the model. Because
changing a container's child *set* is a rebuild in retained mode, the app
reconstructs the `Splitter` widget (with the new `.pane(...)` list) on a
structural change — the model carries the persistent size/collapse state
across that rebuild.

## Tier-3 style

`SplitterStyle::make_handle(cfg, ctx)` paints the divider chrome (line /
hover-dwell / focus indicator); layout dimensions stay on the model.
Install per-call (`.style(...)`) or theme-wide
(`theme.style_slots.splitter = Some(Rc::new(...))`). The default
`RecipeSplitterStyle` ships the IntUI look.

## Not implemented (intentional)

Non-opaque / rubber-band deferred resize (Qt `setOpaqueResize(false)`):
Bastyde resizes live, the modern default.
