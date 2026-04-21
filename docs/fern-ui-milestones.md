# FernUI Milestones

**Companion to:** fern-ui-architecture.md, fern-ui-code-examples.md  
**Date:** April 14, 2026  
**Status:** Living document — reflects actual codebase state and remaining work

---

## Guiding Principles

Each milestone produces a demonstrable application that exercises progressively more of the architecture. Milestones are ordered by dependency — each one builds on capabilities validated by the previous one. Every milestone includes headless tests verifiable via `cargo test` with no display server.

---

## Current State: What Exists

The following capabilities are implemented and tested in the codebase.

**Crate infrastructure.** All crates compile and are wired through the `fern-ui` umbrella crate: fern-tokens, fern-canvas, fern-core, fern-text, fern-render, fern-platform, fern-app, fern-widgets, plus the data and i18n crates added during Milestones 6 and 7.

- **fern-data** — the reactive data-model crate. `ListModel<T>`, `TreeModel<T>`, `TreeSlice<T>`, `ListDataSource` trait, `SelectionModel`, `DataChange` / `TreeChange` notifications. Not part of fern-core because collections are a higher layer than the widget tree.
- **fern-i18n** — the i18n runtime. ~1500 lines. `LocalizedString`, `localized()`, `I18nConfig`, `I18nManager`, `LayoutDirection`, file watcher, locale resolution, `resolve_message` / `resolve_message_widget` runtime entry points. Re-exports `tr!` / `tr_widget!` from fern-i18n-macros.
- **fern-i18n-macros** — the compile-time-validating proc macro crate. ~800 lines. `tr!` and `tr_widget!` read the consuming crate's source `.ftl` file at expansion time, parse it via `fluent-syntax`, validate every call against the parsed key map, and emit runtime resolution code with a compile-time fallback for missing bundles.

**V2 widget authoring model (Section 28 of architecture).** The V1 Widget/CompositeWidget split is gone. One unified `Widget` trait with six methods: `build(&mut self)`, `size_that_fits`, `place_children`, `paint`, `accessibility`, `children`. The `build(&mut self)` lifecycle uses take_widget/restore_widget extraction from the arena for borrow safety. `Signal<T>` is the only reactive primitive — the old `State<T>` / `DerivedState<T>` / `Reactive<T>` types are removed, and the `binding.rs` module (renamed from `state.rs` in commit 851fff2) now contains only the dirty-tracking infrastructure (`Binding`, `BindingRegistry`, `BindingLevel`) shared between signals and the widget tree. `Prop<T>` replaces `Reactive<T>` for widget properties. `ObserverHandle` provides RAII cleanup for observers. Attached event handlers (`on_tap`, `on_hover`, `on_key`, `on_focus`, `on_access_action`, `on_pointer_event`, `on_scroll`, `on_drag`, `on_drag_hover`, `on_drop`) replace the monolithic `event()` method. `HandlerSet` with `ctx.apply_self_handlers()` for handler attachment during `build()`. `BuildContext` provides `ctx.signal()`, `ctx.effect()`, `ctx.animated_signal()`, `ctx.self_id()`, `ctx.apply_self_handlers()`, `ctx.app_state::<T>()`, `ctx.subscribe_event()`. The framework auto-wires gesture recognizers when handlers are attached.

**Application state and backend events.** `FernAppBuilder::app_state<T>(value)` registers a process-wide singleton keyed by `TypeId`, retrieved by widgets via `BuildContext::app_state::<T>() -> Option<&T>` (commit 7969424). The canonical pattern is a single `Rc<AppGlobals>` struct holding Signal fields rather than one state per type. The `EventSource` trait (`fern-core/src/event_source.rs`, commit 86ba93b) lets widgets subscribe to external event sources (backend message buses, database change notifiers, file watchers) directly from `build()` via `BuildContext::subscribe_event`, with cross-thread forwarding through the framework's event-loop proxy. Subscriptions have per-widget lifetime cleanup — when the widget is destroyed, the handle drops and the source unsubscribes.

**Tokens and theming.** Theme struct with ColorTokens, SpacingTokens, TypographyTokens, ShapeTokens, MotionTokens. Light and dark defaults. FernTech-branded color tokens (commit 472d771). Subtree theme overrides with environment propagation. Runtime theme switching via `EventContext::set_theme()` from any handler, triggering composite rebuild across all windows. Easing curves (Linear, EaseIn, EaseOut, EaseInOut) with `lerp()`.

**Canvas and rendering.** Three-tier Canvas API: axis-aligned rects (DecorationRect), SDF shapes (ShapeQuad with rounded rect, circle, ellipse), and CPU-rasterized paths (PathAtlas with tiny-skia, LRU eviction). Image rendering pipeline (Canvas::draw_image, ImageManager in fern-render). Text rendering via text-typeset integration (SharedTypesetter, layout_single_line). Gradient support (linear, radial). Scissor rect stack for viewport clipping (SetClip/ClearClip with intersection). wgpu rendering pipeline with glyph atlas, shape atlas, and per-frame draw command dispatch. Feature-flagged variable fonts for Arabic and Hebrew (Noto Sans Arabic, Noto Sans Hebrew) plus Inter as the Latin-script default, added in commits 1210c82 and 342f948 alongside the i18n work.

**Widget arena and layout.** SlotMap-based arena with parent-child relationships. SwiftUI-style propose/respond/place layout negotiation. Dirty flags (needs_layout, needs_paint) with ancestor propagation for relayout. Alignment system (HAlignment, VAlignment, Alignment) with per-child overrides. Layout direction (LTR/RTL) exposed to widgets via `BuildContext::layout_direction()`. The `widget_tree.rs` module is split into eight implementation files for maintainability: `accessibility_impl`, `event_dispatch_impl`, `focus_impl`, `layout_impl`, `overlay_impl`, `query_impl`, `rendering_impl`, and `test_api`. This is an internal refactor with no API change.

**Primitive widgets.** RectWidget, TextWidget, HStack (with spacing, cross-axis alignment, spacer-aware distribution), VStack, ZStack, Padding, Spacer, Center, Expand, FixedSize, MinSize, MaxSize, Divider, IconWidget (vector icons via Path/PathAtlas), Grid, Wrap/FlowLayout, AspectRatio, Switcher, FocusRing.

**Widget composition (V2).** Unified Widget trait with `build(&mut self)` for child construction. BuildContext provides `ctx.add()`, `ctx.signal()`, `ctx.effect()`, `ctx.animated_signal()`, `ctx.self_id()`, `ctx.apply_self_handlers()`, `ctx.app_state::<T>()`, `ctx.subscribe_event()`. Inline child resolution via `child()`, `children()`, `child_opt()` on containers. `visible_when` and `enabled_when` as builder methods resolved during `add()`.

**Reactive state.** `Signal<T>` is the single reactive primitive — `Signal::new()`, `signal.get()`, `signal.set()`, `signal.map()`, `signal.observe()` returning `ObserverHandle` for RAII cleanup. `WeakSignal` for breaking reference cycles in cross-signal observers (used by `LocalizedString::to_signal()` to avoid the C1 memory leak — see §12.3 of the architecture doc). `attach_keepalive` to tie an observer handle's lifetime to a signal. `Prop<T>` replaces `Reactive<T>` for widget properties, accepting both plain values and signals via `impl Into<Prop<T>>`. `ctx.effect()` for scoped side effects cleaned up on rebuild.

**Event system (V2).** Pointer events (move, down, up, enter, leave) with modifier keys, keyboard events (down, up), scroll events (lines, pixels), IME events (composition, commit), AccessKit action routing. Preview pass (root → target) and bubble pass (target → root). Hit testing against layout bounds. Gesture recognizers: TapRecognizer, DoubleTapRecognizer, DragRecognizer (added for SplitView and reused by ListView/TreeView drag sources). GestureArena for competition, auto-wired from attached handlers. Attached event handlers stored on arena nodes, dispatched by the framework. `HandlerSet` for handler construction during `build()`. Deferred tree mutations via EventContext (dormancy, activation, destruction). Additional EventContext operations for overlay-based widgets: `request_focus(id)`, `dismiss_all_overlays()`, `cancel_delayed_overlay(id)`, `synthetic_click(id)`.

**Focus management.** Tab/Shift-Tab cycling in document order. Focus origin tracking (Keyboard, Pointer, Programmatic). Focus-visible behavior. Scroll-into-view on focus change (dispatches ScrollIntoView to nearest clipping ancestor). FocusGained/FocusLost events. Programmatic focus transfer via `ctx.request_focus()` for menu opening, dialog content, and modal presentation. `first_focusable_descendant` method for initial focus on modal open (commit 029a6cd).

**Overlay system.** OverlayManager with stacking, cascade dismissal, Escape handling, click-outside detection. OverlayPlacement (Below, Above, BelowPreferred, TrailingEdge, AtPointer, NearAnchor, BottomCenter). BelowPreferred flips to Above when insufficient space below (used by ComboBox and MenuBar). DismissBehavior (ClickOutside, PointerLeave, Manual). OverlayLayer (InTree, NativePopup, Auto). In-tree overlay layout and positioning. Delayed overlay opening (for submenu hover delays) with `cancel_delayed_overlay` support. Tooltip attachment with configurable delay and simulated clock for deterministic tests. Auto-dismiss functionality with configurable duration (commit bc4cbad).

**Modal system.** Framework-level modal presentation (`fern-core/src/modal.rs`, commit 10a5225). `ModalPresentation` enum (Auto/InTree/NativeWindow), `ModalCloseBehavior` enum (ClickOutside, EscapeKey, EscapeOrClickOutside, Manual), `ModalContent` (existing widget reference or deferred builder closure), `ModalRequest` with title and close behavior. Native modal windows route through WindowManager's modal support (commit 36312a2) with focus management. ModalContainer widget in fern-widgets (commit 3e9592f) presents content via either in-tree overlay or a separate native window depending on the runtime and the `ModalPresentation::Auto` resolution. Modal dismissal handling (commit a54b3a8) returns focus to the parent window.

**Shortcut / Intent / Action system.** `Shortcut` (rebindable keystroke → intent name), `Intent` (runtime DTO with optional typed payload), `Action` (widget-owned handler keyed by intent name), `ShortcutRegistry` (two-layer: declared defaults + persisted user overrides with graveyard semantics). Source → root intent dispatch via `ctx.send_intent(...)` or registry invocation. Global shortcuts fire with a root-anchor fallback when no widget is focused. `#[derive(IntentKind)]` with `#[name = "..."]` attributes provides the typed DTO bridge (unit, tuple, and struct variants all supported — whole variant is the payload). `ShortcutSettings` widget packages the full rebind UI (Rebind / Reset / conflict auto-unbind / key-capture). `CaptureHandle` RAII for key capture. `MenuItem::for_shortcut("id")` and `TooltipContent::for_shortcut("id")` re-render on rebinds via `ShortcutRegistry::version()`. Replaces the old `AppCommand` + `FernAppBuilder::on_command` pipeline. Current `Display` for `KeyStroke` is `Ctrl+S` regardless of platform and locale — locale/platform-aware formatter remains a TODO. Reference: [shortcut-intent-action.md](shortcut-intent-action.md), demo: `examples/shortcuts_demo`.

**Scrolling.** clips_children flag on WidgetNode. ScrollArea widget with unbounded proposals, offset placement, scroll event handling via attached `on_scroll` handler, ScrollIntoView handling, AccessKit scroll properties. ScrollBar as a standalone widget with thumb drag, track click, keyboard adjustment. `ScrollBarStyle::Overlay` (default, Ubuntu-style) shows a thin resting indicator that expands to a full interactive scroll bar as an overlay on hover — viewport width unchanged. `ScrollBarStyle::Permanent` makes the ScrollBar a layout sibling of the content, reducing the viewport by the scroll bar's thickness. Scissor rect implementation in renderer with clip stack intersection.

**Animation.** AnimationScheduler driving `Signal<f32>` values with duration and easing. Supports cancel, replace, zero-duration immediate set. Multiple independent animations. `Signal<f32>::animate_to()` is the sole animation entry point — the dual-track V1/V2 support described in earlier revisions of this doc is gone (commit 54ca939). Toggle, Accordion, ScrollArea, ProgressBar, Snackbar all animate through Signal. Integrated into the widget tree's frame lifecycle.

**Dormancy.** Three activation states (Active, Dormant, Destroyed). Recursive dormancy/activation. visible_when binding for state-driven toggling. ComboBox and MenuBar pre-create their dropdown content as dormant subtrees, activating them via `ctx.activate()` when the menu opens.

**Window management.** WindowManager in fern-app with per-window WidgetTree. Window creation/closure via `EventContext` from any handler (queued operations). Theme propagation to all windows. Event routing by winit window ID. FernWindowId abstraction. Modal window support with focus management (commit 36312a2). Primary window ID retrieval (commit 738aff5).

**Custom title bar and window chrome.** The `fern-widgets/src/title_bar/` directory contains the custom title bar implementation: `controls.rs` (minimize/maximize/close buttons), `drag_region.rs` (window move via title bar click-and-drag), `resize_strip.rs` (edge-hit-testable resize areas), `window_frame.rs` (the overall frame composition). WindowFrame is now rendered as an overlay set of rects for resize handles rather than as a layout-participating widget (commit db8a609). Wayland gets a no-window-frame mode because it handles decorations server-side (commit c855478). Planning and scope for the custom title bar work is documented in `docs/title-bar-plan.md`.

**Accessibility.** AccessKit integration at the Widget trait level. AccessNodeBuilder with role, name, actions, disabled, bounds. sync_accessibility() generating TreeUpdate. AccessAction routing to target widgets. Focus tracking in TreeUpdate. Accessibility wrappers for list/tree items (commit 63facc3).

**Higher-level widgets.** Button, Panel, Card, Toolbar, StatusBar, Checkbox (two-state and tristate), RadioButton, Toggle/Switch, Slider (horizontal/vertical, stepped), SegmentedControl, ProgressBar (determinate and indeterminate), Badge, Accordion, Link, ScrollArea (overlay and permanent modes), ScrollBar, Tooltip, ComboBox (dropdown with keyboard navigation and type-ahead), MenuItem, MenuList, MenuBar, MenuSeparator, ContextMenu, TabWidget, SplitView with SplitHandle, Dialog with DialogContent and ModalContainer, Popover, Snackbar (queued auto-dismissing with animated slide-in), Breadcrumb (clickable path segments with chevron separators), Wizard (multi-step flow with header, footer, and step switching), Repeater, ListView, TreeView, TitleBar with custom controls and drag region.

**Examples.** `simple_button` (M1), `text_and_layout` (M2), `widget_catalog` (M3), `menus_and_dropdowns` (M4), `split_view` (M5), `tab_widget` (M5), `dialogs_and_popovers` (M5), `data_collections` (M6), `internationalization` (M7), `rich_text_viewer` (M8a read-only preset), `rich_text_editor` (M8b editable preset + mixed editable/read-only on the same document), `spin_box` (M9 SpinBox with generic SpinValue trait, wrap modes, step types), `title_bar_demo` (title bar subsystem), `shortcuts_demo` (Shortcut / Intent / Action system with typed `IntentKind` payloads).

**Divergence note.** The i18n implementation diverged from the architecture doc in one substantive way: framework bundle registration for fern-widgets is **explicit**, not automatic. Applications that use fern-widgets must call `.framework_locales(fern_widgets::framework_locales())` on the `I18nConfig` builder chain. See architecture §12.13.3 for the rationale (fern-app is deliberately widget-agnostic) and the forgiving fallback path: apps that forget to register still see correct English accessibility labels via the proc macro's compile-time fallback.

---

## Milestone 1: Button in a Window ✅

Completed. A window displaying a single themed button with click handling, hover/press states, text rendering, AccessKit accessibility, and keyboard activation. The `simple_button` example demonstrates this with comprehensive tests.

---

## Milestone 2: Text and Layout ✅

Completed. A window with multiple widgets in nested layouts (HStack-in-VStack, Spacer distribution), text rendering with multiple typography styles, and runtime theme switching (light/dark). The `text_and_layout` example demonstrates this with tests for layout correctness, theme swapping, and composite rebuild.

---

## Milestone 3: Core Widget Catalog ✅

Completed. All Milestone 3 widgets are implemented and tested. The milestone also included the V2 widget authoring model migration (architecture Section 28), which was driven by three problems identified during implementation: the Widget/CompositeWidget split forced wrong decisions, the `RefCell<Option<State>>` pattern was required by every stateful composite, and the four reactivity types confused widget authors.

**V2 migration is now complete.** The V1 state types and the compatibility layer from earlier phases are fully removed:

- `composite_widget.rs` and `composite_adapter.rs` deleted. No CompositeWidget references remain.
- All widget files use `impl Widget for` with the unified trait.
- No `RefCell<Option<State<T>>>` pattern remains anywhere.
- Every interactive widget uses `Signal<T>`: Button, Toggle, Checkbox, RadioButton, Slider, Accordion, Badge, Card, Link, SegmentedControl, ScrollArea, ScrollBar, ProgressBar.
- ScrollArea fully migrated to `Signal<f32>` for all scroll state fields.
- Toggle fully Signal-ified (no `Rc<Cell<>>` remaining for interaction state).
- ProgressBar uses `Prop<T>` for fill/track colors and `Signal<f32>` for indeterminate animation.
- AnimationScheduler exclusively uses `Signal<f32>::animate_to()` (commit 54ca939).
- `state.rs` renamed to `binding.rs` (commit 851fff2). The module no longer contains any state primitive — it holds only the dirty-tracking infrastructure (`Binding`, `BindingRegistry`, `BindingLevel`) shared between `Signal<T>` and `WidgetTree`.
- All container primitives implement `build()` for PendingChild resolution.
- `ctx.state()` / `ctx.observe()` removed from BuildContext.

**Remaining low-level state.** ScrollBar uses `Rc<Cell<>>` for drag interaction state (legitimate low-level use; same pattern Slider uses). Slider uses the same pattern. These are not V1 remnants — they are the normal way to store widget-local interaction state that does not need to be observable from outside the widget.

The `widget_catalog` example (766 lines) exercises every Milestone 3 widget with theme switching and comprehensive tests.

---

## Milestone 4: ScrollBar, ComboBox, Menus ✅

Completed. Overlay-based interactive widgets and the ScrollArea overlay/permanent mode refactor from architecture §3.7. The `menus_and_dropdowns` example demonstrates the full set with File/Edit/Format menus, ComboBox, and context menu.

**Delivered:**
- **ScrollArea overlay mode (default).** Thin resting indicator (4px) expands to full interactive ScrollBar as an overlay on hover. Viewport width unchanged. `ScrollBar::overlay_mode(true)` with configurable `resting_thickness`.
- **ScrollArea permanent mode.** ScrollBar is a layout sibling of the content, reducing the viewport by the scroll bar's thickness. Selected via `ScrollBarStyle::Permanent`.
- **ComboBox** (~900 lines). Non-generic, index-based selection via `Signal<Option<usize>>`. Dropdown content pre-created as dormant subtree, activated on open. Keyboard navigation (Arrow Up/Down, Enter, Escape), type-ahead filtering, `BelowPreferred` placement.
- **MenuItem** (~900 lines). Non-generic, closure-based activation (`on_activate_fn(|ctx| ctx.send_intent(AppIntent::X))`). Supports icons, shortcut labels, disabled state, submenu triggers. Automatic shortcut label lookup from `ShortcutRegistry` via `MenuItem::for_shortcut("id")` — labels re-render on rebind through the registry's `version()` signal. Submenu opens on hover delay (timing tuned in commit c4adfa7) with diagonal movement tolerance across other menu items.
- **MenuList** (~460 lines). Vertical container for MenuItem and MenuSeparator. KeyboardHighlightWrapper for focus visualization. Arrow Up/Down navigation.
- **MenuBar** (~1,100 lines). Horizontal bar with dropdown menus. Trailing slot for additional actions. MenuContext coordinates open index, trigger focus, cross-menu Left/Right navigation.
- **MenuSeparator**. Themed 1px horizontal line with padding.
- **ContextMenu.** Right-click opens a MenuList overlay at pointer position via `OverlayPlacement::AtPointer`.
- **Widget-tree refactor.** `widget_tree.rs` split into a main file plus eight implementation modules (`accessibility_impl`, `event_dispatch_impl`, `focus_impl`, `layout_impl`, `overlay_impl`, `query_impl`, `rendering_impl`, `test_api`). Internal refactor only — no API change.

---

## Milestone 5: Tabs, SplitView, Dialogs, Modals ✅

Completed. Application structure widgets — tabbed interfaces, resizable panes, modal and modeless dialogs, popovers, snackbars, breadcrumbs, and the multi-step wizard flow. Three examples cover the milestone: `split_view`, `tab_widget`, and `dialogs_and_popovers`.

**Delivered:**

- **TabWidget** (`tab_widget.rs`). HStack of tab headers above a Switcher for content panes. `Signal<usize>` for selected index. Delegates switching to the Switcher primitive rather than reimplementing the logic. Trailing slot for tab-level actions. Tab header layout fixed in commit e61eee4 (prevented a subtle upward drift). Hidden-tab-bar bug fixed in dc7a3eb.
- **SplitView and SplitHandle** (`split_view.rs`). Draggable divider with `Signal<f32>` split position. Uses `DragRecognizer` from the gesture system. SplitView refactor in commit 299a9ec introduced the SplitHandle as a separate widget with its own properties (cursor, hit region, minimum pane sizes) and improved layout handling. CursorIcon integration for col/row resize feedback. Keyboard adjustment on focused divider.
- **DragRecognizer** (`fern-core/src/gesture.rs:394`). Added to the gesture system for SplitView and reused by ListView/TreeView as the drag-source trigger.
- **Dialog, DialogContent, ModalContainer** (`dialog.rs`). Modal panel with title, content area, action bar. Focus trapping and focus restoration on dismiss. Escape dismissal. Works through either in-tree overlay or native modal window depending on `ModalPresentation`.
- **Modal system** (`fern-core/src/modal.rs`, commits 10a5225, 3e9592f, a54b3a8). `ModalPresentation` enum (Auto/InTree/NativeWindow), `ModalCloseBehavior` (ClickOutside, EscapeKey, EscapeOrClickOutside, Manual), `ModalContent` (existing widget reference or deferred builder closure), `ModalRequest`. `Auto` presentation picks the best backend at runtime — native window where supported, in-tree overlay otherwise. Modal focus management (commit 029a6cd) with `first_focusable_descendant` for initial focus on open. Modal dismissal returns focus to the parent.
- **Native modal windows** (commit 8022888, commit 36312a2). WindowManager supports modal window behavior with focus management. The OverlayDemo example includes a native-modal-window fallback path.
- **Popover** (`popover.rs`). Interactive overlay anchored to a trigger. Arbitrary content, focus on show, ClickOutside dismissal, typed `PopoverSurface` for the content container.
- **Snackbar** (`snackbar.rs`, commit 75b8c65). Auto-dismissing notification with configurable trigger. Animated slide-in/fade-out via `Signal<f32>::animate_to()`. Uses `OverlayPlacement::BottomCenter` (added for this widget in commit bc4cbad). Custom-trigger variant for non-automatic dismissal.
- **Breadcrumb** (`breadcrumb.rs`). HStack of clickable `BreadcrumbItem`s separated by `BreadcrumbSeparator` chevron icons. `current` flag for the last segment with distinct styling.
- **Wizard** (`wizard.rs`, commit 022ac03). Multi-step flow with `WizardStep`, `WizardHeader`, `WizardFooter`, `WizardFlow`. Modal handling, back/next navigation, step activation and dormancy. Used when an application needs a guided sequence of steps rather than a freely-navigable form.
- **Overlay enhancements.** Auto-dismiss functionality with configurable duration (commit c3c62cb and bc4cbad). New overlay methods for retrieval and topmost-centered-overlay detection (commit ac63d97). Improved dismissal behavior (commit 738aff5).

**Still deferred.** LongPressRecognizer is not implemented. Most drag interactions use press-and-hold thresholds inside DragRecognizer, so LongPressRecognizer is not blocking any milestone — it would be an ergonomic improvement for widgets that want a distinct long-press gesture (context menu on touch, for example).

---

## Milestone 6: Data-Driven Collections and Drag & Drop ✅ (largely)

Substantially delivered. Dynamic lists and trees backed by reactive data models, with virtualization, selection, and the drag-and-drop system from architecture Section 14. The new `fern-data` crate hosts the data types; ListView/TreeView/Repeater live in fern-widgets. The `data_collections` example exercises the full system.

**Delivered:**

- **fern-data crate.** New workspace member. Houses the data-model types that are conceptually above the widget tree and should not depend on fern-core's widget internals.
  - `ListModel<T>` (`list_model.rs`). Concrete reactive list owning items as `Vec<T>` behind `Rc<RefCell<>>`. Mutations emit `DataChange` automatically. Cloneable for shared access.
  - `ListDataSource` trait (`list_data_source.rs`). Escape hatch for large/external datasets. Callback-based item access. Implementor emits `DataChange` manually. Not related to ListModel by inheritance — two separate input paths on ListView.
  - `TreeModel<T>` (`tree_model.rs`). Concrete reactive tree with NodeId-addressed nodes. Mutations emit `TreeChange` automatically. Cloneable for shared access.
  - `TreeSlice<T>` (`tree_slice.rs`). Per-view flattened projection of a TreeModel. Owns expand/collapse state. Exposes flat visible-node list with depth. Emits `DataChange`. Created internally by TreeView via `tree.create_slice()`. Multiple TreeViews sharing the same TreeModel get independent expand states.
  - `SelectionModel` (`selection_model.rs`). `Signal<SelectionSet>` utility. Single-select (click), toggle (Ctrl+click), range (Shift+click), select-all (Ctrl+A). Consumed by both ListView and TreeView.
  - `DataChange` and `TreeChange` enums (`data_change.rs`, `tree_change.rs`). ItemsInserted / ItemsRemoved / ItemsMoved / ItemsUpdated for ListModel; NodeInserted / NodeRemoved / NodeMoved / NodeUpdated for TreeModel.

- **Widget integration.**
  - **Repeater** (`repeater.rs`). Non-virtualized dynamic collection. Takes a `ListModel<T>` and a delegate closure. Creates one child subtree per item. Targeted arena mutations on `DataChange` notifications (no full rebuild).
  - **ListView** (`list_view.rs`). Virtualized scrollable list with scroll binding and selection handling (commits e546c25, 14365b8). Accepts `ListModel<T>` or `ListDataSource` through two constructors. Creates widget subtrees only for visible items plus buffer. Item lifecycle management based on scroll position.
  - **TreeView** (`tree_view.rs`, commit bbc27e0). Hierarchical list with indent, expand/collapse arrows, virtualization. Backed by `TreeModel<T>`. Creates its own `TreeSlice` internally. Keyboard navigation, drop feedback for drag-and-drop (commit 1c9b9df), `toggle_expand` method on TreeSliceHandle (commit 386f014).
  - **`list_item_a11y.rs`** — accessibility wrappers that list/tree items use to expose `position_in_set` / `size_of_set` and `expanded` / `level` correctly (commit 63facc3).

- **Drag and drop.** The core types in `fern-core/src/drag_payload.rs` and `fern-core/src/drag_state.rs`:
  - **`DragPayload` with typed MIME representations.** Multiple representations of the same content carried in a single payload. Drop targets check accepted MIME types during hover without deserializing.
  - **`DragSource` / `DropTarget` contract via attached handlers.** `on_drag` receives `DragPhase::{Started, Moved, Ended}`; the source calls `EventContext::start_drag(..)` or `start_drag_with_preview(..)` during `DragPhase::Started` to produce a `DragPayload` and optional preview widget. `on_drag_hover` evaluates whether the current payload is acceptable. `on_drop` handles the drop by emitting a typed command. The recognizer manages the drag state machine: press-threshold-hold-move-release, pointer capture for the duration, and cancellation on Escape.
  - **Drag preview overlay.** The source widget's preview is rendered as a semi-transparent overlay following the pointer. Uses the existing overlay system with `OverlayLayer::InTree`, `OverlayPlacement::AtPointer` (commit 63facc3).
  - **Intra-widget reordering.** ListView and TreeView produce insertion-line feedback and emit typed reorder commands on drop, routing through `ListModel::move_item` / `TreeModel::move_node`.
  - **Pointer event modifiers.** Commit a3510d7 added modifier keys to pointer events, which DnD uses to distinguish copy-vs-move drags and for Ctrl-click multi-selection during a drag.

**Remaining work.**

- **Cross-application drag backends in fern-platform.** `PlatformDragBackend` trait with OS-specific implementations (`WaylandDragBackend` via wl_data_device, `X11DragBackend` via XDnD, `WindowsDragBackend` via OLE IDataObject/IDropTarget, `MacOsDragBackend` via NSPasteboard/NSDraggingSource) are not yet implemented. Intra-application DnD works on all platforms because it does not depend on OS integration. Cross-application transfer (dragging from a file manager into a FernUI window, or dragging from a FernUI ListView to an external target) is tracked as a fern-platform task for a later phase.
- **Keyboard Alt+Arrow reorder contract.** The DnD spec says every drag operation should have a keyboard equivalent emitting the same command. ListView and TreeView have the plumbing, but per-widget verification is outstanding. This is a documentation/test gap, not a design hole.
- **Retroactive ComboBox/MenuList virtualization.** TODO(milestone-6) comments in `combo_box.rs` and `menu_list.rs` mark the spots where a `max_visible_items` option should switch the dropdown to use a virtualized ListView internally. Not required for M6 completion, but tracked.

---

## Milestone 7: Internationalization ✅ (largely)

Substantially delivered. Fluent-based translation runtime with compile-time key validation, hot-reload via file watcher, RTL layout, and the dual-bundle design for fern-widgets' own translatable strings. Two new crates (fern-i18n and fern-i18n-macros) implement the design from architecture §12. The `internationalization` example exercises the full system.

**Delivered:**

- **fern-i18n runtime** (~1500 lines).
  - `I18nConfig` with `source_locale`, `supported_locales`, `compile_in`, `user_locale`, `auto_detect_os_locale`, `fallback_locale`, `runtime_override`, `framework_locales`, `override_widget_strings`, `test_only` / `with_locale` test constructors.
  - `I18nManager` with three-bundle design: application bundles (from `compile_in`), framework bundles (from `framework_locales`), and widget-override bundles (from `override_widget_strings`). `resolve_app` uses two-step precedence (active locale → source locale); `resolve_widget` uses four-step precedence (override active → framework active → override source → framework source) per §12.13.5. `set_locale` returns `LocaleSwitchOutcome { direction_changed }` so app code can decide whether to trigger a composite rebuild.
  - `LocalizedString` with `to_signal()`, `literal()`, `resolve_now()`, `Into<Prop<String>>` conversion. Deliberately no `From<&str>` to prevent accidental untranslated literals. The `to_signal()` path uses a `WeakSignal` in the observer closure and `attach_keepalive` on the target signal to cleanly unsubscribe when the signal is dropped — this fixes the C1 memory leak where the previous implementation called `mem::forget` on the observer handle and left stale callbacks firing after every locale change. A regression test (`to_signal_observer_unsubscribes_when_signal_drops`) pins this behavior.
  - Thread-local bridge (`thread_local.rs`) — `install` / `clear` for setup and teardown, `with_active` for the resolver path, `current_locale()` / `current_direction()` / `current_version_signal()` for widget-facing accessors. Widget code reaches the installed manager via the public accessors; app code reaches it via `BuildContext::locale()` and `BuildContext::layout_direction()`.
  - File watcher (`file_watcher.rs`) via the `notify` crate for hot-reload. Background thread watches override paths and forwards events through the EventSource bridge so reload happens on the UI thread inside `RefCell::borrow_mut`. `I18nManager::reload_from_path` parses the file via `FluentResource::try_new`, replaces the bundle, and bumps the version signal. Previous bundle is kept intact on any error.
  - Locale resolution (`resolve_initial_locale`) with three-tier precedence: explicit user choice → OS auto-detect via `sys-locale::get_locale()` with partial matching → fallback locale.
  - `LayoutDirection` enum re-exported from `fern-core::environment`. `rtl_from_locale` uses a hardcoded lookup table mapping script tags (`Arab`, `Hebr`, `Syrc`, etc.) to `LayoutDirection::Rtl`. Direction changes when `set_locale` crosses the LTR↔RTL boundary are reported back to the caller for composite rebuild.
  - `compile_in_locales!` declarative macro for populating the `compile_in` slice with many locales × many files without writing repetitive `include_str!` calls by hand.

- **fern-i18n-macros proc macro crate** (~800 lines).
  - `tr!` and `tr_widget!` both validate at compile time. They read the consuming crate's source `.ftl` file (or directory) at expansion time, parse via `fluent-syntax`, build a key → `MessageInfo` map, and check every call.
  - **Compile-time errors with suggestions.** Missing keys produce a `compile_error!` at the macro call site with a Levenshtein-based "did you mean?" suggestion (edit budget 3). Missing and unknown arguments are reported with the list of expected argument names. Non-ASCII Rust idents and `__`-containing segments are rejected at compile time. Malformed source `.ftl` files produce parser errors at every `tr!` invocation in the crate.
  - **Source path resolution** via `FERN_I18N_SOURCE_DIR` / `FERN_I18N_SOURCE_PATH` env vars and auto-detection. Default behavior: if `$CARGO_MANIFEST_DIR/locales/en-US/` exists as a directory, enter directory mode and walk it; otherwise read `locales/en-US.ftl` as a single file. Env vars override auto-detection for test fixtures.
  - **Nested module encoding.** `tr!(auth::login_title())` maps to the Fluent key `auth__login-title`. `_` → `-` within segments (kebab-case), `__` between segments (reserved separator). Arbitrary nesting depth supported.
  - **Rebuild tracking via `include_bytes!`.** Every expansion emits anonymous `const _: &[u8] = include_bytes!(path);` tokens for every `.ftl` file read during expansion. Cargo already tracks `include_bytes!` paths as build dependencies, so editing any contributing `.ftl` file triggers a rebuild. More portable than `proc_macro::tracked_path::path` (which requires unstable features on older compilers) and correctly handles directory-mode expansion.
  - **Compile-time fallback for simple patterns.** Patterns composed of literal text and simple `{ $var }` substitutions get a reconstructed source-language fallback at macro expansion time. If the runtime resolver returns the key literal as a placeholder (no manager installed, or key missing in active bundle), the expansion falls back to the reconstructed text rather than displaying the raw key. Patterns with selectors, plurals, term refs, message refs, or function calls get `fallback = None` and return the key literal when runtime resolution fails. This makes widget-level unit tests work without installing an I18nManager, and makes forgotten framework bundle registration silent (widgets still render correct English labels).
  - **Parse cache.** Key maps cached per source path in a process-wide `Mutex<HashMap<PathBuf, KeyMap>>` for the duration of a proc-macro process. A crate with hundreds of `tr!` calls parses each `.ftl` file exactly once.

- **fern-widgets localization** (`crates/fern-widgets/locales/en-US.ftl`, `fr-FR.ftl`). Framework strings for built-in widget accessibility labels. Current keys: `a11y-status-bar-name`, `a11y-dialog-name`, `a11y-snackbar-name`, `a11y-split-view-divider-name`, `a11y-breadcrumb-current-page-value`. The set is small by design — fern-widgets only translates strings that fern-widgets authors, and leaves application strings to the application. `fern_widgets::framework_locales()` exposes the slice for explicit registration via `I18nConfig::framework_locales(...)`.
  - **Explicit registration.** Unlike the original architecture sketch, fern-widgets' bundle is **not** registered automatically by `FernAppBuilder::run`. Applications opt in with `.framework_locales(fern_widgets::framework_locales())`. Rationale: fern-app is deliberately widget-agnostic (does not depend on fern-widgets), so automatic registration would invert the crate graph. Applications that build from custom widgets only would be forced to pull in fern-widgets and its ~3 MB of Arabic/Hebrew fonts. The proc macro's compile-time fallback covers the forget-to-register case — accessibility labels still render in English.

- **Fonts for RTL scripts** (commits 1210c82, 342f948). Noto Sans Arabic and Noto Sans Hebrew variable fonts under feature flags, with Inter as the Latin-script default. Licenses included alongside the fonts. Arabic and Hebrew rendering uses the RTL bidi path in text-typeset (via HarfBuzz).

- **Brand name variable** (commit e166b02). Localization files use a `-brand-name` term reference for the product name, allowing a single edit to rebrand the application across all locales.

**Remaining work.**

- **ShortcutFormatter.** `shortcut.rs:31` still holds a TODO for locale/platform-aware shortcut label formatting. Current behavior returns `Ctrl+S` regardless of platform or locale. The design calls for `Strg+S` in German, `⌘S` on macOS, etc. `MenuItem` and `Tooltip` would auto-query this formatter instead of displaying the raw string. Scope is small (a single format function and a platform-detection helper in fern-platform) but not yet implemented.
- **Composite rebuild on LTR↔RTL switch.** `set_locale` returns `LocaleSwitchOutcome { direction_changed: bool }` so app code can trigger a rebuild, but the rebuild path has not been exercised in an integration test. The `internationalization` example switches between LTR locales (en-US ↔ fr-FR) only. A test that switches from en-US to ar-SA and verifies HStack child order flips is outstanding.
- **Full `ctx.locale()` / `ctx.layout_direction()` integration testing.** The accessors exist and the signals update, but there are no integration tests yet that render through a real widget tree and assert on locale-dependent text / direction-dependent layout. Covered by unit tests on the individual pieces; end-to-end coverage is a remaining test gap.
- **Translator-facing CLI.** The `--translation-dev` CLI flag for translator hot-reload workflow exists in the spec (§12.6) but the example application does not yet wire it up. Implementation is trivial once done; tracked as a small deliverable alongside the M7 polish.
- **Expanded fern-widgets translations.** Only five keys are currently translated. As more built-in widgets gain user-visible strings (MenuItem labels, ComboBox placeholder, Wizard step headers), the list grows. This is an incremental task, not a blocker.

---

## Milestone 8: Rich Text Editor ✅ (largely)

Substantially delivered. Both construction presets ship: `RichTextEditor::read_only(document, typesetter)` (M8a) and `RichTextEditor::editor(document, typesetter)` (M8b) share a single implementation with per-preset policy bundles. Live in [`crates/fern-widgets/src/rich_text.rs`](../crates/fern-widgets/src/rich_text.rs) plus the `rich_text/` support modules: `policy.rs` (PolicyBundle + CommandFilter + CaretPolicy + AccessibilityRole + ClipboardPolicy), `keyboard.rs` (~22 KB of key routing + undo coalescing), `mouse.rs`, `frame_loop.rs` (the text-document → Signal bridge), `clipboard.rs`, `state.rs` — plus `tests.rs` at ~48 KB, the real validation that this milestone landed. Examples: [`examples/rich_text_viewer`](../examples/rich_text_viewer/) (read-only preset bound to a static document) and [`examples/rich_text_editor`](../examples/rich_text_editor/) (full editor + a read-only preview of the same document in one window, exercising the "mixed" cross-preset tests). Recent polish includes the opt-in system color-emoji fallback in text-typeset (commit af425dc).

**Delivered (post-spec polish):**

- **HTML rich clipboard round-trip.** Cut / copy write both HTML and plain-text payloads to the system clipboard via `DocumentFragment::to_html` + `arboard::set_html`; paste prefers a stashed in-process fragment, then HTML via `TextCursor::insert_html`, then plain text. Covers Linux (`text/html` — native Wayland support via `arboard`'s `wayland-data-control` feature), macOS (`public.html`), and Windows (`CF_HTML`). RTF is the remaining rich format, tracked below.
- **Paste Unformatted.** New `EditCommandKind::PasteUnformatted` bound to Ctrl+Shift+V / ⌘⇧V. Strips any rich payload and inserts only the plain-text content. Gated by `ClipboardPolicy::allows_paste_unformatted`.
- **Default right-click context menu.** `RichTextEditor` installs a built-in factory on its arena node's `HandlerSet::context_menu`; the framework's `show_context_menu_for` intercepts Secondary PointerDown and produces a fresh `MenuList` each right-click, with items gated by the live selection / policy state. Editor preset: **Cut / Copy / Paste / Paste Unformatted / Select All**. Read-only preset: **Copy / Select All**. Menu item closures call `rt_clipboard::*` directly (no Action/Intent indirection — the framework's `show_context_menu_for` adds the menu at top-level, so walking to ancestor Actions wouldn't work anyway). Reserved `fern.rich_text.*` intent names are still fired post-hoc for external observation. Host apps replace the default entirely by passing their own factory via `RichTextEditor::context_menu(factory)` or disable it via `.default_context_menu(false)`.
- **godot-rich-text parity port.** Closed seven functional gaps against the [godot-rich-text](../../godot-rich-text/src/rich_text_edit.rs) reference implementation:
  - **Tab / Shift+Tab** — inside a table: navigate to adjacent cell (Tab at last cell auto-inserts a row); at list-item block-start: `indent` / dedent via `BlockFormat::indent`; otherwise: insert a literal `\t`.
  - **Ctrl+Enter** — always inserts a new block, bypassing Enter-in-table navigation.
  - **Enter inside a table cell** — moves to the same column in the row below; on the last row, steps out to the block after the table.
  - **Backspace at list-item start** — dedent while indent > 0; at indent 0, `cursor.remove_current_block_from_list()` converts back to a plain paragraph.
  - **Rectangular cell selection via Shift+Arrow** — `try_extend_cell_selection` activates / extends a `CellRange` selection when the caret is at a cell boundary; repeated Shift+Arrow extends the rectangle.
  - **Link / image click callbacks** — `RichTextEditor::on_link_activated(|href, ctx| …)` / `on_image_activated(|name, ctx| …)` builder hooks fire on `HitRegion::Link` / `HitRegion::Image` primary clicks (hrefs were silently dropped before).
  - **Horizontal caret-visibility** — `ensure_caret_h_visible_locked` adjusts `scroll_x` after navigation in non-wrapped mode (20 px margin, matches godot).
  - **`caret_char_format` selection-start fix** — reads the format at `cursor.selection_start()` when a selection is active rather than `cursor.position()` (which lands at the end and may fall on a different run). Toolbar state observers now always see the format of the *selection*, not of whatever sits past it.
- **Widget API surface (36 methods).** Mirrors godot's `#[func]` surface on `RichTextEditor`: formatting setters (`set_bold` / `set_italic` / `set_underline` / `set_strikethrough` / `set_font_size` / `set_font_family` / `set_alignment` / `set_heading_level`) + toggles (`toggle_bold` / `toggle_italic` / `toggle_underline` / `toggle_strikethrough`); query getters (`is_bold` / `is_italic` / `is_underline` / `is_strikethrough` / `get_heading_level` / `get_alignment` / `is_in_table`); list commands (`insert_list(ordered)` / `create_list(style)`); table commands (`insert_table` / `remove_current_table` / `insert_row_above` / `insert_row_below` / `insert_column_before` / `insert_column_after` / `remove_current_row` / `remove_current_column`); cursor mirrors (`insert_text` / `insert_html` / `insert_image` / `delete_selection` / `select_word` / `select_line` / `set_caret_position`); programmatic clipboard (`copy` / `cut` / `paste` / `paste_unformatted`); runtime zoom (`set_zoom_level` / `get_zoom_level`). Apps now build toolbars without reaching through `TextDocument::cursor()`.
- **Additional observability signals.** `format_version: Signal<u64>` bumps only on `DocumentEvent::FormatChanged` (distinct from `document_version`, which bumps on both content and format); `document_loaded_count: Signal<u64>` pulses on `DocumentEvent::LongOperationFinished` for async `set_html` / `set_markdown` completion.

**Remaining polish (not blocking):**

- **RTF clipboard payload.** The long-tail rich format — needed for Pages / TextEdit / some legacy Windows apps that don't emit HTML on copy. HTML already covers Firefox, Word, Google Docs, Apple Notes, everything Chromium, and everything Gecko. Would require an RTF importer in text-document and a matching `set_rtf` / `get_rtf` pair on `ClipboardBackend`.
- **Keyboard Alt+Arrow reorder contract for embedded lists.** Hooked up plumbing exists but per-widget verification is outstanding.
- **Unfinalized command filter entries.** A handful of less-common editing commands (block quote, horizontal rule insertion) are tracked as incremental adds against the existing command filter.

**Goal (preserved from the pre-implementation spec):** A functional rich text widget using text-document and text-typeset, with formatting toolbar and context menu for the editable preset and a read-only preset for documentation displays, help panels, and message rendering. All UI strings use the `tr!` / `tr_widget!` macros from Milestone 7.

**Delivers:**

`RichTextEditor` widget (fern-widgets, behind `rich-text` feature flag) with **two construction presets** that share a single implementation:

- **`RichTextEditor::editor(document, typesetter)`** — the editable preset. Full command filter (Bold, Italic, Heading, list insertion, table insertion, etc.), blinking caret, `Role::MultilineTextInput`, full clipboard (cut/copy/paste/select-all), IME composition hooks, undo/redo stack, debounced `text_changed` signals.
- **`RichTextEditor::read_only(document, typesetter)`** — the read-only preset. Command filter rejecting every mutating command, non-blinking caret (static on focus for screen-reader navigation, or hidden), `Role::Document`, copy/select-all clipboard only, no undo stack. Link click activation still works — it is the main interaction in a read-only view.

Both presets produce the same Rust type, share the same arena node structure, the same paint pipeline, the same hit-testing logic, the same scroll bar pair, and the same frame-loop bridge from document events to the reactive Signal model. The preset machinery configures a `CommandFilter`, a `CaretPolicy`, an `AccessibilityRole`, and a `ClipboardPolicy` at construction time; the rest of the implementation is unaware of which preset built it. There is no `read_only: bool` field, and there is no runtime toggle — switching a widget from editable to read-only requires a composite rebuild. See architecture §27.10.1 for the rationale.

Editor functionality: integration of text-document's TextCursor with FernUI's event system. Keyboard input for insertion and deletion. Mouse click/drag for cursor positioning and selection. Double-click word selection, triple-click paragraph selection. Multi-cursor support. Formatting toolbar connected via typed commands (labels via `tr!` for application-level toolbars, or `tr_widget!` if the framework eventually ships a built-in toolbar). Context menu: Cut, Copy, Paste, Select All. Text selection rendering via text-typeset's DecorationRect. Syntax highlighting via text-document's Highlighter trait. Undo/redo with widget-level typing coalescing. Scrolling via text-typeset's viewport-scoped rendering inside an editor-owned scroll bar pair (not a ScrollArea — see architecture §27.10 for the circular-dependency reason). `Canvas::draw_render_frame()` embedding text-typeset's output.

Clipboard: platform-level read/write of text and HTML via fern-platform's `ClipboardBackend` trait (`get_text` / `set_text` / `get_html` / `set_html` / `has_text` / `has_html`). Cut / Copy / Paste (Ctrl+X/C/V, ⌘X/C/V on macOS) round-trip rich HTML between applications; Paste Unformatted (Ctrl+Shift+V / ⌘⇧V) strips formatting to plain text. RTF remains a post-milestone refinement for the apps that emit it (Pages, TextEdit, some legacy Windows apps).

**Recommended decomposition (per architecture §27.10.16):**

- **M8a: read-only preset.** Implement the policy-preset machinery (`CommandFilter`, `CaretPolicy`, `AccessibilityRole`, `ClipboardPolicy`) and the `RichTextEditor::read_only(...)` constructor. This delivers a usable read-only widget backed by the full shared core (document + typesetter ownership, frame loop bridge, scroll bar pair, the four-pass paint pipeline, hit testing). Validates the architectural approach without the complications of editing, undo, IME, or formatting commands.
- **M8b: editor preset.** Add the `RichTextEditor::editor(...)` constructor with the editable command filter, cursor mutation commands, undo/redo, debounced `text_changed`, drag-select with auto-scroll, clipboard mutation, and the typed-command builder methods. No file from M8a is rewritten — only extended.

**Blocked by:** text-document and text-typeset integration work (the crates exist but the integration surface into fern-widgets is new). ContextMenu from Milestone 4 (done). Clipboard integration in fern-platform.

**Tests:**

Shared across both presets:
- Text selection via mouse click-drag matches TextCursor range
- Double-click selects word, triple-click selects paragraph
- Link click activation emits the declared `on_link_clicked` command
- Copy places selected text on clipboard in both presets
- Scroll bars integrate correctly with editor-owned scroll signals
- AccessKit: selection and caret position exposed via `set_text_selection` and `set_caret_position`

Read-only preset only:
- Typing a character is rejected (no document mutation)
- Cut, Paste, and Delete commands are rejected
- Undo/Redo are rejected (no undo stack to consult)
- Accessibility role is `Role::Document`, not `Role::MultilineTextInput`
- Screen reader focus does not enter forms-navigation mode
- Static caret is visible on focus (if the widget was constructed with keyboard navigation enabled)

Editor preset only:
- Typing produces correct document mutations
- Formatting commands apply to selection and are reflected in rendering
- Undo reverses last operation, Redo re-applies
- Undo coalescing: consecutive character insertions grouped into one undo step
- Context menu Cut/Copy/Paste work with clipboard
- Toolbar and menu labels respond to locale changes
- Highlighter colors propagate through text-typeset to render output
- RichTextEditor accepts externally-owned TextDocument reference
- Application retains full access to TextDocument API
- Caret blinks at the configured rate via `tree.advance_time()`
- Accessibility role is `Role::MultilineTextInput`
- `Action::SetValue` replaces document content

Mixed:
- Same TextDocument bound to an `editor()` widget and a `read_only()` widget in the same window: edits in the editor widget immediately appear in the read-only widget via the shared document-version Signal

---

## Milestone 9: Text Input ✅ (IME still deferred)

Delivered. `TextInput` ships as a single-line composite built on `RichTextEditor` with `multiline(bool)` for the opt-in multi-line variant ([`crates/fern-widgets/src/text_input.rs`](../crates/fern-widgets/src/text_input.rs), 474 lines). The numeric specialization ships as a generic `SpinBox<T>` with an associated `SpinValue` trait implemented for `i32` / `f32` ([`crates/fern-widgets/src/spin_box.rs`](../crates/fern-widgets/src/spin_box.rs), ~1,400 lines) supporting `WrapMode::{Clamp, Wrap, Cycle}`, `StepType::{Fixed, Adaptive}`, and platform-aware formatting. Example: [`examples/spin_box`](../examples/spin_box/) demonstrates the full widget with step types and wrap modes. Both composites are behind the `rich-text` feature flag (inherited from the underlying editor). Recent relevant commits include 34481a2 (SpinValue trait + demo), 8f676cf (Signal handling for checkbox/radio/text_input), 9248377 (`TextInputField` promoted to primitives), 1be6415 (show_clear_button icon via built-in SVG), and the Signal<Role> migration in fc4cca2.

**Remaining (design-deferred):** IME composition window positioning, composition-text rendering, and CJK input handling. These require platform-specific work in fern-platform (winit IME events are wired; the *positioning* of the composition UI and the composition rendering are the open pieces). The TextInput and RichTextEditor APIs don't change when this lands — the handler hooks are already in place. Latin-script editing works today.

**Goal (preserved from the pre-implementation spec):** Single-line plain text editing, derived from the Rich Text Editor by constraining formatting to a single paragraph of plain text. This reverses the common GUI evolution path (plain-to-rich) — in FernUI the rich editor is the fundamental widget, and TextInput is the constrained specialization.

**Delivers:**

TextInput: Level 2 widget built on RichTextEditor from Milestone 8, with the following constraints enforced at construction:
- Single paragraph (Enter key does not insert a newline; emits `on_submit` instead)
- Single line (configurable via `multiline(bool)` builder method; default is single-line)
- No rich formatting (Bold, Italic, Heading commands disabled at the command filter level)
- Plain text representation exposed via `Signal<String>` (two-way binding with the underlying TextDocument)

Cursor rendering, selection rendering, text selection interactions, and keyboard editing are all inherited from RichTextEditor — no reimplementation.

NumberInput/SpinBox: TextInput with increment/decrement buttons and numeric validation. Composition using TextInput + Buttons. Validation rejects non-numeric input at the command filter level.

Clipboard: already implemented in Milestone 8 — TextInput reuses it directly.

**IME support is deferred** to a post-milestone refinement. IME composition window positioning, composition text rendering, and CJK input handling require platform-specific work in fern-platform. TextInput in this milestone targets Latin-script languages; IME for CJK, Arabic, and Indic scripts is tracked separately and can be added without changing the TextInput API.

**Blocked by:** RichTextEditor from Milestone 8.

**Tests:**
- Typing produces correct text in `Signal<String>`
- Cursor position updates on Arrow Left/Right, Home/End
- Shift+Arrow produces correct selection range
- Double-click selects word
- Ctrl+A selects all
- Backspace/Delete remove character at cursor or selected range
- Ctrl+Backspace removes word
- Cursor blinks at correct rate via `tree.advance_time()`
- Enter in single-line mode does not insert newline; fires `on_submit`
- Formatting commands (Bold, Italic) are filtered and do not affect the document
- Copy places selected text on clipboard; Paste inserts clipboard text at cursor
- Cut removes selected text and places on clipboard
- NumberInput rejects non-numeric typed input
- NumberInput increment/decrement buttons adjust value within bounds
- AccessKit: `Role::TextInput` with value, text_selection, caret position

---

## Milestone 10: Multi-Window and Platform Integration

**Goal:** Multiple windows with shared state, modal/modeless dialogs using platform windows, and native menu bar integration.

**Multi-window API delivered.** The public surface is shipped: `WindowConfig` (single creation entry point for both `FernAppBuilder::initial_window` and runtime `ctx.open_window`), reactive per-window [`WindowState`](../crates/fern-core/src/window/state.rs) with two-way OS↔signal sync guarded against re-entrancy (Compose Multiplatform #1489 cautionary tale), synchronous `EventContext::open_window` / `focus_window` / `close_window_by_id` / `find_window` / `window_state` routed through the `WindowOps` trait, the dispatch-re-entry pattern (`dispatch_in_window` temporarily removes the dispatching window from the map so `open_window` from a handler can call `WindowManager::create_window` on the same loop), and `EventContext::open_modal` as a thin wrapper that unifies native-modal creation with the rest of the open_window path. `PlatformTitleBarHost` trimmed — state operations (`minimize` / `toggle_maximize` / `close` / `is_maximized`) moved to `WindowState`; the `TitleBar` widget binds the maximize glyph to `ctx.window().placement()` and now works with `DecorationsMode::Native` too. Full reference: [docs/multi-window.md](multi-window.md). End-to-end demo: [examples/multi_window](../examples/multi_window/src/main.rs).

**`WindowConfig`** replaces the previous `.window_title` / `.window_size` / `.root` / `.custom_chrome` shim methods on `FernAppBuilder` (deleted — no back-compat). `WindowPlacement` (4-variant `Floating` / `Maximized` / `Fullscreen` / `Minimized`) replaces boolean soup. `DecorationsMode` (3-variant `Native` / `CustomChrome` / `None`) replaces `custom_chrome: bool`. `ModalConfig` (always-paired parent + focus_target) replaces the previous `modal: bool` + `parent: Option<FernWindowId>` two-field pattern. `WindowIcon` adds the icon-at-creation capability. `FernAppBuilder::initial_window(WindowConfig)` is the only window entry point; every example in `examples/*` now uses it.

**Remaining deliverables for Milestone 10:**

Native menu bar: NSMenu on macOS, widget-based MenuBar (from Milestone 4) inside the window on Windows and Linux. Declarative MenuBar description through FernApp builder. Abstraction over the platform difference: the application declares its menu structure once, and FernUI routes to native on macOS and to the in-window MenuBar elsewhere.

File dialog: native open/save via `rfd` crate or OS APIs. Async result via EventLoopProxy.

**Blocked by:** Platform-specific Cocoa/AppKit code for macOS menu bar (goes beyond winit).

**Tests (done):**

- WindowState OS-side write does not enqueue an OS command (re-entrancy guard)
- OS-side write still notifies derived signals (observer still fires for bound widgets)
- App-side write enqueues the matching `WindowCommand`
- Modal dialog blocks parent window input
- Modeless dialog operates independently
- Theme change propagates to all windows
- Locale change propagates to all windows
- `EventContext::open_window` / `find_window` / `focus_window` / `close_window_by_id` route through the `WindowOps` trait
- `EventContext::open_modal` builds a `WindowConfig` and returns synchronously
- `EventContext::open_window` on a standalone tree (no app context) panics by design
- `EventContext::find_window` / `window_state` / `windows` return `None` / empty on standalone trees
- TitleBar's minimize / maximize / close buttons write through `WindowState::placement` (not the chrome host)
- Focus returns to parent after modal dismissal

**Tests (remaining):**

- Native menu bar on macOS mirrors the declared MenuBar structure
- File dialog returns selected path asynchronously without blocking the event loop

---

## Next Candidates

Beyond the Milestone 6 / 7 / 8 / 10 tails tracked above, five areas are the obvious next-up work. None are promoted to numbered milestones yet — they're here so the roadmap is honest about what's in the pipeline:

- **IME / CJK input.** Platform-side composition window positioning + composition rendering in `fern-platform` + dead-key handling. The TextInput and RichTextEditor APIs don't change when this lands; the handler hooks are already in place. Blocked on winit IME glue and per-OS composition-window surface work.
- **Cross-application DnD backends.** `PlatformDragBackend` implementations per OS: `WaylandDragBackend` (wl_data_device), `X11DragBackend` (XDnD), `WindowsDragBackend` (OLE IDataObject / IDropTarget), `MacOsDragBackend` (NSPasteboard / NSDraggingSource). Intra-app drag already works everywhere; this unlocks dragging between a FernUI window and other applications. Payload and handler APIs are stable.
- **Native menu bar on macOS + native file dialogs.** `NSMenu` binding so a single declarative menu description routes to native on macOS and to the existing widget-based `MenuBar` elsewhere. Native file open/save dialog via `rfd`, with async result through `EventLoopProxy`. These are the remainder of Milestone 10.
- **Virtualized dropdowns.** Shipped for `ComboBox` — large lists now materialize only the visible rows via a `ListView` routed through a bridged `ListSource`, and the searchable filtered path rides the same renderer. `MenuList` grew a `max_visible_items` viewport cap (panel height + internal `ScrollArea`) but does **not** yet virtualize: its API takes arbitrary `impl Widget` children, so true virtualization needs a model-driven rewrite. Remaining follow-up: model-backed MenuList API.
- **ShortcutFormatter (locale + platform).** `shortcut.rs` currently renders chords as `Ctrl+S` regardless of platform or locale. The design calls for `⌘S` on macOS and translated modifier names (`Strg+S` in German). A single formatter hook consulted by `MenuItem::for_shortcut(id)` and `TooltipContent::for_shortcut(id)` — tracked as part of the M7 polish tail.

## Summary

| # | Milestone | Status | Key Capability |
|---|-----------|--------|----------------|
| 1 | Button in a Window | ✅ Done | Full vertical slice, rendering pipeline |
| 2 | Text and Layout | ✅ Done | Layout engine, text, theme switching |
| 3 | Core Widget Catalog + V2 Migration | ✅ Done | Form controls, unified Widget trait, Signal<T> only, binding.rs rename |
| 4 | ScrollBar, ComboBox, Menus | ✅ Done | Overlay-based interactive widgets, ScrollArea modes, MenuBar |
| 5 | Tabs, SplitView, Dialogs, Modals | ✅ Done | TabWidget, SplitView+SplitHandle, Dialog, ModalContainer, Popover, Snackbar, Breadcrumb, Wizard, DragRecognizer, in-tree and native modal presentations |
| 6 | Data-Driven Collections + Drag & Drop | ✅ Largely done | fern-data crate, ListModel/TreeModel/TreeSlice/SelectionModel, Repeater/ListView/TreeView with virtualization, typed DnD with preview overlay. Cross-app platform backends remaining. |
| 7 | Internationalization | ✅ Largely done | fern-i18n + fern-i18n-macros, `tr!`/`tr_widget!` with compile-time validation and Levenshtein suggestions, compile-time fallback, dual-bundle with explicit framework registration, hot-reload, RTL with Arabic/Hebrew fonts. ShortcutFormatter remaining. |
| 8 | Rich Text Editor | ✅ Largely done | Both presets delivered: `RichTextEditor::read_only` (M8a) + `RichTextEditor::editor` (M8b). Shared core with per-preset PolicyBundle. HTML rich-clipboard round-trip (arboard, with native Wayland); `PasteUnformatted` (Ctrl+Shift+V); default right-click context menu with slot-based replacement via `context_menu(factory)`. Full godot-rich-text parity port: Tab / Shift+Tab table-nav + list-indent, Ctrl+Enter, Enter-in-cell, Backspace-in-list, Shift+Arrow cell selection, link / image click callbacks, horizontal caret-visibility, 36 widget-level API methods (formatting setters + toggles, query getters, list / table / cursor mirrors, programmatic clipboard, runtime zoom), plus `format_version` / `document_loaded_count` observability signals. RTF clipboard payload remains. |
| 9 | Text Input | ✅ Done | TextInput (single-line, opt-in multiline) + generic SpinBox<T> with SpinValue trait. IME composition / CJK deferred to platform work — TextInput API unchanged when that lands. |
| 10 | Multi-Window and Platform | Largely done | Full multi-window API shipped — `WindowConfig`, reactive `WindowState` with OS↔signal re-entrancy guard, synchronous `ctx.open_window` via `WindowOps` trait, `ctx.open_modal`, `DecorationsMode`, `WindowIcon`, trimmed `PlatformTitleBarHost`, `multi_window` example. Native menu bar + file dialogs remaining. |
