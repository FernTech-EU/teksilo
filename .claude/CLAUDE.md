# FernUI — Claude Code Reference

## Project Overview

FernUI is a pure-Rust GUI framework for serious desktop applications. Architecture: retained widget tree with SwiftUI-style layout, AccessKit accessibility, wgpu rendering.

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
cargo run -p drag-and-drop                      # Drag-and-drop showcase
cargo run -p multi_window                      # Multi-window demo
cargo run -p recent_projects                   # MRU/persistence demo
cargo run -p rich_text_editor                  # Rich text editing
cargo run -p rich_text_viewer                  # Rich text viewing
cargo run -p spin_box                          # Numeric input demo
cargo run -p tool_box                          # Tool box widget demo
cargo run -p fern-widgets-previewer            # Widget catalog previewer
cargo run -p data-grid                          # TableView showcase (1k rows × 6 cols)
cargo run -p tree-table                         # TreeTable showcase (mock filesystem)
cargo run -p datetime-pickers                   # Calendar / DateEdit / TimeEdit / DateTimeEdit gallery
cargo run -p file-dialogs                       # Native file open / save / pick-folder showcase
```

## Tools

```bash
python3 tools/extract_widget_api.py --list                 # List all widget files
python3 tools/extract_widget_api.py Button HStack Dialog   # Extract public API + docs for widgets
python3 tools/extract_widget_api.py --all                  # Every widget
python3 tools/extract_widget_api.py Button -f json -o out.json   # JSON for tooling
python3 tools/bench_examples.py                          # Run benchmarks with report generation
```

[tools/extract_widget_api.py](tools/extract_widget_api.py) parses widget source files in [crates/fern-widgets/src/](crates/fern-widgets/src/) and emits their `//!` module header, `pub struct`/`enum`/`type`/`const` declarations with `///` docs, and `pub fn` builder methods from inherent `impl Foo { ... }` blocks. Skips `impl Widget for Foo` trait plumbing and `pub(crate)` items. Accepts type names (`Button`) or module names (`button`); flags `#[doc(hidden)]` and `#[cfg(...)]`. Use when reading a widget's public surface without opening the file, packing widget docs into LLM context, or auditing API coverage.

The workspace has two member globs: `crates/*` for libraries and `examples/*` for runnable demos. Examples live under [examples/](examples/) (e.g. `simple_button`, `text_and_layout`, `widget_catalog`, `data_collections`, `dialogs_and_popovers`, `menus_and_dropdowns`, `split_view`, `tab_widget`, `title_bar_demo`, `internationalization`, `shortcuts_demo`, `recent_projects`, `drag_and_drop`, `multi_window`, `rich_text_editor`, `rich_text_viewer`, `spin_box`, `tool_box`).

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
- **Error types:** Use `thiserror` (workspace dep) — `#[derive(thiserror::Error)]` with `#[error("...")]` per variant; `#[from]` for transparent conversions, `#[source]` for nested error chains. Don't hand-roll `Display` / `std::error::Error` / `From`.

## Crate Architecture

```
fern-tokens          Pure data: Theme, Color, TextStyle, SpacingTokens, alignment
fern-canvas          Canvas API, RenderFrame, Path, Paint, geometry, TextBackend trait
fern-core            Widget traits, arena, layout, events, focus, state, gestures, overlays
fern-data            Reactive data models: ListModel, TreeModel, SelectionModel, ListDataSource,
                     SortFilterListModel<T> (sort + filter projection over a flat source),
                     SortFilterTreeModel<T> (same for trees, with TreeFilterMode strategies)
fern-settings        Persistent reactive prefs: SettingsStore (dotted-key Signal<T>), SettingsFile<T>,
                     PersistedListModel/PersistedTreeModel, MruList<T: MruEntry>, WindowStateService
fern-telemetry       Privacy-respecting product analytics built on fern-settings: ConsentStore,
                     InstallId, TelemetryBundle, recent-log ring buffer. RGPD-compliant by
                     construction. Reference: docs/telemetry.md. Design + progress log:
                     docs/plans/telemetry-plan.md.
fern-analytics-plausible  Plausible adapter (anonymous mode). HTTP + retry/backoff + redb queue.
fern-analytics-fern  Home-grown gRPC adapter for the FernUI-operated fern-collector backend.
                     Anonymous + pseudonymous modes; bearer token + TLS; fetch + erase wired.
fern-analytics-otlp  OTLP/HTTP-logs adapter. Maps FernUI events to OTLP LogRecords; worker
                     thread with batching, exponential backoff, flush-on-shutdown.
fern-telemetry-codegen  Proc-macro: `include_telemetry_schema!("events.yaml")` reads a YAML
                     manifest at compile time and expands to typed `emit_*` functions + enum
                     types. Validates required fields, prop types, enum variants, expiry dates.
cargo-fern-telemetry-lint  CLI schema-drift linter. Checks expiry, required fields, unused
                     events (declared but not emitted in src/), unknown prop types. Run as
                     `cargo fern-telemetry-lint`. CI mode: `--fail-on-warnings`.
fern-widgets         ~56 widgets + ~21 layout primitives (Button, ListView, TreeView, TableView,
                     TreeTable, MenuBar, Dialog, TextInput, SpinBox, etc.)
fern-charts          BarChart, LineChart, PieChart (pie + donut, with center slot). Sits at the same tier
                     as fern-widgets — no dep on widgets. See docs/plans/charts-plan.md.
fern-text            TextBackend impl via text-typeset (external path dep)
fern-i18n            Fluent-rs runtime: LocalizedString, I18nManager, locale resolution, file watcher.
                     Also locale-aware formatters: NumberFormatter / FernDateTimeFormatter
                     (Signal<T> → Signal<String>), FernDateTime, plus a custom DATETIME() Fluent
                     function and a `bundle.set_formatter` callback so `{ NUMBER(...) }` and
                     `{ DATETIME(...) }` inside .ftl messages render correctly across locales.
                     Built on icu_decimal + icu_datetime + icu_calendar + intl-memoizer.
fern-i18n-macros     Compile-time tr! / tr_widget! proc macros (re-exported by fern-i18n).
                     Also tr_signal! / tr_signal_widget! — reactive variants that accept
                     Signal<T> args and return Signal<String> re-rendering on (any arg ∪
                     locale ∪ hot-reload) change.
fern-ui-macros       fern! DSL proc macro (re-exported by fern-ui as fern!)
fern-render          wgpu renderer: rect/SDF/quad pipelines, atlas upload, path atlas
fern-platform        winit + AccessKit adapter, event translation, clipboard, OS theme,
                     native file dialogs (FileDialogBackend trait + RfdAsyncBackend)
fern-app             FernAppBuilder, WindowManager, event loop
fern-ui              Umbrella crate with re-exports and feature flags
fern-resources       Resource handling and embedding infrastructure
fern-preview         Widget previewer infrastructure (trait + types + inventory registry)
fern-preview-ui      GUI library for widget previewer
fern-widgets-previewer Previewer binary for fern-widgets catalog
```

Dependency flow: `tokens → canvas → core → data → widgets`, `canvas → text`, `core + data → settings`, `canvas → render → platform → app → ui`, `settings → app`, `i18n-macros → i18n`, `ui-macros → ui`, `core → preview`, `preview-ui → preview + widgets`, `widgets-previewer → (preview + preview-ui + widgets)`

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

`layout_response` is the only required method. It returns a `LayoutResponse { size: Size, flex: f32 }` carrying both the size the widget wants and a flex weight for slack distribution. Most widgets just return a `Size` (auto-converts via `From<Size>` to `flex = 0`); flex-bearing widgets like `Spacer` and `Expand` return `LayoutResponse::flexible(size, flex)`.

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

SwiftUI-style two-phase negotiation: parent proposes size → child responds with wanted size → parent places child. All in logical pixels. `Leading`/`Trailing` instead of Left/Right (RTL-aware).

**Flex distribution in stacks.** `HStack`/`VStack` honor every child's wanted size as a floor, then distribute any **slack** (`bounds − Σ wanted − spacing`) proportional to the child's `flex` weight (carried in the same `LayoutResponse` query — no separate trait method). Default flex is `0.0` (rigid). `Spacer` and `Expand` return `1.0`. Ratios are first-class:

```rust
HStack::new()
    .child(Expand::new().flex(1).child(panel_a))   // 1/3 of slack
    .child(Expand::new().flex(2).child(panel_b))   // 2/3 of slack
```

`Expand::new()` defaults to `flex(1)` and stretches its child to its bounds. Default basis is **zero** (CSS flex-basis: 0) so ratios divide bounds cleanly. Call `.respect_intrinsic()` for **auto** basis, where the wrapped child's natural size acts as a floor and slack is added on top — useful in unconstrained parents (e.g. an outer `VStack` with `height = None`) where zero-basis would let the child overflow. Use `.align_child(Alignment::X)` to opt out of fill and align the child at its natural size. `Center::new()` is sugar for `Expand::new().align_child(CENTER)`.

**Layout primitives** (in [crates/fern-widgets/src/primitives/](crates/fern-widgets/src/primitives/)): `HStack`, `VStack`, `ZStack`, `Grid`, `Wrap`, `Padding`, `Spacer`, `Center`, `Expand`, `FixedSize`, `MinSize`, `MaxSize`, `AspectRatio`, `Switcher`, `Divider`, `FocusRing`, `IconWidget`, `ImageWidget`, `MasonryLayout`, `FormLayout`

**Rendering primitives:** `RectWidget`, `TextWidget`

**Text editing primitives:** `TextInputField` (gated behind `rich-text` feature)

## Signals & Reactivity (V2)

- `Signal<T>` — unified reactive type. `Signal::new(value)` for mutable, `signal.map(|v| ...)` for derived
- Multi-source combinators: `a.zip(&b)` / `a.zip3(&b, &c)` on any `Signal<T: Clone>`; `a.and(&b)` / `a.or(&b)` / `s.not()` on `Signal<bool>`. Derived signals dirty-track **every** upstream root, so widgets binding to a composite predicate re-render on any source change.
- `Prop<T>` — widget property type: `Prop::Static(T)` or `Prop::Bound(Signal<T>)`. Methods accept `impl Into<Prop<T>>`
- `ColorProp` / `TextStyleProp` — theme-aware prop types for colors and text styles. See **Theming** below.
- `ObserverHandle` — RAII guard. Dropping removes the callback (no memory leak)
- `BindingLevel::RepaintOnly` (color changes) vs `BindingLevel::Relayout` (size changes)
- Color-accepting methods take `impl Into<ColorProp>` — accepts `Color`, a role (`TextRole`, `SurfaceRole`, `BorderRole`), a `Signal<Color>`, or a `Signal<Role>`. Prefer roles for theme-driven colors; a bare `Color` is frozen.
- `ctx.signal(value)` — create in build(), `ctx.effect(&signal, |v| ...)` — scoped effect (auto-cleaned on rebuild)
- `Signal<f32>::animate_to(target, duration, easing)` — smooth animation

Legacy types (`State<T>`, `DerivedState<T>`, `Reactive<T>`) exist in `fern-core::state` but are not used by widgets. All widget code uses `Signal`/`Prop`.

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
let sidebar = ctx.add(FixedSize::new().bind_width(sidebar_width.clone()).child(content));
let slide = ctx.animate().normal().standard();   // duration_normal + easing_standard

// In a handler:
slide.to_or_snap(&sidebar_width, 0.0);   // snaps under prefers-reduced-motion
```

**Recommended API (looping with sub-perceptual epsilon):**
```rust
// `sweep()` reads `duration_indeterminate_sweep` AND turns on looping
// mode with epsilon = 1/255 + 30 Hz frame interval defaults.
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
| `.looping()` | sub-perceptual `epsilon = 1/255` + 30 Hz frame interval |
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

**Animated wrapper widgets** (live in [crates/fern-widgets/src/animations/](crates/fern-widgets/src/animations/), re-exported flat from `fern_ui::widgets`):

- `Collapse { expanded: Signal<bool>, child }` — wraps a child and animates
  its height (and width gate) between zero and natural when `expanded` flips.
  Used internally by `Accordion`. See [crates/fern-widgets/src/animations/collapse.rs](crates/fern-widgets/src/animations/collapse.rs).
- `Fade { visible: Prop<bool>, child }` — wraps a child and animates the
  entire subtree's opacity between 0 and 1. Layout-transparent: the child
  reports its full natural size at all opacity values. Built on
  `BuildContext::set_opacity` (a node-level opacity scope, parallel to
  `clips_children`). See [crates/fern-widgets/src/animations/fade.rs](crates/fern-widgets/src/animations/fade.rs).
- `Pulse::opacity(min, max).period(d).child(w)` — sine-driven looping
  opacity oscillation. The blinking-red-light / recording-indicator
  pattern. Layout-transparent (same as `Fade`). Reduced motion: pins
  at midpoint. See [crates/fern-widgets/src/animations/pulse.rs](crates/fern-widgets/src/animations/pulse.rs).
- `Cycle::new().period(d).child(a).child(b)…` — steps through children
  on a fixed period (rotating loading tips, status displays). Internally
  a `Switcher` driven by a frame-tick effect. See [crates/fern-widgets/src/animations/cycle.rs](crates/fern-widgets/src/animations/cycle.rs).
- `SmoothSize::new().child(w)` — auto-sizes the slot to the child's
  current intrinsic size, *animating* every change. The "empty panel
  that suddenly must grow gracefully to accept new content" case.
  Distinct from `FixedSize::bind_width(animated_signal)` (numeric target)
  — `SmoothSize` watches the child measure each frame. `.axes(Width|Height|Both)`
  to restrict. Reuses Collapse's "child laid out at natural, framework
  clips overflow" trick. See [crates/fern-widgets/src/animations/smooth_size.rs](crates/fern-widgets/src/animations/smooth_size.rs).
- `Crossfade::new(key_signal, |k| build_for(k))` — when the key
  changes, mounts both old and new content side by side in a `ZStack`,
  fades old → 0 and new → 1. Builders may run more than once per
  lifetime as keys recur. `.duration(d)` overrides the default.
  See [crates/fern-widgets/src/animations/crossfade.rs](crates/fern-widgets/src/animations/crossfade.rs).
- `Slide::new(visible).from(SlideEdge).child(w)` — slides a child in/out
  from the chosen edge (Leading/Trailing/Top/Bottom). Translates child
  position via `place_children`, doesn't change layout size — siblings
  don't reflow. Pair with `Fade` for the snackbar pattern. Clips so the
  off-edge child doesn't bleed past the slot. See [crates/fern-widgets/src/animations/slide.rs](crates/fern-widgets/src/animations/slide.rs).
- `Shake::new(trigger).child(w)` — bumping `trigger: Signal<u32>`
  plays a damped horizontal oscillation (defaults to
  `MotionTokens::duration_slow`, 4 cycles). Invalid-input feedback.
  Layout-stable, clips. Reduced motion: trigger is a no-op. See
  [crates/fern-widgets/src/animations/shake.rs](crates/fern-widgets/src/animations/shake.rs).
- `Scale::new(visible).child(w)` — uniform 2D visual scale 0↔1 driven
  by `Prop<bool>`. Built on `BuildContext::set_transform` (a node-level
  transform scope, parallel to `set_opacity`). Default: visual-only
  (slot stays at natural size, only the visual scales around the
  origin). `.reflow(true)` switches to layout-driving mode where the
  slot itself shrinks (siblings reflow); pair with `.origin(ScaleOrigin::TopLeading)`
  for the "card removal" pattern. See [crates/fern-widgets/src/animations/scale.rs](crates/fern-widgets/src/animations/scale.rs).
- `Rotate::new(angle_signal).child(w)` — rotates a child subtree by
  `angle: Prop<f32>` (radians). No internal animation; caller drives
  the angle signal and pairs with `Signal::animate_to` for animated
  rotations. Layout-stable. Use for chevrons, dial controls, rotation
  feedback. See [crates/fern-widgets/src/animations/rotate.rs](crates/fern-widgets/src/animations/rotate.rs).
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
  See [crates/fern-widgets/src/animations/blur.rs](crates/fern-widgets/src/animations/blur.rs).
- `Spinner::new(size)` — circular-arc loading indicator backed by the
  shader-driven `AnimatedQuadKind::SpinnerArc` pipeline (~one uniform
  write + one `draw_indexed` per frame, no `paint()` re-runs). Honours
  `prefers-reduced-motion` with a static three-quarter arc fallback.
  See [crates/fern-widgets/src/spinner.rs](crates/fern-widgets/src/spinner.rs).
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
- `Easing` — `Linear`, `EaseIn`, `EaseOut`, `EaseInOut` (in fern-tokens)

**Files:** [crates/fern-tokens/src/motion.rs](crates/fern-tokens/src/motion.rs),
[crates/fern-core/src/animation.rs](crates/fern-core/src/animation.rs),
[crates/fern-core/src/animation_builder.rs](crates/fern-core/src/animation_builder.rs),
[crates/fern-core/src/signal.rs](crates/fern-core/src/signal.rs),
[crates/fern-widgets/src/animations/](crates/fern-widgets/src/animations/) (all wrapper widgets).
Visual showcase: `cargo run -p animations-kit`.

## Event System (V2 Attached Handlers)

- **Preview pass** (root → strict ancestors of target) + **Bubble pass** (target → root)
- **Attached handlers** replace monolithic `event()`: `.on_tap()`, `.on_hover()`, `.on_key()`, `.on_key_preview()`, `.on_focus()`, `.on_scroll()`, `.on_pointer_event()`, `.on_access_action()`
- `.on_key_preview()` runs during the preview pass for KeyDown/KeyUp/IME — strict ancestors only, so the focused widget never sees its own preview. Use for ancestors that need to claim chords before a focused inner widget consumes them (a messenger composer claiming Enter, a Dialog claiming Esc, a ListView claiming arrow keys). Shortcuts still resolve first; `on_key_preview` cannot override a registered shortcut.
- `.focus_within(Signal<bool>)` / `.hover_within(Signal<bool>)` — framework writes `true` whenever the focused / hovered widget is a **strict descendant** of this node. Drives unified halos around composite widgets (SpinBox, SplitButton, ComboBox, messenger composer panel, GroupBox sections). Strict-ancestors-only: a widget's own focus/hover does **not** flip its own `_within` signal — combine with `on_focus`/`on_hover` if you need both.
- Handlers attached via `WidgetBuilder` trait (blanket impl) or `HandlerSet` in `build()`
- Framework auto-wires gesture recognizers from handler types (on_tap → TapRecognizer)
- `EventHandlers` struct on `WidgetNode` stores closures, dispatched by framework
- `.focusable(true)`, `.cursor(CursorIcon::Pointer)` — framework-level properties on node
- Cross-widget behavior: `ctx.send_intent(MyIntent::X)` inside handlers; ancestor `Action`s consume it (see "Actions, Intents & Shortcuts")
- **Tap-family callbacks take `&TapEvent` (`{ position, button, modifiers }`)** — `on_tap` / `on_double_tap` / `on_triple_tap` / `on_long_press`. Default acceptance is `ButtonMask::PRIMARY` only (right-click never activates `on_tap` by accident). Widen with `.accept_tap_buttons(...)` / `accept_double_tap_buttons(...)` / `accept_triple_tap_buttons(...)` / `accept_long_press_buttons(...)`. `PointerButton` covers `Primary | Secondary | Middle | Back | Forward`. Multi-tap recognizers require button-match across the whole sequence; mixed-button sequences fail rather than spuriously firing.

## Accessibility Overrides

Builder-level `.access_*` methods on `WidgetBuilder` (and `WidgetWithHandlers`) let an app author augment, replace, or annotate any widget's accessibility info from the outside — analogous to SwiftUI's `.accessibility*` modifiers and Flutter's `Semantics(...)`. Overrides ride the same `HandlerSet → WidgetNode` plumbing as `cursor` / `clips_children` / `focus_within`, then apply from the accessibility tree walker after the inner widget's `accessibility(builder)` runs.

```rust
use accesskit::{Action, Role, Live, HasPopup};

Button::new(tr!("save_icon"))
    .access_label(tr!("save"))                  // replace widget label
    .access_description(tr!("save_explanation")) // long-form context
    .access_role(Role::Button)
    .access_shortcut_id("app.save")             // tracks user rebinds via ShortcutRegistry
    .access_action(Action::ShowContextMenu, |ctx| ctx.send_intent(AppIntent::Menu))
    .access_custom_action(tr!("publish_now"), |ctx| ctx.send_intent(AppIntent::Publish));

// Subtree control:
my_card.access_merge_subtree();        // collapse card into one AT element
animated_logo.access_exclude_subtree();// hide all descendants from AT

// Status region:
toast_panel.access_live(Live::Polite);

// Cross-widget relationships:
combo_button.access_controls(listbox_id);
field.access_described_by(error_message_id);
```

**Naming and i18n.** All user-visible-string methods (`access_label`, `access_description`, `access_hint`, `access_value`, `access_custom_action`) accept `impl Into<String>`. With the `i18n` feature, `fern_i18n::LocalizedString` (the type produced by `tr!`) implements `From<LocalizedString> for String`, so `.access_label(tr!("save"))` resolves and stores the translated literal. Each translated method has a `#[doc(hidden)]` `_literal` twin (`access_label_literal`, etc.) — the same grep marker as `Button::new_literal`/`tooltip_literal` for explicitly-untranslated call sites.

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

Full reference: [docs/accessibility-overrides.md](docs/accessibility-overrides.md). Implementation: [crates/fern-core/src/widget_builder.rs](crates/fern-core/src/widget_builder.rs) (`AccessibilityOverrides`, `AccessSubtreeMode`, `.access_*` methods); [crates/fern-core/src/widget_tree/accessibility_impl.rs](crates/fern-core/src/widget_tree/accessibility_impl.rs) (walker integration, `merge_descendants_into`).

## Actions, Intents & Shortcuts

Three-layer input-to-behavior pipeline. There is **no** `AppCommand`/`on_command` anymore — widgets fire `Intent`s, ancestor widgets register `Action`s keyed by intent name, and `Shortcut`s bind rebindable keystrokes to intent names.

```rust
use fern_ui::IntentKind;
use fern_ui::core::{Action, shortcut::{KeyStroke, Shortcut}};
use fern_ui::prelude::*;

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
        let btn = Button::new_literal("Save")
            .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save));
        vec![ctx.add(btn)]
    }
}
```

Key APIs:
- `Shortcut::new("id").primary(KeyStroke::ctrl(Key::S)).build()` — rebindable keystroke → intent name. `.on_activate(|ks, ctx| AppIntent::X(…))` for parametric payloads (chord-dependent data).
- `ctx.register_shortcut(shortcut)` (widget-scoped, default) / `ctx.register_shortcut_global(shortcut)` (app-wide).
- `Action::new("id").on_invoke(|intent, ctx| …)` — handler. Register with `ctx.register_action(action)`.
- `ctx.send_intent(AppIntent::X)` — fire from any handler. Blanket `impl<K: IntentKind> From<K> for Intent` lets you pass the enum variant directly.
- `#[derive(IntentKind)]` with `#[name = "..."]` on variants — typed DTO bridge. Works with unit, tuple, and struct variants (whole variant = payload).
- `ShortcutRegistry::version()` is a `Signal<u64>`; menu labels and tooltips use `MenuItem::for_shortcut("id")` / `TooltipContent::for_shortcut("id")` to re-render on rebinds.
- `ShortcutSettings::new()` — pre-built rebind UI widget.

Handler rule:  call `AppIntent::from_intent(intent)` **only** when you need typed fields. Unit intents react on name alone — this lets the same handler fire whether the intent came from a shortcut (name-only synthesized) or from `send_intent(AppIntent::X)` (typed payload).

Full reference: [docs/shortcut-intent-action.md](docs/shortcut-intent-action.md). Working demo: [examples/shortcuts_demo](examples/shortcuts_demo/src/main.rs).

## Settings & Persistence

Persistent, reactive user preferences via `fern-settings`. **In-memory is the source of truth** — `Signal<T>` and `*Model<T>` handles drive both UI and disk; the disk side is a debounced atomic projection (write-temp + rename, single shared I/O thread per process).

Three persistence shapes:

- **`SettingsStore`** — dotted-key K/V for **scalars** (numbers, strings, bools, arrays of those). `store.signal::<T>(key, default)` or `store.signal_for(&KEY)` returns a cached `Signal<T>`. Same key → same signal across call sites. Struct values rejected at registration with a clear error pointing to `SettingsFile<T>` (TOML serializes structs as tables, indistinguishable from nested key paths).
- **`SettingsFile<T>`** — typed single-struct persistence with `Versioned` + `Migrator<T>` migrations on raw `toml::Value` *before* deserialize. Corrupt files quarantine to `<path>.broken-<unix_ts>` and fall back to `T::default()`.
- **`PersistedListModel<T>` / `PersistedTreeModel<T>`** — bridges from `ListModel<T>` / `TreeModel<T>` to `SettingsFile<*File<T>>`. Every mutation re-serializes the whole collection (debounced) — fine for <1k items, use SQLite beyond that.

Built-in services on top:

- **`MruList<T: MruEntry>`** — generic dedupe + pin + LRU-cap recents. Apps define their own item type implementing `MruEntry { type Key; fn key(); fn is_pinned()/set_pinned(); fn touch(); }`. The framework knows nothing about projects / files / palettes.
- **`WindowStateService`** — per-`label` window geometry. **Auto-restored and auto-saved by `fern-app`'s window manager** when a `WindowConfig` carries `id(...)` and the bundle has `with_window_state(true)`. No widget-side wiring.

```rust
use fern_ui::settings::{AppPaths, MruEntry, MruList, SettingsBundle, SettingsExt, SettingsKey};

const FONT_SIZE: SettingsKey<f32> = SettingsKey::new("editor.font_size", || 14.0);

fn main() {
    let paths = AppPaths::new("com", "FernTech", "FernUI").expect("config dir");
    let recents: MruList<RecentProject> = MruList::open(&paths, "recent_projects", 10).unwrap();

    FernAppBuilder::new()
        .app_paths(paths)                                         // OR .application(qual, org, app)
        .settings(SettingsBundle::new().with_window_state(true))  // store + window state
        .app_state(recents)                                       // app-typed MRU
        .initial_window(
            WindowConfig::new()
                .id("main")                                       // <- enables auto save/restore
                .title("FernUI")
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
- `SettingsExt` accessors (`use fern_settings::SettingsExt;`): `ctx.settings()`, `ctx.window_state()`, `ctx.mru::<T>()`. `try_*` variants return `Option`.
- Tests use `AppPaths::for_testing(tmp.path())` and `Duration::ZERO` for the debounce — never the real `ProjectDirs`.

Full reference: [docs/settings.md](docs/settings.md). Working demo: [examples/recent_projects](examples/recent_projects/src/main.rs).

## Locale-aware Formatting

Numbers, dates, and times that change with the user's locale flow through one ICU4X-backed layer in `fern-i18n`. Two consumer paths share the same cache, so a UI mixing translated and untranslated displays stays internally consistent on `,` vs `.`, grouping, currency suffixes, etc.

**Bundle-side path — `tr!` / `tr_signal!` messages.** `manager::configure_bundle` installs a `set_formatter` callback on every Fluent bundle and registers a custom `DATETIME()` function. So `{ NUMBER($v) }` and `{ DATETIME($ts, dateStyle: "long") }` inside `.ftl` messages render correctly across locales — no app-side wiring. Pass numeric args as ordinary `f64`/`i32`/etc.; pass datetimes as [`FernDateTime`](crates/fern-i18n/src/format.rs):

```rust
let dt: jiff::civil::DateTime = ...;
tr!(last_saved(ts = FernDateTime::from(dt)))
```

**Signal-side path — non-translated displays.** `NumberFormatter` and `FernDateTimeFormatter` produce a `Signal<String>` from a value (static or `Signal<T>`-bound) plus the i18n manager's locale signal. Re-renders on either change. Used for SpinBox values, TableView cells, status bars, numeric inputs — anywhere the value isn't part of a translated sentence:

```rust
let display = NumberFormatter::new()
    .currency("USD")
    .fraction_digits(2, 2)
    .format(price_signal);  // Signal<f64> → Signal<String>

let when = FernDateTimeFormatter::new()
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

**Files:** [crates/fern-i18n/src/format.rs](crates/fern-i18n/src/format.rs) (Memoizable types, ICU bridge, `FernDateTime`, public formatter types, bundle callback, `DATETIME` function); [crates/fern-i18n/src/manager.rs](crates/fern-i18n/src/manager.rs) `configure_bundle` (one helper, three call sites at the `FluentBundle::new` boundary); [crates/fern-i18n-macros/src/lib.rs](crates/fern-i18n-macros/src/lib.rs) (`tr_signal!` / `tr_signal_widget!` lowering — branches off `tr_impl(input, kind, signal)`); [crates/fern-i18n/tests/format_integration.rs](crates/fern-i18n/tests/format_integration.rs) (end-to-end tests for both paths plus the macro).

## Three-Tier Rendering

| Tier | Type | Used For |
|------|------|----------|
| 1 | `DecorationRect` | Backgrounds, borders, focus ring |
| 2 | `ShapeQuad` (SDF) | Rounded rects, circles, gradients |
| 3 | `PathEntry` (tiny-skia) | Arbitrary paths, SVG icons |
| Text | `GlyphQuad` | Glyph atlas text |

Three wgpu pipelines: `rect_pipeline`, `sdf_pipeline`, `quad_pipeline`.

## Theming

`Theme` struct with five token groups (`ColorTokens`, `LayoutTokens`, `TypographyTokens`, `ShapeTokens`, `MotionTokens`) plus `ComponentStyles`. Built-in presets: `Theme::light_default()`, `Theme::dark_default()`. Runtime switching via `ctx.set_theme(new)` or `tree.set_theme(new)`.

**Theme is reactive.** `set_theme` updates an internal `Signal<Theme>` and dirty-marks every node — no rebuild. Focus, scroll offsets, and all interaction state survive theme changes.

Three knobs that matter in widget code:

1. **Roles** — `TextRole`, `SurfaceRole`, `BorderRole`, `TextStyleRole` in `fern-tokens`. Name *what* a value represents, not which literal. Resolved against the current theme at paint/layout.
2. **`ColorProp`** — unified color-builder input: `Static(Color) | Bound(Signal<Color>) | TextRole | SurfaceRole | BorderRole | DynamicTextRole(Signal<TextRole>) | DynamicSurfaceRole(..) | DynamicBorderRole(..)`. Widget builders accept `impl Into<ColorProp>`.
3. **`TextStyleProp`** — same idea for typography: `Static(TextStyle) | Role(TextStyleRole)`.

Defaults: `TextWidget::new("...")` is `TextRole::Primary` + `TextStyleRole::Body`; `Panel::new()` is `SurfaceRole::Main` + `BorderRole::Default` when unset.

Interaction-driven colors use the `Signal<Role>` pattern — no `theme_signal` zip:

```rust
let bg_role = interaction.map(|s| match s {
    InteractionState::Hovered => SurfaceRole::Hover,
    InteractionState::Pressed => SurfaceRole::Pressed,
    _ => SurfaceRole::Transparent,
});
RectWidget::new().background(bg_role)
```

`ctx.theme_signal()` / `ctx.locale_signal()` are still available for the cases no role covers (alpha blends, rich-text engine palette sync, layout-constant snapshots) — use them sparingly. Full reference: [docs/reactive-theme.md](../docs/reactive-theme.md).

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
- Window management (multi-window, modal dialogs, custom title bar — Wayland + macOS + Windows; X11 falls back to native decorations)
- GPU rendering (3 pipelines, glyph atlas, path atlas)
- All ~21 layout primitives (including Grid, Wrap, AspectRatio, Switcher, MasonryLayout, FormLayout)
- Accessibility (AccessKit integration at trait level + builder-level overrides: `.access_label`, `.access_description`, `.access_hidden`, `.access_role`, `.access_disabled`, `.access_controls`/`described_by`/`labelled_by`, `.access_live`, `.access_shortcut_id`/`access_shortcut_literal`, `.access_action`/`access_remove_action`/`access_custom_action`, `.access_exclude_subtree`/`access_merge_subtree`, `.access_customize` — see "Accessibility Overrides" above)
- Animation system (`Signal<f32>::animate_to`, easing, per-frame scheduler)
- Internationalization (fern-i18n + fern-i18n-macros: Fluent-rs, `tr!`/`tr_widget!`, locale resolution, file watcher, RTL direction signal). Locale-aware formatting via `NumberFormatter` / `FernDateTimeFormatter` (`Signal<T>` → `Signal<String>`) and `FernDateTime` (jiff wrapper for the `DATETIME()` Fluent function). The framework auto-installs a `set_formatter` callback + custom `DATETIME` function on every bundle, so `{ NUMBER(...) }` / `{ DATETIME(...) }` inside `.ftl` messages render correctly across locales. Reactive `tr_signal!` / `tr_signal_widget!` macros bind `Signal<T>` arguments inside translated sentences and re-render on any-arg / locale / hot-reload change. Backed by ICU4X (`icu_decimal` / `icu_datetime` / `icu_calendar`); see "Locale-aware Formatting" below.
- `fern!` DSL (fern-ui-macros: block-structured widget-tree syntax, desugars to V2 builder calls — see `docs/fern-macro-reference.md`)
- Actions / Intents / Shortcuts (`Action`, `Intent`, `Shortcut`, `ShortcutRegistry`, `#[derive(IntentKind)]`, `ShortcutSettings` — rebindable keystrokes, typed-enum DTO bridge, source → root dispatch; see `docs/shortcut-intent-action.md`)
- Ancestor key intercept (`.on_key_preview`) and subtree state signals (`.focus_within(Signal<bool>)` / `.hover_within(Signal<bool>)`) — strict-ancestors-only, see Event System above
- Reactive data models (fern-data: `ListModel`, `TreeModel`, `TreeSlice`, `SelectionModel`, `SortFilterListModel<T>`, `SortFilterTreeModel<T>` with `TreeFilterMode` `HideNonMatching`/`KeepAncestors`/`KeepDescendants`)
- Settings & persistence (fern-settings: `SettingsStore` dotted-key Signal<T> K/V, `SettingsFile<T>` with versioned migrations, `PersistedListModel`/`PersistedTreeModel`, generic `MruList<T: MruEntry>`, `WindowStateService` with framework-driven auto save/restore + monitor-aware sanitize on restore; see `docs/settings.md`)
- Native file dialogs (fern-platform/file_dialog: `FileDialogBackend` trait + `FileDialogHandle` registered in app-state, `FileDialogRequest` builder for open / open-multi / pick-folder / save, `FileDialogResult`, `MemoryFileDialog` test backend, `RfdAsyncBackend` real implementation behind the `rfd-backend` feature using rfd 0.15 + xdg-portal + async-std; `EventContextFileDialogExt` extension trait adds `ctx.pick_file(...)`, `ctx.pick_files(...)`, `ctx.pick_folder(...)`, `ctx.save_file(...)`. Result delivery: backend posts `FileDialogEventPayload` through `AppEventPoster::post_external` → fern-app's `AppEvent::External` arm downcasts and routes to the originating window's tree → `FileDialogHandle::deliver` pops the callback and invokes it on the main thread with a fresh `EventContext`. macOS NSOpenPanel runs on the AppKit main run loop internally; the future drives the wakeup machinery from an async-std worker. Pending callbacks are tagged with the originating `FernWindowId` and purged via `WindowManager::close_window` when the window closes — no use-after-free of widget state. Apps wire up with `FernAppBuilder::install_file_dialog()` (or `.app_state(FileDialogHandle::new(my_backend))` for a custom backend). Demo: `examples/file_dialogs`.)
- Debug inspector (fern-inspector: in-app introspection panel, debug builds only, gated by `cfg(debug_assertions)`; F12 toggles a bottom panel with 9 tabs (Tree, Properties, Accessibility, Theme, Locale, Focus, Shortcuts, Overlays, Models); bounds-overlay visualization (Off/Selection/All) with cursor-following type+size tooltip and Padding/StackGap tinted bands; picker tool with multi-window subtree exclusion; theme JSON Export/Import; resizable panel with persisted height; tree filter input + auto-scroll-into-view; Properties Copy button + right-click `Copy value` context menu + Debug repr row; Models tab with click-to-select per row; panel-scoped Ctrl+P/Ctrl+B/Ctrl+Tab/Ctrl+Shift+Tab/Esc keyboard shortcuts; persistence via `__fern_inspector.*` settings keys when `SettingsStore` is wired. Apps opt in with `FernAppBuilder.install_inspector_in_debug()` (no-op in release) — the extension trait is re-exported from `fern_ui::prelude::*` behind the umbrella's default-on `inspector` feature, so no separate `fern-inspector` dep is needed. See `docs/inspector.md`. Data models opt into the Models tab via `ListModel::debug_named("…")` / `TreeModel::debug_named` / `SelectionModel::debug_named`.)
- Controls: Button, Checkbox, RadioButton, Toggle, Slider, ComboBox, SegmentedControl, ProgressBar, Link, Badge, SpinBox, SplitButton
- Containers: Panel, Card, Accordion, ToolBox, ScrollArea, ScrollBar, Tooltip, SplitView, **TabWidget / TabBar / Tabs** (data-source-driven `TabBar<T>` with `TabDelegate<T>` for per-tab label/icon/leading/trailing/context-menu/closable/pinned/enabled/tooltip; `TabSizing::Shared` (uniform extent) vs `Independent` (per-content); horizontal scroll with leading + trailing arrow buttons + mouse-wheel-to-horizontal mapping + Shift+wheel; "show all tabs" overflow `Popover` dropdown; close button on closable tabs + middle-click close + selection adjustment on close; drag-to-reorder with insertion-line indicator + edge auto-scroll; pinned tab strip (icon-only, fixed-width, no close button — Firefox/Chrome convention) at the leading edge separate from the scrollable region; locale-reactive labels + AT names; legacy `TabWidget::new(...).tab(...)` shim still supported. Generic `Tabs<T>` composes `TabBar<T>` above a `Switcher` driven by a content delegate.), Dialog, Popover, Snackbar, Wizard, Breadcrumb, GroupBox, MessageBox
- Menus: MenuBar, MenuList, MenuItem, MenuContext (context menu)
- Chrome: Toolbar, StatusBar, TitleBar, GroupHeader
- Data-driven: ListView, TreeView, Repeater, **TableView** (multi-column, virtualized, sort/filter via `SortFilterListModel`, drag-resize + drag-reorder of columns, pinned Leading/Trailing, cell-level + row-level selection, full keyboard nav with focus ring, edit hooks via `editing_cell_signal` + `on_cell_edit_request`, row drag-drop reorder, `Role::Table > Role::Row > Role::Cell` accessibility), **TreeTable** (hierarchical multi-column, twist-arrow indent, ArrowLeft/Right collapse/expand, `Role::TreeGrid` with per-row `set_level`/`set_expanded`)
- Text: TextInput (styled single-line), rich text viewer; `RichTextEditor::editor` / `read_only` accept `.min_lines(n)` / `.max_lines(n)` for intrinsic-mode sizing (greedy by default; intrinsic when either knob is set, clamping `content_height` to `[min, max] × default_line_height` — the messenger-composer pattern)

### Partial / In Progress

- Text rendering (depends on external text-typeset)
- Rich text editor widget (rich_text/ module: state, paint, clipboard, keyboard, mouse, hit_test, context_menu, frame_loop, policy, image_cache)

### Not Started

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
- Data models: [crates/fern-data/src/](crates/fern-data/src/) (`list_model.rs`, `tree_model.rs`, `selection_model.rs`, `sort_filter_list_model.rs`, `sort_filter_tree_model.rs`)
- TableView: [crates/fern-widgets/src/table_view.rs](crates/fern-widgets/src/table_view.rs) + submodules at [crates/fern-widgets/src/table_view/](crates/fern-widgets/src/table_view/) (`column.rs`, `selection.rs`, `a11y.rs`, `body.rs`, `header.rs`, `keyboard.rs`, `layout.rs`, `row_navigator.rs`, `tests.rs`). Demo: [examples/data_grid/src/main.rs](examples/data_grid/src/main.rs)
- TreeTable: [crates/fern-widgets/src/tree_table.rs](crates/fern-widgets/src/tree_table.rs) (reuses table_view's column/header/keyboard modules; adds `TreeNavigator` + `TwistArrow`). Demo: [examples/tree_table/src/main.rs](examples/tree_table/src/main.rs)
- i18n runtime: [crates/fern-i18n/src/manager.rs](crates/fern-i18n/src/manager.rs), [crates/fern-i18n/src/localized_string.rs](crates/fern-i18n/src/localized_string.rs)
- i18n locale-aware formatting: [crates/fern-i18n/src/format.rs](crates/fern-i18n/src/format.rs) (Memoizable types, ICU bridge, `FernDateTime` + `FluentType` impl, public `NumberFormatter` / `FernDateTimeFormatter`, bundle `set_formatter` callback, `DATETIME()` Fluent function). Bundle wiring: `configure_bundle` helper in [manager.rs](crates/fern-i18n/src/manager.rs). Tests: [crates/fern-i18n/tests/format_integration.rs](crates/fern-i18n/tests/format_integration.rs)
- i18n macros: [crates/fern-i18n-macros/src/lib.rs](crates/fern-i18n-macros/src/lib.rs) (`tr!`, `tr_widget!`, `tr_signal!`, `tr_signal_widget!`)
- fern! DSL macro: [crates/fern-ui-macros/src/](crates/fern-ui-macros/src/) (parse → IR → lower). Trybuild fixtures at [crates/fern-ui/tests/fern_ui/pass/](crates/fern-ui/tests/fern_ui/pass/)
- fern! reference: [docs/fern-macro-reference.md](docs/fern-macro-reference.md) (user-facing), [docs/fern-language-spec-v3.md](docs/fern-language-spec-v3.md) (design spec)
- Actions/Intents/Shortcuts: [crates/fern-core/src/action.rs](crates/fern-core/src/action.rs), [intent.rs](crates/fern-core/src/intent.rs), [shortcut.rs](crates/fern-core/src/shortcut.rs). `IntentKind` derive: [crates/fern-ui-macros/src/intent_kind.rs](crates/fern-ui-macros/src/intent_kind.rs). Settings widget: [crates/fern-widgets/src/shortcut_settings.rs](crates/fern-widgets/src/shortcut_settings.rs). Reference doc: [docs/shortcut-intent-action.md](docs/shortcut-intent-action.md)
- Settings/persistence: [crates/fern-settings/src/](crates/fern-settings/src/) (`store.rs`, `file.rs`, `mru.rs`, `window_state.rs`, `bundle.rs`, `ext.rs`, `migration.rs`, `flush.rs`, `path.rs`). Auto window save/restore wiring: [crates/fern-app/src/window_persist.rs](crates/fern-app/src/window_persist.rs). Reference doc: [docs/settings.md](docs/settings.md). Demo: [examples/recent_projects/src/main.rs](examples/recent_projects/src/main.rs)
- Canvas API: `crates/fern-canvas/src/canvas.rs`
- Renderer: `crates/fern-render/src/renderer.rs`
- App builder: `crates/fern-app/src/app.rs`
- Umbrella exports: `crates/fern-ui/src/lib.rs`
- Resources: `crates/fern-resources/src/lib.rs`
- Previewer infrastructure: `crates/fern-preview/src/lib.rs` (trait + registry), `crates/fern-preview-ui/src/lib.rs` (GUI library)
- Drag-and-drop: `crates/fern-core/src/drag_payload.rs`, `crates/fern-core/src/drag_state.rs`
- Clipboard: `crates/fern-platform/src/clipboard.rs`
- File dialogs: [crates/fern-platform/src/file_dialog.rs](crates/fern-platform/src/file_dialog.rs) (trait, handle, request, result, payload, mock, `RfdAsyncBackend`, `EventContextFileDialogExt`). Wiring: `WindowOps::current_parent_handle` in [crates/fern-core/src/window/ops.rs](crates/fern-core/src/window/ops.rs); `EventContext::parent_window_handle` + `EventContext::poster` in [crates/fern-core/src/widget.rs](crates/fern-core/src/widget.rs); `WidgetTree::run_with_event_context` in [crates/fern-core/src/widget_tree.rs](crates/fern-core/src/widget_tree.rs); `FernAppHandler::try_route_file_dialog_payload` and the `AppEvent::External` downcast arm in [crates/fern-app/src/app.rs](crates/fern-app/src/app.rs); window-close purge hook in [crates/fern-app/src/window_manager.rs](crates/fern-app/src/window_manager.rs)'s `close_window`. Demo: [examples/file_dialogs/src/main.rs](examples/file_dialogs/src/main.rs).
- Text input: `crates/fern-widgets/src/text_input.rs`, `crates/fern-widgets/src/primitives/text_input_field.rs`
- Rich text: `crates/fern-widgets/src/rich_text/` (state, paint, clipboard, keyboard, mouse, hit_test, context_menu, frame_loop, policy, image_cache)
- New widgets: `crates/fern-widgets/src/spin_box.rs`, `crates/fern-widgets/src/split_button.rs`, `crates/fern-widgets/src/group_box.rs`, `crates/fern-widgets/src/group_header.rs`, `crates/fern-widgets/src/message_box.rs`, `crates/fern-widgets/src/tool_box.rs`, `crates/fern-widgets/src/keystroke_format.rs`, `crates/fern-widgets/src/privacy_settings.rs`
- New primitives: `crates/fern-widgets/src/primitives/masonry.rs`, `crates/fern-widgets/src/primitives/form_layout.rs`, `crates/fern-widgets/src/primitives/image_widget.rs`
- OS integration: `crates/fern-platform/src/os_theme.rs`, `crates/fern-platform/src/accessibility_prefs.rs`
- Title bar hosts: `crates/fern-platform/src/title_bar_host/` (wayland.rs, x11.rs, windows.rs, macos.rs)

## Widget Construction Patterns

```rust
// Inline children (most common) — .child() accepts impl Widget + 'static
VStack::new().spacing(10.0)
    .child(TextWidget::new("Hello").style(TextStyleRole::BodyBold))
    .child(Button::new("Click").on_activate_fn(|ctx| ctx.send_intent(MyIntent::DoThing)))

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
        // No theme snapshot needed — roles resolve at paint/layout time.
        let root = ctx.add(VStack::new()
            .child(TextWidget::new("Hello").style(TextStyleRole::BodyBold))
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
fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .initial_window(
            WindowConfig::new()
                .title("My App")
                .size(800, 600)
                .root(|tree, _state| tree.add(MyRootWidget::new())),
        )
        .run();
}
```

Every window — initial or runtime-opened — is described by a `WindowConfig`. There is no `.window_title` / `.window_size` / `.root` on `FernAppBuilder` directly; secondary windows are opened from handler code via `ctx.open_window(WindowConfig::new()...)`. See [docs/multi-window.md](docs/multi-window.md) for the full multi-window API.

App-wide behavior lives inside the root widget: register `Shortcut`s, declare `Action`s keyed by intent name, and react to them via handlers. See "Actions, Intents & Shortcuts" above and [docs/shortcut-intent-action.md](docs/shortcut-intent-action.md) for the full pattern. Ambient mutations are available on `EventContext` from any handler: `ctx.set_theme(...)`, `ctx.set_locale(...)`, `ctx.close_window()`.

If the app uses persistence, chain `.app_paths(...)` (or `.application(qualifier, organization, application)`) and `.settings(SettingsBundle::new()...)` before `.initial_window(...)`. App-typed handles (`MruList<T>`, `SettingsFile<T>`) register via `.app_state(handle.clone())`. See "Settings & Persistence" above and [docs/settings.md](docs/settings.md).

## Architecture Reference

Full architecture document: `../fern-ui-perso/fern-ui-architecture.md` (28 sections, covers layout model, scrolling, widget state, reactivity, overlays, DnD, data sources, Canvas API, rendering pipeline, theming, threading, accessibility, window management, testability, i18n)

Additional documentation: [docs/accessibility-overrides.md](docs/accessibility-overrides.md), [docs/settings.md](docs/settings.md), [docs/drag-and-drop.md](docs/drag-and-drop.md), [docs/title-bar.md](docs/title-bar.md), [docs/multi-window.md](docs/multi-window.md), [docs/idle-and-animation.md](docs/idle-and-animation.md), [docs/telemetry.md](docs/telemetry.md), [docs/table-view.md](docs/table-view.md), [docs/inspector.md](docs/inspector.md), [docs/plans/previewer-plan.md](docs/plans/previewer-plan.md), [docs/plans/settings-plan.md](docs/plans/settings-plan.md), [docs/plans/telemetry-plan.md](docs/plans/telemetry-plan.md), [docs/plans/fern-collector-plan.md](docs/plans/fern-collector-plan.md)
