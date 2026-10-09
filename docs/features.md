<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Features

<!-- BEGIN README: What's in the box (really, not a wishlist) -->
**Widgets.** Around 100 widgets: buttons, lists, tables, trees, tabs, menus, dialogs, popovers, file/color/date pickers, calendar, charts, wizard, breadcrumb, masonry layout, split button, custom title bar, and rich text editor. The `widget-catalog` example shows most of them, others are in dedicated examples.

**Text.** Full rich-text stack: document model with tables, lists, and undo/redo; typesetting engine with shaping, bidirectional text, color emoji, and zoom without reflow. Even the plain `TextWidget` routes through it, so every label gets correct shaping and font fallback.

**Layout.** SwiftUI-style two-phase negotiation: parents propose sizes, children respond, parents place. Spacers and stretch behave like ordinary widgets, with no special cases.

**Reactive state.** One `Signal<T>` type, used everywhere. A color change repaints; a size change relayouts; nothing rebuilds that doesn't need to.

**Data models.** `ListModel<T>` and `TreeModel<T>` are generic over your domain type and drive `ListView`, `TreeView`, `TableView`, `TreeTableView`, `GridView`, `Repeater`, and `TabBar<T>` directly. Sort/filter projections, per-view tree expand state, shared selection, drag-and-drop reorder, and descendant-to-ancestor tri-state checkbox aggregation come built in.

**Rendering.** GPU-accelerated via wgpu, with text and graphics sharing one pipeline. When nothing is moving, the app is idle: no wasted frames, near-zero CPU and GPU use.

**Accessibility.** Every widget declares its role and name through AccessKit at the trait level, and a per-widget override surface adjusts labels, descriptions, roles, relationships, and actions when the default isn't right. The default light and dark themes meet WCAG 2.1 AA contrast (CI-enforced), an opt-in high-contrast variant follows the OS "increase contrast" setting (re-queried on window focus), and conformance obligations attach to the shipped application.

**Internationalization.** Translations are checked at compile time via macro on top of Fluent: missing or misspelled keys are build errors. Right-to-left layout, locale-aware number and date formatting, and re-rendering on locale change are built in.

**Themes.** Default light and dark, inspired by JetBrains' Int UI; switching is instant and preserves focus, scroll position, and selection. On Linux, the active palette (accent, surface, selection, tooltip colors) follows the desktop environment (GNOME, KDE, Cinnamon). Material 3, Fluent (Windows 11) and macOS Aqua presets ship behind the `theme-material3` / `theme-fluent` / `theme-macos` features. Apps can override anything from a single color to a whole widget's chrome via the four-tier styling system (tokens → variants → recipes → style protocols), described in [`docs/styling-system.md`](styling-system.md).

**Input.** Keyboard shortcuts, menus, and accessibility actions flow through one rebindable pipeline; a user remap updates every surface that mentions the binding. External-source events (databases, file watchers) bypass it; widgets subscribe directly.

**Async.** Optional main-thread executor for imperative `async`/`.await` inside handlers: `spawn_local` for UI futures, `spawn_blocking` to offload work, and `spawn_local_with` to deliver a result with a fresh context. Off by default: the core stays synchronous and pays nothing; `teksilo-tokio` / `teksilo-async-std` add reactors for awaiting native runtime futures (timers, sockets, `reqwest`). For "data arrives, UI reacts," the reactive subscription path above stays simpler. See [`docs/async.md`](async.md).

**Tooltips.** Three tiers from one system: plain text, rich (inline markup + shortcut hint + expandable detail), and composite (arbitrary widget body). Rich and composite tooltips become focusable on dwell.

**Animations.** Composable wrappers for the common cases (collapse, fade, slide, crossfade, blur, shake, pulse). The animation-owning wrappers honor the system "reduce motion" setting; the value-driven wrappers (`Blur`, `Rotate`) carry no motion of their own and delegate that gating to the caller's animate site (`to_or_snap`).

**Drag and drop.** Intra-app DnD with typed payloads, drop indicators, and edge auto-scroll. Cross-application (OS) DnD is supported in both directions: inbound drops and outbound app-to-OS export.

**Persistent settings.** Reactive K/V store and typed structs with migrations, atomic writes, and crash-safe quarantine of corrupt files. Automatic window-state restore with monitor-aware geometry sanitize.

**Scene canvas.** Pannable, zoomable viewport for non-grid content: story corkboards, mind maps, node-graph editors, simple maps. Heavyweight `Widget` nodes and lightweight `SceneItem`s coexist under one transform, with accessibility APIs for both tiers.

**Charts.** BarChart, LineChart, and PieChart (with donut and center slot), generic over the app's data type. Pluggable axis-label formatters and theme integration are built in.

**Live pictures.** `LiveImage` shows a picture another thread rewrites at display rate: a VM screen, a video, a camera preview. A commit uploads only what changed and repaints no widget, a window nobody can see does no work for it, and a texture is freed at the first frame that no longer draws it, so GPU memory stays flat. Input maps to the source pixel under it, and the automation tools aim at and read back the picture's pixels. See [`docs/live-image.md`](live-image.md).

**Web view (prototype).** Embed HTML / web content as a `WebView` widget, the one widget that can't render into the wgpu surface, so the engine is a native OS subview composited on top. It behaves like a normal widget otherwise: SwiftUI-style layout, dormancy-aware visibility (a tab-parked page hides its subview), JS↔Rust messaging, two-way URL binding, and `Role::WebView` accessibility. **Still a prototype.** The default engine is **wry** (macOS WKWebView / Windows WebView2 / Linux-X11 WebKitGTK) and is functional; on Linux it needs the WebKitGTK toolkit and, on a Wayland session, XWayland (see [`docs/web-view.md`](web-view.md)). The **Servo** backend (the native Wayland path) is **work in progress**: it constructs a real engine but isn't frame-driven yet. See [`docs/web-view.md`](web-view.md).

**Widget previewer.** Storybook-style 3-pane explorer (navigator, canvas, knob form) for the widget catalog, with live property editing, multi-variant rendering, and PNG export. Custom widget libraries register via `inventory::submit!` and become previewable with no extra wiring.

**Tooling.** In-app debug inspector (F12, debug builds only) with tabs for tree, properties, accessibility, theme, locale, focus, shortcuts, overlays, data models, and pointers. Opt-in privacy-conscious telemetry stack with compile-time-validated event schemas and a schema-drift linter (`cargo teksilo-telemetry-lint`).

**Agent automation (MCP).** A Model Context Protocol server lets an AI agent observe (the live accessibility tree plus screenshots) and drive (accessibility actions, synthetic pointer / key / IME input) a Teksilo app, in-process, with no OS accessibility layer needed. A debug-only bridge drives a live running app on Linux, Windows and macOS, a Unix-domain socket where the platform has one, a named pipe on Windows, and no surface at all in a release build, and the running app publishes an endpoint descriptor, so an agent attaches with `--attach` rather than a socket path scraped out of a log; a headless mode runs the toolkit's CI harness (and is a kit for building your own headless test harness: `teksilo_automation::execute` against your own tree). It reuses the same AccessKit tree every widget already declares: a node id is stable while the widget lives (re-find after a structural rebuild). Complements, rather than replaces, a real screen-reader smoke test. See [`docs/automation-mcp.md`](automation-mcp.md).

See the [guide index](guide-index.md) for detailed documentation.
<!-- END README: What's in the box (really, not a wishlist) -->

See the [widget catalog](widgets/index.md) for examples and API links, or the
[guide index](guide-index.md) for subsystem documentation.
