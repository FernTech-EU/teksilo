# FernUI Architecture Document

**Version:** 0.2 — V2 refresh  
**Date:** April 19, 2026  
**Author:** Cyril Jacquet, with architectural design by Claude (Anthropic)  
**Status:** Living reference — architecture stabilized, Milestones 1–9 delivered

> **Document scope.** This document covers the *why* of FernUI's architecture.
> Where the *how* is already documented elsewhere — in a dedicated doc or in
> an up-to-date source file — the relevant section here summarizes the
> rationale and points at the authoritative reference rather than duplicating
> code. Readers learning the framework should read this doc for mental model
> and skim the referenced docs for API detail.

> **Status note — V1 surfaces retired.** The original (V1) shapes of several
> subsystems remain described below for historical rationale, but the public
> API they describe is gone. Where you see a V1 section, a short lead-in
> points at the V2 replacement:
>
> - **§5 Widget Extensibility** — the `CompositeWidget` / `Widget` split is
>   replaced by the unified `Widget` trait. See **§29 V2 Widget Authoring
>   Model**.
> - **§7 Reactivity Model** — `State<T>` / `DerivedState<T>` / `Reactive<T>`
>   are gone. `Signal<T>` and `Prop<T>` are the only reactive primitives.
>   See **§29.4**.
> - **§9.1 Input Event Routing** + **§9.2 Typed Command Flow** — the monolithic
>   `event()` method and `AppCommand` trait are replaced by attached handlers
>   and the Shortcut / Intent / Action pipeline. See
>   [`events-and-gestures.md`](events-and-gestures.md) and
>   [`shortcut-intent-action.md`](shortcut-intent-action.md).
> - **§11 Keyboard Shortcuts** — `ShortcutMap<C: AppCommand>` replaced by
>   `ShortcutRegistry` with two-layer defaults + user overrides. See
>   [`shortcut-intent-action.md`](shortcut-intent-action.md).
> - **§26 Button — Reference Widget Design** — the V1 `CompositeWidget`
>   pattern; the actual Button code follows the V2 exemplar in §29.1. See
>   [`crates/fern-widgets/src/button.rs`](../crates/fern-widgets/src/button.rs).
>
> The *design rationale* in each V1 section survives unchanged — only the
> Rust API it lands on has moved. A reader interested in why FernUI unified
> its widget trait, why it kept preview+bubble dispatch but moved handler
> storage off the trait, or why it collapsed four reactivity types into one,
> will find the story in those sections plus the V2 references.

> **See also — dedicated docs in this directory:**
>
> - [`animation.md`](animation.md) — `Signal<f32>::animate_to`, scheduler, MotionTokens.
> - [`events-and-gestures.md`](events-and-gestures.md) — attached handlers, preview/bubble, recognizers, `EventContext`.
> - [`data-models.md`](data-models.md) — `ListModel`, `TreeModel`, `TreeSlice`, `SelectionModel`, MVVM flow.
> - [`reactive-theme.md`](reactive-theme.md) — `Signal<Theme>`, role enums, `ColorProp`, `TextStyleProp`.
> - [`shortcut-intent-action.md`](shortcut-intent-action.md) — rebindable keystrokes, typed intents, widget-owned actions.
> - [`fern-macro-reference.md`](fern-macro-reference.md) — `fern!` DSL user-facing reference.
> - [`fern-language-spec-v3.md`](fern-language-spec-v3.md) — `fern!` DSL grammar and desugaring spec.
> - [`icons-and-resources.md`](icons-and-resources.md) — `res!()` macro, icon formats, asset embedding.
> - [`fern-ui-milestones.md`](fern-ui-milestones.md) — what's delivered, what's next.

---

## 1. Vision and Positioning

FernUI is a pure-Rust GUI framework for serious desktop applications — the kind of software where a user sits down for hours at a time and reaches for the keyboard first. A writing tool for novelists, an IDE, a dispatch console, a course manager for a taxi company's driver training. FernUI is not a general-purpose widget toolkit competing with egui or iced for weekend prototypes; it is infrastructure for professional desktop software that needs native look and feel, full keyboard and screen-reader accessibility, and a rich text surface built from the ground up.

FernUI's thesis rests on three pillars. First, accessibility is a structural requirement, not an afterthought — AccessKit is integrated at the trait level, not bolted on. Second, rich text is a solved problem — the text-document and text-typeset crates provide a complete document model and typesetting engine that no other Rust GUI framework can match. Third, the framework is designed to be consumed by applications with structured architecture (Clean Architecture, MVVM), providing a typed Shortcut / Intent / Action pipeline and reactive data-model crate (`fern-data`) rather than leaving application structure as an exercise for the developer.

### 1.1 Relationship to structured application architectures

FernUI is the outermost layer of an application — the "Frameworks & UI" ring in Clean Architecture's concentric circles. It has no dependency on any particular application framework. A Qleany-structured application is one supported integration path and was the stress test that shaped several of FernUI's architectural choices (typed intents for command flow, view-models over raw entities, data sources for paged external collections), but nothing in FernUI *requires* Qleany. An application that uses diesel + hand-rolled entities, or one that streams events off Kafka and holds view-state in plain structs, fits the same shape.

The integration surface is the typed intent system (FernUI widgets emit application-defined intent variants that ancestor `Action`s consume — see [`shortcut-intent-action.md`](shortcut-intent-action.md)) and the reactive data models in `fern-data` (application-written view-models hold entity collections as `ListModel<EntityVM>` / `TreeModel<EntityVM>` that widgets bind to — see [`data-models.md`](data-models.md)).

The decision not to structure FernUI's internals using a Clean-Architecture split was deliberate. Layout, rendering, and event dispatch are hot paths with fundamentally different performance characteristics from transactional domain operations; the useful seams fall in different places. FernUI instead splits into focused crates (see §25) each with a single concern, which is the right decomposition for a framework whose internal churn pattern is "rendering backend changes independently of widget tree changes independently of the data model layer."

### 1.2 Reuse Strategy

FernUI builds on established crates rather than reinventing solved problems.

**Windowing:** winit provides cross-platform window creation, input handling, and HiDPI support. It is battle-tested and has an AccessKit adapter.

**GPU rendering:** wgpu provides the GPU abstraction layer. FernUI's rendering contract (textured quads, colored rectangles, SDF shapes) is simple enough that wgpu's API is more than sufficient.

**Text:** text-document provides the rich text document model (blocks, fragments, tables, lists, cursors, undo/redo). text-typeset provides the typesetting engine (OpenType shaping via rustybuzz, rasterization via swash, atlas packing via etagere, unicode-linebreak, unicode-bidi). These crates produce GPU-ready glyph quads — the same rendering contract FernUI uses for all visual output.

**Accessibility:** AccessKit provides cross-platform accessibility infrastructure. FernUI pushes an AccessKit tree that platform adapters translate into native accessibility APIs (NSAccessibility on macOS, UI Automation on Windows, AT-SPI on Linux).

**Internationalization:** fluent-rs (Mozilla's Project Fluent) provides locale-aware string resolution with support for plurals, gender, and complex grammar. FernUI wraps it in a `tr!` macro.

**CPU rasterization:** tiny-skia handles Tier 3 path rasterization for arbitrary shapes that cannot be rendered with SDF shaders.

---

## 2. Layout Model

FernUI uses a SwiftUI-style layout negotiation protocol. Layout is a two-phase conversation between parent and child: the parent proposes a size, the child responds with the size it actually wants, and the parent places the child at a specific position.

The `Widget` trait expresses this as two methods:

```rust
trait Widget {
    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size;
    fn place_children(&self, bounds: Rect, proposal: SizeProposal,
                      children: &mut [WidgetPlacement], ctx: &LayoutContext);
}
```

Each layout container (VStack, HStack, ZStack, Grid) implements these two methods differently. No global constraint solver, no flexbox algorithm — just recursive negotiation. This composes naturally: a custom widget is a layout container. Leading/Trailing semantics (rather than Left/Right) ensure automatic RTL mirroring when the locale's `LayoutDirection` is right-to-left.

Layout operates entirely in logical pixels. The scale factor is applied at the rendering boundary, not during layout.

### 2.1 Alignment

The layout negotiation protocol determines how big each widget is, but not where it sits within the allocated space when it is smaller than the space offered. Alignment is the mechanism that controls this positioning.

Alignment is a two-axis value composed from `HAlignment` (horizontal: `Leading`, `Center`, `Trailing`) and `VAlignment` (vertical: `Top`, `Center`, `Bottom`). `Leading` and `Trailing` resolve to left or right depending on the environment's `LayoutDirection`, consistent with all directional properties in FernUI. The combined `Alignment` struct provides convenience constants for the nine common combinations (`center`, `top_leading`, `center_trailing`, etc.).

The `Alignment`, `HAlignment`, and `VAlignment` types are pure data, defined in `fern-tokens`.

### 2.2 Container-Level Alignment

Each layout container accepts an alignment parameter that controls how children are positioned on the cross axis — the axis perpendicular to the container's primary layout direction.

`HStack` lays out children horizontally. Its alignment parameter controls the vertical position of each child within the row. The default is `VAlignment::Center` (vertically centered), which matches the most common expectation. `VStack` lays out children vertically. Its alignment parameter controls the horizontal position of each child. The default is `HAlignment::Leading`. `ZStack` overlays children on top of each other. Its alignment parameter is a full `Alignment` (both axes), defaulting to `Alignment::center`.

The container's `place_children` implementation uses the alignment value to compute each child's position within the allocated bounds. For example, in a VStack with `HAlignment::Center`, each child's x position is `bounds.x + (bounds.width - child.width) / 2.0`. With `HAlignment::Trailing`, it is `bounds.x + bounds.width - child.width`.

### 2.3 Per-Child Alignment Override

Container-level alignment applies to all children uniformly. When one child needs a different alignment than the rest, a per-child alignment override is specified via an `.align()` modifier on the individual widget. The parent container checks each child for an alignment override before falling back to the container's default. The override is stored as an optional property on the widget's arena node, read by the parent during `place_children`.

### 2.4 Spacer

A `Spacer` is a layout utility widget whose `size_that_fits` claims all available space on the container's primary axis. Placing a Spacer before a widget in an HStack pushes the widget to the trailing edge. Placing Spacers on both sides centers the widget. Placing a Spacer after pushes the widget to the leading edge. This is the standard SwiftUI idiom for controlling position without explicit alignment parameters.

### 2.5 Expand, FixedSize, and Size Constraints

Alignment only matters when a widget is smaller than the space available. Several layout modifier widgets control how much space a widget claims.

`Expand` tells the layout system that a widget should claim all available space on one or both axes. An expanded widget's content is positioned within the expanded bounds according to a `content_alignment` parameter. `expand_horizontal()` and `expand_vertical()` expand on a single axis.

`FixedSize` prevents a widget from expanding beyond its natural size, even when the parent offers more space. This is useful for widgets that should not stretch (an icon inside an HStack where other children expand).

`MinSize` enforces a minimum dimension. The widget's `size_that_fits` response is clamped to be at least the specified minimum. This is how buttons enforce the minimum touch target size — the button's composed subtree includes a `MinSize::new(48.0, 48.0)` wrapper rather than overriding sizing on the composite.

`MaxSize` enforces a maximum dimension. The widget's `size_that_fits` response is clamped to be at most the specified maximum. Useful for constraining content width (a text editor that should not exceed 600 pixels wide).

`Center` is a convenience wrapper equivalent to a ZStack with `Alignment::center` — it centers its single child within the available space.

All of these are layout utility widgets in `fern-widgets`. They implement the `Widget` trait's `size_that_fits` and `place_children` methods, wrapping a single child. They require no special framework support — they are ordinary widgets that compose naturally with the layout negotiation protocol.

### 2.6 Dynamic Sizing and Binding Levels

Some property changes affect only a widget's visual appearance (a color change). Others affect the widget's size (a text change, a constraint change). The binding system must distinguish these two cases because they trigger different dirty-tracking responses.

**Repaint-level bindings** (`bind_color`, `bind_background`, `bind_border_color`) mark the widget for repaint only when the bound state changes. The layout pass is skipped — the widget's position and size are unchanged. This is the fast path, used for interaction-driven visual state changes (hover color, pressed color, enabled/disabled appearance).

**Relayout-level bindings** (`bind_text`, `bind_width`, `bind_height`, `bind_min_width`, `bind_max_height`) mark the widget for relayout when the bound state changes. The layout pass reruns on the affected subtree, and the dirty flag propagates upward to ancestors because a child's size change may affect its parent's size, which may affect the grandparent's size, and so on. Propagation stops at an ancestor whose own size is not affected by its children (for example, a `FixedSize` wrapper with a static width).

The classification is determined by the primitive widget's binding method implementation, not by the consumer. A `TextWidget` implementor knows that `bind_text` is relayout-level because changing the text changes the widget's `size_that_fits` result. A composite widget author or application developer does not need to think about this distinction — they call `bind_text(state)` and the framework handles the rest.

**Layout utility widgets with dynamic constraints.** The size constraint widgets (`MinSize`, `MaxSize`, `FixedSize`) accept state bindings for their constraint values, enabling dynamic resizing from application state changes, user-driven splitter interactions, or animation ticks. `FixedSize::bind_width(state)` registers a relayout-level binding — when the state changes, the widget's constraint changes, triggering relayout of the affected subtree.

**Relayout propagation.** When a widget is marked for relayout, the framework marks the widget and all its ancestors up to the root as needing relayout. During the layout pass, it starts from the highest dirty ancestor and works downward, re-running `size_that_fits` and `place_children` for each dirty node. Clean subtrees are skipped. This is the same incremental layout approach used by web browsers and by Qt's layout system. A relayout always implies a repaint for the affected widgets.

**Use cases for dynamic sizing.** A collapsible panel animates between zero height and its natural height by driving a `Signal<f32>` bound to a `FixedSize::bind_height`. A splitter pane resizes two adjacent panels by driving their width constraints from the splitter's drag position. A sidebar width set from user preferences reads from a persistent `Signal<f32>`. An expand/collapse animation drives a height constraint over multiple frames via the animation system.

---

## 3. Scrolling and Viewports

A scroll area is a container whose content may be larger than the visible space. The scroll area acts as a viewport — a window into a potentially large content region. Only the visible portion of the content is rendered, clipped to the viewport boundary.

Scrolling is designed to require minimal changes to the framework. The scroll offset is encoded through the existing layout placement mechanism, not as a separate coordinate transformation layer. Hit testing, event dispatch, and the state system require no modifications. The changes are confined to the arena (one new flag), the paint pass (clip rect support), the renderer (scissor rects), focus management (scroll-into-view), and the scroll area widget itself.

### 3.1 Layout: Unbounded Proposals and Offset Placement

A scroll area participates in layout like any other container widget. In `size_that_fits`, it claims the space its parent offers — this becomes the viewport size. In `place_children`, it proposes an unbounded size on the scroll axis to its content child. For a vertical scroll area, the content receives `SizeProposal { width: Some(viewport_width), height: None }` — "use the viewport width, but be as tall as you need." The content child responds with its natural height (potentially thousands of logical pixels).

The scroll area then positions its content child at `(viewport.x, viewport.y - scroll_offset.y)`. This encodes the scroll offset as a position offset within the normal placement system. No special coordinate transformation infrastructure is needed — the existing `place_children` / `WidgetPlacement` mechanism handles it. The recursive layout function processes the content child and its descendants with the offset origin, and all bounds stored in the arena end up in correct screen-space positions.

`SizeProposal` already supports `None` values for unbounded dimensions. No changes to the `SizeProposal` type or to `layout_widget_recursive` are required.

### 3.2 Hit Testing: No Changes Required

The existing `hit_test_recursive` provides viewport clipping implicitly. It checks `bounds.contains(point)` on the parent before recursing into children. A point outside the scroll area's viewport bounds is rejected at the scroll area's bounds check, and no child is tested. Children scrolled above the viewport have negative screen-space y coordinates that no in-viewport point would match. Children within the viewport have correct screen-space bounds (computed from the offset placement) that match pointer positions directly.

No changes to the hit testing code are needed. The scroll offset encoded in placement positions and the existing parent-bounds containment check together provide correct viewport-clipped hit testing.

### 3.3 Clipping in the Paint Pass

The paint pass requires one new capability: clipping child rendering output to the scroll area's viewport bounds. Without clipping, children positioned near the edge of the viewport would render partially outside it.

The arena's `WidgetNode` gains a `clips_children: bool` flag (default `false`). The scroll area widget sets this flag to `true` on its own arena node. When `paint_widget` enters a node with `clips_children: true`, it pushes a clip rect (the node's own bounds, which represent the viewport) onto the Canvas before recursing into children, and clears the clip after all children are painted.

The Canvas already provides `set_clip(Rect)` and `clear_clip()` methods that produce `DrawCommand::SetClip` and `DrawCommand::ClearClip` entries in the RenderFrame. The change to `paint_widget` is approximately five lines: check the flag, push clip, recurse, pop clip.

### 3.4 Renderer: Scissor Rect Implementation

The `SetClip` and `ClearClip` draw commands exist in the RenderFrame but are currently no-ops in the renderer. The implementation maps directly to wgpu's scissor rect API: `render_pass.set_scissor_rect(x, y, width, height)` for `SetClip` (coordinates in physical pixels, multiplied by scale factor), and resetting the scissor to the full surface dimensions for `ClearClip`. This is approximately ten lines of code in the renderer.

Nested scroll areas (rare but valid — a scrollable sidebar inside a scrollable page) require a clip rect stack. Each `SetClip` pushes a rect, and the effective clip is the intersection of all rects in the stack. `ClearClip` pops the top rect and restores the previous intersection.

### 3.5 Focus and Scroll-Into-View

When Tab navigation moves focus to a widget that is inside a scroll area but outside the current viewport, the scroll area must scroll to make the focused widget visible. Without this, keyboard users cannot see what they have focused.

After `focus_with_origin` sets focus to a widget, the framework walks up the ancestor chain. If any ancestor has `clips_children: true`, the framework checks whether the focused widget's bounds are fully within that ancestor's viewport bounds. If not, the framework dispatches a `WidgetEvent::ScrollIntoView { target_bounds: Rect }` to the clipping ancestor. The scroll area handles this event by adjusting its scroll offset to bring the target bounds into view, using the minimum scroll change needed to make the widget fully visible (or centering it if the widget is larger than the viewport).

### 3.6 The ScrollBar Widget

The scroll bar is a standalone Level 2 widget in `fern-widgets`, not a rendering detail inside ScrollArea. A standalone widget participates in the framework's hit testing, event dispatch, focus, and accessibility systems. Its thumb is a region within its bounds that the framework's existing pointer routing handles. Its accessibility node declares `Role::ScrollBar` with `set_numeric_value`, `set_min_numeric_value`, `set_max_numeric_value`, and `Action::SetValue`.

The ScrollBar stores the current scroll position and the content-to-viewport ratio (both provided by the ScrollArea via shared `Signal<f32>`). It computes thumb position and size from these values. It handles `PointerDown` on the thumb (start drag), `PointerMove` during drag (update position), `PointerUp` (end drag), and `PointerDown` on the track (page-scroll toward click position). It supports both vertical and horizontal orientations.

### 3.7 ScrollArea and ScrollBar Interaction

The ScrollArea owns the scroll state (`Signal<f32>` for each axis). The ScrollBar reads from and writes to this shared state. The ScrollArea and ScrollBar communicate through the reactive binding system, not through events or callbacks.

The ScrollArea supports two scroll bar display modes via `ScrollBarStyle`.

**Overlay mode** (default, matching macOS and modern Linux). The ScrollArea's viewport occupies the full available width — the scroll bar does not reduce the content area. A thin passive scroll indicator (a few semi-transparent pixels at the trailing edge) is painted directly by the ScrollArea during scrolling as a visual hint. When the pointer enters the scroll bar activation zone (a region at the trailing edge wider than the thin indicator), the ScrollArea shows the full interactive ScrollBar widget as an overlay using the existing overlay system (`OverlayPlacement::NearAnchor`, `DismissBehavior::PointerLeave`). The overlay ScrollBar appears on top of the content, receives pointer events for thumb drag and track click, and dismisses when the pointer leaves. The viewport width never changes. The transition from thin indicator to full scroll bar can be animated using the animation scheduler.

**Permanent mode** (matching traditional Windows/GTK style, or when the user's accessibility preferences request always-visible scroll bars). The ScrollBar is a layout sibling of the content viewport. The ScrollArea's internal structure becomes an HStack of `[clipping viewport]` + `[ScrollBar]`. The viewport is narrower by the scroll bar's width. The scroll bar is always visible and always interactive. The viewport width is constant (reduced by the scroll bar width but never changing dynamically).

The mode is selected via `ScrollArea::new(content).scroll_bar_style(ScrollBarStyle::Overlay)` or `ScrollBarStyle::Permanent`. The application or the theme can set a default. An accessibility preference for "always show scroll bars" overrides to Permanent mode.

### 3.8 The Scroll Area Widget

The ScrollArea is a Level 2 (`Widget` trait) widget in `fern-widgets`. It is the viewport container — it owns the clipping behavior, the layout negotiation with unbounded proposals, and the content offset placement described in Sections 3.1–3.5.

The scroll offset for each axis is stored as a `Signal<f32>` (not a raw `Vec2`), because the ScrollBar widget needs to read and write the position through the reactive binding system. When the ScrollBar's thumb is dragged, it sets the shared `Signal<f32>`. The ScrollArea's binding on that state triggers a relayout, which re-runs `place_children` with the updated offset. When the user scrolls via mouse wheel or trackpad (`WidgetEvent::Scroll`), the ScrollArea updates the `Signal<f32>` directly, and the ScrollBar's thumb position updates via the same binding path.

The ScrollArea creates and manages a ScrollBar widget according to the active `ScrollBarStyle` (Section 3.7). In overlay mode, the ScrollArea paints a thin passive indicator during its own `paint()` pass and shows the interactive ScrollBar as an overlay on pointer proximity. In permanent mode, the ScrollBar is a layout child positioned as a sibling of the content viewport. The ScrollArea sets `clips_children: true` on its arena node so the paint pass clips content to the viewport bounds.

The ScrollArea handles `WidgetEvent::ScrollIntoView` to support focus-driven scrolling (Section 3.5) — it adjusts the `Signal<f32>` offset to bring the target bounds into view.

For accessibility, the ScrollArea declares `Role::ScrollView` with scroll position properties (`set_scroll_x`, `set_scroll_y` and their min/max ranges) and page-level scroll actions (`Action::ScrollDown`, `Action::ScrollUp`, `Action::ScrollLeft`, `Action::ScrollRight`). The ScrollBar declares its own `Role::ScrollBar` with `set_numeric_value`, `set_orientation`, and `Action::SetValue` for direct position control. These are two separate AccessKit nodes with complementary roles.

### 3.9 Interaction with Virtualized Lists

The `ListView` widget (backed by `ListModel<T>` or `ListDataSource`) depends on scrolling. The scroll offset determines which items are visible. The `ListView` only instantiates widget subtrees in the arena for visible items plus a small buffer above and below the viewport. As the user scrolls, items leaving the viewport have their subtrees destroyed and items entering the viewport have new subtrees created.

The `ListView` does not need a general-purpose "scroll area wrapper" — it implements the scrolling behavior internally, because it needs tight control over which items have widget subtrees. It uses the same mechanisms as the scroll area (offset placement, `clips_children: true`, `WidgetEvent::Scroll` handling) but also manages the item lifecycle in the arena.

### 3.10 Accessibility for Scroll Areas and Lists

The scroll system produces two AccessKit nodes with complementary roles. The ScrollArea declares `Role::ScrollView` with scroll position properties (`set_scroll_x`, `set_scroll_y` and their min/max ranges), `set_clips_children(true)`, and page-level scroll actions (`Action::ScrollUp`, `Action::ScrollDown`, `Action::ScrollLeft`, `Action::ScrollRight`). The ScrollBar declares `Role::ScrollBar` with `set_numeric_value` (the current scroll position), `set_min_numeric_value`, `set_max_numeric_value`, `set_orientation`, and `Action::SetValue` for direct position control by assistive technologies. Screen readers use the ScrollView node to announce the scrollable region and the ScrollBar node to present the scroll position as an adjustable value.

For lists, AccessKit provides `Role::List` with `Role::ListItem` for static lists, and `Role::ListBox` with `Role::ListBoxOption` for interactive selectable lists. The critical properties for virtualized lists are `set_position_in_set(index)` on each visible item and `set_size_of_set(total_count)` on the list container. These tell screen readers the logical position of each item ("item 5 of 200") even when the AccessKit tree only contains the items currently visible in the viewport. Items outside the viewport do not exist in the arena and therefore do not appear in the AccessKit tree — no special mechanism is needed to exclude them.

---

## 4. Widget State Ownership

FernUI uses a retained widget tree with arena-backed flat storage, following the approach proven by Masonry's TreeArena.

All widgets live in a flat `SlotMap`-like arena. Parent-child relationships are stored as ID references within the arena. The tree structure is explicit (unlike a pure ECS where relationships are implicit), but the flat storage avoids Rust's borrow-checker challenges with recursive mutable tree traversal.

The framework processes the tree through well-defined passes (event, layout, accessibility, paint), each of which traverses the arena without holding multiple mutable references simultaneously. This is the key insight from Masonry: separate the passes so that no pass needs to mutate a widget while reading another widget's state.

---

## 5. Widget Extensibility

> **V1 (superseded).** The original design split extensibility in two. A `CompositeWidget` trait described *what a widget is made of* via `fn build(&self, ctx) -> WidgetId`, deliberately omitted any sizing override, and delegated layout to its composed subtree. A separate `Widget` trait described *how a widget paints itself* via `paint()`, with the full layout surface (`size_that_fits`, `place_children`) for widgets with no pre-existing visual equivalent — color pickers, node graphs, the RichTextEditor.
>
> The split forced authors to *choose at definition time* between composition and custom paint. Real widgets regularly want both (a Card that composes children but paints its own background; a ScrollArea that composes children but clips them with a scissor rect), which pushed the framework into a compatibility adapter and widgets into awkward indirection. The `CompositeWidget::build(&self)` signature — immutable receiver — also forced every stateful composite into the `RefCell<Option<State<T>>>` pattern so mutation-needing event handlers could reach state created in `build`.
>
> **V2 (current).** The two traits are one. The unified `Widget` trait has a single `build(&mut self, ctx)` for composition, a single `paint()` for own-visuals, and both are optional with sensible defaults. There is no composite adapter; widgets are widgets. See [§29.1 Unified Widget Trait](#291-unified-widget-trait) for the full signature and the four common widget shapes (leaf / container / composing / hybrid). The V2 model is what the entire widget library is written against — no `CompositeWidget` references remain in the codebase.

### 5.1 The Slot System

Standard widgets ship with named extension points — slots — at structural boundaries where extension is anticipated. A slot is an optional placeholder that takes zero space when empty and accommodates arbitrary widget content when filled.

```rust
TabWidget::new()
    .tab("Chapter 1", || chapter_editor(1))
    .trailing_slot(|ctx| {
        HStack::new()
            .child(Button::icon_only(Icon::Plus).on_activate_fn(|ctx| ctx.send_intent(AppIntent::AddChapter)))
            .child(Button::icon_only(Icon::ChevronDown).on_activate_fn(|ctx| ctx.send_intent(AppIntent::OpenChapterMenu)))
    })
```

Slots are part of a widget's public API contract. Every standard composite widget in fern-widgets ships with sensible slots at positions where extension is commonly needed, following a consistent naming convention: `leading_slot`, `trailing_slot`, `header_slot`, `footer_slot`.

---

## 6. UI Construction Patterns

A composite widget's `build()` method constructs a widget subtree by adding widgets to the arena and assembling them into parent-child relationships. The raw API (`ctx.add()` returning a `WidgetId`, then `container.add_child(id)`) is correct and always available, but it produces verbose code for common cases. This section defines a convenience layer that reduces boilerplate while preserving full access to the underlying mechanisms.

### 6.1 Inline Children

Most children in a `build()` method are created, added to the arena, and immediately passed as a child to a container — the `WidgetId` is never referenced again. The inline `child()` method eliminates the intermediate variable by accepting a widget value directly rather than a pre-registered ID.

Containers provide three child-addition methods. `add_child(id: WidgetId)` takes a pre-registered ID — used when the composite needs the child's ID for bindings, tooltips, or later reference. `child(widget: impl IntoWidgetTree)` takes a widget value for deferred insertion — the widget is stored temporarily inside the container and resolved into the arena when the container itself is added via `ctx.add()`. Both methods coexist on the same container and can be mixed freely in a single builder chain.

When `BuildContext::add()` inserts a container that has deferred children, it resolves them recursively: each deferred child is inserted into the arena (which may itself have deferred children), and the resulting IDs are wired as children of the container. The resolution is depth-first, matching the visual nesting order.

### 6.2 Iterator-Based Children

The `children()` method accepts an iterator of widgets, adding each element as a deferred child. This replaces the `for` loop + `add_child` pattern for homogeneous child lists known at build time.

For cases where per-item logic is complex (conditional sub-elements, derived values, separators between items), breaking the builder chain and using a `let mut` variable with a regular `for` loop is always available. The builder is an ordinary owned Rust value — standard control flow works naturally.

### 6.3 Conditional Children

The `child_opt()` method accepts an `Option<impl IntoWidgetTree>`. If `Some`, the child is added. If `None`, the method is a no-op and the chain continues. This prevents chain-breaking for simple single-widget conditionals.

For `match` expressions where all arms return the same widget type, the result can be passed directly to `child()` since `match` is an expression in Rust. For arms returning different widget types, `child_boxed(Box<dyn IntoWidgetTree>)` accepts a type-erased widget.

These are static conditionals — evaluated once during `build()`. Dynamic visibility (toggling a panel during interaction) uses `visible_when(Signal<bool>)`, which sets the widget dormant or active without tree reconstruction.

### 6.4 The Repeater — Dynamic Non-Virtualized Collections

The architecture provides two extremes for rendering collections: static children built once in `build()` (loops and iterators), and fully virtualized `ListView` backed by `ListModel<T>` or `ListDataSource` with scroll-position-dependent instantiation. The Repeater fills the middle ground — a dynamic, non-virtualized collection where every item has a widget subtree, and the set of items can change at runtime without a full composite rebuild.

A Repeater takes a `ListModel<T>` and a builder closure (the delegate). It creates one delegate instance per data source item. When the `ListModel` signals that items were inserted, removed, or moved (via `DataChange` notifications), the Repeater creates or destroys delegate subtrees accordingly. The Repeater itself is a Level 2 widget in `fern-widgets` — it is not a framework-level concept.

The builder closure receives each item from the data source and returns a widget tree for that item. The Repeater inserts the resulting subtree as a child at the corresponding position. When the data source notifies `ItemsInserted { range }`, the Repeater calls the builder for each new item and inserts the subtrees at the correct positions. When `ItemsRemoved { range }` is signaled, the Repeater destroys the corresponding subtrees. When `ItemsMoved { from, to }` is signaled, the Repeater reorders its children without destroying or recreating them.

For `ItemUpdated { index }`, the first implementation destroys and recreates the item's subtree. A future optimization path allows in-place updates via reactive bindings — the item's properties are bound to state handles that the data source updates, and the subtree repaints without reconstruction.

The delegate closure comes in two forms. The simple form returns a widget value directly and is sufficient for delegates that do not need reactive state. The context form receives a `BuildContext` and returns a `WidgetId`, matching the `Widget::build()` signature — this is necessary when the delegate needs `ctx.signal()`, `ctx.effect()`, or explicit binding registration.

### 6.5 Repeater vs. ListView

The Repeater creates a widget subtree for every item in the data source. For small, bounded collections (toolbar buttons, tab headers, form fields, chapter lists — typically under 100 items), this is appropriate. The Repeater does not imply scrolling — it produces siblings inside a container, and the container handles overflow.

The `ListView` creates widget subtrees only for visible items plus a small buffer. For large or unbounded collections (log viewers, file browsers, search results — hundreds to millions of items), the `ListView` is required. The `ListView` always implies scrolling and manages item lifecycle based on scroll position.

The boundary is the same as in QML: use a Repeater when the item count is small enough that all subtrees can exist in the arena simultaneously, and a ListView when the item count makes that impractical.

### 6.6 Static vs. Dynamic: When to Use Which

The builder methods (`child()`, `children()`, `child_opt()`, loops, conditionals) are evaluated once during `build()`. The result is baked into the widget tree. When the underlying data changes, the composite must be fully rebuilt to reflect the change.

Dynamic behavior uses different mechanisms. `visible_when(Signal<bool>)` toggles a widget between active and dormant without tree modification — correct for showing/hiding UI sections during interaction. `enabled_when(Signal<bool>)` toggles interactivity without visibility change. The `Repeater` manages a dynamic set of siblings driven by `ListModel<T>` change notifications — correct for collections that grow, shrink, or reorder during interaction. The `ListView` virtualizes large collections with scroll-position-dependent instantiation.

The boundary is clear: if the content structure is fixed for the lifetime of the composite, use builder methods in `build()`. If individual widgets need to appear or disappear, use `visible_when`. If a collection changes, use a `Repeater` (small) or `ListView` (large).

---

## 7. Reactivity Model

The core question this section answers — how does a widget stay in sync with mutable state without full view diffing — has the same answer in V1 and V2. The V2 API cleaned up the surface; the V1 description below stays here for the rationale.

> **V1 (superseded).** The original design had four reactive types: `State<T>` (mutable handle, owned by a widget), `DerivedState<T>` (read-only, `.map()` of another handle), `Reactive<T>` (widget property that might be either static or bound), and `StateHandle<T>` (erased inter-handle reference). Bindings wired state to widget properties via method pairs: `.background(Color)` for a fixed value, `.bind_background(State<Color>)` for a reactive link. A composite's `build(&self, ctx)` returned `&self`, forcing the `RefCell<Option<State<T>>>` pattern so mutation-needing event handlers could reach state created in `build`. The framework rebuilt on theme/environment change to re-capture derived-state closures that had frozen-in theme values.
>
> **V2 (current).** `Signal<T>` replaces `State<T>` and `DerivedState<T>` (derived signals are just `signal.map(|v| ...)`), `Prop<T>` replaces `Reactive<T>`, and `ObserverHandle` replaces `StateHandle<T>`. `Signal::new(x)` is mutable; `signal.map(f)` is read-only and derived. `build(&mut self, ctx)` takes `&mut self`, so event handlers close over `Signal<T>` clones (not `RefCell<Option<…>>`) and mutate them directly. The dual static-vs-bound method pair (`background()` / `bind_background()`) collapses into a single method taking `impl Into<ColorProp>` — the prop type knows whether to track a signal. Theme changes no longer trigger rebuild: the theme is itself a `Signal<Theme>` and role-based props resolve at paint time. See **§29.4** for the full unified model and [`reactive-theme.md`](reactive-theme.md) for the theme-reactivity mechanism.

### 7.1 Why declarative bindings plus imperative structural change

What survives unchanged from V1 is the division of labor. Simple property reactivity (a label shows different text when the model changes, a button disables when a form is invalid, a panel hides when a feature flag is off) is **declarative** — the widget declares a binding, and the framework reacts. Structural changes (switching tabs, adding or removing children from a dynamic list, activating or dormant-ing a subtree) are **imperative** — explicitly requested from a handler.

This split is what lets FernUI avoid both full view diffing (Xilem/React reconciliation) and ad-hoc observer soup (imperative GTK callbacks). Reconciliation is expensive and predictable only for the common case; it pays a cost on every update. Ad-hoc observer registration is fast but fragile — cleanup, cycles, and order-of-notification bite you forever. Declarative bindings for the common case (a property depends on a value; keep them in sync) plus imperative for the rare structural case give cheap updates for the frequent path without surrendering control over the rare one.

Structural mutations are expressed as operations on `EventContext` (`ctx.set_dormant(id)`, `ctx.activate(id)`, `ctx.destroy(id)`) and applied after the current handler returns — see [`events-and-gestures.md`](events-and-gestures.md) §5 for the deferred-operations pattern.

---

## 8. Conditional Rendering and Dormancy

The widget arena supports three activation states for widget subtrees.

**Active** — fully operational. Participates in layout, receives events, paints, has AccessKit nodes, holds rendering resources.

**Dormant** — state preserved, rendering resources released. Does not participate in layout, receives no events, has no AccessKit nodes. The widget data and state values remain in the arenas. Reactivation triggers relayout and repaint, but no reconstruction.

**Destroyed** — removed from the arena entirely. State is gone. Must be rebuilt from scratch.

Three construction strategies control the memory/responsiveness tradeoff for multi-pane widgets:

**Eager** — all subtrees built at construction time, inactive ones set to Dormant. Switching is instant. Suitable for tab widgets with a small number of tabs.

**Lazy** — subtrees built on first activation, then preserved as Dormant. Suitable when building a subtree is expensive and the user may never visit all tabs.

**Transient** — subtrees built on activation, destroyed on deactivation. Lowest memory, highest switch cost. Suitable for browser-like scenarios where each tab is independent.

Atlas entries for dormant widgets are evicted via LRU. When a dormant widget reactivates, glyphs and shapes re-rasterize on demand. Rendering resource consumption is proportional to visible content, not total content.

---

## 9. Event System

The event system answers two questions: how does input reach the widget that should handle it, and how does that handler request application-level behavior. The two-pass preview+bubble routing and the typed-command-flow rationale below survive unchanged from V1; the surface API that lands them — attached handlers replacing a monolithic `event()` method, Shortcut/Intent/Action replacing `AppCommand` — is V2. The dedicated docs [`events-and-gestures.md`](events-and-gestures.md) and [`shortcut-intent-action.md`](shortcut-intent-action.md) cover the current API in detail; the sections below focus on the "why."

### 9.1 Input Event Routing — Preview and Bubble

Platform input from winit is translated into high-level `WidgetEvent` variants and routed through the widget tree in two passes:

**Preview pass:** root → target. Ancestors get a chance to intercept events before the target sees them — a `MenuList` overlay swallowing Arrow keys before any menu item sees them, a modal scrim discarding pointer events outside the modal.

**Bubble pass:** target → root. The standard dispatch path — the target handles first; unhandled events walk up the parent chain until something returns `EventResponse::Handled` or the root is reached.

Pointer events target the deepest hit via hit testing against layout bounds. Keyboard events target the focused widget. AccessKit action requests target their stated node directly. Scroll events hit-test at the pointer and bubble to the nearest handler (a ScrollArea ancestor, typically).

> **V1 (superseded) — handler storage.** V1 required every widget to implement `fn event(&mut self, event: &WidgetEvent, ctx: &mut EventContext) -> EventResponse` — one giant `match` covering every variant it cared about. Gesture recognizers had to be instantiated by hand. Handler composition (a wrapper listening for taps on a child) required proxying through the outer widget's `event()` method. Mutable access to state created in `build()` forced `RefCell<Option<State<T>>>` everywhere.
>
> **V2 (current) — attached handlers.** Widgets register typed closures per event type via the `WidgetBuilder` trait (`.on_tap(...)`, `.on_hover(...)`, `.on_key(...)`, etc.) or via `HandlerSet` + `ctx.apply_self_handlers()` for the composite's own node. The framework stores closures on the arena node and dispatches them automatically. Gesture recognizers auto-wire from attached handlers; handler composition is just layering wrappers that carry their own handlers; handlers close over `Signal<T>` clones and mutate freely without `&mut self` on the widget. See [`events-and-gestures.md`](events-and-gestures.md) for the full catalogue of attached-handler types and the `HandlerSet` pattern.

### 9.2 Typed Application Behavior: The Intent / Action Pipeline

Widgets don't call into application code directly. They emit a typed **intent** (an application-defined enum variant), and ancestor **actions** — widget-owned handlers keyed by intent name — pick the intent up as it walks from source widget to root. Rebindable keyboard **shortcuts** are a third layer on top: a shortcut maps a keystroke to an intent name, and firing the shortcut is indistinguishable from an intent emitted by a widget.

The V2 surface lives in [`shortcut-intent-action.md`](shortcut-intent-action.md). The "why" behind this choice is worth recording here because it's the most opinionated decision in the framework.

#### 9.2.1 Closures vs. typed values — the bet FernUI places

Retained-mode GUI frameworks split roughly into two schools on wiring user actions to application logic.

**Closure-based.** The widget accepts a callback. SwiftUI (`Button(action: { ... })`), Jetpack Compose (`onClick = { ... }`), Flutter (`onPressed:`), GTK signal handlers, Qt signal/slot, every web framework (`onClick`). The more common school by user count.

**Typed message/command actions.** The widget emits a typed value of an application-defined enum; a handler matches on it. Elm (`Msg`), Iced (`Message`), The Composable Architecture (`Action`), Redux (action objects), Bubble Tea (`tea.Msg`). The minority school by user count, but dominant in frameworks that prioritize testability, undo, and large-application correctness.

FernUI goes with typed. The bet is that the target applications — writing tools, IDEs, dispatch consoles, course managers — are long-lived and grow large. Applications with hundreds of operations, complex undoable state, accessibility automation requirements, and multi-year lifespans benefit more from enumerating every operation in one place than they suffer from the prototyping friction. A dozen-button weekend prototype would find the discipline excessive — but that's not what FernUI is for.

#### 9.2.2 What the typed-intent pipeline gains

- **Central command routing.** The application has one place that lists every operation it can perform — the union of its `Action`s. Adding a cross-cutting concern (logging, undo, permission gating) touches one layer, not every widget construction site. Reading the action list tells you what the application does.
- **Undo and replay.** A typed intent is a value: serializable, inspectable, recordable. Recording intent sequences gives an undo log, a replay file, a macro recorder. A closure cannot be recorded.
- **Testing.** Tests assert `captured_intents == vec![AppIntent::Save, AppIntent::Close]` without knowing which widget triggered each. The test decouples from implementation details.
- **External automation.** Command palettes (the Ctrl+Shift+P pattern), accessibility automation, scripting interfaces — all need a vocabulary of operations they can invoke programmatically. Typed intents are that vocabulary.
- **Compile-time exhaustiveness.** `#[derive(IntentKind)]` on the enum produces a variant list; ancestor `Action`s opt in to the subset they care about via `Action::new("id")`. A new variant is guaranteed to be either handled or visibly unhandled.
- **Rebindable keystrokes.** `ShortcutRegistry` maps keystrokes to intent names, with two-layer defaults + user overrides. Because the shortcut fires an intent (not a closure), rebinding Ctrl+S to Ctrl+Alt+S requires no change to the widget tree — the `Action` listening for `"app.save"` is the same either way.

#### 9.2.3 What typed intents cost

- **Friction during prototyping.** Defining the variant, registering the action, only then writing the behavior is slower than writing click behavior inline. Mature applications don't feel this (the intent set stabilizes); exploratory work does. See §9.2.5 on the escape hatch.
- **Third-party widget libraries.** A library widget cannot anticipate the host application's intent type. Iced threads a `Message` generic through every widget signature; FernUI's non-generic `Widget` trait (see §29) cannot do that. Library widgets that need to fire application-level behavior have to either take a closure at the boundary (the `.on_activate_fn(...)` escape hatch) or define their own internal intent type that the host translates.
- **Highly dynamic UIs.** A plugin system where plugins contribute buttons with their own actions cannot have those variants in a central enum at compile time. A scripting console where users type arbitrary run-on-click code has no compile-time vocabulary for "this specific code." See §9.2.5.
- **Cognitive overhead.** A developer coming from SwiftUI or Compose expects inline `Button(onClick: { doSomething() })`. The typed-intent pattern requires understanding why that's not how this framework works. The documentation has to teach the pattern; code review enforces it.

For long-lived commercial applications these costs absorb quickly and the benefits compound. For small utilities they may exceed the benefits — FernUI targets the former.

#### 9.2.4 Local view state is not an intent

The intent pipeline is for *application-level* behavior — operations that affect persistent state, the domain model, or cross-widget coordination. It is not for local view state.

A disclosure triangle's open/closed state is a `Signal<bool>` mutated directly. A character counter binds to the input's signal and recomputes via `signal.map(...)`. A tooltip's delay timer is internal. None of these are intents: they live inside the widget and the application doesn't know or care.

Rule of thumb: *would the application care if this widget were swapped for a different one with the same purpose?* If yes (save is save, regardless of button or menu item), emit an intent. If no (disclosure triangles are part of the widget), mutate a signal. A checkbox for "show advanced options" is local; a checkbox for a persisted setting is an intent. The framework doesn't decide; the developer does.

#### 9.2.5 Escape hatch: `on_activate_fn`

Every activation-firing widget provides a closure-taking variant alongside the intent-taking one:

```rust
// Standard path — emits an intent:
Button::new("Save").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save))

// Plugin button — closure captures plugin-loaded-at-runtime action:
Button::new(&plugin.label).on_activate_fn({
    let plugin = plugin.clone();
    move |ctx| plugin.invoke(ctx)
})
```

Both variants store the same `Box<dyn FnMut(&mut EventContext)>` internally. The `_fn` form documents "this call site has opted out of the typed-intent discipline" — grep for it to audit. Losing the discipline means losing recordability, command-palette discoverability, undo recording, and assertion-based tests *for that specific action*; the other 200 buttons in the application still have them.

The canonical form — `.on_activate_fn(|ctx| ctx.send_intent(AppIntent::X))` — is explicit about what it does: produce and send a typed intent. It's slightly more verbose than a magic `.on_activate(AppIntent::X)` would be, but it makes the intent emission visible at the call site and collapses the "one method per interaction shape" combinatorial explosion into a single method-plus-closure.

#### 9.2.6 Naming: `on_activate`, not `on_click`

Activation can come from a pointer click, a keyboard Enter / Space, an accessibility action from a screen reader, or a synthetic activation from a parent. The handler fires identically in all cases — there is no way to distinguish them, and there should not be, because that would let widgets behave differently for keyboard and screen-reader users than for mouse users. `click` is misleading; `activate` is correct.

### 9.3 Focus Management

The tree maintains a single `focused: Option<WidgetId>`. Tab / Shift-Tab cycles focus through widgets whose node has `focusable = true` (set via `.focusable(true)` on the builder or on a `HandlerSet`) in tree order. Explicit `.tab_index(n)` overrides document order.

Focus carries a **focus origin** — `Keyboard`, `Pointer`, or `Programmatic` — that widgets can observe to paint a focus ring only on keyboard focus by default (Int UI style). `FocusGained` / `FocusLost` events dispatch to the widget via `on_focus(gained: bool, ctx)`.

Programmatic focus transfer is a single call: `ctx.request_focus(id)` from any handler. `first_focusable_descendant(id)` is used by modal openers to land focus on the primary action button. Focus changes synthesize a `ScrollIntoView` event that bubbles to the nearest clipping ancestor so tab-focusing an offscreen widget slides it into view. When dormant subtrees reactivate, focus is restored to the previously focused widget within that subtree.

### 9.4 Backend Events: Direct Subscription via the EventSource Trait

A persistent confusion in early drafts of this document treated backend events (database changes, network responses, file watcher notifications, message bus events) as if they were the same kind of thing as typed application commands. They are not. Section 9.2 argues for typed commands as a discipline for *user intent*: a button click says "I want to save," and the application interprets that intent in its current context. A backend event is the opposite shape — a fact that has happened, observed by the application, that one or more widgets need to react to. There is no decision to make, no routing to perform, no command palette vocabulary, no undo participation. The widget just needs to know the fact and update itself.

Forcing backend events through the typed-command layer means writing a `Cmd::ItemCreated { ids }` variant whose only call site is a forwarder, whose only handler arm is a signal setter, and whose only purpose is to cross the typed-command boundary that did not need to be crossed. The integration with Qleany's event hub made this concrete: every backend event would have required a command variant, a forwarding closure, a match arm, and a shared signal reachable from the handler. Six steps for "when an item is created, show its title." Slint's globals-and-callbacks pattern reaches the same destination in three. The mismatch was a sign that typed commands were the wrong layer for this case, not that the typed-command discipline was wrong overall.

The right separation is: typed commands for user intent (Section 9.2), direct subscription for backend events (this section). The two layers compose. A button click emits `Cmd::AddItem`. The command handler calls a controller. The controller mutates the database and emits a backend event. A widget that subscribes to that event reacts and updates its display. Each layer does what it is best at.

#### 9.4.1 The EventSource Trait

FernUI does not depend on any specific backend event source. The framework defines a trait that any event source (Qleany's `EventHubClient`, a Tokio broadcast channel, a custom message bus, a file watcher) can implement:

```rust
use std::any::Any;
use std::hash::Hash;

/// An external source of events that widgets can subscribe to.
///
/// Implementations include backend message buses, database change notifiers,
/// file watchers, network response channels — any source that publishes events
/// asynchronously and that widgets need to react to.
pub trait EventSource: 'static {
    /// The key by which subscribers identify which events they care about.
    /// Typically an enum (Qleany's Origin) or a topic string.
    type Origin: Clone + Eq + Hash + Send + 'static;

    /// The event payload delivered to subscriber callbacks.
    /// Must be Clone because multiple subscribers may receive the same event,
    /// and must be Send because events cross from the publisher's thread to
    /// the UI thread via the framework's proxy bridge.
    type Event: Clone + Send + 'static;

    /// Subscribe a callback to events of a given origin. The callback is
    /// invoked on whatever thread the source publishes from (typically a
    /// background thread). The returned handle, when dropped, removes the
    /// subscription from the source's internal registry.
    fn subscribe(
        &self,
        origin: Self::Origin,
        callback: std::sync::Arc<dyn Fn(Self::Event) + Send + Sync + 'static>,
    ) -> SubscriptionHandle;
}

/// An opaque handle returned by `EventSource::subscribe`.
/// The source defines what it contains; the framework treats it as a token
/// whose `Drop` impl performs the unsubscription.
pub struct SubscriptionHandle {
    inner: Box<dyn std::any::Any + Send>,
}
```

Two associated types and one method. `Origin` is the subscription key; `Event` is the payload. The single method takes an `Arc<dyn Fn(Event)>` (rather than `Box`) so that the source's internal dispatch can clone the callback cheaply when invoking it for multiple subscribers, and so that the framework's wrapper closure can be `Fn` (called many times) without needing to clone the boxed contents on each call. `SubscriptionHandle` is opaque at the framework level — its `Drop` impl, defined by the source implementation, removes the subscriber entry from whatever internal registry the source maintains.

For Qleany's `EventHubClient`, the implementation is mechanical:

```rust
// Application code, not in fern-core:
impl fern_core::EventSource for EventHubClient {
    type Origin = common::event::Origin;
    type Event = common::event::Event;

    fn subscribe(
        &self,
        origin: Self::Origin,
        callback: Arc<dyn Fn(Self::Event) + Send + Sync + 'static>,
    ) -> SubscriptionHandle {
        let token = self.subscribe_internal(origin, callback);
        SubscriptionHandle::new(token)
    }
}
```

The `subscribe_internal` returns a removal token whose `Drop` impl removes the entry from the EventHubClient's subscriber HashMap. This is a small change to the existing EventHubClient (currently `subscribe` returns `()`); the change is application-side, not framework-side.

#### 9.4.2 Registering a Source with the Builder

The application registers its event source on the `FernAppBuilder` at startup:

```rust
let app_context = Arc::new(AppContext::new());
let event_hub = EventHubClient::new(&app_context.event_hub);
event_hub.start(app_context.quit_signal.clone());

FernAppBuilder::new()
    .theme(Theme::light_default())
    .event_source(event_hub)
    .root({
        let app_context = app_context.clone();
        move |tree| tree.add(App::new(app_context))
    })
    .run();
```

The builder's `event_source<S: EventSource>(source: S)` method takes ownership of the source, wraps it in an internal `EventSourceAdapter` that erases the associated types into stored closures, and stores the adapter as a single non-generic value. The framework itself does not become generic over `S` — the type parameter is consumed at the call site and the stored adapter is a concrete type.

This is the sole point at which the framework learns about the event source's types. The TypeIds of `S::Origin` and `S::Event` are recorded in the adapter for later validation. Type names are recorded as static strings via `std::any::type_name` so error messages are human-readable.

The builder accepts at most one event source per application. Two sources of different types would require either two adapters (a future extension; see §9.4.7) or a single source that internally multiplexes multiple backends. For all current FernUI use cases, one source is sufficient.

#### 9.4.3 Subscribing from Widgets

A widget subscribes to events during its `build()` method via `BuildContext::subscribe_event`:

```rust
impl Widget for App {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let item_label = self.item_label.clone();
        let app_context = self.app_context.clone();

        ctx.subscribe_event(
            Origin::DirectAccess(DirectAccessEntity::Item(EntityEvent::Created)),
            move |event: &Event| {
                if let Some(id) = event.ids.first() {
                    if let Ok(Some(dto)) = item_commands::get_item(&app_context, id) {
                        item_label.set(format!("Created: {} (id={})", dto.title, dto.id));
                    }
                }
            },
        );

        let root = ctx.add(
            VStack::new()
                .child(TextWidget::new("").bind_text(self.item_label.map(|s| s.clone())))
                // ...
        );
        vec![root]
    }
}
```

Read this `build()` top to bottom and the data flow for the item-label is local. The signal is owned by the widget. The subscription is registered next to the signal, with a closure that updates the signal when an event arrives. The TextWidget binds to the signal. Three things, all in one method, all readable in order. There is no `Cmd::ItemCreated` variant defined elsewhere, no match arm in the application's command handler, no global state, no setup closure in `main()` that wires the subscription — the subscription lives where the consumer lives.

The `BuildContext::subscribe_event` signature is generic but does not propagate generics into stored state:

```rust
impl BuildContext<'_> {
    pub fn subscribe_event<O, E, F>(&mut self, origin: O, callback: F)
    where
        O: 'static,
        E: 'static,
        F: Fn(&E) + 'static,
    {
        // Validate that the configured event source uses these types.
        let adapter = self.app_state.event_source.as_ref()
            .expect("subscribe_event called but no event source configured");

        debug_assert_eq!(
            adapter.origin_type, TypeId::of::<O>(),
            "origin type mismatch: source uses {}, subscribe call used {}",
            adapter.origin_type_name, std::any::type_name::<O>(),
        );
        debug_assert_eq!(
            adapter.event_type, TypeId::of::<E>(),
            "event type mismatch: source uses {}, subscribe call used {}",
            adapter.event_type_name, std::any::type_name::<E>(),
        );

        // Allocate a subscription id and store the user's callback on the
        // UI side, indexed by id. The stored closure downcasts &dyn Any
        // back to &E and invokes the user's F.
        let sub_id = self.app_state.allocate_subscription_id();
        let stored_callback: Box<dyn Fn(&dyn Any)> = Box::new(move |event_any| {
            let event = event_any.downcast_ref::<E>()
                .expect("event type mismatch — framework bug");
            callback(event);
        });
        self.app_state.subscription_callbacks.insert(sub_id, stored_callback);

        // Build a Send wrapper that the source will invoke from its publisher
        // thread. The wrapper carries only the sub_id (Copy) and the proxy
        // (Send), boxes the event as Any+Send, and posts to the UI thread.
        let proxy = self.app_state.proxy.clone();
        let wrapper: Arc<dyn Fn(Box<dyn Any + Send>) + Send + Sync> =
            Arc::new(move |erased_event| {
                proxy.post_subscription_event(sub_id, erased_event);
            });

        let handle = (adapter.subscribe_fn)(Box::new(origin), wrapper);

        // Register the subscription with the current widget's cleanup scope.
        self.current_widget_scope().add_subscription(sub_id, handle);
    }
}
```

The user's callback `F: Fn(&E)` is **not** required to be `Send`. It runs on the UI thread, where it is free to touch `Signal<T>`, `Rc<T>`, and any other UI-thread-only state. The `Send` boundary is crossed inside the framework's wrapper, which carries only `(SubscriptionId, Box<dyn Any + Send>)` across threads — nothing else.

#### 9.4.4 Runtime Flow

What happens when a backend event arrives, end to end:

1. A backend operation (controller call, transaction commit, file watcher tick) publishes an event. For Qleany, this is `event_hub.send(Event { origin, ids, data })`.

2. The hub's internal dispatch finds matching subscribers and invokes their callbacks. For Qleany, this happens on the EventHubClient's background thread that drains the flume channel.

3. One of those callbacks is the framework's wrapper, registered by `BuildContext::subscribe_event`. The wrapper boxes the event as `Box<dyn Any + Send>` and calls `proxy.post_subscription_event(sub_id, erased_event)`.

4. The proxy sends `AppEvent::SubscriptionEvent { sub_id, event }` through the winit event loop's user-event channel. winit wakes the UI thread.

5. `FernAppHandler::user_event` receives the AppEvent, looks up the subscription's UI-side callback in `subscription_callbacks` by `sub_id`, downcasts the boxed event to `&E`, and invokes the user's closure.

6. The user's closure runs on the UI thread. It can mutate signals, call other UI-thread methods, anything a normal widget callback can do. The mutated signals trigger the binding system, marking dependent widgets dirty.

7. The framework requests a redraw. The next frame paints the updated widgets.

The only thing crossing the thread boundary is the boxed event plus the subscription id. The user's callback never crosses threads. The widget's signals never cross threads. The `Send` constraints in the trait are not constraints on application code — they are constraints on the framework's internal plumbing, and they are satisfied automatically by the wrapping that `subscribe_event` performs.

#### 9.4.5 Lifecycle and Cleanup

Subscriptions are scoped to widget lifetime. When a widget is destroyed (removed from the arena, replaced by a different widget at the same id, or the entire window closes), the framework iterates that widget's subscription scope and tears down each subscription in a specific order:

1. **Drop the `SubscriptionHandle` first.** The handle's `Drop` impl, defined by the event source implementation, removes the subscriber entry from the source's internal registry. After this point, the source will not invoke this subscription's wrapper for any newly-published events.

2. **Remove the UI-side callback second.** The framework removes the entry from `subscription_callbacks` keyed by the subscription id. After this point, even in-flight events that were already in the proxy queue (delivered between steps 1 and 2) will fail their lookup in `user_event` and be silently dropped.

The ordering matters because of in-flight events. An event published just before the widget is destroyed might already be sitting in the winit event queue when the framework starts cleanup. With the ordering above, that in-flight event is delivered to `user_event`, the lookup succeeds (the callback is still in the map), the user's closure runs one last time. This is correct: the widget existed when the event was published, and the closure executes against the still-valid widget state. Reversing the order would mean the in-flight event finds no callback and is silently dropped, even though the event predates the destruction. The correct ordering preserves causality.

If an event arrives after both steps (e.g., the source publishes from another thread between cleanup and the next event-loop tick), the lookup in `user_event` returns `None` and the event is silently dropped. This is also correct: the widget no longer exists, so there is no one to deliver to.

The cleanup integration uses the same per-widget cleanup scope mechanism that already handles signal observers, animation effects, timers, focus registrations, and accessibility nodes. Subscriptions add one more list to the scope; they do not introduce a new lifecycle concept.

#### 9.4.6 Multiple Instances

The pattern that defeats global-property approaches is multiple simultaneous instances of the same conceptual widget — three open entity editors, four document viewers, a dozen inspector panels, each editing a different thing and each needing its own subscription state. Direct subscription handles this without any coordination:

```rust
struct EntityEditor {
    entity_id: EntityId,
    name: Signal<String>,
    only_for_heritage: Signal<bool>,
    // ... other per-instance state
    app_context: Arc<AppContext>,
}

impl Widget for EntityEditor {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let entity_id = self.entity_id;
        let name = self.name.clone();
        let app_context = self.app_context.clone();

        ctx.subscribe_event(
            Origin::DirectAccess(DirectAccessEntity::Entity(EntityEvent::Updated)),
            move |event: &Event| {
                // Only react if the updated entity is this editor's entity.
                if event.ids.contains(&entity_id) {
                    if let Ok(Some(dto)) = entity_commands::get_entity(&app_context, &entity_id) {
                        name.set(dto.name);
                    }
                }
            },
        );
        // ... rest of build
    }
}
```

Three open editors construct three `EntityEditor` widgets, each with its own `entity_id`. Each calls `subscribe_event` once during its `build()`, registering a closure that captures *its own* entity_id and *its own* name signal. When an `Entity Updated` event arrives with `ids: [7]`, all three subscriptions fire (the source dispatches to all subscribers of that origin); the editor whose captured `entity_id == 7` updates its signal, the other two return early. Each editor's state is independent, no "current editor" indirection exists, and no global state mediates between them.

The filtering happens inside the callback. For small numbers of subscribers (a typical application has dozens, not thousands), this is efficient enough — the cost of an early-return check against a captured `EntityId` is negligible. Applications with very high subscription density could benefit from a richer dispatch with origin-plus-key filtering, but that is a future optimization, not a current requirement.

#### 9.4.7 Comparison with Typed Commands

The two layers serve different purposes and should not be confused:

| | Typed commands (§9.2) | Event subscription (§9.4) |
|---|---|---|
| Direction | Widget → application | External source → widget |
| Semantic | "I want X to happen" | "X has happened" |
| Routing | Central handler interprets | Local callback reacts |
| Discoverability | Command palette enumerates | Subscription is in `build()` |
| Recordability | Yes (the command is a value) | No (event is a fact) |
| Undo participation | Yes | No |
| Best for | Save, Open, format, navigate | DB changes, file watchers, network responses, hub events |

A typical interaction uses both layers. The user clicks Save. The button emits `Cmd::SaveDocument` (typed command, recordable, scriptable). The application's command handler calls a controller. The controller writes to disk and publishes a `DocumentSaved` event on the event source. A status bar widget subscribed to `DocumentSaved` updates a "Saved at HH:MM" label. The typed command captured the user's intent; the subscription captured the resulting fact. Each layer handled what it was designed for.

The wrong pattern is to define `Cmd::DocumentSaved` and route the backend event through the command handler. This treats the event as if it were intent — but no user intended for "DocumentSaved" to happen as a separate decision; it is a consequence of the SaveDocument intent that has already been processed. Routing it through the command layer adds a forwarder, a match arm, and removes nothing.

#### 9.4.8 Testing Subscriptions

A widget that subscribes to an event source can be tested without running the real source. The test provides a mock implementation of `EventSource` that lets the test inject events directly:

```rust
struct MockEventSource {
    subscribers: Mutex<Vec<(MockOrigin, Arc<dyn Fn(MockEvent) + Send + Sync>)>>,
}

impl EventSource for MockEventSource {
    type Origin = MockOrigin;
    type Event = MockEvent;

    fn subscribe(&self, origin: Self::Origin, callback: Arc<dyn Fn(Self::Event) + Send + Sync>)
        -> SubscriptionHandle
    {
        self.subscribers.lock().unwrap().push((origin, callback));
        SubscriptionHandle::new(())  // no real cleanup needed in tests
    }
}

impl MockEventSource {
    fn publish(&self, origin: MockOrigin, event: MockEvent) {
        for (sub_origin, cb) in self.subscribers.lock().unwrap().iter() {
            if *sub_origin == origin {
                cb(event.clone());
            }
        }
    }
}
```

The test constructs a `MockEventSource`, registers it with a headless `FernApp`, instantiates the widget, calls `publish` to inject an event, and asserts that the widget's signals updated as expected. The test runs in milliseconds, has no threads, no winit, no database — just the widget logic and the event source contract.

This is the same mockability that the V2 widget model gives to widget unit testing in general (Section 28). The `EventSource` trait is no different from any other trait that the application can substitute for tests.

#### 9.4.9 Constraints and Future Extensions

**One source per application.** The current design supports a single registered event source per `FernAppBuilder`. For the cases FernUI targets (one Qleany backend, one custom message bus, one Tokio channel — never more than one of these in the same app), this is sufficient. If a future application needs multiple sources of distinct types, the framework can grow `event_source_named("primary", source_a)` and `subscribe_event_on::<S>("primary", origin, callback)` as a forward-compatible extension. The single-source API would remain as the default.

**Send + Clone on Event.** The event payload must be `Clone` (multiple subscribers, plus the wrapper boxes the event into `Any`) and `Send` (crosses the thread boundary to the UI thread). For event types that are expensive to clone, the source can publish `Arc<RealEvent>` as its `Event` type — the trait's bound is satisfied by `Arc`, and the per-subscriber clone is just an Arc refcount bump. This is a per-source decision, not a framework concern.

**Backpressure.** Events published faster than the UI thread can drain them will accumulate in winit's user-event queue. There is no flow control in the framework. For low-rate sources (Qleany emits events on user-driven operations, hundreds per minute at most), this is fine. For high-rate sources (telemetry streams, log tailers), the source should debounce or batch on its own side before publishing. Worth noting in source-specific documentation, not a framework problem.

**Re-entrancy.** A subscription callback is free to mutate signals, emit typed commands, or call methods that publish further events on the source. The chain runs to completion within `user_event`, then a redraw is requested. There is no deadlock risk because everything goes through the event-loop queue, but deeply chained subscriptions can be hard to reason about. Standard advice: keep callbacks short, do the work, return.

### 9.5 Application State and `BuildContext::app_state`

Many applications have state that does not belong to any particular widget but that many widgets need to read: the current theme, the active locale, the current workspace identifier, the current document handle, an online/offline status flag, a global loading indicator. The defining property is "there is exactly one of these, and many things observe it." A handful of places mutate it; many places read it.

The natural place to put such state is a struct constructed at application startup and shared with all widgets that need it. The naive approach — pass the struct as a constructor argument to the root widget, which passes it to its children, which pass it to theirs — is the React "prop drilling" problem: every intermediate widget must accept and forward state it does not itself use, just so distant descendants can reach it. For a widget tree five or ten levels deep, this is unworkable.

FernUI's solution is `BuildContext::app_state<T>()`: a typed, depth-independent way for any widget to retrieve a value the application registered at startup. The framework provides the lookup mechanism; the application defines the shape of the value.

#### 9.5.1 The API

The framework adds two methods, one on the builder and one on `BuildContext`:

```rust
impl FernAppBuilder {
    /// Register an application-defined state value of type T.
    /// The value will be available to every widget via `BuildContext::app_state::<T>()`.
    /// Multiple values of distinct types can be registered; each type T may
    /// be registered at most once.
    pub fn app_state<T: 'static>(mut self, value: T) -> Self {
        self.app_state_registry.insert(TypeId::of::<T>(), Box::new(value));
        self
    }
}

impl BuildContext<'_> {
    /// Retrieve the application state of type T, or None if no value of that
    /// type was registered. The returned reference borrows from the framework
    /// for the duration of the build pass.
    pub fn app_state<T: 'static>(&self) -> Option<&T> {
        self.app_state_registry
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref::<T>())
    }
}
```

Internally the registry is a `HashMap<TypeId, Box<dyn Any>>` populated at builder time and propagated to every `BuildContext` constructed during widget building. There is no per-widget state, no lifecycle, no notification — the registry is read-only after `FernAppBuilder::run` is called. The TypeId lookup ensures type safety: a request for `app_state::<Foo>()` can only return a `Foo` that was registered, and the compiler enforces the type at the call site.

The "at most one per type" constraint is the discipline that makes the API safe. If two distinct values of the same type were registered, lookups would be ambiguous and one would silently win. Forcing distinct types means each registered value has a clear identity. Applications that want to register multiple values of the same logical kind (two separate workspace handles, say) can wrap each in a newtype: `struct PrimaryWorkspace(WorkspaceHandle)` and `struct SecondaryWorkspace(WorkspaceHandle)` are distinct types from the registry's perspective.

#### 9.5.2 The Canonical Pattern

The application defines a struct that holds whatever app-wide state it needs. Reactive state goes in `Signal<T>` fields; immutable services go in plain fields:

```rust
struct AppGlobals {
    // Reactive state — widgets bind to these signals.
    current_workspace: Signal<Option<WorkspaceId>>,
    current_document: Signal<Option<DocumentId>>,
    is_loading: Signal<bool>,
    online_status: Signal<OnlineStatus>,

    // Services — accessed but not observed.
    app_context: Arc<AppContext>,
    config: Rc<AppConfig>,
}

impl AppGlobals {
    fn new(app_context: Arc<AppContext>, config: AppConfig) -> Rc<Self> {
        Rc::new(Self {
            current_workspace: Signal::new(None),
            current_document: Signal::new(None),
            is_loading: Signal::new(false),
            online_status: Signal::new(OnlineStatus::Unknown),
            app_context,
            config: Rc::new(config),
        })
    }
}
```

The struct is wrapped in `Rc` so that widgets can hold cheap clones, and registered at startup:

```rust
fn main() {
    let app_context = Arc::new(AppContext::new());
    let config = AppConfig::load();
    let globals = AppGlobals::new(app_context.clone(), config);

    FernAppBuilder::new()
        .theme(Theme::light_default())
        .event_source(event_hub_client)
        .app_state(globals.clone())
        .root(|tree| tree.add(App::new()))
        .run();
}
```

Any widget at any depth can read it without receiving it as a constructor argument:

```rust
struct WorkspaceLabel;

impl Widget for WorkspaceLabel {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let globals = ctx.app_state::<Rc<AppGlobals>>()
            .expect("AppGlobals not registered");

        let label = globals.current_workspace.map(|opt| match opt {
            Some(id) => format!("Workspace #{}", id),
            None => "No workspace".to_string(),
        });

        vec![ctx.add(TextWidget::new("").bind_text(label))]
    }
}
```

`WorkspaceLabel` could be ten levels deep in the tree. Its parent does not need to know that it reads workspace state. Its grandparent does not need to forward an `AppGlobals` reference. The widget reaches into `BuildContext::app_state` directly, retrieves the typed reference, and binds to the relevant signal. The chain of intermediate widgets is unaffected.

#### 9.5.3 Mutation Patterns

App state mutations come from three places, each with a clear pattern:

**Direct mutation from the UI thread.** An `Action` handler, an `on_activate_fn` closure, or any other UI-thread code that holds a reference to the globals struct mutates the signal directly:

```rust
ctx.register_action(Action::new("app.set_loading").on_invoke(
    move |intent, _ctx| {
        if let Some(AppIntent::SetLoading(loading)) = AppIntent::from_intent(intent) {
            globals.is_loading.set(*loading);
        }
    },
));
```

The closure captures `Rc<AppGlobals>` from the enclosing scope. The signal mutation propagates through the binding system as usual; widgets that observe `is_loading` repaint on the next frame.

**From an `EventSource` subscription.** A widget subscribes to a backend event and updates a global signal in the callback (which runs on the UI thread per Section 9.4):

```rust
impl Widget for ConnectionMonitor {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let globals = ctx.app_state::<Rc<AppGlobals>>().unwrap().clone();

        ctx.subscribe_event(
            Origin::Network(NetworkEvent::StatusChanged),
            move |event: &NetworkEvent| {
                globals.online_status.set(event.new_status);
            },
        );

        vec![]  // ConnectionMonitor has no visual; it just bridges events to globals.
    }
}
```

This is the canonical pattern for "background source updates app-wide state." The widget that owns the subscription captures the globals Rc, the callback runs on the UI thread, the signal mutation is direct.

**Lower-level cross-thread updates.** If a global needs to be written from a background thread without going through an event source — for example, a network throughput counter updated by a background task at high frequency — the application stores an `Arc<AtomicU64>` in the globals struct alongside the `Signal<T>` fields and uses a frame-tick effect to copy the atomic value into the signal on each frame. This is application-level engineering, not a framework concern. For the vast majority of cases, the EventSource path is the right answer.

#### 9.5.4 Multiple State Structs

The "one value per type" rule means an application can register multiple state structs as long as their types are distinct:

```rust
FernAppBuilder::new()
    .app_state(Rc::new(AppGlobals::new(...)))
    .app_state(Rc::new(EditorGlobals::new(...)))
    .app_state(Rc::new(NetworkGlobals::new(...)))
    .app_state(Arc::new(MetricsCollector::new()))
    .root(...)
    .run();
```

Each struct has a clear scope: app-wide state in `AppGlobals`, editor-specific state in `EditorGlobals`, network state in `NetworkGlobals`, metrics in `MetricsCollector`. Widgets retrieve whichever they need:

```rust
let editor = ctx.app_state::<Rc<EditorGlobals>>().unwrap();
let metrics = ctx.app_state::<Arc<MetricsCollector>>().unwrap();
```

This is more disciplined than a single monolithic globals struct, because it forces the application to think about which state is genuinely app-wide versus subsystem-scoped, and the type system enforces the separation. A widget that asks for `EditorGlobals` but the application registered only `AppGlobals` gets a clear `None` at the lookup site, not an obscure runtime error.

#### 9.5.5 Testing

Headless tests construct the same state structs and register them with a headless app:

```rust
#[test]
fn workspace_label_displays_current_workspace() {
    let test_globals = Rc::new(AppGlobals::new_for_test());
    test_globals.current_workspace.set(Some(WorkspaceId(42)));

    let mut headless = FernAppBuilder::new()
        .app_state(test_globals.clone())
        .build_headless();

    let widget_id = headless.tree.add(WorkspaceLabel);
    headless.tree.layout(SizeProposal::exact(200.0, 50.0));

    let frame = headless.tree.render();
    assert!(frame.contains_text("Workspace #42"));
}
```

The test controls the app state directly, the widget reads from the same registry the framework would provide in a real run, and the rendered output reflects the test-injected values. No widget code needs to change between production and test — both paths read from `BuildContext::app_state`, the only difference is what was registered at builder time.

#### 9.5.6 What `app_state` Is Not

`app_state` is a depth-independent value lookup. It is not:

- **A reactive primitive.** The registry itself is not reactive — registered values cannot be replaced after `run()` is called, and there is no notification when the registry changes (it doesn't). Reactivity comes from the `Signal<T>` fields *inside* the registered struct, not from the registry.
- **A message bus.** Widgets do not communicate through `app_state`. They communicate through the signals they share via `app_state`. The registry holds the values; the values hold the signals; the signals do the propagation.
- **A dependency injection container.** It does not resolve construction order, manage lifetimes, or inject services into constructors. It just lets a widget retrieve a value the application chose to share.
- **Cross-thread storage.** The registry is read-only at runtime and accessed only on the UI thread. Background threads cannot read or write `app_state`. To share state across threads, the application uses `Arc<T>` as the registered value (so the Arc can be cloned into widget closures that capture it for use by background-spawned tasks they trigger), and synchronizes the underlying state via the EventSource path or atomic primitives.

The framework provides exactly one thing: a typed bag of values reachable from any `BuildContext`. Everything else — the shape of the values, how they are mutated, how they propagate, how they are tested — is the application's responsibility. This is the right level of abstraction. The framework provides the mechanism, the application chooses the policy.

### 9.6 Deferred Operations via EventContext

Event handlers run while the widget tree is mid-dispatch. Mutating the tree, changing focus, or showing an overlay synchronously during a handler would invalidate iterators, borrow guards, or the dispatch state. Instead, handlers enqueue operations on `EventContext` that are applied after dispatch completes, before the next frame.

The deferred operations supported by `EventContext` are: `emit(command)` for typed application commands; `activate(id)` / `deactivate(id)` / `destroy(id)` for dormancy changes; `request_focus(id)` for programmatic focus transfer (used when opening menus, dialogs, or overlay content that should receive keyboard input); `show_overlay(request)` for displaying an overlay; `dismiss_all_overlays()` for closing the entire overlay stack (used when navigating between menu bar entries); `cancel_delayed_overlay(content_id)` for aborting a pending delayed overlay (used when a submenu hover ends before the open delay elapses); `synthetic_click(id)` for triggering a click from keyboard Enter activation; `capture_pointer()` / `release_pointer()` for drag operations; `set_theme(theme)` and `set_locale(locale)` for runtime configuration changes that update the tree-level reactive `Signal<Theme>` / `Signal<Option<String>>` and dirty-mark every node — no rebuild, and interaction state (focus, scroll, hover) is preserved.

These operations compose: a menu bar Left-arrow handler can dismiss the current overlay, request focus on the trigger, and show the next overlay in a single handler call, with all three operations applied atomically after the handler returns.

---

## 10. Gesture Recognition

FernUI uses a UIKit-style gesture recognizer model: composable state machines that monitor the raw pointer event stream and emit recognized gestures when patterns complete. Built-in recognizers include tap, double-tap, triple-tap, long-press, drag, and swipe; pinch and rotation arrive pre-recognized from the OS on desktop (winit's `TouchpadMagnify` / `RotationGesture`). A `GestureArena` arbitrates competing recognizers with cooperative-or-reset semantics.

Recognizers are pure platform-free state machines, trivially unit-testable. In the V1 design, widget authors instantiated them per-widget and plumbed the raw events in by hand. In V2, attaching a handler (`.on_tap(...)`, `.on_long_press(...)`, etc.) auto-wires the relevant recognizer on the node's gesture arena — widget authors never touch recognizer state machines directly.

Full catalogue, arena rules, and the `DragPhase` / `PinchPhase` / `SwipeDirection` handler signatures live in [`events-and-gestures.md`](events-and-gestures.md) §4.

---

## 11. Actions, Intents, and Shortcuts

The V1 "Keyboard Shortcuts" section described a `ShortcutMap<C: AppCommand>` — a bidirectional map between key chords and application command variants, consulted during the preview pass, rebindable at runtime. That mechanism is gone; its three-way successor is the **Action / Intent / Shortcut** pipeline in fern-core. The dedicated reference is [`shortcut-intent-action.md`](shortcut-intent-action.md); this section records the "why" for anyone reading top-to-bottom.

Three concerns were conflated in V1 and are separate layers in V2:

1. **The operation the application can perform** is a typed intent — an application-defined enum variant ("save this document," "open file X," "scroll this view by N"). Intents are values: serializable, recordable, replayable, testable. `#[derive(IntentKind)]` wires the enum into a name-keyed DTO bridge. See §9.2 for the rationale behind typed operations generally.
2. **The handler that reacts to an operation** is an `Action` — a widget-owned, name-keyed callback registered in a widget's `build()`. Actions compose: a button emits an intent; the intent walks source → root; *every* `Action` along the way matching the intent's name fires. An ancestor can intercept with a more specific action; a root can provide the default. `Action::enabled_when(Signal<bool>)` gates activation without tree rebuild.
3. **The trigger that fires an operation** is a `Shortcut` — a named keystroke (or two) that produces an intent. Users rebind the keystroke; the widget tree doesn't change. `ShortcutRegistry` holds the two-layer defaults + user overrides with graveyard semantics (explicitly-unbound-by-user defaults stay unbound across registry replays). A menu item or tooltip that references a shortcut by id re-renders on rebind via the registry's `version()` signal — no manual wiring.

The three layers buy what V1's single `ShortcutMap` couldn't cleanly express: a shortcut can fire the same intent as a button without any coupling (press Ctrl+S or click the toolbar save button — same code path), an action can be moved up or down the tree to change scope without editing widgets, and shortcut labels on menu items track rebinds without the widget knowing it's bound to a shortcut.

Shortcut labels remain translatable and platform-aware — "Ctrl+S" / "⌘S" / "Strg+S" — via a `ShortcutFormatter` hook (still pending implementation as of April 2026; see [`fern-ui-milestones.md`](fern-ui-milestones.md) M7 remaining work). Shortcuts bind to logical keys (character produced), not scancodes, so Ctrl+Z works on AZERTY keyboards where the Z key is physically elsewhere.

Working demo: [`examples/shortcuts_demo`](../examples/shortcuts_demo/src/main.rs).

---

## 12. Internationalization

Internationalization is a Milestone 7 deliverable, scheduled before Milestone 8 (rich text editor) so that editor UI labels are translatable from the start. The design is opinionated: FernUI commits to Fluent as the translation format, to compile-time key checking via a custom procedural macro, and to RTL support via direction-aware layout. The framework owns the translation lookup mechanism — there is no abstract `Translator` trait — because Fluent is good enough that pluggability would be cost without benefit. The application provides .ftl files, the framework provides the `tr!` macro, the `LocalizedString` type, the bundle management, and the layout direction signal.

### 12.1 Foundation: fluent-rs Plus a Custom Validating Macro

FernUI's i18n is built on two pieces:

1. **`fluent-bundle` and `fluent-syntax`** (the runtime crates from the `fluent-rs` project) — used directly for runtime translation lookup. Bundles are constructed from `.ftl` resource strings at startup, store the parsed AST, support adding more resources at runtime, and resolve message keys with arguments via `format_pattern`. This is the standard fluent-rs usage pattern, with no wrapper layer.

2. **A custom procedural macro `tr!`** — defined in `fern-i18n-macros`, it provides compile-time key checking by reading the application's source-language `.ftl` file at macro expansion time, parsing it via `fluent-syntax`, and validating that every `tr!(...)` call references a key that exists with arguments that match. The macro emits code that calls into the runtime FluentBundle at execution time. The compile-time validation and the runtime lookup are two separate paths: the macro's job is to catch bugs at build time, the runtime's job is to actually resolve translated strings.

This design is deliberately different from the `fluent-static` crate (which generates pure Rust functions inlining the translation logic at build time, bypassing FluentBundle entirely at runtime). `fluent-static` would give us compile-time checking but would make runtime override and hot-reload structurally impossible — there is no FluentBundle to swap. The custom macro keeps compile-time checking while leaving the runtime path open for hot-reload, partial translations, and per-key fallback. The trade-off is that FernUI maintains its own proc macro (~500–1000 lines of Rust) instead of leveraging `fluent-static`'s code generator. The trade-off is acceptable because the macro's scope is well-bounded and the alternative loses essential features.

#### 12.1.1 Compile-Time Key Validation

The application designates one `.ftl` file as the *source language* (typically `en-US.ftl`). The `tr!` proc macro reads this file at every compilation, parses it via `fluent-syntax`, and builds a key/argument-signature map. When the macro encounters `tr!(welcome_title())` in source code, it checks the map: does a message named `welcome-title` exist? Does it take zero arguments? If yes, the call expands to runtime resolution code. If no, the macro fails with a compile error pointing at the `tr!` invocation:

```
error: translation key `welcome-title` not found in en-US.ftl
  --> src/widgets/welcome.rs:42:31
   |
42 |     TextWidget::new(tr!(welcome_title()))
   |                         ^^^^^^^^^^^^^
   |
   = help: did you mean `welcome-greeting`?
   = note: looked in /home/cyril/atelier/locales/en-US.ftl
```

The macro also validates argument counts and names. `tr!(welcome_greeting())` when `welcome-greeting = Hello, { $name }!` is defined fails with a clear error about the missing `name` argument. `tr!(welcome_greeting(name = "Alice", extra = "ignored"))` fails because `extra` is not a variable in the message.

The macro reads the .ftl file via standard filesystem I/O during expansion, with `proc_macro::tracked_path::path` (stable since Rust 1.83) to inform `cargo` that the macro depends on the file so the crate is rebuilt when the .ftl changes. The path defaults to `$CARGO_MANIFEST_DIR/locales/en-US.ftl`; applications can override it via a crate-level attribute (see §12.2).

The map is built once per crate per compilation by parsing the .ftl file once and caching the result in a thread-local for the duration of the compilation. Subsequent `tr!` invocations in the same crate use the cached map. Build-time cost is negligible — parsing a typical .ftl file takes single-digit milliseconds.

#### 12.1.2 Only the Source Language Is Required at Build Time

Because the proc macro validates against one `.ftl` file (the source language), and because the runtime path uses real FluentBundle lookup with per-key fallback, **only the source language must be present at build time**. Other locales are runtime data, may be missing or partially translated, and fall back per-key to the source language when a key is absent.

Adding a new translatable string is a one-step operation: add it to `en-US.ftl`. Code compiles immediately because the proc macro now sees the new key. The application runs in any locale immediately, with the new string rendering in the source language wherever it has not been translated yet. There is no broken state where a French build fails because someone added a key the translator has not reached, and no requirement for translators to keep every locale's .ftl in lockstep with code changes.

The implication for development workflow: **modifying an existing message is a breaking change; adding a new message is not.** Changing `welcome-greeting = Hello, { $name }!` to `welcome-greeting = Hello, { $name } from { $city }!` adds a `city` parameter to the validated signature, breaking every `tr!(welcome_greeting(...))` call site at compile time. This is correct behavior — the message now needs a city — but contributors should understand that message signatures are semi-stable and changes propagate through the codebase. Adding `welcome-farewell = Goodbye!` is a pure addition and breaks nothing.

### 12.2 The `tr!` Macro

The widget-facing API is the `tr!` macro, which produces a reactive `LocalizedString`:

```rust
TextWidget::new(tr!(welcome_title()))
TextWidget::new(tr!(welcome_greeting(name = user.name.clone())))
TextWidget::new(tr!(unread_count(count = unread)))

// Nested message modules for feature-organized applications:
TextWidget::new(tr!(auth::login_title()))
Button::new(tr!(editor::save_button()))
TextWidget::new(tr!(settings::display::resolution_label(width = w, height = h)))
```

The macro accepts a path-and-arguments expression: a function-call-like syntax with an identifier (or path of identifiers separated by `::`) followed by named arguments in parentheses. The path determines which message key to look up; the named arguments correspond to Fluent message variables.

#### 12.2.1 What the Macro Expands To

`tr!(welcome_title())` expands to roughly:

```rust
fern_i18n::localized(move || {
    fern_i18n::resolve_message("welcome-title", &[])
})
```

`tr!(welcome_greeting(name = user.name.clone()))` expands to roughly:

```rust
fern_i18n::localized({
    let name = user.name.clone();
    move || {
        fern_i18n::resolve_message(
            "welcome-greeting",
            &[("name", fern_i18n::FluentValue::from(name.clone()))],
        )
    }
})
```

`fern_i18n::resolve_message` is the runtime entry point. It looks up the active FluentBundle (per the locale resolution from §12.5), calls `bundle.get_message("welcome-greeting")`, calls `bundle.format_pattern(...)` with the arguments, and returns the formatted String. If the message is missing in the active locale, the bundle's per-key fallback to the source language takes over. If it is missing in both, the framework returns the literal key as a placeholder and logs a warning (this should be impossible because the macro validated the key at compile time — if it happens, the source .ftl was modified between build and run).

The arguments are captured by `let` binding before the closure, then cloned inside the closure on each invocation. This is the same `.clone()` pattern that the original macro design used: the closure may be called many times over the LocalizedString's lifetime (once on initial resolution, then on every locale change and hot-reload), and the captured arguments must survive multiple invocations. Fluent's argument types (strings, numbers, booleans) are all cheap to clone.

The macro's compile-time validation happens *before* expansion. If the validation fails, the macro produces a `compile_error!` instead of the expanded code, and the user sees an error at the `tr!` invocation site rather than a misleading error at the runtime resolution site.

#### 12.2.2 Configuring the Source File Path

By default, the macro auto-detects the source file at one of two locations in the consuming crate's `CARGO_MANIFEST_DIR`:

1. **`locales/en-US/`** — if this path exists as a directory, the macro enters *directory mode* and walks every `.ftl` file underneath it (see §12.2.3).
2. **`locales/en-US.ftl`** — otherwise the macro reads this single file in *file mode*.

Auto-detection prefers directory mode. An application that starts with a flat `locales/en-US.ftl` file and later promotes it to a `locales/en-US/` directory layout does not need to change any configuration — the macro picks up the new layout on the next compilation.

For tests and other situations where the source `.ftl` lives outside the default path, the macro honors two environment variables read at compile time:

- **`FERN_I18N_SOURCE_DIR`** — forces directory mode with the given path (relative to `CARGO_MANIFEST_DIR`, or absolute).
- **`FERN_I18N_SOURCE_PATH`** — forces file mode with the given path (same resolution rules).

The env-var precedence is `FERN_I18N_SOURCE_DIR` > `FERN_I18N_SOURCE_PATH` > auto-detected directory > auto-detected file. Both variables are primarily used by the framework's own `trybuild` tests, which need to point the macro at a fixture `.ftl` file in the test's source tree rather than the consuming crate's real `locales/` directory. Applications should not set these variables in production builds — the default auto-detection covers the normal case.

The architecture deliberately uses env vars rather than a crate-level attribute (like `#![fern_i18n::source_locale(...)]`) because env vars compose cleanly with `cargo test`, `trybuild`, and other tooling that already expects to set compile-time environment through `build.rs` or shell-level overrides. A crate attribute would require the proc macro to parse attribute arguments at a different layer from where it reads the file, and would not help the framework's own internal test fixtures.

#### 12.2.3 Nested Message Modules

Larger applications organize their .ftl files by feature rather than dumping everything into one file. The proc macro handles this through a directory hierarchy:

```
locales/en-US/
  main.ftl           → tr!(welcome_title())
  auth.ftl           → tr!(auth::login_title())
  editor.ftl         → tr!(editor::save_button())
  settings/
    display.ftl      → tr!(settings::display::resolution_label())
    keyboard.ftl     → tr!(settings::keyboard::shortcut_label())
```

Each file's contents are parsed as a separate Fluent resource, and the proc macro walks the directory at compile time to build a single merged key map. The directory walk is recursive — nested subdirectories like `settings/` produce multi-level paths.

**Key encoding from Rust path to Fluent id.** Fluent's message-id grammar is `[a-zA-Z][a-zA-Z0-9_-]*`, which does not allow `::` and does not naturally express hierarchy. The macro encodes Rust paths into flat Fluent keys by:

1. Replacing `_` with `-` within each path segment (matching Fluent's preference for kebab-case).
2. Joining segments with `__` (double underscore) as the module separator.

So `tr!(auth::login_title())` looks up the Fluent key `auth__login-title`, and `tr!(settings::display::resolution_label())` looks up `settings__display__resolution-label`. The `__` separator is reserved — if a Rust path segment contains `__`, the macro rejects it at compile time with an explicit error pointing at the offending segment, because allowing it would let a single-segment path collide with a two-segment path after encoding.

**ASCII-only guard.** Rust identifiers permit Unicode (`tr!(héllo())` would parse), but Fluent message ids are ASCII-only per the grammar. The macro rejects non-ASCII segments at compile time with a clear error message, rather than letting them flow through to a confusing "key not found" error at runtime. Every segment must match `[a-zA-Z][a-zA-Z0-9_]*`.

At runtime, the FluentBundle for each locale is constructed by adding *multiple resources* — one per .ftl file in the locale's directory tree. FluentBundle natively supports multiple resources per bundle, and `resolve_message` looks up keys by their encoded name (`auth__login-title`) so that nested modules don't collide with each other.

The same nesting works on the runtime side: the application's `compile_in` slice (see §12.4) carries a list of resources per locale rather than a single concatenated blob, with each `.ftl` file becoming one entry. The `.ftl` files themselves use the encoded key form — authors write `auth__login-title = Log in` in the file, not `login-title` nested in some directory-implied scope. This keeps the runtime lookup a single flat HashMap probe and avoids ambiguity between flat and nested layouts.

#### 12.2.4 Why the `.clone()` on Arguments

The closure passed to `localized()` must be `Fn`, not `FnOnce`, because it may be called many times — once on initial resolution, then on every locale switch and every hot-reload. Capturing arguments by move and consuming them inside the closure body would make the closure `FnOnce`. The macro expands to a clone-on-call pattern: arguments are captured by `let` binding outside the closure (moving them once), then `.clone()`'d inside the closure body each time it runs.

For Fluent's argument types — strings (`String`), numbers (`i64`, `f64`), booleans, dates — cloning is cheap. The `.clone()` is invisible in performance terms. For the rare case where an application wants to pass a non-cloneable argument, the escape hatch is to call `localized(move || fern_i18n::resolve_message("key", &[...]))` directly without the macro, accepting that the resulting closure may be `FnOnce` and the LocalizedString cannot be hot-reloaded for that specific instance. Most applications never hit this.

#### 12.2.5 Compile-Time Fallback for Missing Runtime Bundles

The runtime `resolve_message` function returns the literal key as a placeholder when no `I18nManager` is installed on the current thread (typical in unit tests for lower-level widgets) or when the active locale's bundle is missing the key (possible only if the source `.ftl` was edited between the compile-time validation and the runtime execution). A literal key placeholder like `"welcome-title"` is not useful in either situation — tests want to see the real English text, and production deserves a meaningful fallback rather than a raw id.

The proc macro solves this by **reconstructing the source-language text at expansion time** and emitting it as an inline fallback. When parsing the source `.ftl`, the macro walks each message's pattern AST and records it as a list of `FallbackPart` nodes:

- `FallbackPart::Text(String)` for verbatim literal text from a Fluent `TextElement`.
- `FallbackPart::Var(String)` for a `{ $var }` substitution.

The macro expansion concatenates these parts into a String at runtime, pulling variable values from the already-captured argument bindings via `ToString`. The expansion looks roughly like:

```rust
::fern_i18n::localized({
    let name = user.name.clone();
    move || {
        let result = ::fern_i18n::resolve_message(
            "welcome-greeting",
            &[("name", FluentValue::from(name.clone()))],
        );
        if result == "welcome-greeting" {
            // The runtime returned the key literal — fall back to the
            // source-language text reconstructed at macro expansion time.
            let mut fallback = String::new();
            fallback.push_str("Hello, ");
            fallback.push_str(&name.to_string());
            fallback.push_str("!");
            fallback
        } else {
            result
        }
    }
})
```

**Not every pattern is eligible for fallback reconstruction.** The macro only emits a fallback for patterns composed entirely of literal text and simple `{ $var }` substitutions. Patterns that use Fluent selectors (`{ $count -> [one] ... [other] ... }`), plural rules, term references (`{ -brand-name }`), message references, or function calls set `fallback = None` at parse time, and the expansion omits the reconstruction branch. For those messages, the runtime's literal-key placeholder is returned as-is, because reproducing Fluent's selector resolution logic in macro-generated Rust code would double the runtime. The trade-off is acceptable: simple patterns cover the overwhelming majority of user-facing strings, and complex patterns almost always need a real runtime bundle to format correctly anyway.

This feature has two practical consequences:

1. **Widget-level unit tests work without installing an I18nManager.** A test that constructs a Button and checks its label text sees the English source text directly, without needing to set up locale resolution, bundle loading, or a thread-local install.
2. **Forgotten framework bundle registration is silent.** An application that uses fern-widgets but forgets to call `.framework_locales(fern_widgets::framework_locales())` still sees English accessibility labels — the proc macro's fallback takes over. The missing registration is a mild configuration smell, not a broken UI. Per §12.13.3, applications that need localized framework strings must still opt in explicitly.


### 12.3 The `LocalizedString` Type

`LocalizedString` is the developer-facing handle that the `tr!` macro produces. It packages a closure (the resolver) with everything needed to produce a reactive `Signal<String>` when bound to a widget. Critically, `LocalizedString` is *not* itself reactive — it is a reactive *recipe* that becomes a live binding only when the widget consumes it.

```rust
pub struct LocalizedString {
    resolver: Rc<dyn Fn() -> String + 'static>,
}

pub fn localized<F: Fn() -> String + 'static>(resolver: F) -> LocalizedString {
    LocalizedString {
        resolver: Rc::new(resolver),
    }
}

impl LocalizedString {
    /// Construct a non-translated literal. Used for debug labels, internal
    /// names, and other strings that are intentionally not localized. The
    /// resulting LocalizedString does not observe locale changes — its
    /// content is fixed for the lifetime of the application.
    pub fn literal(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            resolver: Rc::new(move || text.clone()),
        }
    }

    /// Convert this LocalizedString into a reactive `Signal<String>` that
    /// observes the framework's translation version and re-resolves on
    /// every locale change or hot-reload. Called by widget builder methods
    /// internally when binding a LocalizedString to a text-displaying widget.
    pub fn to_signal(&self) -> Signal<String> {
        let initial = (self.resolver)();
        let signal = Signal::new(initial);

        // Observe the framework's translation version Signal and re-resolve
        // on each increment. The version Signal is reached via a thread-local
        // populated by FernAppBuilder at startup; calling `to_signal()`
        // before the framework is initialized panics with a clear message.
        let version = i18n::current_version_signal();
        let resolver = self.resolver.clone();
        let target = signal.clone();
        version.observe(move |_| {
            target.set((resolver)());
        });

        signal
    }
}
```

The reactivity lives in `to_signal()`, not in the LocalizedString itself. Calling `to_signal()` reaches into a thread-local established by `FernAppBuilder` to find the current translation version Signal, registers an observer that re-runs the resolver on each version increment, and returns a `Signal<String>` that the binding system can consume normally. This is the same kind of thread-local pattern used by Rust's logger and panic hook — it is the standard way for a free function to reach a process-wide singleton without threading context through every call site. The architecture document calls this out explicitly so it is not mistaken for an oversight.

When the framework's i18n manager increments the translation version Signal (because the locale changed or a runtime override file was reloaded), every Signal produced by `to_signal()` re-runs its resolver and updates its cached String value. Any widget bound to that Signal repaints automatically through the existing binding system. The widget never knows the locale changed — it just sees its bound text update. This is the same reactive bridge pattern used elsewhere in the framework (Section 27.10's document version Signal for the rich text editor, Section 9.4's subscription event system): a non-reactive data source bridged into FernUI's reactive model via a version counter.

`LocalizedString` is the type that widget builder methods accept anywhere a translatable string is appropriate:

```rust
TextWidget::new(tr!(welcome_title()))                  // direct construction
Button::new(tr!(btn_save()))                           // button label
TextInput::new().placeholder(tr!(search_hint()))       // placeholder
ctx.tooltip(self_id, tr!(toolbar_save_tooltip()))      // tooltip text
```

Internally these constructors call `to_signal()` on the LocalizedString and bind the resulting Signal to the widget's text source. The developer never sees the conversion — they pass a `tr!(...)` and the widget displays a translated, reactive string.

**Untranslated literals must be explicit.** Widget constructors accept `LocalizedString`, not `&str`. There is no `From<&str> for LocalizedString` impl, and this is intentional: an automatic conversion would let untranslated literals slip through to the user interface without the developer noticing. The strict version forces every literal to be wrapped in `LocalizedString::literal("Debug Tools")` (for genuinely non-translated strings) or in a `tr!(...)` call against a key in `en-US.ftl` (for everything user-facing). The cost is one extra method call per literal; the benefit is that grep-ing the codebase for `LocalizedString::literal` finds every untranslated string in one pass, and untranslated user-facing text cannot be shipped accidentally.

For test code and prototype scaffolding where translation is overkill, `LocalizedString::literal` is the escape hatch. For production code, the linter can reject `LocalizedString::literal` outside of test modules if the team wants stricter enforcement.

### 12.4 I18nConfig

The application configures i18n at builder time. The compiled-in locales are loaded from `.ftl` files via `include_str!` and passed directly to the framework — no build script, no `OUT_DIR`, no generated files:

```rust
FernAppBuilder::new()
    .i18n(I18nConfig::new()
        .source_locale("en-US".parse().unwrap())
        .supported_locales([
            "en-US".parse().unwrap(),
            "fr-FR".parse().unwrap(),
            "es-ES".parse().unwrap(),
            "ar-SA".parse().unwrap(),
        ])
        .compile_in(&[
            ("en-US", &[include_str!("../locales/en-US.ftl")]),
            ("fr-FR", &[include_str!("../locales/fr-FR.ftl")]),
            ("es-ES", &[include_str!("../locales/es-ES.ftl")]),
            ("ar-SA", &[include_str!("../locales/ar-SA.ftl")]),
        ])
        .user_locale(settings.user_locale.clone())
        .auto_detect_os_locale(true)
        .fallback_locale("en-US".parse().unwrap()))
    .root(...)
    .run();
```

For nested .ftl directory layouts (per §12.2.3), each entry contains multiple resource strings:

```rust
.compile_in(&[
    ("en-US", &[
        include_str!("../locales/en-US/main.ftl"),
        include_str!("../locales/en-US/auth.ftl"),
        include_str!("../locales/en-US/editor.ftl"),
        include_str!("../locales/en-US/settings/display.ftl"),
    ]),
    ("fr-FR", &[
        include_str!("../locales/fr-FR/main.ftl"),
        include_str!("../locales/fr-FR/auth.ftl"),
        // editor.ftl missing — French translator hasn't gotten there yet.
        // The keys defined in en-US/editor.ftl will fall back to English at runtime.
    ]),
    // ...
])
```

For applications with many locales or many files per locale, a small declarative helper macro reduces repetition without introducing a build script:

```rust
.compile_in(compile_in_locales!(
    base = "../locales/",
    locales = ["en-US", "fr-FR", "es-ES", "ar-SA"],
    files = ["main.ftl", "auth.ftl", "editor.ftl", "settings/display.ftl"],
))
```

The `compile_in_locales!` macro expands at compile time to the same nested slice literal of `include_str!` calls. It is convenience sugar, not a different mechanism.

The configuration methods:

- **`source_locale(LanguageIdentifier)`** — the language the proc macro validates against. Defaults to `en-US`. This is the only locale that *must* be present at build time, and its `.ftl` file is the one the macro reads.
- **`supported_locales(impl IntoIterator<Item=LanguageIdentifier>)`** — the set of locales the application will accept at runtime. Used to validate user_locale and auto-detection results; locales outside this set fall back.
- **`compile_in(&'static [(&'static str, &'static [&'static str])])`** — the static slice of `(locale_tag, resource_contents_list)` pairs. Each entry is a locale paired with a list of one or more `&'static str` Fluent resources. A flat layout produces one resource per locale; a nested layout produces one resource per file. The slice is typically constructed inline with `include_str!` calls, requiring no build script.
- **`user_locale(Option<LanguageIdentifier>)`** — the explicit user choice from application settings. Highest precedence in resolution. `None` means the user has not made a choice (or has chosen "Use System Default").
- **`auto_detect_os_locale(bool)`** — defaults to `true`. When enabled, the framework reads the OS locale at startup via the `sys-locale` crate and matches it against `supported_locales`.
- **`fallback_locale(LanguageIdentifier)`** — the locale used when neither `user_locale` nor auto-detection produces a supported result. Defaults to `source_locale`.
- **`runtime_override(LanguageIdentifier, PathBuf)`** — replaces a compiled-in locale with a file from disk and watches the file for changes. Multiple overrides for multiple locales are supported by calling the method multiple times. See §12.6.

`compile_in` and `runtime_override` compose: the application bakes in all supported locales for production, and the translator (using a development build of the same binary) overrides one or more of them via CLI flags. The override always wins for the duration of the run, and the file watcher provides hot-reload as the translator edits the file.

### 12.5 Locale Resolution at Startup

The framework resolves the active locale once at startup, in a clear precedence order:

1. **User explicit choice.** If `user_locale` is `Some(loc)` and `loc` is in `supported_locales`, use it. The user's settings always win.

2. **OS auto-detection.** If `auto_detect_os_locale` is enabled, read the OS locale via `sys-locale::get_locale()`. Parse it as a `LanguageIdentifier` (handles both `fr_FR.UTF-8` Linux form and `fr-FR` BCP-47 form via `unic-langid`). Match against `supported_locales` with partial matching: a detected `fr-CA` matches a supported `fr` if no exact match exists.

3. **Fallback.** Use `fallback_locale`, which defaults to `source_locale`.

The resolution function:

```rust
fn resolve_initial_locale(config: &I18nConfig) -> LanguageIdentifier {
    if let Some(user) = &config.user_locale {
        if config.supported_locales.contains(user) {
            return user.clone();
        }
    }

    if config.auto_detect_os {
        if let Some(os_locale) = sys_locale::get_locale() {
            if let Ok(parsed) = os_locale.parse::<LanguageIdentifier>() {
                if config.supported_locales.contains(&parsed) {
                    return parsed;
                }
                if let Some(matched) = config.supported_locales.iter()
                    .find(|s| s.matches(&parsed, true, true)) {
                    return matched.clone();
                }
            }
        }
    }

    config.fallback_locale.clone()
}
```

The result is stored in the framework's `Signal<LanguageIdentifier>`, exposed to widgets via `BuildContext::locale() -> Signal<LanguageIdentifier>`. Most widgets do not need to observe this directly — they bind to LocalizedStrings which observe the version Signal internally. The locale Signal is useful for widgets that need to know *which* language is active rather than just resolving translated text: a "Language" menu that highlights the current selection, a date formatter that picks a calendar based on the locale, or a flag icon that displays the country associated with the active language. The detection happens once and does not change while the application runs — even if the OS locale changes mid-session, the application keeps its initial choice. This matches the behavior of native applications on every platform.

There are two distinct "no user choice" states the application might want to distinguish: "the user has never been asked" (first run, application might want to prompt "we detected French, is this correct?") and "the user explicitly chose Use System Default" (no prompt needed). This distinction lives in the application's settings layer, not the framework. The framework just sees `user_locale: Option<LanguageIdentifier>` and resolves accordingly.

**Runtime locale switches.** The user changes language via a settings menu by calling `framework.set_locale(new_locale)`. The framework validates against `supported_locales`, updates the locale signal, increments the translation version Signal (so all LocalizedStrings re-resolve), and — if the layout direction changed (LTR ↔ RTL) — triggers a composite rebuild because layout direction is a build-time decision. For LTR-to-LTR or RTL-to-RTL transitions, the reactive update via the version Signal is sufficient and no rebuild happens.

### 12.6 Runtime Override and Hot-Reload

The translator development workflow is the reason `runtime_override` exists. A translator runs:

```bash
atelier --translation-dev fr-FR=/path/to/fr-FR.ftl
```

The application's CLI parser collects all `--translation-dev` occurrences into a `Vec<(LanguageIdentifier, PathBuf)>` and feeds each one through to the I18nConfig:

```rust
for (locale, path) in &cli_args.translation_dev {
    config = config.runtime_override(locale.clone(), path.clone());
}
```

`runtime_override` performs three setup steps in sequence at builder time:

1. Records the path against the locale tag in the I18nConfig. The actual file load is deferred to `FernAppBuilder::run`, so a missing file at this stage is not an error.
2. At `run`, loads the .ftl file from the given path, parses it via `FluentResource::try_new`, and adds it to the FluentBundle for that locale (replacing whatever was compiled in for that locale, or augmenting it if the bundle is empty). If the file is missing or malformed at load time, the framework logs an error and falls back to the compiled-in version (if any) or to the source locale.
3. Starts a file watcher on the path using the `notify` crate. The watcher runs on a background thread and forwards file-changed events through the framework's EventSource mechanism (Section 9.4) so that they arrive on the UI thread as event subscriptions fire.

When the translator saves the file, the watcher fires, the framework's i18n manager (a small internal component subscribed to the watcher events) re-reads the file, parses it via `FluentResource::try_new`, replaces the corresponding bundle's contents, increments the translation version Signal, and the binding system propagates the change to every LocalizedString currently in the widget tree. The translator sees their change reflected in the running application within ~100ms (file watcher latency plus event loop wake plus signal propagation). No restart, no rebuild, no command rerun.

The hot-reload path works because the runtime side is plain `fluent-bundle` lookup. There is no compiled-in translation logic to bypass — the FluentBundle holds the parsed AST, and replacing its resources at runtime fully replaces the active translation. This is the property that motivated choosing `fluent-rs` directly over `fluent-static`: hot-reload requires a runtime-mutable bundle, and `fluent-bundle` provides exactly that.

If the .ftl file is malformed (Fluent syntax error), the reload fails, the previous bundle stays in place, and the error is logged with the file path and the syntax error location. A development-mode UI overlay can surface these errors visibly — but the architecture document leaves that as an application concern, not a framework feature.

Multiple `runtime_override` calls work for multiple locales simultaneously. A translator working on French and Spanish at the same time runs:

```bash
atelier --translation-dev fr-FR=/path/to/fr.ftl --translation-dev es-ES=/path/to/es.ftl
```

The framework sets up two file watchers, each producing reload events for its respective locale. Each save triggers a reload of just that locale's bundle. The version Signal increments globally, but the per-locale isolation means that switching between French and Spanish in the running app picks up the latest version of each.

**`runtime_override` always implies hot-reload.** There is no separate "load this file once and don't watch" option. The use case for runtime overrides is the translator workflow, which always wants hot-reload. If a use case ever arises for static runtime loading (a plugin shipping its own translations? a memory-constrained embedded device?), it can be added later as a separate method without disturbing the existing API. For now, the API is minimal.

### 12.7 RTL and the LayoutDirection Signal

FernUI's layout primitives already use logical axis names — `leading` and `trailing` instead of `left` and `right`, the same convention Apple's frameworks use. This means the RTL migration is small: the framework adds a `LayoutDirection` enum and a `Signal<LayoutDirection>` derived from the current locale, and the layout pass consults this signal when resolving leading/trailing into physical coordinates.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutDirection {
    Ltr,
    Rtl,
}
```

The direction is derived from the locale's script subtag. `unic-langid` parses the locale string and exposes the script tag (`Arab`, `Hebr`, `Latn`, `Cyrl`, etc.). The set of RTL scripts is small and stable — Arabic, Hebrew, Syriac, Thaana, N'Ko, Samaritan, Mandaic, and a few historical scripts — so `fern-i18n` maintains a hardcoded lookup table mapping script tags to LayoutDirection. This is more reliable than depending on a third-party crate that may not expose direction data, and the table is trivial to maintain (Unicode does not add new RTL scripts often).

The framework computes the direction once when the locale is set and stores it in a `Signal<LayoutDirection>` exposed to widgets via `BuildContext::layout_direction()`. Layout containers, scroll bar placement code, and any custom widget that needs to do direction-aware layout observe this signal directly.

**What changes in RTL mode:**

- **`HStack`** lays out children in reverse order. Children that were leftmost in LTR are rightmost in RTL. The widget code that constructs the HStack does not change — the layout pass handles the flip.
- **`Padding::leading(8)`** resolves to a left padding in LTR and a right padding in RTL. Same for `trailing`.
- **Alignment** values like `Align::leading` and `Align::trailing` flip in the same way.
- **Vertical scroll bar** placement: appears on the trailing edge of inline text. In LTR locales the trailing edge is the right side; in RTL locales it is the left side. ScrollArea and the standalone ScrollBar widget consult the LayoutDirection signal when computing their position.
- **The rich text editor's caret** advances right-to-left when editing RTL text. This is per-paragraph (a French user editing an Arabic paragraph in a multilingual document still gets RTL caret movement in that paragraph), driven by the script of the text being edited, handled inside text-typeset via HarfBuzz's bidi support.

**What does not change:**

- **Top and bottom remain physical.** Vertical layout is consistent across all common writing systems. `Padding::top(8)` always means top.
- **Widget geometry math** continues to use physical coordinates internally. The leading/trailing → left/right resolution happens at one well-defined boundary in the layout pass.
- **Touch/pointer event coordinates** remain in physical screen space. RTL does not invert input coordinates.

**Locale switches that change direction trigger composite rebuild.** Switching from English (LTR) to Arabic (RTL) cannot be handled by the reactive translation version Signal alone, because layout direction affects how children are placed during the build pass — child order is decided at build time, not paint time. The framework detects when `set_locale` produces a direction change and triggers a full composite rebuild after updating the locale and version signals. LTR-to-LTR and RTL-to-RTL switches do not rebuild.

Hot-reloading the same locale via `runtime_override` never changes direction, so it never triggers a rebuild. Only explicit `set_locale` calls can.

### 12.8 The Fluent Bundle Lifecycle

The framework constructs and owns one `FluentBundle<FluentResource>` per supported locale. At startup:

1. The proc macro has already validated all `tr!` calls against the source `.ftl` file at compile time. The runtime starts with confidence that every key referenced in code exists in the source language.
2. For each entry in `compile_in`, the framework constructs a new `FluentBundle` for the locale, parses each resource string via `FluentResource::try_new`, and adds it to the bundle via `bundle.add_resource`. The source locale is included in this set.
3. For each locale that has a `runtime_override`, the framework reads the file from disk, parses it the same way, and replaces the corresponding bundle (or creates a new one if no compile-in entry exists for that locale). It also starts the file watcher.
4. The initial locale is resolved (per §12.5) and stored in `Signal<LanguageIdentifier>`.
5. The translation version Signal is initialized to 0.

At runtime, the bundles are mutable via reload. A reload (triggered by a `runtime_override` file change or by `set_locale` to a locale with a different runtime override) replaces the bundle's contents and increments the translation version. LocalizedStrings observing the version re-call their resolvers and update their cached strings.

The bundles are stored in a `HashMap<LanguageIdentifier, FluentBundle<FluentResource>>` on the framework's i18n manager, behind a `RefCell` because it is single-threaded UI state. File watcher events from the background thread arrive on the UI thread via the EventSource bridge (Section 9.4), so the actual bundle reload happens on the UI thread inside the `RefCell::borrow_mut` — there is no cross-thread mutation and no need for a real lock. The only constraint is the standard single-threaded borrow rule, which is trivially satisfied because reload events are sequential.

`resolve_message` (the runtime entry point that the `tr!` macro expands into) takes the active locale, looks up the bundle in the HashMap, calls `bundle.get_message(key)`, calls `bundle.format_pattern(...)` with the arguments, and returns the formatted String. If the key is missing in the active locale's bundle, the function falls back to the source locale's bundle. If it is missing there too (which should be impossible because the proc macro validated it at compile time), it returns the literal key as a placeholder and logs a warning.

### 12.9 Translation Errors

Errors split into two categories: compile-time errors from the proc macro, and runtime errors from the bundle lookup.

**Compile-time errors from the proc macro.** The macro catches most translation mistakes before the code builds, and emits `compile_error!` output that surfaces in cargo's regular error stream:

- **Missing key.** `tr!(welcom_title())` when the source file defines `welcome-title` fails to compile with an error pointing at the macro invocation site. The message includes a "did you mean `welcome-title`?" suggestion computed via a Levenshtein edit-distance search over the source file's keys, with a small edit budget (typos within 3 edits are suggested; larger distances are not, to avoid noise). This single feature catches the overwhelming majority of real translation mistakes — misspellings, stale renames, and copy-paste errors from adjacent keys.
- **Missing argument.** `tr!(welcome_greeting())` when `welcome-greeting = Hello, { $name }!` expects a `name` variable fails with an error naming the missing argument.
- **Unknown argument.** `tr!(welcome_greeting(name = "A", extra = "B"))` when `welcome-greeting` only expects `name` fails with an error listing the expected arguments so the author can see what was valid.
- **Non-ASCII or reserved path segment.** `tr!(héllo())` or `tr!(foo__bar())` fails with an error explaining the Fluent grammar constraint (ASCII only) or the reserved `__` separator.
- **Malformed source .ftl.** If the source file contains a Fluent syntax error, every `tr!` invocation in the crate fails with a parser error quoting the file path and the parser's reported location. Authoring the source file with a Fluent-aware editor (or trybuild test against it) catches this before it blocks a compilation.

**Runtime errors from the bundle lookup.** Three categories can occur at runtime, and each has defined handling:

**Missing key in the active locale, present in the source.** The lookup falls back to the source-locale bundle and returns the source-language text. No error is raised. This is the normal flow for partial translations and requires no configuration.

**Missing key in both the active locale and the source locale bundles.** This should be impossible because the proc macro validated the key at compile time — if a key is referenced in code, it must exist in the source `.ftl` file, or the code does not compile. If this state is somehow reached (source file edited between build and run, stale `include_str!` reference, corrupted bundle), the runtime returns the key literal as a placeholder. **The compile-time fallback (see §12.2.5) intercepts this placeholder** for simple patterns and produces the reconstructed English text from the macro's expansion, so the user sees meaningful output even in this failure mode. Patterns too complex for the fallback (selectors, plurals) do return the literal key and log a warning.

**Malformed .ftl file at hot-reload.** The reload fails, the previous bundle stays in place, and the error is logged with the file path and the syntax error location. The application continues with the previous (working) translation. The translator sees no change in the running app and goes back to fix the file.

**Argument type mismatch at runtime.** This should also be impossible because the proc macro validates argument names against the source `.ftl` at build time. If it occurs (edge case in a Fluent selector that the validation missed), `FluentBundle::format_pattern` returns the formatted result with default formatting and a `FluentError` in the errors vector, which the framework logs.

### 12.10 Testing Translations

Headless tests can exercise translation logic without spinning up a real bundle file by constructing a minimal `I18nConfig` programmatically:

```rust
#[test]
fn welcome_widget_displays_french_greeting() {
    let mut headless = FernAppBuilder::new()
        .i18n(I18nConfig::test_only(
            "en-US",
            &[("welcome-greeting", "Hello, { $name }!")],
        ).with_locale("fr-FR", &[
            ("welcome-greeting", "Bonjour, { $name } !"),
        ]))
        .build_headless();

    headless.set_locale("fr-FR".parse().unwrap());
    let widget_id = headless.tree.add(WelcomeWidget { name: "Alice".into() });
    headless.tree.layout(SizeProposal::exact(400.0, 100.0));

    let frame = headless.tree.render();
    assert!(frame.contains_text("Bonjour, Alice !"));
}
```

`I18nConfig::test_only` is a separate constructor from the production `compile_in` path. It accepts inline `(key, value)` message pairs rather than the static resource slice that production uses, because tests want to specify messages individually rather than load whole .ftl files. The two paths converge inside the framework: both produce FluentBundles indexed by locale, and the rest of the i18n machinery (the version Signal, the LocalizedString resolution, the binding system) is identical. The widget under test does not change between production and test paths — it uses `tr!(...)` either way, and the test controls which bundles the framework finds when the macro resolves.

**Headless apps support `set_locale`.** The `HeadlessApp` API exposed by `build_headless()` includes `set_locale(LanguageIdentifier)`, mirroring the windowed app's runtime locale switching. Tests can switch locales mid-test to verify that LocalizedStrings re-resolve correctly:

```rust
headless.set_locale("en-US".parse().unwrap());
let frame_en = headless.tree.render();
assert!(frame_en.contains_text("Hello, Alice!"));

headless.set_locale("fr-FR".parse().unwrap());
let frame_fr = headless.tree.render();
assert!(frame_fr.contains_text("Bonjour, Alice !"));
```

The version Signal increments, observers re-resolve, the next render reflects the new locale. No widget reconstruction is needed.

**Compile-time key checking is a separate test layer.** The proc macro's validation runs as part of every `cargo build` and `cargo test` — if a `tr!` call references a missing key, the test build fails with the same compile error as a production build. There is no separate "translation lint" step because the regular compiler is the lint.

**Testing the proc macro itself.** The macro's own tests (in the `fern-i18n-macros` crate) use `trybuild` to verify both successful expansions (for valid inputs) and compile errors (for invalid inputs — missing keys, wrong argument names, malformed source files). These tests run at the framework's CI level and verify the macro's behavior independently of any application.

### 12.11 Crate Structure

The i18n implementation is split across two crates:

- **`fern-i18n`** — the runtime API: `LocalizedString`, `localized()`, `I18nConfig`, the `LayoutDirection` enum, the locale resolution logic, the bundle manager, the file watcher integration, the `resolve_message` / `resolve_message_widget` runtime entry points, and the `compile_in_locales!` declarative helper macro. Depends on `fluent-bundle`, `fluent-syntax`, `unic-langid`, `sys-locale`, `notify`. Used at runtime.

- **`fern-i18n-macros`** — the procedural macro crate exporting `tr!` and `tr_widget!`. The macros read the consuming crate's source `.ftl` file (or directory) at expansion time, parse it via `fluent-syntax`, and validate every invocation. Procedural macros must live in their own crate type; `fern-i18n` re-exports the macros through its public API so application developers write `use fern_i18n::tr;` without needing to know about the macros crate.

There is no separate `fern-i18n-build` crate. There is no build script. There is no generated file in `OUT_DIR`. The application's `Cargo.toml` declares one dependency:

```toml
[dependencies]
fern-ui = "..."
fern-i18n = "..."
```

The application's source layout looks like:

```
my-app/
  Cargo.toml
  src/
    main.rs
  locales/
    en-US.ftl       # required: validated by the proc macro at compile time
    fr-FR.ftl       # optional: loaded at runtime via include_str!
    es-ES.ftl       # optional
    ar-SA.ftl       # optional
```

The proc macro reads `locales/en-US.ftl` at compile time to validate `tr!` calls. The runtime FluentBundle for each locale is constructed from `include_str!` references in the `compile_in` slice (see §12.4). The two paths read the same files but at different times: the macro reads the source language at build time, the runtime reads all locales at startup via `include_str!`.

The fern-widgets crate has its own copy of the same setup: a `locales/en-US.ftl` source file, framework-internal `tr_widget!` calls validated against it at compile time, and a public `framework_locales()` function returning a slice the application passes to `I18nConfig::framework_locales(...)` on the builder chain. See §12.13 for the dual-bundle design.

**Rebuild tracking.** When cargo compiles a crate that invokes `tr!`, it needs to know that the crate depends on the `.ftl` file so that editing a translation triggers a rebuild. The proc macro solves this by emitting an anonymous `const _: &[u8] = include_bytes!(path);` token for every `.ftl` file it read during expansion. `include_bytes!` is a compiler builtin that registers the path as a build dependency — exactly the same mechanism cargo uses to track `include_str!` references in normal Rust code. The constants are discarded (their value is never read), they exist only so cargo sees the dependency. This is more portable than `proc_macro::tracked_path::path` (which requires an unstable feature on older compilers) and correctly handles directory-mode expansion (one `include_bytes!` per file walked).

**Build-time cost.** The proc macro reads and parses the source `.ftl` file (or directory) once per proc-macro process, caching the parsed key map in a `Mutex<HashMap<PathBuf, KeyMap>>`. A crate with hundreds of `tr!` calls parses each `.ftl` file exactly once. For a typical application with a few hundred messages in `en-US.ftl`, the parse takes single-digit milliseconds. Subsequent `tr!` invocations hit the cache.

### 12.12 Constraints and Limitations

**Source language must be present at build time.** This is the trade-off for compile-time key checking. There is no way to add or remove keys at runtime without recompiling, because the proc macro validates against a fixed `.ftl` file at compile time. Adding a key requires editing `en-US.ftl` and rebuilding. This is the right trade-off for an application framework — runtime-modifiable translation tables are a different feature (more like a CMS) and not what FernUI targets.

**Argument types must be Clone.** The `tr!` macro inserts `.clone()` on each argument inside the closure. For Fluent's argument types (strings, numbers, dates) this is automatic and free. For non-cloneable types, the developer drops to `localized(move || resolve_message(...))` and accepts that the closure will be `FnOnce` (single-use, no hot-reload of *that specific instance*). Most cases never hit this.

**Hot-reload is per-locale, not per-message.** When a `runtime_override` file changes, the entire bundle for that locale is reloaded and every LocalizedString observing it re-resolves. There is no way to reload only the single message the translator just edited. This is fine because reloads are rare and bundles are small (typical .ftl files are kilobytes, not megabytes).

**OS locale changes mid-session are not honored.** The OS locale is read once at startup. If the user changes their system locale while the application is running, the application keeps its initial choice. To pick up the new OS locale, the user restarts the application. This matches every native application's behavior.

**No translator-facing GUI.** The translator workflow assumes the translator uses a text editor (or a Fluent-aware tool like Pontoon) to edit .ftl files. FernUI does not ship a translation editing UI. The CLI override + hot-reload is the integration point, not a built-in editor.

**The proc macro requires the source `.ftl` file to be reachable from `CARGO_MANIFEST_DIR`.** This is the standard Rust convention for crate-relative paths and is not a real limitation in practice. Workspaces with shared translation files can use the `#![fern_i18n::source_locale(path = "...")]` attribute to point at a relative path that resolves correctly.

### 12.13 Framework Strings: fern-widgets Translations and Application Overrides

A separate concern from application-level translation is the translation of strings *inside* fern-widgets itself — the accessibility labels, default error messages, tooltip text, and similar strings that built-in widgets expose to AccessKit and to the user. These strings live in the framework's source code, not the application's, so they cannot be translated through the application's source `.ftl` file the way application strings are. They need their own translation path.

#### 12.13.1 The Two-Bundle Design

FernUI maintains *two* sets of FluentBundles per locale: an **application bundle** populated from the application's `compile_in` slice, and a **framework bundle** populated automatically from fern-widgets' own .ftl files. Each bundle has its own namespace and its own resolver macro:

- **`tr!`** — application-facing macro, validates against the application's source `.ftl` at compile time and resolves against the application bundle at runtime. Used by application code.
- **`tr_widget!`** — framework-internal macro, validates against fern-widgets' own source `.ftl` at compile time and resolves against the framework bundle at runtime. Used inside fern-widgets and not exported to application code.

Both macros produce `LocalizedString` values, both observe the same translation version Signal, and both re-resolve on locale change or hot-reload. The only difference is which `.ftl` file the macro validates against and which bundle the resulting resolver consults.

The two-bundle separation is a hard wall: `tr!` never looks in the framework bundle, `tr_widget!` never looks in the application bundle (except for overrides — see §12.13.4). Application string keys and framework string keys live in separate namespaces and cannot collide. An application can have its own `a11y-scrollbar-name` key for some unrelated purpose without affecting the framework's `a11y-scrollbar-name` key.

#### 12.13.2 fern-widgets as a Self-Contained Translatable Crate

The fern-widgets crate ships its own `locales/` directory with a source language `.ftl` file and whatever additional locales the framework can commit to maintaining:

```
fern-widgets/
  Cargo.toml
  src/
    lib.rs
    scrollbar.rs
    button.rs
    ...
  locales/
    en-US.ftl          # source language, required, validated by tr_widget!
    fr-FR.ftl          # framework-shipped translation (if available)
    es-ES.ftl          # framework-shipped translation (if available)
    de-DE.ftl          # framework-shipped translation (if available)
```

The `tr_widget!` macro is the framework-internal twin of `tr!`. It reads `fern-widgets/locales/en-US.ftl` at compile time (relative to the fern-widgets crate's `CARGO_MANIFEST_DIR`) and validates every `tr_widget!` invocation against it. Because each crate has its own `CARGO_MANIFEST_DIR` at compile time, the macro automatically reads the correct source file when expanding inside fern-widgets versus inside an application — no configuration needed.

```rust
// fern-widgets/src/scrollbar.rs
use fern_i18n::tr_widget;

impl Widget for ScrollBar {
    fn accessibility(&self) -> AccessibilityNode {
        AccessibilityNode::new()
            .role(Role::ScrollBar)
            .name(tr_widget!(a11y_scrollbar_name()))
            .description(tr_widget!(a11y_scrollbar_description()))
    }
}
```

The runtime side mirrors the application's setup. fern-widgets exposes a public function returning its compile-in slice:

```rust
// fern-widgets/src/lib.rs
pub fn framework_locales() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        ("en-US", &[include_str!("../locales/en-US.ftl")]),
        ("fr-FR", &[include_str!("../locales/fr-FR.ftl")]),
    ]
}
```

This means **fern-widgets compiles in isolation**. Its source code uses `tr_widget!` calls validated against its own `en-US.ftl`. Its runtime exposes its own resource slice. There is no dependency on any application-side `.ftl` file, and the crate works as a normal library that can be added to any application's `Cargo.toml` without configuration.

#### 12.13.3 Explicit Framework Bundle Registration

Applications that use fern-widgets register its translation bundle explicitly on the builder chain:

```rust
FernAppBuilder::new()
    .i18n(I18nConfig::new()
        .compile_in(&[ /* application locales */ ])
        .framework_locales(fern_widgets::framework_locales())
        // ...
    )
    .root(...)
    .run();
```

**fern-app is deliberately widget-agnostic** — it does not depend on fern-widgets, and therefore cannot automatically register fern-widgets' translation bundle. The alternative (fern-app depending on fern-widgets) would invert the crate graph and force every application to pull in fern-widgets whether or not it uses the built-in widget catalog. An application that builds its UI from its own custom widgets should not be required to ship fern-widgets' translation bundle or its ~3 MB of Arabic/Hebrew fonts.

The explicit registration pattern also lets applications compose multiple framework-style crates. A hypothetical third-party widget crate `spiffy-widgets` can expose its own `framework_locales()` function, and the application registers both in a single builder chain:

```rust
.framework_locales(fern_widgets::framework_locales())
.framework_locales(spiffy_widgets::framework_locales())
```

Multiple calls to `framework_locales` accumulate — each slice is registered independently, and keys from one crate do not collide with keys from another because the proc macros validate against each crate's own source `.ftl` file at compile time.

**Widgets that have not been explicitly registered still work.** The proc macro's compile-time fallback (see §12.2.5) reconstructs the source-language text from the `.ftl` parse tree at expansion time for simple patterns. If an application forgets to call `framework_locales()`, fern-widgets' accessibility labels still render in English (the source language) because the macro emits an inline fallback. The missing registration is silent — a log message at startup would be the right place to surface it, but the application does not crash or render empty strings.

The framework bundle is constructed identically to the application bundle: one `FluentBundle` per locale in the slice, with each `.ftl` file becoming a separate `FluentResource` per the multi-resource design from §12.4. The fern-widgets bundles are stored on the same `I18nManager` as the application bundles, in a separate `HashMap<LanguageIdentifier, FluentBundle>`. The version Signal increments apply to both maps simultaneously — a hot-reload of any locale (application or framework) increments the version, and every LocalizedString re-resolves regardless of which bundle its resolver consults.

#### 12.13.4 Application Override

Applications can override individual framework strings via an opt-in `I18nConfig` method:

```rust
.i18n(I18nConfig::new()
    .compile_in(&[
        ("en-US", &[include_str!("../locales/en-US.ftl")]),
        ("fr-FR", &[include_str!("../locales/fr-FR.ftl")]),
    ])
    .override_widget_strings(&[
        ("en-US", &[include_str!("../locales-fern-widgets/en-US.ftl")]),
        ("fr-FR", &[include_str!("../locales-fern-widgets/fr-FR.ftl")]),
        ("ja-JP", &[include_str!("../locales-fern-widgets/ja-JP.ftl")]),
    ])
    // ...
)
```

The override is a separate slice with the same `&[(&str, &[&str])]` shape as `compile_in`, pointing at .ftl files in a parallel `locales-fern-widgets/` directory in the application's source tree. The override files only need to define the keys the application wants to change. Keys not present in an override file fall back to the framework's default for that locale, and from there to the framework's source language.

The override .ftl files are not validated by any compile-time macro — they only contain key/value definitions, no Rust code references them, and they are loaded purely as runtime resources. The framework checks them at startup: if an override file contains a key that does not exist in fern-widgets' source `.ftl`, the framework logs a warning at startup but does not fail (the unknown key is harmless, just unreachable). If an override file is malformed, the affected locale's override is discarded with an error log, and the framework's default for that locale takes over.

#### 12.13.5 Lookup Precedence for Framework Strings

When `tr_widget!(a11y_scrollbar_name())` resolves, the framework consults the bundles in this order:

1. **Application override bundle for the active locale.** If the application called `override_widget_strings` and the active locale's override bundle defines the key, use that value.
2. **Framework bundle for the active locale.** If the framework ships a translation of this locale and it defines the key, use that value.
3. **Application override bundle for the source locale.** If the override bundle for the source locale (typically `en-US`) defines the key, use that value. This handles the case where the application overrides a key in English but the active locale has no override and no framework translation.
4. **Framework bundle for the source locale.** The ultimate fallback — fern-widgets' own en-US text.

All four steps are HashMap lookups against pre-loaded bundles. There is no file I/O, no parsing, no allocation beyond the resolved string itself. The lookup happens once when `to_signal()` is called on the LocalizedString and again on each version increment.

For application strings (resolved via `tr!`), the precedence is unchanged from §12.5: application's bundle for the active locale, then application's bundle for the source locale. The framework bundle is never consulted for application strings; the application override bundle is never consulted for application strings.

#### 12.13.6 Locale Coverage Asymmetry

Because fern-widgets' locale coverage is independent of the application's locale coverage, mismatches are possible. An application supports `en-US`, `fr-FR`, `es-ES`, and `ja-JP`; fern-widgets ships translations for `en-US`, `fr-FR`, and `de-DE`. When the application runs in Spanish, the framework strings fall back to English (because fern-widgets has no Spanish bundle), while the application strings render in Spanish normally. When the application runs in Japanese, the same thing happens. When the application runs in German, the application strings fall back to English (because the application has no German bundle) — but the framework strings render in German.

This asymmetry is acceptable. The framework is honest about which locales it can maintain translations for, and applications fill in the gaps via `override_widget_strings` for the locales they care about. An application targeting Japanese users would ship a `locales-fern-widgets/ja-JP.ftl` file with Japanese translations of the framework's strings, and that file takes precedence over the framework's fallback-to-English. The override mechanism is the application's tool for guaranteeing locale parity between application strings and framework strings.

The hot-reload path applies to override bundles as well as production bundles. A translator running:

```bash
atelier --translation-dev fr-FR=/path/to/app-fr.ftl --translation-dev-widget fr-FR=/path/to/widget-fr.ftl
```

can iterate on both application strings and framework override strings simultaneously, with both saves triggering reloads of the appropriate bundles. The `--translation-dev-widget` CLI flag is the override-bundle equivalent of `--translation-dev`, mapping to `runtime_override_widget_strings(locale, path)` on `I18nConfig`. The semantics are identical: load the file, watch for changes, reload on save, increment the version Signal.

#### 12.13.7 Application Accessibility Strings Are Not Special

Accessibility labels written in **application** code are not special. They are regular translatable strings using the regular `tr!` macro, defined in the application's own .ftl files alongside every other application string:

```rust
// In application code:
impl Widget for MyCustomWidget {
    fn accessibility(&self) -> AccessibilityNode {
        AccessibilityNode::new()
            .role(Role::Button)
            .name(tr!(my_custom_widget_label()))
            .description(tr!(my_custom_widget_description()))
    }
}
```

The `accessibility()` method accepts `LocalizedString` parameters like any other widget builder method. There is no separate accessibility-specific translation system. The dual-bundle design exists only to handle the boundary between application-defined strings and framework-defined strings; once you are inside application code, all strings flow through the same `tr!` path regardless of whether they are user-visible labels, button text, tooltips, error messages, or accessibility names.

---

## 13. Overlay System

Tooltips, dropdown menus, context menus, and popovers render outside the normal layout hierarchy. They do not participate in their parent's layout negotiation — they float above the main content, positioned relative to an anchor widget or the pointer. They are managed by an `OverlayManager` in fern-core, which coordinates creation, positioning, stacking, dismissal, event routing, and accessibility.

### 13.1 Core Data Structures

An overlay is requested through an `OverlayRequest` that specifies what to show, where to show it, how to dismiss it, and whether to use an in-tree layer or a native popup window.

The `OverlayPlacement` enum determines positioning relative to the anchor: `Below` (dropdown: below the anchor, leading-edge-aligned), `Above` (fallback when no space below), `BelowPreferred` (like `Below`, but automatically flips to `Above` when there is insufficient space below the anchor — used by ComboBox and MenuBar dropdowns), `TrailingEdge` (submenu: to the trailing side of the parent menu item), `AtPointer` (context menu: at the click position), and `NearAnchor` (tooltip: near the anchor with a preferred alignment and offset). The framework performs smart positioning — if the primary placement would position the overlay outside the visible window bounds, it falls back to the opposite direction (Below → Above, TrailingEdge → LeadingEdge) automatically.

The `DismissBehavior` enum controls when the overlay is dismissed: `ClickOutside` (dismiss when the user clicks anywhere outside the overlay — standard for menus and dropdowns), `PointerLeave` with a configurable delay (dismiss when the pointer leaves both the anchor and the overlay — standard for tooltips), `Manual` (dismiss only via explicit API call — for modeless popovers), and `Any` (a combination of multiple behaviors).

The `OverlayLayer` enum determines the rendering mechanism: `InTree` (rendered within the application window's wgpu surface), `NativePopup` (rendered in a separate native OS window), or `Auto` (the framework decides based on content size and available space).

### 13.2 Two Rendering Mechanisms

**In-tree overlays** (`OverlayLayer::InTree`) are widget subtrees in the same arena as the main content, tagged as overlay members. The rendering pass draws the main content first, then overlays in stack order. Hit testing checks overlays first (topmost wins). Suitable for tooltips, small popups, and any overlay that fits within the application window.

**Native popup overlays** (`OverlayLayer::NativePopup`) create separate winit windows with their own wgpu surfaces. Each popup window has its own widget subtree but shares the same state arena, palette, locale, and shortcut map as the main window. Necessary for menus that must extend beyond the application window boundary — a dropdown menu near the bottom of the screen should not be clipped to the window edge.

Both mechanisms share the same `OverlayManager` and the same logical parent chain. The rendering mechanism is invisible to the widget author and to the event routing system.

### 13.3 Overlay Stack and Cascading

The `OverlayManager` maintains a stack of active overlays. Each active overlay records its ID, anchor widget, parent overlay (for submenu cascading), dismissal behavior, rendering layer, and root widget ID of the overlay's content subtree.

Overlays can be nested — a context menu item that triggers a submenu opens a secondary overlay anchored to the submenu item, with a parent reference to the first overlay. Dismissing an overlay at level N also dismisses all overlays at level N+1, N+2, and so on. Pressing Escape closes the topmost overlay. Clicking outside all overlays closes the entire stack.

### 13.4 Command Routing Through Logical Parents

When a `MenuItem` inside a native popup overlay emits a command, that command must bubble through the *anchor widget's* tree path in the main window, not through the popup window's own tree. The menu is logically parented to the widget that triggered it, even though it is rendered in a separate window. The overlay manager maintains this logical parentage so that command routing works correctly: the command bubbles from the menu item to the overlay's anchor widget, then continues up through the main tree to the application root where the Qleany controller handles it.

This design ensures that a context menu on an editor widget emits commands that reach the editor's command handling context, not a detached popup context.

### 13.5 Tooltips

A tooltip is the simplest overlay — a non-interactive text label (or rich content widget) that appears after a hover delay and disappears when the pointer moves away.

Tooltips are attached to any widget via a builder method (`.tooltip(text)` for simple text, `.rich_tooltip(|| content)` for arbitrary widget content). Under the hood, the builder wraps the widget in a `TooltipHost` that monitors `PointerEnter` and `PointerLeave` events and manages a timer. After a configurable delay (defaulting to approximately 500ms, sourced from the theme's `MotionTokens` or a platform-appropriate value), the `TooltipHost` requests an overlay with `OverlayLayer::InTree`, `OverlayPlacement::NearAnchor`, and `DismissBehavior::PointerLeave`.

The tooltip's visual appearance (background color, text color, corner radius, padding, font size) is resolved from the theme's tooltip tokens (`tooltip_surface`, `tooltip_text`). The tooltip does not receive focus and does not participate in tab navigation.

For accessibility, the `TooltipHost` sets AccessKit's `DescribedBy` property on the anchor widget, pointing to the tooltip's AccessKit node when the tooltip is visible. Screen readers announce the tooltip content when the user focuses the anchor widget.

### 13.6 Context Menus

A context menu appears at the pointer position on right-click (or long-press on touch devices). It is a vertical list of actionable items, potentially with submenus, separators, icons, keyboard shortcut labels, and disabled states.

Context menus are attached to any widget via a builder method (`.context_menu(|ctx| menu_content)`). The closure is called at show-time, not at build-time. This is deliberate — the menu content may depend on the current application state (some items enabled or disabled, some items conditionally present based on selection state). The closure receives a context that allows it to query state.

Context menus render as native popup overlays (`OverlayLayer::NativePopup`) because they must be able to extend beyond the application window boundary. Dismissal uses `DismissBehavior::ClickOutside` — clicking any menu item or clicking outside the menu dismisses it. Activating a menu item emits a command and then dismisses the menu.

**Submenu cascade.** A menu item that has a submenu indicator opens a secondary overlay anchored to the submenu item, with placement `TrailingEdge` (to the right in LTR, to the left in RTL). The submenu opens on hover with a brief delay (approximately 200ms) to avoid accidental activation. The diagonal movement problem — where the user moves the pointer diagonally from the parent item to the submenu, briefly passing over other items — is handled by a triangular hit-test zone between the parent item and the submenu boundary. While the pointer is within this triangle, other menu items do not activate.

**Keyboard navigation within menus.** When a menu is open, Arrow Up and Arrow Down move the highlight between items (skipping separators and disabled items). Arrow Right opens a submenu on the highlighted item. Arrow Left closes the current submenu and returns to the parent menu. Enter activates the highlighted item. Escape closes the topmost menu. Home and End jump to the first and last items. Type-ahead (typing a character) jumps to the next item whose label starts with that character.

**Accessibility.** The menu's AccessKit structure uses `Role::Menu` for the container and `Role::MenuItem` for each item. Submenu triggers declare `HasPopup::Menu`. The anchor widget declares `HasPopup::Menu` when a context menu is attached. Disabled items are marked with the disabled state. Keyboard shortcut labels are included via the `KeyboardShortcut` property, resolved from the `ShortcutRegistry` (see [`shortcut-intent-action.md`](shortcut-intent-action.md)).

### 13.7 Dropdown (Combobox / Select)

A dropdown is structurally similar to a context menu but differs in two important ways: it is anchored to a specific trigger widget (a button with a chevron) rather than appearing at the pointer position, and it has two-way state binding — the selected item drives both the trigger's display text and which item is highlighted in the open list.

The dropdown trigger is built with the unified `Widget` trait, composed of an HStack containing a label (showing the current selection or a placeholder) and a chevron icon. Clicking the trigger opens an overlay with `OverlayPlacement::BelowPreferred`, which automatically flips to `Above` when there is insufficient space below. The overlay contains a list of items (a scrollable ListView once Milestone 6 ships — currently a VStack). Selecting an item updates the bound `Signal<Option<usize>>`, dismisses the overlay, and returns focus to the trigger.

The selected value is bound via a `Signal<Option<T>>` handle provided by the application. When the state changes (either through the dropdown or through external application logic), the trigger's display text updates via the binding system.

**Accessibility.** The trigger widget declares `Role::ComboBox` with `HasPopup::ListBox` and `Expanded` state (true when the overlay is open). The overlay list uses `Role::ListBox` with `Role::Option` for each item. The selected item is marked with `Selected`. Arrow Up and Arrow Down navigate the list while the dropdown is open. Typing characters performs type-ahead filtering.

### 13.8 Menu Bar

Context menus and dropdowns cover popup-style overlays. The application-level menu bar (File, Edit, View, Help) is a related but distinct concern. On macOS, the menu bar is a native `NSMenu` managed by the OS and rendered outside the application window entirely. On Windows and Linux, the menu bar is a widget rendered inside the application window, typically at the top.

FernUI must abstract this platform difference. The menu bar is defined through the `FernApp` builder using a declarative `MenuBar` description. On macOS, `fern-platform` translates this description into native `NSMenu` items. On Windows and Linux, `fern-widgets` renders it as a horizontal bar of menu triggers, each opening a dropdown overlay. Both paths emit the same typed commands to the application's command handler.

This is listed as an open question for post-first-milestone design (Section 28) because the native menu bar integration requires platform-specific code in `fern-platform` that goes beyond what winit currently provides.

### 13.9 Overlay Manager Internals

The `OverlayManager` is a component within `fern-core`, separate from the widget arena and the state arena. It maintains the overlay stack, coordinates between in-tree and native popup layers, tracks logical parent relationships for command routing, and handles the smart positioning fallback logic.

For in-tree overlays, the overlay's widget subtree is part of the main arena but tagged as an overlay member. The rendering pass draws the main content first, then overlays in stack order. Hit testing checks the overlay layer before the main content layer — the topmost overlay receives pointer events first.

For native popup overlays, each popup has its own winit window, its own wgpu surface, and its own widget subtree. The overlay manager coordinates between the main window and popup windows for dismissal detection (clicking on the main window while a popup menu is open dismisses the menu).

### 13.10 Testability

Overlays are testable headlessly because the `OverlayManager` is a data structure in `fern-core` with no platform dependencies. In headless tests, `NativePopup` overlays are downgraded to `InTree` overlays — same behavior, same event routing, just no actual second window. The simulated clock (`tree.advance_time()`) enables deterministic testing of time-dependent overlay behavior (tooltip hover delay, submenu open delay, double-click interval) without `thread::sleep` or flaky timing in CI.

---

## 14. Drag and Drop

### 14.1 Three Scenarios

**Intra-widget rearrangement** — reordering items within a single list or tree. No serialization needed. The drag source and target are the same widget.

**Inter-widget transfer** — dragging content between widgets within the same application. Source and target agree on a typed payload. Serialization is optional.

**Cross-application transfer** — dragging to/from other applications via OS-native protocols. Data must be serialized into MIME types. Requires platform integration through a `PlatformDragBackend` trait.

### 14.2 Typed Payload Model

A `DragPayload` carries multiple MIME-typed representations of the same content. For intra-application use, FernUI provides a typed wrapper via the `DragData` trait, avoiding raw byte manipulation. Drop targets declare which types they accept without deserializing the payload — only MIME type lists are checked during hover. Deserialization occurs on drop.

### 14.3 Source and Target Traits

`DragSource` produces the payload and visual preview when a drag begins. `DropTarget` evaluates acceptance and handles the drop. Visual feedback (insertion lines, highlight rectangles) is rendered by the drop target during its paint pass, using `DropFeedback` descriptors.

The source attaches `on_drag` and, on `DragPhase::Started`, calls either `EventContext::start_drag(..)` (no visible preview) or `EventContext::start_drag_with_preview(..)` (preview `Box<dyn Widget>` that floats at the pointer via `OverlayPlacement::AtPointer`). `ListView` and `TreeView` use the `_with_preview` variant by default — they re-invoke their delegate closure for the dragged row and wrap the result in a sized raised panel so the preview reads as "picked up" against the window.

The drop target sees four lifecycle callbacks, in this order, each firing at most once per role per drag:

1. `on_drag_hover(payload, pos, ctx) -> DropFeedback` — per `PointerMove` while this widget is the current drop target. The widget stashes feedback state (an insertion-line y, a highlight rect) and returns the matching descriptor.
2. `on_drag_tick(local_pos, ctx)` — per layout frame while this widget is the current drop target. Used for time-driven behaviours (viewport-edge auto-scroll, spring-loaded folder expansion) that must keep progressing when the pointer is stationary.
3. `on_drag_leave(ctx)` — when the widget stops being the drop target for any reason (pointer moved to another target, drop completed, Escape cancel, source destroyed). **Widgets own their feedback state and MUST clear it here** — the framework doesn't touch widget-held Signals or Cells.
4. `on_drop(payload, pos, ctx) -> bool` — only on `PointerUp` if this widget is the drop target at the release position. Already preceded by `on_drag_leave`, so feedback is cleared by the time the drop handler runs.

Widgets that expose their drop-feedback state via a `Signal<T>` bound at `BindingLevel::RepaintOnly` (the recommended pattern) get automatic invalidation on `set(...)` — both the hover-set and leave-clear repaint the widget on the next frame without a rebuild.

**Scroll during drag.** When the wheel fires while `active_drag` is active, the framework routes the `Scroll` event to the drag's current target (instead of the stale `hovered` widget) and then re-fires the hover pipeline at the stationary pointer. The drop target's `on_scroll` handler moves content; the synthesised re-hover recomputes the insertion index against the new scroll offset.

### 14.4 Accessibility Contract

Every drag-and-drop operation must have a keyboard-accessible equivalent that emits the same command. A `ReorderableList` supports both drag gesture and Alt+Arrow keyboard shortcuts, both calling the same `move_item` method and emitting the same `AppCmd::ReorderItem` command. The semantic operation is decoupled from the input gesture.

---

## 15. Data Model

### 15.1 Why Data Models Are Their Own Crate

Three things force an abstraction between domain data and view widgets: virtualization (a list with 10,000 items cannot instantiate 10,000 widget subtrees), sharing across multiple views (a sidebar tree and a "Move to…" dialog operating on the same document outline), and change notification for incremental updates (a single item insertion must not rebuild the entire view). FernUI answers all three with reactive collection types in a dedicated crate — `fern-data` — sitting *above* fern-core in the dependency graph.

The separation matters because collections are a higher-level concept than the widget tree. A view-model layer wants to hold domain entities in reactive collections without pulling in the renderer; a test wants to assert on model state without instantiating widgets; a Qleany-style Clean Architecture application wants its domain crate to depend on fern-data without inheriting widget-authoring concerns. Keeping the collection types out of fern-core enforces this separation in the dependency graph, not just by convention.

The crate contains five concrete types and one trait:

- **`ListModel<T>`** — concrete reactive list. `Vec<T>` behind `Rc<RefCell<…>>`. Every mutation method emits a `DataChange` notification after releasing the mutable borrow. Cloneable for shared access.
- **`ListDataSource` trait** — escape hatch for datasets that don't fit in memory (paged DB cursor, filesystem listing, memory-mapped log). Implementors own the data and emit `DataChange` manually. Not object-safe, not a supertype of `ListModel<T>` — two separate input paths on `ListView`.
- **`TreeModel<T>`** — concrete reactive tree. SlotMap-backed; `NodeId` handles are stable across mutations. Emits `TreeChange`.
- **`TreeSlice<T>`** — per-view flattened projection of a `TreeModel`. Owns its own expand/collapse state, so two `TreeView`s sharing a model have independent expansion. Exposes a `version: Signal<u64>` that bumps on each re-flatten.
- **`SelectionModel`** — `Signal<BTreeSet<usize>>` plus an anchor for Shift+click, in three modes (None / Single / Multi). Consumed by both `ListView` and `TreeView`.
- **`DataChange` / `TreeChange`** — the notification enums.

The MVVM command flow stays one-way: widget emits a typed intent → ancestor `Action` runs → domain mutation → model notification → widgets repaint. The view never writes directly to the model. See §9.2 for the intent layer and [`shortcut-intent-action.md`](shortcut-intent-action.md) for its surface API.

**Authoritative reference:** [`data-models.md`](data-models.md) covers the full API, worked examples, Repeater-vs-ListView tradeoffs, intra-widget DnD integration, testing patterns, and a worked MVVM diagram. The section below keeps its V1 shape intact for now (the type signatures here predate the actual fern-data crate — the current API is in data-models.md and in [`crates/fern-data/src/`](../crates/fern-data/src/)). The design rationale — why virtualization forces a model abstraction, why the model owns the data and the view owns the view state, why `TreeSlice` is per-view — is what matters.

### 15.2 ListModel<T>

`ListModel<T>` is a concrete framework type that stores items in an internal `Vec<T>` behind `Rc<RefCell<>>`. It is the primary data model for flat collections. Cloning a `ListModel` gives a second handle to the same data — multiple widgets can hold clones and all see the same items.

```rust
pub struct ListModel<T> { /* Rc<RefCell<ListModelInner<T>>> */ }

impl<T: 'static> ListModel<T> {
    pub fn new() -> Self;
    pub fn from_vec(items: Vec<T>) -> Self;

    // Queries
    pub fn count(&self) -> usize;
    pub fn with_item<R>(&self, index: usize, f: impl FnOnce(&T) -> R) -> R;

    // Mutations — each emits the corresponding DataChange automatically
    pub fn push(&self, item: T);
    pub fn insert(&self, index: usize, item: T);
    pub fn remove(&self, index: usize);
    pub fn set(&self, index: usize, item: T);
    pub fn move_item(&self, from: usize, to: usize);
    pub fn replace_all(&self, items: Vec<T>);
    pub fn clear(&self);

    // Observation
    pub fn observe_changes(&self, callback: impl Fn(&DataChange) + 'static) -> ObserverHandle;
}

pub enum DataChange {
    ItemsInserted { range: Range<usize> },
    ItemsRemoved { range: Range<usize> },
    ItemsMoved { from: usize, to: usize, count: usize },
    ItemUpdated { index: usize },
    Reset,
}
```

Every mutation method is atomic: it modifies the internal Vec, drops the mutable borrow, then notifies observers. By the time any observer runs (including a ListView's internal watcher), the borrow is released and shared borrows (for `count()` and `with_item()`) are safe. This prevents the `RefCell` double-borrow panic.

The `with_item()` callback API avoids returning a reference that would need to outlive the `RefCell` borrow guard. The ListView calls `with_item()` during layout to pass each item to the delegate closure.

`ListModel<T>` is suitable for any collection where the data fits in memory: project lists (tens of items), chapter lists (hundreds), tag lists (dozens), combo box option lists (tens), toolbar button sets (a few). These are the vast majority of lists in a desktop application.

`ListModel<T>` can live anywhere — on an application-wide ViewModel struct for shared data, or as a local field on a widget for ephemeral data (a combo box's option list, a filtered search result). The `Rc`-based ownership means it is deallocated only when all handles are dropped.

### 15.3 ListDataSource Trait

`ListDataSource` is a trait for the rare case where the data is too large to hold in memory or lives in an external system (paged database cursor, filesystem directory listing, memory-mapped log file). It is not related to `ListModel<T>` by inheritance — they are two separate input paths.

```rust
pub trait ListDataSource: 'static {
    type Item;

    fn count(&self) -> usize;
    fn with_item<R>(&self, index: usize, f: &mut dyn FnMut(&Self::Item) -> R) -> R;
    fn observe_changes(&self, callback: impl Fn(&DataChange) + 'static) -> ObserverHandle;
}
```

The `with_item()` callback pattern (rather than returning `&Item`) allows the implementor to hold internal locks, borrow guards, or temporary buffers for the duration of the callback. A paged data source can fetch a page into a cache, call `f(&cached_item)`, and release the page. A filesystem browser can read a directory entry, call `f(&entry)`, and move on.

The implementor is responsible for emitting correct `DataChange` notifications when the data changes. This is the cost of not using `ListModel<T>`, which handles notifications automatically.

`ListModel<T>` does not implement `ListDataSource`. They share the same `DataChange` enum and the same callback-based item access pattern, but they are consumed through separate paths on the view widgets.

### 15.4 ListView and Repeater Consumption

ListView accepts either a `ListModel<T>` or a `dyn ListDataSource` through two constructors:

```rust
impl ListView {
    pub fn new<T>(model: ListModel<T>, delegate: impl Fn(&T) -> Box<dyn Widget>) -> Self;
    pub fn from_source<S: ListDataSource>(source: S, delegate: impl Fn(&S::Item) -> Box<dyn Widget>) -> Self;
}
```

Internally, the ListView stores an enum distinguishing the two sources. When the source is a `ListModel`, the ListView borrows the internal Vec directly during layout (holding the `Ref` for the duration of the layout pass). When the source is a `ListDataSource`, the ListView uses `with_item()` callbacks.

The Repeater accepts only `ListModel<T>`. It is designed for small, non-virtualized dynamic collections where all items have widget subtrees simultaneously. The `ListDataSource` escape hatch is not needed because the Repeater instantiates every item — if the dataset were large enough to need paging, a ListView with virtualization would be the correct widget.

### 15.5 TreeModel<T>

`TreeModel<T>` is a concrete framework type that stores a hierarchy of items. It is `Rc<RefCell<>>` internally, cheaply cloneable, shared across multiple views. It stores nodes in a flat arena (SlotMap or similar) with parent-child links, identified by opaque `NodeId` handles.

```rust
pub struct TreeModel<T> { /* Rc<RefCell<TreeModelInner<T>>> */ }
pub struct NodeId(/* opaque */);

impl<T: 'static> TreeModel<T> {
    pub fn new() -> Self;

    // Structural queries
    pub fn root_count(&self) -> usize;
    pub fn root(&self, index: usize) -> NodeId;
    pub fn child_count(&self, parent: NodeId) -> usize;
    pub fn child(&self, parent: NodeId, index: usize) -> NodeId;
    pub fn parent(&self, node: NodeId) -> Option<NodeId>;
    pub fn depth(&self, node: NodeId) -> usize;
    pub fn with_item<R>(&self, node: NodeId, f: impl FnOnce(&T) -> R) -> R;
    pub fn find_by(&self, predicate: impl Fn(&T) -> bool) -> Option<NodeId>;

    // Mutations — each emits the corresponding TreeChange automatically
    pub fn insert_root(&self, index: usize, item: T) -> NodeId;
    pub fn insert_child(&self, parent: NodeId, index: usize, item: T) -> NodeId;
    pub fn remove(&self, node: NodeId);       // removes entire subtree
    pub fn move_node(&self, node: NodeId, new_parent: NodeId, index: usize);
    pub fn update(&self, node: NodeId, item: T);

    // Observation
    pub fn observe_changes(&self, callback: impl Fn(&TreeChange) + 'static) -> ObserverHandle;

    // Per-view flattened projection
    pub fn create_slice(&self) -> TreeSlice<T>;
}

pub enum TreeChange {
    Inserted { node: NodeId, parent: Option<NodeId>, index: usize },
    Removed { node: NodeId, parent: Option<NodeId> },
    Moved { node: NodeId, old_parent: Option<NodeId>, new_parent: Option<NodeId>, new_index: usize },
    Updated { node: NodeId },
    Reset,
}
```

The `TreeModel` is the shared source of truth for the hierarchy. It knows nothing about expand/collapse state, which is per-view.

### 15.6 TreeSlice<T> — Per-View Flattened Projection

The expand/collapse state of a tree is view state, not data state. Two TreeViews showing the same data (a sidebar tree and a "Move to..." dialog) will have different nodes expanded. The `TreeSlice<T>` manages this per-view state.

A `TreeSlice` references a shared `TreeModel`, owns its own set of expanded node IDs, and maintains a flat list of currently-visible nodes with depth information. It observes the `TreeModel`'s `TreeChange` notifications and translates them into flat `DataChange` notifications that the TreeView consumes identically to a ListView consuming a ListModel.

```rust
pub struct TreeSlice<T> {
    // References the source TreeModel (Rc clone)
    // Owns: HashSet<NodeId> for expanded nodes
    // Maintains: Vec<(NodeId, usize /* depth */)> — the current flat visible list
}

impl<T: 'static> TreeSlice<T> {
    // Flat visible-node access (what TreeView consumes)
    pub fn visible_count(&self) -> usize;
    pub fn visible_item(&self, index: usize, f: impl FnOnce(&T, usize /* depth */));
    pub fn visible_node_id(&self, index: usize) -> NodeId;

    // Expand/collapse (per-view state)
    pub fn is_expanded(&self, node: NodeId) -> bool;
    pub fn expand(&self, node: NodeId);     // emits DataChange::ItemsInserted for newly visible children
    pub fn collapse(&self, node: NodeId);   // emits DataChange::ItemsRemoved for hidden children
    pub fn toggle(&self, node: NodeId);
    pub fn expand_all(&self);
    pub fn collapse_all(&self);
    pub fn expanded_nodes(&self) -> Vec<NodeId>;          // for persistence
    pub fn set_expanded_nodes(&self, nodes: &[NodeId]);   // for restore

    // Flat change observation (same protocol as ListModel)
    pub fn observe_flat(&self, callback: impl Fn(&DataChange) + 'static) -> ObserverHandle;
}
```

When the `TreeModel` mutates (a node is inserted, removed, moved, or updated), every `TreeSlice` observing it receives the `TreeChange` and independently determines the impact on its own flat visible list. If TreeSlice A has the parent expanded, the new child appears in the flat list and `DataChange::ItemsInserted` is emitted. If TreeSlice B has the parent collapsed, the flat list does not change and no `DataChange` is emitted.

The consumer never creates a `TreeSlice` directly. The TreeView calls `tree_model.create_slice()` internally during its `build()`:

```rust
TreeView::new(chapters_model.clone(), |chapter, depth| {
    HStack::new()
        .child(Padding::left(depth as f32 * 20.0))
        .child(IconWidget::chevron_right(16.0))
        .child(TextWidget::new(&chapter.title))
})
```

If the consumer needs programmatic control over expand state (expand-all from a toolbar button, restoring saved state), the TreeView exposes methods that delegate to its internal TreeSlice.

### 15.7 MVVM Command Flow

The overall architecture follows MVVM with unidirectional flow. User interaction produces typed intents that reach application `Action`s (see §9.2 and [`shortcut-intent-action.md`](shortcut-intent-action.md)). The domain mutation fires, the data model receives the resulting change event, updates its cached collection, and the notification propagates to the view:

```text
User clicks "Delete" → Widget fires AppIntent::DeleteProject(id)
    → Ancestor Action runs → Domain use case executes
        → ViewModel calls projects.remove(index)
            → ListModel emits DataChange::ItemsRemoved
                → ListView removes visible widget, relayouts
```

The view never mutates the data model directly in response to user actions — it emits intents. The data model never pushes UI updates directly — it mutates its reactive collections, and the notification system propagates the changes. This is unidirectional data flow.

### 15.8 Qleany Integration — Generated EntityListModel

For Qleany-structured applications, the repetitive wiring between event notifications, controllers, and data models is generated by Qleany. The framework provides `ListModel<T>`, `Signal<T>`, and `ObserverHandle` as building blocks. Qleany generates entity-specific model structs that assemble them.

A generated model manages three concerns: the reactive collection (`ListModel<T>`), the parent relationship (`Signal<Option<i32>>` for the parent entity ID), and the event wiring (observers on the Qleany event registry that trigger data refresh).

```rust
// Generated by Qleany
pub struct WorkspaceProjectsModel {
    pub items: ListModel<ProjectDto>,
    pub parent_id: Signal<Option<i32>>,
    pub loading: Signal<LoadingStatus>,
    pub error: Signal<Option<String>>,
    event_handles: Vec<ObserverHandle>,
}
```

The generated constructor wires three behaviors. When `parent_id` changes (the user selects a different workspace), the model fetches the projects for that workspace via the controller and calls `items.replace_all(dtos)`. When the event registry signals that a project was created, updated, or removed, the model performs the corresponding `items.push()`, `items.set()`, or `items.remove()`. The `ObserverHandle` values are stored on the model struct and cleaned up when the model is dropped.

The widget binds to `model.items` (a `ListModel<ProjectDto>` clone) without knowing about the controller, the events, or the parent relationship:

```rust
ListView::new(model.items.clone(), |project| {
    HStack::new()
        .child(TextWidget::new(&project.title))
        .child(Spacer::new())
})
```

When the user selects a workspace in the sidebar, a handler calls `model.parent_id.set(Some(workspace_id))`. The observer inside the model triggers a refetch. The `ListModel` emits `DataChange::Reset`. The ListView rebuilds its visible items.

Nothing in fern-data requires Qleany. Applications using diesel + hand-rolled entities, sqlx, or Kafka-event-driven view-models follow the same shape with their own entity-to-ViewModel transform.

### 15.9 Model Lifetime and Scope

Data models can live at any scope appropriate to the use case.

**Application-wide.** A `WorkspaceProjectsModel` is created once at startup, stored on an `AppViewModel` struct, and shared with widgets via constructor arguments or environment propagation. It persists for the lifetime of the application (or the workspace session). When the user switches workspaces, `parent_id.set(Some(new_id))` triggers a refetch — the model instance is reused, not recreated.

**Component-scoped.** A combo box's option list is a `ListModel<ComboOption>` created during a widget's `build()`, stored as a struct field on the widget, and destroyed when the widget is destroyed. It is not shared with any other widget. It may be populated statically (from an enum) or dynamically (fetched once from a controller).

**Dialog-scoped.** A search results list in a "Find and Replace" dialog is a `ListModel<SearchResult>` that lives for the duration of the dialog. It is created when the dialog opens, populated when the user types a query, and destroyed when the dialog closes.

The `Rc`-based ownership ensures that a model is deallocated when all handles are dropped. No explicit lifecycle management is needed. A model created in `build()` is stored on the widget struct; when the widget is destroyed, the handle is dropped. If no other widget holds a clone, the model is deallocated. If a parent widget or ViewModel also holds a clone, the model persists.

### 15.10 Summary of Types

| Type | What it is | Who provides it | Ownership |
|------|-----------|----------------|-----------|
| `ListModel<T>` | Concrete reactive list. Owns the data as `Vec<T>`. Mutations emit `DataChange` automatically. | Framework | `Rc`-based, cloneable |
| `ListDataSource` | Trait for large/external datasets. Implementor owns the data and emits `DataChange` manually. | Application | Implementor-defined |
| `TreeModel<T>` | Concrete reactive tree. Owns the hierarchy. Mutations emit `TreeChange` automatically. | Framework | `Rc`-based, cloneable |
| `TreeSlice<T>` | Per-view flattened projection of a `TreeModel`. Owns expand/collapse state. Emits flat `DataChange`. | Framework (created internally by TreeView) | Owned by TreeView |
| `DataChange` | Enum describing a flat list mutation (inserted, removed, moved, updated, reset). | Framework | Value type |
| `TreeChange` | Enum describing a tree mutation (inserted, removed, moved, updated, reset, with node/parent info). | Framework | Value type |
| Entity-specific models (e.g., `WorkspaceProjectsModel`) | Generated struct wrapping `ListModel<T>` + parent ID signal + event wiring + loading state. | Qleany generator | `Rc`-based via contained fields |

---

## 16. Canvas API

### 16.1 Purpose

The Canvas is the high-level drawing API that widget authors program against. It replaces direct `RenderFrame` manipulation with operations that match how developers think about graphics — shapes, colors, text, transforms.

```rust
fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
    canvas.fill_rounded_rect(bounds, CornerRadius::uniform(6.0), theme.colors.primary);
    canvas.draw_text(&self.label, bounds.center(), &theme.typography.label);
}
```

### 16.2 Three-Tier Rendering

The Canvas internally classifies each drawing operation and routes it to the appropriate rendering tier.

**Tier 1 — Axis-aligned rectangles.** `fill_rect`, `stroke_rect`, simple `draw_line`. Translated directly to `DecorationRect` entries. Zero rasterization cost. Covers the majority of UI drawing.

**Tier 2 — SDF shader shapes.** Rounded rectangles, circles, ellipses, gradients. Rendered as quads with a specialized fragment shader computing signed distance fields. Smooth antialiasing at any resolution without rasterization. The `RenderFrame` gains `ShapeQuad` entries for this tier.

**Tier 3 — Arbitrary paths.** Complex shapes, custom curves, SVG icons. Rasterized on CPU via tiny-skia, cached in a shape atlas, rendered as textured quads. Rasterization is amortized by caching — static paths rasterize once.

### 16.3 Path Builder

The `Path` type provides a builder for arbitrary shapes:

```rust
let star = Path::star(center, outer_radius, inner_radius, 5);
canvas.fill_path(&star, Color::GOLD);
```

### 16.4 Text Integration

The Canvas delegates text rendering to the shared `Typesetter` instance from text-typeset. `draw_text` handles simple single-line text. `draw_text_layout` renders pre-measured text for cases where layout measurement and painting are separated. The RichTextEditor widget uses `draw_render_frame` to embed a complete text-typeset `RenderFrame` at a specific position, sharing the same glyph atlas.

### 16.5 Paint Types

Beyond solid colors, the Canvas supports `Paint` types: `LinearGradient`, `RadialGradient`, and `Image`. Gradients are rendered in the SDF fragment shader (Tier 2).

---

## 17. Rendering Pipeline

### 17.1 Frame Lifecycle

A frame is produced only when something has changed. Between frames, the application is idle and the GPU is quiescent. The frame lifecycle has five phases executing sequentially on the main thread.

**Phase 1: Event processing.** Raw input from winit is translated and dispatched through the widget tree. State changes from property bindings are resolved. Widgets are marked dirty.

**Phase 2: Layout.** SwiftUI-style negotiation runs only on dirty subtrees. Output: positioned rectangle for every active widget.

**Phase 3: Accessibility sync.** The AccessKit tree is updated incrementally — only changed nodes are pushed.

**Phase 4: Paint.** Each dirty widget's `paint()` is called with a Canvas. The Canvas accumulates drawing operations and produces a merged `RenderFrame`.

**Phase 5: GPU submission.** Atlas textures are uploaded, vertex buffers are built, draw calls are issued through wgpu. The surface presents.

### 17.2 RenderFrame

The `RenderFrame` is the boundary between platform-independent logic (fern-core, fern-canvas) and GPU-specific code (fern-render). It contains five drawable types: `GlyphQuad` (textured from glyph atlas), `ImageQuad` (textured from image), `DecorationRect` (untextured colored rectangle), `ShapeQuad` (SDF-rendered shape), and `RasterizedQuad` (textured from shape atlas). A `draw_order` array records painter's order (back-to-front) for correct occlusion across all drawable types.

### 17.3 GPU Pipeline

Three shader pipelines in fern-render: the **quad pipeline** (textured quads for glyphs, images, rasterized paths), the **rect pipeline** (untextured colored quads for decorations), and the **SDF pipeline** (signed distance field shapes with optional gradient fills). A typical frame produces five to six draw calls total.

### 17.4 Atlas Management

Three atlas textures serve different purposes. The **glyph atlas** is owned by the shared Typesetter (from text-typeset), containing rasterized glyph bitmaps. The **shape atlas** stores Tier 3 rasterized path results from tiny-skia. The **image atlas** (or texture array) stores application images. All use LRU eviction — dormant widgets' entries age out naturally.

### 17.5 Dirty Tracking

Each widget has a dirty flag at two granularities: **needs relayout** (size may have changed) and **needs repaint** (appearance changed, size unchanged). Clean widgets replay cached Canvas output without recomputation.

---

## 18. HiDPI and Scaling

Layout works in logical pixels. Rendering works in physical pixels. The conversion happens at the boundary between Phase 4 (paint) and Phase 5 (GPU submission).

`SizeProposal`, widget dimensions, spacing, padding, and font sizes are all logical. The Canvas also works in logical coordinates — `canvas.fill_circle(center, 10.0, color)` draws a circle with a 10-logical-pixel radius regardless of display density.

The scale factor is applied in two places: text-typeset rasterizes glyphs at physical pixel size (logical × scale factor), and fern-render multiplies screen coordinates by the scale factor when building vertex buffers.

When the scale factor changes (window dragged to a different monitor), the glyph and shape atlases are invalidated and a full relayout is triggered.

---

## 19. Theming — Design Tokens

### 19.1 Why Not QPalette

QPalette organizes colors along two axes: color roles (Window, WindowText, Button, ButtonText, Base, Text, Highlight, HighlightedText) and color groups (Active, Inactive, Disabled). This design has four structural problems that FernUI must avoid. The role set is fixed — adding `SidebarBackground` or `AccentHover` requires modifying QPalette's source. The roles are widget-centric rather than semantic — `Button` and `Window` name specific widgets, not design intent. QPalette handles only color — there is no concept of spacing, typography, corner radii, or shadows. And the three color groups (Active, Inactive, Disabled) do not cover hover, pressed, focused, or other modern interaction states, forcing every widget to compute its own state-dependent colors by ad-hoc blending.

FernUI replaces QPalette with a design token system inspired by modern web/mobile design systems (Material Design, Atlassian Design Tokens, GitLab Pajamas) and SwiftUI's environment-based theming.

### 19.2 The Theme Struct

The `Theme` struct aggregates six token categories: `colors`, `layout`, `typography`, `shape`, `motion`, and `components`. All visual properties of all widgets are resolved from these tokens — no widget contains hardcoded visual values. The token naming follows the JetBrains Int UI reference design — surfaces are differentiated by **role** (main / content / raised / sunken / hover / pressed / selected), not by elevation, and borders are a uniform 1 dp with emphasis carried by color rather than thickness.

**ColorTokens** groups semantic color slots by purpose: surfaces (`surface_main`, `surface_content`, `surface_raised`, `surface_sunken`, `surface_hover`, `surface_pressed`, `surface_selected`, `surface_selected_inactive`); text / foreground (`text_primary`, `text_secondary`, `text_disabled`, `text_on_accent`, `text_link`, `text_link_hover`, `text_error`, `text_warning`, `text_success`); accent (`accent`, `accent_hover`, `accent_pressed`, `accent_disabled`, `accent_subtle_bg`); borders and dividers (`border`, `border_strong`, `border_focused`, `border_error`, `border_warning`, `divider`, `divider_strong`); status colors for banners and validation (`status_info_fg/bg`, `status_success_fg/bg`, `status_warning_fg/bg`, `status_error_fg/bg`); selection (`selection_bg_active`, `selection_text_active`, `selection_bg_inactive`, `selection_text_inactive`, `search_match_bg`, `search_match_current_bg`); scrollbar track/thumb variants; tooltip surface (kept dark in both themes as an Int UI house convention); an editor pane group (`editor_bg`, `editor_fg`, `editor_caret`, `editor_current_line_bg`, `editor_gutter_fg`, `editor_selection_bg`) architecturally separated from the general UI chrome; and miscellaneous (`focus_ring`, `focus_ring_error`, `scrim`).

**LayoutTokens** is deliberately small. Only two genuinely cross-cutting values live here: `control_gap` (sibling controls in a row or column) and `section_gap` (between sections of a panel or dialog). Per-widget spacing — button padding, menu item height, dialog gutters — lives on the per-component style structs in `components::ComponentStyles` rather than in a single generic scale. This is a deliberate departure from web-style "t-shirt" scales (`xs`/`sm`/`md`/`lg`/`xl`): Int UI sizes controls by role, not by size class.

**TypographyTokens** defines six named styles keyed to UI role, not heading level: `body` (13 sp regular — the default for buttons, field text, body copy), `body_bold` (13 sp semi-bold — the closest thing Int UI has to a heading), `small` (12 sp regular — secondary info, captions, hints), `small_bold` (12 sp semi-bold), `tiny` (11 sp regular — status bar, tag labels, timestamps), and `mono` (JetBrains Mono, 13 sp — code, paths, identifiers). Each `TextStyle` specifies family, size, weight, line height, and letter spacing; letter spacing is 0 everywhere (Int UI never tracks text). There are no heading-level tokens: section headers are `body_bold` with extra spacing above/below. Typography tokens bridge to text-typeset's `TextFormat` — the Canvas's `draw_text` method constructs a `TextFormat` from the resolved `TextStyle` when the widget does not provide an explicit format.

**ShapeTokens** covers three corner radii (`radius_control` at 4 dp for buttons, fields, combo boxes, checkboxes, and menu items; `radius_popup` at 8 dp for tooltips, balloons, large popups, dialogs, and panels; `radius_pill` at 9999 for fully rounded tags, chips, and badges), a uniform `border_width` (1 dp — Int UI has no thicker variant), focus ring geometry (`focus_ring_width`, `focus_ring_offset` — the ring is drawn *outside* the control with a 2 dp gap), and four shadow tiers (`shadow_xs` for tooltips, `shadow_sm` for menus and dropdowns, `shadow_md` for notification balloons, `shadow_lg` for modal dialogs). Dark-theme shadows use roughly 4× the alpha of light-theme shadows to remain visible against dark surfaces.

**MotionTokens** defines animation durations and easing curves consumed by the animation system and by time-dependent UI behaviors (tooltip delay, cursor blink rate).

**ComponentStyles** holds per-widget style structs — button padding, menu item geometry, scrollbar thickness, dialog insets, and similar widget-specific values that do not belong in a generic cross-cutting scale. Widgets read their own component style inside `size_that_fits` and `paint`.

### 19.3 Theme Storage and Access

The theme is held by the `WidgetTree` as two parallel handles: a plain `Theme` (the resolved tree-level value) and a reactive `Signal<Theme>` that widgets subscribe to. Both are updated in lockstep by `set_theme`. The tree is the single source of truth; `fern-app`'s `WindowManager` forwards application-level theme changes into each window's tree.

Widgets access the theme at three times through their context objects. `LayoutContext::theme` is read during `size_that_fits` and `place_children` for spacing, typography, and shape values. `PaintContext::theme` is read during `paint` for colors, corner radii, border widths, shadows, and text styles. `BuildContext::theme_signal` returns the reactive handle and is the preferred access path for any widget that needs theme values after `build` — derived signals built from `theme_signal` stay live across theme switches automatically. `BuildContext::theme` still exists as a synchronous read for things that are only needed once at build time (computing a widget ID, reading an initial layout constant), but any value derived from `theme` and captured into a closure becomes stale on the next theme switch and should be expressed as a role or a `theme_signal` binding instead.

The preferred API surface for widget authors is the role enums and the `ColorProp` / `TextStyleProp` wrappers documented in [docs/reactive-theme.md](./reactive-theme.md). Widgets store roles (`TextRole::Primary`, `SurfaceRole::AccentSubtle`, `BorderRole::Focused`, `TextStyleRole::Body`) and resolve them against the current theme at paint or layout time via the `resolve(&theme)` methods on each role enum and prop wrapper. Writing `.bind_color(theme_signal.map(|t| t.colors.text_primary))` is still valid but almost never necessary: most widget-authoring use cases reduce to `.color(TextRole::Primary)` or a `Signal<Role>` emitted by an interaction signal.

### 19.4 Environment Propagation and Subtree Overrides

The theme flows downward through the widget tree via the environment system. Every widget inherits the theme from its nearest ancestor. A subtree can override the theme partially — a dark sidebar inside a light application is achieved by overriding only the color tokens within that subtree while inheriting layout, typography, shape, motion, and component tokens from the parent.

The override is applied via `WidgetTree::set_theme_override(id, |theme| …)` on any widget node. The override closure receives a mutable `Theme` and mutates the fields it wants to change. The modified theme becomes the resolved value for all descendants of that node. Setting an override marks the target's subtree dirty for layout and paint — it does not rebuild composites.

Nodes without overrides inherit from their parent with no per-node storage cost. Only nodes with active overrides carry a boxed `ThemeOverride` closure. The environment lookup walks up the arena until it finds a node with an override or reaches the root, which holds the tree-level theme.

Subtree overrides compose with `set_theme`. When the tree-level theme changes, overrides are re-applied on top of the new base theme lazily — the next time any widget inside the overridden subtree asks for its resolved theme via `resolve_theme`. A dark sidebar override that sets `colors = ColorTokens::dark_default()` will still produce dark colors regardless of whether the base theme was light or dark — the override replaces the color tokens entirely. There is no rebuild cascade; widgets observing the tree-level `theme_signal` see the new base value and re-paint, and resolvers that walk the arena return the new overridden composition.

### 19.5 Theme Switching from Handlers

Theme switching is an application-level action triggered by the user (selecting a theme in preferences, toggling dark mode). In FernUI's architecture, this action flows through a handler like any other user action — typically an `Action` keyed by an intent name, fired by a button or shortcut.

`EventContext` (the context object received by every widget handler and every `Action`) exposes `set_theme()` and `set_locale()` methods. When called, the framework updates the tree-level `Signal<Theme>` (or `Signal<Option<String>>`), stores the new tree-level `Theme`, and calls `arena.mark_all_dirty()` to force a repaint and relayout pass. It does **not** rebuild composites, it does **not** destroy any widget state, and it does **not** clear interaction state. Focus, hovered widget, scroll offsets, text-input cursors, expanded accordion sections, and any other per-widget interaction state are preserved across the switch.

The cost model follows from this design: a theme switch is a paint (and possibly a relayout when typography sizes change), not a structural rebuild. It is cheap enough to be viable on a keystroke path if an application wants to support live theme preview.

Widget-level reactivity works because the role enums and prop wrappers resolve against the current theme at every paint. A button that stored `SurfaceRole::AccentSubtle` on first build still resolves to the correct accent-subtle color after the switch without any rebuild. Widgets that need to react to theme changes at build time — for example, to compute a derived signal once — subscribe to `ctx.theme_signal()` and let the signal graph handle the update.

For the locale system, `set_locale()` follows the same pattern. The tree-level `Signal<Option<String>>` updates and the arena is marked dirty. Per-string reactivity flows through `LocalizedString::to_signal()`, which observes the fern-i18n manager; any other widget state that depends on the tree-level locale can bind to `ctx.locale_signal()`.

For `fern-app` specifically: when the user has multiple windows open and triggers a theme change on one of them, the `WindowManager` forwards the new theme to each window's `WidgetTree::set_theme`, so every window repaints with the new tokens on the next frame. No cross-window rebuild is needed.

### 19.6 Built-In Themes

FernUI ships with two built-in themes: `Theme::light_default()` and `Theme::dark_default()`. These provide sensible defaults for all token categories. They are not intended to be visually distinctive — they are neutral baselines that applications customize.

Custom themes are created either from scratch or by modifying an existing theme using Rust's struct spread syntax:

```rust
let editor_light = Theme {
    colors: ColorTokens {
        accent: Color::from_hex("#2E7D32"),
        accent_hover: Color::from_hex("#1B5E20"),
        text_on_accent: Color::WHITE,
        surface_main: Color::from_hex("#FAFAF5"),
        ..ColorTokens::light_default()
    },
    typography: TypographyTokens {
        body: TextStyle {
            family: "Literata".to_string(),
            size: 16.0,
            ..TextStyle::default()
        },
        ..TypographyTokens::default()
    },
    ..Theme::light_default()
};
```

### 19.7 Serialization and User-Defined Themes

The `Theme` struct and all its sub-structs derive `Serialize` and `Deserialize` (via serde). This enables themes to be loaded from files in any format the application chooses (TOML, JSON, YAML). A theme file defines token values; the application deserializes it into a `Theme` and applies it via `set_theme()`.

This enables user-created themes, theme marketplaces, and runtime theme loading — all without code changes. The application loads the file, deserializes it, and applies the result. Incomplete theme files can be handled by deserializing with defaults — missing fields fall back to `Theme::light_default()` or `Theme::dark_default()` values.

### 19.8 Accessibility-Driven Theming

High-contrast, large-text, and reduced-motion accessibility needs are served by theme variants, not by separate accessibility mechanisms. A high-contrast theme overrides color tokens to meet WCAG AAA contrast ratios. A large-text theme increases all typography token sizes by a configurable factor. A reduced-motion theme sets all motion token durations to zero.

The `PaintContext` exposes OS-level accessibility preference flags: `prefers_high_contrast` (from the OS accessibility settings), `prefers_reduced_motion` (from the OS), and `prefers_large_text` (from the OS or from application preferences). The application reads these flags at startup and selects an appropriate theme variant. FernUI does not automatically apply accessibility themes — the application makes the choice, because the correct response to `prefers_high_contrast` depends on the application's visual design (a dark-themed application may already meet contrast requirements).

The relationship between the theme and AccessKit is indirect. The theme determines visual appearance; AccessKit determines what screen readers announce. They are independent systems that happen to be affected by the same user preference. A high-contrast theme changes colors but does not change AccessKit node properties. A large-text theme changes font sizes, which affects layout and therefore AccessKit node positions, but does not change roles or names.

### 19.9 Theme and Text-Typeset Integration

Typography tokens must bridge to text-typeset's `TextFormat`. The `TextStyle` struct in the theme maps directly to the fields that `TextFormat` expects: font family, font size, font weight, line height, letter spacing. The Canvas's `draw_text` method constructs a `TextFormat` from the theme's `TextStyle` when the widget does not provide an explicit format. This means changing the theme's typography tokens automatically changes how all text in the application renders — button labels, menu items, tooltips, headings — without any widget code changes.

The font size in the theme is specified in logical pixels. Text-typeset rasterizes glyphs at physical pixel size (logical × scale factor), so the glyph atlas is sensitive to both theme changes (which may change font size) and scale factor changes (which change physical pixel size). Both trigger atlas invalidation and re-rasterization.

### 19.10 Role Enums and Reactive Props

The widget-authoring surface for the theme is role-based. Rather than storing concrete colors or text styles on widget structs, widgets store semantic *roles* and resolve them against the current theme at the moment they are needed. Four role enums live in `fern-tokens::roles`: `TextRole` (foreground colors — `Primary`, `Secondary`, `Disabled`, `OnAccent`, `Accent`, `Error`, `Warning`, `Success`, `Link`, `LinkHover`, `TooltipText`, `TooltipShortcut`, `EditorFg`, `EditorGutterFg`); `SurfaceRole` (backgrounds, including an explicit `Transparent` variant so a chain like "transparent at rest, accent-subtle on hover, accent on pressed" can be expressed without a separate "no background" code path); `BorderRole` (focus rings, error borders, dividers, also with a `Transparent` variant); and `TextStyleRole` (the six typography slots: `Body`, `BodyBold`, `Small`, `SmallBold`, `Tiny`, `Mono`). Each role has a `resolve(&tokens)` method that returns the concrete value.

The `ColorProp` and `TextStyleProp` wrappers in `fern-core::color_prop` let widget builders accept any of: a literal `Color` or `TextStyle`; a role; a `Signal<Color>` / `Signal<TextStyle>`; a `Prop<T>`; or — importantly — a `Signal<Role>`. The `Signal<Role>` variant is the canonical pattern for interaction-driven colors: the interaction signal (`Idle | Hover | Pressed | Focused`) maps to `Signal<SurfaceRole>` via `interaction.map(|s| match s { ... })`, and the surface resolves to the right color on every paint. This replaces the older "zip interaction with theme_signal and compute a color" pattern — the widget never sees a concrete color until paint time, so theme switches propagate for free. See the Button widget for a reference implementation and [docs/reactive-theme.md](./reactive-theme.md) for the full API surface, DX guidance, and migration cheat sheet.

---

## 20. Threading Model

### 20.1 Single UI Thread

All five phases of the frame lifecycle run sequentially on the main thread. The widget tree, state arena, overlay manager, Canvas, and all contexts are non-`Send` types — the compiler prevents accidental access from background threads.

This matches Qleany's synchronous model. A Qleany controller call from a FernUI command handler executes synchronously. No `async`/`await`, no tokio, no runtime.

### 20.2 Background Work

Long operations use Qleany's `LongOperationManager`, which runs use cases on background threads. The background thread communicates with the UI thread through winit's `EventLoopProxy` — a unidirectional channel that wakes the event loop and delivers custom events. The UI thread processes these events like any other input, triggering data source refreshes and widget repaints.

### 20.3 Incremental Work

Operations that take 5–50ms (too short for a background thread, too long for a single frame) are broken into chunks via `request_idle_callback`. The event loop runs idle work during gaps between frames, respecting a time budget.

### 20.4 Event Loop

The winit event loop uses `ControlFlow::Wait` — it sleeps when no events are pending and no widgets are dirty. CPU and GPU consumption is near-zero when the user is not interacting.

### 20.5 Animation

FernUI does not ship a separate animation subsystem. Animation is a thin layer over `Signal<f32>`: `signal.animate_to(target, duration, easing)` asks the tree's `AnimationScheduler` to smoothly interpolate the value over time, and any widget bound to the signal re-paints on each tick as the value slides. The scheduler integrates with the frame lifecycle (pause when the window is occluded, rebase on resume, skip offscreen ticks, cancel animations on widget rebuild/destroy), so widgets never own animation lifetime manually.

The design intent is narrow: motion is reserved for a small set of floating transitions — dialog appearance, snackbar slide-in, accordion expansion, toggle thumb motion, indeterminate progress, smooth programmatic scroll. Hover, press, and focus state changes are explicitly *instant* in Int UI's vocabulary; they are expressed as `Signal<Role>` mapped from an interaction signal and resolved per-frame through the theme, not through the animation scheduler. Looping animations respect `ctx.prefers_reduced_motion()`.

Full rationale, API, worked examples, and testing patterns: [`animation.md`](animation.md).

---

## 21. Accessibility

### 21.1 First-Class Integration

AccessKit is integrated at the trait level. The unified `Widget` trait includes an `accessibility()` method that declares the widget's role, name, state, and available actions. Widgets that compose children via `build()` automatically contribute AccessKit nodes for each child as part of the arena traversal — the composite itself declares its own identity (e.g. `Role::Group` or `Role::Button`) and its children's nodes sit underneath.

### 21.2 Structural Guarantees

The focus system, overlay system, keyboard shortcut system, and drag-and-drop system all have accessibility paths designed in. AccessKit actions (from screen readers) flow through the same event system as pointer and keyboard input — `WidgetEvent::AccessAction`. The test harness queries widgets by AccessKit role and label, ensuring accessibility is verified by every test.

### 21.3 Dormancy and Overlays

Dormant subtrees produce no AccessKit nodes (screen readers only see active content). Overlay content generates correct AccessKit tree structures — tab lists have `Role::TabList` and `Role::Tab` nodes, menus have `Role::Menu` and `Role::MenuItem` nodes, tooltips are linked to their anchor widget via `DescribedBy`.

---

## 22. Window Management

Full reference: [`docs/multi-window.md`](multi-window.md). Canonical end-to-end example: [`examples/multi_window`](../examples/multi_window/src/main.rs).

### 22.1 Architecture: Per-Window Trees with Shared Application State

Each window owns its own independent `WidgetTree`, its own layout pass, its own paint pass, its own `RenderFrame`, and its own wgpu surface. What windows share is application-level context: the theme, the locale, the `ShortcutRegistry`, the data-model handles (`ListModel` / `TreeModel` clones are cheap `Rc` handles), the root widget's registered `Action`s, and any app-scoped backend wiring. Same model as Qt, SwiftUI, WPF.

Multi-window management lives in `fern-app` (`WindowManager` + `WindowOpsImpl`). `fern-core` owns the abstractions: `WindowConfig`, reactive `WindowState`, the `WindowOps` trait consumed by `EventContext`, and `DecorationsMode` / `WindowPlacement` / `WindowIcon`. `fern-platform` routes events by winit `WindowId` and hosts custom-chrome backends. `WidgetTree` stores the hosting window's `WindowState` on itself (`Option<WindowState>`), reachable via `BuildContext::window()` / `EventContext::window()`; beyond that, tree / layout / dispatch / rendering are unchanged from the single-window case.

### 22.2 `WindowConfig` — the single creation entry point

A `WindowConfig` describes any window. One uniform surface for both the initial window (passed to `FernAppBuilder::initial_window`) and every secondary window (passed to `EventContext::open_window`). No `.window_title` / `.window_size` / `.root` shims on `FernAppBuilder`:

```rust
WindowConfig::new()
    .title("Inspector")
    .id("inspector")                             // find_window key
    .size(420, 640)
    .min_size(320, 400)
    .initial_placement(WindowPlacement::Floating)
    .decorations(DecorationsMode::CustomChrome)  // Native | CustomChrome | None
    .resizable(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .icon(WindowIcon::from_rgba(rgba, w, h))
    .modal(ModalConfig { parent, focus_target }) // Option<ModalConfig>
    .root(|tree, state| tree.add(Inspector::new(state)))
```

`WindowPlacement` is a 4-variant enum (`Floating` / `Maximized` / `Fullscreen` / `Minimized`) — **not** three booleans. Size and position are independent signals on `WindowState` holding the last-known *restored* values, mirroring macOS `frameAutosaveName` and Windows `WINDOWPLACEMENT` so un-maximize / un-fullscreen return the window to the right rect.

Modal is `Option<ModalConfig>` carrying `parent: FernWindowId` and `focus_target: Option<WidgetId>` — the type system enforces that a modal names its parent.

### 22.3 `WindowState` — per-window reactive state

Per-window state is a cloneable `Rc<WindowStateInner>` handle holding a `Signal<T>` per field:

```rust
state.placement()    // Signal<WindowPlacement>
state.title()        // Signal<String>
state.size()         // Signal<(u32, u32)>
state.position()     // Signal<(i32, i32)>
state.focused()      // Signal<bool>
state.resizable()    // Signal<bool>
state.always_on_top()// Signal<bool>
```

Widgets bind against these signals through `ctx.window()` at build time (or capture a clone at `WindowConfig::root`). One canonical example: a toolbar fullscreen-toggle label that re-renders automatically whether the change came from the button, the F11 shortcut, or the green traffic light on macOS:

```rust
let fs = ctx.window().unwrap().placement().map(|p| p.is_fullscreen());
Button::new().bind_label(fs.map(|f| if f { "Exit fullscreen" } else { "Fullscreen" }))
```

### 22.4 Two-way OS↔state sync with a re-entrancy guard

Every `WindowState` signal has two writers:

- **App-side** — `state.placement().set(Fullscreen)` fires the signal's observer which queues a `WindowCommand::SetPlacement(Fullscreen)` on `pending_os_commands`.
- **OS-side** — winit fires `Resized` / `Moved` / `Focused`; the app-level manager translates to `state.set_*_from_os(new)` which flips `applying_from_os: true` before updating the signal. The observer sees the guard set and skips enqueuing — the OS already knows.

Each event-loop tick, `WindowManager::drain_window_commands` drains every window's queue and translates commands into the appropriate winit call. Without the guard, OS-initiated state changes would loop back through the observer as redundant OS calls — at best wasteful, at worst a mid-animation state-drift bug ([Compose Multiplatform #1489](https://github.com/JetBrains/compose-multiplatform/issues/1489) is the cautionary tale). The guard is the single concrete mechanism that makes `WindowState` safe as a shared source of truth across the app/OS boundary.

### 22.5 Window lifecycle

`FernAppBuilder::run()` consumes a `WindowConfig` via `.initial_window(...)` and delegates to `WindowManager::create_window(config, event_loop)` on `resumed()`. Creation is synchronous — the winit window is built, the `WidgetTree` is built via `config.root`, `WindowState` observers are wired, and `ManagedWindow` is registered before returning.

Every other window opens the same way. From a handler, `ctx.open_window(config)` routes through `WindowOpsImpl` (see §22.8) into the same `create_window` call; the returned `FernWindowId` is immediately usable for `focus_window` / `window_state(id)` / subsequent `open_window` calls referencing it as a modal parent.

Closure is initiated by the user (OS close button → `WindowEvent::CloseRequested`), by the application (`ctx.close_window()` for the current window, `ctx.close_window_by_id(id)` for any, `state.close()` via the command queue), or by a custom-chrome title bar (`TitleBarHostCallbacks::request_close` → user event → `CloseWindowRequest` → `queue_close`). All paths funnel into `pending_closes`, drained once per tick in `process_pending`.

### 22.6 Event routing

Winit tags every event with a window ID. `FernAppHandler` translates the event and dispatches via `dispatch_in_window(window_id, event, event_loop)` (see §22.8 for re-entry mechanics). Keyboard events go to the active window (OS focus), pointer events to the window they occurred in. Focus/blur/occluded/resize/move all route to their window.

Keyboard shortcuts are per-window: each tree consults the shared `ShortcutRegistry`, and a shortcut fires in whichever window holds OS focus.

### 22.7 `EventContext` multi-window API

Every handler receives an `EventContext<'_>` carrying `&'_ mut dyn WindowOps` for the duration of dispatch:

```rust
impl EventContext<'_> {
    pub fn window(&self) -> Option<&WindowState>;               // source window
    pub fn open_window(&mut self, config: WindowConfig) -> FernWindowId;
    pub fn open_modal(&mut self, req: ModalRequest) -> Option<FernWindowId>;
    pub fn find_window(&self, string_id: &str) -> Option<FernWindowId>;
    pub fn focus_window(&mut self, id: FernWindowId);
    pub fn close_window(&mut self);                             // current window
    pub fn close_window_by_id(&mut self, id: FernWindowId);
    pub fn window_state(&self, id: FernWindowId) -> Option<WindowState>;
    pub fn windows(&self) -> Vec<WindowState>;
}
```

Idiomatic idempotent open (single-instance preferences, inspector, help):

```rust
ctx.register_action(Action::new("app.help").on_invoke(|_i, ctx| {
    if let Some(id) = ctx.find_window("help") {
        ctx.focus_window(id);
        return;
    }
    ctx.open_window(
        WindowConfig::new()
            .title("Help")
            .id("help")
            .size(720, 480)
            .root(|tree, _state| tree.add(HelpRoot::new())),
    );
}));
```

Cross-window read (dim inspector when main is fullscreen):

```rust
if let Some(main_id) = ctx.find_window("main") {
    if let Some(main_state) = ctx.window_state(main_id) {
        let dim = main_state.placement().map(|p| p.is_fullscreen());
        // …
    }
}
```

### 22.8 `WindowOps` trait + dispatch re-entry

`WindowOps` is a trait in `fern-core` implemented by `WindowOpsImpl` in `fern-app`. This inverts the crate dependency: `EventContext` in `fern-core` references the trait, and the impl in `fern-app` carries `&mut WindowManager` + `&ActiveEventLoop` + the current window's raw platform handle — everything `open_window` needs to reach winit synchronously.

Re-entry pattern — `FernAppHandler::dispatch_in_window`:

1. `self.wm.take_managed(winit_id)` temporarily removes the dispatching window from the map.
2. Constructs `WindowOpsImpl::new(&mut self.wm, event_loop, current_id, current_handle)`. The `&mut self.wm` borrow excludes the current window, which is now held locally as `current`.
3. `current.tree.dispatch_event_with_ops(event, &mut ops)` runs handlers. Any `ctx.open_window(...)` call reaches `self.wm.create_window(...)` directly — no borrow conflict because the map entry for the current window isn't live.
4. Reinsert the current window.

Modal parent self-reference works because the current window's raw handle is stashed on the ops object before removal; `create_window`'s parent-attach path uses it when `modal_parent == current_id`.

The same threading applies to `tick_gestures_with_ops`, `layout_with_ops`, and `render_with_ops` — every frame-scope handler (drag-tick, delayed-overlay activation, state-driven rebuild of a composite) can open windows too. Standalone test paths use `NoopWindowOps`, which panics on `open_window` by design.

### 22.9 Modal dialogs

A modal `WindowConfig` sets `modal: Some(ModalConfig { parent, focus_target })`. `WindowManager::create_window` then:

- sets `WindowLevel::AlwaysOnTop`
- wires OS-level parent attachment: `with_parent_window` on non-macOS (Win32 owner, X11 `WM_TRANSIENT_FOR`, `xdg_toplevel.set_parent`); `attach_child_window` (AppKit `addChildWindow:ordered:`) on macOS after the AccessKit adapter is installed
- records the parent in `modal_blocked` so events for the parent are routed to refocusing the modal child

`EventContext::open_modal(request)` is a thin wrapper: it builds a `WindowConfig` from the `ModalRequest`'s title / size / focus_target and calls `open_window`. There is no separate modal drain — one create path, zero special-casing.

### 22.10 Modeless dialogs

Regular secondary windows. Construct a `WindowConfig` with no `.modal(...)` and call `ctx.open_window(...)`. They share app-level state (theme, locale, data models, shortcut registry) with their creator but own their own tree, focus, and rendering.

### 22.11 Custom chrome

`DecorationsMode::CustomChrome` constructs a `PlatformTitleBarHost` alongside the window and attaches it to the tree. The `TitleBar` widget retrieves the host via `WidgetTree::title_bar_host()` for chrome-specific operations (drag region, resize borders, platform-specific insets, system window menu on Wayland).

The host's interface is intentionally minimal — `reserved_leading_inset`, `reserved_trailing_inset`, `renders_custom_controls`, `needs_custom_resize_handles`, `begin_drag`, `begin_resize`, `show_window_menu`, `update_hit_regions`. State operations (`minimize`, `toggle_maximize`, `close`, `is_maximized`) live on `WindowState` instead; the `TitleBar` widget's maximize button binds directly to `ctx.window().placement()`. One consequence: the maximize glyph swap now works with `DecorationsMode::Native` too — previously custom-chrome-only.

### 22.12 Data source sharing

Unchanged from the single-window model. Data models are app-level `Rc` handles; multiple windows observe the same `ListModel<T>` / `TreeModel<T>` and update independently when the domain model changes.

### 22.13 Focus across windows

Each tree has its own `FocusManager`. OS-level focus (`WindowEvent::Focused`) determines which window receives keyboard events; the widget-level focus inside that window is preserved across deactivation and restored on reactivation. Focus changes inside a window — including ones triggered by handlers that opened new windows — thread the ops sink through so focus-lost / focus-gained handlers can themselves open windows if they want to.

### 22.14 Impact on crate structure

- `fern-core` owns the window abstractions: `WindowConfig`, `WindowState`, `WindowOps` trait, `WindowPlacement`, `DecorationsMode`, `WindowIcon`, `FernWindowId`, `WindowCommand`, `NoopWindowOps`, plus `EventContext`/`BuildContext` integration.
- `fern-app` owns `WindowManager`, `WindowOpsImpl`, the `dispatch_in_window` re-entry pattern, the OS→state writeback in `handle_window_event_inner`, and the per-tick drains (`drain_window_commands`, `process_pending`).
- `fern-platform` owns `PlatformTitleBarHost` backends (macOS / Windows / Wayland; X11 falls back to native decorations), parent-child attachment, and the OS-placement query used when `WindowEvent::Resized` fires.
- `fern-widgets`' `TitleBar` widget consumes `WindowState::placement` and the (trimmed) `PlatformTitleBarHost` interface.
- Every other crate (`fern-canvas`, `fern-tokens`, `fern-data`, `fern-i18n`, `fern-text`, `fern-render`) is untouched by the multi-window machinery.

---

## 23. Settings and Persistence

User-visible state survives across sessions through `fern-settings`,
which sits between `fern-data` (reactive models) and `fern-widgets`
(consumers). The crate exposes three persistence shapes — a dynamic
K/V store, a typed-struct file, and reactive-collection bridges —
plus two built-in services (`MruList<T>` for recents-style lists, and
`WindowStateService` for window geometry) that fern-app wires
automatically.

The architectural rule is **in-memory is the source of truth**.
Widgets bind against `Signal<T>` / `ListModel<T>` / `TreeModel<T>`
handles whose mutations propagate (a) to the UI through the
existing reactive graph, and (b) to disk through a debounced atomic
writer. Disk is a flushed projection of the in-memory state, never
the read path during runtime — files are loaded once at startup and
written on change. This is the same shape used by
[reactive-theme.md](reactive-theme.md): the *signal* is the
authoritative copy, anything else is a downstream observer.

For the user-facing API surface, see
[`docs/settings.md`](settings.md). This section covers only the
architectural decisions and their consequences for the rest of the
framework.

### 23.1 Three persistence shapes, not one

| Shape               | Type                                                  | Use for                                                            |
| ------------------- | ----------------------------------------------------- | ------------------------------------------------------------------ |
| Dynamic K/V         | `SettingsStore` → `Signal<T>`                         | Scalar prefs (font size, theme name, bools, arrays of scalars)     |
| Typed file          | `SettingsFile<T>` (with `Versioned` + `Migrator<T>`)  | App-shaped structs with their own schema and migrations            |
| Reactive collection | `PersistedListModel<T>` / `PersistedTreeModel<T>`     | Anything driving a `Repeater` / `ListView` / `TreeView`            |

The shapes can't be collapsed into one without losing information:

- **`SettingsStore`** stores scalars under dotted keys (`editor.font_size`,
  `ui.theme`). The on-disk form is a TOML map. Struct values are
  rejected at registration with a clear error: TOML serializes structs
  as tables, indistinguishable on a re-read from "a parent of nested
  keys" — so allowing `signal::<MyStruct>("foo")` would corrupt the
  K/V model the moment another caller did `signal::<f32>("foo.bar")`.
- **`SettingsFile<T>`** owns one struct per file and is the right tool
  for arbitrary app schemas. It's where the `Versioned` /
  `Migrator<T>` machinery lives. Migrations operate on raw
  `toml::Value` *before* deserialize — a v1 payload that no longer
  matches the v2 type can still be upgraded.
- **`PersistedListModel<T>` / `PersistedTreeModel<T>`** wrap the
  reactive `*Model<T>` (§15) and re-serialize on every mutation,
  debounced. They're a convenience layer over `SettingsFile<ListFile<T>>`,
  not a separate primitive.

A common temptation is to back recents with `Signal<Vec<T>>`. That
would full-rebuild every `Repeater` on every add — the persisted
list bridge keeps the incremental-update contract `ListModel`
provides.

### 23.2 `MruList<T: MruEntry>` — generic recents

`MruList` is the only collection-shaped service the framework ships,
and it's deliberately *generic*. Earlier drafts had a hardcoded
`RecentsService` typed to `RecentProject`; that put application
vocabulary ("projects") into a framework crate. The current design
exposes a small `MruEntry` trait (dedupe key, optional pin flag,
optional touch hook) and lets apps define their own item type:

```rust
pub trait MruEntry: Clone + Serialize + DeserializeOwned + 'static {
    type Key: PartialEq + ?Sized + 'static;
    fn key(&self) -> &Self::Key;
    fn is_pinned(&self) -> bool { false }
    fn set_pinned(&mut self, _pinned: bool) {}
    fn touch(&mut self) {}
}
```

`Key: ?Sized` makes unsized keys like `Path` and `str` work without
forcing apps to box them. The dedupe / pin-aware-cap policy lives in
`MruList`; the schema lives in the app type. Apps register their
`MruList<T>` via `FernAppBuilder::app_state(handle.clone())` and
recover it through `ctx.mru::<T>()` (the `SettingsExt` accessor).

The framework knows nothing about projects, files, palettes, or
saved searches — those are application concepts.

### 23.3 `WindowStateService` and framework-driven save/restore

Window geometry is the one persistence concern that genuinely belongs
in the framework: it interacts with `WindowConfig`, the winit window
manager, and the `WindowState` signals (§22). `WindowStateService`
stores per-`label` entries — a multi-window app records each window
under its own id. The label is the same `string_id` set via
`WindowConfig::id(...)` (the existing `find_window` lookup key), so
the persistence entry is keyed by an identifier the rest of the
framework already recognizes.

A window participates in auto save / restore when both:

1. Its `WindowConfig` carries `id(...)` (a stable string label), **and**
2. A `WindowStateService` is registered (via `SettingsBundle::with_window_state(true)`).

That naturally excludes modal dialogs, popovers, and any transient
surface that never asked for an id. The integration lives in
`fern-app`'s [`window_persist.rs`](../crates/fern-app/src/window_persist.rs):

- **Restore.** At the top of `WindowManager::create_window`, before
  any winit attribute is built, the service is consulted and the
  saved `PerWindowState` is sanitized against the active monitor's
  work area (queried via `winit::ActiveEventLoop::primary_monitor()`,
  converted to logical pixels with the monitor's scale factor). The
  sanitized values are written back into the `WindowConfig` so the
  window opens at the right geometry from the first frame.
- **Save.** Once the `WindowState` exists, observers are installed
  on the `size`, `position`, and `placement` signals (at the end of
  `create_window`, before the `ManagedWindow` is inserted into
  `WindowManager.windows`). Each observer fires `service.record(...)`
  with the current full state. The `ObserverHandle`s are stashed on
  `ManagedWindow._persist_handles` so they live exactly as long as
  the window itself; removing the window drops the handles, which
  unsubscribes the observers.

This split (restore *before* winit, save *after* `WindowState`) is
load-bearing. Restoring inside the root widget's `build()` would
race the OS layer's initial sync; saving inside `WindowManager` lets
the same `applying_from_os` re-entrancy guard from §22.4 prevent
echo loops.

### 23.4 Sanitizing geometry against the current monitor

`PerWindowState::sanitize(min_size, work_area)` is pure math; the
caller (`window_persist.rs`) supplies the work-area hint. The policy:

- **Width / height** clamp to `[min, work_area]`. A 4K saved size on
  a 1080p host comes back at `1920×1080`.
- **Position is checked per-axis** against a 50-pixel intersection
  test with the work area. A window saved at `x = 2200, y = 100` on
  a now-disconnected secondary monitor recenters its `x` (the saved
  X axis no longer overlaps the work area) but *keeps* `y = 100`
  (it was always on-screen vertically). A fully off-screen position
  recenters both axes.
- **`WindowPlacement::Minimized`** is downgraded to `Floating` on
  restore. A window that comes back invisible looks like the app
  failed to start — no other native platform behaves that way.
- **Original on-disk state is untouched.** Re-plugging the missing
  monitor restores the original geometry on the next launch — the
  sanitize step is a *runtime adjustment*, not a destructive write.

### 23.5 Wayland constraint

Wayland's xdg-shell protocol does not expose
`set_position(x, y)` — the compositor is the sole authority on
window placement, by design (security, tiling-manager compatibility,
multi-output policy). Concretely, on Wayland:

- `winit::Window::set_outer_position(...)` silently no-ops.
- `winit::Window::outer_position()` returns
  `Err(NotSupportedError)`.
- The position observer in `window_persist.rs` rarely fires.
- `WindowState.position` keeps whatever value we initialized it
  with at `WindowState::new` time.

The framework persists `(x, y)` regardless because the saved
coordinate is *portable storage* — useful when the same config
roams to an X11 / macOS / Windows session later. On Wayland itself,
compositors with per-app placement memory (KWin window rules, sway
`for_window` patterns, GNOME's heuristic stickiness) match windows
by their Wayland `app_id` (typically derived from the binary name
by winit) — *that* is where window placement on Wayland actually
gets remembered, not by us. Width / height / `WindowPlacement`
round-trip on every platform regardless.

### 23.6 Atomic writes through one shared I/O thread

Every persistence shape ultimately goes through `DebouncedWriter`,
which:

- Holds a `WriterId` and routes payloads through a single lazy
  `OnceLock<Sender<PoolMsg>>` worker thread shared by every writer
  in the process. An app with the K/V store + window state + a
  recents list opens *one* I/O thread, not three.
- Coalesces rapid bursts inside a debounce window: a new payload
  during the window replaces the prior one and resets the deadline.
- Writes atomically — write to a temp file in the same directory,
  fsync, then `tempfile::persist` (rename) into place. Same-directory
  rename is atomic on every supported filesystem.
- Synchronously flushes pending payloads in `Drop`: a process that
  exits cleanly never loses queued state.

Application code stays single-threaded and reactive. Only the
atomic write hops onto the worker. Signal observers fire on the UI
thread as ever — the path from `signal.set(v)` to the worker's
channel is `tx.send(...)`, cheap and synchronous.

### 23.7 Cycle-free observer wiring in `SettingsStore`

Each registered key in `SettingsStore` owns an observer that writes
the new value back into the in-memory `toml::Value` and schedules a
flush. The closure captures **`Weak<RefCell<StoreInner>>`**, never a
strong `Rc` — a strong capture would trap the entire store inside
its own observer (the closure lives in an `ObserverHandle` stored in
a `SignalCell` stored in `StoreInner.cells`), leaking it for the
life of the process. This is the same `Rc` cycle pattern documented
on `Signal::downgrade` and the same trap any persistence layer that
wires "model writes its own state to disk" can fall into.

The `weak.upgrade()?` early-return also gives correct teardown
semantics: in-flight signal sets after a store drop simply bail.

### 23.8 Threading and out-of-scope

`Signal<T>`, `ListModel<T>`, and `TreeModel<T>` are
`Rc<RefCell<>>`-based; the settings store inherits that. **Single-
threaded UI logic, debounced I/O on one shared worker** is the
explicit threading model. Multi-process is out of scope: two app
instances writing to the same file are last-write-wins (single-instance
apps are the target). Encryption is
out of scope — secrets go through a future `fern-secrets` crate
against the OS keychain. Cloud sync is out of scope. Per-document
state belongs in the document file or its sidecar, not in app
settings — `SettingsFile<T>` is reusable for that, but no built-in
service.

For the full API surface, recipes, and the v1→v2 migration record
on `WindowStateService`, see [`docs/settings.md`](settings.md).

---

## 24. Testability

### 23.1 Headless by Design

The widget tree runs without a window, without GPU, and without winit. All five phases (minus GPU submission) execute in pure Rust with no platform dependencies. Tests use fern-core's `WidgetTree` directly:

```rust
#[test]
fn button_click_fires_action() {
    use std::cell::Cell;
    use std::rc::Rc;
    let mut tree = WidgetTree::new();
    let clicked = Rc::new(Cell::new(false));
    let clicked_flag = clicked.clone();
    let root = tree.add(FillWidget::new());
    tree.push_action(
        root,
        Action::new("app.save").on_invoke(move |_i, _c| clicked_flag.set(true)),
    );
    let button = tree.add_child(
        root,
        Button::new("Save").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save)),
    );
    tree.layout(SizeProposal::exact(200.0, 40.0));
    tree.click(button);
    assert!(clicked.get());
}
```

### 23.2 What Is Testable

Layout (given a widget tree, do children end up at the right positions), event dispatch (does the right widget receive events, does focus cycle correctly), state transitions (hover/pressed/disabled), accessibility (correct AccessKit role, name, actions), render output (expected quads/shapes in the RenderFrame), theming (palette swap produces correct colors), gesture recognition (pure state machine tests), overlay behavior (tooltip timing via simulated clock), drag-and-drop (payload transfer, insertion indicator rendering), and composition (multiple widgets interacting correctly).

### 23.3 Mock Backend

Cargo feature flags (`mock-backend`) swap Qleany controller implementations with mock modules providing static data. Same API surface, zero backend. Familiar to the developer from Qleany's C++/Qt mock system for QtQuick.

### 23.4 CI Friendly

No `Xvfb`, no GPU, no display server required. Pure logic tests run in `cargo test` in milliseconds. The simulated clock (`tree.advance_time()`) enables deterministic testing of time-dependent behavior.

---

## 25. Crate Structure

### 25.1 Crate Map

```
fern-tokens          Pure data types: Theme, Color, TextStyle, SpacingTokens, etc.
                     Dependencies: serde

fern-canvas          Canvas API, RenderFrame, Path, Paint, ShapeQuad, shape cache.
                     Defines the TextBackend trait for pluggable text rendering.
                     Dependencies: fern-tokens, tiny-skia

fern-core            Unified Widget trait (V2), arena, layout, events, focus,
                     shortcuts, overlays, DnD, signal, state (V1 internals),
                     environment, gesture recognizers, headless test harness,
                     build_context, event_handlers, widget_builder, compat (V1<->V2 bridge).
                     The widget_tree module is split into eight implementation files:
                     accessibility_impl, event_dispatch_impl, focus_impl, layout_impl,
                     overlay_impl, query_impl, rendering_impl, test_api.
                     Dependencies: fern-tokens, fern-canvas, accesskit

fern-text            TextBackend implementation backed by text-typeset.
                     Manages the shared Typesetter instance (glyph atlas, shaping,
                     rasterization). Provides layout_single_line fast path.
                     Contains NO widgets.
                     Dependencies: fern-canvas, text-typeset

fern-widgets         All standard widgets: Button, Label, TextWidget, TextInput,
                     TabWidget, ListView, TreeView, Menu, ScrollArea, Panel,
                     Slider, Checkbox, etc.
                     TextWidget renders via Canvas::draw_text (delegates to
                     whatever TextBackend is registered — no fern-text dependency).
                     Optional [rich-text] feature: RichTextEditor widget, which
                     accepts externally-owned TextDocument and Typesetter references.
                     Dependencies: fern-core, fern-tokens, fern-canvas
                     Optional [rich-text]: + text-document, text-typeset

fern-settings        Persistent reactive user preferences. SettingsStore (dotted-key
                     Signal<T> K/V), SettingsFile<T> with versioned migrations,
                     PersistedListModel/PersistedTreeModel bridges, generic
                     MruList<T: MruEntry>, WindowStateService. Atomic write-temp +
                     rename through one shared I/O thread per process.
                     Dependencies: fern-core, fern-data, serde, toml, directories, tempfile

fern-telemetry       Privacy-respecting product analytics built on fern-settings.
                     ConsentStore, InstallId, TelemetryBundle. Phase-1 scaffolding;
                     full design in docs/plans/telemetry-plan.md.
                     Dependencies: fern-core, fern-settings, serde, uuid

fern-i18n            Fluent integration, tr! macro, ShortcutFormatter, locale management
                     Dependencies: fluent-rs, unic-langid

fern-render          wgpu renderer, shader pipelines (quad/rect/SDF), atlas GPU upload
                     Dependencies: fern-canvas, wgpu

fern-platform        winit integration, AccessKit winit adapter, cursor management,
                     native popup windows, IME bridge, platform DnD backend
                     Dependencies: fern-core, fern-render, winit, accesskit-winit

fern-app             Application runner, FernApp builder, event loop glue.
                     Wires the fern-text TextBackend into the Canvas system.
                     Wires fern-settings: opens the SettingsBundle on run(),
                     and (in window_persist.rs) auto-restores + auto-saves
                     window geometry for any WindowConfig that carries id(...).
                     Dependencies: fern-core, fern-canvas, fern-render, fern-platform,
                                   fern-widgets, fern-i18n, fern-settings
                     Optional: fern-text

fern-ui              Umbrella crate with re-exports and default features.
                     Dependencies: all of the above
```

### 25.2 Dependency Graph

```
fern-tokens
    ↑
fern-canvas ← tiny-skia
    ↑           ↑
fern-core    fern-text ← text-typeset
    ↑ ← accesskit
    │
    ├── fern-data
    │       ↑
    │   fern-settings ← serde, toml, directories, tempfile
    │
    ├── fern-widgets
    │   └── [rich-text] ← text-document, text-typeset
    │
    │   fern-i18n ← fluent-rs
    │
fern-render ← wgpu
    ↑
fern-platform ← winit, accesskit-winit
    ↑
fern-app (wires fern-text into Canvas, fern-widgets, fern-i18n,
          fern-settings — auto-restores/saves window geometry,
          optionally fern-text)
    ↑
fern-ui (umbrella, re-exports)
```

Note that fern-text depends only on fern-canvas (for the `TextBackend` trait) and text-typeset. It does not depend on fern-core, text-document, or any platform crate. The `TextBackend` trait is defined in fern-canvas so that the Canvas can call text rendering methods without knowing which backend implementation is active.

The RichTextEditor widget (in fern-widgets behind the `rich-text` feature) depends directly on text-document and text-typeset. The application owns the `TextDocument` instance and passes it to the widget — FernUI never owns or wraps the document model. The application depends on text-document directly for model access (highlighter, cursors, import/export). Cargo deduplicates the shared dependency automatically.

Platform-specific code (winit, wgpu, accesskit-winit) is confined to fern-render and fern-platform. Everything above them is platform-independent and headlessly testable.

### 25.3 The fern-ui Umbrella

The standard application developer depends on a single crate:

```toml
[dependencies]
fern-ui = "0.1"
```

fern-ui re-exports the public API and controls feature flags. `text`, `i18n`, and `rich-text` are default features (opt-out, not opt-in), because the kinds of applications FernUI targets — writing tools, editors, IDEs, content managers, long-running desktop apps — routinely need text rendering, translations, and rich text editing. `TextInput` itself derives from the rich-text widget (see [`fern-ui-milestones.md`](fern-ui-milestones.md) M9), so anything with an editable text field pulls in `rich-text` anyway.

```toml
[features]
default = ["widgets", "text", "i18n", "rich-text"]
widgets = ["dep:fern-widgets"]
text = ["dep:fern-text"]
i18n = ["dep:fern-i18n"]
rich-text = ["fern-widgets/rich-text"]
```

A typical application's dependencies:

```toml
[dependencies]
fern-ui = "0.1"
text-document = "0.1"    # direct dependency for document model access
app-core = { path = "../app-core" }
```

The application depends on text-document directly — not through FernUI — for full access to the document model API (highlighter setup, cursor operations, format queries, import/export, document events). The `RichTextEditor` widget accepts a reference to the externally-owned `TextDocument` and renders it; it does not expose text-document's API.

Sub-crates remain independently publishable for advanced users (custom widget authors, custom renderer implementors).

---

## 26. Button — Reference Widget Design

The button serves as the reference implementation exercising most architectural features: composition of primitives, interaction state as a `Signal<InteractionState>`, role-based color resolution per visual state, attached handler activation from multiple input paths, AccessKit role and actions. A new widget author implementing their first custom widget should read Button's source alongside this doc.

> **V1 (superseded).** The original description below framed Button as a `CompositeWidget` — the two-trait split — with interaction state stored in `RefCell<Option<State<InteractionState>>>` and an `event()` method that matched `PointerEnter` / `PointerLeave` / `PointerDown` / `PointerUp` / `KeyDown`. The V2 Button at [`crates/fern-widgets/src/button.rs`](../crates/fern-widgets/src/button.rs) is about half the size, uses `Signal<InteractionState>` directly, registers attached handlers via `HandlerSet` in `build(&mut self)`, and resolves colors through `Signal<Role>` mapped from the interaction signal (see [`reactive-theme.md`](reactive-theme.md)). Read the file; it's the authoritative exemplar.

### 26.1 What the V1 and V2 versions agree on

- **Composition.** A button is a `RectWidget` (background, border, corner radius) wrapping an internal `HStack` or `VStack` (by `IconPosition`) containing an optional `IconWidget` and a `TextWidget` label. Leading/Trailing positions respect locale `LayoutDirection`.
- **Visual states.** Five: idle, hovered, pressed, focused, disabled. Each resolves to a different color role. Four styles: Filled, Outlined, Flat, Tonal. Style × state → (background role, border role, text role) resolved at paint time.
- **Behavior.** Pointer enter/leave/down/up drives interaction state; keyboard Space/Enter triggers activation; cursor is `Pointer` on hover; `TapRecognizer` commits the click.
- **Accessibility.** `Role::Button`, name from label (resolved via `tr!` / `tr_widget!`), disabled state, actions (`Click`, `Focus`). Focus ring painted only on keyboard focus (origin-aware).

What changed was only the Rust code that lands these behaviors. The widget is the same from the outside.

---

## 27. Architectural Comparisons

### 27.1 vs. QPalette → Design Tokens

QPalette provides a fixed set of color roles across three interaction groups, with no support for spacing, typography, or shape. FernUI's design token system covers the full visual vocabulary, uses typed Rust structs instead of role/group enums, and supports subtree overrides through environment propagation.

### 27.2 vs. QAbstractItemModel → ListModel<T> and TreeModel<T>

Qt's model uses type-erased `QVariant` with role-based data access and `void*` internal pointers. FernUI's `ListModel<T>` and `TreeModel<T>` are concrete generic types with compile-time type safety. The delegate closure receives `&T` directly — no variant casting, no role integers. The `ListDataSource` trait provides an escape hatch for large/external datasets, also with an associated `Item` type.

### 27.3 vs. Existing Rust GUI Frameworks

FernUI is architecturally ahead on accessibility (AccessKit at the trait level, tested by every test), text rendering (text-document + text-typeset), and widget extensibility (two-tier model with slots). It is comparable to Xilem/Masonry on layout and event design. It is weaker on rendering sophistication (quad-based vs. Vello's GPU compute renderer) and has zero maturity compared to established frameworks.

The honest comparison is not against other Rust GUI frameworks but against Qt Widgets — the framework FernUI is designed to replace for the specific use case of a writing application.

---

## 28. Widget Catalog

This section defines every widget in `fern-widgets`, organized by implementation tier. Each entry specifies the widget's purpose, its implementation approach, its accessibility contract, and whether any infrastructure blocks it. The codebase currently provides: animation system (AnimationScheduler with easing), image rendering pipeline (Canvas::draw_image, ImageManager), CPU-rasterized path rendering with atlas caching (PathAtlas with tiny-skia), overlay system with dismissal and cascading, scrolling infrastructure (clips_children, scissor rects, ScrollIntoView), two-level reactive bindings, and gesture recognizers. These capabilities unblock most widgets in the catalog.

### 28.1 Primitives (`fern-widgets/src/primitives/`)

Primitives are Level 2 widgets that serve as building blocks for composition. They implement the `Widget` trait directly, have no reactive internal state, and read theme tokens from context during layout and paint. They are not composites.

**RectWidget** — exists. Axis-aligned rectangle with fill color, border color, border width, and corner radius. Supports reactive bindings for background and border color (`bind_background`, `bind_border_color`). The most basic visual primitive.

**TextWidget** — exists. Single-line text rendered via the TextBackend. Supports reactive color binding (`bind_color`), static or reactive text content (`bind_text`), and text style from theme typography tokens. Accessibility: `Role::Label`.

**HStack** — exists. Horizontal layout container. Cross-axis alignment (`VAlignment`), spacing between children, spacer-aware distribution. Supports per-child alignment overrides.

**VStack** — exists. Vertical layout container. Cross-axis alignment (`HAlignment`), spacing, spacer-aware distribution.

**ZStack** — exists. Overlay layout container. All children positioned at the same origin, stacked in insertion order. Two-axis alignment (`Alignment`).

**Padding** — exists. Single-child wrapper adding uniform or per-edge padding. Adjusts `size_that_fits` and `place_children` to account for padding.

**Spacer** — exists. Flexible space filler. Claims all remaining space on the container's primary axis. Used in stacks to push siblings to edges.

**Center** — exists. Single-child wrapper that centers its child within the available space. Equivalent to a ZStack with `Alignment::center`.

**Expand** — exists. Single-child wrapper that claims all available space on one or both axes. Content alignment within the expanded bounds.

**FixedSize** — exists. Single-child wrapper that reports a fixed size regardless of the parent's proposal. Prevents children from expanding.

**MinSize** — exists. Single-child wrapper that clamps the reported size to a minimum. Used for minimum touch target enforcement.

**MaxSize** — exists. Single-child wrapper that clamps the reported size to a maximum.

**Divider** — new primitive. A 1-pixel line (horizontal or vertical) colored from the theme's `border` token. `size_that_fits` returns 1px on the cross axis and claims the full proposed size on the primary axis. Used as a visual separator in stacks, toolbars, and menus. Accessibility: `Role::Splitter` (non-interactive) or `Role::GenericContainer` with `set_hidden(true)` since it is purely decorative.

**IconWidget** — new primitive. Renders a vector icon from a predefined icon set. Icons are defined as `Path` data (sequences of `PathCommand`) and rendered via the Canvas's `fill_path` method, which is CPU-rasterized by tiny-skia and cached in the PathAtlas. Supports color (from theme or explicit), size (defaulting to 16×16 or 24×24 from theme), and reactive color binding. Accessibility: `Role::Image` with `set_name` describing the icon's meaning. The icon set is a separate data module (`fern-icons` or an icon enum in `fern-widgets`) providing named paths: `Icon::Search`, `Icon::Close`, `Icon::ChevronDown`, `Icon::ChevronRight`, `Icon::Check`, `Icon::Plus`, `Icon::Minus`, etc. This widget is a dependency for many composites (Button with icon, ComboBox chevron, TreeView expand arrow, Checkbox checkmark).

### 28.2 Layout Primitives (`fern-widgets/src/primitives/`)

These are Level 2 widgets that provide layout behavior beyond simple stacking.

**Grid** — new. Two-dimensional layout with row and column tracks. Tracks are defined as fixed, fractional (`1fr`, `2fr`), or auto-sized. `place_children` distributes space across tracks, then places each child in its assigned cell (row, column, optional row/column span). Unlike CSS Grid, the track definitions and child assignments are explicit — no auto-placement algorithm. Accessibility: `Role::Grid` with children as `Role::Row` containing `Role::Cell`, or `Role::GenericContainer` when used purely for layout.

**Wrap / FlowLayout** — new. Children flow horizontally, wrapping to the next line when the available width is exhausted. `place_children` fills rows left-to-right (or right-to-left in RTL), breaking to a new row when the next child would exceed the container's width. Configurable spacing between items and between rows. Used for tag lists, chip collections, and responsive layouts. Accessibility: `Role::GenericContainer`.

**AspectRatio** — new. Single-child wrapper that constrains the child's bounds to a specific width-to-height ratio. `size_that_fits` computes the largest rectangle with the given ratio that fits within the proposal. Used for images, videos, and fixed-proportion containers.

### 28.3 Container Widgets (`fern-widgets/src/`)

These are higher-level widgets built from primitives, providing themed visual framing and structural organization.

**Panel** — exists. Themed container with background, border, corner radius, and padding. Reads defaults from theme tokens, all overridable. Used as the visual foundation for cards, sections, and framed content.

**Card** — new composite. A Panel with elevated styling: shadow (from `ShapeTokens::shadow_md`), surface background, and optional header/footer slots. Header slot typically contains a title and optional action buttons. Footer slot typically contains action buttons or metadata. Accessibility: `Role::Group` with an accessible name from the header content.

**Toolbar** — new composite. An HStack wrapped in a Panel with tight spacing and a themed background (`surface_secondary`). Children are typically Buttons with `ButtonStyle::Flat` and compact sizing, Dividers for visual grouping, and Spacers for alignment. Provides a theme override that reduces `widget_padding` for denser layout. Accessibility: `Role::Toolbar`.

**StatusBar** — new composite. An HStack in a Panel positioned at the bottom of a window. Similar structure to Toolbar but with `caption` typography. Typically contains read-only text labels showing application state (word count, line number, connection status). Accessibility: `Role::Status`.

### 28.4 Interactive Controls (`fern-widgets/src/`)

**Button** ✅ implemented. Non-generic composite using the V2 unified `Widget` trait. Four visual styles (Filled, Outlined, Flat, Tonal), five interaction states (Idle, Hovered, Pressed, Focused, Disabled). Reactive color bindings driven by `Signal<InteractionState>` mapped to `Signal<Role>` (see [`reactive-theme.md`](reactive-theme.md)). Attached `on_tap` handler auto-wires the TapRecognizer. MinSize wrapper for touch target enforcement. Supports optional icon (leading or trailing) via the slot system. Tooltip attachment via builder method. Accessibility: `Role::Button`.

**Checkbox** — new composite. An HStack containing a check box (ZStack of a bordered RectWidget and a checkmark IconWidget) and an optional TextWidget label. Bound to a `Signal<bool>`. Clicking or pressing Space toggles the state. The checkmark icon's visibility is driven by `visible_when` on the `Signal<bool>`. The box's border and background change on hover/press via interaction state bindings, identical to Button's pattern. Accessibility: `Role::CheckBox` with `set_toggled`.

**RadioButton** — new composite. Visually similar to Checkbox but with a circular indicator (filled circle inside an outlined circle) and mutual exclusion. A RadioButton does not manage its own state in isolation — it receives a shared `State<T>` (where T is the value this radio represents) from a RadioGroup or from the application. Clicking a RadioButton sets the shared state to its own value. The filled indicator's visibility is driven by `selected_value.map(|v| *v == self.value)`. Accessibility: `Role::RadioButton` with `set_toggled`, and `set_member_of` referencing the radio group's AccessKit node.

**Toggle / Switch** — new composite. A track (horizontal RectWidget with pill-shaped corners) and a circular knob (RectWidget or circle). Bound to a `Signal<bool>`. The knob's horizontal position is driven by a `Signal<f32>` derived from the boolean state via `signal.map()`, animated via `Signal<f32>::animate_to()` for smooth sliding. Accessibility: `Role::Switch` with `set_toggled`.

**SegmentedControl** — new composite. A horizontal bar of mutually exclusive segments, visually connected with shared corners. Bound to a `Signal<usize>` for the selected index. Internally composes an HStack of segment elements with custom corner radius logic: the first segment has leading corner radius only, the last has trailing corner radius only, middle segments have no corner radius. The selected segment has a distinct background color. Clicking a segment updates the index state. Accessibility: `Role::RadioGroup` containing `Role::RadioButton` children, each with `set_toggled` and `set_position_in_set`.

**Slider** — new Level 2 widget. A horizontal or vertical track with a draggable thumb. Bound to a `Signal<f32>` with configurable min/max range and optional step. The thumb position is computed from the state value and the track length. DragRecognizer handles thumb dragging. Track clicks jump the thumb to the click position (or page-step toward it, depending on configuration). Keyboard: Left/Right (or Up/Down for vertical) adjusts by step, Home/End jump to min/max. Accessibility: `Role::Slider` with `set_numeric_value`, `set_min_numeric_value`, `set_max_numeric_value`, `set_numeric_value_step`, `Action::SetValue`.

**ComboBox / Dropdown** ✅ implemented (Milestone 4). A trigger button (HStack of label TextWidget + ChevronDown IconWidget) that opens an overlay list of selectable items. Non-generic, index-based via `Signal<Option<usize>>`. The trigger's display text is derived from the selected index and the items vector. The dropdown content is pre-created as a dormant subtree during `build()` and activated via `ctx.activate()` when the combo box opens. Overlay placement is `BelowPreferred` (flips to `Above` when no space below), with `DismissBehavior::ClickOutside`. Selecting an item updates the signal, dismisses the overlay, and restores focus to the trigger. Arrow Up/Down navigate the list while open. Type-ahead filtering by first character. Accessibility: trigger is `Role::ComboBox` with `HasPopup::ListBox` and `set_expanded`; list is `Role::ListBox`; items are `Role::ListBoxOption` with `set_selected`.

**ContextMenu** ✅ implemented (Milestone 4). A menu shown at the pointer position on right-click. Built using the overlay system (`OverlayPlacement::AtPointer`, `DismissBehavior::ClickOutside`). The menu content is a MenuList widget provided via a closure called at show-time to reflect current application state. Submenu cascade, keyboard navigation (Arrow Up/Down/Left/Right), and diagonal movement tolerance via 200ms submenu open delay and 150ms close delay. Accessibility: `Role::Menu` with `Role::MenuItem` children.

**MenuBar** ✅ implemented (Milestone 4). A horizontal bar of top-level menu triggers with dropdown menus. Each trigger opens its MenuList as an overlay (`OverlayPlacement::BelowPreferred`). `MenuContext` coordinates open index, trigger focus, and cross-menu Left/Right navigation. Keyboard: Tab focuses the menu bar, Arrow Left/Right navigates between triggers, Enter or Arrow Down opens the active menu, Escape closes. Supports a trailing slot for additional actions (e.g., settings button). On macOS, this is replaced by native `NSMenu` at runtime — Milestone 10 handles the platform abstraction.

**MenuList** ✅ implemented (Milestone 4). A vertical container for `MenuItem` and `MenuSeparator` widgets, providing a themed surface (background, border, corner radius) and keyboard navigation (Arrow Up/Down, Enter, Escape). `KeyboardHighlightWrapper` adds a subtle background behind the currently focused item, driven by a shared `focused_index` signal. Used by MenuBar, ContextMenu, and any widget that needs a vertical menu structure.

**MenuItem** ✅ implemented (Milestone 4). A single clickable item in a MenuList. Non-generic, closure-based activation (`.on_activate_fn(|ctx| ctx.send_intent(AppIntent::X))`). Supports icon, label, shortcut label (auto-looked-up from the `ShortcutRegistry` via `MenuItem::for_shortcut("id")` — the developer never writes the label manually, and labels re-render on rebind through the registry's `version()` signal), disabled state, and submenu triggers. Submenu opens after ~200ms of hover (providing diagonal movement tolerance across other menu items) and closes after ~150ms. Keyboard Enter activates the item and closes the menu stack; Arrow Right opens a submenu; Arrow Left closes the current submenu. Accessibility: `Role::MenuItem` with `set_disabled`, keyboard shortcut in `KeyboardShortcut`, `HasPopup::Menu` for submenu items.

**MenuSeparator** ✅ implemented (Milestone 4). A 1px horizontal line with 4px padding top and bottom. Themed via `theme.colors.border` at 0.3 alpha. Accessibility: `Role::Splitter`.

**Link** — new composite. A focusable, clickable TextWidget with underline decoration and `CursorIcon::Pointer` on hover. Emits a command on click. Visually distinguished from plain text by color (theme `primary`) and underline. Accessibility: `Role::Link` with `set_name`.

### 28.5 Display Widgets (`fern-widgets/src/`)

**ProgressBar** — new composite. A ZStack of a background track RectWidget and a foreground fill RectWidget. The fill width is driven by a `Signal<f32>` (0.0 to 1.0) that computes the fill width as `value * track_width`. Optionally displays a percentage label. Determinate mode (known progress) shows the fill bar. Indeterminate mode (unknown duration) uses an animated sweep driven by the AnimationScheduler. Accessibility: `Role::ProgressIndicator` with `set_numeric_value`, `set_min_numeric_value(0.0)`, `set_max_numeric_value(1.0)`.

**Badge / Chip** — new composite. A small labeled element with a pill-shaped background, optional leading icon, and optional trailing dismiss button. Used for tags, filters, and selected items in multi-select inputs. Accessibility: `Role::Group` or `Role::Button` (if interactive/dismissible).

**Avatar** — new composite. A circular element displaying user initials (TextWidget on a colored circle RectWidget) or an image (when image loading is implemented). The initials color is computed from the user's name hash for consistent per-user coloring. Accessibility: `Role::Image` with `set_name`.

**Accordion / CollapsibleSection** — new composite. A clickable header bar (HStack of label + chevron IconWidget) above a content panel. Bound to a `Signal<bool>` for expanded/collapsed. The content panel uses `visible_when` for instant show/hide, or animated expand/collapse via `Signal<f32>::animate_to()` on a max-height signal. The chevron rotates (via a rotation path or by swapping between ChevronDown and ChevronRight icons). Accessibility: header is `Role::Button` with `set_expanded`; content is `Role::Group`.

### 28.6 Scroll and Split (`fern-widgets/src/`)

**ScrollBar** ✅ implemented (Milestones 3 and 4). Detailed in Section 3.6. Standalone interactive scroll bar with thumb drag, track click, and keyboard adjustment. Shared `Signal<f32>` for scroll position with ScrollArea. Supports an `overlay_mode(true)` flag for the Ubuntu-style thin-to-full expansion: a `resting_thickness` (default 4px) indicator paints at rest, expanding to the full thickness (default 12px) when hovered. Accessibility: `Role::ScrollBar` with `set_numeric_value`, `set_orientation`, `Action::SetValue`.

**ScrollArea** ✅ implemented (Milestones 3 and 4). Detailed in Section 3.8. Viewport-clipping container with two scroll bar modes selected via `ScrollBarStyle`: `Overlay` (default — thin indicator expands to full scroll bar as overlay on hover, viewport width unchanged) or `Permanent` (scroll bar is a layout sibling, reducing viewport by its thickness). The scroll bar widget is always the standalone ScrollBar — ScrollArea does not paint scroll indicators itself. Accessibility: `Role::ScrollView`.

**SplitView** — new Level 2 widget. Two children separated by a draggable divider. The divider's position is a `Signal<f32>` representing the proportion or pixel width of the first child. DragRecognizer on the divider handles resizing. `CursorIcon::ColResize` (horizontal split) or `CursorIcon::RowResize` (vertical split) on hover. Configurable minimum sizes for each pane. Keyboard: when focused, Left/Right (or Up/Down) adjusts the split position by a step. Double-click on divider resets to default position. Accessibility: divider declares `Role::Splitter` with `set_numeric_value`, `Action::SetValue`.

### 28.7 Tabs and Navigation (`fern-widgets/src/`)

**Switcher** — new primitive (in `fern-widgets/src/primitives/`). A container that shows exactly one child at a time, driven by an external `Signal<usize>` index. Internally a ZStack where each child has a `visible_when` binding derived from `selected_index.map(|i| *i == this_child_index)`. The selected child is active (layout, paint, events, accessibility); all others are dormant (state preserved, no rendering cost). The Switcher does not own the selection logic — it receives the `Signal<usize>` from outside, so it composes with any navigation pattern (wizard Next/Back buttons, sidebar navigation, routing logic, tab headers) without encoding assumptions about how the index changes. Used for wizard flows, view mode switching (list/grid/detail), authentication gates (login → main), and navigation-driven content areas. If animated transitions are desired (crossfade, slide), the outgoing and incoming children's opacity or position are driven by the AnimationScheduler before the dormancy toggle completes. Accessibility: `Role::GenericContainer` — the semantic meaning comes from the external control, not the Switcher itself. Only the active child produces AccessKit nodes.

**TabWidget** — new composite. An HStack of tab headers above a Switcher. Bound to a `Signal<usize>` for the selected tab index. The tab headers drive the index state; the Switcher consumes it. Each tab header is a clickable element whose background is driven by `selected_index.map(|i| *i == this_tab_index)`. The TabWidget does not implement switching logic — it delegates to Switcher, which handles dormancy toggling through the binding system. Trailing slot for tab-level actions (add tab button, overflow menu). Keyboard: Arrow Left/Right moves between tab headers; the content pane is focusable independently. Accessibility: `Role::TabList` containing `Role::Tab` headers; content panes are `Role::TabPanel` with `labelled_by` referencing the corresponding tab header.

**Breadcrumb** — new composite. An HStack of clickable path segments separated by chevron or slash icons. Each segment emits a navigation command. The last segment is non-interactive (current location). Accessibility: `Role::Navigation` containing `Role::Link` items, with the last item marked `aria-current`.

### 28.8 Overlays and Dialogs (`fern-widgets/src/`)

**Tooltip** — exists. Non-interactive text or rich content shown after a hover delay. Managed by the TooltipHost wrapper and the WidgetTree's tooltip timer system.

**Popover** — new composite. An interactive overlay anchored to a trigger widget, containing arbitrary content (a form, settings panel, mini-editor). Uses the overlay system with `DismissBehavior::ClickOutside`. Distinguished from tooltip (non-interactive, text-only, short delay) and from menu (structured list of actions). The popover's content receives focus when shown. Accessibility: trigger declares `HasPopup::Dialog`; popover content is `Role::Dialog`.

**Dialog** — new composite. A modal Panel shown via `OverlayLayer::NativePopup` (or as an in-window overlay with a scrim backdrop). Contains a title (heading), content area (arbitrary widgets), and an action bar (HStack of buttons). Modal behavior: focus is trapped within the dialog (Tab cycles only within the dialog's widgets), a scrim covers the parent window, and Escape dismisses. Accessibility: `Role::AlertDialog` or `Role::Dialog` with `set_modal`, `labelled_by` pointing to the title.

**Snackbar / Toast** — new. An auto-dismissing notification shown as an overlay near the bottom of the window. Managed by a `SnackbarManager` (similar to OverlayManager) that queues notifications and displays them sequentially. Each notification has a message, an optional action button, and a configurable display duration. The SnackbarManager handles show/hide animation (slide in from bottom, fade out) via the AnimationScheduler. Accessibility: `Role::Alert` with `set_live("polite")` so screen readers announce the notification without interrupting the current task.

### 28.9 Data-Driven Widgets (`fern-widgets/src/`)

**Repeater** — new Level 2 widget. Creates one child subtree per item in a `ListModel<T>`. When the `ListModel` emits `DataChange` notifications (insert, remove, move), the Repeater performs targeted arena mutations. Designed for small, non-virtualized dynamic collections (toolbar buttons, chapter list, tag chips). Detailed in Section 6.4.

**ListView** — new Level 2 widget. Virtualized scrollable list backed by `ListModel<T>` (common case) or `ListDataSource` trait (large/external datasets). Only instantiates widget subtrees for visible items plus a buffer. Manages item lifecycle based on scroll position. The delegate closure creates the widget subtree for each visible item. Inherits scrolling behavior from ScrollArea's mechanisms (offset placement, clips_children, scroll events) but manages item creation/destruction internally. Accessibility: `Role::List` or `Role::ListBox` with `set_size_of_set`; visible items declare `set_position_in_set`.

**TreeView** — new Level 2 widget. Hierarchical list with expand/collapse. Backed by `TreeModel<T>`, with a `TreeSlice<T>` created internally for per-view expand/collapse state and flat visible-node projection. The visible item set is computed from the expanded nodes, then virtualized like ListView. Indent level is computed from tree depth. Expand/collapse toggle via click on the arrow icon or Left/Right arrow keys. Multiple TreeViews can share the same `TreeModel` with independent expand states. Accessibility: `Role::Tree` with `Role::TreeItem` children; items declare `set_expanded`, `set_level`, `set_position_in_set`, `set_size_of_set`.

**SelectionModel** — new utility (not a widget). A `Signal<SelectionSet>` that tracks which items are selected, with methods for single-select (click), toggle (Ctrl+click), range-select (Shift+click), and select-all (Ctrl+A). Consumed by ListView and TreeView. The SelectionSet stores selected indices as a `BTreeSet<usize>`. The SelectionModel emits selection change notifications through the Signal binding system.

### 28.10 Text Editing (`fern-widgets/src/`, feature-gated)

> **Note on `AppCommand` in this section.** The API surface below
> (event callbacks typed as `Fn(...) -> Box<dyn AppCommand>`) is from
> the pre-implementation draft. As of Milestone 8 the rich text editor
> ships in [`crates/fern-widgets/src/rich_text.rs`](../crates/fern-widgets/src/rich_text.rs)
> and the equivalent hooks take closures that call
> `ctx.send_intent(AppIntent::X)`. The *information architecture* the
> section describes — separate presets for editable vs. read-only,
> policy bundles covering command filter / caret / accessibility
> role / clipboard, shared document across widgets, frame-loop bridge
> from document events to Signals — carries over verbatim, just with
> `Intent` in place of `Box<dyn AppCommand>`.

This section is longer than other widget catalog entries because the rich text editor is the most architecturally distinctive widget in FernUI. It cannot use ScrollArea, it cannot delegate text layout to TextWidget, and its frame loop has to bridge text-document's deferred event model into FernUI's reactive Signal model. The design below is informed by `godot-rich-text`, a working reference implementation of the same `text-document` + `text-typeset` integration in Godot 4 (~2,100 lines of editor logic, plus a 780-line read-only viewer).

#### 27.10.1 One Widget, Two Construction Presets

FernUI ships a single rich text widget, `RichTextEditor`, feature-gated behind the `[rich-text]` cargo feature. The widget has two public constructors that bundle different **policy presets** — a command filter, a caret policy, an accessibility role, and a clipboard policy — at construction time. There is no separate `RichTextView` type and no runtime-mutable `read_only` flag.

**`RichTextEditor::editor(document)`** produces an editable widget. All editing commands accepted. Caret blinks. Accessibility role is `Role::MultilineTextInput`. Full clipboard support (cut, copy, paste). IME composition hooks active (even when IME itself is deferred to a post-M9 refinement; see §28.10.14). Undo stack active. This is the foundation for the writer-IDE use case (Atelier, novelist tools), code editors, note-taking applications, and any case where the user authors rich content.

**`RichTextEditor::read_only(document)`** produces a non-editing display widget. The command filter rejects every mutating command. The caret does not blink — it is either static (visible on focus for screen-reader navigation) or hidden entirely, depending on whether the application wants keyboard navigation. Accessibility role is `Role::Document`. Clipboard is limited to copy and select-all. No undo stack (nothing to undo). Link click activation still works (and is in fact the main interaction in a read-only view). This is the right starting point for documentation viewers, help panels, message displays, log readers, and any case where text content needs rich rendering without modification.

Both constructors produce the same Rust type, share the same arena node structure, use the same paint pipeline, the same hit-testing logic, the same scroll bar pair, the same frame loop bridge, and the same ~60% of shared code that would otherwise justify a separate `RichTextView` widget. The difference is entirely in the policy bundle selected at construction.

**Why presets instead of a boolean flag.** A `read_only: bool` field suggests runtime togglability, which is a trap. The real policies that differ between "read-only" and "editable" are not one bit of state but four independent decisions:

- **Command filter** — which typed commands are accepted and dispatched to the cursor.
- **Caret policy** — blinking, static visible, or hidden, plus whether the caret participates in keyboard navigation.
- **Accessibility role** — `Role::Document` vs. `Role::MultilineTextInput`. Critically important because screen readers enter forms-navigation mode on focus for `MultilineTextInput` roles, announcing "editing" and expecting typed input. Reporting `MultilineTextInput` for a read-only widget is a real accessibility bug.
- **Clipboard policy** — which clipboard operations (cut, copy, paste, select-all) are allowed.

A single boolean would have to be consulted in at least four places (command dispatch, paint, accessibility, clipboard setup), with nothing preventing a future contributor from adding a fifth branch and forgetting one of them. The named-constructor approach sets every policy once, at construction, and the widget implementation never sees a boolean — it sees a `CommandFilter`, a `CaretPolicy`, an `AccessibilityRole`, and a `ClipboardPolicy`, each consulted where it belongs.

**Why not runtime-toggleable.** Toggling between read-only and editable at runtime creates a set of nasty transitional questions with no clean answers: what happens to the blinking caret animation when the flag flips mid-blink, what happens to an in-progress IME composition, what happens to the undo stack, what happens during a drag-selection, what does the accessibility role flip do to a focused screen reader. All of these have to be handled for a runtime toggle to be correct, and none of them have obvious right answers. The named-constructor design sidesteps all of this: once a `RichTextEditor::read_only(doc)` is built, it stays read-only for its entire lifetime. An application that needs the semantic of toggling editability — a document that becomes editable after a permission check, for example — destroys the widget and rebuilds it through composite rebuild, with the document reference surviving intact because it is externally owned.

**Future presets.** The preset machinery naturally accommodates additional construction modes without breaking the existing two. A hypothetical `RichTextEditor::comments_only(doc, comment_regions)` could accept edits only inside marked regions by configuring the command filter to reject commands whose cursor position falls outside the regions. A `RichTextEditor::restricted(doc, filter)` could accept an application-supplied command filter directly for specialized cases. None of these would require touching the existing `editor()` / `read_only()` constructors — each preset is an independent bundle of policies over the same shared widget core.

```rust
// Editable — full rich text editor.
let editor = ctx.add(
    RichTextEditor::editor(document.clone(), shared_typesetter.clone())
        .wrap_mode(WrapMode::Word)
        .zoom(1.0)
        .on_text_changed(|ctx| ctx.send_intent(AppIntent::DocumentMarkDirty))
        .on_link_clicked(|href, ctx| ctx.send_intent(AppIntent::OpenUrl(href.into())))
        .on_undo_redo_changed(|can_undo, can_redo, ctx| {
            ctx.send_intent(AppIntent::UpdateUndoButtons { can_undo, can_redo })
        })
);

// Read-only — same widget type, different construction preset.
let viewer = ctx.add(
    RichTextEditor::read_only(documentation_doc.clone(), shared_typesetter.clone())
        .wrap_mode(WrapMode::Word)
        .zoom(1.0)
        .on_link_clicked(|href, ctx| ctx.send_intent(AppIntent::OpenDocsUrl(href.into())))
);
```

The document can be externally owned and passed in via either constructor, allowing multiple widgets to share a single document or the application to retain access for save/load operations. A common pattern is one `RichTextEditor::editor(doc)` for authoring and a second `RichTextEditor::read_only(doc)` bound to the same document as a preview pane — edits in the author widget appear in the preview via the same reactive document-version Signal that drives the editor's own rendering.

#### 27.10.2 The Triple Ownership Model

The widget owns three things that work together: a `TextDocument` (the data, with its event log and undo stack), a `Typesetter` (the layout engine that takes a document snapshot and produces glyph positions, line boxes, and decoration rects), and a `TextCursor` (the editing handle that mutates the document and tracks the caret position).

```rust
pub struct RichTextEditor {
    // Triple ownership
    document: TextDocument,
    typesetter: Typesetter,
    cursor: TextCursor,

    // Reactive bridge
    document_version: Signal<u64>,        // increments on every doc event
    can_undo: Signal<bool>,
    can_redo: Signal<bool>,
    has_selection: Signal<bool>,
    caret_visible: Signal<bool>,          // animated for blink

    // Scroll state — NOT inside a ScrollArea
    scroll_y: Signal<f32>,
    scroll_x: Signal<f32>,
    max_scroll_y: Signal<f32>,
    max_scroll_x: Signal<f32>,
    viewport_ratio_y: Signal<f32>,
    viewport_ratio_x: Signal<f32>,

    // Layout strategy state
    needs_full_layout: bool,
    last_relayout_block_id: Option<usize>,
    content_dirty: bool,

    // Input batching
    pending_chars: String,                // collected key inputs flushed per frame
    preferred_x: Option<f32>,             // sticky column for vertical movement

    // Click counting
    click_count: u32,
    last_click_time: Instant,
    last_click_pos: Point,

    // Debounce
    debounce_timer: f32,
    pending_text_changed: bool,
    pending_format_changed: bool,
    pending_undo_redo: Option<(bool, bool)>,

    // Application command factories
    on_text_changed: Option<CommandFactory>,
    on_link_clicked: Option<Box<dyn Fn(&str) -> Box<dyn AppCommand>>>,
    on_image_clicked: Option<Box<dyn Fn(&str) -> Box<dyn AppCommand>>>,
    on_undo_redo_changed: Option<Box<dyn Fn(bool, bool) -> Box<dyn AppCommand>>>,

    // Internal child IDs
    text_area_id: Option<WidgetId>,
    v_scrollbar_id: Option<WidgetId>,
    h_scrollbar_id: Option<WidgetId>,
}
```

The `TextDocument` is owned, not wrapped in `Rc<RefCell<>>`, because text-document already provides interior mutability through its own internal locking. Mutations go through the cursor (`cursor.insert_text("hello")`); the document records the mutation in its event log and the editor's per-frame effect drains the events.

If the application needs shared access to the document (a save handler, an outline panel, a word counter), it constructs the document externally and passes it to the editor's constructor. The editor then takes a clone of the document handle rather than constructing a new one. text-document's internal Rc-based design makes this cheap.

#### 27.10.3 The Frame Loop

The editor cannot rely on Signal observers alone for its update logic. text-document's mutations produce events asynchronously (the cursor mutates, the document queues an event, the event must be drained later). FernUI's reactive system propagates Signal changes immediately. Bridging these two models requires a per-frame effect that polls the document's event queue, decides what relayout and repaint work is needed, and updates the relevant Signals.

The editor registers a `ctx.effect(&frame_tick, |_| ...)` on the animation scheduler's frame tick signal. The closure runs once per frame and executes the equivalent of the following pseudo-code:

```
1. Flush pending_chars: if any characters were buffered from key events,
   call cursor.insert_text(&pending_chars) as a single insertion.

2. Drain document events: events = document.poll_events()
   - ContentsChanged { position, blocks_affected }: if blocks_affected <= 1,
     remember the position for incremental relayout. Otherwise mark needs_full_layout.
   - FormatChanged: needs_full_layout = true.
   - DocumentReset, FlowElementsInserted/Removed, BlockCountChanged: needs_full_layout = true.
   - UndoRedoChanged { can_undo, can_redo }: stash for debounced emission.
   - LongOperationFinished: emit a one-shot DocumentLoaded notification.

3. Pre-adjust viewport for word-wrap mode: if wrap_mode == Word and the
   vertical scroll bar is visible, set typesetter.viewport_width =
   widget_width - scrollbar_width and mark needs_full_layout. This breaks
   the circular dependency between viewport width, content height, and
   scrollbar visibility.

4. Apply layout strategy:
   - If needs_full_layout: typesetter.layout_full(&document.snapshot_flow())
   - Else if a single block changed: typesetter.relayout_block(...) and
     remember last_relayout_block_id for the incremental render path.

5. Update cursor display: typesetter.set_cursor(CursorDisplay {
     position: cursor.position(),
     anchor: cursor.anchor(),
     visible: caret_visible.get(),
     ..
   })

6. Ensure caret visible: if the caret moved off-screen, adjust scroll_y or
   scroll_x to bring it back into view with a margin.

7. Update scroll signals: set max_scroll_y, max_scroll_x, viewport_ratio_y,
   viewport_ratio_x from typesetter.content_height(), max_content_width(),
   and the current widget size.

8. Debounced signal emission: if 150ms have passed since the last edit,
   emit pending_text_changed, pending_format_changed, and pending_undo_redo
   as typed application commands.

9. Mark content_dirty for the paint phase if anything changed.
```

This is the only place where the editor synchronizes with text-document's event model. Everything else (signal observers, gesture handlers, accessibility queries) reacts to the Signals that this loop updates.

#### 27.10.4 Three-Tier Render Strategy

The editor has three distinct render paths, used for different change kinds. The choice between them is based on `content_dirty` and `last_relayout_block_id`:

- **Full render** (`typesetter.render()`): used for structural changes (block insertion, paragraph format change, document reset). Rebuilds the entire `RenderFrame` from scratch. The most expensive path.
- **Incremental block render** (`typesetter.render_block_only(block_id)`): used after `relayout_block` for a single-block edit (typing inside a paragraph). The typesetter merges the new block's glyphs and decorations into the existing render frame, leaving the rest untouched. Ten to fifty times faster than a full render for documents with hundreds of blocks.
- **Cursor-only render** (`typesetter.render_cursor_only()`): used for caret blink. Updates only the cursor decoration in the render frame; glyphs and other decorations are unchanged. This is what allows the caret to blink at 60fps without triggering a full re-render twice per second.

The editor's `paint()` method walks the resulting `RenderFrame` in four passes:

1. **Background decorations**: Selection, CellSelection, Background, BlockBackground, TableCellBackground, TableBorder. Drawn first so text renders on top.
2. **Glyph quads**: each glyph has a screen rect, an atlas rect, and a color. Drawn via `canvas.draw_glyph_quad(screen, atlas, color)`. The atlas is the typesetter's shared glyph atlas, integrated with fern-render's atlas pipeline.
3. **Inline images**: PNG/JPEG/WebP images embedded in the document. Each has a screen rect and a resource name; the editor's image cache resolves the name to a texture and draws it.
4. **Foreground decorations**: Cursor, Underline, Overline, Strikeout. Drawn last so they render on top of glyphs.

The Canvas operations are standard FernUI primitives — `canvas.fill_rect()` for background and cursor, `canvas.draw_line()` for underline/overline/strikeout, `canvas.draw_glyph_quad()` for glyphs, `canvas.draw_image()` for inline images. There is no special canvas API for rich text; the editor walks the frame and emits ordinary calls.

#### 27.10.5 Why the Editor Cannot Use ScrollArea

This is the most important architectural distinction in the rich text editor design, and the reason its catalog entry is much longer than every other widget's.

A `ScrollArea` wraps a child widget. The child has an intrinsic size (computed by `size_that_fits` with an unbounded proposal); the ScrollArea derives `max_scroll = max(0, child_size - viewport_size)` and clips the child's painting to the viewport. The scroll bar's visibility is determined after the child's intrinsic size is known.

This works for any widget whose layout does not depend on the viewport. It does not work for the rich text editor, because the editor's layout has a circular dependency:

- The viewport width depends on whether the vertical scroll bar is visible.
- The vertical scroll bar's visibility depends on whether the content height exceeds the viewport height.
- The content height depends on text layout.
- Text layout (in word-wrap mode) depends on the viewport width.

A naive ScrollArea wrapper would either over-allocate width (assume the scroll bar is always visible, leaving an empty strip when content fits) or oscillate (lay out without the scroll bar, discover it's needed, lay out again with it, discover content now fits without it, lay out again...). Neither is acceptable.

The Godot reference resolves this by pre-adjusting the viewport before the layout call: at the start of each frame loop, it checks whether the vertical scroll bar is currently visible, sets `typesetter.viewport_width = widget_width - (vsb_visible ? sb_width : 0)`, and then runs `layout_full`. The decision is one-frame-stale (the scroll bar visibility is based on the previous frame's content height) but converges in two frames for any content change and is invisible to the user. ScrollArea cannot express this pre-adjustment because it does not know about the typesetter or about the widget's frame loop.

The editor therefore manages its own scroll directly. It owns six `Signal<f32>` fields (`scroll_y`, `scroll_x`, `max_scroll_y`, `max_scroll_x`, `viewport_ratio_y`, `viewport_ratio_x`) and constructs two `ScrollBar` widgets as siblings of the text content area in `build()`. The ScrollBars bind to these signals just as they would inside a ScrollArea. The editor's `place_children()` positions the scroll bars at the right edge (vertical) and bottom edge (horizontal), trimming each by the other's thickness when both are visible. The editor's `paint()` translates by `(-scroll_x.get() * zoom, -scroll_y.get() * zoom)` before walking the render frame, which is the equivalent of ScrollArea's offset placement.

```rust
// Inside RichTextEditor::build():
let v_sb = ScrollBar::vertical(
    self.scroll_y.clone(),
    self.max_scroll_y.clone(),
    self.viewport_ratio_y.clone(),
).overlay_mode(false);  // permanent layout sibling, not overlay

let h_sb = ScrollBar::horizontal(
    self.scroll_x.clone(),
    self.max_scroll_x.clone(),
    self.viewport_ratio_x.clone(),
).overlay_mode(false);

self.v_scrollbar_id = Some(ctx.add(v_sb));
self.h_scrollbar_id = Some(ctx.add(h_sb));
self.text_area_id = Some(ctx.self_id());  // the editor itself is the text area
```

The editor's `place_children()` lays out the two scroll bars manually:

```rust
fn place_children(&self, bounds: Rect, ...) -> Vec<WidgetPlacement> {
    let v_visible = self.max_scroll_y.get() > 0.0;
    let h_visible = self.max_scroll_x.get() > 0.0;
    let sb = SCROLLBAR_THICKNESS;

    let mut placements = Vec::new();
    if v_visible {
        placements.push(WidgetPlacement {
            id: self.v_scrollbar_id.unwrap(),
            bounds: Rect {
                x: bounds.x + bounds.width - sb,
                y: bounds.y,
                width: sb,
                height: bounds.height - if h_visible { sb } else { 0.0 },
            },
        });
    }
    if h_visible {
        placements.push(WidgetPlacement {
            id: self.h_scrollbar_id.unwrap(),
            bounds: Rect {
                x: bounds.x,
                y: bounds.y + bounds.height - sb,
                width: bounds.width - if v_visible { sb } else { 0.0 },
                height: sb,
            },
        });
    }
    placements
}
```

The editor uses `ScrollBarStyle::Permanent` semantics (the scroll bars are layout siblings, not overlays), but it does not actually use the `ScrollBarStyle` enum because there is no ScrollArea wrapper to configure. The two ScrollBar widgets are constructed in their permanent-mode form directly.

#### 27.10.6 Hit Testing and HitRegion

`Typesetter::hit_test(x, y)` returns an `Option<HitResult>` where `HitResult` contains a text position and a `HitRegion` enum:

```rust
pub enum HitRegion {
    Text,
    Link { href: String },
    Image { name: String },
    TableCell { table_id: usize, row: usize, col: usize },
}
```

The editor's pointer handler checks the region first and dispatches differently for each:

- `HitRegion::Link { href }`: emit `link_clicked` with the URL. Do not place the cursor (the click was on a link, not on text).
- `HitRegion::Image { name }`: emit `image_clicked` with the image name. Do not place the cursor.
- `HitRegion::Text` or `HitRegion::TableCell`: place the cursor at the hit position via `cursor.set_position(hit.position, MoveMode::MoveAnchor)`.

The hit test runs in document coordinates, so the editor adjusts the input pointer position by the current scroll offset (and zoom factor) before passing it to `hit_test`:

```rust
let hit_x = pointer.x + self.scroll_x.get() * zoom;
let hit_y = pointer.y + self.scroll_y.get() * zoom;
let hit = typesetter.hit_test(hit_x, hit_y);
```

#### 27.10.7 Click Counting and Selection Modes

Single click positions the cursor. Double click selects the word under the cursor. Triple click selects the paragraph. Shift+click extends the selection to the click position.

The editor implements this by tracking `click_count`, `last_click_time`, and `last_click_pos`. A click within 400ms and 5px of the previous click increments the counter; otherwise it resets to 1. The handler then dispatches based on the counter:

```rust
match self.click_count {
    1 => self.place_cursor(position),
    2 => {
        self.place_cursor(position);
        cursor.select(SelectionType::WordUnderCursor);
    }
    _ => {  // 3 or more
        self.place_cursor(position);
        cursor.select(SelectionType::BlockUnderCursor);
        self.click_count = 3;  // cap at 3
    }
}
```

FernUI's gesture system has `TapRecognizer` and `DoubleTapRecognizer`. A `TripleTapRecognizer` is a small addition; alternatively, the editor implements click counting itself in its pointer handler (the Godot version does this and it works fine). For Milestone 8, implementing it inline is simpler than adding a third gesture recognizer.

Drag-select is handled separately. When the pointer moves while the button is held, the editor calls `cursor.set_position(hit.position, MoveMode::KeepAnchor)` to extend the selection. If the pointer is within 20px of the top or bottom of the viewport, the editor auto-scrolls toward the pointer at a speed proportional to the distance into the margin.

#### 27.10.8 Caret Blink as a Separate Render Path

The caret blinks at a configurable interval (default 530ms — the Windows default). The blink is implemented as a `Signal<bool>` (`caret_visible`) updated by the animation scheduler:

```rust
// In build():
self.caret_visible = ctx.signal(true);
ctx.effect(&frame_tick, {
    let visible = self.caret_visible.clone();
    let mut accumulated = 0.0_f32;
    move |delta| {
        accumulated += delta;
        if accumulated >= 0.530 {
            accumulated = 0.0;
            visible.set(!visible.get());
        }
    }
});
```

The frame loop sets the typesetter's cursor display to match `caret_visible.get()`, then triggers a *cursor-only* render (not a full render). The render frame's glyphs and other decorations are unchanged; only the cursor decoration is replaced. The widget's paint pass walks the same number of glyph quads but the paint is fast because no text layout has changed.

Without this distinction, every caret blink would mark the editor's content_dirty flag and trigger a full re-render of the entire visible page, which would dominate frame time for documents with hundreds of glyphs.

#### 27.10.9 Debounced Signal Emission

Signals like `text_changed`, `format_changed`, and `undo_redo_changed` should not fire on every keystroke. A user typing 100 characters per second would otherwise trigger 100 application command emissions per second, hammering observers (the modified-indicator UI, the document outline panel, the autosave timer).

The editor batches these into pending flags (`pending_text_changed`, `pending_format_changed`, `pending_undo_redo`) that are set during the frame loop. A `debounce_timer` accumulates `delta` each frame. When the timer exceeds 150ms (chosen to feel responsive while still batching bursts of typing), the editor flushes the pending flags by emitting the corresponding typed commands and resets the timer.

This is purely an emission optimization. The document and typesetter are always up to date; only the application-facing notification is debounced. Observers that need real-time access to the document state (a live word counter, for example) read the document directly via the `document_version: Signal<u64>` which is incremented immediately on each event drain.

#### 27.10.10 The Signal<u64> Document Version

The bridge between text-document's imperative-then-events model and FernUI's reactive Signal model is a `Signal<u64>` ("document version") that the editor's frame loop increments whenever it processes a non-empty event batch. This signal is exposed publicly via `editor.document_version()`.

Any widget that wants to react to document changes observes this signal:

```rust
// A word counter widget:
let counter = ctx.signal(0_usize);
ctx.effect(&editor.document_version(), {
    let document = document.clone();
    let counter = counter.clone();
    move |_| {
        let count = document.to_plain_text()
            .map(|s| s.split_whitespace().count())
            .unwrap_or(0);
        counter.set(count);
    }
});

TextWidget::new_bound(counter.map(|n| format!("{} words", n)))
```

The version signal does not carry information about *what* changed — only that something did. Observers that need granular change information must track their own state and compare against the document. For most use cases (modified indicators, word counts, outline panels), this coarse-grained notification is sufficient and avoids the complexity of typed change events.

#### 27.10.11 Application Commands for Editor Events

The editor exposes typed-command builder methods for application-relevant events:

- `on_text_changed(cmd)`: emitted (debounced) when the document content changes.
- `on_format_changed(cmd)`: emitted (debounced) when character or block formatting changes.
- `on_link_clicked(|href| -> AppCmd)`: takes a constructor function that builds a typed command from the clicked URL. The closure exists because the URL is dynamic data; the command type is application-defined.
- `on_image_clicked(|name| -> AppCmd)`: same pattern for image clicks.
- `on_undo_redo_changed(|can_undo, can_redo| -> AppCmd)`: emitted when the can_undo or can_redo state changes.
- `on_document_loaded(cmd)`: emitted once when async loading (`set_html`, `set_markdown`) completes.
- `on_selection_changed(cmd)`: emitted when the selection range changes.
- `on_caret_changed(cmd)`: emitted when the caret moves without selecting.

The constructor-function pattern (`|href| AppCmd::OpenUrl(href.into())`) is used for events that carry runtime data (URLs, image names, undo/redo booleans). This is consistent with Section 9.2's typed-command discipline: the closure constructs a typed command from the runtime data, rather than the closure being the action handler itself. The result is a `Box<dyn Fn(...) -> Box<dyn AppCommand>>` that the editor invokes when the event fires.

For applications that need direct closure handling (a plugin system that registers custom link handlers), the `on_*_fn` variants from Section 9.2.6 are also available: `on_link_clicked_fn(|href, ctx| { ... })` accepts a closure that runs directly with `EventContext` access. This is the documented escape hatch for cases where a typed command is not appropriate.

#### 27.10.12 Editing Operations and the Cursor API

All editing happens through the `TextCursor`. The editor's keyboard handler dispatches each key event to the appropriate cursor method:

- Character input: `cursor.insert_text(&str)` (batched via `pending_chars`).
- Backspace: `cursor.delete_previous_char()`.
- Delete: `cursor.delete_char()`.
- Ctrl+Backspace / Ctrl+Delete: `cursor.move_position(WordLeft/WordRight, KeepAnchor, 1)` then `cursor.remove_selected_text()`.
- Enter: `cursor.insert_block()` (or table cell navigation when inside a table).
- Tab: `cursor.insert_text("\t")` (or list indent when at the start of a list item).
- Arrow keys: `cursor.move_position(direction, MoveAnchor, 1)`.
- Shift+Arrow: `cursor.move_position(direction, KeepAnchor, 1)`.
- Home/End: `cursor.move_position(StartOfBlock/EndOfBlock, MoveAnchor, 1)`.
- Ctrl+Home/Ctrl+End: `cursor.move_position(Start/End, MoveAnchor, 1)`.
- Page Up/Down: compute the target Y (current Y minus/plus viewport height), hit-test for the position, set the cursor.
- Ctrl+Z: `document.undo()`.
- Ctrl+Y / Ctrl+Shift+Z: `document.redo()`.
- Ctrl+B: toggle bold via `cursor.set_char_format(CharFormat { font_bold: Some(!current), .. })`.
- Ctrl+I, Ctrl+U: same pattern for italic and underline.
- Ctrl+A: select all (with escalation in tables, see below).

**Sticky preferred X for vertical movement.** When the user moves the cursor up or down across lines, the X coordinate is remembered in `preferred_x`. Successive Up/Down presses use this X to find the target column on each line, even when crossing short lines. Any non-vertical action clears `preferred_x`.

**Ctrl+A escalation in tables.** When the cursor is inside a table cell, the first Ctrl+A selects the cell content. The second Ctrl+A selects the entire cell (including its formatting). The third selects the entire table. The fourth selects the entire document. A fifth resets to the cell content. This is a convenience for table editing and is implemented entirely in the editor's command handling (no framework support needed). Outside of tables, Ctrl+A always selects the entire document.

#### 27.10.13 Clipboard Integration

The editor supports plain-text, HTML, and in-process rich (format-preserving) clipboard operations. `fern-platform`'s `ClipboardBackend` trait exposes four named payload methods — `get_text` / `set_text` / `get_html` / `set_html` — plus `has_text` / `has_html` probes. The `ClipboardHandle` threads these through to widgets without binding them to `arboard`; headless tests swap in a `MemoryClipboard` that supports both payloads.

**Copy / cut.** Cut and copy place the selected content on the system clipboard via `set_html(html, plain_fallback)`, which on every real backend writes both payloads in one transaction — Linux `text/html` + `UTF8_STRING`, macOS `NSPasteboardTypeHTML` + `NSPasteboardTypeString`, Windows `CF_HTML` + `CF_UNICODETEXT`. The HTML serialisation comes from `DocumentFragment::to_html`. The editor additionally stores the `DocumentFragment` (typed rich representation) in its internal `rich_clipboard_fragment` field along with the plain-text version for self-round-trip detection.

**Paste** reads the clipboard in a three-step preference order:

1. **Self-round-trip fragment.** If the plain text matches what this editor last copied, reinsert the stashed `DocumentFragment` via `cursor.insert_fragment(&fragment)`. Guarantees bit-exact intra-editor round-trip (preserves exotic format flags that HTML serialisation might drop).
2. **External HTML payload.** If the clipboard carries HTML (checked via `has_html`), parse it via `TextCursor::insert_html(&html)` — text-document's importer turns the HTML into a `DocumentFragment` and inserts at the caret. This path is what makes rich paste *from another app* work: Firefox, Word, Google Docs, Apple Notes, anything Chromium, anything Gecko.
3. **Plain-text fallback.** When neither rich path applies, insert the clipboard's plain text via `cursor.insert_text(&text)`.

**Paste Unformatted** (Ctrl+Shift+V / ⌘⇧V) bypasses both rich branches and inserts only plain text — the user explicitly asked to strip formatting, so even when the clipboard has an HTML payload we use `get_text()` verbatim.

`NSAttributedString` is not a separate payload — it is a Cocoa *type* that serialises to RTF on the pasteboard; we rely on HTML, which every modern macOS app also writes.

**RTF** (Rich Text Format) remains a post-Milestone-8 refinement. It is the last-mile payload for applications that don't emit HTML on copy — Pages, TextEdit, some legacy Windows apps. Adding RTF is a pure-additive change: `ClipboardBackend` gains `get_rtf` / `set_rtf` with the same default-body fallback convention, text-document gains an RTF importer, and the paste path grows a branch between HTML and plain text.

#### 27.10.16 Default Context Menu

`RichTextEditor` installs a default right-click menu via the framework's built-in `HandlerSet::context_menu(factory)` plumbing. The framework's `show_context_menu_for` — a `fern-core` hook that intercepts Secondary `PointerDown` on any arena node — walks up from the hit widget to find the nearest ancestor whose `context_menu_factory` is set, calls that factory to produce a **fresh** menu widget each right-click, and shows it at the pointer position (`OverlayPlacement::AtPointer`). No widget-level overlay wiring, no manual selection-preservation guards, no `context_menu_open` flag — the framework handles it.

The factory returns a `MenuList` whose item set is filtered by `ClipboardPolicy`:

- `editor()` preset (`ClipboardPolicy::Full`): Cut / Copy / Paste / Paste Unformatted / — / Select All.
- `read_only()` preset (`ClipboardPolicy::CopyAndSelectAllOnly`): Copy / Select All.

Each `MenuItem`'s `on_activate_fn` captures a clone of the editor's `SharedState` and calls the corresponding `rt_clipboard::*` function **directly** — no Action/Intent indirection. The reason: the framework's `show_context_menu_for` adds the menu widget at the top of the arena (via `add_boxed`), so the menu's parent chain doesn't reach the editor; an `Intent` fired from a menu item can't walk up to an `Action` registered on the editor. Direct closure invocation sidesteps the whole question.

After doing the work, each closure fires a reserved `fern.rich_text.*` intent (`fern.rich_text.cut`, `…copy`, `…paste`, `…paste_unformatted`, `…select_all`) for observational handlers — the framework's current intent dispatch won't reach the editor from inside a top-level menu widget either, but the contract is stable for a future reworked dispatch.

Item enabled-state is computed at factory-call time from live state: `cursor.has_selection()` gates Cut and Copy; `document.to_plain_text()` gates Select All. Because the factory runs on every right-click, greyed entries reflect the moment the user opened the menu.

Host applications customise by passing their own factory:

```rust
RichTextEditor::editor(doc)
    .context_menu(|| Box::new(MyCustomMenu::new(...)))
```

The custom factory completely replaces the default. The inherent `RichTextEditor::context_menu` method shadows the blanket [`WidgetBuilder::context_menu`](../crates/fern-core/src/widget_builder.rs) trait method so users can chain it directly on the editor. Opt out entirely with `.default_context_menu(false)` — right-click then bubbles past the widget and [`context_target_at(point)`](../crates/fern-widgets/src/rich_text.rs) remains available for applications that want to render a menu from outside.

#### 27.10.14 IME and Composition

IME (Input Method Editor) support allows CJK and other complex-script users to compose characters via multi-keystroke sequences. The OS provides composition events: `ime_composition_changed` with the in-progress text, and `ime_commit` with the final committed string.

The editor handles IME via its `on_focus` handler: gaining focus enables IME via `fern-platform` and updates the IME composition window position to sit just below the caret. Losing focus disables IME. Composition events are routed through the editor's pointer/keyboard handler chain and applied to the document via cursor operations.

**IME is deferred to a post-Milestone-9 refinement.** Milestone 8 (RichTextEditor) and Milestone 9 (TextInput) both target Latin-script editing. IME requires platform-specific composition window positioning (winit exposes IME events but the composition window position is OS-specific) and rich text composition rendering (the in-progress text needs distinct visual styling — typically an underline). Both are achievable but add scope. The architectural hooks (`update_ime_position()` on focus enter, IME event handling in the keyboard pipeline) are designed in from the start so that adding IME later does not require API changes.

#### 27.10.15 Accessibility

The widget's AccessKit role is determined by the construction preset, not by a runtime flag:

- **`RichTextEditor::editor(...)`** declares `Role::MultilineTextInput` and sets `set_multiline(true)`. Screen readers enter forms-navigation mode on focus and announce the widget as an editable multi-line text field.
- **`RichTextEditor::read_only(...)`** declares `Role::Document` and sets `set_multiline(true)`. Screen readers treat it as a document to be read, not a form field to be filled.

Both presets expose the same AccessKit properties for the underlying content:

- `set_value(text)` — the plain-text content of the document.
- `set_text_selection(range)` — the current selection range as a character offset pair.
- `set_caret_position(offset)` — the caret position as a character offset, for presets where the caret participates in navigation.
- `Action::SetTextSelection` — handled by setting the cursor's anchor and position.
- `Action::ScrollIntoView` — handled by adjusting the scroll signals.

The `editor()` preset additionally handles `Action::SetValue` by replacing the document content, and sets `set_read_only(false)` so screen readers know the widget accepts typed input. The `read_only()` preset sets `set_read_only(true)` and does not handle `Action::SetValue` (the action is silently ignored if dispatched, rather than overwriting the document).

Screen readers can read the entire document content via `set_value` and track the caret position for keyboard navigation. Format information (bold, italic, headings) is not exposed via AccessKit in the first version because AccessKit's text-attribute support is platform-specific and incomplete. The plain-text representation is sufficient for the most common screen reader use cases.

**Why this matters for accessibility correctness.** Reporting `Role::MultilineTextInput` from a widget that does not accept input is a real accessibility bug — screen readers will announce "editing" on focus and enter forms-navigation mode, promising the user a text field that does nothing when they type. Reporting `Role::Document` from an editable widget would have the opposite bug: screen readers would treat the widget as read-only content and would not switch to forms-navigation mode, making typing feel broken. The preset-based design makes the right role an unambiguous consequence of which constructor was used, rather than a boolean flag that could drift out of sync with the widget's actual command acceptance.

#### 27.10.16 What This Means for Milestone 8 Implementation

Milestone 8 is significantly larger than other milestones because of the editor's complexity. The recommended decomposition is:

**M8a: Read-only preset** — `RichTextEditor::read_only(...)` with text selection, link/image click events, scroll bar integration, the frame loop pattern, the three-tier render strategy, and the document version Signal. This validates the architectural approach (document + typesetter ownership, frame loop bridge, no ScrollArea, dual scroll bars) without the complications of editing, undo, IME, or formatting commands. The command filter, caret policy, accessibility role, and clipboard policy bundle — i.e. the policy-preset machinery itself — is designed and implemented in this stage, so that M8b's `editor()` constructor is a second preset layered over the same core rather than a second widget.

**M8b: Editor preset** — `RichTextEditor::editor(...)` adds the editable command filter, cursor positioning, character insertion, deletion, formatting commands, undo/redo, debounced `text_changed` signals, click counting for double/triple click, sticky preferred X, drag-select with auto-scroll, plain-text clipboard mutations, and the typed-command builder methods.

Both stages share the same module structure (`fern-widgets/src/rich_text/`) with files for `lib.rs` (the `RichTextEditor` type and its two public constructors), `policy.rs` (the `CommandFilter`, `CaretPolicy`, `AccessibilityRole`, `ClipboardPolicy` types and the two preset bundles), `frame_loop.rs` (the per-frame effect logic), `paint.rs` (the four-pass render frame walker), `hit_test.rs` (region dispatch), and `keyboard.rs` (the keyboard handler, which consults the command filter before dispatching to the cursor). The M8a stage produces a usable read-only widget; the M8b stage adds the editor preset without modifying any file from M8a — it only extends `policy.rs` with the editable bundle and adds editor-only methods to `keyboard.rs`.

The Godot reference at github.com/jacquetc/godot-rich-text is the working implementation of this same design in a different framework. It is approximately 2,100 lines for the editor and 780 lines for the viewer, with another 580 lines of shared bridge/input/fonts code. In the Godot implementation, the editor and viewer are two separate classes because GDScript has no clean mechanism for a type with multiple named constructors that bundle different behavior. FernUI's Rust implementation collapses the two into one type with two constructors; the overall line count is similar (~3,500 lines), but the split between "shared" and "editor-specific" is cleaner because the policy types mediate the interface rather than inheritance.

---

**TextInput** — Milestone 9 widget, plain-text specialization of `RichTextEditor`. A natural fit for the policy-preset machinery introduced in M8 (§28.10.1): TextInput is a thin wrapper that constructs a `RichTextEditor` with a command filter rejecting formatting commands (Bold, Italic, Heading), an Enter key handler that emits `on_submit` instead of inserting a new block, and an optional single-line constraint that rejects Enter entirely. Bound to a `Signal<String>` via two-way binding with the underlying TextDocument's plain-text representation. Cursor rendering, selection, keyboard editing, and clipboard are all inherited from RichTextEditor — TextInput is a thin configuration layer, not a reimplementation. Whether TextInput exposes itself as its own public type or as a third `RichTextEditor::plain_text(...)` constructor preset is a judgement call for M9 — the former gives TextInput a distinct name and builder methods, the latter emphasizes the shared implementation. Either way, the underlying code is the same. NumberInput/SpinBox is TextInput plus increment/decrement buttons plus a numeric validation filter on character input. IME is deferred (see §28.10.14). Accessibility: `Role::TextInput` with `set_value`, `set_text_selection`, `Action::SetValue`.

### 28.11 Platform-Dependent (`fern-platform/`, `fern-app/`)

**Native MenuBar integration** — planned for Milestone 10. The in-window `MenuBar` widget (Milestone 4, above) already provides the Windows/Linux implementation. On macOS, Milestone 10 adds a platform abstraction: the application declares its menu structure once through the FernApp builder, and `fern-platform` translates it to native `NSMenu` at runtime. Blocked on: Cocoa/AppKit interop code that goes beyond what winit provides.

**Clipboard** — new utility (not a widget). Platform-specific read/write of text and typed data via OS clipboard APIs. Shares the MIME-typed payload model with drag-and-drop. Blocked on: platform integration code in `fern-platform` (`arboard` crate or direct OS API calls).

**FileDialog** — new utility (not a widget). Native open/save file dialogs via platform APIs. Implemented via the `rfd` crate or direct OS API calls. Returns a path asynchronously. Blocked on: nothing in principle (rfd is a standalone crate), but integrating the async result back into the single-threaded UI model requires the EventLoopProxy channel pattern.

---

## 29. V2 Widget Authoring Model

This section defines the redesigned widget authoring surface for FernUI. The redesign unifies two traits into one, replaces four reactivity types with one, and moves event handling from a monolithic method to attached handlers. The underlying framework infrastructure — the arena, layout protocol, rendering pipeline, overlay system, animation scheduler, accessibility integration, window management — is unchanged. What changes is the API that widget authors program against.

The redesign is motivated by three problems identified during Milestone 3 implementation. The `Widget` / `CompositeWidget` split forces the wrong decision at definition time — a widget must choose between custom painting and child composition, when real widgets need both. The `RefCell<Option<State<T>>>` pattern is required by every stateful composite because `CompositeWidget::build()` takes `&self` while `event()` needs mutable access to state created during `build()`. The four reactivity types (`State<T>`, `DerivedState<T>`, `Reactive<T>`, `StateHandle<T>`) expose implementation details that widget authors should not need to understand.

### 29.1 Unified Widget Trait

The `CompositeWidget` trait (composite_widget.rs) and `CompositeWidgetAdapter` (composite_adapter.rs) are removed. There is one trait:

```rust
pub trait Widget: std::fmt::Debug + 'static {
    /// Construct children. Called once after the widget is placed in the arena.
    /// Takes &mut self — store child IDs, signal handles, any state needed later.
    /// Returns the list of root child IDs (empty for leaf widgets).
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        vec![]
    }

    /// Respond to the parent's size proposal.
    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size;

    /// Position children within the allocated bounds.
    fn place_children(
        &self, _bounds: Rect, _children: &mut [ChildPlacement], _ctx: &LayoutContext,
    ) {}

    /// Paint this widget's own visuals. Children are painted automatically
    /// after this method returns.
    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {}

    /// Declare accessibility identity.
    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}

    /// Return child widget IDs (for arena traversal).
    fn children(&self) -> Vec<WidgetId> { vec![] }
}
```

Six methods. Only `size_that_fits` is required — all others have defaults. A leaf widget (TextWidget, RectWidget, IconWidget) implements `size_that_fits` and `paint`. A layout container (HStack, VStack, ZStack) implements `size_that_fits`, `place_children`, and `children`. A composing widget (Button, Checkbox) implements `build` and `accessibility`. A hybrid widget (Card, Toggle, ScrollArea) implements `build` for children and `paint` for its own visuals.

Methods removed from the trait compared to V1:
- `event()`, `preview_event()` — replaced by attached handlers (Section 28.3).
- `is_focusable()`, `tab_index()` — replaced by `.focusable(true)`, `.tab_index(n)` builder methods stored on the arena node.
- `is_spacer()` — replaced by a flag on the arena node, set by Spacer during construction.
- `is_composite()`, `as_any_mut()` — gone, no composite adapter to distinguish or downcast.
- `register_bindings()` — gone, signal-to-widget bindings register automatically through `Prop<T>` resolution.
- `take_pending_children()`, `set_resolved_children()`, `take_visible_when()`, `take_enabled_when()` — moved to the `WidgetBuilder` blanket impl and arena resolution.

### 29.2 The `build(&mut self)` Lifecycle

The `build` method takes `&mut self`, eliminating the `RefCell<Option<State<T>>>` pattern. Widget authors store child IDs, signal handles, and any construction-time state as plain struct fields:

```rust
pub struct Button {
    label: String,
    style: ButtonStyle,
    action: Option<Box<dyn Fn(&mut EventContext)>>,
    // Plain fields — no RefCell, no Option wrapper
    interaction: Signal<InteractionState>,
    rect_id: WidgetId,
    text_id: WidgetId,
}
```

The `interaction` signal, `rect_id`, and `text_id` are set during `build()` with `&mut self` access:

```rust
fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    self.interaction = ctx.signal(InteractionState::Idle);
    self.text_id = ctx.add(TextWidget::new(&self.label).color(text_color));
    self.rect_id = ctx.add(RectWidget::new().background(bg_color));
    // ...
    vec![root_id]
}
```

**Borrow safety.** The widget is stored in the arena. During `build()`, both `&mut self` (the widget) and `&mut BuildContext` (wrapping `&mut WidgetTree`, which owns the arena) are needed. This would be a double mutable borrow if the widget remained in the arena. The solution already exists in the codebase: `arena.take_widget(id)` extracts the widget box from its arena node (replacing it with a `PlaceholderWidget`), the extracted widget is mutated freely, then `arena.restore_widget(id, widget_box)` puts it back. The borrow checker is satisfied because the widget is not in the arena during the `build()` call:

```rust
// Inside WidgetTree — widget authors never see this:
fn build_widget(&mut self, id: WidgetId) {
    let mut widget_box = self.arena.take_widget(id).unwrap();
    let child_ids = {
        let mut ctx = BuildContext { tree: self };
        widget_box.build(&mut ctx)
    };
    self.arena.restore_widget(id, widget_box);
    // Wire child_ids as children of id in the arena
}
```

**When build() is called.** Once, after the widget is inserted into the arena via `ctx.add()` or `tree.add()`. On environment change (theme switch, locale switch), `build()` is called again on the same widget: the old child subtree is destroyed, effects from the previous build are cleaned up, and `build()` runs with `&mut self` to construct a fresh subtree. The widget struct persists across rebuilds — only the children are replaced.

**Constraint.** During `build()`, the widget cannot read its own arena node (bounds, parent, activation state) because it has been extracted. This is correct — `build()` runs before the first layout, so the widget has no bounds yet. If a widget needs its own ID during `build()`, `BuildContext` provides `ctx.self_id()`.

### 29.3 Attached Event Handlers

Instead of implementing a monolithic `event()` method with a match on every event variant, widgets attach named handlers that express intent. Handlers are closures stored on the arena node, dispatched by the framework during the existing preview/bubble event passes.

```rust
// At the widget construction site or inside build():
MinSize::new(48.0, 48.0)
    .child(content)
    .on_tap(|ctx| { ctx.send_intent(AppIntent::Clicked); })
    .on_hover(|entered, ctx| {
        interaction.set(if entered { Hovered } else { Idle });
    })
    .focusable(true)
    .cursor(CursorIcon::Pointer)
```

The handler methods are defined on a `WidgetBuilder` trait that is blanket-implemented for all `Widget` types:

```rust
pub trait WidgetBuilder: Widget + Sized {
    // Gesture handlers — the framework attaches the appropriate recognizer
    fn on_tap(self, f: impl FnMut(&mut EventContext) + 'static) -> Self;
    fn on_double_tap(self, f: impl FnMut(&mut EventContext) + 'static) -> Self;
    fn on_long_press(self, f: impl FnMut(Point, &mut EventContext) + 'static) -> Self;
    fn on_drag(self, f: impl FnMut(DragPhase, &mut EventContext) + 'static) -> Self;

    // Focus and keyboard
    fn on_focus(self, f: impl FnMut(bool, &mut EventContext) + 'static) -> Self;
    fn on_key(self, f: impl FnMut(Key, Modifiers, &mut EventContext) -> bool + 'static) -> Self;
    fn focusable(self, focusable: bool) -> Self;
    fn tab_index(self, index: i32) -> Self;

    // Pointer (low-level escape hatch for custom interaction)
    fn on_pointer_event(self, f: impl FnMut(&PointerEvent, &mut EventContext) -> bool + 'static) -> Self;
    fn on_hover(self, f: impl FnMut(bool, &mut EventContext) + 'static) -> Self;
    fn cursor(self, cursor: CursorIcon) -> Self;

    // Scroll
    fn on_scroll(self, f: impl FnMut(ScrollDelta, &mut EventContext) -> bool + 'static) -> Self;

    // Accessibility actions
    fn on_access_action(self, f: impl FnMut(accesskit::Action, &mut EventContext) -> bool + 'static) -> Self;

    // Framework-level properties
    fn visible_when(self, signal: impl Into<Prop<bool>>) -> Self;
    fn enabled_when(self, signal: impl Into<Prop<bool>>) -> Self;
    fn tooltip(self, text: impl Into<String>) -> Self;
}
```

Handler attachment works by storing closures temporarily on the widget value (via a generic metadata wrapper or a separate `HandlerSet` struct stored alongside the widget). When `BuildContext::add()` or `WidgetTree::add()` inserts the widget into the arena, the handlers are transferred to the `WidgetNode`. This is the same mechanism currently used for `take_visible_when()` and `take_pending_children()`, generalized to all node metadata.

**How dispatch works.** The framework's event dispatch path (preview pass root → target, bubble pass target → root) is unchanged. At each node, instead of calling `node.widget.event(event, ctx)`, the framework checks which handler is relevant for the event type and calls it. A `PointerDown`/`PointerUp` sequence on a node with `on_tap` feeds through a `TapRecognizer` (attached automatically when `on_tap` is called) and invokes the handler when the recognizer reports a recognized tap. A `PointerEnter`/`PointerLeave` on a node with `on_hover` invokes the hover handler. The gesture recognizer infrastructure (`TapRecognizer`, `DragRecognizer`, `GestureArena`) is used internally — the widget author never instantiates a recognizer.

**Low-level escape hatch.** For widgets that need raw pointer events (a color wheel, a node graph editor, a custom drawing canvas), `on_pointer_event` provides unprocessed `PointerEvent::Down`, `PointerEvent::Move`, `PointerEvent::Up` with full position and button information.

### 29.4 Signal<T> — Unified Reactivity

`State<T>`, `DerivedState<T>`, `Reactive<T>`, and `StateHandle<T>` are replaced by a single public type: `Signal<T>`.

```rust
pub struct Signal<T> { /* Rc<RefCell<SignalInner<T>>> */ }

impl<T: 'static> Signal<T> {
    /// Create a mutable signal with an initial value.
    pub fn new(value: T) -> Self;

    /// Read the current value.
    pub fn get(&self) -> Ref<'_, T>;

    /// Set a new value. Marks the signal as dirty, which causes
    /// the framework to mark bound widgets for repaint or relayout.
    /// Panics if called on a derived (read-only) signal.
    pub fn set(&self, value: T);

    /// Create a derived (read-only) signal whose value is computed
    /// from this signal. The closure runs lazily when the derived
    /// signal is read, not eagerly when the source changes.
    pub fn map<U: 'static>(&self, f: impl Fn(&T) -> U + 'static) -> Signal<U>;

    /// Register an observer callback. Returns an ObserverHandle —
    /// dropping the handle removes the callback. For application-level
    /// coordination, not for widget bindings (use properties or effects).
    pub fn observe(&self, f: impl Fn(&T) + 'static) -> ObserverHandle;

    /// Whether two Signal handles point to the same underlying value.
    pub fn same(a: &Self, b: &Self) -> bool;
}

impl Signal<f32> {
    /// Animate to a target value over a duration with an easing curve.
    /// Registers the animation with the AnimationScheduler.
    pub fn animate_to(&self, target: f32, duration: Duration, easing: Easing);
}
```

Internally, `Signal<T>` has two variants: mutable (created via `Signal::new`) and derived (created via `signal.map()`). The mutable variant stores the value and a dirty flag. The derived variant stores a computation closure and a reference to the source signal's dirty flag. This is the same internal structure as the current `State<T>` and `DerivedState<T>`, but exposed through a single type.

`ObserverHandle` is a RAII guard. Dropping it removes the observer callback from the signal, fixing the memory leak in the current `observe()` which has no unsubscribe mechanism.

**Prop<T>** replaces `Reactive<T>` as the widget property type:

```rust
pub enum Prop<T: Clone + 'static> {
    Static(T),
    Bound(Signal<T>),
}

impl<T: Clone> From<T> for Prop<T> { /* Static */ }
impl<T: Clone> From<Signal<T>> for Prop<T> { /* Bound */ }
```

Widget property methods accept `impl Into<Prop<T>>`, allowing both plain values and signals:

```rust
// Static — set once, never changes:
TextWidget::new("Hello").color(Color::RED)

// Reactive — updates when signal changes:
TextWidget::new("Hello").color(text_color_signal)

// Same method signature handles both:
fn color(mut self, color: impl Into<Prop<Color>>) -> Self
```

The dirty tracking level (repaint vs relayout) is determined by the property method, not by the consumer. `.color()` registers a repaint-level binding. `.text()` registers a relayout-level binding. The `BindingRegistry` remains as an internal framework mechanism — widget authors never see it.

### 29.5 Scoped Effects

`ctx.effect()` replaces the unscoped `state.observe()` pattern for widget-internal side effects. Effects registered during `build()` are tied to the build cycle — on rebuild, old effects are cleaned up before the new `build()` runs. On widget destruction, effects are cleaned up with the widget.

```rust
fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    // This effect fires whenever self.selected_chapter changes.
    // Automatically cleaned up on rebuild or destruction.
    ctx.effect(&self.selected_chapter, |chapter_id| {
        println!("Chapter selected: {chapter_id}");
    });

    // ...
}
```

The `BuildContext` tracks all effects registered during this build call as a list of `ObserverHandle` values. Before each rebuild, the framework drops the handles from the previous build, removing the old callbacks. This is the same lifecycle model as SolidJS createEffect or React useEffect cleanup.

### 29.6 Arena Node Changes

The `WidgetNode` in `arena.rs` stores handlers and framework-level properties that were previously on the Widget trait or on the widget struct:

```rust
pub struct WidgetNode {
    // Unchanged from V1:
    pub widget: Box<dyn Widget>,
    pub parent: Option<WidgetId>,
    pub children: Vec<WidgetId>,
    pub activation: ActivationState,
    pub dirty: DirtyFlags,
    pub bounds: Rect,
    pub clips_children: bool,
    pub cached_paint: Option<RenderFrame>,
    pub(crate) theme_override: Option<ThemeOverride>,
    pub(crate) alignment_override: Option<fern_tokens::Alignment>,

    // V2: replaces gesture_binding, visible_state, enabled_state
    pub(crate) handlers: EventHandlers,
    pub(crate) visible_signal: Option<Signal<bool>>,
    pub(crate) enabled_signal: Option<Signal<bool>>,
    pub(crate) focusable: bool,
    pub(crate) tab_index: Option<i32>,
    pub(crate) is_spacer: bool,
    pub(crate) cursor: Option<CursorIcon>,
    pub(crate) has_built_children: bool,
    pub(crate) effect_handles: Vec<ObserverHandle>,
}
```

`EventHandlers` is a struct holding optional closures for each handler type:

```rust
pub(crate) struct EventHandlers {
    pub on_tap: Option<Box<dyn FnMut(&mut EventContext)>>,
    pub on_double_tap: Option<Box<dyn FnMut(&mut EventContext)>>,
    pub on_long_press: Option<Box<dyn FnMut(Point, &mut EventContext)>>,
    pub on_drag: Option<Box<dyn FnMut(DragPhase, &mut EventContext)>>,
    pub on_hover: Option<Box<dyn FnMut(bool, &mut EventContext)>>,
    pub on_key: Option<Box<dyn FnMut(Key, Modifiers, &mut EventContext) -> bool>>,
    pub on_focus: Option<Box<dyn FnMut(bool, &mut EventContext)>>,
    pub on_pointer_event: Option<Box<dyn FnMut(&PointerEvent, &mut EventContext) -> bool>>,
    pub on_scroll: Option<Box<dyn FnMut(ScrollDelta, &mut EventContext) -> bool>>,
    pub on_access_action: Option<Box<dyn FnMut(accesskit::Action, &mut EventContext) -> bool>>,
    pub(crate) gesture_arena: Option<GestureArena>,
}
```

When a widget with `on_tap` is added to the arena, the framework creates a `TapRecognizer` in the `gesture_arena`. When a widget has both `on_tap` and `on_drag`, both recognizers compete in the same `GestureArena`. The widget author never manages this — the framework infers the correct recognizer configuration from which handlers are attached.

### 29.7 Widget Tree Changes

The `WidgetTree` in `widget_tree.rs` simplifies:

**Insertion.** Two public methods replace the current five:

```rust
pub fn add(&mut self, widget: impl Widget) -> WidgetId;
pub fn add_child(&mut self, parent: WidgetId, widget: impl Widget) -> WidgetId;
```

No `add_widget()`, `add_composite()`, `add_composite_inner()`, `add_widget_direct()`. No `IntoWidgetTree` trait. No `impl_composite_into_widget_tree!` macro. One insertion path for all widgets.

**Build dispatch.** After insertion, if the widget's `build()` method returns a non-empty `Vec<WidgetId>`, the framework records `has_built_children = true` on the arena node. This replaces the `composite_ids: Vec<WidgetId>` tracking list.

**Rebuild on environment change.** When `set_theme()` or `set_locale()` is called, the framework iterates all nodes with `has_built_children == true`, calls the `take_widget` / `build` / `restore_widget` sequence for each, destroying old child subtrees and constructing new ones. This replaces `rebuild_composites()` and its `CompositeWidgetAdapter` downcast.

**Event dispatch.** The `dispatch_to_widget` method changes from calling `node.widget.event(event, ctx)` to checking the appropriate handler on `node.handlers` and calling it. The preview/bubble pass structure is unchanged. The gesture recognizer integration moves from per-node `gesture_binding` to `handlers.gesture_arena`, with the framework feeding raw pointer events through the arena and dispatching recognized gestures to the corresponding handler.

### 29.8 What Each Widget Type Looks Like

**Leaf primitive (TextWidget, RectWidget, IconWidget, Divider).** Implements `size_that_fits` and `paint`. Properties use `Prop<T>` with `Into<Prop<T>>` for static/reactive flexibility. No `build()`, no handlers, no `children()`. Binding registration happens automatically during arena insertion through `Prop<T>` resolution. Migration: replace `Reactive<T>` with `Prop<T>`, remove `register_bindings()`, `take_visible_when()`, `take_enabled_when()`. Net change per file: ~20 lines.

**Layout container (HStack, VStack, ZStack, Grid, Wrap, Padding, Expand, MinSize, MaxSize, FixedSize, Center, AspectRatio, Spacer, Switcher).** Implements `size_that_fits`, `place_children`, and `children`. No `build()`, no `paint()`, no handlers. The `place_children` logic is unchanged — it is the container's core responsibility and the SwiftUI layout negotiation is preserved exactly. Migration: remove trait methods that moved off Widget, replace `Reactive<bool>` with `Prop<bool>`. Net change per file: ~15 lines.

**Composing widget (Button, Checkbox, RadioButton).** Implements `build(&mut self)` to construct child subtrees, `size_that_fits` (delegates to child subtree via `ctx.child_size()`), and `accessibility`. Handlers (`on_tap`, `on_hover`, `on_key`) are attached to child widgets inside `build()`. The `RefCell<Option<State<T>>>` pattern is eliminated — state handles are plain struct fields set in `build()`. Migration: merge CompositeWidget impl into Widget impl, replace `State<T>` with `Signal<T>`, move `event()` match arms to handler attachments. Net reduction: ~30%.

**Custom-paint widget (Toggle, Slider, ScrollBar, ProgressBar).** Implements `paint` for custom rendering, `size_that_fits` for sizing, and optionally `build` if it has children (e.g., Toggle with a label). Handlers are attached at the construction site: `.on_tap(...)`, `.on_drag(...)`, `.on_pointer_event(...)`. The monolithic `event()` match block is replaced by focused handler closures. Migration: remove `event()`, attach handlers at construction. Net reduction: ~20%.

**Hybrid widget (Card, Panel, ScrollArea, Accordion).** Implements both `build` for child construction and `paint` for custom background/shadow/decoration rendering. This combination was awkward in V1 — Card had to be a Widget with manual child management because CompositeWidget had no `paint()`. In V2, `build()` and `paint()` coexist naturally on the same trait. Migration: add `build()` for child creation, keep `paint()` for visuals. Net simplification for every hybrid widget.

### 29.9 DSL Readiness

The unified model has one construction pattern for every widget: `WidgetType::new(args).property(value).on_event(handler).child(child_widget)`. This uniformity means a proc macro can provide a thin syntactic transform:

```rust
// Builder form:
ctx.add(
    VStack::new().spacing(8.0)
        .child(TextWidget::new("Title").style(heading))
        .child(
            HStack::new().spacing(4.0)
                .child(Checkbox::new(checked).label("Accept"))
                .child(Spacer::new())
                .child(Button::new("Submit").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Submit)))
        )
)

// Equivalent DSL form (future proc macro):
fern! {
    VStack(spacing: 8) {
        TextWidget("Title", style: heading)
        HStack(spacing: 4) {
            Checkbox(checked, label: "Accept")
            Spacer
            Button("Submit", on_activate_fn: |ctx| ctx.send_intent(AppIntent::Submit))
        }
    }
}
```

The macro compiles to the builder calls. No runtime overhead, no hidden allocation. Builder and DSL can be mixed — use DSL for tree structure, drop to builder for complex conditional logic. The macro can be added later without changing any widget code because the underlying builder API is the stable target.

**Status.** The `fern!` DSL has since shipped as [`fern-ui-macros`](../crates/fern-ui-macros/) (re-exported from the `fern-ui` umbrella crate as `fern!`). The surface is block-structured with newline-separated body items rather than the property-in-parens sketch above; every construct desugars one-to-one to the V2 builder calls at macro-expansion time — no runtime, no virtual tree. The user-facing reference is [`fern-macro-reference.md`](fern-macro-reference.md) (cheat sheet, limitations, worked translations). The formal grammar, error reporting, and desugaring rules are in [`fern-language-spec-v3.md`](fern-language-spec-v3.md). The widget_catalog example uses both the classic builder form and the `fern!` form side by side in split panes; see [`examples/widget_catalog`](../examples/widget_catalog/).

### 29.10 Impact Assessment (Post-Migration)

The V2 migration is complete. All widgets use the unified Widget trait and Signal<T> reactivity.

**Deleted.** `composite_widget.rs` (142 lines) and `composite_adapter.rs` (139 lines). No `CompositeWidget` references remain anywhere in the codebase. No `RefCell<Option<State<T>>>` pattern remains.

**New files in fern-core.** `signal.rs` (793 lines) implements `Signal<T>`, `Prop<T>`, `ObserverHandle`, with bridge `From` impls to/from V1 types. `build_context.rs` (139 lines) provides `BuildContext` with V2 APIs (`ctx.signal()`, `ctx.effect()`, `ctx.animated_signal()`, `ctx.self_id()`, `ctx.apply_self_handlers()`) and V1 compatibility APIs (marked as legacy). `event_handlers.rs` (86 lines) defines `EventHandlers` struct with optional closures for each handler type. `widget_builder.rs` (438 lines) defines `HandlerSet` and the `WidgetBuilder` blanket trait.

**Unified Widget trait.** `widget.rs` (266 lines) has the six-method trait: `build(&mut self)`, `size_that_fits`, `place_children`, `paint`, `accessibility`, `children`. All 22 widget files in fern-widgets use `impl Widget for` with no composite distinction. All container primitives (HStack, VStack, Grid, Wrap, Expand, FixedSize, MinSize, MaxSize, Center, AspectRatio) implement `build()` for PendingChild resolution.

**Signal<T> adoption.** Every interactive widget uses `Signal<T>`: Button, Toggle, Checkbox, RadioButton, Slider, Accordion, Badge, Card, Link, SegmentedControl, ScrollArea, ScrollBar, ProgressBar. ScrollArea uses `Signal<f32>` for all six scroll state fields (scroll_y, scroll_x, max_scroll_y, max_scroll_x, viewport_ratio_y, viewport_ratio_x). Toggle is fully Signal-ified (no `Rc<Cell<>>` remaining). ProgressBar uses `Prop<T>` for fill/track colors and `Signal<f32>` for indeterminate animation. The widget_catalog example uses exclusively `ctx.signal()` (14 calls, zero `ctx.state()`).

**Handler attachment.** Two valid patterns: widgets attach handlers to child widgets via `.on_tap()` builder methods (Checkbox on MinSize, Accordion on its header), or attach handlers to themselves via `HandlerSet::new()` + `ctx.apply_self_handlers()` (Button, Toggle, Slider, SegmentedControl). The framework auto-wires gesture recognizers when handlers are attached.

**Animation.** The `AnimationScheduler` supports `Signal<f32>` for animation targets. Toggle, Accordion, ScrollArea, and ProgressBar use `Signal<f32>::animate_to()` for smooth animated transitions.

**Final line counts.** fern-core: 9,989 lines (was 8,554 in V1 — net increase from signal.rs, build_context.rs, event_handlers.rs, widget_builder.rs, animation.rs expansion, offset by composite deletions). fern-widgets: 11,641 lines (was 11,780 in V1 — slight net reduction despite adding more features, each widget simpler). Infrastructure crates (tokens, canvas, text, render, platform, app): 8,946 lines — untouched by V2 migration.

### 29.11 Superseded Sections

The following sections describe the V1 model and should be read as historical context:

- Section 5 (Widget Extensibility) — the two-tier Widget/CompositeWidget model is replaced by the unified Widget trait.
- Section 7 (Reactivity Model) — State/DerivedState/Reactive is replaced by Signal/Prop. The V1 types remain in state.rs for internal use and backward compatibility but are not the primary API.
- Section 9.1 (Input Event Routing) — the `event()` dispatch model is replaced by attached handlers. Widgets no longer implement `event()` on the trait.
- Section 25 (Button — Reference Widget Design) — the Button implementation should be updated to V2 patterns (the actual Button code in button.rs is already V2).

Sections 2 (Layout Model), 3 (Scrolling), 6 (UI Construction Patterns), 8 (Dormancy), 10 (Gesture Recognition), and all infrastructure sections remain current and accurate.

---

## 30. Open Questions (Current, April 2026)

The bulk of the original post-milestone question list has landed. The short list below is what remains actively open; see [`fern-ui-milestones.md`](fern-ui-milestones.md) for detailed status and the Next-candidates roadmap.

**IME composition and CJK input.** The text input widget ships (Milestone 9), but IME composition window positioning, composition-text rendering, and dead-key / CJK input handling still need platform backends in `fern-platform`. The TextInput and RichTextEditor APIs don't change when this lands — the hooks are already in place.

**Cross-application drag-and-drop.** Intra-app DnD works everywhere (Milestone 6). Dragging between a FernUI window and a file manager or other app requires per-OS backends: `WaylandDragBackend` (wl_data_device), `X11DragBackend` (XDnD), `WindowsDragBackend` (OLE IDataObject / IDropTarget), `MacOsDragBackend` (NSPasteboard / NSDraggingSource). The payload type (`DragPayload`) and the widget-side handler API are stable; what's pending is the platform integration surface.

**Native menu bar on macOS + native file dialogs.** The widget-based `MenuBar` (Milestone 4) is correct for Windows and Linux where menu bars live inside the window chrome. On macOS the OS expects menus to live in the global `NSMenu`. The remaining work is a platform abstraction that routes a single declarative menu description through either path. Native file dialogs (via `rfd`) follow the same pattern — async result through `EventLoopProxy`.

**Virtualized dropdowns.** `ComboBox` now virtualizes via `ListView` under `max_visible_items`: lists beyond the cap materialize only the visible rows (plus `ListView`'s small buffer) instead of building every `DropdownItem` eagerly. The searchable (`rich-text`) filtered path shares the same virtualized renderer. `MenuList` grew a `max_visible_items` builder that caps panel height and wraps the item column in a `ScrollArea`, but does **not** virtualize — its API still takes arbitrary `impl Widget` children, so true virtualization would require a model-driven MenuList rewrite (tracked as follow-up). The eager build is cheap enough that capped 100+ item menus are fine in practice.

**ShortcutFormatter.** `shortcut.rs` still renders chords as `Ctrl+S` regardless of platform or locale. The design calls for `⌘S` on macOS and translated modifier names (`Strg+S` in German). Scope is a single formatter hook consulted by `MenuItem::for_shortcut(id)` and `TooltipContent::for_shortcut(id)`. Tracked alongside the M7 polish.

**Widget-level vs. application-level undo.** Text input undo (last few typed characters coalesced into one undo step) and application undo (the domain's use-case undo stack) coexist in the rich text editor. The current design keeps them separate and lets the application decide when to promote widget-local undo records into the domain log. The generalization to other widgets — undo for a slider drag, undo for a selection change — has not been designed and may or may not prove necessary.

---

## 31. First Milestone: Button in a Window

The first concrete deliverable is a window displaying a single button that responds to clicks, changes visual state on hover/press, renders text via text-typeset, announces itself to screen readers via AccessKit, and respects a theme.

This milestone exercises: fern-tokens (theme definition), fern-canvas (Canvas API, SDF rounded rect), fern-core (arena, layout, event dispatch, focus, accessibility), fern-text (shared Typesetter for button label), fern-render (wgpu pipeline, atlas upload, quad/rect/SDF shaders), fern-platform (winit window, input translation, AccessKit adapter), and fern-app (event loop, FernApp builder).

The milestone does not require: fern-i18n (use literal strings), fern-widgets (the button is built inline as a test), overlays, drag-and-drop, data sources, dormancy, or scrolling.
