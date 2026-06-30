<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

![CI](https://img.shields.io/github/actions/workflow/status/ferntech-eu/bastyde/ci.yml?branch=main&style=flat-square&label=CI)
![audit](https://img.shields.io/github/actions/workflow/status/ferntech-eu/bastyde/audit.yml?branch=main&style=flat-square&label=audit)
[![license](https://img.shields.io/badge/license-MPL--2.0-blue?style=flat-square)](#license)

# Bastyde

A pure-Rust, batteries-included GUI framework for desktop applications. Accessibility, internationalization, rich text, themes, persistent settings, drag-and-drop, charts, and a scene canvas all ship as first-class citizens.

```rust
use bastyde::prelude::*;
use bastyde::widgets::Button;

fn main() {
    BastydeAppBuilder::new()
        .theme(intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Hello Bastyde")
                .size(400, 300)
                .root(|tree, _state| {
                    tree.add(
                        Button::new(lit!("Click Me"))
                            .on_activate_fn(|_ctx| println!("Clicked!")),
                    )
                }),
        )
        .run();
}
```

Add reactive state and derived bindings:

```rust
use bastyde::prelude::*;
use bastyde::widgets::{Button, TextWidget, VStack};

fn main() {
    BastydeAppBuilder::new()
        .theme(intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Counter")
                .size(300, 150)
                .root(|tree, _state| {
                    let count = Signal::new(0_i32);
                    let label = count.map(|n| format!("Count: {n}"));
                    tree.add(
                        VStack::new()
                            .spacing(12.0)
                            .child(TextWidget::new(lit!("")).bind_text(label))
                            .child(
                                Button::new(lit!("Increment"))
                                    .on_activate_fn(move |_| {
                                        count.set(count.get() + 1)
                                    }),
                            ),
                    )
                }),
        )
        .run();
}
```

## Who is this for

Built primarily for professional desktop applications (writing tools, IDEs, dispatcher consoles, admin panels) where users spend hours and expect keyboard-first interaction, screen-reader support, and locale-aware formatting. Small tools and one-off utilities are equally well served: the batteries-included surface means a "window with a list and a few buttons" needs little more than the boilerplate above.

Particularly suited to projects with regulatory accessibility or internationalization requirements (EU Accessibility Act, US Section 508, France RGAA, government procurement, regulated industries such as healthcare and finance). Both AccessKit-backed a11y and Fluent-backed locale support are built into every widget rather than retrofitted, which makes conformance audits straightforward.

Also useful as a shelf of ready-to-use widgets if you're shopping the Rust GUI ecosystem for a specific component (rich text editor, table view, tree view, scene canvas, calendar, color picker) to drop into your app.

## Design priorities

Bastyde is a retained-tree framework inspired by Qt and the ShiftUI layouting.

- **Composition with painting layered on top.** The `Widget` trait offers both `build()` (compose children) and `paint()` (draw chrome); both methods are optional, and a single widget can do both. Most of the widgets are pure compositions of primitives (`RectWidget`, `TextWidget`, `HStack`, `Padding`); `Card`, `Panel`, overlays, and custom chrome layer paint on top of their composed children.
- **Accessibility at the trait level.** Every widget declares its role and name in an `accessibility()` method that sits beside `layout_response` and `paint`. The AT tree is built alongside the widget tree, not reconstructed from it.
- **Compile-time-checked i18n.** The `tr!` macro reads `.ftl` files at proc-macro expansion time and rejects missing keys, missing arguments, and unknown arguments at build time.
- **Rich-text stack as foundation.** The document model, shaping, BiDi, color emoji, and undo/redo are the foundation, not a widget on top. Even the plain `TextWidget` routes through it.
- **Two API surfaces, one semantics.** A fluent builder API for everything, plus an optional `bati!` macro for SwiftUI-style declarative syntax. The macro desugars one-to-one to builder calls, so you can mix both in one file.

## Status

Expect breaking changes between 0.x versions.

The test suite is roughly 2,600 tests in bastyde and over 4,000 across the whole stack. Tests target behavior (event dispatch, layout output, accessibility-tree structure), not implementation snapshots. The same widget tree runs under tests without a window, a GPU, or winit, and a simulated clock makes time-dependent behavior deterministic.

Bastyde builds on two earlier MPL-2.0 crates already at v1.x: [text-document](https://github.com/jacquetc/text-document) (rich-text document model) and [text-typeset](https://github.com/jacquetc/text-typeset) (typesetting engine).

Production deployment is currently limited to FernTech's own applications; the 0.x version label reflects this scope. The known gaps are listed at the end of this README.

This is one developer's project. Architecture, design reviews, code review and final acceptance were human; code generation and routine refactoring were LLM-assisted (Claude Opus and Mistral Medium) under that review.

**Scale:** 30+ framework crates · 200k+ lines of Rust · 100+ widgets · 1000+ builder methods.

## Authorship and review

The rules under which Bastyde is built:

1. Direct human communication is written by humans. PR messages, issues, posts, replies: no AI drafting, no AI polish. Common decency.

2. Documentation may be drafted by AI; every line is reviewed by a human. API examples must compile against the current API. Claims are checked, not skimmed.

3. Code, including tests, may be written by AI; every line is reviewed by a human. "Reviewed" means the reviewer understands the change well enough to defend it without the AI in the room. Vibe coding is forbidden. Plausible-looking code is not reviewed code.

4. Architecture and public API are human. AI implements within them; it does not design them. The load-bearing surface is specified by a human: the `Widget` trait, `Signal`/`Prop`, the event model, anything downstream apps depend on.

5. Authors and reviewers, both human, are the voluntary bottleneck. Final responsibility rests with them, not the AI. They may use any tool to help, AI included; what is missed lands on them regardless. They take their time; high-speed AI output is not a reason for high-speed work.

6. The human who signs the work owns it, AI or not. Provenance is not disclosed in commits or PR text.

7. No AI has ever been condemned by judges. Only humans and companies have. Stay sharp.

## What's in the box (really, not a wishlist)

**Widgets.** Around 100 widgets: buttons, lists, tables, trees, tabs, menus, dialogs, popovers, file/color/date pickers, calendar, charts, wizard, breadcrumb, masonry layout, split button, custom title bar, and rich text editor. The `widget-catalog` example shows most of them, others are in dedicated examples.

**Text.** Full rich-text stack: document model with tables, lists, and undo/redo; typesetting engine with shaping, bidirectional text, color emoji, and zoom without reflow. Even the plain `TextWidget` routes through it, so every label gets correct shaping and font fallback.

**Layout.** SwiftUI-style two-phase negotiation: parents propose sizes, children respond, parents place. Spacers and stretch behave like ordinary widgets, with no special cases.

**Reactive state.** One `Signal<T>` type, used everywhere. A color change repaints; a size change relayouts; nothing rebuilds that doesn't need to.

**Data models.** `ListModel<T>` and `TreeModel<T>` are generic over your domain type and drive `ListView`, `TreeView`, `TableView`, `TreeTableView`, `Repeater`, and `TabBar<T>` directly. Sort/filter projections, per-view tree expand state, shared selection, drag-and-drop reorder, and descendant-to-ancestor tri-state checkbox aggregation come built in.

**Rendering.** GPU-accelerated via wgpu, with text and graphics sharing one pipeline. When nothing is moving, the app is idle: no wasted frames, near-zero CPU and GPU use.

**Accessibility.** Every widget declares its role and name through AccessKit at the trait level. A per-widget override surface adjusts labels, descriptions, roles, and actions when the default isn't right.

**Internationalization.** Translations are checked at compile time via macro on top of Fluent: missing or misspelled keys are build errors. Right-to-left layout, locale-aware number and date formatting, and re-rendering on locale change are built in.

**Themes.** Default light and dark, inspired by JetBrains' Int UI; switching is instant and preserves focus, scroll position, and selection. On Linux, the active palette (accent, surface, selection, tooltip colors) follows the desktop environment (GNOME, KDE, Cinnamon). Apps can override anything from a single color to a whole widget's chrome via the four-tier styling system (tokens → variants → recipes → style protocols), described in [`docs/styling-system.md`](docs/styling-system.md).

**Input.** Keyboard shortcuts, menus, and accessibility actions flow through one rebindable pipeline; a user remap updates every surface that mentions the binding. External-source events (databases, file watchers) bypass it; widgets subscribe directly.

**Async.** Optional main-thread executor for imperative `async`/`.await` inside handlers: `spawn_local` for UI futures, `spawn_blocking` to offload work, and `spawn_local_with` to deliver a result with a fresh context. Off by default — the core stays synchronous and pays nothing; `bastyde-tokio` / `bastyde-async-std` add reactors for awaiting native runtime futures (timers, sockets, `reqwest`). For "data arrives, UI reacts," the reactive subscription path above stays simpler. See [`docs/async.md`](docs/async.md).

**Tooltips.** Three tiers from one system: plain text, rich (inline markup + shortcut hint + expandable detail), and composite (arbitrary widget body). Rich and composite tooltips become focusable on dwell.

**Animations.** Composable wrappers for the common cases (collapse, fade, slide, crossfade, blur, shake, pulse). The animation-owning wrappers honor the system "reduce motion" setting; the value-driven wrappers (`Blur`, `Rotate`) carry no motion of their own and delegate that gating to the caller's animate site (`to_or_snap`).

**Drag and drop.** Intra-app DnD with typed payloads, drop indicators, and edge auto-scroll. Cross-application (OS) DnD is supported in both directions — inbound drops and outbound app-to-OS export.

**Persistent settings.** Reactive K/V store and typed structs with migrations, atomic writes, and crash-safe quarantine of corrupt files. Automatic window-state restore with monitor-aware geometry sanitize.

**Scene canvas.** Pannable, zoomable viewport for non-grid content: story corkboards, mind maps, node-graph editors, simple maps. Heavyweight `Widget` nodes and lightweight `SceneItem`s coexist under one transform, both fully accessible.

**Charts.** BarChart, LineChart, and PieChart (with donut and center slot), generic over the app's data type. Locale-aware axis formatting and theme integration are built in.

**Web view (prototype).** Embed HTML / web content as a `WebView` widget — the one widget that can't render into the wgpu surface, so the engine is a native OS subview composited on top. It behaves like a normal widget otherwise: SwiftUI-style layout, dormancy-aware visibility (a tab-parked page hides its subview), JS↔Rust messaging, two-way URL binding, and `Role::WebView` accessibility. **Still a prototype.** The default engine is **wry** (macOS WKWebView / Windows WebView2 / Linux-X11 WebKitGTK) and is functional; on Linux it needs the WebKitGTK toolkit and, on a Wayland session, XWayland (see [`docs/web-view.md`](docs/web-view.md)). The **Servo** backend (the native Wayland path) is **work in progress** — it constructs a real engine but isn't frame-driven yet. See [`docs/web-view.md`](docs/web-view.md).

**Widget previewer.** Storybook-style 3-pane explorer (navigator, canvas, knob form) for the widget catalog, with live property editing, multi-variant rendering, and PNG export. Custom widget libraries register via `inventory::submit!` and become previewable with no extra wiring.

**Tooling.** In-app debug inspector (F12, debug builds only) with tabs for tree, properties, accessibility, theme, focus, shortcuts, overlays, and data models. Opt-in privacy-conscious telemetry stack with compile-time-validated event schemas and a build-time linter for schema drift.

**Agent automation (MCP).** A Model Context Protocol server lets an AI agent observe (the live accessibility tree plus screenshots) and drive (accessibility actions, synthetic pointer / key / IME input) a Bastyde app — in-process, with no OS accessibility layer needed. A debug-only Unix-socket bridge (Linux/macOS only; no surface in release builds) drives a live running app; a headless mode runs the toolkit's CI harness (and is a kit for building your own headless test harness — `bastyde-automation::execute` against your own tree). It reuses the same AccessKit tree every widget already declares: a node id is stable while the widget lives (re-find after a structural rebuild). Complements, rather than replaces, a real screen-reader smoke test. See [`docs/automation-mcp.md`](docs/automation-mcp.md).

For depth on any of these, see `docs/`.

## Getting started

```sh
cargo new my-app
cd my-app
cargo add bastyde
```

Then read the examples:

```sh
git clone https://github.com/ferntech-eu/bastyde
cd bastyde
cargo run -p simple-button      # the minimal app
cargo run -p widget-catalog     # browse every widget
cargo run -p file-dialogs       # native file dialogs
```

For a tour of every public widget API:

```sh
python3 tools/extract_widget_api.py --list
python3 tools/extract_widget_api.py button calendar tree_view
```

## Documentation

Reference documents live in `docs/`. Good entry points:

- `docs/architecture.md`, overall design and gaps
- `docs/layout-primitives.md`, the two-phase layout
- `docs/reactive-theme.md`, themes, color tokens, OS color integration
- `docs/shortcut-intent-action.md`, keyboard shortcuts and rebinding
- `docs/accessibility-overrides.md`, per-widget AT customization
- `docs/idle-and-animation.md`, idle discipline and the animation gates
- `docs/tooltips.md`, `docs/tab-widget.md`, `docs/title-bar.md`, `docs/drag-and-drop.md`

## Known gaps

What is not yet shipped:

- **CJK IME composition.** Latin and BiDi input compose correctly; Chinese, Japanese, and Korean input methods need to be tested by actual users.
- **X11 custom title bars.** Wayland, Windows, and macOS backends ship. On X11 the custom-chrome operations return `PlatformError::Unsupported` rather than failing silently, and the window falls back to native server-side decorations.
- **Mobile and web.** Linux and Windows are the primary targets. No mobile or web targets.
- **API stability.** Pre-1.0; breaking changes are expected between minor versions.

## Architecture stack

Bastyde is part of a small stack:

- [text-document](https://github.com/jacquetc/text-document), the document model. **Required dependency.**
- [text-typeset](https://github.com/jacquetc/text-typeset), the typesetting engine. **Required dependency.**
- [Qleany](https://github.com/jacquetc/qleany), an architecture materializer that generates Clean Architecture (Vertical Slice variant) in Rust or C++/Qt from a YAML manifest. Independent and optional; pairs naturally with Bastyde for application backends.

## Contributing

Bug reports and patches are welcome. Please open an issue before sending a non-trivial pull request so we can discuss whether the change fits.

- The framework was built to support FernTech's application portfolio; roadmap priorities are weighted by what those applications need.
- Architectural changes need a design discussion first. Surface-level changes (new builder methods, bug fixes, new examples) are easier.
- Tests are required for new code. The suite runs headlessly, with no GPU or display server.
- The `bati!` macro and the builder API both need to keep working. New widgets should be usable from both.

No CLA. A DCO sign-off (`git commit -s`) on each commit is enough.

## License

Mozilla Public License 2.0. See `LICENSE`. Bastyde can be used in commercial and closed-source software without restriction; modifications to the Bastyde files themselves must be shared under MPL2 if distributed; application code that merely uses Bastyde is under its own license.

## Commercial support

For priority bug fixes, written support, or an indemnification agreement, contact <support@ferntech.eu>. For everyone else, the issue tracker is the right place.

## Trademark

"Bastyde" is trademark of FernTech (EUIPO application 019381386, filed 2026-06-16). The MPL-2.0 source license does **not** grant trademark rights. Forks and derivative works may use the source code under MPL-2.0 but must adopt a distinct name and distinct branding when distributed (compare Firefox / Iceweasel, Chromium / Chrome). Nominative use ("built with Bastyde", "Bastyde-compatible widget", articles describing Bastyde) is fine. For other uses, contact <trademarks@ferntech.eu>.

## Acknowledgments

Bastyde builds on the work of others: AccessKit; winit and wgpu; HarfBuzz (via harfrust), swash, fontdb, etagere, and ICU4X; unicode-bidi and unicode-linebreak; Fluent and the Mozilla l10n team; the published design notes of the Druid, Masonry, and Xilem projects; and SwiftUI's layout protocol. Anthropic and Mistral provided the language models whose code generation contributed substantially under human review.
