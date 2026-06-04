# Bastyde Documentation Index

Reference and design documents for Bastyde. All paths are relative to this
directory.

## Architecture & roadmap

- [architecture.md](architecture.md) — framework-internals
  reference: scrolling, arena, Canvas API, rendering pipeline, HiDPI,
  threading, testability, crate dependency graph, design comparisons,
  open questions. Per-subsystem APIs live in the focused docs below.
- [bastyde-milestones.md](bastyde-milestones.md) — the demo-driven milestone
  roadmap; each milestone produces a runnable example exercising one slice
  of the architecture.

## Widget catalog

- [widgets-overview.md](widgets-overview.md) — every shipped widget
  categorized (layout / visual / containers / buttons / inputs / text
  family / menus / overlays / data-driven / charts / animations /
  settings) with a one-line description and source-file link. Pair with
  `python3 tools/extract_widget_api.py <Widget…>` for the full API
  surface of any widget.

## Authoring widgets

- [layout-primitives.md](layout-primitives.md) — `HStack` / `VStack` /
  `ZStack`, `Grid`, `Wrap`, `MasonryLayout`, `FormLayout`, `Switcher`,
  and the size wrappers (`Expand`, `FixedSize`, `MinSize`, `MaxSize`,
  `AspectRatio`, `Center`, `Padding`, `Spacer`, `Divider`).
- [events-and-gestures.md](events-and-gestures.md) — preview/bubble dispatch,
  attached handlers (`.on_tap`, `.on_hover`, …), `on_key_preview`,
  `focus_within` / `hover_within`, gesture recognizers.
- [styling-system.md](styling-system.md) — the four-tier styling ladder
  (tokens → variants → recipes → style protocols); `Theme` aggregator,
  `ThemeAppearance`, per-widget `*Variant` enums and `*Style` traits,
  per-call vs theme-wide style installation, writing a custom preset.
- [reactive-theme.md](reactive-theme.md) — `Signal<Theme>`, role-driven
  colors (`ColorProp`, `TextStyleProp`), reactive switching without rebuild.
- [animation.md](animation.md) — `Signal<f32>::animate_to`, `MotionTokens`,
  `AnimationSpec` builder, the animated wrapper widgets (Fade, Pulse,
  Crossfade, Scale, Blur, …).
- [idle-and-animation.md](idle-and-animation.md) — the zero-frame rule;
  how `next_timer_deadline()` keeps the event loop asleep when nothing
  is moving.
- [accessibility-overrides.md](accessibility-overrides.md) — builder-level
  `.access_*` modifiers (label, description, subtree merge/exclude, custom
  actions, shortcut binding) for the cases widget-emitted a11y misses.

## `bati!` DSL & formatting

- [bati-macro-reference.md](bati-macro-reference.md) — user-facing reference
  for the `bati!` block-DSL (parse → IR → builder calls).
- [bati-language-spec-v3.md](bati-language-spec-v3.md) — design spec with
  full grammar, structural forms, and worked translations of catalog
  examples.
- [bastyde-fmt.md](bastyde-fmt.md) — `cargo bastyde-fmt`, the formatter for `bati!`
  blocks (`rustfmt` skips macro bodies).
- [bastyde-fmt-vscode.md](bastyde-fmt-vscode.md) — wiring `bastyde-fmt-lsp` into
  VS Code for in-editor formatting.

## Input, navigation, chrome

- [shortcut-intent-action.md](shortcut-intent-action.md) — `Shortcut` /
  `Intent` / `Action` pipeline, `#[derive(IntentKind)]`, rebindable
  keystrokes via `ShortcutRegistry`.
- [tooltips.md](tooltips.md) — plain `TooltipWidget`, registry-driven
  `RichTooltipWidget`, sticky-on-dwell promotion, focus-driven a11y
  promotion, attach helpers.
- [toast.md](toast.md) — `Toast` floating notifications (`info` /
  `success` / `warning` / `error` / `loading` severities, link +
  button actions, `Toast::id` update-in-place) + `ToastHost` queue +
  persistent `NotificationArchiveModel` + `NotificationLog` / bell
  `NotificationCenterButton` / `NotificationLogDialog` UI;
  `BastydeAppBuilder::install_toast_default()` one-line install.
- [drag-and-drop.md](drag-and-drop.md) — drag payloads, drop targets,
  hit testing, the three user stories that share the underlying machinery.
- [multi-window.md](multi-window.md) — `WindowConfig`, signal-driven
  multi-window orchestration, modal dialogs, restore-from-state.
- [title-bar.md](title-bar.md) — custom widget-level title bar plus the
  per-OS `PlatformTitleBarHost` for drag / zoom / close / inset.
- [toolbar.md](toolbar.md) — `Toolbar` command bar with automatic overflow:
  actions (priority / `always_overflow` / toggle), pinned + collapsible
  custom widgets (`overflow_as` menu row, `overflow_widget` live embedded
  control, the `ToolbarOverflow` trait), the `MenuList`-backed chevron menu,
  display modes / orientation, and the ARIA roving-tabindex pattern.

## Data, persistence, telemetry

- [data-models.md](data-models.md) — `ListModel`, `TreeModel`,
  `SelectionModel`, `CheckedModel` / `TreeCheckedModel` (per-row
  checkbox state with optional descendant→ancestor tristate
  aggregation), sort/filter projections; the model layer that sits
  above the widget tree.
- [settings.md](settings.md) — reactive end-to-end persistence:
  `SettingsStore`, `SettingsFile<T>`, `MruList<T>`, window-state auto
  save/restore.
- [telemetry.md](telemetry.md) — consent-gated event reporting, the
  bastyde-collector / Plausible / OTLP adapters, the `events.yaml` schema
  pipeline.

## Async & concurrency

- [async.md](async.md) — the optional main-thread async executor
  (`bastyde-async`): `install_async()`, `ctx.spawn_local(...)` /
  `spawn_local_with`, `spawn_blocking`, the async-agnostic `on_loop_tick`
  hook, and the `bastyde-tokio` / `bastyde-async-std` reactor adapters for
  awaiting native runtime futures. Off by default; complements the reactive
  `subscribe_event` data path.

## Specialized widgets

- [table-view.md](table-view.md) — virtualized `TableView` and `TreeTable`
  (multi-column, sort/filter, drag-resize, drag-reorder, full keyboard
  navigation).
- [tab-widget.md](tab-widget.md) — `TabBar<T>` and `TabWidget` (static +
  dynamic tabs, `Signal<Option<TabId>>` selection, pinned tabs, drag
  reorder, overflow dropdown, horizontal + vertical orientations).
- [charts.md](charts.md) — `BarChart` / `LineChart` / `PieChart`
  (shared axis / palette / legend / tooltip infrastructure).
- [bastyde-scene.md](bastyde-scene.md) — the pannable, zoomable scene viewport
  (canvases, board layouts, diagram editors).
- [bastyde-scene-a11y.md](bastyde-scene-a11y.md) — shaping the accessibility
  tree of a `bastyde-scene` viewport.

## Visuals & resources

- [icons-and-resources.md](icons-and-resources.md) — `res!()`-embedded
  SVG / PNG / WebP icons with theme-aware tinting.

## Tooling

- [inspector.md](inspector.md) — `bastyde-inspector`, the in-app debug
  surface (Tree / Properties / Accessibility / Theme / Models tabs;
  picker + bounds overlay; debug-only).
