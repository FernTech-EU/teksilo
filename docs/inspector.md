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

One line at the builder, regardless of release/debug:

```rust
use fern_inspector::FernAppBuilderInspectorExt;

FernAppBuilder::new()
    .theme(Theme::light_default())
    .install_inspector_in_debug()       // no-op in release
    .initial_window(WindowConfig::new()…)
    .run();
```

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

## Toolbar

```
[ Pick ] [ Off | Sel | All ] [ ── opacity ── ]                      [ × ]
```

- **Pick** — toggles the picker tool. While picking, a transparent
  overlay covers the user-root area and the next click selects the
  widget under the cursor. The picker auto-exits after one pick.
- **Bounds overlay** — `Off` (no overlay), `Sel` (stroke around the
  selected widget only), `All` (stroke every widget; layout
  primitives in cyan, content widgets in magenta).
- **Opacity slider** — dims the bounds-overlay strokes for dense UIs.
  Range 0.1 .. 1.0.
- **×** — closes the panel.

## Tabs

| Tab | What it shows |
|---|---|
| **Tree** | Live widget hierarchy, indented by depth. Click a row to select. Excludes the inspector's own subtree. |
| **Properties** | For the selected widget: type, bounds, dirty flags, parent, children count, activation, `clips_children`, `event_pass_through`. |
| **Accessibility** | Role / name / value / advertised actions / toggled / expanded / selected / hidden, from the widget's `accessibility(builder)` output. |
| **Theme** | Two preset buttons (Light / Dark) — clicking calls `EventContext::set_theme(...)`. Below: a curated read-only swatch list (accent, surfaces, text roles, borders, status colors). |
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
the inspector bridges four signals to keys under the framework-reserved
`__fern_inspector.*` namespace:

- `__fern_inspector.open` (bool) — last toggle state. Read at startup
  if neither `--fern-inspector` nor `FERN_INSPECTOR` was given.
- `__fern_inspector.bounds_mode` (`"off"` / `"selection"` / `"all"`)
- `__fern_inspector.overlay_opacity` (f32)
- `__fern_inspector.active_tab` (i64) — index of the last-used panel
  tab. Stored as `i64` because TOML lacks unsigned integers and
  `usize` width varies by target. Out-of-range values seed at 0.

Bridging is one-way (state → store), with the persisted value used as
the initial seed. Bridge wiring runs once per process, on the first
window's creation. Apps without `SettingsStore` configured see no
behavior change.

Panel height persistence waits on a drag-resize handle (no UI for it
yet — the panel is currently a fixed 280 px).

## Bounds overlay color legend

When the bounds overlay is set to **All**:

- **Cyan** strokes — *layout primitives* (anything whose
  `Widget::type_name()` contains `::primitives::` —
  `HStack`, `VStack`, `ZStack`, `Padding`, `Expand`, `Spacer`,
  `FixedSize`, `Switcher`, `Center`, …).
- **Magenta** strokes — *content widgets* (everything else).
- **Blue accent** — the currently selected widget, drawn 2 px on top.

Padding/gap visualization (tinted bands inside `Padding` and between
stack siblings) is queued for a later slice.

## Limitations

- **Hit-test ignores `set_transform` scopes.** Picking under a
  rotated or scaled subtree returns the widget at its pre-transform
  bounds. Acceptable for a debug picker.
- **Bounds-overlay AllBounds mode walks the arena once per layout
  pass.** Cost is ~O(N) per frame while active. Toggle to `Off` /
  `Sel` when not actively inspecting layout.
- **Theme tab does not yet support per-color editing.** Slice 4 ships
  the preset switcher only; per-color RGB / hex editing waits for
  the real `ColorPicker` widget (Phase B in `widgets-plan.md`).
- **Multi-window picker exclusion uses a single `shell_root_id`
  signal.** When more than one window is open, the picker excludes
  the most recently opened window's shell — fine for the common
  single-window case, but may shadow widgets in older windows.

## Where the code lives

- Crate: [crates/fern-inspector/](../crates/fern-inspector/)
- Entry point: [crates/fern-inspector/src/lib.rs](../crates/fern-inspector/src/lib.rs)
- Shared state: [crates/fern-inspector/src/state.rs](../crates/fern-inspector/src/state.rs)
- Wrapping shell: [crates/fern-inspector/src/shell.rs](../crates/fern-inspector/src/shell.rs)
- Highlight overlay: [crates/fern-inspector/src/highlight.rs](../crates/fern-inspector/src/highlight.rs)
- Picker tool: [crates/fern-inspector/src/picker.rs](../crates/fern-inspector/src/picker.rs)
- Persistence: [crates/fern-inspector/src/persistence.rs](../crates/fern-inspector/src/persistence.rs)
- Tabs: [crates/fern-inspector/src/tabs/](../crates/fern-inspector/src/tabs/)
- Debug-registry hook: [crates/fern-data/src/debug_registry.rs](../crates/fern-data/src/debug_registry.rs)

## Related core API additions (debug-build only in spirit)

These public APIs were added to fern-core to support the inspector
but are not gated by `cfg` — they are useful for any tooling that
wants to introspect a running tree:

- `WidgetTree::hovered() -> Option<WidgetId>`
- `WidgetTree::hit_test(point)` (delegates to `WidgetArena::hit_test_at`)
- `WidgetArena::hit_test_at(point, exclude)`
- `WidgetBuilder::event_pass_through(bool)` and the corresponding
  `WidgetNode::event_pass_through` field
- `WindowConfig::post_root(F)` per-window root-wrapping hook
- `LayoutContext::widget_bounds(id)`, `widget_at_point(point, exclude)`,
  `arena()`, `focused()`, `shortcut_registry()`, `overlay_manager()`
- `fern_app::DefaultPostRoot` typed `app_state` slot for an app-wide
  default `post_root` wrapper
