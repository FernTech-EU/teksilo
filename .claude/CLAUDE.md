# Teksilo — Claude Code Reference

## Project Overview

Teksilo is a pure-Rust GUI framework for serious desktop applications. Architecture: retained widget tree with SwiftUI-style layout, AccessKit accessibility, wgpu rendering.

- **License:** MPL2.0 — Copyright (c) 2026-2026 FernTech, all rights reserved
- **Rust edition:** 2024 (resolver 3)
- **Author:** Cyril Jacquet

## Build Commands

```bash
cargo build                                    # Build all crates
cargo test                                     # Run all tests (headless, no GPU needed)
cargo run -p simple-button                     # Milestone 1 demo
cargo run -p text-and-layout                   # Milestone 2 demo
cargo run -p text-and-layout --release         # Release mode (much faster rendering)
cargo run -p widget-catalog                    # Browse all available widgets
cargo test -p teksilo-core                        # Test a specific crate
cargo test -p teksilo-widgets                     # Includes layout integration tests
cargo doc --no-deps --open                     # Generate docs
cargo run -p drag-and-drop                      # Drag-and-drop showcase (in-view reorder + cross-widget export: Library ListView .exportable(Copy) → Playlist .accept_foreign_rows → Trash DropTarget; plus a multi-zone DropTarget: leading=play-next / centre=add / trailing=favourite, per-zone hints)
cargo run -p multi_window                      # Multi-window demo
cargo run -p recent_projects                   # MRU/persistence demo
cargo run -p rich_text_editor                  # Rich text editing
cargo run -p rich_text_viewer                  # Rich text viewing
cargo run -p code_editor                       # CodeEditor: gutter, current-line band, bracket auto-close/match, Ctrl+/ comment, smart Tab, Ctrl+D multi-caret, Alt+arrows move-line, injected language-agnostic highlighter + completion. Also PlainTextEditor. See docs/code-editor.md
cargo run -p log_view                          # LogView: read-only append-only tail-following streaming view — windowed layout (only the visible rows shaped, ~4 MB flat at 100k+ lines), derived follow-tail, scrollback cap, injected per-line severity colour, windowed a11y. See docs/log-view.md
cargo run -p font-picker                       # FontPicker: list/search/filter all installed fonts (per-row in-font samples, script + monospaced filters), live preview
cargo run -p spin_box                          # Numeric input demo
cargo run -p password-field                    # Secure/password input (reveal toggle, masking, caps-lock)
cargo run -p tool_box                          # Tool box widget demo
cargo run -p teksilo-widgets-previewer            # Widget catalog previewer
cargo run -p data-grid                          # TableView showcase (1k rows × 7 cols, auto-measured variable row heights; --uniform for the fixed-height fast path)
cargo run -p tree-table-view                         # TreeTableView showcase (mock filesystem, auto-measured variable row heights)
cargo run -p data-collections                   # Repeater / ListView / TreeView showcase (exact + auto-measured variable row heights)
cargo run -p grid-view                          # GridView: virtualized 2D tile grid (adaptive, marquee, reorder, sections, sticky headers)
cargo run -p datetime-pickers                   # Calendar / DateEdit / TimeEdit / DateTimeEdit gallery
cargo run -p file-dialogs                       # Native file open / save / pick-folder showcase
cargo run -p file-drop                          # External (OS) drag-and-drop: DropZone showcase (files / text / URLs)
cargo run -p tooltips-showcase                  # Three-tier tooltip cascade demo (plain / rich / composite)
cargo run -p scene_showcase                     # Scene viewport: pan/zoom + heavyweight+lightweight tier mix
cargo run -p scene_corkboard                    # Scene-based story corkboard (worked-example use case)
cargo run -p scene-magnetism                    # Scene magnetism: node-graph with typed snap-and-connect (item-drag-snap / port-drag wires / keyboard connect), persistent PathItem wires
cargo run -p chart-demo                         # BarChart / LineChart / PieChart (+ donut + center slot); planned redesign: gradient-vs-flat ChartStyle toggle, dashed grid, interactive legend, live strip-chart via ChartWindow — see docs/charts.md §14
cargo run -p toast-demo                         # Toast notifications + persistent NotificationLog + bell button + dialog
cargo run -p async-demo                         # teksilo-async: spawn_local + spawn_blocking + spawn_local_with (opt-in async executor)
cargo run -p tab-migration                      # Cross-TabWidget tab drag-and-drop (migrate tabs between two groups)
cargo run -p over-constraint                     # Graceful shrink / layout priority / height-for-width + inspector overflow stripes (F12)
cargo run -p collapsible-menu-bar                # MenuBar hamburger mode: responsive collapse + reveal-trailing-the-button + Alt/F10 keyboard reveal
cargo run -p native-menu                         # Native OS menu bar: one MenuModel → in-window MenuBar + macOS NSMenu (focus-follows-window, reactive checks, ⌘ key equivalents)
cargo run -p web-view-demo                       # Embedded WebView (wry by default) in a TabWidget: dormancy/visibility bridge + JS↔Rust IPC; --features servo adds the Wayland engine. Linux needs WebKitGTK dev deps — see docs/web-view.md "Linux build dependencies" (apt: libgtk-3-dev libjavascriptcoregtk-4.1-dev libwebkit2gtk-4.1-dev …)
cargo run -p docking                             # DockingLayout: VS Code-style dockable IDE shell — centre editor + 4 collapsible/splittable/draggable sides, activity rail, per-corner ownership, drag-to-dock, Lock-layout toggle (DockPolicy) + disable-side, export/restore. See docs/docking.md
cargo run -p close-confirmation                  # Window close / quit confirmation: per-window WindowConfig::on_close_requested guard (CloseResponse::Veto + close_window_forced re-issue) + can_close/on_close_blocked sugar. See docs/multi-window.md "Intercepting close / quit"
cargo run -p terminal-demo                       # Terminal (Console): a real shell over a PTY (ConPTY/openpty), VT emulation by alacritty_terminal, view by Teksilo — colored output, cursor, scrollback, selection, Ctrl+C/Ctrl+Shift+C-V, mouse reporting, Role::Terminal a11y. Behind the umbrella `terminal` feature. See docs/terminal.md
```

## Tools

```bash
python3 tools/extract_widget_api.py --list                 # List all widget files
python3 tools/extract_widget_api.py Button HStack Dialog   # Extract public API + docs for widgets
python3 tools/extract_widget_api.py --all                  # Every widget
python3 tools/extract_widget_api.py Button -f json -o out.json   # JSON for tooling
python3 tools/extract_widget_api.py --all --md-dir docs/widgets  # Regenerate the mdBook Widget Catalog
python3 tools/bench_examples.py                          # Run benchmarks with report generation
```

[tools/extract_widget_api.py](tools/extract_widget_api.py) parses widget source files in [crates/teksilo-widgets/src/](crates/teksilo-widgets/src/) and emits their `//!` module header, `pub struct`/`enum`/`type`/`const` declarations with `///` docs, and `pub fn` builder methods from inherent `impl Foo { ... }` blocks. Skips `impl Widget for Foo` trait plumbing and `pub(crate)` items. Accepts type names (`Button`) or module names (`button`); flags `#[doc(hidden)]` and `#[cfg(...)]`. Use when reading a widget's public surface without opening the file, packing widget docs into LLM context, or auditing API coverage.

## Documentation site (mdBook + rustdoc)

The online docs are a **static site published to Cloudflare Pages** (`teksilo-docs.pages.dev`) by [.github/workflows/docs.yml](.github/workflows/docs.yml): [mdBook](https://rust-lang.github.io/mdBook/) renders the curated guides under [docs/](docs/) plus a generated **Widget Catalog** at the site root, and `cargo doc` renders the deep API reference under `/api/` (each catalog page deep-links to its widget's rustdoc module page). Build it locally with:

```bash
cargo install mdbook                                                       # once
cargo doc -p teksilo-widgets --no-deps                                     # the /api reference (build first)
python3 tools/extract_widget_api.py --all --md-dir docs/widgets --api-dir target/doc  # catalog + patch SUMMARY
python3 tools/fix_book_links.py docs                                       # ../crates → github.com/ferntech-eu links
mdbook serve --open                                                        # live preview, or `mdbook build` → ./book
```

- The generated **Widget Catalog** pages under `docs/widgets/*.md` and the populated `<!-- BEGIN/END GENERATED WIDGETS -->` region of `docs/SUMMARY.md` **are committed** (so the book + its widget sidebar build on any checkout); regenerate with the `--md-dir` command above. Only `book/` (build output) and `docs/widgets/img/` are gitignored.
- [tools/fix_book_links.py](tools/fix_book_links.py) rewrites the guides' repo-relative source links (`../crates/…`) to absolute `github.com/ferntech-eu/teksilo` URLs and neutralizes rustdoc intra-doc links, so nothing 404s in the rendered book. It mutates the guide `*.md` **in place** (a build step — CI runs it on the runner; after a local preview run `git checkout -- $(git diff --name-only docs/*.md | grep -v SUMMARY)` to drop the guide edits). It is a no-op on the catalog pages (they carry only `../api/` links).
- Smoke tests: `python3 tools/extract_widget_api.py --test` and `python3 tools/fix_book_links.py --test`. The Cloudflare deploy needs a Pages project named `teksilo-docs` + `CLOUDFLARE_API_TOKEN`/`CLOUDFLARE_ACCOUNT_ID` secrets.

The workspace has two member globs: `crates/*` for libraries and `examples/*` for runnable demos. Examples live under [examples/](examples/) (e.g. `simple_button`, `text_and_layout`, `widget_catalog`, `data_collections`, `dialogs_and_popovers`, `menus_and_dropdowns`, `splitter`, `tab_widget`, `title_bar_demo`, `internationalization`, `shortcuts_demo`, `recent_projects`, `drag_and_drop`, `multi_window`, `rich_text_editor`, `rich_text_viewer`, `spin_box`, `tool_box`).

## Widget Previewer

Storybook-style 3-pane (navigator / canvas / knob-form) explorer for the entire widget catalog. Run with `cargo run -p teksilo-widgets-previewer`. Widgets self-register via `inventory::submit!(&'static dyn CatalogEntry)` — no central registry file; adding a previewable widget is a single submission. Each entry declares typed `KnobSpec`s (live-editable properties) and `PreviewVariant`s (Default / Disabled / Loading / Error / etc.); the UI generates an editing form and a multi-variant canvas. Built-in PNG export (`png_export.rs`) per widget — useful for design review, marketing assets, and visual regression testing. Two CLI modes: standalone (whole catalog, default) and targeted (preview just one widget for fast iteration). Architecture is split deliberately: `teksilo-preview` is the trait + registry crate with no GUI dep — third-party widget libraries implement `WidgetCatalog` without depending on the previewer GUI; `teksilo-preview-ui` is the reusable GUI library so apps can build their own previewer binaries for app-specific widget catalogs; `teksilo-widgets-previewer` is the bundle binary for the stock catalog. Mode C (VS Code extension with CodeLens "Preview ▶" buttons inline in source) is designed but deferred.

Tests are fully headless — no Xvfb, no GPU, no display server needed.

## Coding Conventions

- **Module declarations:** Use 2018+ style (`mod foo;` with `foo.rs`), NOT `foo/mod.rs`
- **Builder pattern:** Fluent API throughout — `.child()`, `.spacing()`, `.style()`, etc.
- **Type erasure:** Non-generic Widget trait — concrete types erased at arena insertion
- **Unified Widget trait:** One trait for all widgets. `build(&mut self)` for composition, `paint()` for rendering.
- **Reactive properties:** `Signal<T>` for mutable state, `Prop<T>` for widget properties (static or signal-bound)
- **Event handlers:** Attached via `WidgetBuilder` methods (`.on_tap()`, `.on_hover()`, `.focusable()`) or `HandlerSet` in `build()`
- **Naming:** snake_case functions, CamelCase types, standard Rust conventions
- **Dependencies:** Centralized in workspace `[workspace.dependencies]`
- **No `mod.rs` files** — always use `foo.rs` alongside `foo/` directory
- **Error types:** Use `thiserror` (workspace dep) — `#[derive(thiserror::Error)]` with `#[error("...")]` per variant; `#[from]` for transparent conversions, `#[source]` for nested error chains. Don't hand-roll `Display` / `std::error::Error` / `From`.

## Crate Architecture

```
teksilo-tokens          Pure data: Theme, Color, TextStyle, SpacingTokens, alignment
teksilo-canvas          Canvas API, RenderFrame, Path, Paint, geometry, TextBackend trait
teksilo-core            Widget traits, arena, layout, events, focus, state, gestures, overlays
teksilo-data            Reactive data models, designed as a *peer* of the GUI, not part of it: depends on
                     teksilo-core only for `Signal<T>` + `ObserverHandle`, so a CLI tool, validation pass,
                     Qleany ViewModel layer, or headless test can share a `ListModel<Project>` with a
                     `ListView` without pulling in the renderer. All handles are `Rc<RefCell<…>>` under
                     the hood — `.clone()` = share-by-handle, not deep-copy. Mutation methods are
                     written to drop the mutable borrow *before* notifying observers (intended to
                     avoid a re-entrant borrow on the notify path — verify in the code if it
                     matters). Concrete generic typing throughout — `ListModel<T>` / `TreeModel<T>` give
                     `&T` directly to delegates, no `QVariant`, no role integers.
                       • `ListModel<T>` / `ListDataSource` (escape hatch for huge / external sources).
                       • `TreeModel<T>` — tree *shape* only.
                       • `TreeSlice<T>` — **per-view** flattening + expand state. Two `TreeView`s on the
                         same `TreeModel` have independent expand state, so dual-pane file managers,
                         overview panes, and side-panel search results are one-line each. Slice exposes
                         `version: Signal<u64>` + `with_entry(idx, |t, FlatEntry| …)`.
                       • `TreeDataSlice<K, T>` — the same per-view flatten + expand + divergence, but
                         over an **external** indent-ordered source (Qleany entity store, DB, VFS)
                         instead of a `TreeModel`. Feed `Vec<TreeRow{key,item,depth}>` on each (re)load
                         via `set_source`; the engine derives the tree from the indent depth, keyed by
                         your domain `K` (so expand + keyed selection survive a full re-source — unlike
                         a `TreeModel` mirror whose `NodeId`s churn on rebuild). Implements
                         `TreeDataSource`; domain policy injected as closures (`set_reorder` /
                         `set_drag_policy` / `set_drop_resolver`, the resolver gets the target's item so
                         it needn't capture the slice). `T: PartialEq` (divergence compares content).
                         Ready-made counterpart to hand-rolling `TreeDataSource` for an outline. See
                         docs/data-models.md §4.4.
                       • `SelectionModel` — shared by ListView/TreeView, `Single`/`Multi`/`None` modes,
                         `selection_signal(): Signal<BTreeSet<usize>>`, anchor for Shift+click range.
                       • `SortFilterListModel<T>` — sort + filter projection over a flat source.
                       • Divergence side-channel: `TreeSlice` / `TreeDataSlice` / `SortFilterTreeModel` /
                         `SortFilterListModel` expose `first_changed_index() -> Option<usize>` —
                         the first visible index whose content may differ after the latest
                         rebuild (read synchronously from a change observer). Lets consumers
                         caching per-row state (e.g. measured row heights) keep the valid
                         prefix across the proxies' blanket Resets / reflattens. See
                         docs/data-models.md §13.
                       • `SortFilterTreeModel<T>` — same for trees, with three first-class strategies
                         via `TreeFilterMode`: `HideNonMatching` | `KeepAncestors` (show the path to
                         each match — VS Code / Spotlight behaviour) | `KeepDescendants` (match a node,
                         show its subtree). Wraps a concrete `TreeModel` (owns its own expand state).
                       • `TreeRowFilter<K, T>` — the sort/filter stage for the `TreeDataSlice` pipeline
                         (external trees): a pure `Vec<TreeRow>` → `Vec<TreeRow>` transform (same three
                         `TreeFilterMode` strategies + per-parent sibling sort) wired *below* the slice
                         via `set_source`, so there's one projection — not two stacked expand states like
                         a `SortFilterTreeModel`-over-`TreeDataSlice` would be. `KeepDescendants` surfaces
                         a matching subtree even under non-matching ancestors (intent-correct; differs
                         from `SortFilterTreeModel`'s flatten). It reshapes rows only, NOT expand state,
                         so while filtering call `slice.set_all_expanded(true)` (a reveal override that
                         preserves the persistent per-view expand set) so `KeepAncestors`' kept ancestors
                         don't hide the matches; `false` restores. See docs/data-models.md §4.5.
                       • `CheckedModel` + `TreeCheckedModel<T>` — per-row checkbox state parallel to
                         `SelectionModel`. Tree variant aggregates **descendant → ancestor**: 3 of 5
                         children checked = parent `Indeterminate`; all 5 = parent `Checked`. The
                         "Permissions" / "select files to back up" tristate pattern, handled in the
                         data layer rather than re-implemented per call site.
                       • `KeyedTreeCheckedModel<K>` — the domain-keyed twin of `TreeCheckedModel` (the
                         checkbox counterpart of `KeyedSelectionModel`): tristate tree checkboxes over a
                         `TreeDataSlice` / any `TreeDataSource`, tree shape injected as `children`/`parent`
                         closures (`from_source(slice.clone())`). State survives a re-source (keyed by
                         domain id); `prune_missing(exists)` drops deleted nodes AND re-aggregates the
                         ancestors they affected, `reaggregate()` re-derives all parents from the new
                         shape. See docs/data-models.md §6.1.
                       • `CheckState` (`Unchecked` / `Checked` / `Indeterminate`).
                       • `ChartModel<T>` — reactive multi-series chart data model for teksilo-charts
                         (BarChart/LineChart/PieChart): `SlotMap` arena keyed by `SeriesId` (stable,
                         like `NodeId`) + `order: Vec<SeriesId>`, mutate-then-notify, emits
                         `ChartChange` (series add/remove/move/rename/recolor/show-hide, point
                         push/insert/remove/update/replace) to observers and bumps one of two
                         `Signal<u64>`s — `structure_version()` (everything except color; bind
                         `Relayout`) / `style_version()` (`SeriesColorChanged` only; bind
                         `RepaintOnly`). `ChartSeries<T>`/`ChartDatum<T>` are its construction DTOs
                         (re-exported from `teksilo_charts`); `ChartSeries::visible` is a plain
                         `bool` — reactivity lives in the model's version signals, not the DTO.
                         `ChartWindow<T>` — last-N-points streaming projection (strip charts),
                         translates `ChartChange` incrementally instead of collapsing to `Reset`.
                         `ChartAggregate<T>` + `ChartAggregateFn` (Mean/Sum/Min/Max/First/Last/Custom)
                         — bucket/rollup projection (downsample a long series for display).
                         `ChartSelection` — the chart counterpart of `SelectionModel`, keyed
                         `HashSet<(SeriesId, usize)>` (not `BTreeSet`: `SeriesId` isn't `Ord`).
                         See docs/data-models.md §15.
                       • `debug_registry.rs` — opt-in registration for the Inspector's Models tab via
                         `ListModel::debug_named("…")` / `TreeModel::debug_named` /
                         `SelectionModel::debug_named`.
                     Reference: [docs/data-models.md](docs/data-models.md).
teksilo-settings        Persistent reactive prefs: SettingsStore (dotted-key Signal<T>), SettingsFile<T>,
                     PersistedListModel/PersistedTreeModel, MruList<T: MruEntry>, WindowStateService
teksilo-telemetry       Privacy-respecting product analytics built on teksilo-settings: ConsentStore,
                     InstallId, TelemetryBundle, recent-log ring buffer. Designed with GDPR/RGPD
                     constraints in mind (opt-in consent, anonymisation) — compliance is not
                     self-certified; assess against the actual requirements. Reference: docs/telemetry.md.
teksilo-analytics-plausible  Plausible adapter (anonymous mode). HTTP + retry/backoff + redb queue.
teksilo-analytics-native  Home-grown gRPC adapter for the Teksilo-operated teksilo-collector backend.
                     Anonymous + pseudonymous modes; bearer token + TLS; fetch + erase wired.
teksilo-analytics-otlp  OTLP/HTTP-logs adapter. Maps Teksilo events to OTLP LogRecords; worker
                     thread with batching, exponential backoff, flush-on-shutdown.
teksilo-telemetry-codegen  Proc-macro: `include_telemetry_schema!("events.yaml")` reads a YAML
                     manifest at compile time and expands to typed `emit_*` functions + enum
                     types. Validates required fields, prop types, enum variants, expiry dates.
cargo-teksilo-telemetry-lint  CLI schema-drift linter. Checks expiry, required fields, unused
                     events (declared but not emitted in src/), unknown prop types. Run as
                     `cargo teksilo-telemetry-lint`. CI mode: `--fail-on-warnings`.
teksilo-widgets         ~56 widgets + ~21 layout primitives (Button, ListView, TreeView, TableView,
                     TreeTableView, MenuBar, Dialog, TextInput, SpinBox, etc.)
teksilo-charts          BarChart, LineChart, PieChart (pie + donut, with center slot). Sits at the same tier
                     as teksilo-widgets — no dep on widgets. Series data is a `ChartModel<T>`
                     (teksilo-data, see above) — `Bar/Line/PieChart::new(model)`, no `Prop<Vec<…>>`
                     binding path. Tier-3 `ChartStyle` trait (teksilo-core::styles; 4 all-recipe
                     methods, no `make_*→WidgetId`) — its default `RecipeChartStyle` lives in THIS
                     crate, not teksilo-widgets/src/styles/* (the one styling exception — charts
                     don't depend on widgets), resolved `.style(...)` → `style_slots.chart` →
                     `RecipeChartStyle::default()`. Dashed/dotted gridlines (`AxisConfig::gridline_dash`
                     wins over the style's `BorderRecipe`) + gradient area/donut fills (`FillRecipe::
                     Linear/RadialGradient` via a custom `ChartStyle` + `Canvas::fill_path`'s
                     gradient-capable `Paint`). Per-datum `Role::GraphicsObject` a11y nodes
                     (`SyntheticKind::ChartMark`, name+numeric value) on every visible bar/point/slice.
                     Interactive legend (`.legend_interactive(true)` on Bar/Line, `ChartLegend::
                     interactive(true)`) toggles `ChartModel::set_series_visible` directly. BarChart
                     now has hover tooltips (parity with Line/Pie). Reference: docs/charts.md.
teksilo-scene           Pannable/zoomable scene viewport (Qt QGraphicsScene equivalent). Two-tier content
                     under one view transform: heavyweight `Widget`s placed at scene coordinates (focus,
                     animation, DnD, AT all survive embedding) + lightweight `SceneItem`s (paint-only,
                     no arena overhead, thousands cheap). Exact-shape hit-test,
                     per-item GPU cache, collision API, reactive `item_change_signal`. A standalone
                     `SceneMinimap` widget (a scene thumbnail + live viewport rect) is provided but
                     NOT auto-embedded — the app places it next to the view, fed by
                     `view.viewport_in_scene_signal()` + `Scene::item_thumbnails()` (both tiers). Demo
                     in `scene_showcase` (bottom-trailing overlay). Full a11y:
                     synthetic AT nodes per lightweight item with screen-projected bounds, rotor
                     categories, reparenting (visual tree ≠ AT tree), landmark roles, live regions.
                     **Shared model & multi-view**: `SceneModel` is a cloneable `Rc<RefCell<Scene>>`
                     handle (the `ListModel` pattern) — clone it into several `SceneView::with_model`
                     panes to render one scene many ways (overview+detail, same-doc multi-window,
                     headless reuse). Heavyweight content is stored as a type-erased payload via
                     `add_widget_item(payload, rect)` and each view builds its OWN instance through a
                     per-view delegate (`delegate_typed::<P>(|&P, ItemId| Box<dyn Widget>)`); mutate the
                     model once (`&self` mutators) and every view reconciles. `set_payload(id, p)`
                     rebuilds an item across views. Single-view `Scene::add_widget(w, rect)` (the `Once`
                     path) is kept as sugar. Selection is per-view by default; pass a shared
                     `SceneSelection` via `.selection_model(..)` to sync panes (reactive — cards bind the
                     selection signal, no rebuild).
                     **Magnetism**: typed snap-and-connect between anchor points ("magnets") on items.
                     Mechanism in scene, policy in the consumer: scene owns geometry, broad-phase, snap
                     math, feedback, the accept/reject predicate hook, and the connection event; the
                     consumer's predicate decides compatibility and `on_connect` decides meaning (scene
                     stores no connection state). `Magnet` = a local point + `MagnetRole` (advisory
                     Source/Target/Bidirectional) + type-erased payload. Three input methods, one
                     mechanism: item-drag-snap (mouse, lightweight items), port-drag wires (grab a
                     magnet handle), and a keyboard connect flow (`m` → arrows/Enter, any item kind);
                     reusable `SceneModel::compute_item_snap`/`compute_port_snap` serve heavyweight
                     consumers that drive their own drag. Synthetic `SceneMagnet` AT nodes +
                     roving `active_descendant`. `SceneView::magnetism(MagnetismConfig)`; demo
                     `scene_magnetism`.
                     **Item colours & theming**: lightweight items' fill/stroke/foreground fields are
                     `ColorProp`s (plain `Color` / theme role / `Signal<Color>` / `Signal<Role>`),
                     resolved at paint via `prop.resolve(ctx.theme, ctx.enabled)` against a new
                     `theme: &Theme` + `window_active: bool` + `enabled: bool` threaded onto
                     `SceneItemPaintContext` (mirrors the widget-tier `PaintContext`) — so a role fill
                     auto-desaturates on window blur with zero per-item code. Build-time `Signal`/role
                     colours are continuously reactive (`register_bindings` at `RepaintOnly`); live
                     `SceneModel::set_item_fill`/`clear_item_fill`/`set_item_stroke`/`clear_item_stroke`
                     mutate a mounted item's colour and emit `ItemChange::AppearanceChanged`
                     (repaint-only — evicts the item's paint cache, never relayout/rebuild). `RectItem`
                     gained `corner_radius` (`GroupItem` already had it); `RectItem`/`PathItem`/
                     `GroupItem` gained `stroke_styled(color, StrokeStyle)` for dashed/dotted/custom
                     strokes; `TextItem` gained `align(TextAlign::{Leading,Center,Trailing})`,
                     `rotation(radians)`, and `measure(&mut dyn TextBackend) -> Size`. Per-item a11y
                     overrides gained `access_value`/`access_numeric_value`/`access_numeric_range`/
                     `access_numeric_step` for gauge/value-mark-like items.
                     `SceneListAdapter<T>` (`from_model`/`from_source`, in `scene_list_adapter.rs`)
                     keeps a run of lightweight items in sync with a `ListModel<T>` /
                     `ListDataSource<Item = T>` via a `Fn(&T, usize) -> Box<dyn SceneItem>` delegate,
                     reconciling `DataChange` (rebuild-all on structural change, single-item rebuild on
                     `ItemUpdated`) — the data-adapter analogue of `ListView` for the lightweight tier.
                     Use cases: story corkboards, mind maps, node-graph editors, timeline views, CAD
                     canvases, simple maps. Sits at the teksilo-widgets tier; depends on widgets so the
                     heavyweight tier can be any widget in the catalog. See docs/teksilo-scene.md +
                     docs/teksilo-scene-a11y.md.
teksilo-text            TextBackend impl via text-typeset (external path dep)
teksilo-i18n            Fluent-rs runtime: LocalizedString, I18nManager, locale resolution, file watcher.
                     Also locale-aware formatters: NumberFormatter / TeksiloDateTimeFormatter
                     (Signal<T> → Signal<String>), TeksiloDateTime, plus a custom DATETIME() Fluent
                     function and a `bundle.set_formatter` callback so `{ NUMBER(...) }` and
                     `{ DATETIME(...) }` inside .ftl messages render correctly across locales.
                     Built on icu_decimal + icu_datetime + icu_calendar + intl-memoizer.
teksilo-i18n-macros     Compile-time tr! / tr_widget! proc macros (re-exported by teksilo-i18n).
                     Also tr_signal! / tr_signal_widget! — reactive variants that accept
                     Signal<T> args and return Signal<String> re-rendering on (any arg ∪
                     locale ∪ hot-reload) change.
teksilo-macros       teksu! DSL proc macro (re-exported by teksilo as teksu!)
teksilo-render          wgpu renderer: rect/SDF/quad pipelines, atlas upload, path atlas
teksilo-platform        winit + AccessKit adapter, event translation, clipboard, OS theme,
                     native file dialogs (FileDialogBackend trait + RfdAsyncBackend),
                     external (OS) drag-and-drop (ExternalDndBackend trait + per-OS backends),
                     native (OS) menu bar (NativeMenuBackend trait + macOS NSMenu backend; behind the `native-menu` feature)
teksilo-app             TeksiloAppBuilder, WindowManager, event loop. Exposes an async-agnostic
                     `on_loop_tick(poll_source, FnMut() -> bool)` hook + routes `AsyncCompletionPayload`
                     (a teksilo-core type) to a window's tree — the only core touch-points the
                     optional async executor needs.
teksilo-async           Optional main-thread async executor (opt-in, OFF by default). `spawn_local` /
                     `spawn_local_with` / `spawn_blocking`, driven by teksilo-app's `on_loop_tick`
                     hook; runtime-free core (no tokio/async-std). `!Send` futures capture `Signal`s
                     and mutate them on resume (the owned-handles model); `spawn_local_with` delivers
                     a result with a fresh `EventContext`. The completion router
                     (`AsyncCompletionHandle` + `AsyncCompletionPayload`) lives in teksilo-core so
                     delivery routes through teksilo-app without a dependency cycle. See docs/async.md.
teksilo-tokio           Thin reactor adapter: `install_async_tokio()` + `TokioHandle`. Wraps each
                     executor tick in a Tokio runtime context so native Tokio futures
                     (timers/sockets/reqwest) can be `.await`-ed inside `spawn_local` bodies.
teksilo-async-std       Thin reactor adapter: `install_async_async_std()`; async-std's global reactor
                     needs no per-tick guard.
teksilo-webview         Embeddable `WebView` widget. The one widget that can't render into the
                     wgpu surface — every engine (WKWebView / WebView2 / WebKitGTK / Servo) is a
                     native OS subview on top of the wgpu pass. Mirrors the platform-backend
                     pattern: swappable `WebViewBackend` → `WebViewHandle`, per-app
                     `WebViewRegistry` (app-state) routes JS→Rust / lifecycle events. Engine is
                     feature-gated (`wry-backend` default-on via umbrella `web-view`;
                     `servo-backend` additive for Wayland; `MemoryWebViewBackend` / `NoopWebViewBackend`
                     for tests / no-engine). Sits at the teksilo-widgets tier; depends only on
                     teksilo-core (the widget is self-contained — apps that don't embed web pay zero
                     compile time). See docs/web-view.md.
teksilo-terminal        Embeddable terminal-emulator (`Terminal` / Console) widget. Teksilo owns the
                     *view* (grid render, keyboard→byte encoding, mouse reporting, selection,
                     `Role::Terminal` a11y, theming, lifecycle); the PTY + VT model are delegated to
                     `portable-pty` (ConPTY/openpty) + `alacritty_terminal` behind the
                     `TerminalEngine` trait (default backend feature `alacritty`; `MemoryEngine` for
                     tests). Unlike webview it renders INTO the wgpu surface (full a11y/theming, no
                     native subview). `TerminalController` (Weak handle, the ListModel pattern) drives
                     it from anywhere + exposes reactive `title/cwd/child_running/columns/rows/…`
                     signals; `ColorScheme` (16 ANSI + truecolor); Tier-3 `TerminalStyle`. Off by
                     default (umbrella `terminal` feature). Introduced two reusable framework
                     primitives: `WidgetBuilder::keyboard_capture(bool)` (a focused surface receives
                     every KeyDown raw, bypassing shortcut resolution — so Ctrl+C reaches the child)
                     and `RepaintWindowRequest { window_id }` (a thread-safe, teksilo-app-routed
                     "repaint this window" for content changed off the UI thread — the PTY reader
                     thread; the off-thread analogue of `ctx.request_frame()`). See docs/terminal.md.
teksilo              Umbrella crate with re-exports and feature flags
teksilo-resources       Resource handling and embedding infrastructure
teksilo-preview         Storybook-equivalent infrastructure for desktop Rust widgets. `WidgetCatalog`
                     trait + object-safe `CatalogEntry` collected via `inventory`. Typed
                     `KnobSpec`/`KnobValue`/`KnobOverrides` for live property editing, `PreviewVariant`
                     enum for multi-state showcasing (Default/Disabled/Loading/Error/...), `SourceLoc`
                     for "open in editor" navigation. Zero GUI dep — third-party widget libraries can
                     implement the trait and stay independent of the previewer UI.
teksilo-preview-ui      Reusable 3-pane previewer GUI (navigator + canvas + knob-form, plus toolbar,
                     inspector pane, CLI parsing, PNG export). Apps build their own previewer binary
                     for app-specific catalogs by depending on this crate + their widget set.
teksilo-widgets-previewer Bundle binary that combines teksilo-widgets + teksilo-preview + teksilo-preview-ui to
                     preview the stock catalog. Two CLI modes: standalone (whole catalog) and
                     targeted (preview one widget).
```

Dependency flow: `tokens → canvas → core → data → widgets`, `canvas → text`, `core + data → settings`, `canvas → render → platform → app → ui`, `settings → app`, `i18n-macros → i18n`, `ui-macros → ui`, `core → preview`, `preview-ui → preview + widgets`, `widgets-previewer → (preview + preview-ui + widgets)`, `widgets → scene` (scene sits at the teksilo-widgets tier and reuses the full widget catalog as its heavyweight content), `core → webview` (webview sits at the teksilo-widgets tier but depends only on core; `teksilo-app` gains an optional dep on it under its `web-view` feature for event routing), `(app + core) → async → {tokio, async-std}` (optional executor; the `on_loop_tick` hook in app and the `AsyncCompletionHandle` types in core stay async-runtime-free, so teksilo-app never depends on teksilo-async)

External path dependency: `text-typeset` lives at `../text-typeset` (outside workspace).

## Unified Widget Trait (V2)

One trait for all widgets — leaf, container, composite, hybrid:

```rust
pub trait Widget: std::fmt::Debug + 'static {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> { vec![] }
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse; // required
    fn place_children(&self, _bounds: Rect, _proposal: SizeProposal, _children: &mut [WidgetPlacement], _ctx: &LayoutContext) {}
    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {}
    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
    fn children(&self) -> Vec<WidgetId> { vec![] }
    fn clips_children(&self) -> bool { false }   // ScrollArea, MaxSize
}
```

`layout_response` is the only required method. It returns a `LayoutResponse { size, flex, min, shrink }` carrying the wanted size, a grow weight (`flex`) for positive slack, a compression floor (`min`), and a shrink weight (`shrink`) for over-constraint deficits. Most widgets just return a `Size` (auto-converts via `From<Size>` to fully rigid: `flex = 0`, `shrink = 0`, `min = size`); grow-bearing widgets like `Spacer`/`Expand` use `LayoutResponse::flexible(size, flex)`; shrink-bearing ones (`Shrinkable`, single-line `TextWidget`) use `LayoutResponse::shrinkable(size, min, shrink)`. See **Layout Model** below.

- **Leaf** (TextWidget, RectWidget): `layout_response` + `paint`
- **Container** (VStack, HStack, ZStack): `layout_response` + `place_children` + `children`
- **Composing** (Button, Checkbox): `build` + `layout_response` (delegates to child) + `accessibility`
- **Hybrid** (Card, ScrollArea): `build` + `paint`

### Widget insertion

- `tree.add(w)` — add any widget to the tree
- `tree.add_child(parent, w)` — add as child of another widget
- `ctx.add(w)` — inside `BuildContext` during `build()`
- `ctx.add_boxed(w)` — inside `BuildContext`, accepts `Box<dyn Widget>`

## Layout Model

SwiftUI-style negotiation: parent proposes size → child responds with a `LayoutResponse` → parent distributes the main axis, measures the cross axis at each child's final main size, then places. All in logical pixels. `Leading`/`Trailing` instead of Left/Right (RTL-aware).

**`LayoutResponse { size, flex, min, shrink }`.** Four quantities drive a stack's distribution along its main axis: `size` (wanted/ideal — the growth floor), `flex` (grow weight for positive slack), `min` (hard compression floor), `shrink` (shrink weight for an over-constraint deficit). `flex` and `shrink` are independent (CSS-flexbox: grow vs shrink). `From<Size>` and `rigid`/`flexible` default to `min = size`, `shrink = 0` (fully rigid). See `LayoutResponse::shrinkable(size, min, shrink)` / `.with_min` / `.with_shrink`.

**Grow (positive slack).** `HStack`/`VStack` honor every child's wanted size as a floor, then distribute leftover **slack** (`bounds − Σ wanted − spacing`) proportional to `flex`. Default flex `0.0` (rigid); `Spacer`/`Expand` return `1.0`. Ratios are first-class:

```rust
HStack::new()
    .child(Expand::new().flex(1).child(panel_a))   // 1/3 of slack
    .child(Expand::new().flex(2).child(panel_b))   // 2/3 of slack
```

`Expand::new()` defaults to `flex(1)` and stretches its child to its bounds. Default basis is **zero** (CSS flex-basis: 0). Call `.respect_intrinsic()` for **auto** basis. `Center::new()` centers a child within the space it is **given** — fills a bounded axis, **shrink-wraps to the child on an unbounded one**, and reports `flex = 0`. It is *not* a synonym for `Expand::new().align_child(CENTER)` (`flex = 1`): a bare `Center` does **not** claim a stack's slack (it sizes to its child on the open main axis, like Flutter's `Center`/`Align`) — wrap it in `Expand` to center within leftover space (`Expand::horizontal { Center { w } }`).

**Shrink (over-constraint).** When children exceed the bounds, the deficit is distributed across children with `shrink > 0` proportional to their shrink weight, **never below `min`** (iterative clamp-and-redistribute). Shrink is **opt-in**: rigid by default → overflow. The one widget that opts in natively is **single-line / ellipsis `TextWidget`** (truncates to fit; `.min_shrink_width` / `.no_shrink` to tune) — truncating *display text* is expected. **Controls are deliberately rigid** (`Button` / `IconButton` / `Badge` / `ComboBox` size to content and overflow): a truncated *action* label reads poorly, so the desktop convention is to overflow excess actions into a menu — see `Toolbar` below. Wrap `Padding` / `ZStack` / `MinSize` propagate flex+shrink+min; the `Shrinkable` wrapper (the shrink counterpart to `Expand`) opts arbitrary content in: `Shrinkable::new().min_width(40.0).child(w)`. "Compress A before B" = give A `shrink>0`, B `shrink=0`. A stack only advertises its aggregate grow/shrink to its parent on its **own main axis** (so an `HStack` with a horizontal `Spacer` inside a `VStack` doesn't grow vertically).

**Height-for-width.** Because the main axis is decided before the cross axis is measured, a child whose height depends on its width (wrapped text, aspect-ratio image) reports the **correct** height at its final width, and that height propagates up the tree. A per-pass layout memoization cache (`WidgetArena::cached_layout_response`, keyed `(id, proposal)`, cleared each pass) keeps the main-then-cross queries O(n); widgets that mutate state in `layout_response` must keep it idempotent, or opt out via `Widget::cacheable_layout() -> false`. `LayoutContext::measure_intrinsic(id, proposal)` measures a widget's intrinsic size **regardless of activation** (even dormant/collapsed widgets + their dormant subtrees) — used by adaptive layouts (e.g. `Toolbar` overflow) that must size items they keep hidden.

**Overflow `Toolbar`.** [`Toolbar`](crates/teksilo-widgets/src/toolbar.rs) is a command bar (`ToolbarAction` / `ToolbarItem`) that collapses excess commands into a trailing `⌄` menu (Qt extension / NSToolbar overflow / WinUI CommandBar). Per-action overflow `priority` (lowest collapses first), `always_overflow` (WinUI secondary commands), `toggle` (checkable), separators, flexible space, `display_mode`, `orientation`, `is_overflowing()` signal. **Custom widgets**: `ToolbarItem::custom(w)` is pinned (never collapses); make it collapsible by declaring an overflow form — `.overflow_as(action)` (a menu row; an icon-only control reuses its icon as the menu glyph), `.overflow_widget(|| Box::new(...))` (a *live* embedded control, e.g. a `ComboBox` bound to the same signal, that stays usable while collapsed), or implement the `ToolbarOverflow` trait + `ToolbarItem::collapsible(w)`. The chevron drop-down is a real `MenuList` (size-to-content, focus-on-open, arrow/Home/End/Enter nav) driven by `MenuList::item_when` (conditionally-visible menu rows). Full ARIA toolbar a11y: `Role::Toolbar` + orientation + name, roving tab-index with arrow / Home / End navigation (the roving suppression reaches composite controls — Tab doesn't stick on a `ComboBox`), chevron `HasPopup::Menu`; overflowed commands are dormant (no AT duplication), represented by their menu rows. Built on `measure_intrinsic` (so collapsed items reappear correctly as the bar widens). Reactive `access_hidden` (`impl Into<Prop<bool>>`) is a related primitive for reactively hiding any node from AT. Reference: [docs/toolbar.md](docs/toolbar.md).

**Over-constraint debugging.** The debug inspector paints Flutter-style yellow/black **overflow hazard stripes** wherever a distributing container's children still spill past its bounds — on by default (F12). See [docs/inspector.md](docs/inspector.md). Demo: `cargo run -p over-constraint`.

**Layout primitives** (in [crates/teksilo-widgets/src/primitives/](crates/teksilo-widgets/src/primitives/)): `HStack`, `VStack`, `ZStack`, `Grid`, `Wrap`, `Padding`, `Spacer`, `Center`, `Expand`, `Shrinkable`, `FixedSize`, `MinSize`, `MaxSize`, `AspectRatio`, `Switcher`, `Divider`, `DeadZone`, `IconWidget`, `ImageWidget`, `ImageMask`, `MasonryLayout`, `ColumnFlow`, `FormLayout`, `ValidationStrip`, `TextInputField`

**`ColumnFlow`** (responsive reflowing columns) — the newspaper / CSS multi-column model, and the one flow container whose **column count follows the width**: `N = floor((avail + gap) / (min_column_width + gap))`, capped by `max_columns`; when the width no longer affords N columns it drops to N−1 and **re-partitions every child**. Distinct from its neighbours: `MasonryLayout` has a *fixed* count and packs into the shortest column; `Wrap` is row-major line-breaking; `Grid` is static tracks; `GridView` is a virtualized `ListModel`-bound data view. Children are **atomic** (never split across a column) and distributed as **contiguous source-order runs**, so source order == visual reading order == focus order at every count — which is *why* Masonry's shortest-column packing isn't reused (it interleaves, divorcing visual from source order; focus traversal and the AT walk both derive from tree order). Balance minimises the tallest column via a fixed-iteration bisection (greedy fill is the feasibility oracle), uses exactly *k* columns when `n ≥ k` (no stranded trailing column), and is re-run from scratch in both `layout_response` and `place_children` (no persisted partition state — the Masonry pattern; determinism comes from the fixed iteration count). **a11y by construction**: default `Role::GenericContainer` with *no* properties, so the walker **prunes** it and promotes children in source order — for a layout primitive, maximum accessibility is being invisible to AT while preserving order; setting any property (even orientation) would make it AT noise. Deliberately **not** `Role::Grid` (that ARIA pattern mandates arrow-key cell nav + roving focus, which `GridView` implements and this doesn't). `.semantic_list(true)` opts into `Role::List` + per-child `Role::ListItem`/posinset/setsize via a private layout-transparent wrapper (the `list_item_a11y.rs` precedent). No `aria-flowto` needed. Knobs: `min_column_width` (240 dp default) / `max_column_width` (Kirigami `CardsLayout` pairing — stops 1900 dp columns on a 4K display) / `max_columns` / `column_spacing` / `item_spacing` (both `Prop<f32>`) / `alignment` (only bites when `max_column_width` leaves leftover) / `column_rule` (decorative, no a11y node) / `column_count_signal()` (published from `place_children` behind an equality guard — the `Toolbar::is_overflowing` pattern; **safe for `RepaintOnly`/`AccessibilityOnly` consumers and for `Relayout` consumers that don't feed back into its own width**). Rigid (`flex = 0`, `shrink = 0`) — adapts when width is decided *for* it (VStack child / ScrollArea / window root), not as an `HStack` main-axis child (wrap in `Expand`); same as `Wrap`. Compose with a `ScrollArea` (it reports the tallest column as its height); it does **not** embed one. Column-count math is shared with `GridView` via `common/column_geometry.rs` (`ColumnGeometry` + `WidthPolicy`, extracted from `grid_view/layout/columns.rs` — the `common/row_offsets.rs` precedent). Reference: [docs/layout-primitives.md §5.4](docs/layout-primitives.md). Demo: `cargo run -p widget-catalog` (Layout tab — narrow the window).

**`DeadZone`** (gesture isolation) — a layout-transparent wrapper whose subtree is a **gesture dead zone**: a pointer press inside it never arms a drag/swipe on an ancestor. Wrap interactive controls (buttons, a `⋮` menu, a slider) placed inside a draggable/swipeable container (a dock-panel header, a card, a list row, a scene item) so clicking them — *even with the few px of jitter a real click carries* — can't start the ancestor's drag, while the container drags everywhere else. The framework counterpart of Electron's `-webkit-app-region: no-drag`. Robust **structurally**, not by a recognizer-timing race: it sets the node-level **`gesture_dead_zone`** flag (a `WidgetBuilder` / `HandlerSet` builder, mirroring `event_pass_through`) which [`arm_drag_observers`](crates/teksilo-core/src/widget_tree/event_dispatch_impl.rs) honours by stopping the ancestor-arming walk at the boundary (so it survives the descendant-tap-cancel/capture-release path a no-op "absorber" loses to). Used by the `Accordion` trailing slot so dock-header action buttons don't drag the panel. **Layout-transparent for the *full* `LayoutResponse`** — it forwards its child's `flex` / `shrink` / `min` (via `child_layout_response`), not just the size, so a shrink-to-fit child (e.g. a header-action `Toolbar`) still collapses when wrapped: flattening to a bare `Size` would make the wrapper rigid (`shrink = 0`) and over-constrain a tight dock header. [crates/teksilo-widgets/src/primitives/dead_zone.rs](crates/teksilo-widgets/src/primitives/dead_zone.rs).

**Rendering primitives:** `RectWidget`, `TextWidget`

**Text editing primitives:** `TextInputField`

## Signals & Reactivity (V2)

- `Signal<T>` — unified reactive type. `Signal::new(value)` for mutable, `signal.map(|v| ...)` for derived
- Multi-source combinators: `a.zip(&b)` / `a.zip3(&b, &c)` on any `Signal<T: Clone>`; `a.and(&b)` / `a.or(&b)` / `s.not()` on `Signal<bool>`. Derived signals dirty-track **every** upstream root, so widgets binding to a composite predicate re-render on any source change.
- Switch/bind combinator: `selector.flat_map(|t| pick_signal(t))` returns a `Signal<U>` whose value **and** dirty-tracking follow the inner signal chosen by the closure from `selector`'s current value; when `selector` changes it re-selects and follows the new inner (reactive "switchLatest"). Binding stays O(1) via a single composite source that resolves the active inner on each dirty-poll. Use to track the *active* item's reactive flag out of a set — e.g. gate a Next button on the currently-shown step's completion `Signal<bool>`.
- `Prop<T>` — widget property type: `Prop::Static(T)` or `Prop::Bound(Signal<T>)`. Methods accept `impl Into<Prop<T>>`
- `ColorProp` / `TextStyleProp` — theme-aware prop types for colors and text styles. See **Theming** below.
- `ObserverHandle` — RAII guard. Dropping removes the callback (no memory leak)
- `BindingLevel::RepaintOnly` (color changes) vs `BindingLevel::Relayout` (size changes)
- Color-accepting methods take `impl Into<ColorProp>` — accepts `Color`, a role (`TextRole`, `SurfaceRole`, `BorderRole`), a `Signal<Color>`, or a `Signal<Role>`. Prefer roles for theme-driven colors; a bare `Color` is frozen.
- `ctx.signal(value)` — create in build(), `ctx.effect(&signal, |v| ...)` — scoped effect (auto-cleaned on rebuild)
- `Signal<f32>::animate_to(target, duration, easing)` — smooth animation

Legacy types (`State<T>`, `DerivedState<T>`, `Reactive<T>`) exist in `teksilo-core::state` but are not used by widgets. All widget code uses `Signal`/`Prop`.

## Animation System

Smooth animated transitions for `Signal<f32>` values. The recommended path is
the fluent **`ctx.animate()`** spec builder — it captures `MotionTokens` and
the platform reduced-motion preference at build time, picks pixel-stable
defaults for looping animations, and provides a one-call `to_or_snap()` that
respects accessibility settings.

**Recommended API (one-shot, theme-aware):**
```rust
// In build():
let sidebar_width = ctx.animated_signal(300.0);
let sidebar = ctx.add(FixedSize::new().width(sidebar_width.clone()).child(content));
let slide = ctx.animate().normal().standard();   // duration_normal + easing_standard

// In a handler:
slide.to_or_snap(&sidebar_width, 0.0);   // snaps under prefers-reduced-motion
```

**Recommended API (looping with sub-perceptual epsilon):**
```rust
// `sweep()` reads `duration_indeterminate_sweep` AND turns on looping
// mode with epsilon = 1/255 + 60 Hz frame interval defaults.
ctx.animate().sweep().linear().to(&sweep_pos, 1.0);
```

`AnimationSpec` presets (all `pub fn ... (self) -> Self`):

| Method | Pulls from `MotionTokens` |
| --- | --- |
| `.instant()` | `duration_instant` |
| `.fast()` | `duration_fast` (tooltip fade, interactive feedback) |
| `.normal()` | `duration_normal` |
| `.slow()` | `duration_slow` |
| `.collapse()` | `duration_collapse` (accordion / disclosure tween) |
| `.sweep()` | `duration_indeterminate_sweep`, plus implies `looping()` |
| `.duration(d)` | explicit `Duration` |
| `.standard()` | `easing_standard` (the Int-UI mild ease-out) |
| `.linear()` / `.ease_in_out()` / etc. | explicit `Easing` |
| `.looping()` | sub-perceptual `epsilon = 1/255` + 60 Hz frame interval |
| `.frame_interval(d)` | throttle ticks (e.g. 66 ms = 15 Hz for slow sweeps) |

Application:

- `.to(&signal, target)` — always tween.
- `.to_or_snap(&signal, target)` — one-shot tween that snaps under
  `prefers-reduced-motion`. Use this for almost all UI transitions.

**Per-node paint scopes** (the engine primitives wrappers build on):
`BuildContext::set_opacity`, `set_clips_children`, `set_transform`, and
`set_blur` each attach a Prop to a node; the render walker emits a
matching push/pop pair (`SetOpacity`/`RestoreOpacity`, `SetClip`/
`ClearClip`, `PushTransform`/`PopTransform`, `BeginBlurredSubtree`/
`EndBlurredSubtree`) around the subtree. The renderer maintains a stack
per scope. `SetTransform` is *compose-with-stack-top* semantics — a
widget's canvas-local transforms (canvas.translate/scale/rotate)
compose with any ancestor transform scope instead of clobbering it.
`set_blur` is the only scope that triggers an offscreen render pass
(intermediate texture + dual-Kawase chain + composite blit) — sub-
perceptual radii (`< 0.5 px`) skip the Begin/End emit at the walker so
animated `0 → target_radius` patterns pay zero cost when fully off.
Scope nesting order on a single node, outermost to innermost:
`Begin (blur) → SetOpacity → PushTransform → ...paint...`.

**Animated wrapper widgets** (live in [crates/teksilo-widgets/src/animations/](crates/teksilo-widgets/src/animations/), re-exported flat from `teksilo::widgets`):

- `Collapse { expanded: Signal<bool>, child }` — wraps a child and animates
  its height (and width gate) between zero and natural when `expanded` flips.
  Used internally by `Accordion`. See [crates/teksilo-widgets/src/animations/collapse.rs](crates/teksilo-widgets/src/animations/collapse.rs).
- `Fade { visible: Prop<bool>, child }` — wraps a child and animates the
  entire subtree's opacity between 0 and 1. Layout-transparent: the child
  reports its full natural size at all opacity values. Built on
  `BuildContext::set_opacity` (a node-level opacity scope, parallel to
  `clips_children`). See [crates/teksilo-widgets/src/animations/fade.rs](crates/teksilo-widgets/src/animations/fade.rs).
- `Pulse::opacity(min, max).period(d).child(w)` — sine-driven looping
  opacity oscillation. The blinking-red-light / recording-indicator
  pattern. Layout-transparent (same as `Fade`). Reduced motion: pins
  at midpoint. Uses the per-frame-effect path
  (`ctx.subscribe_frame_tick()`) — chain auto-pauses when parked in a
  hidden Switcher branch, resumes on show. See [crates/teksilo-widgets/src/animations/pulse.rs](crates/teksilo-widgets/src/animations/pulse.rs).
- `Cycle::new().period(d).child(a).child(b)…` — steps through children
  on a fixed period (rotating loading tips, status displays). Internally
  a `Switcher` driven by a frame-tick effect. Same per-frame-effect
  visibility-aware path as `Pulse`. See [crates/teksilo-widgets/src/animations/cycle.rs](crates/teksilo-widgets/src/animations/cycle.rs).
- `SmoothSize::new().child(w)` — auto-sizes the slot to the child's
  current intrinsic size, *animating* every change. The "empty panel
  that suddenly must grow gracefully to accept new content" case.
  Distinct from `FixedSize::width(animated_signal)` (numeric target)
  — `SmoothSize` watches the child measure each frame. `.axes(Width|Height|Both)`
  to restrict. Reuses Collapse's "child laid out at natural, framework
  clips overflow" trick. See [crates/teksilo-widgets/src/animations/smooth_size.rs](crates/teksilo-widgets/src/animations/smooth_size.rs).
- `Crossfade::new(key_signal, |k| build_for(k))` — when the key
  changes, mounts both old and new content side by side in a `ZStack`,
  fades old → 0 and new → 1. Builders may run more than once per
  lifetime as keys recur. `.duration(d)` overrides the default.
  See [crates/teksilo-widgets/src/animations/crossfade.rs](crates/teksilo-widgets/src/animations/crossfade.rs).
- `Slide::new(visible).from(SlideEdge).child(w)` — slides a child in/out
  from the chosen edge (Leading/Trailing/Top/Bottom). Translates child
  position via `place_children`, doesn't change layout size — siblings
  don't reflow. Pair with `Fade` for the snackbar pattern. Clips so the
  off-edge child doesn't bleed past the slot. See [crates/teksilo-widgets/src/animations/slide.rs](crates/teksilo-widgets/src/animations/slide.rs).
- `Shake::new(trigger).child(w)` — bumping `trigger: Signal<u32>`
  plays a damped horizontal oscillation (defaults to
  `MotionTokens::duration_slow`, 4 cycles). Invalid-input feedback.
  Layout-stable, clips. Reduced motion: trigger is a no-op. See
  [crates/teksilo-widgets/src/animations/shake.rs](crates/teksilo-widgets/src/animations/shake.rs).
- `Scale::new(visible).child(w)` — uniform 2D visual scale 0↔1 driven
  by `Prop<bool>`. Built on `BuildContext::set_transform` (a node-level
  transform scope, parallel to `set_opacity`). Default: visual-only
  (slot stays at natural size, only the visual scales around the
  origin). `.reflow(true)` switches to layout-driving mode where the
  slot itself shrinks (siblings reflow); pair with `.origin(ScaleOrigin::TopLeading)`
  for the "card removal" pattern. See [crates/teksilo-widgets/src/animations/scale.rs](crates/teksilo-widgets/src/animations/scale.rs).
- `Rotate::new(angle_signal).child(w)` — rotates a child subtree by
  `angle: Prop<f32>` (radians). No internal animation; caller drives
  the angle signal and pairs with `Signal::animate_to` for animated
  rotations. Layout-stable. Use for chevrons, dial controls, rotation
  feedback. See [crates/teksilo-widgets/src/animations/rotate.rs](crates/teksilo-widgets/src/animations/rotate.rs).
- `Blur::new(radius).child(w)` — Gaussian-equivalent blur applied to
  the entire child subtree. `radius: Prop<f32>` in logical pixels;
  accepts static, signal, or animated values. Built on
  `BuildContext::set_blur` — the renderer redirects the subtree's
  draws into an intermediate texture, runs a dual-Kawase blur chain,
  and composites the blurred result back at the widget's bounds.
  Layout-transparent. Sub-perceptual radii (`< 0.5`) are zero-cost.
  Use for modal backdrops, click-to-reveal sensitive content
  (numerics/characters obscured), out-of-focus emphasis, animated
  frosted glass on modal show. The most expensive paint scope; don't
  use it for items that animate radius every frame at full radius.
  See [crates/teksilo-widgets/src/animations/blur.rs](crates/teksilo-widgets/src/animations/blur.rs).
- `Spinner::new(size)` — circular-arc loading indicator backed by the
  shader-driven `AnimatedQuadKind::SpinnerArc` pipeline (~one uniform
  write + one `draw_indexed` per frame, no `paint()` re-runs). Honours
  `prefers-reduced-motion` with a static three-quarter arc fallback.
  See [crates/teksilo-widgets/src/spinner.rs](crates/teksilo-widgets/src/spinner.rs).
- `OverlayRequest::with_fade(duration)` — fade-in / fade-out animation
  for any overlay (tooltip, popover, snackbar, …). The framework wires
  everything internally: creates an animated opacity signal, applies it
  as an opacity scope on the content (via `set_opacity`), kicks off the
  0→1 tween at show time, runs the 1→0 tween on dismiss, and defers
  the actual stack removal by `duration` so the tween plays out before
  the content goes dormant. Caller specifies just the duration — no
  `Fade` widget wrapping, no signal management.

**Lower-level types** (still public, used when you need full control):

- `Signal<f32>::animate_to(target, duration, easing)` — direct one-shot
- `Signal<f32>::animate_looping(target, period, easing, frame_interval)` — direct loop
- `Signal<f32>::try_animate_with_options(AnimationRequest)` — full control (epsilon, max_duration)
- `BuildContext::animated_signal(value)` — creates the `Signal<f32>` itself
- `BuildContext::prefers_reduced_motion()` — raw platform pref query
- `Easing` — `Linear`, `EaseIn`, `EaseOut`, `EaseInOut` (in teksilo-tokens)

**Three motion subsystems share one visibility primitive.** All
three sit on `WidgetTree`, all three consult
[`motion_visibility`](crates/teksilo-core/src/motion_visibility.rs)
(`alive` / `painted_this_frame` / `painted_recently`) to decide
whether their owner widget is visible enough to keep waking the event
loop. Pick by shape:

| Path | Surface | Use for |
| --- | --- | --- |
| Signal-tween — `AnimationScheduler` | `Signal<f32>::animate_to`, `ctx.animate().to_or_snap(...)` | linear tweens (toggle thumbs, scroll offsets, slider fills, dialog scale-in). One-shots and looping. |
| Shader-quad — `AnimatedQuadRegistry` | `ctx.animated_quad(kind)` | decorative motion that fits a quad + fragment shader (`Spinner`, `ProgressBar::indeterminate`, animated `IconWidget`). `paint()` does not re-run. |
| Per-frame-effect — `FrameTickScheduler` | `ctx.subscribe_frame_tick()` (returns RAII `FrameTickSubscription`) | hand-rolled per-frame closures (`Pulse` sine oscillation, `Cycle` discrete step). The framework re-arms the chain after every render iff at least one subscriber's owner painted that frame, so a Pulse parked inside a hidden Switcher branch contributes zero idle frames. Drop the guard to unsubscribe. |

The third path replaces the older "manual `frame_request_handle().set(true)` re-arm inside a `frame_tick` effect" pattern for visual continuous animations — the manual re-arm has no visibility gate and was the source of the catalog idle-fps bug. Keep `frame_request_handle` for owner-driven, non-visibility-bound needs (caret blink, drag auto-scroll while pointer captured).

**Files:** [crates/teksilo-tokens/src/motion.rs](crates/teksilo-tokens/src/motion.rs),
[crates/teksilo-core/src/animation.rs](crates/teksilo-core/src/animation.rs),
[crates/teksilo-core/src/animation_builder.rs](crates/teksilo-core/src/animation_builder.rs),
[crates/teksilo-core/src/animated_quad.rs](crates/teksilo-core/src/animated_quad.rs),
[crates/teksilo-core/src/frame_tick_scheduler.rs](crates/teksilo-core/src/frame_tick_scheduler.rs),
[crates/teksilo-core/src/motion_visibility.rs](crates/teksilo-core/src/motion_visibility.rs),
[crates/teksilo-core/src/signal.rs](crates/teksilo-core/src/signal.rs),
[crates/teksilo-widgets/src/animations/](crates/teksilo-widgets/src/animations/) (all wrapper widgets).
Visual showcase: `cargo run -p animations-kit`.

## Event System (V2 Attached Handlers)

- **Preview pass** (root → strict ancestors of target) + **Bubble pass** (target → root)
- **Attached handlers** replace monolithic `event()`: `.on_tap()`, `.on_hover()`, `.on_key()`, `.on_key_preview()`, `.on_focus()`, `.on_scroll()`, `.on_pointer_event()`, `.on_access_action()`
- `.on_key_preview()` runs during the preview pass for KeyDown/KeyUp/IME — strict ancestors only, so the focused widget never sees its own preview. Use for ancestors that need to claim chords before a focused inner widget consumes them (a messenger composer claiming Enter, a Dialog claiming Esc, a ListView claiming arrow keys). Shortcuts still resolve first; `on_key_preview` cannot override a registered shortcut.
- `.focus_within(Signal<bool>)` / `.hover_within(Signal<bool>)` — framework writes `true` whenever the focused / hovered widget is a **strict descendant** of this node. Drives unified halos around composite widgets (SpinBox, SplitButton, ComboBox, messenger composer panel, GroupBox sections). Strict-ancestors-only: a widget's own focus/hover does **not** flip its own `_within` signal — combine with `on_focus`/`on_hover` if you need both.
- Handlers attached via `WidgetBuilder` trait (blanket impl) or `HandlerSet` in `build()`
- Framework auto-wires gesture recognizers from handler types (on_tap → TapRecognizer)
- **Cross-widget tap/drag disambiguation** (`GestureArena` is per-widget, no cross-arena arbitration): when a descendant `on_tap` captures the pointer on PointerDown, the framework arms **drag observers** on the captured widget's strict ancestors that carry an `on_drag`/`on_swipe` — feeding the down/move into *that ancestor's own* gesture arena; if the ancestor recognizes a drag it calls `start_drag` and the existing `active_drag` takeover (checked before capture routing) pulls the pointer from the descendant. A captured widget that already has its own `on_drag` is skipped (untouched). So press-release = descendant tap; press-drag = ancestor drag (e.g. a `SceneView` behind tappable cards: click selects, drag marquees). See docs/events-and-gestures.md §4.2.
- `EventHandlers` struct on `WidgetNode` stores closures, dispatched by framework
- `.focusable(true)`, `.cursor(CursorIcon::Pointer)` — framework-level properties on node
- Cross-widget behavior: `ctx.send_intent(MyIntent::X)` inside handlers; ancestor `Action`s consume it (see "Actions, Intents & Shortcuts")
- **Tap-family callbacks take `&TapEvent` (`{ position, button, modifiers }`)** — `on_tap` / `on_double_tap` / `on_triple_tap` / `on_long_press`. Default acceptance is `ButtonMask::PRIMARY` only (right-click never activates `on_tap` by accident). Widen with `.accept_tap_buttons(...)` / `accept_double_tap_buttons(...)` / `accept_triple_tap_buttons(...)` / `accept_long_press_buttons(...)`. `PointerButton` covers `Primary | Secondary | Middle | Back | Forward`. Multi-tap recognizers require button-match across the whole sequence; mixed-button sequences fail rather than spuriously firing.

## Accessibility Overrides

Builder-level `.access_*` methods on `WidgetBuilder` (and `WidgetWithHandlers`) let an app author augment, replace, or annotate any widget's accessibility info from the outside — analogous to SwiftUI's `.accessibility*` modifiers and Flutter's `Semantics(...)`. Overrides ride the same `HandlerSet → WidgetNode` plumbing as `cursor` / `clips_children` / `focus_within`, then apply from the accessibility tree walker after the inner widget's `accessibility(builder)` runs.

```rust
use accesskit::{Action, Role, Live, HasPopup};

Button::new(tr!(save_icon()))
    .access_label(tr!(save()))                  // replace widget label
    .access_description(tr!(save_explanation())) // long-form context
    .access_role(Role::Button)
    .access_shortcut_id("app.save")             // tracks user rebinds via ShortcutRegistry
    .access_action(Action::ShowContextMenu, |ctx| ctx.send_intent(AppIntent::Menu))
    .access_custom_action(tr!(publish_now()), |ctx| ctx.send_intent(AppIntent::Publish));

// Subtree control:
my_card.access_merge_subtree();        // collapse card into one AT element
animated_logo.access_exclude_subtree();// hide all descendants from AT

// Status region:
toast_panel.access_live(Live::Polite);

// Cross-widget relationships:
combo_button.access_controls(listbox_id);
field.access_described_by(error_message_id);
```

**Naming and i18n.** All user-visible-string methods (`access_label`, `access_description`, `access_hint`, `access_value`, `access_custom_action`) accept `impl Into<Prop<String>>` and store a `Prop<String>`. With the `i18n` feature, `teksilo_i18n::LocalizedString` (the type produced by `tr!`) implements `From<LocalizedString> for Prop<String>`, so `.access_label(tr!(save()))` stays **locale-reactive** — the AT tree re-walks on a locale change and re-resolves the announced value (no composite rebuild needed). For intentionally-untranslated AT strings use `lit!(...)` (`access_label(lit!("Debug"))`); a bare `&str` no longer compiles. The `#[doc(hidden)]` `access_*_literal` twins survive only as the literal path reachable from inside `teksilo-core` itself (where `lit!` isn't available).

**Merge rules.** Scalars (`label`, `description`, `value`, `role`, `identifier`, `shortcut`, `live`, `aria_current`, `has_popup`, `orientation`, numeric range/step) replace if `Some`. Lists (`controls`, `described_by`, `labelled_by`, advertised actions, custom actions) append. `access_remove_action` suppresses an action the widget emitted before override-advertised actions are added. `access_customize(|b| ...)` runs **last** with full `&mut AccessNodeBuilder` access (including `inner_mut()`) — it's the supported escape hatch for synthetic-children surgery (rich-text paragraphs / text runs) and any AccessKit field the typed surface doesn't cover.

**Shortcuts.** Two variants for announcing a chord on the AT node — pick by where the binding lives.

- `.access_shortcut_id("app.save")` — bind to a `Shortcut` registered in `ShortcutRegistry`. The walker resolves the current keystroke at AT-build time and reformats on user rebinds (the registry's `version()` signal dirties the AT cache automatically). Use this for any chord routed through the `Shortcut`/`Action`/`Intent` pipeline, so a user rebind via `ShortcutSettings` retitles the announcement too. Same model as `MenuItem::for_shortcut(...)` / `TooltipContent::for_shortcut(...)`.
- `.access_shortcut_literal("Ctrl+S")` — explicit pre-formatted string. Use for chords NOT going through the `Shortcut` system: platform-native keys, app-internal hotkeys not exposed to rebinding, or stand-alone demos. Frozen at builder time — does not track rebinds.

**Clearing widget-set state.** `access_hidden(false)` calls `clear_hidden()` to un-set a hidden flag the widget emitted unconditionally (e.g. `Panel::a11y_presentational`). `access_disabled(false)` clears both widget-emitted and arena-driven disabled — the framework's gate at `accessibility_impl::build_accessibility_recursive` respects the override.

**Subtree modes.** `access_subtree(AccessSubtreeMode::{Inherit, Exclude, Merge})` (or the convenience `access_exclude_subtree()` / `access_merge_subtree()`) controls how the AT walker handles descendants:

- `Inherit` (default) — descendants emit normally.
- `Exclude` — descendants pruned from the AT tree; parent emits as-is.
- `Merge` — descendants' label / description / value (first non-empty) / actions / relationships are concatenated into the parent, then descendants pruned. The parent reads as one AT element. Nested `Exclude` inside `Merge` wins (excluded subtree contributes nothing); nested `Merge` inside `Merge` walks the inner merge first then absorbs the result as one element.

**Action callbacks.** `.access_action(action, callback)` advertises the action AND registers the callback. The dispatcher routes AT-invoked actions through `WidgetEvent::AccessAction` to the callback, layered on top of any user-installed `.on_access_action(...)` (both fire). Custom actions (SwiftUI `accessibilityAction(named:)`) use `accesskit::ActionData::CustomAction(idx)` and route by index in declaration order.

Full reference: [docs/accessibility-overrides.md](docs/accessibility-overrides.md). Implementation: [crates/teksilo-core/src/widget_builder.rs](crates/teksilo-core/src/widget_builder.rs) (`AccessibilityOverrides`, `AccessSubtreeMode`, `.access_*` methods); [crates/teksilo-core/src/widget_tree/accessibility_impl.rs](crates/teksilo-core/src/widget_tree/accessibility_impl.rs) (walker integration, `merge_descendants_into`).

## Global Text Scale

App-wide "grow all text" accessibility setting. A user factor (`1.0` = 100 %) **multiplies** the OS accessibility text-scale preference into an *effective text scale*; the change marks the tree dirty (relayout + repaint, **no rebuild** — focus/scroll survive) and is **persisted + restored at startup** with no per-app boilerplate.

- **Drop-in control:** [`TextScaleControl`](crates/teksilo-widgets/src/text_scale_control.rs) — a specialized `SpinBox` (80 %–200 %, step 10 %). `ctx.add(TextScaleControl::new(ctx.settings().signal_for(&TEXT_SCALE_KEY)).label(tr!(text_size())))` both persists and applies app-wide on edit. `TEXT_SCALE_KEY` (`"accessibility.text_scale"`) is re-exported in `teksilo::prelude`.
- **Mechanism (blanket):** the tree caches an `effective_theme` = active theme with `typography` scaled by `user × OS`; the layout/paint walkers read it, so **every widget that sizes text from `ctx.theme.typography` scales for free** (Button/Badge/ListItem/MenuItem/TableView/…). The combined factor is also published as `ctx.text_scale` (on `LayoutContext`/`PaintContext`), `ctx.text_scale_signal()` (reactive `Signal<f32>`), and `WidgetTree::{set_user_text_scale, effective_text_scale, text_scale_signal}`. Apply from a handler with `ctx.set_text_scale(f)` (fans out to all windows like `set_theme`/`set_locale`).
- **Editable text** (`TextInput`/`SpinBox`/`DateEdit`/hex input + `RichTextEditor`) shapes through a per-widget `RichTextEngine` and so bypasses typography; it scales via a **true per-engine logical `font_scale`** added to `../text-typeset` (`DocumentFlow::set_font_scale` → forwarded by `RichTextEngine::set_font_scale`), driven from `ctx.text_scale`. This `font_scale` (logical magnification, applied at *shaping*) is orthogonal to the existing `scale_factor` (HiDPI raster, cancels out of logical metrics) and `zoom` (post-layout display transform) — and never double-applies (the standalone `TextWidget`/`Canvas` shapers pass `font_scale = 1.0` since their size is already theme-scaled).
- **Opt-in / opt-out** (unified `.follow_text_scale(bool)`): `IconWidget` off by default (opt in; `SeverityBadge` enables it so status glyphs track adjacent text); `RichTextEditor` on by default (opt **out** for WYSIWYG editors whose font sizes are document content); `teksilo-scene` `TextItem` off by default (scene has its own pan/zoom — opt in); `Calendar` on (rebuilds with its fixed cell/header constants multiplied by `ctx.text_scale`, via a `Rebuild`-level binding on `ctx.text_scale_signal()`).

Reference: [docs/text-scale.md](docs/text-scale.md). Demo: `cargo run -p widget-catalog` (the `TextScaleControl` in the title bar grows the whole catalog live).

## Window-Active Appearance

When a window loses OS focus, its widgets change appearance — the modern desktop convention (SwiftUI `@Environment(\.appearsActive)`, Jetpack Compose `LocalWindowInfo.isWindowFocused`, GTK4 `:backdrop`, Qt `QPalette::Inactive`). Teksilo does this automatically for stock widgets and exposes an opt-in hook for custom content. A window is **active** when `focused AND not occluded`; this is **per-window** (no app-wide fan-out, unlike theme / locale / text-scale).

- **The primitive (mirrors the global text-scale plumbing):** `WidgetTree::window_active_signal() -> Signal<bool>` (starts `true` — a window must not be born inactive), written by `set_window_active` ([crates/teksilo-core/src/widget_tree.rs](crates/teksilo-core/src/widget_tree.rs)). Surfaced as `ctx.window_active()` / `ctx.window_active_signal()` on `BuildContext`, `ctx.window_active: bool` on `PaintContext`, and `ctx.window_active()` on `EventContext`. App layer feeds it from `WindowEvent::Focused`/`Occluded` ([crates/teksilo-app/src/app.rs](crates/teksilo-app/src/app.rs), already computing `focused && !occluded`).
- **Repaint, never relayout:** a focus flip calls `WidgetArena::mark_all_needs_paint_only()` (a new paint-only mark beside `mark_all_dirty`) — geometry never changes (the caret keeps its space). Cheaper than `set_theme`'s `mark_all_dirty`; makes any paint-time `ctx.window_active` reader correct with no per-widget binding.
- **Automatic, default-on (correctness, not opt-in).** Two complementary layers — **theme-side** for everything that's a plain accent colour, **per-widget** only where the theme genuinely can't decide:
  1. **Accent desaturation (theme-side — the broad sweep).** When inactive, the paint walker paints against `effective_theme.for_inactive_window()` ([crates/teksilo-core/src/styles/theme.rs](crates/teksilo-core/src/styles/theme.rs) → `ColorTokens::for_inactive_window` in [crates/teksilo-tokens/src/theme.rs](crates/teksilo-tokens/src/theme.rs)), which desaturates the accent family (`accent`/`accent_hover`/`accent_pressed`/`accent_disabled`/`accent_subtle_bg`) + focus indicators (`border_focused`/`focus_ring`) toward graphite — the Qt `QPalette::Inactive` model (swap at [rendering_impl.rs](crates/teksilo-core/src/widget_tree/rendering_impl.rs)). Because every themed control resolves its accent from the live `ColorTokens` *at paint time* (e.g. Toggle reads `colors.accent`), this **one swap greys out every accent control with zero per-widget code**: `Button`(Filled/Destructive), `Toggle`, checked `Checkbox`/`RadioButton`, selected `TabBar` tab + `SegmentedControl` segment, `Slider` fill, `ProgressBar`, `Badge`, focus rings. It applies to **any preset** that populates these tokens — IntUI, **Material 3** (where `accent` = M3 *primary*; covered with no material3-specific code), future presets. Repaint-only (colours, not typography). Don't add per-widget window-active accent checks — they belong here.
  2. **Caret hiding (per-widget — behavioural, a theme can't hide a caret).** The caret hides in an inactive window for every policy, in *both* text stacks (`RichTextEditor` rich_text/, and `TextInputField` primitives/ — base for `TextInput`/`PasswordField`/`SpinBox`/`SearchField`). The frame-loop `tick()` has no context, so `build()` registers an effect on `window_active_signal` that mirrors the value onto the editor state and hides the caret **synchronously** on deactivation (the loop may not tick while parked); blink + paint gates AND-in `state.window_active`. Restored immediately on reactivate.
  3. **Selection desaturation (per-widget — needs *view* focus + a dedicated token).** Vivid `SurfaceRole::Selected` only when **view-focused AND window-active**, else the muted `SurfaceRole::SelectedInactive`. Per-widget — *not* theme-side — because it (a) depends on *view* keyboard focus (a per-view runtime state the theme can't know) and (b) uses the hand-tuned `surface_selected_inactive` / `selection_bg_inactive` tokens so view-unfocused and window-inactive show **one** consistent muted colour (a theme desaturation of `surface_selected` would be a *different* computed gray). Composed in `StandardListItem`/`StandardTreeItem` `bg_signal` (`is_focused.and(is_window_active)`), `TableView`/`TreeTableView` selection-band paint (`view_focused.get() && ctx.window_active`), and the text engines (swap to `selection_bg_inactive`). A custom `.selection_color(..)` stays fixed (macOS semantics). `MenuList` excluded (an open menu is always active).
- **Opt-in for custom content:** `.dim_when_inactive(factor)` (and `dim_when_inactive_default()`, 70 %) on the `WidgetBuilder` trait wraps a subtree in `DimWhenInactive` ([crates/teksilo-core/src/dim_when_inactive.rs](crates/teksilo-core/src/dim_when_inactive.rs)), driving a node-level opacity scope from `window_active_signal`. Layout-/a11y-transparent; opacity snaps (reduced-motion-correct). Re-exported in `teksilo::prelude` as `DimWhenInactive`.
- **a11y unaffected** (caret/selection/accent are all paint-only). **Test** with `WidgetTree::set_window_active(bool)` (a fresh tree is active).

Reference: [docs/window-activation.md](docs/window-activation.md). Demo: `cargo run -p multi_window` (two windows; click between them to watch the inactive one hide its caret, mute its selection, dim its panel, flip its status label).

## Actions, Intents & Shortcuts

Three-layer input-to-behavior pipeline. There is **no** `AppCommand`/`on_command` anymore — widgets fire `Intent`s, ancestor widgets register `Action`s keyed by intent name, and `Shortcut`s bind rebindable keystrokes to intent names.

```rust
use teksilo::IntentKind;
use teksilo::core::{Action, shortcut::{KeyStroke, Shortcut}};
use teksilo::prelude::*;

#[derive(Debug, IntentKind)]
enum AppIntent {
    #[name = "app.save"]      Save,
    #[name = "app.open"]      Open(String),
    #[name = "app.scroll_by"] ScrollBy(i32),
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.register_shortcut_global(
            Shortcut::new("app.save").name("Save")
                .primary(KeyStroke::ctrl(Key::S)).build(),
        );
        // Unit handler: name match is enough — no payload to extract.
        ctx.register_action(
            Action::new("app.save").on_invoke(|_i, _c| println!("saved")),
        );
        // Data-bearing handler: extract typed variant.
        ctx.register_action(Action::new("app.open").on_invoke(|i, _c| {
            if let Some(AppIntent::Open(path)) = AppIntent::from_intent(i) {
                open_file(path);
            }
        }));
        // Fire programmatically:
        let btn = Button::new(lit!("Save"))
            .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save));
        vec![ctx.add(btn)]
    }
}
```

Key APIs:
- `Shortcut::new("id").primary(KeyStroke::ctrl(Key::S)).build()` — rebindable keystroke → intent name. `.on_activate(|ks, ctx| AppIntent::X(…))` for parametric payloads (chord-dependent data).
- `ctx.register_shortcut(shortcut)` (widget-scoped, default) / `ctx.register_shortcut_global(shortcut)` (app-wide).
- `Action::new("id").on_invoke(|intent, ctx| …)` — handler. Register with `ctx.register_action(action)` (scoped to the widget's source→root path) or `ctx.register_action_global(action)` (**app-global dispatch fallback** — reachable no matter where the intent originated). Use the global form for commands fired from the **menu bar / chrome** (menu dropdowns render in an *overlay*, so a plain `register_action` on a sibling widget is never on the menu intent's walk — the classic dead-menu-command bug) or from a root-anchored global shortcut. Action-side counterpart to `register_shortcut_global`; owned by the registering widget (torn down on rebuild/destroy).
- `ctx.send_intent(AppIntent::X)` — fire from any handler. Blanket `impl<K: IntentKind> From<K> for Intent` lets you pass the enum variant directly.
- `#[derive(IntentKind)]` with `#[name = "..."]` on variants — typed DTO bridge. Works with unit, tuple, and struct variants (whole variant = payload).
- `ShortcutRegistry::version()` is a `Signal<u64>`; menu labels and tooltips use `MenuItem::for_shortcut("id")` / `TooltipContent::for_shortcut("id")` to re-render on rebinds.
- `ShortcutSettings::new()` — pre-built rebind UI widget.

Handler rule:  call `AppIntent::from_intent(intent)` **only** when you need typed fields. Unit intents react on name alone — this lets the same handler fire whether the intent came from a shortcut (name-only synthesized) or from `send_intent(AppIntent::X)` (typed payload).

Full reference: [docs/shortcut-intent-action.md](docs/shortcut-intent-action.md). Working demo: [examples/shortcuts_demo](examples/shortcuts_demo/src/main.rs).

## Settings & Persistence

Persistent, reactive user preferences via `teksilo-settings`. **In-memory is the source of truth** — `Signal<T>` and `*Model<T>` handles drive both UI and disk; the disk side is a debounced atomic projection (write-temp + rename, single shared I/O thread per process).

Three persistence shapes:

- **`SettingsStore`** — dotted-key K/V for **scalars** (numbers, strings, bools, arrays of those). `store.signal::<T>(key, default)` or `store.signal_for(&KEY)` returns a cached `Signal<T>`. Same key → same signal across call sites. Struct values rejected at registration with a clear error pointing to `SettingsFile<T>` (TOML serializes structs as tables, indistinguishable from nested key paths).
- **`SettingsFile<T>`** — typed single-struct persistence with `Versioned` + `Migrator<T>` migrations on raw `toml::Value` *before* deserialize. Corrupt files quarantine to `<path>.broken-<unix_ts>` and fall back to `T::default()`.
- **`PersistedListModel<T>` / `PersistedTreeModel<T>`** — bridges from `ListModel<T>` / `TreeModel<T>` to `SettingsFile<*File<T>>`. Every mutation re-serializes the whole collection (debounced) — fine for <1k items, use SQLite beyond that.

Built-in services on top:

- **`MruList<T: MruEntry>`** — generic dedupe + pin + LRU-cap recents. Apps define their own item type implementing `MruEntry { type Key; fn key(); fn is_pinned()/set_pinned(); fn touch(); }`. The framework knows nothing about projects / files / palettes.
- **`WindowStateService`** — per-`label` window geometry. **Auto-restored and auto-saved by `teksilo-app`'s window manager** when a `WindowConfig` carries `id(...)` and the bundle has `with_window_state(true)`. No widget-side wiring.

```rust
use teksilo::settings::{AppPaths, MruEntry, MruList, SettingsBundle, SettingsExt, SettingsKey};

const FONT_SIZE: SettingsKey<f32> = SettingsKey::new("editor.font_size", || 14.0);

fn main() {
    let paths = AppPaths::new("eu", "FernTech", "Teksilo").expect("config dir");
    let recents: MruList<RecentProject> = MruList::open(&paths, "recent_projects", 10).unwrap();

    TeksiloAppBuilder::new()
        .app_paths(paths)                                         // OR .application(qual, org, app)
        .settings(SettingsBundle::new().with_window_state(true))  // store + window state
        .app_state(recents)                                       // app-typed MRU
        .initial_window(
            WindowConfig::new()
                .id("main")                                       // <- enables auto save/restore
                .title("Teksilo")
                .size(1200, 800),
        )
        .run();
}

// Inside a handler / build:
let size = ctx.settings().signal_for(&FONT_SIZE);  // Signal<f32>
size.set(18.0);                                    // schedules debounced flush
let recents = ctx.mru::<RecentProject>();          // &MruList<RecentProject>
```

Key rules:

- `app_paths(...)` or `application(...)` must come **before** `settings(...)`. Bundle has nowhere to write otherwise; calling `settings(...)` without paths panics at `run` / `build_headless`.
- `application(qual, org, app)` panics if the OS can't resolve a home directory. Sandboxed CI / portable apps use `app_paths(AppPaths::for_testing(...) | from_dirs(...))`.
- Window auto-persist requires both `.with_window_state(true)` on the bundle AND `WindowConfig::id("...")`. Modal dialogs and popovers are naturally excluded — they don't carry stable ids.
- Saved geometry is sanitized on restore against the current monitor's work area: oversized rectangles clamp, off-screen positions recenter per-axis (so a saved coordinate from a now-disconnected secondary monitor gets recentered onto the primary while keeping the other axis if it was visible). `WindowPlacement::Minimized` is downgraded to `Floating` on restore.
- Wayland ignores window position by protocol design (compositor-authority); size and `WindowPlacement` round-trip, position is a no-op there.
- `SettingsExt` accessors (`use teksilo_settings::SettingsExt;`): `ctx.settings()`, `ctx.window_state()`, `ctx.mru::<T>()`. `try_*` variants return `Option`.
- Tests use `AppPaths::for_testing(tmp.path())` and `Duration::ZERO` for the debounce — never the real `ProjectDirs`.

Full reference: [docs/settings.md](docs/settings.md). Working demo: [examples/recent_projects](examples/recent_projects/src/main.rs).

## Locale-aware Formatting

Numbers, dates, and times that change with the user's locale flow through one ICU4X-backed layer in `teksilo-i18n`. Two consumer paths share the same cache, so a UI mixing translated and untranslated displays stays internally consistent on `,` vs `.`, grouping, currency suffixes, etc.

**Bundle-side path — `tr!` / `tr_signal!` messages.** `manager::configure_bundle` installs a `set_formatter` callback on every Fluent bundle and registers a custom `DATETIME()` function. So `{ NUMBER($v) }` and `{ DATETIME($ts, dateStyle: "long") }` inside `.ftl` messages render correctly across locales — no app-side wiring. Pass numeric args as ordinary `f64`/`i32`/etc.; pass datetimes as [`TeksiloDateTime`](crates/teksilo-i18n/src/format.rs):

```rust
let dt: jiff::civil::DateTime = ...;
tr!(last_saved(ts = TeksiloDateTime::from(dt)))
```

**Signal-side path — non-translated displays.** `NumberFormatter` and `TeksiloDateTimeFormatter` produce a `Signal<String>` from a value (static or `Signal<T>`-bound) plus the i18n manager's locale signal. Re-renders on either change. Used for SpinBox values, TableView cells, status bars, numeric inputs — anywhere the value isn't part of a translated sentence:

```rust
let display = NumberFormatter::new()
    .currency("USD")
    .fraction_digits(2, 2)
    .format(price_signal);  // Signal<f64> → Signal<String>

let when = TeksiloDateTimeFormatter::new()
    .date_style(DateStyle::Long)
    .format(timestamp_signal);
```

`NumberStyle` is `Decimal | Percent | Currency`. `DateStyle` / `TimeStyle` are `Long | Medium | Short`. The Signal-side formatter builders are zero-arg `::new()` then chain options; there's no `BuildContext` argument — the formatter resolves the active locale via the same thread-local accessor `LocalizedString` uses.

**`tr_signal!` / `tr_signal_widget!` — `Signal<T>` inside translated sentences.** When a reactive value belongs in the middle of a localized sentence (counters in messages, balances in alerts, timestamps in status lines), use `tr_signal!`. Every named arg must be a `Signal<T>`; the result is a `Signal<String>` that re-renders on any-arg / locale / `.ftl`-hot-reload change:

```rust
let count: Signal<i64> = ...;
let price: Signal<f64> = ...;
let label: Signal<String> = tr_signal!(cart_summary(count = count, price = price));
// label re-renders on count.set(...), price.set(...), manager.set_locale(...), or hot reload
```

The macro auto-clones the `Signal<T>` arg expressions, so the caller's handles survive after the call. Compile-time `.ftl` validation works exactly like `tr!` — same `KeyMap` parser, same key-existence + arg-name checks.

**ICU coverage and known limitations.** `Decimal` is fully ICU-correct (locale-aware grouping, digit shaping, signs). `Percent` multiplies by 100 and appends ASCII `%` (the locale-correct percent sign lives in unstable `icu_experimental`). `Currency` formats as decimal and appends the ISO-4217 code as a suffix (`"42,50 EUR"`); no symbol substitution or per-locale prefix/suffix positioning. Both percent and currency promote to full ICU once `icu_experimental` stabilises. `DateTime` is fully ICU-correct via `CompositeDateTimeFieldSet`.

**Files:** [crates/teksilo-i18n/src/format.rs](crates/teksilo-i18n/src/format.rs) (Memoizable types, ICU bridge, `TeksiloDateTime`, public formatter types, bundle callback, `DATETIME` function); [crates/teksilo-i18n/src/manager.rs](crates/teksilo-i18n/src/manager.rs) `configure_bundle` (one helper, three call sites at the `FluentBundle::new` boundary); [crates/teksilo-i18n-macros/src/lib.rs](crates/teksilo-i18n-macros/src/lib.rs) (`tr_signal!` / `tr_signal_widget!` lowering — branches off `tr_impl(input, kind, signal)`); [crates/teksilo-i18n/tests/format_integration.rs](crates/teksilo-i18n/tests/format_integration.rs) (end-to-end tests for both paths plus the macro).

## Three-Tier Rendering

| Tier | Type | Used For |
|------|------|----------|
| 1 | `DecorationRect` | Backgrounds, borders, focus ring |
| 2 | `ShapeQuad` (SDF) | Rounded rects, circles, gradients |
| 3 | `PathEntry` (tiny-skia) | Arbitrary paths, SVG icons |
| Text | `GlyphQuad` | Glyph atlas text |

Three wgpu pipelines: `rect_pipeline`, `sdf_pipeline`, `quad_pipeline`.

## Theming

Four-tier ladder: **tokens → variants → recipes → style protocols**. Full reference at [docs/styling-system.md](../docs/styling-system.md).

**Theme construction.** No `Theme::default()` / `Theme::*_default()` — apps explicitly pick a preset:

```rust
use teksilo::prelude::intui;
let theme = intui::light();   // or intui::dark()
```

`appearance: ThemeAppearance::{Light, Dark}` is a required field on every theme. Drives shadow density, OS-theme matching, and asset variant selection.

`Theme` (in `teksilo-core::styles`) carries five token groups (`ColorTokens`, `LayoutTokens`, `TypographyTokens`, `ShapeTokens`, `MotionTokens`), `ComponentStyles` (dimension data for the *non-themable* widgets only — the 17 per-themable-widget dim structs were deleted in Step 7 and folded into the recipe modules), `ComponentStyleSlots` (typed `Rc<dyn FooStyle>` slot bag for theme-wide style overrides), and `ThemeExtensions` (typed app registry).

**Theme is reactive.** `set_theme` updates an internal `Signal<Theme>` and dirty-marks every node — no rebuild. Focus, scroll offsets, and all interaction state survive theme changes.

**Tier 1 — Variants** (per themable widget):

```rust
Button::new(tr!(save())).variant(ButtonVariant::Filled)        // primary action
Toggle::new(on).variant(ToggleVariant::Switch)                  // default
```

`ButtonVariant {Filled, Tinted, Outlined, Plain, Ghost, Link, Destructive}` — default `Plain`. IntUI maps `Destructive` → Filled, `Tinted`/`Outlined` → Plain, `Link` → Ghost. Other widgets follow the same shape (see styling-system.md).

**Tier 2 — Recipes** (paint vocabulary): `ShapeRecipe`, `FillRecipe` (Solid/LinearGradient/RadialGradient), `BorderRecipe` (with `BorderStyle`/`BorderPosition`), `ShadowRecipe`, `PerStateRecipe<T>` (Teksilo's answer to Flutter's `WidgetStateProperty<T>` — explicit fallback chain `pressed → hover → idle`), `WidgetState`. All in `teksilo_core::styles::*`. Recipes use `RecipeColor` (plain data) instead of `ColorProp` so they serialize cleanly.

**Tier 3 — Style protocols** (the escape hatch):

```rust
pub trait ButtonStyle: 'static {
    fn make_body(&self, cfg: &ButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}
```

Three precedence levels (highest wins):

1. **Per-call:** `Button::new(lit!("X")).style(MyGlow)` — instance override.
2. **Theme-wide:** `theme.style_slots.button = Some(Rc::new(MyGlow))` — applies to every Button using the active theme.
3. **Default:** `RecipeButtonStyle::default()` shipped in `teksilo-widgets/src/styles/` reading IntUI tokens.

Migration status (as of this branch): **all 35 themable widgets are on Tier 3.** Controls: `Button`, `SplitButton`, `IconButton`, `Toggle`, `Checkbox`, `RadioButton`, `Slider`, `SegmentedControl`, `ProgressBar`, `Link`, `Avatar`, `Badge`. Inputs: `TextInput`, `SearchField`, `ComboBox`, `SpinBox`, `DateEdit`, `ColorPicker`, `Calendar`, `RichTextEditor`. Containers: `Panel`, `Card`, `TabBar`, `ListView`/`TreeView` (via `ListContainerStyle`), `TableView`/`TreeTableView` (via `TableStyle`), `DropZone`, `DropTarget`. Overlays: `TooltipWidget`, `Popover`, `Dialog`, `Snackbar`, `Banner`. Rows: `MenuItem`, `StandardListItem`/`StandardTreeItem`. Chrome: `ScrollBar`. No themable widget self-paints anymore; each delegates chrome to `style.make_*(cfg, ctx)`. Four traits are multi-method: `TabStyle` (`make_body` + `make_bar`), `DialogStyle` (`make_panel` + `make_scrim`), `TableStyle` (`make_header_cell` + `make_sort_indicator` + `make_row_background`), `CalendarStyle` (`make_day_cell` + `make_zoom_cell` + `make_header`). The per-widget dim structs are deleted and their IntUI constants live in `teksilo-widgets/src/styles/recipe_*_style.rs`. Still pending: image-backed styles, `ImageTheme` TOML loader, sibling preset crates.

**Roles** stay relevant — they name *what* a value represents (`TextRole::Primary`, `SurfaceRole::Hover`), resolved against the current theme at paint time. Widget builders accept `impl Into<ColorProp>` so any of `Color | Signal<Color> | TextRole | SurfaceRole | BorderRole | DynamicTextRole(Signal<TextRole>) | DynamicSurfaceRole(..) | DynamicBorderRole(..)` works.

Interaction-driven colors use the `Signal<Role>` pattern:

```rust
let bg_role = interaction.map(|s| match s {
    InteractionState::Hovered => SurfaceRole::Hover,
    InteractionState::Pressed => SurfaceRole::Pressed,
    _ => SurfaceRole::Transparent,
});
RectWidget::new().background(bg_role)
```

`ctx.theme_signal()` / `ctx.locale_signal()` are still available for the cases no role covers. Use sparingly. Reactive-theme details: [docs/reactive-theme.md](../docs/reactive-theme.md).

## Testing Patterns

```rust
// Headless widget tree test
let mut tree = WidgetTree::new();
let widget = tree.add(MyWidget::new());
tree.layout(SizeProposal::exact(400.0, 300.0));
assert!((tree.bounds(widget).width - expected).abs() < 0.01);

// Test utilities
MockTextBackend::new()         // Fixed 8px char width
LayoutContext::for_testing(&theme)
```

Test widgets: `FillWidget` (minimal leaf), `StackWidget` (minimal container) — in `teksilo-core::test_widgets` (pub(crate)).

Property-based tests (proptest) cover relational invariants (round-trip, idempotence, conservation, monotonicity, oracle-vs-brute-force) in `teksilo-tokens`, `teksilo-data`, `teksilo-scene`, `teksilo-widgets` — convention, generator cost discipline, and the mandatory `--no-run` + `ulimit` safe-run protocol are in [docs/property-testing.md](../docs/property-testing.md).

## Implementation Status

### Complete

- Core framework (arena, layout engine, event dispatch, focus management)
- V2 Widget authoring model (unified Widget trait, Signal/Prop, attached handlers)
- Signal-based reactivity (Signal, Prop, ObserverHandle, scoped effects)
- Gesture recognition (UIKit-style state machines, auto-wired from handlers)
- Overlay system (OverlayManager, OverlayRequest, positioning)
- Design tokens (full Theme system) + four-tier styling ladder (tokens → variants → recipes → style protocols). `ThemeAppearance::{Light, Dark}` required field. `presets::intui::{light, dark}` shipped in `teksilo-core`; no `Theme::default()` / `Theme::*_default()`. Recipe types (`ShapeRecipe`, `FillRecipe`, `BorderRecipe`, `ShadowRecipe`, `PerStateRecipe<T>`, `WidgetState`) in `teksilo_core::styles`. Per-widget style traits (`ButtonStyle`, `ToggleStyle`, `CheckboxStyle`, `RadioStyle`, `IconButtonStyle`, `PanelStyle`, `CardStyle`, `TooltipStyle`, `MenuItemStyle`, `PopoverStyle`, `SliderStyle`, `TextInputStyle`, `ComboBoxStyle`, `ScrollBarStyle`, `StandardItemStyle`, `TabStyle`, `SplitButtonStyle`, … — see the **Theming** section above for the full 35-widget list) with default `Recipe*Style` impls in `teksilo-widgets/src/styles/`. All themable widgets delegate visual chrome via `style.make_body(cfg, ctx)`; apps install per-call (`.style(impl …Style)`) or theme-wide (`theme.style_slots.<widget> = Some(Rc::new(...))`). The legacy per-widget dim structs are deleted. Image-backed styles + `ImageTheme` manifest loader + sibling preset crates (Material 3 / macOS / Fluent) are still pending. Reference: [docs/styling-system.md](docs/styling-system.md).
- Window management (multi-window, modal dialogs, custom title bar — Wayland + macOS + Windows + X11; X11 needs a WM with `_NET_WM_MOVERESIZE`, probed before window creation, else it falls back to native decorations. X11 has no OS window menu, so `PlatformTitleBarHost::has_window_menu()` is `false` there and `TitleBar` builds its own)
- GPU rendering (3 pipelines, glyph atlas, path atlas)
- All ~22 layout primitives (including Grid, Wrap, AspectRatio, Switcher, MasonryLayout, ColumnFlow, FormLayout)
- Accessibility (AccessKit integration at trait level + builder-level overrides: `.access_label`, `.access_description`, `.access_hidden`, `.access_role`, `.access_disabled`, `.access_controls`/`described_by`/`labelled_by`, `.access_live`, `.access_shortcut_id`/`access_shortcut_literal`, `.access_action`/`access_remove_action`/`access_custom_action`, `.access_exclude_subtree`/`access_merge_subtree`, `.access_customize` — see "Accessibility Overrides" above)
- Animation system (`Signal<f32>::animate_to`, easing, per-frame scheduler)
- Internationalization (teksilo-i18n + teksilo-i18n-macros: Fluent-rs, `tr!`/`tr_widget!`, locale resolution, file watcher, RTL direction signal). Locale-aware formatting via `NumberFormatter` / `TeksiloDateTimeFormatter` (`Signal<T>` → `Signal<String>`) and `TeksiloDateTime` (jiff wrapper for the `DATETIME()` Fluent function). The framework auto-installs a `set_formatter` callback + custom `DATETIME` function on every bundle, so `{ NUMBER(...) }` / `{ DATETIME(...) }` inside `.ftl` messages render correctly across locales. Reactive `tr_signal!` / `tr_signal_widget!` macros bind `Signal<T>` arguments inside translated sentences and re-render on any-arg / locale / hot-reload change. Backed by ICU4X (`icu_decimal` / `icu_datetime` / `icu_calendar`); see "Locale-aware Formatting" below.
- `teksu!` DSL (teksilo-macros: block-structured widget-tree syntax, desugars to V2 builder calls — see `docs/teksu-macro-reference.md`)
- Actions / Intents / Shortcuts (`Action`, `Intent`, `Shortcut`, `ShortcutRegistry`, `#[derive(IntentKind)]`, `ShortcutSettings` — rebindable keystrokes, typed-enum DTO bridge, source → root dispatch; see `docs/shortcut-intent-action.md`)
- Ancestor key intercept (`.on_key_preview`) and subtree state signals (`.focus_within(Signal<bool>)` / `.hover_within(Signal<bool>)`) — strict-ancestors-only, see Event System above
- Reactive data models (teksilo-data: `ListModel`, `TreeModel`, `TreeSlice`, `TreeDataSlice<K, T>` (reusable `TreeDataSource` over an external indent-ordered source: `Vec<TreeRow{key,item,depth}>` in → per-view expand + flatten + divergence + DnD, keyed by a domain `K`, injected policy closures — the ready-made alternative to hand-rolling `TreeDataSource`), `TreeRowFilter<K, T>` (row-stream sort/filter for the `TreeDataSlice` pipeline — `Vec<TreeRow>` → `Vec<TreeRow>`, reuses `TreeFilterMode`, sits below the slice), `SelectionModel`, `SortFilterListModel<T>`, `SortFilterTreeModel<T>` with `TreeFilterMode` `HideNonMatching`/`KeepAncestors`/`KeepDescendants`, `CheckedModel` + `TreeCheckedModel<T>` + `KeyedTreeCheckedModel<K>` (domain-keyed twin for external trees, `from_source(slice)`) for per-row checkbox state with optional descendant→ancestor tristate aggregation, `CheckState`)
- Settings & persistence (teksilo-settings: `SettingsStore` dotted-key Signal<T> K/V, `SettingsFile<T>` with versioned migrations, `PersistedListModel`/`PersistedTreeModel`, generic `MruList<T: MruEntry>`, `WindowStateService` with framework-driven auto save/restore + monitor-aware sanitize on restore; see `docs/settings.md`)
- Native file dialogs (teksilo-platform/file_dialog: `FileDialogBackend` trait + `FileDialogHandle` registered in app-state, `FileDialogRequest` builder for open / open-multi / pick-folder / save, `FileDialogResult`, `MemoryFileDialog` test backend, `RfdAsyncBackend` real implementation behind the `rfd-backend` feature using rfd 0.15 + xdg-portal + async-std; `EventContextFileDialogExt` extension trait adds `ctx.pick_file(...)`, `ctx.pick_files(...)`, `ctx.pick_folder(...)`, `ctx.save_file(...)`. Result delivery: backend posts `FileDialogEventPayload` through `AppEventPoster::post_external` → teksilo-app's `AppEvent::External` arm downcasts and routes to the originating window's tree → `FileDialogHandle::deliver` pops the callback and invokes it on the main thread with a fresh `EventContext`. macOS NSOpenPanel runs on the AppKit main run loop internally; the future drives the wakeup machinery from an async-std worker. Pending callbacks are tagged with the originating `TeksiloWindowId` and purged via `WindowManager::close_window` when the window closes — no use-after-free of widget state. Apps wire up with `TeksiloAppBuilder::install_file_dialog()` (or `.app_state(FileDialogHandle::new(my_backend))` for a custom backend). Demo: `examples/file_dialogs`.)
- External (OS) drag-and-drop (teksilo-platform/external_dnd: `ExternalDndBackend` trait + `ExternalDndHandle` registered in app-state, `ExternalDragEvent` `Entered`/`Moved`/`Left`/`Dropped` posted via `AppEventPoster::post_external`, routed by teksilo-app's `AppEvent::External` arm to `WidgetTree::{begin,update,end,cancel}_external_drag` which reuse the in-app drag pipeline. An OS drop is a `DragPayload` with `origin() == DragOrigin::External` carrying `files()`/`text()`/`uris()`; the same `on_drag_hover`/`on_drag_leave`/`on_drop` handlers fire. Per-OS backends: macOS `NSDraggingDestination` overlay view (verified), Windows OLE `RegisterDragDrop`/`IDropTarget`, Wayland `wl_data_device`, X11 XDND v5 (an `XdndProxy` `InputOnly` helper window on its own `x11rb` connection + thread — winit owns the toplevel's X connection and consumes XDND `ClientMessage`s itself with no hook; outbound owns `XdndSelection` and *polls* `QueryPointer` rather than grabbing, because winit's implicit grab from the ButtonPress makes `GrabPointer` fail every time). winit's own `DroppedFile`/`HoveredFile` are unused (no position, files-only, no Wayland). Apps wire up with `TeksiloAppBuilder::install_external_dnd()`; windows attach on create / detach on close. Ready-made `DropZone` widget consumes drops. `MemoryExternalDndBackend` for headless tests. Demo: `examples/file_drop`. **Outbound (app → OS) export is implemented**: a normal `start_drag` whose `DragPayload` carries `mime_data` auto-escalates to a native OS drag when the pointer leaves the window (`WidgetTree::try_escalate_to_os_drag` → `WindowOps::begin_os_drag` → `ExternalDndGuard::begin_drag`). macOS `NSDraggingSource` + Wayland `wl_data_source` are verified; Windows/X11 decline (the in-app drag stays alive). Completion is reported once via the `.on_drag_ended(|DropOutcome, ctx|)` source handler (`InApp{accepted}` / `OsCopy` / `OsMove` / `Cancelled`); advertised operation is Copy-only. The original typed payload is parked in an app-global stash and recovered if the OS drag re-enters any window (enabling drag-and-drop between two windows of the same app); re-entered payloads also expose `files()`/`text()`/`uris()` (via `DragPayload::enrich_external_from_mime`) so `DropZone`-style targets accept them too. New types: `DropOutcome`, `OutboundDragData`, `DragImageData` (teksilo-core). Reference: [docs/drag-and-drop.md §11.5](docs/drag-and-drop.md).)
- Cross-widget row-drag export (all five data views — ListView/TreeView/TableView/TreeTableView/GridView): rows dragged out of a data view are droppable **elsewhere** — a `DropTarget`, `DropZone`, another data view, or the OS — not just reordered in place. One public generic payload `RowDragData<T> { source: ViewId, rows: Vec<usize>, items: Option<Vec<T>> }` ([crates/teksilo-widgets/src/data_views.rs](crates/teksilo-widgets/src/data_views.rs)) occupies the single typed slot and serves BOTH same-view reorder (read `source`+`rows`) and foreign drop (read `items`); a foreign target does `on_drop_typed::<RowDragData<T>>()` / `accept_when(|p| p.get_typed::<RowDragData<T>>()…)` — no serialization. `ViewId` is kind-tagged + process-global (a `ListView` and a `TreeView` can never be misread as same-view — replaces the old per-view `NEXT_ID` collision hazard). Send builders on every view: `.exportable(DragTransferMode::{Copy,Move})` (carry item clones, `where T: Clone`; also makes rows a drag source without `.reorderable`), `.export_external(|&[T]| -> Vec<(mime,bytes)>)` (OS/DropZone MIME), `.on_rows_transferred_out` (override the Move removal). Receive: `.accept_foreign_rows(true)` + `.on_rows_received(|Vec<T>, insertion, ctx|)` (zero-custom-source), or a custom source's `DragSource::Foreign` branch; `TreeTableView` (non-pluggable) adds a raw `.on_foreign_drop(|&DragPayload, NodeId, DropPosition, ctx|)` hatch. Selection-aware multi-row drags (PointerDown collapse deferred to a release-without-drag so grabbing a multi-selection drags the whole set); Move-vs-Copy completion via `on_drag_ended` + a `self_reorder_flag` (never double-removes after an own reorder); move-out resolves a STABLE key at drag-start (correct across a mid-drag tree spring-load). The shared machinery lives in `data_views::RowExport<T>` (one field per view, not 8× duplicated) + `data_views::deferred_select`; `ListModel::move_items` block-move (emits `ItemsMoved` for a contiguous block so index selection follows) + defaulted `reorder_within` on both source traits (trees filter descendants + reject a drop into a dragged subtree). **Move caveat**: cross-window / cross-app is Copy-only (OS backends deliberately advertise Copy). Reference: [docs/drag-and-drop.md §12](docs/drag-and-drop.md). Demo: `cargo run -p drag-and-drop`.)
- Debug inspector (teksilo-inspector: in-app introspection panel, debug builds only, gated by `cfg(debug_assertions)`; F12 toggles a bottom panel with 9 tabs (Tree, Properties, Accessibility, Theme, Locale, Focus, Shortcuts, Overlays, Models); bounds-overlay visualization (Off/Selection/All) with cursor-following type+size tooltip and Padding/StackGap tinted bands; picker tool with multi-window subtree exclusion; theme JSON Export/Import; resizable panel with persisted height; tree filter input + auto-scroll-into-view; Properties Copy button + right-click `Copy value` context menu + Debug repr row; Models tab with click-to-select per row; panel-scoped Ctrl+P/Ctrl+B/Ctrl+Tab/Ctrl+Shift+Tab/Esc keyboard shortcuts; persistence via `__teksilo_inspector.*` settings keys when `SettingsStore` is wired. Apps opt in with `TeksiloAppBuilder.install_inspector_in_debug()` (no-op in release) — the extension trait is re-exported from `teksilo::prelude::*` behind the umbrella's default-on `inspector` feature, so no separate `teksilo-inspector` dep is needed. See `docs/inspector.md`. Data models opt into the Models tab via `ListModel::debug_named("…")` / `TreeModel::debug_named` / `SelectionModel::debug_named`.)
- WebView (teksilo-webview): embeddable HTML/web-content widget. The one widget that can't render into the wgpu surface — every engine (WKWebView / WebView2 / WebKitGTK / Servo) is a native OS subview composited on top of the wgpu pass. Mirrors the platform-backend pattern: swappable `WebViewBackend` → `WebViewHandle`, per-app `WebViewRegistry` (app-state) routes JS→Rust + lifecycle events back through `teksilo-app`'s `AppEvent::External` arm (`try_route_web_view_payload`) to the originating window's tree. The `WebView` widget opens its engine subview from a **post-mount `EventContext`** (the new `BuildContext::run_after_mount` hook — the only place the OS parent handle + `app_state` + poster are all reachable), tracks bounds via `place_children`, and **bridges framework dormancy → engine `set_visible`** via the new per-node **`activation_signal`** (arena flips it on `Active↔Dormant`, batched like `focus_within`): a `WebView` parked in a `Switcher`/`visible_when` branch hides its native subview (paint-pass dormancy alone can't, since the subview is outside wgpu). Tier-3 `WebViewStyle` overlay chrome; `Role::WebView` a11y (page tree published by the engine, not duplicated). Engine selection is feature-gated on the umbrella `teksilo` crate: **`web-view` = wry by default** (macOS WKWebView / Windows WebView2 / Linux-X11 WebKitGTK); **`web-view-servo`** (implies `web-view`) adds Servo, runtime-selected only under a Wayland session via `is_wayland()`; **`web-view-headless`** = widget + routing, no engine (`NoopWebViewBackend`, for tests / bring-your-own via `install_web_view(...)`). `MemoryWebViewBackend` records ops for headless tests. wry backend is functionally complete (build_as_child, IPC, navigation/page-load/title events, post_message); Servo backend is real-API + compile-verified but **not yet frame-driven** (Phase 4: wire `spin_event_loop`/`paint`/`present` + an `EventLoopWaker` into the render loop; whole-window GL conflicts with wgpu). Pinned `wry = 0.55.1`, `servo = 0.2.0`. Apps wire up with `TeksiloAppBuilder::install_web_view_default()` (or `install_web_view(backend)`). Reference: [docs/web-view.md](docs/web-view.md). Demo: `cargo run -p web-view-demo`.
- Automation MCP (teksilo-automation + teksilo-automation-mcp): a Model Context Protocol server that lets an AI agent **observe** (semantic AccessKit tree + screenshots) and **drive** (AT actions + synthetic pointer/key/IME input) a Teksilo app — in-process, without the OS accessibility layer. `teksilo-automation` is the GUI-free toolkit (core-only peer like `teksilo-data`): serde DTOs + one `execute(tree, ops, op, settle) -> reply` function + `RecordingWindowOps` (records `open_window` instead of panicking) + the 24-tool catalog. `teksilo-automation-mcp` is the rmcp (`rmcp 1.8`) server binary, `tokio`/`rmcp` confined to it. **Two modes:** `--headless` (default) owns a demo `HeadlessApp` on a dedicated `!Send`-tree thread (the async handlers marshal `Send` DTOs to it + a `oneshot`; screenshots render offscreen via `create_test_renderer` + `pollster::block_on` on that thread); `--connect <sock> --token <uuid>` drives a *live* app through its debug-only in-app bridge. **26 tools:** snapshot_tree/read_node/find_node/assert_node/list_windows, layout_tree/inspect_node (full widget/layout-arena tree + per-widget detail incl. Debug repr — the inspector's Tree/Properties data, richer than the AT snapshot since it includes widgets the AT tree prunes; keyed by the same node ids), invoke_action/focus_node/set_value/expand/collapse/scroll, inject_pointer/inject_key/type_text/type_ime/drag_node, get_overlays/get_shortcuts/list_live_regions/pull_announcements, advance_clock/settle/wait_for_condition, screenshot (MCP image block). The **live bridge** (`teksilo-app` `automation` feature, gated `#[cfg(all(feature="automation", debug_assertions))]`) binds a `0600` PID-unique Unix socket under `$XDG_RUNTIME_DIR` with a per-process UUID token, 4-byte-LE+JSON framing, single connection/inflight; reads framed `AutomationRequest`s and posts an `AutomationPayload` (Send reply `SyncSender`) through the existing `AppEvent::External` path → the winit main thread runs `execute` (or, for screenshots, the new `PlatformWindow::capture_offscreen`); the settle runs synchronously on the main thread. Framework deltas (Stage 0, all additive): `WidgetTree::dispatch_access_action`/`at_version() -> Signal<u64>`/`announcements_since(seq)` (live-region diff ring buffer + `Announcement` type)/`last_proposal()`; `WindowManager::teksilo_ids()`; `PlatformWindow::capture_offscreen`. Apps opt in with `TeksiloAppBuilder::install_automation_bridge_in_debug()` (umbrella `automation` feature; no-op in release / on Windows). **Debug-only, zero release surface.** Reference: [docs/automation-mcp.md](docs/automation-mcp.md). Smoke: `cargo run -p automation_bridge_smoke` (or `-- --serve` to keep it alive for an external `--connect` client). Run the server: `teksilo-automation-mcp --headless`.
- Widget previewer (teksilo-preview + teksilo-preview-ui + teksilo-widgets-previewer): Storybook-equivalent for desktop Rust widgets. `inventory`-backed registry where widgets self-register via `inventory::submit!(&'static dyn CatalogEntry)`. Typed `KnobSpec`/`KnobValue` for live property editing, `PreviewVariant` for multi-state showcasing, `SourceLoc` for "open in editor" navigation, PNG export per widget. 3-pane GUI (navigator/canvas/knob-form). Two CLI modes (standalone catalog vs single-widget targeting). Architecture splits trait+registry (no GUI dep, third-party widget libraries integrate cleanly) from the reusable GUI library (apps build their own previewer for app-specific catalogs) from the stock-catalog bundle binary. Mode C (VS Code extension with CodeLens "Preview ▶") designed but deferred. Run with `cargo run -p teksilo-widgets-previewer`.
- Tooltip system — three tiers sharing one attachment pipeline:
  1. **Plain** ([`TooltipWidget`](crates/teksilo-widgets/src/tooltip.rs)) — single-line localized text; ephemeral.
  2. **Rich** ([`RichTooltipWidget`](crates/teksilo-widgets/src/tooltip/rich.rs)) — registry-driven (`TooltipContent`), inline markup + shortcut chip + "more" Accordion; `[label](:key)` cascade to other rich tooltips; dwell-to-sticky flips the AT role to `Role::Dialog` + advertises a `Focus` action (does not auto-transfer focus — the user Tabs in).
  3. **Composite** ([`CompositeTooltipWidget`](crates/teksilo-widgets/src/tooltip/composite.rs)) — hosts arbitrary `impl Widget + 'static` body (CK3-style: TabWidget, charts, progress bars, conditional rows). Same dwell-to-sticky machinery, separate `composite_tooltip` token bundle, larger default 480 × 480 with vertical-scroll-as-needed. "Primary-only" by construction (no registry key, can't be a `:key` cascade target). Child widgets inside the body keep their own tooltip setters and cascade normally. `attach_composite_tooltip` / `attach_composite_tooltip_boxed` helpers; default delay `theme.motion.tooltip_delay_heavy` (400 ms).
  Per-widget setters are mutually exclusive (last-call-wins, every setter clears the other two): `.tooltip(...)` / `.rich_tooltip(key)` / `.rich_tooltip_content(c)` / `.composite_tooltip(w)`. Available on essentially every interactive control: buttons (`Button`, `IconButton`, `CommandLinkButton`, `SplitButton` + `.chevron_*` family, `PopoverWidget`/`Popover`, `NotificationCenterButton`); inputs (`TextInput`, `PasswordField`, `SearchField`, `SpinBox`, `TextScaleControl`, `ComboBox`, `HexColorInput`, `FilePickerField`, `DateEdit`/`TimeEdit`/`DateTimeEdit`/`DateRangeEdit`, `ColorEdit`/`ColorPicker`/`ColorSwatch`); selection controls (`Checkbox`, `RadioButton`, `Toggle`, `Slider`, `SegmentedControl` — per-`Segment`); `Avatar`, `Badge`, `Breadcrumb`, `Stepper`, `StandardListItem`/`StandardTreeItem`, `ToolBox`; the `ThemeSwitcher`/`LanguageSwitcher` presets (forward to their inner `ComboBox`); `Link`, `MenuItem`; and the tab/command data delegates (`TabInfo`/`TabDelegate`, `ToolbarAction`). **`Clone` value types** (`Segment`, `ToolbarAction`) take `.composite_tooltip` as a factory closure (`impl Fn() -> Box<dyn Widget>`) instead of an `impl Widget` instance, because `Box<dyn Widget>` isn't `Clone`. **Not** on `Toast` (a presentable request builder, not a `Widget` — its tooltip is data rendered by `toast/surface.rs`). The tooltip never leaks onto a control's own open overlay (dropdown / popover / calendar): a hover inside an active overlay whose content isn't the anchor's own subtree is suppressed (`WidgetTree::tooltip_hover_targets_anchor`). Reference: [docs/tooltips.md](docs/tooltips.md). Demo: `cargo run -p tooltips-showcase`.
- **Full widget catalog with source links**: [docs/widgets-overview.md](docs/widgets-overview.md). The bullets below are a quick-reference cheat sheet; the catalog is the authoritative list and the place to start for "what widgets ship?".
- Controls: Button, IconButton, CommandLinkButton, PopoverButton, PopoverIconButton, SplitButton, Checkbox, RadioButton, Toggle, Slider, ComboBox, **SegmentedControl** (mutually exclusive segments; selection is **keyed** by `SegmentId` (`Signal<Option<SegmentId>>`, the `TabId` pattern — a contributed segment can never silently re-point the selection; `SegmentedControl::indexed(Signal<usize>)` is the opt-in positional adapter for closed local lists, and `segmented_control::index_signal` bridges to a `Switcher`). Real **width story**: segments that don't fit move into a trailing chevron `MenuList` (`SegmentOverflow::{Menu,Compress}`) with **sticky promotion** — declaration order is stable except that the selected segment is always visible, taking the last slot until another is picked from the menu, and the promotion clears once everything fits again. Pure `overflow::plan()` planner + `measure_intrinsic` (measures dormant, overflowed cells too); the chrome paints from `SegmentSlots` geometry published each `place_children` instead of dividing bounds by `n`. Also `SegmentSizing::{Uniform,Fit}`, `SegmentDisplay::{Auto,Text,Icon,IconText}`, `fill_width`, `on_change(SegmentId, &mut EventContext)`, reactive per-`Segment` `disabled`/`visible` (read at event time, never snapshotted), `active_descendant` + pos/size-in-set over the whole list + Home/End + RTL arrows. One tab stop while everything fits, two while overflowing (the chevron). Reference: [docs/segmented-control.md](docs/segmented-control.md)), ProgressBar, Spinner, Link, Badge, SpinBox, Avatar, **TextScaleControl** (specialized `SpinBox`, 80 %–200 %, for the global "grow all text" accessibility setting — binds the persisted `teksilo_settings::TEXT_SCALE_KEY` and applies app-wide on edit. See **Global text scale** below + [docs/text-scale.md](docs/text-scale.md))
- Containers: Panel, Card, Accordion, ToolBox, ScrollArea, ScrollBar, Tooltip, **Splitter** (N-pane resizable split container — Qt `QSplitter` model: 2+ panes, N−1 draggable, collapsible dividers; shared serializable `SplitterModel` (`Rc<RefCell>` handle, the `SceneModel`/`ListModel` pattern) holding per-pane pixel size / min / max / stretch (container-resize policy) / collapsed; pure `distribute()` sizing engine — grow ∝ stretch, shrink ∝ room-above-min, container resize never writes back so drag positions survive resizes; animated collapse (reduced-motion aware) with four triggers (programmatic `set_collapsed`/`toggle_collapsed`, double-click divider, drag-past-min snap, Enter on focused divider); reactive per-pane hide/show (`set_pane_visible` — removes the pane AND its gutter, content dormant + handle Tab-skipped/AT-hidden; the "add/remove a pane from a fixed set" trick, no rebuild) distinct from collapse (which keeps the gutter); collapsed/hidden pane content parked dormant (out of paint/focus/AT/animation) and laid out at full size + clipped (no reflow); anti-jump pointer drag + keyboard resize (arrows/Home/End) + AccessKit Increment/Decrement/Collapse/Expand; `Role::Splitter` dividers carrying value/step/expanded + `controls` relations, optional named `Role::Group` panes via `.pane_label`; RTL-correct horizontal; Tier-3 `SplitterStyle`; `SplitterState` serde DTO (`Versioned`) via `export_state`/`import_state`, persisted through `SettingsFile` — compose all splitters into one app-layout struct, not one file each. Replaces the old two-pane `SplitView` (no backward compat); the building block for `DockingLayout`. Reference: [docs/splitter.md](docs/splitter.md). Demo: `cargo run -p splitter`), **DockingLayout** (VS Code-style dockable layout — a *layout*, not a window shell: a fixed **centre** slot + 4 collapsible/splittable/draggable side regions (leading/trailing/top/bottom), backed by a cloneable serializable `DockingModel`. **Four levels**: Side → one tab stack (in-side strip optional or replaced by an activity rail) → Tab → a `Splitter` of panes, **one `DockWidget` per pane** (a sole pane renders bare — the tab/rail is its header; a split pane is wrapped in an `Accordion` whose draggable header titles the dock, is its drag handle, and collapses the dock's content on click — header-only toggle/drag, content taps absorbed). Stacking two docks side-by-side adds a Splitter pane (each its own `Accordion`). **Per-corner ownership** (Qt `setCorner`) is computed directly in `place_children` by a pure `geometry::compute_rects` engine — nested Splitters can't express corners; corner-owner *degrades* when its side is hidden; all extents clamp non-negative (centre shrinks first). **Drag-to-dock**: drag a split pane's `ToolBox` header → a five-zone overlay (centre = `stack_into_tab` appends a Splitter pane, edge fifths = `split_into_tab` before/after); plus `promote_to_tab` / `move_dock` / `move_tab` (whole tab across sides, re-deriving Splitter orientation). **Activity rail** (`DockActivityBar`, `set_side_rail(side, px)`): always-visible `Role::TabList`, click-active-hides-the-side, the reopen affordance. **Sides** are one shown/hidden concept (animated, reduced-motion aware), resized by a `DockResizeHandle` (`Role::Splitter`, window-absolute anti-jump drag, arrows/Home/End, snap-to-hide). **Programmatic open-from-outside**: `reveal_dock`/`open_dock`/`toggle_dock`/`set_side_visible` + reactive `dock_open_signal`/`side_selected_tab_signal`/`side_visible_signal` for an external toolbar. **a11y**: sides `Role::Complementary` landmarks, rail/strip `Role::Tab`, handles `Role::Splitter`. `DockLayoutState` serde DTO (`Versioned`) via `export_state`/`import_state` (also reset-to-default; unknown ids dropped). Framework reuses gained capabilities for this: `TabWidget::bar_visibility(Always/WhenMultiple/Never)` + `compact_bar()` / `tab_bar_height(dp)` (denser 38 dp strip) + `TabSizing::Independent` (per-content tab width); `Accordion` gained `orientation`/`horizontal()` (rotated vertical header strip for top/bottom sides), a header **`trailing`/`trailing_id`** slot (an options `⋮` button / action toolbar placed before the chevron; the accordion wraps it in a **`DeadZone`** so the whole header still drags via `on_header_drag` EXCEPT the trailing controls — they can be clicked, even with click jitter, without starting the panel drag, and gap-taps there don't toggle the disclosure), `fill(true)` (content fills the pane via a custom layout — header + a `FillBody` that fills the leftover the enclosing Splitter pane gives it, clipped), `on_header_drag` (the dock drag handle), and **header-only** toggle/drag (content taps/drags absorbed so clicking the panel body never collapses/moves it). **Collapse is the Splitter pane resizing**: toggling a dock accordion drives `SplitterModel::set_collapsed` so the pane **folds to the header** (siblings grow) and **restores to the same size** on expand — wired in `build_pane_inner`. This needed two Splitter additions: per-pane **`collapsed_size`** (`PaneDescriptor::collapsed_size` / `set_collapsed_size` — a collapsed pane folds to this floor, e.g. the header height, not 0; a non-zero `collapsed_size` keeps the pane's content **live** while collapsed so the header sliver shows), and the widget **captures the displayed size into `stored_size` on collapse** (`set_stored_size_silent`) so the tween animates from where the pane actually is (independent of a stretch-grown pane's tiny fallback size); `ToolBox` collapsed-section content now goes **dormant** (`visible_when`) instead of clamped-to-zero (no overflow, no wasted layout) + `fill(true)` + `collapsible(true)`; `DockResizeHandle` renders via the active `SplitterStyle::make_handle` so its grip/colours/sizes/hover match a Splitter divider; **`DockActivityBar`** is vertical-only with top-pushed items, a selectable one-size-for-all item size (`IconButtonSize`), `top_slot`/`bottom_slot`, and capacity-based **overflow** into a caller-chosen trigger. The rail always hugs its side's **leading edge**: leading/trailing → the outer (window) edge; **top/bottom → a vertical column on the leading cross-edge** (left in LTR, right in RTL, content inboard) so the vertical rail reads naturally, NOT a thin horizontal strip (`geometry::split_side` branches by axis; `SideLayout::band_depth` excludes the cross-axis rail from a top/bottom band's depth; a hidden top/bottom band collapses **completely** — rail included, since a vertical rail can't stand in a zero-depth band — so reveal it via an external button (`set_side_visible`/`reveal_dock`), unlike a leading/trailing rail which persists; `compute_rects` takes `rtl` for the cross-edge). The overflow trigger (`DockRail::overflow_icon`) opens a popover of the surplus entries; the rail is configured per side via `DockingLayout::rail(DockRail::new(side).size(..).top_slot(..).bottom_slot(..).overflow_icon(..))`. **Rail width follows the size mode** (the rail thickness is derived from the effective item size — Compact shrinks the whole strip, not just the items; the `DockingLayout` binds `rail_size_signal` at `Relayout`). Slot factories stay plain `Fn() -> impl Widget` (consistent with all other slots); a slot that wants to match the rail size binds `DockingModel::rail_size_mode_signal(side) -> Signal<DockRailItemSize>` (the public reactive mode signal, also for any external toolbar) and the rail rebuilds its slots on each change. **The rail is also a drop target** (the `TabWidget` reorder + `accept_external_tabs` analogue): each `DockRailItem` upserts its world bounds into a shared sink keyed by visible position during layout; the `DockActivityBar`'s own `on_drag_hover`/`on_drop` compute an insertion position from the pointer vs those bounds (pure `rail_insertion` fn), paint a horizontal line via a topmost `RailDropIndicator` overlay (`Signal<Option<f32>>` bound `RepaintOnly`, `event_pass_through`), and route the drop — a whole dock tab (a rail item from this side = internal reorder, or a tab dragged from another side's rail/strip = external move; both `move_tab`, the visible index mapped back through the shown→model-index table exactly like the strip's `on_reorder`/`on_tab_received`) or a single dock (`promote_to_tab`, becomes a new activity at the line). Recognised via the same `dropped_dock_tab` (`DockTabDragData` + `TabBarDragData<TabHandle>`) / `dropped_dock_widget` helpers the side panel uses; an empty / hidden Rail side accepts the first drop this way (and reveals). **Cross-surface wiring**: a rail item dropped on *another side's tab strip* needs the dock `TabWidget`'s `on_external_drop` (panel.rs) — the strip bar is the drop target (`find_drop_target_at_or_above` stops at the first handler) and natively only accepts `TabBarDragData<TabHandle>`, so without it a rail-item `DockTabDragData` is silently rejected; `on_external_drop` routes it to `move_tab` / `promote_to_tab` at the bar's insertion index. **Selection-sync gotcha**: the `DockSidePanel` model↔TabWidget selection effects must BOTH resolve index↔id against the *live* model (`DockingModel::tab_id_at` is the live inverse of `select_tab_by_id`) — a build-time `all_tab_ids` snapshot in the model→TabWidget direction disagrees with the live id→index direction after a reorder and feeds back unboundedly (the "Signal notification nested 257 deep" panic on import-an-activity-then-reorder-it). **Context menus** (auto-wired, [docking/context_menu.rs](crates/teksilo-widgets/src/docking/context_menu.rs)): right-click a rail item or a dock tab → `Hide "<activity>"` / `Move to ▸ <other sides>` (`move_tab`) / a checkable activities list / `Activity bar size ▸ Default·Compact·Icon+Label` (rail) or `Tab size ▸ Text·Icon·Icon+Text` (dock tab). The menu lives **only** on tabs, rail items, and the `DockActivityBar` — NOT on panes/accordions/content (the context-menu factory walks up the tree, so a menu on the side panel would leak onto everything). **Restore-when-all-hidden**: Rail presentation → right-click the always-visible rail (`background_menu`); Strip presentation → the tab bar keeps a trailing **hamburger** (`IconButton::menu()` in `bar_trailing_slot`, shown only when no tab is visible) opening the same menu, placed `BelowPreferred` so it flips/clamps on-screen for a bottom bar. "Hide" is **hide-but-remember** — a per-tab `hidden` flag (`DockTab.hidden`, restorable via the checklist), NOT close; selection hands off to the nearest visible tab. New reactive per-side prefs `DockRailItemSize` (`Default`/`Compact`/`Labeled` — `Labeled` adds a 90°-rotated title under the rail icon, reusing `ToolBox`'s `RotatedLabel`; the others are icon-only with the title as a hover tooltip) / `DockTabDisplay` (`Text`/`Icon`/`IconText`) (signals the rail/strip bind at `Rebuild`, persisted in `DockSideState` as `rail_size: usize` / `tab_display: usize`); both rail and tab strip fall back to the title's **initial letter** when a dock has no icon in an icon-only mode (so the mode is never a silent no-op; the full title stays in the tooltip); model API `set_tab_hidden`/`is_tab_hidden`/`side_visible_tab_count`/`select_tab_by_id`/`set_side_rail_size`/`set_side_tab_display`. Per-tab menus on a plain `TabWidget` are available generally via the new `TabInfo::context_menu(..)` (threaded through `build_delegate`). Rebuild/Relayout split: structural change rebuilds, resize/show-hide/tab-switch are relayout/repaint (content preserved across those). **Lock-down policy** (`DockPolicy` on the model, app-declared, NOT persisted): four global `allow_*` flags — `activity_drag` (rail/tab drag + reorder + context-menu "Move to"), `dock_drag` (split-pane accordion drag handle), `side_collapse` (resize-handle hide actions + rail click-active-hide — resize itself stays), `activity_hide` (context-menu Hide + checklist) — each gates only the **user affordance**; the programmatic API (a "Toggle panel" button, `open_dock`, `set_tab_hidden`) keeps working. Threaded with ZERO constructor changes: every gate reads `self.model.policy()` at build time (the rail/strip/accordion conditionally register their drag handlers; gating the *source* leaves the drop-accept branches inert, so no double-gating). Plus per-side `set_side_enabled(side,false)` (reactive): a disabled side renders 3 transparent placeholders (fixed `ordered` indexing preserved), gets zero geometry, and the placement mutators (`open_dock`/`move_tab`/`promote_to_tab`/`split_into_tab`/`stack_into_tab`) reject it via `Self::side_accepts` — docks already on it survive and reappear on re-enable. `set_policy`/`policy`/`set_side_enabled`/`is_side_enabled` on the model; `DockingLayout::policy(..)`/`.disable_side(..)` builder sugar; `DockPolicy::locked()` = all forbidden. **Dock header & options** (the VS Code / IntelliJ "view header" pattern): a per-dock header carries (a) app-supplied inline actions — `DockWidget::header_actions(|id| …)` returns a flat **list** of action widgets (e.g. `IconButton`s: New / Collapse All / refresh); the framework arranges them + the `⋮` along the header's axis (a horizontal `HStack` row on leading/trailing, a vertical `VStack` column on the rotated top/bottom strip), so apps never pick the orientation — and (b) an always-visible **`⋮` options menu** (`context_menu::dock_options_menu`, omitted when empty via `dock_has_options`): for a pane in a grouped activity → `Move to new activity` (`promote_to_tab`) / `Move to side ▸`; for a sole-pane dock → `Hide` / `Move to ▸`. There is **no Close** — a dock can only be hidden (restored from the activity checklist / rail-strip background menu); closing would leave no way back. `DockWidget` therefore has no `closable` knob. Split panes always have the `Accordion` header (actions + `⋮` go in its `trailing_id` slot); a **bare** sole-pane dock is headerless unless it opts in with **`DockWidget::show_header(true)`** (then `[title][Spacer][actions][⋮]` above its content, built by `panel::build_bare_dock_header`). **Activity names**: a tab's label derives title → `set_tab_title`/`set_dock_activity_title` (app-config, NOT persisted — set a stable name on a grouped multi-dock activity so the rail/tab reads "Source" instead of silently tracking pane 0) → first **non-collapsed** pane's dock title (`activity_label`/`primary_dock_of`) → "Panel". **Availability-aware "Move to"**: all relocation menus iterate `DockingModel::enabled_move_targets(from)` (enabled sides only; the "Move to" entry is omitted when none remain) — a disabled side is never offered as a silently-rejected target. Review bug fixes: `set_tab_hidden` re-homes selection off the last hidden tab (→ 0); `import_state` clamps a persisted selection off a hidden tab; the activity-checklist checkmark binds the model's live `tab_hidden_signal` (was a dead snapshot `Signal`); whole-tab appends use `side_append_index` (after the last *visible* tab, not past trailing hidden ones); a drop whose hover target was destroyed mid-drag re-hit-tests instead of vanishing (teksilo-core `revalidate_interaction_state` + `handle_drag_drop` clear/ignore a stale `active_drag.current_target`). **v1 limitations**: content rebuilt on *structural* moves (not yet preserved across them), no floating/cross-window/recursive-nesting, RTL handle direction is a follow-up. Reference: [docs/docking.md](docs/docking.md). Demo: `cargo run -p docking`), **TabWidget / TabBar / Tabs** (data-source-driven `TabBar<T>` with `TabDelegate<T>` for per-tab label/icon/leading/trailing/context-menu/closable/pinned/enabled/tooltip; `TabSizing::Shared` (uniform extent) vs `Independent` (per-content); bar-level `TabDisplayMode` (`Auto`/`Text`/`Icon`/`IconText`) via `tab_display(mode)` / `tab_display(Signal)` — each tab declares title+icon, the mode decides what's painted: `Icon` blanks the label so the header **sizes to its icon** (title → tooltip, icon-less falls back to the title's initial), `IconText` grows to fit both; horizontal scroll with leading + trailing arrow buttons + mouse-wheel-to-horizontal mapping + Shift+wheel; "show all tabs" overflow `Popover` dropdown, gated by `TabOverflowButton` (`Auto` default — shown only when the headers overflow the viewport, like the scroll arrows / `Always` / `Never`; `show_overflow_dropdown(bool)` is the `Always`/`Never` shorthand); close button on closable tabs + middle-click close + selection adjustment on close; drag-to-reorder with insertion-line indicator + edge auto-scroll; pinned tab strip (icon-only, fixed-width, no close button — Firefox/Chrome convention) at the leading edge separate from the scrollable region; locale-reactive labels + AT names; horizontal and vertical orientations (multi-line/multi-row wrapping is the one layout mode not yet implemented). **Appearance API** (`TabBar` + forwarded on `TabWidget`): per-state tab backgrounds `selected_tab_background`/`hover_tab_background`/`idle_tab_background` with the `tab_background` all-states shorthand (precedence selected>hover>idle, each falling back to shorthand then transparent; rendered as three `visible_when`-gated `RectWidget`s so a state change is a repaint, not a rebuild); `bar_background` for the strip backdrop **decoupled** from the per-tab fill (the old `tab_surface_role` knob — which did both — is renamed to `tab_background` and no longer tints the bar); per-state text **colour** via `selected_text_role`/`idle_text_role`; `tab_dividers()`/`tab_divider_color(..)` for a 1 dp line between tabs in **both** strips (scrollable row = on-top `TabRowDividers` overlay reading `header_bounds_buf`; pinned strip = interleaved `Divider` widgets); `active_indicator(TabIndicatorPosition::{OuterEdge,InnerEdge})` moves the active-tab highlight to the inner edge (below the label horizontally / trailing vertically) — threaded through `TabStyleConfig::indicator_position` and honoured by `RecipeTabStyle` for all four edges, RTL-correct on the vertical axis. See [docs/tab-widget.md](docs/tab-widget.md) "Appearance". Generic `Tabs<T>` composes `TabBar<T>` above a `Switcher` driven by a content delegate.), Dialog, Popover, Snackbar, **DropZone** (standalone external/OS "drop files here" target — `accept_extensions` filter, `allow_multiple`, `on_files_dropped`/`on_text_dropped`/`on_urls_dropped`, keyboard Browse fallback, Tier-3 `DropZoneStyle`, `Role::Group` + `Live::Polite`; consumes drops from `install_external_dnd()`. Demo: `cargo run -p file-drop`), **DropTarget** (wrapping drop container — turns any child into a drop target without hiding it; the child stays visible and the highlight is a border, not a fill. Reacts to internal (typed `DragPayload`) AND external drops; optional centered hint popup (`.hint(...)`, gated by `visible_when` so it's culled from paint+AT while idle); `accept_external_*`/`accept_typed::<T>`/`accept_when` filters, `on_drop`/`on_drop_typed::<T>`, `targeted_signal`/`drag_state_signal` (SwiftUI `isTargeted`); `DropTargetVariant` Default/Prominent/Subtle/None border weight; Tier-3 `DropTargetStyle`, `Role::Group`. **Optional multi-zone drops** — expose any of `DropRegion::{Center,Top,Bottom,Leading,Trailing}` via `.region(region, |z| z.hint(w).enabled(sig))`, each independently declare-able with its own optional hint AND a reactive `.enabled(impl Into<Prop<bool>>)` (default true — a bound `Signal<bool>` disables a zone **live, no rebuild**: its strip falls through to the next-priority enabled zone / `Center` / reject, and it stops highlighting + hiding its hint). Route by which zone released via `.on_region_drop(|region, payload, pos, ctx|)` (or observe `.active_region_signal(..)`). The four side zones share one `.zone_size_factor(0.1..=1.0)` (fraction of the axis each edge strip occupies — `0.2` = fifth, `0.5` = bisect); the active zone highlights (centre = rounded full-bounds frame via a `RectWidget` so content shows through, edge = translucent fill + accent frame via `DropRegionOverlay`) and only that zone's hint shows (centered + clamped to its zone rect). Declaring any region → exactly those regions; none → the `Center`-only whole-bounds default (`.hint(w)` == `.region(Center, |z| z.hint(w))`). A drop in a middle covered by no enabled zone is **rejected** (hover never engages there; `on_region_drop` only ever gets an enabled region). `Leading`/`Trailing` map to left/right (no RTL flip yet). This is the reusable form of `DockingLayout`'s hand-computed five-zone drag-to-dock overlay (`compute_drop_zone`/`DockDropOverlay`); the pure hit-test (`region_at`) + geometry (`region_rect`) + `DropRegion`/`DropRegionSet` live in `teksilo-core::styles`. Demo: the internal drop targets (single + multi-zone) in `cargo run -p file-drop` / `cargo run -p drag-and-drop`. Reference: [docs/drag-and-drop.md §11.6](docs/drag-and-drop.md)), **Toast** (stackable, action-rich, severity-aware floating notification — `info`/`success`/`warning`/`error`/`loading` constructors, Link + Button actions, `Toast::id` update-in-place, hover-pause-group, `Role::Alert`/`Status` per severity × priority, persistent `NotificationArchiveModel`-backed log via [`NotificationLog`](crates/teksilo-widgets/src/notification/log.rs) / [`NotificationCenterButton`](crates/teksilo-widgets/src/notification/center_button.rs) / [`NotificationLogDialog`](crates/teksilo-widgets/src/notification/log_dialog.rs); one-line `TeksiloAppBuilder::install_toast_default()` wires the host + archive + bell glyph. Demo: `cargo run -p toast-demo`. Reference: [docs/toast.md](docs/toast.md)), Wizard, Breadcrumb, GroupBox, MessageBox
- Menus: MenuBar, MenuList, **MenuItem** (`Plain` / `Check` / `Radio` modes — `.checked(Signal<bool>)` (two-way: activation flips the signal — the signal *is* the source of truth), `.reflect_checked(Signal<bool>)` (**reflect-only**: the checkmark mirrors the signal but activation does NOT write it — the truth lives elsewhere and the item's `on_activate`/intent owns the change; the "View ▸ Sidebar / Full Screen" pattern that mirrors externally-owned state like `DockingModel::dock_open_signal`, where two-way `checked` would fight the model), `.check_state(Signal<CheckState>)` for tristate, `.radio(value, Signal<usize>)`; emits `Role::MenuItemCheckBox` / `Role::MenuItemRadio` with `set_toggled` matching `Checkbox` semantics; tristate click cycles Unchecked↔Checked only — Indeterminate stays external-source-only; radio items in the same `MenuList` auto-group via `Signal::same` and announce "2 of N" via `push_to_radio_group`; check / radio glyph rendered in the existing 16dp leading slot via a reactive `Switcher`), MenuContext (context menu). **Mnemonic markers** use the in-string Windows / Qt `&` convention (`&Save` → underline 'S' when Alt is held; `&&` → literal `&`). `MenuLabel` (private leaf) draws the underline via `canvas.fill_rect` at `bounds.y + layout.height - thickness` (bottom of the laid-out text box — reliable across fonts; the font-metric `underline_offset` from text-typeset is often too small and lands inside the descender zone, visually cutting capital letters). The underline is gated on `WindowState::alt_down` AND `!cfg!(target_os = "macos")` — see the macOS note below. AT name strips the `&`, mnemonic letter goes onto `inner_mut().set_access_key("S")` for Windows Narrator. **Keyboard navigation**: ArrowUp/Down + wrap, `Home`/`End`, Enter/Space activates the focused item, ArrowRight opens a submenu, ArrowLeft/Esc bubble to the overlay host. **Type-ahead** (500 ms default reset, ASCII case-fold; `.type_ahead_timeout(d)` to override; respects current focus as the search-start anchor; separators skipped). **In-menu mnemonic activation**: bare letter (no modifiers) inside an open menu activates the item whose `&`-marker matches. Mnemonic wins over type-ahead when both fire. **Window-level menubar dispatch** for F10 / `Alt+<letter>` / bare-Alt-tap: `MenuBar::build` constructs an `Rc<dyn MenubarDispatcher>` and installs it into `WindowState` on every platform. `teksilo-app` consults the slot BEFORE focus-based dispatch on every `KeyboardInput` so the chord intercepts even when focus is in a `TextInput`. Dispatcher returns `MenubarAction::{OpenMenu, FocusTrigger, Intercept}`; F10 = Alt-tap = focus first trigger (no menu opens); Alt+letter = open matching menu (silent intercept on no match — prevents garbled text input); Alt-tap detected on the falling edge of `WindowState::alt_down` with `other_key_pressed_during_alt == false`. **Declarative `MenuModel` + native macOS menu bar**: a widget-free [`MenuModel`](crates/teksilo-widgets/src/menu/model.rs) (`MenuModel::menu(title, \|m\| m.item(MenuEntry::new(..).intent("app.x").shortcut("app.x")).separator().submenu(..))`, plus `.checkable` (two-way) / `.checked` (reflect-only, mirrors `Signal<bool>` without writing it — pair with `.intent`/`.on_activate`) / `.tri_checkable`/`.radio`/`.enabled`/`.visible`/`.standard(StandardMenuRole)` or `.standard_menu(StandardMenu::app()…)`) is the single source of truth shared by the in-window bar (`MenuBar::from_model(model)`) and the OS menu bar. Standard-menu chrome (App About/Hide/Quit, Window Minimize/Zoom) is **localized** — `StandardMenu` carries `LocalizedString` labels resolved in the widget layer (English `lit!` defaults; pass `tr!`), so the platform crate never hardcodes English; a leading App menu is auto-injected (localized) when the model declares none. `MenuBar::native_on_macos(NativeMenuMode::{Off,Suppress,Coexist})` mirrors the model into `NSApplication.mainMenu` on macOS (Suppress also hides the in-window strip — the native-looking default; non-macOS ignores the flag and renders in-window). App opts in with `TeksiloAppBuilder::install_native_menu()`. Each `MenuEntry` carries a process-unique `MenuItemId` (teksilo-core); the macOS `NSMenuItem` callback posts a `NativeMenuEventPayload` via `post_external`, routed back into the focused window's tree to fire the item's intent/action with `IntentSource::Menu` (same pipeline as in-window). Reactive `enabled`/check/radio `Signal`s update the live `NSMenuItem` in place (`update_item`); the global bar follows window focus (`activate_window` on `WindowEvent::Focused`). `.shortcut("id")` becomes an `NSMenuItem` key equivalent (AppKit fires it directly → no double-fire with the in-app dispatcher); Ctrl/Super → ⌘ (Qt-style cross-platform mapping). The platform boundary is a plain `NativeMenuSnapshot` ([teksilo-platform/native_menu](crates/teksilo-platform/src/native_menu.rs), `NativeMenuBackend` trait + `NativeMenuHandle` + macOS `NSMenu` backend; `NoopNativeMenuBackend` elsewhere — extensible to Windows `HMENU` / Linux DBus). **Drive it from anywhere**: fire the intent (`ctx.send_intent`) to *trigger*; bind a `Signal` to `MenuEntry::enabled`/`visible`/`checkable`/`radio` and `.set()` it to change *state* live (no rebuild — `enabled`/`visible` are reactive on BOTH surfaces now; `MenuItem::enabled` takes `impl Into<Prop<bool>>`). **Dynamic structure**: `MenuModel` is a cloneable handle with `&self` mutators — `push_item(into_submenu_id, entry)` / `push_separator` / `push_menu(title, …) -> MenuItemId` / `remove(id)` / `modify(\|&mut Vec<MenuNode>\|)`; each bumps `version`, and `from_model` binds `version` at `BindingLevel::Rebuild` so the in-window dropdowns re-derive and the native menu re-installs. Address submenus via `menu_with_id`/`submenu_with_id` (pre-allocate `MenuItemId::next()`). Demo: `cargo run -p native-menu`. Reference: [docs/native-menu.md](docs/native-menu.md).

  **macOS limitation (documented; intentional)**: on macOS, the OS rewrites Option+letter for accented character composition (Option+E → ´, Option+F → ƒ, …) *before* winit hands the keystroke to the app. The translated logical key never matches the mnemonic table, and silently intercepting the chord would break legitimate accented text input system-wide. The dispatcher's Alt+letter branch is therefore compiled out on macOS (`#[cfg(not(target_os = "macos"))]` inside `MenuBarDispatcher::try_handle`), and the mnemonic-underline visual is hidden (`!cfg!(target_os = "macos")` gate in `MenuLabel::paint`) so the UI doesn't promise a chord that won't fire. F10 and bare-Alt-tap continue to work on macOS — neither involves a letter key that the OS rewrites. The same applies to the menubar dropdowns: once a menu is open, bare-letter in-menu activation works on macOS too (the OS only rewrites Option+letter, not letter-alone). Recommended macOS keyboard path: F10 → arrows → Enter, plus the existing `Shortcut` system for Cmd+? accelerators (which are not rewritten by the OS).

  **Mnemonics never enter `ShortcutRegistry`** — by construction `ShortcutSettings` cannot show them, which is the desired behaviour (they're derived from labels, change with locale, and are not user-rebindable per Win32 / GNOME HIG). **Safe-triangle submenu hover gate**: when a submenu opens, the trigger MenuItem stamps a shared anchor (cursor position at open) into the enclosing `MenuList`'s `SafeTriangleState`; sibling items, before firing their hover-switch, call `point_in_safe_triangle(cursor, anchor, submenu_bounds)` — the triangle's near edge is inferred from `anchor.x` vs `submenu.x` so the algorithm is automatically RTL-symmetric. The anchor is cleared by the overlay's dismiss callback (when the submenu actually closes) OR by the trigger's hover-leave WHEN the overlay never opened (the 400 ms delay was cancelled mid-pending) — clearing on every hover-leave defeats the gate because trigger-leave fires before sibling-enter. EventContext exposes `tree_pointer_position()` + `overlay_bounds_for_content(content_id)` (snapshotted per dispatch). Existing 150 ms `PointerLeave` close stays as a graceful fallback.
- Chrome: Toolbar, StatusBar, TitleBar, GroupHeader
- Data-driven: ListView, TreeView (with 4-arg `new_with_context(...)` exposing a `TreeRowContext` for one-line chevron-toggle wiring), Repeater, **GridView** (virtualized 2D tile grid bound to `ListModel<T>`/`ListDataSource` — the photo-gallery / icon-view / collection-view widget; pluggable `GridLayoutStrategy` with three shipped strategies: `UniformGrid` (fixed size / fixed column count / adaptive min-width, exact O(1)), `VariableRowGrid` (each row sized to tallest tile — auto-measure + scroll-anchoring via a binary-search prefix-sum offset table, or exact `.item_height(i)`), `VirtualizedMasonry` (Pinterest waterfall); flat `SelectionModel` (Single/Multi) with click/Ctrl/Shift + rubber-band marquee (`select_indices`); full 2D keyboard nav (arrows ±1/±cols, Home/End, Ctrl+Home/End, PageUp/Down, Tab, type-ahead, Alt+Arrow reorder) with a container-painted focus ring; drag-to-reorder routed through the bound source's `drag`/`can_accept`/`accept_drop` capabilities (insertion bar shown only on an accepted hover) + an `on_item_drop` escape hatch for foreign/external payloads; per-tile `on_tile_activate` (double-click/Enter) + `tile_context_menu`; sections via `SectionProvider`/`grouping_sections` with in-flow + sticky pinned headers; `empty_view`/`loading_view`/`is_loading`; source-driven lazy loading (`request_window(start..end)` each realize pass + `can_fetch_more`/`fetch_more` near the end + `RowState::Loading` placeholder rows — no view-level `on_near_end`); `Role::Grid > Role::GridCell` with logical row/column counts, `pos_in_set`/`size_of_set`, `active_descendant` roving focus, `Role::RowHeader` sections. Body lives in a `GridBodyPane` sibling-of-scrollbar (survives mid-thumb-drag rebuilds); a post-measure realization re-check rebuilds the pane when tiles measure shorter than the estimate, so the viewport never shows a bottom gap. Tier-3 `GridViewStyle` recipes (focus-ring / marquee / insertion-bar / pinned-header surface) via `.style(...)` or `theme.style_slots.grid_view`. Self-registers a previewer catalog entry. Reference: [docs/grid-view.md](docs/grid-view.md). Demo: `cargo run -p grid-view`), **TableView** (multi-column, virtualized, sort/filter via `SortFilterListModel`, drag-resize + drag-reorder of columns, pinned Leading/Trailing, cell-level + row-level selection, full keyboard nav with focus ring, edit hooks via `editing_cell_signal` + `on_cell_edit_request`, row drag-drop reorder, `Role::Table > Role::Row > Role::Cell` accessibility), **TreeTableView** (hierarchical multi-column, twist-arrow indent, ArrowLeft/Right collapse/expand, `Role::TreeGrid` with per-row `set_level`/`set_expanded`; rows live in a `TreeBodyPane` sibling-of-scrollbar — buffer-exit/selection/expand rebuilds survive mid-thumb-drag, like TableView's `BodyPane`). **Variable row heights on all four row widgets** (ListView/TreeView via `.item_height_fn(..)` / `.auto_item_height(est)`, TableView/TreeTableView via `.row_height_fn(..)` / `.auto_row_height(est)`): shared `RowMetrics` (common/row_metrics.rs) wraps uniform / exact-callback / auto-measure modes over the `PrefixSumOffsets` table extracted from GridView (common/row_offsets.rs); auto mode measures realized rows height-for-width with scroll anchoring + a realization re-check, and model changes invalidate from the projections' `first_changed_index()` so measured prefixes survive appends and expand/collapse — see docs/table-view.md "Row heights", **StandardListItem** + **StandardTreeItem** (canonical row layout — `[checkbox?] [leading_slot?] [center_slot?] [label] [Spacer] [trailing_slot?]` with optional subtitle line carrying its own `[subtitle_leading_slot?] [subtitle] [subtitle_trailing_slot?]`; selection bg mirrors MenuItem/ComboBox rounded `item_corner_radius: 8.0` / `SurfaceRole::Selected | AccentSubtle | Pressed`; tree variant adds depth-driven indent + always-reserved chevron column; `.from_entry(&FlatEntry)` shortcut, `.on_toggle_rc(ctx.toggle_callback())` from the new TreeView delegate; both accept two-state `Signal<bool>` or tri-state `Signal<CheckState>` checkbox; `_literal` shims for untranslated strings)
- Text: TextInput (styled single-line), **PasswordField** (secure entry — embedded reveal toggle via `IconButton::visibility_toggle`, character masking at the text-engine layer so plaintext never reaches the shaper/atlas/AT value while masked, Caps Lock warning, clipboard suppression while masked; `EchoMode` Masked/NoEcho/RevealWhileTyping, `RevealMode` Toggle/Hold/None, `AtRevealPolicy` SwapRole/AlwaysProtected; `Role::PasswordInput`. Built on a secure `TextInputField` — `.secure(EchoMode)`/`.echo_char`/`.revealed`/`.allow_copy`/`.at_reveal_policy`. Caps state via `WindowState::caps_lock` (`Key::CapsLock`); IME-readiness via the `ime_allowed` arena node flag. Demo: `cargo run -p password-field`), rich text viewer; `RichTextEditor::editor` / `read_only` accept `.min_lines(n)` / `.max_lines(n)` for intrinsic-mode sizing (greedy by default; intrinsic when either knob is set, clamping `content_height` to `[min, max] × default_line_height` — the messenger-composer pattern)
- Scene viewport (teksilo-scene: `Scene`, `SceneView`, `SceneItem` trait, built-in `RectItem` and friends, `minimap`). Two-tier — heavyweight `Widget` (full focus/animation/DnD/AT survives embedding) + lightweight `SceneItem` (paint-only, no arena cost). Pan / zoom gestures with per-axis policy, drag modes, exact-shape hit-test via `shape_contains`, per-item `CacheMode::ItemCoordinate` GPU cache, collision API, reactive `item_change_signal`, background/foreground paint hooks, signal-driven dynamic bounds, selection, z-order, removal. **Shared model + per-view delegate (multi-view)**: `SceneModel` (cloneable `Rc<RefCell<Scene>>` handle) drives N `SceneView::with_model(model.clone())` panes; heavyweight content is a type-erased payload (`add_widget_item(payload, rect)`) built per-view by `delegate_typed::<P>(|&P, ItemId| Box<dyn Widget>)`, so each pane owns its own arena instances. Single-view `Scene::add_widget(w, rect)` (the `Once` path, drained by the first view) stays as sugar. **Live runtime mutation**: every `SceneModel` mutator is `&self`, so a handler holding a clone (`view.model()`) drives the scene directly — no `with_widget_mut` needed for content; `with_widget_mut::<SceneView>(id, Relayout, |v| v.ensure_visible(..))` is for per-view camera. Each view self-reconciles on every `ItemChange` variant (add materialises, remove destroys the orphaned arena widget + cleans maps, move/transform/reparent/visibility re-place + re-walk, `PayloadChanged` rebuilds that item's widget) AND on a separate `Scene::a11y_change_signal` for pure-a11y mutations (group add, reparent, live, landmark, categories) — so the visual tree and the separate AccessKit tree both follow per-view (`build()` calls `request_accessibility_update()`, since relayout no longer re-walks AT). App-injectable view state via `view_state(pan_x, pan_y, zoom, rotation)` / `initial_pan` / `initial_zoom` / `initial_rotation` so view state survives a rebuild-from-state; selection is per-view, opt-in shared via `.selection_model(SceneSelection)`. **Accessibility-complete**: every visible heavyweight widget participates as a normal child; every visible lightweight item gets a synthetic AT node with role + screen-projected bounds; Tab cycles in scene-insertion order. Override surface mirrors widget-level: logical groups, reparenting (visual tree ≠ AT tree), relations (controls/described_by/labelled_by), live regions, landmark roles, rotor/quick-nav categories, subtree mode (`Merge`/`Exclude`), custom focus order, `access_*` builder chain. Intended for story corkboards, mind maps, node-graph editors, timeline views, CAD canvases, simple maps. **Magnetism** (typed snap-and-connect between item anchors): `Magnet` (local point + advisory `MagnetRole` Source/Target/Bidirectional + type-erased payload), per-view `SceneView::magnetism(MagnetismConfig)` with a consumer accept/reject predicate + `on_connect`; three input methods sharing one mechanism — item-drag-snap (mouse, lightweight items) + port-drag wires (grab a magnet handle) + a keyboard connect flow (`m` then arrows/Enter, works for any item kind); reusable `SceneModel::compute_item_snap`/`compute_port_snap` for heavyweight consumers driving their own drag; built-in feedback renderer (markers + ghost wire, `MarkerVisibility`); mechanism-in-scene/policy-in-consumer (scene stores no connection state — persistent edges vs one-shot reparent is the consumer's choice); synthetic `SceneMagnet` AT nodes + roving `active_descendant` keyboard flow. Demos: `cargo run -p scene_showcase`, `cargo run -p scene_corkboard`, `cargo run -p scene_magnetism`. References: [docs/teksilo-scene.md](docs/teksilo-scene.md), [docs/teksilo-scene-a11y.md](docs/teksilo-scene-a11y.md).
- **RichTextEditor / RichTextViewer** (rich_text/ module: state, keyboard, mouse, hit_test, paint, clipboard, context_menu, frame_loop, policy, image_cache) — a full QTextEdit-class rich editing surface over the external `text-document` model (blocks, lists, tables, formats, **undo/redo**) + `text-typeset` (shaping/bidi/line-break/atlas). **112 tests** (count, not a guarantee of adequacy — see Caveats), exercising: IME composition (commit + cancel-leaves-document-clean), HTML + plain-text clipboard with self-fragment → HTML → plain paste preference + CRLF normalisation + paste-unformatted, tables (Ctrl+A scope in/out, Ctrl+Enter block insert), lists (Tab/Shift+Tab indent/dedent, backspace-at-list-start), blockquotes (enter/backspace/delete unwrap rules), caret affinity (upstream/downstream at wrapped-line boundaries), context menu (copy/cut/paste/paste-unformatted, slot override), undo/redo, and an accessible `Role::Paragraph` → text-run tree carrying character positions and byte-vs-char multibyte lengths with signal-driven rebuild on edit. `RichTextEditor::editor` / `read_only` accept `.min_lines(n)` / `.max_lines(n)` for intrinsic-mode sizing (the messenger-composer pattern). **Colour overrides** — `.background(impl Into<ColorProp>)` / `.text_color(..)` / `.caret_color(..)` / `.selection_color(..)` all take `Color | theme role | Signal` (like the rest of the framework): `background` is threaded into the active `RichTextEditorStyle::make_body` so the common "give the editor a surface" case needs no custom style; `text`/`caret`/`selection` are resolved against the active theme on every paint (a role / `Signal` stays reactive, a bare `Color` pins it) — `None` tracks the theme's `editor_fg`/`editor_caret` roles. **Caveats:** the 112 tests run against `MockTextBackend` (fixed metrics) — they verify editing logic and caret math, NOT real shaping/bidi/complex-script layout, which is delegated to `text-typeset`'s own test suite (external sibling crate); embedded-image support is minimal (45-line `image_cache.rs`, lightly tested); and `TextInput` / `TextInputField` / `PasswordField` all derive from this stack, so it is load-bearing for every text field in the framework. Demos: `cargo run -p rich_text_editor`, `cargo run -p rich_text_viewer`.

### Partial / In Progress

- Text rendering quality is bounded by the external `text-typeset` crate (shaping, bidi, line-breaking, glyph atlas) — a sibling path-dep maintained alongside Teksilo, not a workspace member; the GPU glyph path is exercised by demos, not headless tests

### Not Started

## Key Files

- Workspace config: `Cargo.toml`
- Widget trait: `crates/teksilo-core/src/widget.rs`
- Signal/Prop system: `crates/teksilo-core/src/signal.rs`
- BuildContext: `crates/teksilo-core/src/build_context.rs`
- Event handlers: `crates/teksilo-core/src/event_handlers.rs`
- WidgetBuilder trait: `crates/teksilo-core/src/widget_builder.rs`
- Arena: `crates/teksilo-core/src/arena.rs`
- Widget tree orchestrator: `crates/teksilo-core/src/widget_tree.rs`
- State system: `crates/teksilo-core/src/state.rs`
- Event types: `crates/teksilo-core/src/event.rs`
- Theme + styling system: [crates/teksilo-core/src/styles/](crates/teksilo-core/src/styles/) (`theme.rs`, `theme_appearance.rs`, `theme_extension.rs`, `recipe.rs`, `component_style_slots.rs`, one `*_style.rs` trait file per themable widget). IntUI preset: [crates/teksilo-core/src/presets/intui.rs](crates/teksilo-core/src/presets/intui.rs). Default `Recipe*Style` impls: [crates/teksilo-widgets/src/styles/](crates/teksilo-widgets/src/styles/). Reference: [docs/styling-system.md](docs/styling-system.md)
- Color tokens: `crates/teksilo-tokens/src/color.rs`
- Motion subsystems: [crates/teksilo-core/src/animation.rs](crates/teksilo-core/src/animation.rs) (signal-tween `AnimationScheduler`), [crates/teksilo-core/src/animated_quad.rs](crates/teksilo-core/src/animated_quad.rs) (shader-quad `AnimatedQuadRegistry`), [crates/teksilo-core/src/frame_tick_scheduler.rs](crates/teksilo-core/src/frame_tick_scheduler.rs) (per-frame-effect `FrameTickScheduler` — `Pulse` / `Cycle`), [crates/teksilo-core/src/motion_visibility.rs](crates/teksilo-core/src/motion_visibility.rs) (shared `alive` / `painted_this_frame` / `painted_recently` helpers). Reference: [docs/idle-and-animation.md](docs/idle-and-animation.md), [docs/animation.md](docs/animation.md).
- Button (reference widget): [crates/teksilo-widgets/src/button.rs](crates/teksilo-widgets/src/button.rs)
- Switcher: [crates/teksilo-widgets/src/primitives/switcher.rs](crates/teksilo-widgets/src/primitives/switcher.rs)
- Layout primitives: [crates/teksilo-widgets/src/primitives/](crates/teksilo-widgets/src/primitives/)
- ColumnFlow (responsive reflowing columns): [crates/teksilo-widgets/src/primitives/column_flow.rs](crates/teksilo-widgets/src/primitives/column_flow.rs) (widget + `balance_columns` + the private `ColumnFlowItem` a11y wrapper + inline tests). Shared column-count solver: [crates/teksilo-widgets/src/common/column_geometry.rs](crates/teksilo-widgets/src/common/column_geometry.rs) (`ColumnGeometry` / `WidthPolicy`), reached from GridView via the `GridSizing → WidthPolicy` door at [crates/teksilo-widgets/src/grid_view/layout/columns.rs](crates/teksilo-widgets/src/grid_view/layout/columns.rs). Reference: [docs/layout-primitives.md §5.4](docs/layout-primitives.md)
- Data models: [crates/teksilo-data/src/](crates/teksilo-data/src/) (`list_model.rs`, `list_data_source.rs`, `tree_model.rs`, `tree_slice.rs` — per-view expand state, `tree_data_source.rs` + `tree_data_slice.rs` — external indent-ordered tree source + its ready-made `TreeDataSlice` engine, `tree_row_filter.rs` — row-stream sort/filter below the slice, `selection_model.rs`, `sort_filter_list_model.rs`, `sort_filter_tree_model.rs`, `checked_model.rs`, `tree_checked_model.rs`, `keyed_tree_checked_model.rs` — domain-keyed tree checkboxes, `check_state.rs`, `data_change.rs`, `tree_change.rs`, `debug_registry.rs`). Reference: [docs/data-models.md](docs/data-models.md). Design rules: per-view independent expand state via `TreeSlice<T>`; concrete `T` (no `QVariant`); `Rc<RefCell<…>>` share-by-clone; mutation-then-notify discipline; the crate is GUI-free so a ViewModel layer or headless consumer can use it without pulling in `teksilo-widgets`
- Standard row items: [crates/teksilo-widgets/src/standard_item.rs](crates/teksilo-widgets/src/standard_item.rs) (`StandardListItem`, `StandardTreeItem`); chevron primitive: [crates/teksilo-widgets/src/primitives/twist_arrow.rs](crates/teksilo-widgets/src/primitives/twist_arrow.rs); style trait: `StandardItemStyle` in [crates/teksilo-core/src/styles/standard_item_style.rs](crates/teksilo-core/src/styles/standard_item_style.rs), default impl + dim constants in [crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs](crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs)
- TableView: [crates/teksilo-widgets/src/table_view.rs](crates/teksilo-widgets/src/table_view.rs) + submodules at [crates/teksilo-widgets/src/table_view/](crates/teksilo-widgets/src/table_view/) (`column.rs`, `selection.rs`, `a11y.rs`, `body.rs`, `header.rs`, `keyboard.rs`, `layout.rs`, `row_navigator.rs`, `tests.rs`). Demo: [examples/data_grid/src/main.rs](examples/data_grid/src/main.rs)
- TreeTableView: [crates/teksilo-widgets/src/tree_table_view.rs](crates/teksilo-widgets/src/tree_table_view.rs) (reuses table_view's column/header/keyboard modules; adds `TreeNavigator` + `TwistArrow`). Demo: [examples/tree_table_view/src/main.rs](examples/tree_table_view/src/main.rs)
- Toast notifications: [crates/teksilo-widgets/src/toast.rs](crates/teksilo-widgets/src/toast.rs) (request + builder + `ToastAction` + `ToastDismissCause` + `ToastHandle` + `ToastInstallOptions`) + submodules at [crates/teksilo-widgets/src/toast/](crates/teksilo-widgets/src/toast/) (`registry.rs` — queue + archive bridge + in-place merge; `ext.rs` — `EventContextToastExt::show_toast`; `surface.rs` — chrome + a11y; `host.rs` — per-window queue + frame-tick timer + hover-pause). Style: [crates/teksilo-widgets/src/styles/recipe_toast_style.rs](crates/teksilo-widgets/src/styles/recipe_toast_style.rs) + trait at [crates/teksilo-core/src/styles/toast_style.rs](crates/teksilo-core/src/styles/toast_style.rs). Persistent archive + log family: [crates/teksilo-widgets/src/notification.rs](crates/teksilo-widgets/src/notification.rs) + [crates/teksilo-widgets/src/notification/](crates/teksilo-widgets/src/notification/) (`archive.rs` — `NotificationArchiveModel` with `InMemory` / `Persistent` backends + bounded eviction + unread-count signal + version signal; `log.rs` — toolbar + day-bucket sections + replayable action rows; `center_button.rs` — bell + badge + popover; `log_dialog.rs` — one-liner modal preset). Install hook: [crates/teksilo/src/toast_install.rs](crates/teksilo/src/toast_install.rs) (`TeksiloAppBuilderToastExt::install_toast` / `install_toast_default`). Reference: [docs/toast.md](docs/toast.md). Demo: [examples/toast_demo/src/main.rs](examples/toast_demo/src/main.rs).
- Native (OS) menu bar: declarative model in [crates/teksilo-widgets/src/menu.rs](crates/teksilo-widgets/src/menu.rs) + [crates/teksilo-widgets/src/menu/](crates/teksilo-widgets/src/menu/) (`model.rs` — `MenuModel`/`MenuEntry`/`MenuItems`/`MenuNode`/`MenuItemState` + `build_menu_list` for the in-window dropdowns; `native.rs` — `NativeMenuMode` + `install` bridge: model→`NativeMenuSnapshot` resolution, `KeyStroke`→key-equivalent mapping, reactive `update_item` observers). `MenuBar::from_model`/`native_on_macos` in [crates/teksilo-widgets/src/menu_bar.rs](crates/teksilo-widgets/src/menu_bar.rs). Id token: [crates/teksilo-core/src/menu_item_id.rs](crates/teksilo-core/src/menu_item_id.rs) (`MenuItemId`). Platform surface: [crates/teksilo-platform/src/native_menu.rs](crates/teksilo-platform/src/native_menu.rs) (`NativeMenuBackend` trait, `NativeMenuHandle`, `NativeMenuSnapshot`/`NativeMenuNode`/`MenuItemDelta`/`NativeCheck`/`NativeKeyEquivalent`/`StandardMenuRole`, `NativeMenuEventPayload`, `NoopNativeMenuBackend`, `MemoryNativeMenuBackend`) + macOS `NSMenu` impl at [crates/teksilo-platform/src/native_menu/macos.rs](crates/teksilo-platform/src/native_menu/macos.rs) (`MacOsNativeMenuBackend` + `TeksiloMenuTarget`). App wiring in [crates/teksilo-app/src/app.rs](crates/teksilo-app/src/app.rs) (`install_native_menu`, `try_route_native_menu_payload`, `WindowEvent::Focused` → `activate_window`) + close cleanup in [crates/teksilo-app/src/window_manager.rs](crates/teksilo-app/src/window_manager.rs). Behind teksilo-platform's `native-menu` feature (always on for widgets + app). Reference: [docs/native-menu.md](docs/native-menu.md). Demo: [examples/native_menu/src/main.rs](examples/native_menu/src/main.rs).
- Scene viewport: [crates/teksilo-scene/src/](crates/teksilo-scene/src/) — `scene.rs` (65 KB core scene model + `WidgetSource` Once/Delegated + `ItemChange::PayloadChanged`), `scene_model.rs` (`SceneModel` = cloneable `Rc<RefCell<Scene>>` handle; `&self` mutators, `add_widget_item`/`set_payload`/`payload`, multi-handle tests), `view.rs` (137 KB `SceneView` widget with pan/zoom/gestures, `with_model`/`delegate_typed`/`selection_model`/`model`, per-view delegate materialisation in `build()`), `view/tests/multi_view.rs` (two-views-one-model coverage), `item.rs` + `items/` (`SceneItem` trait and built-in items), `a11y.rs` (AT walker), `minimap.rs`, `cache.rs`, `transform.rs`, `selection.rs`, `index.rs` (spatial index), `flags.rs`, `item_handlers.rs`, `animation.rs`, `state.rs`, `magnet.rs` (magnetism: `Magnet`/`MagnetId`/`MagnetRole`/`MagnetRef`/`MagnetVerdict`/`MagnetConnection`/`MagnetSnap`/`MagnetismConfig`/`MagnetFeedback` + `Scene::compute_item_snap`/`compute_port_snap`/`nearest_magnet` snap helpers), `view/magnetism.rs` (per-view runtime state, built-in feedback renderer, keyboard connect handler); drag/port-drag wiring in `view/gestures_impl.rs`, feedback paint in `view/paint_impl.rs`, synthetic magnet AT nodes + `active_descendant` in `view/a11y_impl.rs`; magnet-or-selection gate for the drag handler in `view/build_impl.rs`. Core touchpoint: `SyntheticKind::SceneMagnet` in [crates/teksilo-core/src/accessibility.rs](crates/teksilo-core/src/accessibility.rs). References: [docs/teksilo-scene.md](docs/teksilo-scene.md), [docs/teksilo-scene-a11y.md](docs/teksilo-scene-a11y.md). Demos: [examples/scene_showcase/src/main.rs](examples/scene_showcase/src/main.rs), [examples/scene_corkboard/src/main.rs](examples/scene_corkboard/src/main.rs), [examples/scene_magnetism/src/main.rs](examples/scene_magnetism/src/main.rs)
- i18n runtime: [crates/teksilo-i18n/src/manager.rs](crates/teksilo-i18n/src/manager.rs), [crates/teksilo-i18n/src/localized_string.rs](crates/teksilo-i18n/src/localized_string.rs)
- i18n locale-aware formatting: [crates/teksilo-i18n/src/format.rs](crates/teksilo-i18n/src/format.rs) (Memoizable types, ICU bridge, `TeksiloDateTime` + `FluentType` impl, public `NumberFormatter` / `TeksiloDateTimeFormatter`, bundle `set_formatter` callback, `DATETIME()` Fluent function). Bundle wiring: `configure_bundle` helper in [manager.rs](crates/teksilo-i18n/src/manager.rs). Tests: [crates/teksilo-i18n/tests/format_integration.rs](crates/teksilo-i18n/tests/format_integration.rs)
- i18n macros: [crates/teksilo-i18n-macros/src/lib.rs](crates/teksilo-i18n-macros/src/lib.rs) (`tr!`, `tr_widget!`, `tr_signal!`, `tr_signal_widget!`)
- teksu! DSL macro: [crates/teksilo-macros/src/](crates/teksilo-macros/src/) (parse → IR → lower). Trybuild fixtures at [crates/teksilo/tests/teksilo/pass/](crates/teksilo/tests/teksilo/pass/)
- teksu! reference: [docs/teksu-macro-reference.md](docs/teksu-macro-reference.md) (user-facing), [docs/teksu-language-spec-v3.md](docs/teksu-language-spec-v3.md) (design spec)
- Actions/Intents/Shortcuts: [crates/teksilo-core/src/action.rs](crates/teksilo-core/src/action.rs), [intent.rs](crates/teksilo-core/src/intent.rs), [shortcut.rs](crates/teksilo-core/src/shortcut.rs). `IntentKind` derive: [crates/teksilo-macros/src/intent_kind.rs](crates/teksilo-macros/src/intent_kind.rs). Settings widget: [crates/teksilo-widgets/src/shortcut_settings.rs](crates/teksilo-widgets/src/shortcut_settings.rs). Reference doc: [docs/shortcut-intent-action.md](docs/shortcut-intent-action.md)
- Settings/persistence: [crates/teksilo-settings/src/](crates/teksilo-settings/src/) (`store.rs`, `file.rs`, `mru.rs`, `window_state.rs`, `bundle.rs`, `ext.rs`, `migration.rs`, `flush.rs`, `path.rs`). Auto window save/restore wiring: [crates/teksilo-app/src/window_persist.rs](crates/teksilo-app/src/window_persist.rs). Reference doc: [docs/settings.md](docs/settings.md). Demo: [examples/recent_projects/src/main.rs](examples/recent_projects/src/main.rs)
- Canvas API: `crates/teksilo-canvas/src/canvas.rs`
- Renderer: `crates/teksilo-render/src/renderer.rs`
- App builder: `crates/teksilo-app/src/app.rs`
- Umbrella exports: `crates/teksilo/src/lib.rs`
- Resources: `crates/teksilo-resources/src/lib.rs`
- Previewer: [crates/teksilo-preview/src/](crates/teksilo-preview/src/) (`catalog.rs` — `WidgetCatalog` / `CatalogEntry`, `knob.rs` — `KnobSpec`/`KnobValue`/`KnobOverrides`/`KnobValues`, `variant.rs` — `PreviewVariant`, `registry.rs` — `inventory::iter` wrappers, `source_loc.rs` — `SourceLoc`). GUI library: [crates/teksilo-preview-ui/src/](crates/teksilo-preview-ui/src/) (`app_state.rs`, `canvas.rs`, `navigator.rs`, `knob_form.rs`, `inspector.rs`, `toolbar.rs`, `cli.rs`, `png_export.rs`). Stock binary: [crates/teksilo-widgets-previewer/src/main.rs](crates/teksilo-widgets-previewer/src/main.rs). Run: `cargo run -p teksilo-widgets-previewer`
- Drag-and-drop: `crates/teksilo-core/src/drag_payload.rs` (`DragPayload`, `DragOrigin`, `ExternalDropData`), `crates/teksilo-core/src/drag_state.rs`, external-drag tree methods in [crates/teksilo-core/src/widget_tree/drag_drop_impl.rs](crates/teksilo-core/src/widget_tree/drag_drop_impl.rs)
- External (OS) DnD: [crates/teksilo-platform/src/external_dnd.rs](crates/teksilo-platform/src/external_dnd.rs) (trait, handle, payload, Noop/Memory backends) + [external_dnd/macos.rs](crates/teksilo-platform/src/external_dnd/macos.rs); app wiring (`install_external_dnd`, `try_route_external_dnd_payload`) in [crates/teksilo-app/src/app.rs](crates/teksilo-app/src/app.rs), attach/detach in [crates/teksilo-app/src/window_manager.rs](crates/teksilo-app/src/window_manager.rs). Widgets: [crates/teksilo-widgets/src/drop_zone.rs](crates/teksilo-widgets/src/drop_zone.rs) (standalone) + `DropZoneStyle` ([crates/teksilo-core/src/styles/drop_zone_style.rs](crates/teksilo-core/src/styles/drop_zone_style.rs), [recipe](crates/teksilo-widgets/src/styles/recipe_drop_zone_style.rs)); [crates/teksilo-widgets/src/drop_target.rs](crates/teksilo-widgets/src/drop_target.rs) (wrapping container, internal + external) + `DropTargetStyle` ([crates/teksilo-core/src/styles/drop_target_style.rs](crates/teksilo-core/src/styles/drop_target_style.rs), [recipe](crates/teksilo-widgets/src/styles/recipe_drop_target_style.rs)). Demo: [examples/file_drop/src/main.rs](examples/file_drop/src/main.rs).
- Clipboard: `crates/teksilo-platform/src/clipboard.rs`
- File dialogs: [crates/teksilo-platform/src/file_dialog.rs](crates/teksilo-platform/src/file_dialog.rs) (trait, handle, request, result, payload, mock, `RfdAsyncBackend`, `EventContextFileDialogExt`). Wiring: `WindowOps::current_parent_handle` in [crates/teksilo-core/src/window/ops.rs](crates/teksilo-core/src/window/ops.rs); `EventContext::parent_window_handle` + `EventContext::poster` in [crates/teksilo-core/src/widget.rs](crates/teksilo-core/src/widget.rs); `WidgetTree::run_with_event_context` in [crates/teksilo-core/src/widget_tree.rs](crates/teksilo-core/src/widget_tree.rs); `TeksiloAppHandler::try_route_file_dialog_payload` and the `AppEvent::External` downcast arm in [crates/teksilo-app/src/app.rs](crates/teksilo-app/src/app.rs); window-close purge hook in [crates/teksilo-app/src/window_manager.rs](crates/teksilo-app/src/window_manager.rs)'s `close_window`. Demo: [examples/file_dialogs/src/main.rs](examples/file_dialogs/src/main.rs).
- WebView: [crates/teksilo-webview/src/](crates/teksilo-webview/src/) — `backend.rs` (`WebViewBackend`/`WebViewHandle` traits, `WebViewId`/`WebViewAttributes`/`WebViewEvent`/`WebViewEventPayload`, `WebViewRegistry` app-state service, `MemoryWebViewBackend`/`NoopWebViewBackend`, shared `post_event` + `js_string`), `lib.rs` (`WebView` widget + `is_wayland()`), `styles/recipe_web_view_style.rs` (default overlay), `wry_backend.rs` (feature `wry-backend`), `servo_backend.rs` (feature `servo-backend`), `tests/basic_lifecycle.rs`. Style trait: [crates/teksilo-core/src/styles/web_view_style.rs](crates/teksilo-core/src/styles/web_view_style.rs). Framework hooks (teksilo-core): `WidgetNode::activation_signal` + `BuildContext::activation_signal` + `WidgetTree::flush_activation_signals` (fired from `set_dormant`/`activate`); `BuildContext::run_after_mount` + `WidgetTree::run_mount_actions` (post-mount EventContext). App wiring: `try_route_web_view_payload` + `process_pending_mount_actions` + `run_in_window` helper in [crates/teksilo-app/src/app.rs](crates/teksilo-app/src/app.rs); per-window purge in `window_manager.rs`'s `close_window`. Install ext: [crates/teksilo/src/webview_install.rs](crates/teksilo/src/webview_install.rs) (`TeksiloAppBuilderWebViewExt`). Reference: [docs/web-view.md](docs/web-view.md). Demo: [examples/web_view_demo/src/main.rs](examples/web_view_demo/src/main.rs).
- Text input: `crates/teksilo-widgets/src/text_input.rs`, `crates/teksilo-widgets/src/primitives/text_input_field.rs`
- Rich text: `crates/teksilo-widgets/src/rich_text/` (state, paint, clipboard, keyboard, mouse, hit_test, context_menu, frame_loop, policy, image_cache)
- New widgets: `crates/teksilo-widgets/src/spin_box.rs`, `crates/teksilo-widgets/src/split_button.rs`, `crates/teksilo-widgets/src/group_box.rs`, `crates/teksilo-widgets/src/group_header.rs`, `crates/teksilo-widgets/src/message_box.rs`, `crates/teksilo-widgets/src/tool_box.rs`, `crates/teksilo-widgets/src/keystroke_format.rs`, `crates/teksilo-widgets/src/privacy_settings.rs`
- New primitives: `crates/teksilo-widgets/src/primitives/masonry.rs`, `crates/teksilo-widgets/src/primitives/form_layout.rs`, `crates/teksilo-widgets/src/primitives/image_widget.rs`
- OS integration: `crates/teksilo-platform/src/os_theme.rs`, `crates/teksilo-platform/src/accessibility_prefs.rs`
- Title bar hosts: `crates/teksilo-platform/src/title_bar_host/` (wayland.rs, x11.rs, windows.rs, macos.rs). Backend selection is by the window's **live `RawDisplayHandle`**, never env vars (`WAYLAND_DISPLAY` and `DISPLAY` are both set in any modern session) — `window_system::window_system_for_display_handle` is the one shared discriminator, also used by the DnD backend.
- X11 support: `crates/teksilo-platform/src/x11.rs` + `x11/` (`connection.rs` own `RustConnection` + atom cache + property/timestamp helpers, `ewmh.rs` the once-per-process WM capability probe behind custom chrome, `xdnd.rs` the **pure** XDND codec / version negotiation / `XdndProxy` validation / `INCR` assembly — connection-free so it is exhaustively unit-testable with no display). DnD backend: `external_dnd/x11.rs` + `x11/tests.rs` (`#[ignore]`d protocol tests against a real X server; CI runs them under `xvfb-run` + `openbox`). Reference: [docs/drag-and-drop.md §11.3.1](docs/drag-and-drop.md), [docs/title-bar.md](docs/title-bar.md)

## Widget Construction Patterns

```rust
// Inline children (most common) — .child() accepts impl Widget + 'static
VStack::new().spacing(10.0)
    .child(TextWidget::new(lit!("Hello")).style(TextStyleRole::BodyBold))
    .child(Button::new(lit!("Click")).on_activate_fn(|ctx| ctx.send_intent(MyIntent::DoThing)))

// Pre-registered children (when you need the ID) — .add_child() takes WidgetId
let label_id = ctx.add(TextWidget::new(lit!("Status")).text(status_signal));
HStack::new().add_child(label_id)

// Iterator children
VStack::new().children(items.iter().map(|item| TextWidget::new(lit!(item.name.clone()))))

// Conditional children
container.child_opt(show_extra.then(|| TextWidget::new(lit!("Extra"))))

// Switcher — shows one child at a time, driven by Signal<usize>
let selected = ctx.signal(0_usize);
ctx.add(Switcher::new(selected.clone())
    .child(TextWidget::new(lit!("Page 0")))
    .child(TextWidget::new(lit!("Page 1")))
    .child(TextWidget::new(lit!("Page 2"))))

// Composing widget — build() creates child subtree, &mut self
#[derive(Debug)]
struct MyWidget {
    root_child_id: Option<WidgetId>,
}

impl Widget for MyWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // No theme snapshot needed — roles resolve at paint/layout time.
        let root = ctx.add(VStack::new()
            .child(TextWidget::new(lit!("Hello")).style(TextStyleRole::BodyBold))
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }
}

// Attached event handlers — via WidgetBuilder on child widgets
ctx.add(
    MinSize::new(48.0, 48.0).child(content)
        .on_tap(|_event, ctx| { ctx.send_intent(MyIntent::Clicked); })
        .on_hover(move |entered, _ctx| { interaction.set(if entered { Hovered } else { Idle }); })
        .focusable(true)
        .cursor(CursorIcon::Pointer)
)

// Attached event handlers — via HandlerSet on self in build()
fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    let handlers = HandlerSet::new()
        .on_tap(move |_event, ctx| { /* ... */ })
        .on_hover(move |entered, _ctx| { /* ... */ })
        .focusable(true)
        .cursor(CursorIcon::Pointer);
    ctx.apply_self_handlers(handlers);
    // ...
}
```

## `teksu!` DSL

Block-structured DSL for widget trees. Desugars one-to-one to builder
calls at macro-expansion time — no runtime, no virtual tree.

```rust
use teksilo::prelude::*;

fn build(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx =>
        VStack {
            spacing: 12.0
            TextWidget::new(lit!("Title")) { style: t.body_bold.clone() }
            open_btn = Button(lit!("Open")) {
                on_activate: Cmd::Open
            }
            TextWidget(lit!("Status")) { linked_to: open_btn }
        }
    )
}
```

Body items are separated by newlines, not commas. `name: value` →
`.name(value)`. `name = Element` hoists `let name = ctx.add(...)` and
attaches by id. Bare UpperCamel children at body position → `.child(...)`.
Structural forms (`if`/`match`/`for`/`let`/`rust { }`/`..spread`) and
the `#{ expr }` escape work as documented. Category B widgets (Card,
Dialog, TabWidget, etc.) address content by named slots; a bare child
there emits a targeted hint pointing at the right slot name.

See [docs/teksu-macro-reference.md](docs/teksu-macro-reference.md) for
the full surface language, desugaring cheat sheet, and limitations;
[docs/teksu-language-spec-v3.md](docs/teksu-language-spec-v3.md) for the
design spec with worked translations of the widget-catalog examples.
Slash command `/teksilo-macro` loads the skill for read/write/explain/
translate/debug workflows.

## App Entry Point Pattern

```rust
fn main() {
    TeksiloAppBuilder::new()
        .theme(intui::light())
        .initial_window(
            WindowConfig::new()
                .title("My App")
                .size(800, 600)
                .root(|tree, _state| tree.add(MyRootWidget::new())),
        )
        .run();
}
```

Every window — initial or runtime-opened — is described by a `WindowConfig`. There is no `.window_title` / `.window_size` / `.root` on `TeksiloAppBuilder` directly; secondary windows are opened from handler code via `ctx.open_window(WindowConfig::new()...)`. See [docs/multi-window.md](docs/multi-window.md) for the full multi-window API.

App-wide behavior lives inside the root widget: register `Shortcut`s, declare `Action`s keyed by intent name, and react to them via handlers. See "Actions, Intents & Shortcuts" above and [docs/shortcut-intent-action.md](docs/shortcut-intent-action.md) for the full pattern. Ambient mutations are available on `EventContext` from any handler: `ctx.set_theme(...)`, `ctx.set_locale(...)`, `ctx.close_window()`, `ctx.with_widget_mut::<W>(id, BindingLevel, |w| ...)` (typed, deferred by-id mutation of any mounted widget that overrides `Widget::as_any_mut` — the supported way to reach e.g. `SceneView::scene_mut()` post-mount; applied after the handler returns, then dirty-marked at the given level), and `ctx.request_accessibility_update()` (also on `BuildContext`; forces an AccessKit re-walk after a subtree restructure a relayout wouldn't otherwise surface to AT).

If the app uses persistence, chain `.app_paths(...)` (or `.application(qualifier, organization, application)`) and `.settings(SettingsBundle::new()...)` before `.initial_window(...)`. App-typed handles (`MruList<T>`, `SettingsFile<T>`) register via `.app_state(handle.clone())`. See "Settings & Persistence" above and [docs/settings.md](docs/settings.md).

## Architecture Reference

Framework-internals reference: [docs/architecture.md](docs/architecture.md) — scrolling, arena state, Canvas API, rendering pipeline, HiDPI, threading, testability, crate dependency graph, architectural comparisons, open questions. Subsystems with a dedicated reference doc (events, layout, animation, theming, i18n, shortcuts, accessibility, settings, multi-window, drag-and-drop, data models, …) are stubbed with one-paragraph pointers; section numbers are preserved so external `§N` refs still resolve. Doc index: [docs/SUMMARY.md](docs/SUMMARY.md).

Additional documentation: [docs/widgets-overview.md](docs/widgets-overview.md), [docs/accessibility-overrides.md](docs/accessibility-overrides.md), [docs/settings.md](docs/settings.md), [docs/drag-and-drop.md](docs/drag-and-drop.md), [docs/title-bar.md](docs/title-bar.md), [docs/multi-window.md](docs/multi-window.md), [docs/idle-and-animation.md](docs/idle-and-animation.md), [docs/telemetry.md](docs/telemetry.md), [docs/table-view.md](docs/table-view.md), [docs/inspector.md](docs/inspector.md), [docs/web-view.md](docs/web-view.md), [docs/automation-mcp.md](docs/automation-mcp.md), [docs/property-testing.md](docs/property-testing.md), [docs/segmented-control.md](docs/segmented-control.md)
