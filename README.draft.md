# FernUI

A pure-Rust GUI framework for serious desktop applications: the kind of software a user sits down with for hours at a time and reaches for the keyboard before the mouse. The target use cases are long-lived professional tools: writing applications for novelists, IDEs and code editors, dispatch consoles, course managers, internal business tools. Accessibility, internationalization, and rich text aren't add-ons you bolt on later, but things the framework owes you from day one.

```rust
use fern_ui::prelude::*;
use fern_ui::widgets::Button;

fn main() {
    FernAppBuilder::new()
        .theme(intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Hello FernUI")
                .size(400, 300)
                .root(|tree, _state| {
                    tree.add(
                        Button::new_literal("Click Me")
                            .on_activate_fn(|_ctx| println!("Clicked!")),
                    )
                }),
        )
        .run();
}
```

## Design priorities

FernUI is a retained-tree framework. Accessibility, internationalization, and the text stack are load-bearing parts of the architecture rather than later additions.

- **Composition over painting.** Widgets are built from primitives (`RectWidget`, `TextWidget`, `HStack`, `Padding`, and so on) and the framework paints them. Of 78 widget files, 51 implement no `paint()` at all, including `Button`. The ones that do, mostly overlays and custom chrome, earn it.
- **Accessibility at the trait level.** Every widget declares its role and name in an `accessibility()` method that sits beside `layout_response` and `paint`. The AT tree is built alongside the widget tree, not reconstructed from it.
- **Compile-time-checked i18n.** The `tr!` macro reads `.ftl` files at proc-macro expansion time and rejects missing keys, missing arguments, and unknown arguments at build time.
- **A real text stack.** Rich text, meaning the document model, shaping, BiDi, color emoji, and undo/redo, is the foundation rather than a widget. Even the plain `TextWidget` routes through it.
- **Two API surfaces, one semantics.** A fluent builder API for everything, plus an optional `fern!` macro for SwiftUI-style declarative syntax. The macro desugars one-to-one to builder calls, so you can mix both in one file.

## Status

**Version 0.1, first public release.** Expect breaking changes between 0.x versions.

This is one developer's project. Architecture, design reviews, and final acceptance were human; code generation and routine refactoring were LLM-assisted (Claude Opus and Mistral) under that review. It is built on top of two earlier foundational crates ([text-document](https://github.com/jacquetc/text-document) and [text-typeset](https://github.com/jacquetc/text-typeset)). It moved fast, but it is not a sketch. The test suite is the evidence: roughly 2,600 tests in fern-ui, over 4,000 across the whole stack, and they test behavior, things like event dispatch, layout output, and accessibility-tree structure, rather than implementation snapshots. The same widget tree runs under tests without a window, a GPU, or winit, and a simulated clock makes time-dependent behavior deterministic.

It is in production use by FernTech's own applications, including the fern-collector dashboard. That is real but narrow exposure; treat 0.1 accordingly. The honest list of known gaps is at the end of this README.

**Scale:** 30 framework crates · ~224k lines of Rust · 101 widget files · 109 public widget structs · ~1,200 builder methods.

## What's in the box

**Widgets.** Around 100, covering the usual desktop surface (buttons, lists, tables, trees, tabs, menus, dialogs, popovers, file/color/date pickers, calendar, charts) plus less common ones: a wizard, a breadcrumb, a masonry layout, a split button, a three-tier tooltip system, a scene canvas, a custom title bar, and a rich text editor. The `widget-catalog` example shows them all.

**Layout.** A SwiftUI-style system: you describe intent, the framework negotiates sizes and places everything. Spacers and stretch behave like ordinary widgets, with no special cases to learn.

**Reactive state.** One reactive value type, used everywhere. Change a value and only what depends on it updates, at the right cost: a color change repaints, a size change re-lays-out, and nothing rebuilds that doesn't need to.

**Themes.** Light and dark out of the box. Switching is instant and keeps focus, scroll position, and selection intact. On Linux, accent colors follow the desktop environment.

**Internationalization.** Translations are checked at compile time, so a missing or misspelled key is a build error rather than a runtime surprise. Right-to-left layout, locale-aware number and date formatting, and re-rendering on locale change are built in.

**Accessibility.** Every widget ships with correct screen-reader semantics. When the default isn't right, a per-widget override surface lets you adjust labels, descriptions, roles, and actions.

**Input.** Keyboard shortcuts, menus, and accessibility actions flow through one rebindable pipeline, so a user can remap a key and every surface that mentions it updates. Events from external sources (databases, message buses, file watchers) stay separate: widgets subscribe to those directly.

**Text.** A full rich-text stack: a document model with tables, lists, and undo/redo, and a typesetting engine with proper shaping, bidirectional text, color emoji, and zoom without reflow. Even the plain text widget routes through it, so any label gets correct shaping and font fallback for free.

**Tooltips.** Three tiers from one system: a plain line of text; a rich tooltip with inline markup, a keyboard-shortcut hint, and an expandable detail section; and a composite tier that hosts arbitrary widgets such as tabs, charts, and conditional rows. Rich and composite tooltips can be pinned: dwell on one and it becomes a focusable surface the user can interact with.

**Animations.** Ready-made wrappers for the common cases (collapse, fade, slide, crossfade, blur, and more). Each respects the system "reduce motion" setting.

**Rendering.** GPU-accelerated, with text and graphics sharing one pipeline. When nothing is moving, the app is genuinely idle: no wasted frames, near-zero CPU and GPU use.

**And also.** Drag and drop, persistent settings with automatic window-state restore, an opt-in privacy-conscious telemetry stack, a pannable scene canvas, charts, an in-app debug inspector (compiled out of release builds), and a widget catalog browser.

For the depth behind any of these, see `docs/`.

## Getting started

```sh
cargo new my-app
cd my-app
cargo add fern-ui
```

Then read the examples:

```sh
git clone https://github.com/ferntech/fern-ui
cd fern-ui
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

- **macOS native menu bar.** Menus work; they don't yet live in the system menu bar.
- **CJK IME composition.** Latin and BiDi input compose correctly; Chinese, Japanese, and Korean input methods don't yet integrate with the input event flow.
- **Cross-application drag and drop.** Intra-app DnD with typed payloads works fully; cross-app DnD via the OS clipboard needs platform IPC that isn't in place.
- **X11 custom title bars.** Wayland, Windows, and macOS backends ship. On X11 the custom-chrome operations return `PlatformError::Unsupported` rather than failing silently, and the window falls back to native server-side decorations.
- **Mobile and web.** Linux and Windows are the primary targets; macOS works modulo the menu bar gap. No mobile or web targets.
- **Typed errors everywhere.** A few subsystems (notably parts of `fern-settings` and some SVG and date-time parsers) still panic on paths that should return typed errors.
- **A stable API**, and **fast issue response.** This is a one-person project one month past its initial framework work. Please be patient.

## Companion projects

FernUI is part of a small stack:

- [text-document](https://github.com/jacquetc/text-document), the document model FernUI uses for rich text.
- [text-typeset](https://github.com/jacquetc/text-typeset), the typesetting engine FernUI uses for text rendering.
- [Qleany](https://github.com/jacquetc/qleany), an architecture materializer that generates Clean Architecture in Rust or C++/Qt from a YAML manifest. Independent and optional; pairs naturally with FernUI for application backends.

FernUI depends on text-document and text-typeset.

## Contributing

Bug reports and patches are welcome. Please open an issue before sending a non-trivial pull request so we can discuss whether the change fits.

- The framework was built to support FernTech's application portfolio; roadmap priorities are weighted by what those applications need.
- Architectural changes need a design discussion first. Surface-level changes (new builder methods, bug fixes, new examples) are easier.
- Tests are required for new code. The suite runs headlessly, with no GPU or display server.
- The `fern!` macro and the builder API both need to keep working. New widgets should be usable from both.

No CLA. A DCO sign-off (`git commit -s`) on each commit is enough.

## License

Mozilla Public License 2.0. See `LICENSE`. FernUI can be used in commercial and closed-source software without restriction; modifications to the FernUI files themselves must be shared under MPL2 if distributed; application code that merely uses FernUI is under its own license.

## Commercial support

For priority bug fixes, written support, or an indemnification agreement, contact <support@ferntech.eu>. For everyone else, the issue tracker is the right place.

## Acknowledgments

FernUI builds on a lot of other people's work: AccessKit; winit and wgpu; HarfBuzz (via rustybuzz), swash, fontdb, etagere, and ICU4X; unicode-bidi and unicode-linebreak; Fluent and the Mozilla l10n team; and the published design notes of the Druid, Masonry, and Xilem projects, along with SwiftUI's layout protocol. And to Anthropic and Mistral, whose models did much of the typing.
