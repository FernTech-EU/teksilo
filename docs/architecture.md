# Bastyde Architecture Document

**Version:** 0.3 — slim refresh
**Date:** May 6, 2026
**Author:** Cyril Jacquet, with architectural design by Claude (Anthropic)
**Status:** Living reference — framework-internals doc; companion focused docs in this directory own the per-subsystem API surface

> **Document scope.** This document covers the *framework-internals* topics
> that have no dedicated home elsewhere: scrolling, arena state, Canvas,
> rendering pipeline, HiDPI, threading, testability, crate structure,
> and the comparative-design rationale. Every subsystem with a dedicated
> reference doc in this directory has been collapsed here to a one-paragraph
> pointer; section numbers are preserved so external links by `§N` and
> heading-slug anchors continue to resolve.
>
> If you are looking for *how to use* a subsystem, the focused doc is the
> right entry point. Read this doc when you are debugging the engine,
> porting to a new platform, writing a custom widget that needs the Canvas
> escape hatch, or onboarding to maintain the framework itself.
>
> **Where the per-subsystem references live:**
>
> - Layout: [`layout-primitives.md`](layout-primitives.md)
> - Events / gestures / focus / DnD lifecycle: [`events-and-gestures.md`](events-and-gestures.md)
> - Animation: [`animation.md`](animation.md)
> - Idle / zero-frame rule: [`idle-and-animation.md`](idle-and-animation.md)
> - Reactivity & theming: [`reactive-theme.md`](reactive-theme.md)
> - Shortcuts / intents / actions: [`shortcut-intent-action.md`](shortcut-intent-action.md)
> - i18n: [`i18n.md`](i18n.md)
> - Accessibility overrides: [`accessibility-overrides.md`](accessibility-overrides.md)
> - Drag and drop: [`drag-and-drop.md`](drag-and-drop.md)
> - Data models: [`data-models.md`](data-models.md)
> - Settings and persistence: [`settings.md`](settings.md)
> - Telemetry: [`telemetry.md`](telemetry.md)
> - Multi-window: [`multi-window.md`](multi-window.md)
> - Custom title bar: [`title-bar.md`](title-bar.md)
> - Tooltips and overlays: [`tooltips.md`](tooltips.md)
> - `bati!` DSL: [`bati-macro-reference.md`](bati-macro-reference.md), [`bati-language-spec-v3.md`](bati-language-spec-v3.md)
> - Inspector: [`inspector.md`](inspector.md)
> - Widget catalog snapshot: [`bastyde-milestones.md`](bastyde-milestones.md), `tools/extract_widget_api.py --all`
> - Per-widget reference docs: [`table-view.md`](table-view.md), [`tab-widget.md`](tab-widget.md), [`charts.md`](charts.md), [`bastyde-scene.md`](bastyde-scene.md)

---

## 1. Vision and Positioning

Bastyde is a pure-Rust GUI framework for serious desktop applications — the kind of software where a user sits down for hours at a time and reaches for the keyboard first. A writing tool for novelists, an IDE, a dispatch console, a course manager for a taxi company's driver training. Bastyde is infrastructure for professional desktop software that needs native look and feel, full keyboard and screen-reader accessibility, and a rich text surface built from the ground up.

Bastyde's thesis rests on three pillars. First, accessibility is a structural requirement, not an afterthought — AccessKit is integrated at the trait level, not bolted on. Second, rich text is a first-class concern — the text-document and text-typeset crates provide a complete document model and typesetting engine covering shaping, bidi, line-breaking, and atlas rasterization. Third, the framework is designed to be consumed by applications with structured architecture (Clean Architecture, MVVM), providing a typed Shortcut / Intent / Action pipeline and reactive data-model crate (`bastyde-data`) rather than leaving application structure as an exercise for the developer.

### 1.1 Relationship to structured application architectures

Bastyde is the outermost layer of an application — the "Frameworks & UI" ring in Clean Architecture's concentric circles. It has no dependency on any particular application framework. A Qleany-structured application is one supported integration path and was the stress test that shaped several of Bastyde's architectural choices (typed intents for command flow, view-models over raw entities, data sources for paged external collections), but nothing in Bastyde *requires* Qleany.

The integration surface is the typed intent system (Bastyde widgets emit application-defined intent variants that ancestor `Action`s consume — see [`shortcut-intent-action.md`](shortcut-intent-action.md)) and the reactive data models in `bastyde-data` (application-written view-models hold entity collections as `ListModel<EntityVM>` / `TreeModel<EntityVM>` that widgets bind to — see [`data-models.md`](data-models.md)).

Bastyde splits internally into focused crates (see §25) each with a single concern, rather than imposing a Clean-Architecture split on its own internals. Layout, rendering, and event dispatch have fundamentally different performance characteristics from transactional domain operations; the useful seams fall in different places.

### 1.2 Reuse Strategy

Bastyde builds on established crates rather than reinventing solved problems. **winit** for windowing and HiDPI; **wgpu** for GPU rendering; **text-document + text-typeset** for the rich text model and typesetting (rustybuzz shaping, swash rasterization, etagere atlas, unicode-linebreak, unicode-bidi); **AccessKit** for cross-platform a11y; **fluent-rs** for i18n; **tiny-skia** for Tier 3 path rasterization.

---

## 2. Layout Model

Full reference: [`layout-primitives.md`](layout-primitives.md). The protocol is SwiftUI-style two-phase negotiation — the parent proposes a size, the child responds with `LayoutResponse { size, flex }`, the parent places. Slack distribution, flex weights, zero-basis vs `respect_intrinsic`, container alignment, per-child alignment overrides, and the size-wrapper primitives (`Expand`, `FixedSize`, `MinSize`, `MaxSize`, `Center`, `Padding`, `Spacer`, `Divider`) all live there.

What's *not* in the focused doc and stays here:

### 2.1 Binding Levels and Dirty Propagation

Some property changes affect only a widget's visual appearance (a color change). Others affect the widget's size (a text change, a constraint change). The binding system distinguishes these two cases because they trigger different dirty-tracking responses.

**Repaint-level bindings** (`bind_color`, `bind_background`, `bind_border_color`) mark the widget for repaint only when the bound state changes. The layout pass is skipped — the widget's position and size are unchanged. This is the fast path, used for interaction-driven visual state changes (hover color, pressed color, enabled/disabled appearance).

**Relayout-level bindings** (`bind_text`, `bind_width`, `bind_height`, `bind_min_width`, `bind_max_height`) mark the widget for relayout when the bound state changes. The layout pass reruns on the affected subtree, and the dirty flag propagates upward to ancestors because a child's size change may affect its parent's size, which may affect the grandparent's size, and so on. Propagation stops at an ancestor whose own size is not affected by its children (for example, a `FixedSize` wrapper with a static width).

The classification is determined by the primitive widget's binding method implementation, not by the consumer. A `TextWidget` implementor knows that `bind_text` is relayout-level because changing the text changes the widget's `layout_response` result. A composite widget author or application developer does not need to think about this distinction — they call `bind_text(state)` and the framework handles the rest.

**Layout utility widgets with dynamic constraints.** The size constraint widgets (`MinSize`, `MaxSize`, `FixedSize`) accept state bindings for their constraint values, enabling dynamic resizing from application state changes, user-driven splitter interactions, or animation ticks. `FixedSize::bind_width(state)` registers a relayout-level binding — when the state changes, the widget's constraint changes, triggering relayout of the affected subtree.

**Relayout propagation.** When a widget is marked for relayout, the framework marks the widget and all its ancestors up to the root as needing relayout. During the layout pass, it starts from the highest dirty ancestor and works downward, re-running `layout_response` and `place_children` for each dirty node. Clean subtrees are skipped. This is the same incremental layout approach used by web browsers and by Qt's layout system. A relayout always implies a repaint for the affected widgets.

---

## 3. Scrolling and Viewports

A scroll area is a container whose content may be larger than the visible space. The scroll area acts as a viewport — a window into a potentially large content region. Only the visible portion of the content is rendered, clipped to the viewport boundary.

Scrolling is designed to require minimal changes to the framework. The scroll offset is encoded through the existing layout placement mechanism, not as a separate coordinate transformation layer. Hit testing, event dispatch, and the state system require no modifications. The changes are confined to the arena (one new flag), the paint pass (clip rect support), the renderer (scissor rects), focus management (scroll-into-view), and the scroll area widget itself.

### 3.1 Layout: Unbounded Proposals and Offset Placement

A scroll area participates in layout like any other container widget. In `layout_response`, it claims the space its parent offers — this becomes the viewport size. In `place_children`, it proposes an unbounded size on the scroll axis to its content child. For a vertical scroll area, the content receives `SizeProposal { width: Some(viewport_width), height: None }` — "use the viewport width, but be as tall as you need." The content child responds with its natural height (potentially thousands of logical pixels).

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

The scroll bar is a standalone Level 2 widget in `bastyde-widgets`, not a rendering detail inside ScrollArea. A standalone widget participates in the framework's hit testing, event dispatch, focus, and accessibility systems. Its thumb is a region within its bounds that the framework's existing pointer routing handles. Its accessibility node declares `Role::ScrollBar` with `set_numeric_value`, `set_min_numeric_value`, `set_max_numeric_value`, and `Action::SetValue`.

The ScrollBar stores the current scroll position and the content-to-viewport ratio (both provided by the ScrollArea via shared `Signal<f32>`). It computes thumb position and size from these values. It handles `PointerDown` on the thumb (start drag), `PointerMove` during drag (update position), `PointerUp` (end drag), and `PointerDown` on the track (page-scroll toward click position). It supports both vertical and horizontal orientations.

### 3.7 ScrollArea and ScrollBar Interaction

The ScrollArea owns the scroll state (`Signal<f32>` for each axis). The ScrollBar reads from and writes to this shared state. The ScrollArea and ScrollBar communicate through the reactive binding system, not through events or callbacks.

The ScrollArea supports two scroll bar display modes via `ScrollBarStyle`.

**Overlay mode** (default, matching macOS and modern Linux). The ScrollArea's viewport occupies the full available width — the scroll bar does not reduce the content area. A thin passive scroll indicator (a few semi-transparent pixels at the trailing edge) is painted directly by the ScrollArea during scrolling as a visual hint. When the pointer enters the scroll bar activation zone (a region at the trailing edge wider than the thin indicator), the ScrollArea shows the full interactive ScrollBar widget as an overlay using the existing overlay system (`OverlayPlacement::NearAnchor`, `DismissBehavior::PointerLeave`). The overlay ScrollBar appears on top of the content, receives pointer events for thumb drag and track click, and dismisses when the pointer leaves. The viewport width never changes. The transition from thin indicator to full scroll bar can be animated using the animation scheduler.

**Permanent mode** (matching traditional Windows/GTK style, or when the user's accessibility preferences request always-visible scroll bars). The ScrollBar is a layout sibling of the content viewport. The ScrollArea's internal structure becomes an HStack of `[clipping viewport]` + `[ScrollBar]`. The viewport is narrower by the scroll bar's width. The scroll bar is always visible and always interactive. The viewport width is constant (reduced by the scroll bar width but never changing dynamically).

The mode is selected via `ScrollArea::new(content).scroll_bar_style(ScrollBarStyle::Overlay)` or `ScrollBarStyle::Permanent`. The application or the theme can set a default. An accessibility preference for "always show scroll bars" overrides to Permanent mode.

### 3.8 The Scroll Area Widget

The ScrollArea is a Level 2 (`Widget` trait) widget in `bastyde-widgets`. It is the viewport container — it owns the clipping behavior, the layout negotiation with unbounded proposals, and the content offset placement described in Sections 3.1–3.5.

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

Bastyde uses a retained widget tree with arena-backed flat storage, following the approach proven by Masonry's TreeArena.

All widgets live in a flat `SlotMap`-like arena. Parent-child relationships are stored as ID references within the arena. The tree structure is explicit (unlike a pure ECS where relationships are implicit), but the flat storage avoids Rust's borrow-checker challenges with recursive mutable tree traversal.

The framework processes the tree through well-defined passes (event, layout, accessibility, paint), each of which traverses the arena without holding multiple mutable references simultaneously. This is the key insight from Masonry: separate the passes so that no pass needs to mutate a widget while reading another widget's state.

---

## 5. Widget Extensibility

The unified `Widget` trait has a single `build(&mut self, ctx)` for composition, a single `paint()` for own-visuals, and both are optional with sensible defaults. Leaf widgets implement `layout_response` + `paint`; container widgets implement `layout_response` + `place_children` + `children`; composing widgets implement `build` + `layout_response` (delegating to the child); hybrid widgets (Card, ScrollArea) implement `build` + `paint`. Reference: CLAUDE.md "Unified Widget Trait" and [`crates/bastyde-widgets/src/button.rs`](../crates/bastyde-widgets/src/button.rs).

### 5.1 The Slot System

Standard widgets ship with named extension points — slots — at structural boundaries where extension is anticipated. A slot is an optional placeholder that takes zero space when empty and accommodates arbitrary widget content when filled. Slots are part of a widget's public API contract; standard composites in bastyde-widgets ship with `leading_slot`, `trailing_slot`, `header_slot`, `footer_slot` at positions where extension is commonly needed.

```rust
TabWidget::new()
    .tab("Chapter 1", || chapter_editor(1))
    .trailing_slot(|ctx| {
        HStack::new()
            .child(Button::icon_only(Icon::Plus).on_activate_fn(|ctx| ctx.send_intent(AppIntent::AddChapter)))
            .child(Button::icon_only(Icon::ChevronDown).on_activate_fn(|ctx| ctx.send_intent(AppIntent::OpenChapterMenu)))
    })
```

---

## 6. UI Construction Patterns

The framework provides three child-addition methods on container builders — `add_child(WidgetId)` for pre-registered children, `child(impl IntoWidgetTree)` for inline insertion, and `children(iter)` / `child_opt(Option<_>)` for iterator and conditional shapes — plus the `Repeater` for dynamic non-virtualized collections driven by `ListModel<T>` change notifications. Composites use the static `child()` chain when content structure is fixed for the lifetime of the widget; `visible_when(Signal<bool>)` toggles individual subtrees between active and dormant without reconstruction; the `Repeater` handles small collections that change during interaction; `ListView` virtualizes large collections. The `bati!` DSL desugars to these same builder calls.

References: CLAUDE.md "Widget Construction Patterns", [`bati-macro-reference.md`](bati-macro-reference.md), [`data-models.md`](data-models.md) (Repeater vs ListView).

---

## 7. Reactivity Model

`Signal<T>` is the only reactive primitive. `Signal::new(x)` is mutable; `signal.map(f)` is read-only and derived; multi-source combinators (`a.zip(&b)`, `a.and(&b)` / `a.or(&b)` / `s.not()`) compose. `Prop<T>` is the widget-property wrapper accepting either a static `T` or a signal-bound value. `ObserverHandle` provides RAII cleanup; `WeakSignal<T>` breaks reference cycles. Builders accept `impl Into<Prop<T>>` for properties and `impl Into<ColorProp>` / `impl Into<TextStyleProp>` for theme-aware colors and typography.

The division of labor: simple property reactivity is **declarative** (the widget declares a binding, the framework reacts); structural changes (switching tabs, adding/removing children, activating/dormant-ing subtrees) are **imperative**, requested from a handler via `EventContext` (`ctx.set_dormant`, `ctx.activate`, `ctx.destroy`). This split is what lets Bastyde avoid both full view diffing and ad-hoc observer soup.

References: CLAUDE.md "Signals & Reactivity", [`reactive-theme.md`](reactive-theme.md), [`events-and-gestures.md`](events-and-gestures.md) (deferred operations).

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

---

## 9. Event System

Full reference: [`events-and-gestures.md`](events-and-gestures.md). Preview pass (root → strict ancestors of target) plus bubble pass (target → root); attached handlers stored on `WidgetNode` (`.on_tap` / `.on_hover` / `.on_key` / `.on_key_preview` / `.on_focus` / `.on_scroll` / `.on_pointer_event` / `.on_access_action`); auto-wired gesture recognizers; `EventContext` API including deferred mutations (`set_dormant` / `activate` / `destroy` / `request_focus` / `dismiss_all_overlays`); subtree state signals (`.focus_within(Signal<bool>)` / `.hover_within(Signal<bool>)`); AccessAction routing through the same dispatch machinery.

Backend events (database change notifiers, file watchers, message buses) plug in via the `EventSource` trait — widgets subscribe from `build()` via `BuildContext::subscribe_event`, and cross-thread forwarding goes through winit's `EventLoopProxy`. Per-widget lifetime cleanup: when the widget is destroyed, the subscription handle drops and the source unsubscribes.

---

## 10. Gesture Recognition

Full reference: [`events-and-gestures.md`](events-and-gestures.md). UIKit-style state machines (TapRecognizer, DoubleTapRecognizer / TripleTapRecognizer, LongPressRecognizer, DragRecognizer) with a `GestureArena` for competition. Recognizers are auto-wired from attached handlers — the framework instantiates a `TapRecognizer` when a node has an `on_tap` handler, a `DragRecognizer` when it has `on_drag`, and so on. Tap-family callbacks receive `&TapEvent { position, button, modifiers }`; default acceptance is `ButtonMask::PRIMARY` only (right-click never spuriously fires `on_tap`), widened via `.accept_tap_buttons(...)` and friends.

---

## 11. Actions, Intents, and Shortcuts

Full reference: [`shortcut-intent-action.md`](shortcut-intent-action.md). The three-layer pipeline: `Shortcut` (rebindable keystroke → intent name) → `Intent` (runtime DTO with optional typed payload) → `Action` (ancestor-registered handler keyed by intent name). `#[derive(IntentKind)]` with `#[name = "..."]` provides the typed-enum DTO bridge (unit, tuple, and struct variants). `ShortcutRegistry` holds two layers (declared defaults + persisted user overrides with graveyard semantics) and exposes a `Signal<u64> version` so menu labels and tooltips re-render on rebinds.

---

## 12. Internationalization

Full reference: [`i18n.md`](i18n.md). Fluent-rs runtime (`I18nManager`, `LocalizedString`, locale resolution, `.ftl` file watcher, fallback chains, `LayoutDirection` signal); compile-time-validating macros `tr!` / `tr_widget!` / `tr_signal!` / `tr_signal_widget!` that read `.ftl` files at expansion and validate every call against the parsed key map; locale-aware formatters (`NumberFormatter`, `BastydeDateTimeFormatter`, `BastydeDateTime`) backed by ICU4X (icu_decimal / icu_datetime / icu_calendar) with a custom `DATETIME()` Fluent function and bundle `set_formatter` callback so `{ NUMBER(...) }` / `{ DATETIME(...) }` inside `.ftl` messages render correctly across locales. Framework-string registration for `bastyde-widgets` is **explicit**: applications call `.framework_locales(bastyde_widgets::framework_locales())` on the `I18nConfig` builder chain.

---

## 13. Overlay System

Full reference: [`tooltips.md`](tooltips.md) for tooltips (plain + rich + registry, sticky-on-dwell, focus-driven a11y promotion). Multi-window modal flow lives in [`multi-window.md`](multi-window.md).

Engine internals: `OverlayManager` per `WidgetTree`. Two rendering layers — `OverlayLayer::InTree` (drawn into the same `RenderFrame` as the host) and `OverlayLayer::NativePopup` (separate winit window, used for menus that must escape the host window's bounds); `OverlayLayer::Auto` picks based on placement and platform. `OverlayPlacement` covers `Below` / `Above` / `BelowPreferred` (auto-flips on insufficient space) / `TrailingEdge` / `AtPointer` / `NearAnchor` / `BottomCenter`. `DismissBehavior::ClickOutside | PointerLeave | Manual` plus an Escape-cascade root handler. Delayed-open overlays (submenu hover delays) cancel via `EventContext::cancel_delayed_overlay(id)`. Overlay anchor positions invalidate on host relayout. AccessKit nodes for overlay content cascade under their logical parent, not the geometric root, so tooltips DescribeBy their anchor and menus Owned-By their menu bar item.

`OverlayRequest::with_fade(duration)` wires the framework-managed opacity tween at show/dismiss — caller specifies the duration, framework handles the signal, the `set_opacity` scope, and the deferred dormant-set after the dismiss tween completes.

---

## 14. Drag and Drop

Full reference: [`drag-and-drop.md`](drag-and-drop.md). Three scenarios (intra-widget reorder, inter-widget transfer, external/OS drops) share one machinery: typed `DragPayload`, source/target traits, hit testing under the cursor, drop-zone preview overlay, edge auto-scroll during hover, spring-load on dwell, full keyboard equivalence (`Cut` / `Copy` / `Paste` actions on a focused list/tree). **Inbound** external drops (files / text / URLs dragged from the OS into a window) are implemented via the `ExternalDndBackend` per-OS backends (macOS `NSDraggingDestination` verified; Windows OLE + Wayland `wl_data_device`; X11 no-op) and reuse the same machinery — see [`drag-and-drop.md` §11](drag-and-drop.md) and the `DropZone` widget. **Outbound** drags (Bastyde window → another app) are still pending — see §30 Open Questions.

---

## 15. Data Model

Full reference: [`data-models.md`](data-models.md). The `bastyde-data` crate sits between the widget tree and application view-models, providing `ListModel<T>`, `TreeModel<T>` + `TreeSlice<T>`, `SelectionModel`, and the `ListDataSource` trait for paged/external collections. `SortFilterListModel<T>` and `SortFilterTreeModel<T>` are projection wrappers that sort and filter without copying the source. `DataChange` / `TreeChange` notifications drive `Repeater`, `ListView`, `TreeView`, `TableView`, `TreeTable` updates.

The crate is separate from `bastyde-core` because collections are a higher layer than the widget tree — view-models live in the application, hold these models as fields, and bind widgets to them. Qleany integration (generated `EntityListModel` / `EntityTreeModel` typed against entity DTOs) is one supported path; nothing in `bastyde-data` requires it.

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

The `RenderFrame` is the boundary between platform-independent logic (bastyde-core, bastyde-canvas) and GPU-specific code (bastyde-render). It contains five drawable types: `GlyphQuad` (textured from glyph atlas), `ImageQuad` (textured from image), `DecorationRect` (untextured colored rectangle), `ShapeQuad` (SDF-rendered shape), and `RasterizedQuad` (textured from shape atlas). A `draw_order` array records painter's order (back-to-front) for correct occlusion across all drawable types.

### 17.3 GPU Pipeline

Three shader pipelines in bastyde-render: the **quad pipeline** (textured quads for glyphs, images, rasterized paths), the **rect pipeline** (untextured colored quads for decorations), and the **SDF pipeline** (signed distance field shapes with optional gradient fills). A typical frame produces five to six draw calls total.

### 17.4 Atlas Management

Three atlas textures serve different purposes. The **glyph atlas** is owned by the shared Typesetter (from text-typeset), containing rasterized glyph bitmaps. The **shape atlas** stores Tier 3 rasterized path results from tiny-skia. The **image atlas** (or texture array) stores application images. All use LRU eviction — dormant widgets' entries age out naturally.

### 17.5 Dirty Tracking

Each widget has a dirty flag at two granularities: **needs relayout** (size may have changed) and **needs repaint** (appearance changed, size unchanged). Clean widgets replay cached Canvas output without recomputation.

---

## 18. HiDPI and Scaling

Layout works in logical pixels. Rendering works in physical pixels. The conversion happens at the boundary between Phase 4 (paint) and Phase 5 (GPU submission).

`SizeProposal`, widget dimensions, spacing, padding, and font sizes are all logical. The Canvas also works in logical coordinates — `canvas.fill_circle(center, 10.0, color)` draws a circle with a 10-logical-pixel radius regardless of display density.

The scale factor is applied in two places: text-typeset rasterizes glyphs at physical pixel size (logical × scale factor), and bastyde-render multiplies screen coordinates by the scale factor when building vertex buffers.

When the scale factor changes (window dragged to a different monitor), the glyph and shape atlases are invalidated and a full relayout is triggered.

---

## 19. Theming — the four-tier styling ladder

Full references: [`styling-system.md`](styling-system.md) (the four-tier ladder — tokens → variants → recipes → style protocols) and [`reactive-theme.md`](reactive-theme.md) (the `Signal<Theme>` reactive layer).

`Theme` lives in `bastyde-core::styles` (not `bastyde-tokens`) so the per-widget style trait protocols and the typed `Rc<dyn FooStyle>` slot bag can sit on the same struct. It carries a required `appearance: ThemeAppearance` ({Light, Dark} — drives shadow density, OS-theme matching, asset selection), five token groups (`ColorTokens`, `LayoutTokens`, `TypographyTokens`, `ShapeTokens`, `MotionTokens`), `ComponentStyles` (dimension data for the not-yet-themable widgets), `ComponentStyleSlots` (typed style-trait overrides), and a `ThemeExtensions` registry. There is no `Theme::default()` / `Theme::*_default()` — apps pick a preset explicitly (`bastyde_core::presets::intui::{light, dark}`).

Every themable widget composes its chrome through a Tier-3 style trait (`ButtonStyle`, `ToggleStyle`, …) rather than self-painting: the widget builds its parts, hands a `*StyleConfig` to the active style, and uses the returned `WidgetId` as its root child. The style is resolved per-call (`.style(...)`) → theme-wide (`theme.style_slots.<widget>`) → `Recipe*Style` default. `Signal<Theme>` reactivity — `set_theme` updates the signal and dirty-marks every node, no rebuild; focus, scroll, text-input cursor, expanded sections all survive a switch. Role-based widget surface (`TextRole`, `SurfaceRole`, `BorderRole`, `TextStyleRole`) plus `ColorProp` / `TextStyleProp` wrappers; widgets resolve roles against the current theme at paint/layout time. Subtree theme overrides via `set_theme_override(id, |theme| …)`. Themes derive `Serialize` + `Deserialize` for user-loadable theme files (the `style_slots` and `extensions` fields are `#[serde(skip)]`).

---

## 20. Threading Model

### 20.1 Single UI Thread

All five phases of the frame lifecycle run sequentially on the main thread. The widget tree, state arena, overlay manager, Canvas, and all contexts are non-`Send` types — the compiler prevents accidental access from background threads.

This matches Qleany's synchronous model. A Qleany controller call from a Bastyde command handler executes synchronously. No `async`/`await`, no tokio, no runtime.

### 20.2 Background Work

Long operations use Qleany's `LongOperationManager`, which runs use cases on background threads. The background thread communicates with the UI thread through winit's `EventLoopProxy` — a unidirectional channel that wakes the event loop and delivers custom events. The UI thread processes these events like any other input, triggering data source refreshes and widget repaints.

### 20.3 Incremental Work

Operations that take 5–50ms (too short for a background thread, too long for a single frame) are broken into chunks via `request_idle_callback`. The event loop runs idle work during gaps between frames, respecting a time budget.

### 20.4 Event Loop

The winit event loop uses `ControlFlow::Wait` — it sleeps when no events are pending and no widgets are dirty. CPU and GPU consumption is near-zero when the user is not interacting. Full rationale and the four enforcement gates: [`idle-and-animation.md`](idle-and-animation.md).

### 20.5 Animation

Bastyde does not ship a separate animation subsystem. Animation is a thin layer over `Signal<f32>`: `signal.animate_to(target, duration, easing)` asks the tree's `AnimationScheduler` to smoothly interpolate the value over time, and any widget bound to the signal re-paints on each tick as the value slides. The scheduler integrates with the frame lifecycle (pause when the window is occluded, rebase on resume, skip offscreen ticks, cancel animations on widget rebuild/destroy), so widgets never own animation lifetime manually.

The design intent is narrow: motion is reserved for a small set of floating transitions — dialog appearance, snackbar slide-in, accordion expansion, toggle thumb motion, indeterminate progress, smooth programmatic scroll. Hover, press, and focus state changes are explicitly *instant* in Int UI's vocabulary; they are expressed as `Signal<Role>` mapped from an interaction signal and resolved per-frame through the theme, not through the animation scheduler. Looping animations respect `ctx.prefers_reduced_motion()`.

Full rationale, API, worked examples, and testing patterns: [`animation.md`](animation.md).

---

## 21. Accessibility

Full reference: [`accessibility-overrides.md`](accessibility-overrides.md). AccessKit is integrated at the `Widget` trait level — every widget's `accessibility(builder)` declares role, name, state, and available actions. AT actions flow through the same dispatch as pointer/keyboard input via `WidgetEvent::AccessAction`. Builder-level `.access_*` modifiers (`access_label`, `access_description`, `access_hidden`, `access_role`, `access_disabled`, `access_controls` / `described_by` / `labelled_by`, `access_live`, `access_shortcut_id` / `access_shortcut_literal`, `access_action` / `access_remove_action` / `access_custom_action`, `access_exclude_subtree` / `access_merge_subtree`, `access_customize`) let app authors augment, replace, or annotate any widget's accessibility info from the outside.

Dormant subtrees produce no AccessKit nodes (screen readers only see active content). Overlay content generates correct AccessKit tree structures — tab lists have `Role::TabList` and `Role::Tab` nodes, menus have `Role::Menu` and `Role::MenuItem` nodes, tooltips are linked to their anchor widget via `DescribedBy`. Scene-content a11y customization (off-screen modes, logical groups) lives in [`bastyde-scene-a11y.md`](bastyde-scene-a11y.md).

---

## 22. Window Management

Full reference: [`multi-window.md`](multi-window.md). Each window owns its own independent `WidgetTree`, layout pass, paint pass, `RenderFrame`, and wgpu surface. Application-level context (theme, locale, `ShortcutRegistry`, data-model handles, app-scoped backend wiring) is shared across windows. `WindowConfig` is the single creation entry point for both initial and runtime-opened windows; `WindowState` is the per-window cloneable signal handle (placement, title, size, position, focused, resizable, always_on_top). Two-way OS↔state sync uses an `applying_from_os` re-entrancy guard to prevent observer→OS→observer loops.

Custom window chrome (drag region, resize strip, window controls, per-OS title bar host backends): [`title-bar.md`](title-bar.md).

---

## 23. Settings and Persistence

Full reference: [`settings.md`](settings.md). In-memory is the source of truth — `Signal<T>` and `*Model<T>` handles drive both UI and disk; the disk side is a debounced atomic projection (write-temp + rename, single shared I/O thread per process). Three persistence shapes: `SettingsStore` (dotted-key K/V for scalars), `SettingsFile<T>` (typed single-struct with `Versioned` + `Migrator<T>` migrations on raw `toml::Value`), and `PersistedListModel<T>` / `PersistedTreeModel<T>` (bridges from `ListModel<T>` / `TreeModel<T>`). Built-in services: `MruList<T: MruEntry>` for generic dedupe + pin + LRU-cap recents; `WindowStateService` with framework-driven auto-save/restore for any `WindowConfig` carrying `id(...)`. Saved geometry is sanitized on restore against the current monitor's work area. Wayland ignores window position by protocol design (size and `WindowPlacement` round-trip).

---

## 24. Testability

### 24.1 Headless by Design

The widget tree runs without a window, without GPU, and without winit. All five phases (minus GPU submission) execute in pure Rust with no platform dependencies. Tests use bastyde-core's `WidgetTree` directly:

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

### 24.2 What Is Testable

Layout (given a widget tree, do children end up at the right positions), event dispatch (does the right widget receive events, does focus cycle correctly), state transitions (hover/pressed/disabled), accessibility (correct AccessKit role, name, actions), render output (expected quads/shapes in the RenderFrame), theming (palette swap produces correct colors), gesture recognition (pure state machine tests), overlay behavior (tooltip timing via simulated clock), drag-and-drop (payload transfer, insertion indicator rendering), and composition (multiple widgets interacting correctly).

### 24.3 Mock Backend

Cargo feature flags (`mock-backend`) swap Qleany controller implementations with mock modules providing static data. Same API surface, zero backend. Familiar to the developer from Qleany's C++/Qt mock system for QtQuick.

### 24.4 CI Friendly

No `Xvfb`, no GPU, no display server required. Pure logic tests run in `cargo test` in milliseconds. The simulated clock (`tree.advance_time()`) enables deterministic testing of time-dependent behavior.

---

## 25. Crate Structure

Full per-crate descriptions live in CLAUDE.md "Crate Architecture". The dependency graph is the part that belongs here.

### 25.1 Dependency Graph

```text
bastyde-tokens
    ↑
bastyde-canvas ← tiny-skia
    ↑           ↑
bastyde-core    bastyde-text ← text-typeset
    ↑ ← accesskit
    │
    ├── bastyde-data
    │       ↑
    │   bastyde-settings ← serde, toml, directories, tempfile
    │       ↑
    │   bastyde-telemetry ← uuid
    │
    ├── bastyde-widgets
    │   └── [rich-text] ← text-document, text-typeset
    │
    │   bastyde-i18n ← fluent-rs, icu_decimal, icu_datetime, icu_calendar
    │
bastyde-render ← wgpu
    ↑
bastyde-platform ← winit, accesskit-winit
    ↑
bastyde-app (wires bastyde-text into Canvas, bastyde-widgets, bastyde-i18n,
          bastyde-settings — auto-restores/saves window geometry,
          optionally bastyde-text)
    ↑
bastyde (umbrella, re-exports)
```

`bastyde-text` depends only on `bastyde-canvas` (for the `TextBackend` trait) and `text-typeset`. It does not depend on `bastyde-core`, `text-document`, or any platform crate. The `TextBackend` trait is defined in `bastyde-canvas` so that the Canvas can call text rendering methods without knowing which backend implementation is active.

The RichTextEditor widget (in `bastyde-widgets` behind the `rich-text` feature) depends directly on `text-document` and `text-typeset`. The application owns the `TextDocument` instance and passes it to the widget — Bastyde never owns or wraps the document model. The application depends on `text-document` directly for model access (highlighter, cursors, import/export). Cargo deduplicates the shared dependency automatically.

Platform-specific code (winit, wgpu, accesskit-winit) is confined to `bastyde-render` and `bastyde-platform`. Everything above them is platform-independent and headlessly testable.

### 25.2 The bastyde Umbrella

The standard application developer depends on a single crate: `bastyde`. It re-exports the public API and controls feature flags. `text`, `i18n`, and `rich-text` are default features (opt-out, not opt-in), because the kinds of applications Bastyde targets — writing tools, editors, IDEs, content managers, long-running desktop apps — routinely need text rendering, translations, and rich text editing. `TextInput` itself derives from the rich-text widget, so anything with an editable text field pulls in `rich-text` anyway. Sub-crates remain independently publishable for advanced users (custom widget authors, custom renderer implementors).

---

## 26. Button — Reference Widget Design

The button serves as the reference implementation exercising most architectural features: composition of primitives, interaction state as a `Signal<InteractionState>`, role-based color resolution per visual state, attached handler activation from multiple input paths, AccessKit role and actions. A new widget author implementing their first custom widget should read [`crates/bastyde-widgets/src/button.rs`](../crates/bastyde-widgets/src/button.rs) — it's the authoritative exemplar, and concrete code is more useful than prose at this point. See also [`reactive-theme.md`](reactive-theme.md) for the `Signal<Role>` pattern Button uses for its visual states.

What Button exercises:

- **Composition.** A `RectWidget` (background, border, corner radius) wrapping an internal `HStack` or `VStack` (by `IconPosition`) containing an optional `IconWidget` and a `TextWidget` label. Leading/Trailing positions respect locale `LayoutDirection`.
- **Visual states.** Five (idle, hovered, pressed, focused, disabled) × seven variants (`Filled`, `Tinted`, `Outlined`, `Plain`, `Ghost`, `Link`, `Destructive`) → (background role, border role, text role) resolved at paint time via `Signal<InteractionState>` mapped to `Signal<Role>`.
- **Behavior.** Pointer enter/leave/down/up drives interaction state; keyboard Space/Enter triggers activation; cursor is `Pointer` on hover; `TapRecognizer` commits the click.
- **Accessibility.** `Role::Button`, name from label (resolved via `tr!` / `tr_widget!`), disabled state, actions (`Click`, `Focus`). Focus ring painted only on keyboard focus (origin-aware).

---

## 27. Architectural Comparisons

### 27.1 vs. QPalette → Design Tokens

QPalette covers color roles across three interaction groups. Bastyde's design token system extends that scope to spacing, typography, and shape, uses typed Rust structs, and supports subtree overrides through environment propagation.

### 27.2 vs. QAbstractItemModel → `ListModel<T>` and `TreeModel<T>`

Qt's `QAbstractItemModel` uses a role-based, type-erased data access protocol (`QVariant`). Bastyde's `ListModel<T>` and `TreeModel<T>` are concrete generic types: the delegate closure receives `&T` directly, with compile-time type safety. The `ListDataSource` trait provides an escape hatch for large/external datasets, also with an associated `Item` type.

### 27.3 vs. Existing Rust GUI Frameworks

Bastyde's focus areas are accessibility (AccessKit at the trait level, tested by every test), text rendering (text-document + text-typeset), and widget extensibility (unified Widget trait with slots). Its layout and event design are comparable to Xilem/Masonry. It is currently weaker on rendering sophistication (quad-based vs. Vello's GPU compute renderer) and much younger than established frameworks.

The primary reference point for Bastyde's feature scope is Qt Widgets — the framework most commonly used for the kind of professional desktop applications Bastyde targets.

---

## 28. Widget Catalog

The current widget inventory is no longer maintained as prose in this document — it drifted faster than it could be edited. The authoritative sources are:

- **`tools/extract_widget_api.py --all`** — emits the public surface (struct, builder methods, enums, module doc) of every widget in `bastyde-widgets`. Run `python3 tools/extract_widget_api.py --list` to see the full file list, or pass widget names to extract just those.
- **[`bastyde-milestones.md`](bastyde-milestones.md)** — the "Current State: What Exists" section enumerates every widget currently shipped, grouped by category, and tracks remaining milestone work.
- **CLAUDE.md** — the "Implementation Status" block and the per-widget reference docs ([`table-view.md`](table-view.md), [`tab-widget.md`](tab-widget.md), [`charts.md`](charts.md), [`tooltips.md`](tooltips.md), [`bastyde-scene.md`](bastyde-scene.md)) cover the widgets with the deepest API surface.

For a one-shot dump suitable for downstream tooling: `python3 tools/extract_widget_api.py --all -f json -o widgets.json`.

---

## 29. V2 Widget Authoring Model

The unified `Widget` trait, `Signal<T>` reactivity, attached handlers, `BuildContext::signal` / `effect` / `animated_signal` / `app_state` / `subscribe_event`, the four widget shapes (leaf / container / composing / hybrid), and the `take_widget` / `restore_widget` arena extraction pattern that makes `build(&mut self)` borrow-safe — all documented in CLAUDE.md "Unified Widget Trait" plus the focused docs ([`events-and-gestures.md`](events-and-gestures.md), [`reactive-theme.md`](reactive-theme.md), [`animation.md`](animation.md)). The V2 model is what the entire widget library is written against; reading [`crates/bastyde-widgets/src/button.rs`](../crates/bastyde-widgets/src/button.rs) is the fastest way to see all of it together in one ~200-line widget.

The `bati!` DSL desugars to V2 builder calls one-to-one at macro-expansion time — no runtime, no virtual tree. References: [`bati-macro-reference.md`](bati-macro-reference.md) (user-facing) and [`bati-language-spec-v3.md`](bati-language-spec-v3.md) (grammar and desugaring spec).

---

## 30. Open Questions (Current, May 2026)

The bulk of the original post-milestone question list has landed. The short list below is what remains actively open; see [`bastyde-milestones.md`](bastyde-milestones.md) for detailed status and the Next-candidates roadmap.

**External (OS) drag-and-drop.** Intra-app DnD works everywhere (Milestone 6). **Inbound** OS drops — files / text / URLs dragged from a file manager or another app into a Bastyde window — are implemented through the `ExternalDndBackend` trait in `bastyde-platform` (`install_external_dnd()`): macOS via a `NSDraggingDestination` overlay view (verified), Windows via OLE `RegisterDragDrop`/`IDropTarget`, Wayland via `wl_data_device`, X11 a documented no-op (the `DropZone` Browse button covers it). They reuse the in-app pipeline — an OS drop is a `DragPayload` with `origin() == External`. winit's own `DroppedFile`/`HoveredFile` are not used (no position, files-only, no Wayland). **Outbound** drags (Bastyde window → another app, e.g. `NSDraggingSource`) are still pending; the payload type and handler API are stable.

**Native menu bar on macOS.** The widget-based `MenuBar` (Milestone 4) is correct for Windows and Linux where menu bars live inside the window chrome. On macOS the OS expects menus to live in the global `NSMenu`. The remaining work is a platform abstraction that routes a single declarative menu description through either path.

**Virtualized dropdowns.** `ComboBox` now virtualizes via `ListView` under `max_visible_items`: lists beyond the cap materialize only the visible rows (plus `ListView`'s small buffer) instead of building every `DropdownItem` eagerly. The searchable (`rich-text`) filtered path shares the same virtualized renderer. `MenuList` grew a `max_visible_items` builder that caps panel height and wraps the item column in a `ScrollArea`, but does **not** virtualize — its API still takes arbitrary `impl Widget` children, so true virtualization would require a model-driven MenuList rewrite (tracked as follow-up). The eager build is cheap enough that capped 100+ item menus are fine in practice.

---

## 31. First Milestone: Button in a Window

Status of every milestone (M1 through current) lives in [`bastyde-milestones.md`](bastyde-milestones.md). M1 — a window displaying a single themed button with click handling, hover/press states, text rendering, AccessKit accessibility, and keyboard activation — landed as the `simple_button` example. It exercised the full vertical slice (bastyde-tokens for theme, bastyde-canvas for the SDF rounded rect, bastyde-core for arena/layout/events/focus/a11y, bastyde-text for the label, bastyde-render for the wgpu pipeline, bastyde-platform for the winit window + AccessKit adapter, bastyde-app for the event loop) and proved the end-to-end stack before any further widget work.
