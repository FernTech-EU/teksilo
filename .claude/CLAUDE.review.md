# Teksilo: Code Review Reference

> This is a trimmed variant of the project's full `CLAUDE.md`, built for code review.
>
> It deliberately keeps only **objective, checkable facts**: coding conventions, build/test
> commands, crate layering, public API contracts, file navigation, and documented intentional
> decisions. It strips the descriptive feature tours and every claim that asserts the code is
> correct, complete, idiomatic, performant, well-tested, or compliant.
>
> **Reviewer stance:** treat every "intentional decision" below as a claim to verify against the
> code, not a reason to wave an issue through. Where this file is silent about a subsystem, that
> silence carries no information about quality. Read the code.

---

## Project Overview (factual)

- Pure-Rust desktop GUI framework. Retained widget tree, SwiftUI-style layout, AccessKit accessibility, wgpu rendering.
- License: MPL-2.0.
- Rust edition: 2024 (resolver 3).
- Workspace member globs: `crates/*` (libraries) and `examples/*` (runnable demos).
- External path dependency outside the workspace: `text-typeset` at `../text-typeset`.

---

## Build & Test Commands

```bash
cargo build                          # Build all crates
cargo test                           # Run all tests (headless, no GPU/display needed)
cargo test -p teksilo-core           # Test a single crate
cargo test -p teksilo-widgets        # Includes layout integration tests
cargo doc --no-deps --open           # Generate docs
cargo run -p <example>               # Run a demo (see examples/ for the list)
cargo run -p <example> --release     # Release mode
```

Demos live under `examples/` (one crate per demo: `simple_button`, `text_and_layout`,
`widget_catalog`, `data_collections`, `data_grid`, `tree_table_view`, `grid_view`,
`drag_and_drop`, `file_drop`, `multi_window`, `recent_projects`, `rich_text_editor`,
`password_field`, `scene_showcase`, `scene_corkboard`, `scene_magnetism`, `docking`,
`native_menu`, `web_view_demo`, `toast_demo`, `async_demo`, `over_constraint`, and others).

Tests are headless: no Xvfb, no GPU, no display server. The GPU glyph path is exercised by
demos, not by headless tests.

---

## Tools

```bash
python3 tools/extract_widget_api.py --list                # List widget files
python3 tools/extract_widget_api.py Button HStack Dialog   # Extract public API + docs
python3 tools/extract_widget_api.py --all                  # Every widget
python3 tools/extract_widget_api.py Button -f json -o out.json
python3 tools/bench_examples.py                            # Benchmarks + report
```

`tools/extract_widget_api.py` parses widget sources in `crates/teksilo-widgets/src/` and emits
the `//!` module header, public `struct`/`enum`/`type`/`const` declarations with `///` docs, and
`pub fn` builder methods from inherent `impl` blocks. It skips `impl Widget for Foo` plumbing and
`pub(crate)` items. Accepts type names or module names.

---

## Coding Conventions (review criteria)

These are the explicit rules the codebase is meant to follow. Flag deviations.

- **Module style:** 2018+ (`mod foo;` with `foo.rs`). No `foo/mod.rs` files; use `foo.rs`
  alongside a `foo/` directory.
- **Builder pattern:** fluent API (`.child()`, `.spacing()`, `.style()`, etc.).
- **Widget trait:** one non-generic `Widget` trait for all widgets; concrete types are erased at
  arena insertion. `build(&mut self)` for composition, `paint()` for rendering.
- **Reactive properties:** `Signal<T>` for mutable state, `Prop<T>` for widget properties
  (static or signal-bound).
- **Event handlers:** attached via `WidgetBuilder` methods (`.on_tap()`, `.on_hover()`,
  `.focusable()`) or a `HandlerSet` inside `build()`.
- **Naming:** snake_case functions, CamelCase types, standard Rust conventions.
- **Dependencies:** centralized in `[workspace.dependencies]`.
- **Error types:** `thiserror` (`#[derive(thiserror::Error)]` with `#[error("...")]` per variant;
  `#[from]` for transparent conversions, `#[source]` for nested chains). Do not hand-roll
  `Display` / `std::error::Error` / `From`.

---

## Crate Layering (factual roles only)

One-line role per crate. Verify actual responsibilities and dependency edges against `Cargo.toml`
files and `use` graphs; the descriptions below are the stated intent, not a guarantee.

```
teksilo-tokens          Pure data: Theme, Color, TextStyle, SpacingTokens, alignment
teksilo-canvas          Canvas API, RenderFrame, Path, Paint, geometry, TextBackend trait
teksilo-core            Widget traits, arena, layout, events, focus, state, gestures, overlays
teksilo-data            Reactive data models (ListModel/TreeModel/etc.); depends on teksilo-core
                        only for Signal<T> + ObserverHandle; GUI-free
teksilo-settings        Persistent reactive prefs: SettingsStore, SettingsFile<T>,
                        Persisted{List,Tree}Model, MruList<T>, WindowStateService
teksilo-telemetry       Product-analytics primitives built on teksilo-settings
teksilo-analytics-*     Telemetry adapters: plausible, teksilo (gRPC), otlp
teksilo-telemetry-codegen   Proc-macro generating typed emit_* fns from a YAML manifest
cargo-teksilo-telemetry-lint   CLI schema-drift linter
teksilo-widgets         Widgets + layout primitives
teksilo-charts          BarChart, LineChart, PieChart; no dep on teksilo-widgets
teksilo-scene           Pannable/zoomable scene viewport; depends on teksilo-widgets
teksilo-text            TextBackend impl via text-typeset (external path dep)
teksilo-i18n            Fluent-rs runtime + locale-aware formatters (ICU4X-backed)
teksilo-i18n-macros     tr! / tr_widget! / tr_signal! proc macros
teksilo-macros          teksu! DSL proc macro
teksilo-render          wgpu renderer: rect/SDF/quad pipelines, atlas upload, path atlas
teksilo-platform        winit + AccessKit adapter; clipboard, OS theme, file dialogs,
                        external DnD, native menu bar
teksilo-app             TeksiloAppBuilder, WindowManager, event loop
teksilo-async           Optional main-thread async executor (off by default)
teksilo-tokio           Tokio reactor adapter for teksilo-async
teksilo-async-std       async-std reactor adapter for teksilo-async
teksilo-webview         Embeddable WebView widget (native OS subview over the wgpu pass)
teksilo                 Umbrella crate: re-exports + feature flags
teksilo-resources       Resource handling and embedding
teksilo-preview         Previewer infrastructure (WidgetCatalog trait, CatalogEntry); no GUI dep
teksilo-preview-ui      Reusable 3-pane previewer GUI
teksilo-widgets-previewer   Bundle binary for the stock catalog
```

Stated dependency flow (verify):
`tokens → canvas → core → data → widgets`, `canvas → text`, `core + data → settings`,
`canvas → render → platform → app`, `settings → app`, `i18n-macros → i18n`,
`core → preview`, `preview-ui → preview + widgets`, `widgets → scene`, `core → webview`,
`(app + core) → async → {tokio, async-std}`.

---

## Public API Contracts (objective)

### Widget trait

```rust
pub trait Widget: std::fmt::Debug + 'static {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> { vec![] }
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse; // required
    fn place_children(&self, _bounds: Rect, _proposal: SizeProposal, _children: &mut [WidgetPlacement], _ctx: &LayoutContext) {}
    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {}
    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
    fn children(&self) -> Vec<WidgetId> { vec![] }
    fn clips_children(&self) -> bool { false }
}
```

`layout_response` is the only required method. Widget categories: Leaf (`layout_response` +
`paint`), Container (`+ place_children + children`), Composing (`build + layout_response`
delegating to a child `+ accessibility`), Hybrid (`build + paint`).

### Layout model

SwiftUI-style negotiation: parent proposes a size, child returns a
`LayoutResponse { size, flex, min, shrink }`, parent distributes the main axis, measures the cross
axis at each child's final main size, then places. All in logical pixels. `Leading`/`Trailing`
(RTL-aware) rather than Left/Right.

- `size`: wanted/ideal size (growth floor).
- `flex`: grow weight for positive slack. Default `0.0` (rigid). `Spacer`/`Expand` return `1.0`.
- `min`: hard compression floor.
- `shrink`: shrink weight for over-constraint deficit. Default `0.0` (rigid). Shrink is opt-in.

`From<Size>` yields fully rigid (`flex = 0`, `shrink = 0`, `min = size`).

### Widget insertion

`tree.add(w)`, `tree.add_child(parent, w)`, `ctx.add(w)`, `ctx.add_boxed(w)`.

### Theming entry points (factual)

`ThemeAppearance::{Light, Dark}` is a required field. Presets `presets::intui::{light, dark}` ship
in `teksilo-core`. There is no `Theme::default()`. Per-widget style traits live in
`teksilo-core/src/styles/`; default `Recipe*Style` impls in `teksilo-widgets/src/styles/`. Apps
install styles per-call (`.style(...)`) or theme-wide (`theme.style_slots.<widget> = ...`).

### App entry point

```rust
fn main() {
    TeksiloAppBuilder::new()
        .theme(intui::light())
        .initial_window(
            WindowConfig::new().title("My App").size(800, 600)
                .root(|tree, _state| tree.add(MyRootWidget::new())),
        )
        .run();
}
```

Every window is described by a `WindowConfig`. There is no `.window_title` / `.window_size` /
`.root` directly on `TeksiloAppBuilder`. Secondary windows open via
`ctx.open_window(WindowConfig::new()...)`. Persistence chains `.app_paths(...)` /
`.application(...)` and `.settings(...)` before `.initial_window(...)`.

### Testing entry points

```rust
let mut tree = WidgetTree::new();
let widget = tree.add(MyWidget::new());
tree.layout(SizeProposal::exact(400.0, 300.0));
assert!((tree.bounds(widget).width - expected).abs() < 0.01);

MockTextBackend::new()          // fixed 8px char width
LayoutContext::for_testing(&theme)
```

Note for review: tests using `MockTextBackend` verify editing/caret/layout logic against fixed
metrics, not real shaping, bidi, or complex-script layout (delegated to `text-typeset`).

---

## Documented Intentional Decisions (verify, do not assume correct)

Listed so a reviewer recognizes deliberate design when reading the code. Each is a claim to
confirm against the implementation and against the project's own requirements, not a reason to
skip scrutiny.

- **No `mod.rs`** (mirrors the convention above): files are `foo.rs` beside `foo/`.
- **Controls are deliberately rigid:** `Button` / `IconButton` / `Badge` / `ComboBox` size to
  content and overflow rather than truncate; only single-line/ellipsis `TextWidget` opts into
  shrink. Excess toolbar actions are meant to overflow into a trailing menu (`Toolbar`).
- **macOS mnemonics:** the Alt+letter menu-mnemonic branch is compiled out on macOS
  (`#[cfg(not(target_os = "macos"))]` in `MenuBarDispatcher::try_handle`) and the underline visual
  is hidden, because macOS rewrites Option+letter for accented input before the app sees it. F10
  and bare-Alt continue to work. Confirm the cfg gates and that no equivalent path is lost.
- **X11 window decorations:** custom title bar is supported on Wayland/macOS/Windows; X11 falls
  back to native decorations.
- **Async executor:** `teksilo-async` is opt-in and off by default; `teksilo-app` stays
  runtime-free (the `on_loop_tick` hook + `AsyncCompletionHandle` types are async-agnostic). Check
  there is no implicit tokio/async-std dependency in the core path.
- **Data layer is GUI-free:** `teksilo-data` depends on `teksilo-core` only for `Signal<T>` +
  `ObserverHandle`. Confirm no `teksilo-widgets` leakage.

---

## Navigation: Key Files

Core:
- Widget trait: `crates/teksilo-core/src/widget.rs`
- Signal/Prop: `crates/teksilo-core/src/signal.rs`
- BuildContext: `crates/teksilo-core/src/build_context.rs`
- Event handlers / types: `crates/teksilo-core/src/event_handlers.rs`, `event.rs`
- WidgetBuilder: `crates/teksilo-core/src/widget_builder.rs`
- Arena: `crates/teksilo-core/src/arena.rs`
- Widget tree orchestrator: `crates/teksilo-core/src/widget_tree.rs`
- State: `crates/teksilo-core/src/state.rs`
- Accessibility: `crates/teksilo-core/src/accessibility.rs`
- Animation: `crates/teksilo-core/src/animation.rs`, `animated_quad.rs`, `frame_tick_scheduler.rs`, `motion_visibility.rs`
- Theme/styling: `crates/teksilo-core/src/styles/` (+ preset `presets/intui.rs`); default impls `crates/teksilo-widgets/src/styles/`
- Actions/Intents/Shortcuts: `crates/teksilo-core/src/{action,intent,shortcut}.rs`; `IntentKind` derive in `crates/teksilo-macros/src/intent_kind.rs`
- Drag-and-drop core: `crates/teksilo-core/src/drag_payload.rs`, `drag_state.rs`, `widget_tree/drag_drop_impl.rs`

Widgets / primitives:
- Reference widget (Button): `crates/teksilo-widgets/src/button.rs`
- Layout primitives: `crates/teksilo-widgets/src/primitives/`
- Data models: `crates/teksilo-data/src/` (`list_model.rs`, `tree_model.rs`, `tree_slice.rs`, `selection_model.rs`, `sort_filter_*.rs`, `checked_model.rs`, `tree_checked_model.rs`, `check_state.rs`)
- Standard row items: `crates/teksilo-widgets/src/standard_item.rs`
- TableView: `crates/teksilo-widgets/src/table_view.rs` + `table_view/`
- TreeTableView: `crates/teksilo-widgets/src/tree_table_view.rs`
- GridView: `crates/teksilo-widgets/src/` (grid view + `primitives/`)
- Toast/notifications: `crates/teksilo-widgets/src/toast.rs` + `toast/`, `notification.rs` + `notification/`
- Menus (native + in-window): `crates/teksilo-widgets/src/menu.rs` + `menu/`, `menu_bar.rs`
- Text input: `crates/teksilo-widgets/src/text_input.rs`, `primitives/text_input_field.rs`
- Rich text: `crates/teksilo-widgets/src/rich_text/`

Platform / app / render:
- Renderer: `crates/teksilo-render/src/renderer.rs`
- Canvas API: `crates/teksilo-canvas/src/canvas.rs`
- App builder: `crates/teksilo-app/src/app.rs`
- File dialogs: `crates/teksilo-platform/src/file_dialog.rs`
- External (OS) DnD: `crates/teksilo-platform/src/external_dnd.rs` + `external_dnd/macos.rs`
- Native menu backend: `crates/teksilo-platform/src/native_menu.rs` + `native_menu/macos.rs`
- Clipboard / OS theme: `crates/teksilo-platform/src/clipboard.rs`, `os_theme.rs`
- Title bar hosts: `crates/teksilo-platform/src/title_bar_host/` (wayland/x11/windows/macos)
- WebView: `crates/teksilo-webview/src/`
- Scene viewport: `crates/teksilo-scene/src/`

Macros / i18n / previewer:
- teksu! DSL macro: `crates/teksilo-macros/src/`; trybuild fixtures in `crates/teksilo/tests/teksilo/pass/`
- i18n runtime + formatting: `crates/teksilo-i18n/src/` (`manager.rs`, `localized_string.rs`, `format.rs`)
- i18n macros: `crates/teksilo-i18n-macros/src/lib.rs`
- Previewer: `crates/teksilo-preview/src/`, `crates/teksilo-preview-ui/src/`, `crates/teksilo-widgets-previewer/src/main.rs`
- Workspace config: `Cargo.toml`

---

## What was removed, and why

So the review is not anchored, the following were intentionally dropped from the full `CLAUDE.md`:

- **Correctness assertions** (for example "prevents the classic reactive deadlock", "compliant by
  construction", "everyone gets this subtly wrong, done right here"). These are conclusions the
  review must reach independently from the code.
- **Coverage-as-adequacy** statements (test counts framed as proof of quality).
- **Complete / Partial / Not Started** status taxonomy, which tends to suppress scrutiny of
  anything labeled done and hides whatever the author did not think to list.
- **Feature tours and marketing narration** (extended descriptions of scene magnetism, grid
  strategies, etc.), which consume context without giving checkable rules.

If you want the design rationale for a specific subsystem during review, read its `docs/*.md`
reference directly rather than relying on summary claims.
