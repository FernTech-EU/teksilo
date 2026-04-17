# FernUI — Claude Code Reference

## Project Overview

FernUI is a pure-Rust GUI framework for serious desktop applications. Primary target: **Skribisto** (cross-platform writing app). Architecture: retained widget tree with SwiftUI-style layout, AccessKit accessibility, wgpu rendering.

- **License:** Proprietary — Copyright (c) 2026-2026 FernTech, all rights reserved
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
cargo test -p fern-core                        # Test a specific crate
cargo test -p fern-widgets                     # Includes layout integration tests
cargo doc --no-deps --open                     # Generate docs
```

## Tools

```bash
python3 tools/extract_widget_api.py --list                 # List all widget files
python3 tools/extract_widget_api.py Button HStack Dialog   # Extract public API + docs for widgets
python3 tools/extract_widget_api.py --all                  # Every widget
python3 tools/extract_widget_api.py Button -f json -o out.json   # JSON for tooling
```

[tools/extract_widget_api.py](tools/extract_widget_api.py) parses widget source files in [crates/fern-widgets/src/](crates/fern-widgets/src/) and emits their `//!` module header, `pub struct`/`enum`/`type`/`const` declarations with `///` docs, and `pub fn` builder methods from inherent `impl Foo { ... }` blocks. Skips `impl Widget for Foo` trait plumbing and `pub(crate)` items. Accepts type names (`Button`) or module names (`button`); flags `#[doc(hidden)]` and `#[cfg(...)]`. Use when reading a widget's public surface without opening the file, packing widget docs into LLM context, or auditing API coverage.

The workspace has two member globs: `crates/*` for libraries and `examples/*` for runnable demos. Examples live under [examples/](examples/) (e.g. `simple_button`, `text_and_layout`, `widget_catalog`, `data_collections`, `dialogs_and_popovers`, `menus_and_dropdowns`, `split_view`, `tab_widget`, `title_bar_demo`, `internationalization`).

Tests are fully headless — no Xvfb, no GPU, no display server needed.

## Coding Conventions

- **Module declarations:** Use 2018+ style (`mod foo;` with `foo.rs`), NOT `foo/mod.rs`
- **Builder pattern:** Fluent API throughout — `.child()`, `.spacing()`, `.style()`, etc.
- **Type erasure:** Non-generic Widget trait (Approach B) — concrete types erased at arena insertion
- **Unified Widget trait:** One trait for all widgets. `build(&mut self)` for composition, `paint()` for rendering.
- **Reactive properties:** `Signal<T>` for mutable state, `Prop<T>` for widget properties (static or signal-bound)
- **Event handlers:** Attached via `WidgetBuilder` methods (`.on_tap()`, `.on_hover()`, `.focusable()`) or `HandlerSet` in `build()`
- **Naming:** snake_case functions, CamelCase types, standard Rust conventions
- **Dependencies:** Centralized in workspace `[workspace.dependencies]`
- **No `mod.rs` files** — always use `foo.rs` alongside `foo/` directory

## Crate Architecture

```
fern-tokens          Pure data: Theme, Color, TextStyle, SpacingTokens, alignment
fern-canvas          Canvas API, RenderFrame, Path, Paint, geometry, TextBackend trait
fern-core            Widget traits, arena, layout, events, focus, state, gestures, overlays
fern-data            Reactive data models: ListModel, TreeModel, SelectionModel, ListDataSource
fern-widgets         ~35 widgets + ~19 layout primitives (Button, ListView, TreeView, MenuBar, Dialog, etc.)
fern-text            TextBackend impl via text-typeset (external path dep)
fern-i18n            Fluent-rs runtime: LocalizedString, I18nManager, locale resolution, file watcher
fern-i18n-macros     Compile-time tr! / tr_widget! proc macros (re-exported by fern-i18n)
fern-ui-macros       fern! DSL proc macro (re-exported by fern-ui as fern!)
fern-render          wgpu renderer: rect/SDF/quad pipelines, atlas upload, path atlas
fern-platform        winit + AccessKit adapter, event translation
fern-app             FernAppBuilder, WindowManager, event loop, command dispatch
fern-ui              Umbrella crate with re-exports and feature flags
```

Dependency flow: `tokens → canvas → core → data → widgets`, `canvas → text`, `canvas → render → platform → app → ui`, `i18n-macros → i18n`, `ui-macros → ui`

External path dependency: `text-typeset` lives at `../../../text-typeset` (outside workspace).

## Unified Widget Trait (V2)

One trait for all widgets — leaf, container, composite, hybrid:

```rust
pub trait Widget: std::fmt::Debug + 'static {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> { vec![] }
    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size; // required
    fn place_children(&self, _bounds: Rect, _proposal: SizeProposal, _children: &mut [WidgetPlacement], _ctx: &LayoutContext) {}
    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {}
    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
    fn children(&self) -> Vec<WidgetId> { vec![] }
    fn is_spacer(&self) -> bool { false }       // Spacer, Expand
    fn clips_children(&self) -> bool { false }   // ScrollArea, MaxSize
}
```

`size_that_fits` is the only required method. All others have sensible defaults.

- **Leaf** (TextWidget, RectWidget): `size_that_fits` + `paint`
- **Container** (VStack, HStack, ZStack): `size_that_fits` + `place_children` + `children`
- **Composing** (Button, Checkbox): `build` + `size_that_fits` (delegates to child) + `accessibility`
- **Hybrid** (Card, ScrollArea): `build` + `paint`

### Widget insertion

- `tree.add(w)` — add any widget to the tree
- `tree.add_child(parent, w)` — add as child of another widget
- `ctx.add(w)` — inside `BuildContext` during `build()`
- `ctx.add_boxed(w)` — inside `BuildContext`, accepts `Box<dyn Widget>`

## Layout Model

SwiftUI-style two-phase negotiation: parent proposes size → child responds with actual size → parent places child. All in logical pixels. `Leading`/`Trailing` instead of Left/Right (RTL-aware).

**Layout primitives** (in [crates/fern-widgets/src/primitives/](crates/fern-widgets/src/primitives/)): `HStack`, `VStack`, `ZStack`, `Grid`, `Wrap`, `Padding`, `Spacer`, `Center`, `Expand`, `FixedSize`, `MinSize`, `MaxSize`, `AspectRatio`, `Switcher`, `Divider`, `FocusRing`, `IconWidget`

**Rendering primitives:** `RectWidget`, `TextWidget`

## Signals & Reactivity (V2)

- `Signal<T>` — unified reactive type. `Signal::new(value)` for mutable, `signal.map(|v| ...)` for derived
- `Prop<T>` — widget property type: `Prop::Static(T)` or `Prop::Bound(Signal<T>)`. Methods accept `impl Into<Prop<T>>`
- `ObserverHandle` — RAII guard. Dropping removes the callback (no memory leak)
- `BindingLevel::RepaintOnly` (color changes) vs `BindingLevel::Relayout` (size changes)
- Property methods accept both plain values and signals: `TextWidget::new("Hello").color(Color::RED)` or `.color(signal)`
- `ctx.signal(value)` — create in build(), `ctx.effect(&signal, |v| ...)` — scoped effect (auto-cleaned on rebuild)
- `Signal<f32>::animate_to(target, duration, easing)` — smooth animation

Legacy types (`State<T>`, `DerivedState<T>`, `Reactive<T>`) exist in `fern-core::state` but are not used by widgets. All widget code uses `Signal`/`Prop`.

## Animation System

Smooth animated transitions for `Signal<f32>` values. Call `animate_to()` instead of `set()`.

**High-level API:**
```rust
// In build():
let sidebar_width = ctx.animated_signal(300.0);
let sidebar = ctx.add(FixedSize::new().bind_width(sidebar_width.clone()).child(content));

// In a handler:
sidebar_width.animate_to(0.0, Duration::from_millis(200), Easing::EaseInOut);
```

**Key types:**
- `Easing` — `Linear`, `EaseIn`, `EaseOut`, `EaseInOut` (in fern-tokens)
- `Signal<f32>::animate_to(target, duration, easing)` — start animated transition
- `AnimationScheduler` — internal scheduler that ticks each frame
- `BuildContext::animated_signal(value)` — creates animated Signal<f32>

**Files:** `fern-tokens/src/motion.rs`, `fern-core/src/animation.rs`, `fern-core/src/signal.rs`

## Event System (V2 Attached Handlers)

- **Preview pass** (root → target) + **Bubble pass** (target → root) — unchanged structure
- **Attached handlers** replace monolithic `event()`: `.on_tap()`, `.on_hover()`, `.on_key()`, `.on_focus()`, `.on_scroll()`, `.on_pointer_event()`, `.on_access_action()`
- Handlers attached via `WidgetBuilder` trait (blanket impl) or `HandlerSet` in `build()`
- Framework auto-wires gesture recognizers from handler types (on_tap → TapRecognizer)
- `EventHandlers` struct on `WidgetNode` stores closures, dispatched by framework
- `.focusable(true)`, `.cursor(CursorIcon::Pointer)` — framework-level properties on node
- Typed commands: `AppCommand` trait, emitted via `ctx.emit(cmd)` inside handlers

## Three-Tier Rendering

| Tier | Type | Used For |
|------|------|----------|
| 1 | `DecorationRect` | Backgrounds, borders, focus ring |
| 2 | `ShapeQuad` (SDF) | Rounded rects, circles, gradients |
| 3 | `PathEntry` (tiny-skia) | Arbitrary paths, SVG icons |
| Text | `GlyphQuad` | Glyph atlas text |

Three wgpu pipelines: `rect_pipeline`, `sdf_pipeline`, `quad_pipeline`.

## Theming

`Theme` struct with five token groups:

- `ColorTokens` — semantic roles (surface, primary, error, etc.) with interaction variants
- `SpacingTokens` — scale (xs→xxl) + semantic (widget_padding, content_padding)
- `TypographyTokens` — body, heading_1-3, label, caption, monospace
- `ShapeTokens` — corner radii, border widths, shadows
- `MotionTokens` — animation durations, easing curves

Built-in: `Theme::light_default()`, `Theme::dark_default()`. Runtime switching via `ctx.set_theme()`.

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

Test widgets: `FillWidget` (minimal leaf), `StackWidget` (minimal container) — in `fern-core::test_widgets` (pub(crate)).

## Implementation Status

### Complete

- Core framework (arena, layout engine, event dispatch, focus management)
- V2 Widget authoring model (unified Widget trait, Signal/Prop, attached handlers)
- Signal-based reactivity (Signal, Prop, ObserverHandle, scoped effects)
- Gesture recognition (UIKit-style state machines, auto-wired from handlers)
- Overlay system (OverlayManager, OverlayRequest, positioning)
- Design tokens (full Theme system)
- Window management (multi-window, modal dialogs, custom title bar)
- GPU rendering (3 pipelines, glyph atlas, path atlas)
- All ~19 layout primitives (including Grid, Wrap, AspectRatio, Switcher)
- Accessibility (AccessKit integration at trait level)
- Animation system (`Signal<f32>::animate_to`, easing, per-frame scheduler)
- Internationalization (fern-i18n + fern-i18n-macros: Fluent-rs, `tr!`/`tr_widget!`, locale resolution, file watcher, RTL direction signal)
- `fern!` DSL (fern-ui-macros: block-structured widget-tree syntax, desugars to V2 builder calls — see `docs/fern-macro-reference.md`)
- Reactive data models (fern-data: `ListModel`, `TreeModel`, `TreeSlice`, `SelectionModel`)
- Controls: Button, Checkbox, RadioButton, Toggle, Slider, ComboBox, SegmentedControl, ProgressBar, Link, Badge
- Containers: Panel, Card, Accordion, ScrollArea, ScrollBar, Tooltip, SplitView, TabWidget, Dialog, Popover, Snackbar, Wizard, Breadcrumb
- Menus: MenuBar, MenuList, MenuItem, MenuContext (context menu)
- Chrome: Toolbar, StatusBar, TitleBar
- Data-driven: ListView, TreeView, Repeater (backed by fern-data models)

### Partial / In Progress

- Text rendering (depends on external text-typeset)
- ScrollArea (viewport clipping + scroll bars work, no virtualized content yet)

### Not Started

- Rich text editor widget
- Drag-and-drop
- Text input / IME
- Clipboard integration

## Key Files

- Workspace config: `Cargo.toml`
- Widget trait: `crates/fern-core/src/widget.rs`
- Signal/Prop system: `crates/fern-core/src/signal.rs`
- BuildContext: `crates/fern-core/src/build_context.rs`
- Event handlers: `crates/fern-core/src/event_handlers.rs`
- WidgetBuilder trait: `crates/fern-core/src/widget_builder.rs`
- Arena: `crates/fern-core/src/arena.rs`
- Widget tree orchestrator: `crates/fern-core/src/widget_tree.rs`
- State system: `crates/fern-core/src/state.rs`
- Event types: `crates/fern-core/src/event.rs`
- Theme: `crates/fern-tokens/src/theme.rs`
- Color tokens: `crates/fern-tokens/src/color.rs`
- Button (reference widget): [crates/fern-widgets/src/button.rs](crates/fern-widgets/src/button.rs)
- Switcher: [crates/fern-widgets/src/primitives/switcher.rs](crates/fern-widgets/src/primitives/switcher.rs)
- Layout primitives: [crates/fern-widgets/src/primitives/](crates/fern-widgets/src/primitives/)
- Data models: [crates/fern-data/src/](crates/fern-data/src/) (`list_model.rs`, `tree_model.rs`, `selection_model.rs`)
- i18n runtime: [crates/fern-i18n/src/manager.rs](crates/fern-i18n/src/manager.rs), [crates/fern-i18n/src/localized_string.rs](crates/fern-i18n/src/localized_string.rs)
- i18n macros: [crates/fern-i18n-macros/src/lib.rs](crates/fern-i18n-macros/src/lib.rs)
- fern! DSL macro: [crates/fern-ui-macros/src/](crates/fern-ui-macros/src/) (parse → IR → lower). Trybuild fixtures at [crates/fern-ui/tests/fern_ui/pass/](crates/fern-ui/tests/fern_ui/pass/)
- fern! reference: [docs/fern-macro-reference.md](docs/fern-macro-reference.md) (user-facing), [docs/fern-language-spec-v3.md](docs/fern-language-spec-v3.md) (design spec)
- Canvas API: `crates/fern-canvas/src/canvas.rs`
- Renderer: `crates/fern-render/src/renderer.rs`
- App builder: `crates/fern-app/src/app.rs`
- Umbrella exports: `crates/fern-ui/src/lib.rs`

## Widget Construction Patterns

```rust
// Inline children (most common) — .child() accepts impl Widget + 'static
VStack::new().spacing(10.0)
    .child(TextWidget::new("Hello").style(theme.typography.heading_1.clone()))
    .child(Button::new("Click").on_click(MyCmd::DoThing))

// Pre-registered children (when you need the ID) — .add_child() takes WidgetId
let label_id = ctx.add(TextWidget::new("Status").bind_text(status_signal));
HStack::new().add_child(label_id)

// Iterator children
VStack::new().children(items.iter().map(|item| TextWidget::new(item.name.clone())))

// Conditional children
container.child_opt(show_extra.then(|| TextWidget::new("Extra")))

// Switcher — shows one child at a time, driven by Signal<usize>
let selected = ctx.signal(0_usize);
ctx.add(Switcher::new(selected.clone())
    .child(TextWidget::new("Page 0"))
    .child(TextWidget::new("Page 1"))
    .child(TextWidget::new("Page 2")))

// Composing widget — build() creates child subtree, &mut self
#[derive(Debug)]
struct MyWidget {
    root_child_id: Option<WidgetId>,
}

impl Widget for MyWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let root = ctx.add(VStack::new()
            .child(TextWidget::new("Hello").style(theme.typography.heading_1.clone()))
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
    }
}

// Attached event handlers — via WidgetBuilder on child widgets
ctx.add(
    MinSize::new(48.0, 48.0).child(content)
        .on_tap(|ctx| { ctx.emit(Cmd::Clicked); })
        .on_hover(move |entered, _ctx| { interaction.set(if entered { Hovered } else { Idle }); })
        .focusable(true)
        .cursor(CursorIcon::Pointer)
)

// Attached event handlers — via HandlerSet on self in build()
fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    let handlers = HandlerSet::new()
        .on_tap(move |ctx| { /* ... */ })
        .on_hover(move |entered, _ctx| { /* ... */ })
        .focusable(true)
        .cursor(CursorIcon::Pointer);
    ctx.apply_self_handlers(handlers);
    // ...
}
```

## `fern!` DSL

Block-structured DSL for widget trees. Desugars one-to-one to builder
calls at macro-expansion time — no runtime, no virtual tree.

```rust
use fern_ui::prelude::*;

fn build(ctx: &mut BuildContext) -> WidgetId {
    fern!(ctx =>
        VStack {
            spacing: 12.0
            TextWidget::new_literal("Title") { style: t.body_bold.clone() }
            open_btn = Button("Open") {
                on_activate: Cmd::Open
            }
            TextWidget("Status") { linked_to: open_btn }
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

See [docs/fern-macro-reference.md](docs/fern-macro-reference.md) for
the full surface language, desugaring cheat sheet, and limitations;
[docs/fern-language-spec-v3.md](docs/fern-language-spec-v3.md) for the
design spec with worked translations of the widget-catalog examples.
Slash command `/fern-macro` loads the skill for read/write/explain/
translate/debug workflows.

## App Entry Point Pattern

```rust
#[derive(Debug, Clone, PartialEq)]
enum MyCmd { Save, ToggleTheme }
impl AppCommand for MyCmd {}

fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("My App")
        .window_size(800, 600)
        .on_command(|cmd: &MyCmd, ctx: &mut CommandContext| match cmd {
            MyCmd::Save => { /* call Qleany controller */ }
            MyCmd::ToggleTheme => { ctx.set_theme(Theme::dark_default()); }
        })
        .root(|tree| tree.add(MyRootWidget::new()))
        .run();
}
```

`CommandContext` provides: `set_theme()`, `create_window()`, `close_window()`, `source_window()`.

## Architecture Reference

Full architecture document: `../fern-ui-perso/fern-ui-architecture.md` (28 sections, covers layout model, scrolling, widget state, reactivity, overlays, DnD, data sources, Canvas API, rendering pipeline, theming, threading, accessibility, window management, testability, i18n)