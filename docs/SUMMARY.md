# FernUI Documentation Index

Reference and design documents for FernUI. All paths are relative to this
directory; in-progress design work lives separately under [plans/](plans/).

## Architecture & roadmap

- [fern-ui-architecture.md](fern-ui-architecture.md) — framework-internals
  reference: scrolling, arena, Canvas API, rendering pipeline, HiDPI,
  threading, testability, crate dependency graph, design comparisons,
  open questions. Per-subsystem APIs live in the focused docs below.
- [fern-ui-milestones.md](fern-ui-milestones.md) — the demo-driven milestone
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

## `fern!` DSL & formatting

- [fern-macro-reference.md](fern-macro-reference.md) — user-facing reference
  for the `fern!` block-DSL (parse → IR → builder calls).
- [fern-language-spec-v3.md](fern-language-spec-v3.md) — design spec with
  full grammar, structural forms, and worked translations of catalog
  examples.
- [fern-fmt.md](fern-fmt.md) — `cargo fern-fmt`, the formatter for `fern!`
  blocks (`rustfmt` skips macro bodies).
- [fern-fmt-vscode.md](fern-fmt-vscode.md) — wiring `fern-fmt-lsp` into
  VS Code for in-editor formatting.

## Input, navigation, chrome

- [shortcut-intent-action.md](shortcut-intent-action.md) — `Shortcut` /
  `Intent` / `Action` pipeline, `#[derive(IntentKind)]`, rebindable
  keystrokes via `ShortcutRegistry`.
- [tooltips.md](tooltips.md) — plain `TooltipWidget`, registry-driven
  `RichTooltipWidget`, sticky-on-dwell promotion, focus-driven a11y
  promotion, attach helpers.
- [drag-and-drop.md](drag-and-drop.md) — drag payloads, drop targets,
  hit testing, the three user stories that share the underlying machinery.
- [multi-window.md](multi-window.md) — `WindowConfig`, signal-driven
  multi-window orchestration, modal dialogs, restore-from-state.
- [title-bar.md](title-bar.md) — custom widget-level title bar plus the
  per-OS `PlatformTitleBarHost` for drag / zoom / close / inset.

## Data, persistence, telemetry

- [data-models.md](data-models.md) — `ListModel`, `TreeModel`,
  `SelectionModel`, sort/filter projections; the model layer that sits
  above the widget tree.
- [settings.md](settings.md) — reactive end-to-end persistence:
  `SettingsStore`, `SettingsFile<T>`, `MruList<T>`, window-state auto
  save/restore.
- [telemetry.md](telemetry.md) — consent-gated event reporting, the
  fern-collector / Plausible / OTLP adapters, the `events.yaml` schema
  pipeline.

## Specialized widgets

- [table-view.md](table-view.md) — virtualized `TableView` and `TreeTable`
  (multi-column, sort/filter, drag-resize, drag-reorder, full keyboard
  navigation).
- [tab-widget.md](tab-widget.md) — `TabBar<T>` and `TabWidget` (static +
  dynamic tabs, `Signal<Option<TabId>>` selection, pinned tabs, drag
  reorder, overflow dropdown, horizontal + vertical orientations).
- [charts.md](charts.md) — `BarChart` / `LineChart` / `PieChart`
  (shared axis / palette / legend / tooltip infrastructure).
- [fern-scene.md](fern-scene.md) — the pannable, zoomable scene viewport
  (canvases, board layouts, diagram editors).
- [fern-scene-a11y.md](fern-scene-a11y.md) — shaping the accessibility
  tree of a `fern-scene` viewport.

## Visuals & resources

- [icons-and-resources.md](icons-and-resources.md) — `res!()`-embedded
  SVG / PNG / WebP icons with theme-aware tinting.

## Tooling

- [inspector.md](inspector.md) — `fern-inspector`, the in-app debug
  surface (Tree / Properties / Accessibility / Theme / Models tabs;
  picker + bounds overlay; debug-only).

## In-progress design notes

Design and progress logs for unfinished work live in [plans/](plans/) —
they are intentionally not indexed here because their lifetime ends at
landing.
