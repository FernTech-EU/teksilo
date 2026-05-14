# Debug Inspector Reference

`fern-inspector` is an in-app debug surface for FernUI applications.
It compiles to nothing in release builds (everything lives behind
`cfg(debug_assertions)`) and adds zero overhead when not enabled.

Mental model in one line:

```
FernAppBuilder.install_inspector_in_debug() → F12 toggles a bottom panel inside every window
```

The panel hosts nine tabs: live widget tree + properties + accessibility,
theme + locale switchers, focus chain, registered shortcuts, active
overlays, and registered data models. Plus a toolbar with a picker
tool, a bounds-overlay mode selector, and an opacity slider for the
overlay strokes.

End-to-end smoke example: `cargo run -p simple-button` then press F12.

---

## Enabling the inspector

One line at the builder, regardless of release/debug. The
`FernAppBuilderInspectorExt` extension trait is re-exported from the
umbrella prelude (`fern_ui::prelude::*`) so `install_inspector_in_debug()`
is callable without an extra import or dependency:

```rust
use fern_ui::prelude::*;

FernAppBuilder::new()
    .theme(intui::light())
    .install_inspector_in_debug()       // no-op in release
    .initial_window(WindowConfig::new()…)
    .run();
```

The inspector ships behind the umbrella's `inspector` feature
(default-on). To drop it (and the transitive `rich-text` chain it
pulls in for the Tree-tab filter), depend on `fern-ui` with
`default-features = false` and re-add only the features you need.
Apps that drop the feature can still call
`install_inspector_in_debug` only if they take a direct dependency
on `fern-inspector` themselves.

`install_inspector_in_debug` is a no-op stub when
`!cfg(debug_assertions)`, so the call site stays clean of `#[cfg]`
lines. In debug builds it:

1. Parses `--fern-inspector` from `std::env::args()` and
   `FERN_INSPECTOR=1` (or `=true`) from the environment to seed
   the inspector's initial visibility.
2. Stores a shared `InspectorState` (toggle / selection / picker
   mode / overlay mode / opacity / shell ids) into `app_state`.
3. Registers a default `WindowConfig::post_root` hook that wraps
   every window's user root with `InspectorShell` and registers
   the F12 shortcut.
4. If the app has wired a `SettingsStore` via
   `FernAppBuilder::settings(...)`, bridges the inspector's
   persistent preferences to it (see *Persistence* below).

## Toggling the inspector

| Path | What it does |
|---|---|
| **F12** | Global shortcut, owned by the user-root widget per window. Toggles the panel on/off. |
| **`--fern-inspector` CLI arg** | Open the inspector at startup. |
| **`FERN_INSPECTOR=1` env** | Same as the CLI arg. |
| **`×` toolbar button** | Closes the panel (F12 reopens). |
| **Persisted state** | If the app uses `SettingsStore`, the toggle remembers its last state across launches. |

The shortcut id is `__fern_inspector.toggle`. The double-underscore
prefix marks it as framework-reserved — do not bind it from app code.
It is also shown dimmed in the inspector's Shortcuts tab.

## Panel keyboard shortcuts

Once the panel is open, a handful of single-letter chords speed up
common actions. They are **scoped to the panel subtree** — they only
fire when focus is on the panel or one of its descendants, so the
single-letter `P` / `B` / `T` chords don't hijack typing in the user
app's text inputs. Click anywhere in the panel (a tab header, a
button) to take focus, then:

| Key | Action |
|---|---|
| **Ctrl+P** | Toggle the picker tool (same as the toolbar **Pick** button). |
| **Ctrl+B** | Cycle bounds overlay: `Off → Sel → All → Off`. |
| **Ctrl+Tab** | Switch to the next tab. |
| **Ctrl+Shift+Tab** | Switch to the previous tab. |
| **Esc** | If picker mode is active, stop picking. Otherwise close the panel. |

All five share the framework-reserved `__fern_inspector.*` prefix and
appear dimmed in the Shortcuts tab.

## Toolbar

```
[ Pick ] [ Off | Sel | All ] [ ── opacity ── ]                      [ × ]
```

- **Pick** — toggles the picker tool. While picking, a transparent
  overlay covers the user-root area. Clicking on a widget opens a
  context menu listing the deepest-hit widget plus its ancestors
  (up to 10 entries, walking up to the user-root inclusive). Pick
  any row to select that level — useful for composites where the
  deepest hit is an inner leaf (e.g. a `TextWidget` inside a
  `Button`) but you want the wrapping widget. Click outside the
  menu or press Escape to dismiss; the picker auto-exits after one
  selection or dismissal.
- **Bounds overlay** — `Off` (no overlay), `Sel` (stroke around the
  selected widget only), `All` (stroke every widget; layout
  primitives in cyan, content widgets in magenta; cursor-following
  tooltip with type + size — see *Bounds overlay color legend*).
- **Opacity slider** — dims the bounds-overlay strokes for dense UIs.
  Range 0.1 .. 1.0.
- **×** — closes the panel.

## Tabs

| Tab | What it shows |
|---|---|
| **Tree** | Live widget hierarchy, indented by depth. Click a row to select. Top text input filters by case-insensitive substring match against each type's last segment. When the picker resolves to a widget that's currently off-screen, the row scrolls into view automatically (skipped when the user clicked the row directly — the row is already on-screen). Excludes every InspectorShell subtree (multi-window safe). |
| **Properties** | For the selected widget: type, bounds, dirty flags, parent, children count, activation, `clips_children`, `event_pass_through`, plus a single-line `debug_repr` row. **Copy** button dumps every row plus the full multi-line Debug repr to the clipboard via `ClipboardHandle`. **Right-click** any row to open a `Copy value` context menu that copies just that row's value. |
| **Accessibility** | Role / name / value / advertised actions / toggled / expanded / selected / hidden, from the widget's `accessibility(builder)` output. |
| **Theme** | Preset buttons (**Light** / **Dark**) — clicking calls `EventContext::set_theme(...)`. **Apply** folds every per-row draft back into the active theme; **Reset** discards drafts and re-syncs from the active theme. **Export** dumps the current `Theme` as pretty JSON to the clipboard; **Import** parses the clipboard JSON back into a `Theme` and applies it (silently ignores parse errors). Below: a curated list of editable colors (accent, surfaces, text roles, borders, status colors). Each row carries a `ColorEdit` field — clicking it opens a `ColorPicker` popover with HSV canvas, hue / alpha strips, RGB spinners, hex input, and preset swatches. The picker writes through to the row's draft on every drag; Apply commits the batch. |
| **Locale** | Every locale declared in `I18nConfig::supported_locales`. Click a row to call `EventContext::set_locale(...)`. The active locale is highlighted. |
| **Focus** | Current focused widget plus its ancestor chain (root → leaf). Leaf shown in primary color, ancestors dimmed. |
| **Shortcuts** | Every shortcut in the tree's `ShortcutRegistry` with its effective primary keystroke. Framework-reserved ids (`__`-prefixed) are dimmed. |
| **Overlays** | Active overlays from `OverlayManager`, with their content + anchor labels. |
| **Models** | Data models registered via `.debug_named(...)` (see *Data models*). For each: name, kind (`ListModel`, `TreeModel`, `SelectionModel`), and len. Click a row to select it — its `debug_dump` output is shown below. With nothing selected, the most recently registered model is dumped (dimmed row highlight). Click the same row again to clear the selection. |

## Data models

To make a model show up in the **Models** tab, call `.debug_named("…")`
after construction. Available on `ListModel<T>`, `TreeModel<T>`, and
`SelectionModel`:

```rust
use fern_data::{ListModel, SelectionMode, SelectionModel, TreeModel};

let recents: ListModel<RecentProject> =
    ListModel::from_vec(load_recents()).debug_named("recents");

let outline: TreeModel<OutlineNode> =
    TreeModel::new().debug_named("outline");

let row_selection: SelectionModel =
    SelectionModel::new(SelectionMode::Multi).debug_named("rows");
```

`ListModel` and `TreeModel` require `T: Debug + 'static` (used by the
dump). `debug_named` is always available — in release builds it is a
no-op pass-through, so call sites do not need `#[cfg]` lines.

Internally, each model registers a `Weak<dyn ModelDebug>` adapter in
the thread-local `fern_data::debug_registry`:

- `ListModel` / `TreeModel` — the adapter holds a `Weak` to the
  model's inner `Rc<RefCell<…>>`, so it never extends the model's
  lifetime. When the last model handle drops, the registration
  becomes dead and is pruned on the next `snapshot()`.
- `SelectionModel` — has no shared inner; the strong adapter `Rc`
  lives inside an `Rc<RefCell<Option<…>>>` cloned across handles.
  When the last `SelectionModel` clone drops, the holder reaches
  zero, the adapter is freed, and the registry's `Weak` goes dead.

In all three cases, the inspector never keeps a model alive past
its natural lifetime.

## Persistence

When `FernAppBuilder::settings(SettingsBundle::new())` has been wired,
the inspector bridges five signals to keys under the framework-reserved
`__fern_inspector.*` namespace:

- `__fern_inspector.open` (bool) — last toggle state. Read at startup
  if neither `--fern-inspector` nor `FERN_INSPECTOR` was given.
- `__fern_inspector.bounds_mode` (`"off"` / `"selection"` / `"all"`)
- `__fern_inspector.overlay_opacity` (f32)
- `__fern_inspector.active_tab` (i64) — index of the last-used panel
  tab. Stored as `i64` because TOML lacks unsigned integers and
  `usize` width varies by target. Out-of-range values seed at 0.
- `__fern_inspector.panel_height` (f32) — last user-set panel height.
  Clamped to `[120, 720]` on load and on every observer fire so a
  hand-edited or stale value can't shrink the panel below the toolbar
  or grow it past the user-root.

Bridging is one-way (state → store), with the persisted value used as
the initial seed. Bridge wiring runs once per process, on the first
window's creation. Apps without `SettingsStore` configured see no
behavior change.

The panel grows / shrinks via a **6 px top-edge resize handle**.
Drag captures the pointer, anchors at the click point in widget-local
coords, and updates `state.panel_height` on every move so the handle's
top edge tracks the cursor exactly under live layout.

## Bounds overlay color legend

When the bounds overlay is set to **All**:

- **Cyan** strokes — *layout primitives* (anything whose
  `Widget::type_name()` contains `::primitives::` —
  `HStack`, `VStack`, `ZStack`, `Padding`, `Expand`, `Spacer`,
  `FixedSize`, `Switcher`, `Center`, …).
- **Magenta** strokes — *content widgets* (everything else).
- **Blue accent** — the currently selected widget, drawn 2 px on top.

A small **cursor-following tooltip** also follows the mouse in
`All` mode, showing the deepest widget under the pointer and its
laid-out size — for example `Button · 96×32`. Background tint matches
the bounds-stroke color (cyan for layout primitives, magenta for
content widgets); positioned above the widget by default, flipping
below or shifting left if it would clip the user-root area. Suppressed
when the cursor is over the inspector's own panel. Driven off the
framework's `WidgetTree::hovered_signal()` (added in slice 6) — no
polling.

`All` mode also paints **spacing bands** behind the strokes:

- **Yellow** fill — the four `Padding`-inset bands between a
  `Padding` widget's outer rect and its child's inner rect
  (top, bottom, leading, trailing).
- **Green** fill — the gap between consecutive `HStack` / `VStack`
  siblings, spanning the parent's cross-axis extent.

The bands are translucent so the underlying widget colors still show
through. Use the opacity slider to dim them for dense UIs.

## Limitations

- **Hit-test ignores `set_transform` scopes.** Picking under a
  rotated or scaled subtree returns the widget at its pre-transform
  bounds. Acceptable for a debug picker.
- **Bounds-overlay AllBounds mode walks the arena once per layout
  pass.** Cost is ~O(N) per frame while active. Toggle to `Off` /
  `Sel` when not actively inspecting layout.
- **Theme tab edits a curated subset of `ColorTokens`.** Sixteen
  commonly-edited fields are surfaced; the remaining tokens
  (typography, spacing, etc.) are read-only. The Apply / Reset
  buttons commit or discard the per-row drafts; Light / Dark /
  Import switch the active theme and re-sync drafts via the same
  observer.
- **Multi-window picker exclusion** now tracks every InspectorShell
  id in `state.shell_root_ids: Signal<Vec<WidgetId>>`. The picker
  walks every shell id when hit-testing, so opening a second window
  no longer shadows widgets in older windows.

## Where the code lives

- Crate: [crates/fern-inspector/](../crates/fern-inspector/)
- Entry point: [crates/fern-inspector/src/lib.rs](../crates/fern-inspector/src/lib.rs)
- Shared state: [crates/fern-inspector/src/state.rs](../crates/fern-inspector/src/state.rs)
- Wrapping shell: [crates/fern-inspector/src/shell.rs](../crates/fern-inspector/src/shell.rs)
- Highlight overlay: [crates/fern-inspector/src/highlight.rs](../crates/fern-inspector/src/highlight.rs)
- Picker tool: [crates/fern-inspector/src/picker.rs](../crates/fern-inspector/src/picker.rs)
- Resize handle: [crates/fern-inspector/src/resize_handle.rs](../crates/fern-inspector/src/resize_handle.rs)
- Panel keyboard shortcuts: [crates/fern-inspector/src/keyboard.rs](../crates/fern-inspector/src/keyboard.rs)
- Persistence: [crates/fern-inspector/src/persistence.rs](../crates/fern-inspector/src/persistence.rs)
- Tabs: [crates/fern-inspector/src/tabs/](../crates/fern-inspector/src/tabs/)
- Debug-registry hook: [crates/fern-data/src/debug_registry.rs](../crates/fern-data/src/debug_registry.rs)

## Related core API additions (debug-build only in spirit)

These public APIs were added to fern-core to support the inspector
but are not gated by `cfg` — they are useful for any tooling that
wants to introspect a running tree:

- `WidgetTree::hovered() -> Option<WidgetId>`
- `WidgetTree::hovered_signal() -> Signal<Option<WidgetId>>` — reactive
  mirror updated at every hover change (added in slice 6 to drive the
  AllBounds tooltip without polling)
- `WidgetTree::focused_signal() -> Signal<Option<WidgetId>>` — reactive
  mirror of focused id, drives the inspector's Focus tab without
  polling (added in slice 7)
- `OverlayManager::version() -> &Signal<u64>` — bumped on every
  show/dismiss, drives the inspector's Overlays tab without polling
  (added in slice 7)
- `WidgetTree::hit_test(point)` (delegates to `WidgetArena::hit_test_at`)
- `WidgetArena::hit_test_at(point, exclude)`
- `WidgetBuilder::event_pass_through(bool)` and the corresponding
  `WidgetNode::event_pass_through` field
- `WindowConfig::post_root(F)` per-window root-wrapping hook
- `LayoutContext::widget_bounds(id)`, `widget_at_point(point, exclude)`,
  `arena()`, `focused()`, `shortcut_registry()`, `overlay_manager()`
- `fern_app::DefaultPostRoot` typed `app_state` slot for an app-wide
  default `post_root` wrapper
