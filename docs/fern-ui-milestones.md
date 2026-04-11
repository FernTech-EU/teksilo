# FernUI Milestones

**Companion to:** fern-ui-architecture.md, fern-ui-code-examples.md  
**Date:** April 11, 2026  
**Status:** Living document — reflects actual codebase state and remaining work

---

## Guiding Principles

Each milestone produces a demonstrable application that exercises progressively more of the architecture. Milestones are ordered by dependency — each one builds on capabilities validated by the previous one. Every milestone includes headless tests verifiable via `cargo test` with no display server.

---

## Current State: What Exists

The following capabilities are implemented and tested in the codebase.

**Crate infrastructure.** All crates compile and are wired through the `fern-ui` umbrella crate: fern-tokens, fern-canvas, fern-core, fern-text, fern-render, fern-platform, fern-app, fern-widgets. The fern-i18n crate exists as a stub.

**V2 widget authoring model (Section 28 of architecture).** The V1 Widget/CompositeWidget split is removed. One unified `Widget` trait with six methods: `build(&mut self)`, `size_that_fits`, `place_children`, `paint`, `accessibility`, `children`. The `build(&mut self)` lifecycle uses take_widget/restore_widget extraction from the arena for borrow safety. `Signal<T>` replaces State/DerivedState/Reactive as the primary reactivity type (V1 types retained in state.rs for internal binding infrastructure). `Prop<T>` replaces `Reactive<T>` for widget properties. `ObserverHandle` provides RAII cleanup for observers. Attached event handlers (`on_tap`, `on_hover`, `on_key`, `on_focus`, `on_access_action`, `on_pointer_event`, `on_scroll`) replace the monolithic `event()` method. `HandlerSet` with `ctx.apply_self_handlers()` for handler attachment during `build()`. `BuildContext` provides `ctx.signal()`, `ctx.effect()`, `ctx.animated_signal()`, `ctx.self_id()`, `ctx.apply_self_handlers()`. The framework auto-wires gesture recognizers when handlers are attached. A separate `compat.rs` file isolates the V1<->V2 bridge conversions.

**Tokens and theming.** Theme struct with ColorTokens, SpacingTokens, TypographyTokens, ShapeTokens, MotionTokens. Light and dark defaults. Subtree theme overrides with environment propagation. Runtime theme switching via `CommandContext::set_theme()` triggering composite rebuild across all windows. Easing curves (Linear, EaseIn, EaseOut, EaseInOut) with `lerp()`.

**Canvas and rendering.** Three-tier Canvas API: axis-aligned rects (DecorationRect), SDF shapes (ShapeQuad with rounded rect, circle, ellipse), and CPU-rasterized paths (PathAtlas with tiny-skia, LRU eviction). Image rendering pipeline (Canvas::draw_image, ImageManager in fern-render). Text rendering via text-typeset integration (SharedTypesetter, layout_single_line). Gradient support (linear, radial). Scissor rect stack for viewport clipping (SetClip/ClearClip with intersection). wgpu rendering pipeline with glyph atlas, shape atlas, and per-frame draw command dispatch.

**Widget arena and layout.** SlotMap-based arena with parent-child relationships. SwiftUI-style propose/respond/place layout negotiation. Dirty flags (needs_layout, needs_paint) with ancestor propagation for relayout. Alignment system (HAlignment, VAlignment, Alignment) with per-child overrides. Layout direction (LTR/RTL). The `widget_tree.rs` module is split into eight implementation files for maintainability: `accessibility_impl`, `event_dispatch_impl`, `focus_impl`, `layout_impl`, `overlay_impl`, `query_impl`, `rendering_impl`, and `test_api`. This is an internal refactor with no API change.

**Primitive widgets.** RectWidget, TextWidget, HStack (with spacing, cross-axis alignment, spacer-aware distribution), VStack, ZStack, Padding, Spacer, Center, Expand, FixedSize, MinSize, MaxSize, Divider, IconWidget (vector icons via Path/PathAtlas), Grid, Wrap/FlowLayout, AspectRatio, Switcher.

**Widget composition (V2).** Unified Widget trait with `build(&mut self)` for child construction. BuildContext provides `ctx.add()`, `ctx.signal()`, `ctx.effect()`, `ctx.animated_signal()`, `ctx.self_id()`, `ctx.apply_self_handlers()`. Inline child resolution via `child()`, `children()`, `child_opt()` on containers. `visible_when` and `enabled_when` as builder methods resolved during `add()`.

**Reactive state (V2).** Signal<T> as the primary reactivity type — `Signal::new()`, `signal.get()`, `signal.set()`, `signal.map()`, `signal.observe()` returning `ObserverHandle` for RAII cleanup. `Prop<T>` replaces `Reactive<T>` for widget properties, accepting both plain values and signals via `impl Into<Prop<T>>`. `ctx.effect()` for scoped side effects cleaned up on rebuild. V1 types (State<T>, DerivedState<T>, Reactive<T>, BindingRegistry) retained in state.rs for internal use.

**Event system (V2).** Pointer events (move, down, up, enter, leave), keyboard events (down, up), scroll events (lines, pixels), IME events (composition, commit), AccessKit action routing. Preview pass (root -> target) and bubble pass (target -> root). Hit testing against layout bounds. Gesture recognizers (TapRecognizer, DoubleTapRecognizer) with GestureArena for competition, auto-wired from attached handlers. Attached event handlers stored on arena nodes, dispatched by the framework. `HandlerSet` for handler construction during `build()`. Deferred tree mutations via EventContext (dormancy, activation, destruction). Additional EventContext operations for overlay-based widgets: `request_focus(id)`, `dismiss_all_overlays()`, `cancel_delayed_overlay(id)`, `synthetic_click(id)`.

**Focus management.** Tab/Shift-Tab cycling in document order. Focus origin tracking (Keyboard, Pointer, Programmatic). Focus-visible behavior. Scroll-into-view on focus change (dispatches ScrollIntoView to nearest clipping ancestor). FocusGained/FocusLost events. Programmatic focus transfer via `ctx.request_focus()` for menu opening and dialog content.

**Overlay system.** OverlayManager with stacking, cascade dismissal, Escape handling, click-outside detection. OverlayPlacement (Below, Above, BelowPreferred, TrailingEdge, AtPointer, NearAnchor). BelowPreferred flips to Above when insufficient space below (used by ComboBox and MenuBar). DismissBehavior (ClickOutside, PointerLeave, Manual). OverlayLayer (InTree, NativePopup, Auto). In-tree overlay layout and positioning. Delayed overlay opening (for submenu hover delays) with `cancel_delayed_overlay` support. Tooltip attachment with configurable delay and simulated clock for deterministic tests.

**Shortcut system.** ShortcutMap with global and scoped bindings. Preview-pass interception before widget event dispatch. Shortcut unbinding. Automatic shortcut label lookup on MenuItem via `ctx.shortcut_label_for_any()`.

**Scrolling (Sections 3.6-3.8 fully implemented).** clips_children flag on WidgetNode. ScrollArea widget with unbounded proposals, offset placement, scroll event handling via attached `on_scroll` handler, ScrollIntoView handling, AccessKit scroll properties. ScrollBar as a standalone widget with thumb drag, track click, keyboard adjustment. `ScrollBarStyle::Overlay` (default, Ubuntu-style) shows a thin resting indicator that expands to a full interactive scroll bar as an overlay on hover — viewport width unchanged. `ScrollBarStyle::Permanent` makes the ScrollBar a layout sibling of the content, reducing the viewport by the scroll bar's thickness. Scissor rect implementation in renderer with clip stack intersection.

**Animation.** AnimationScheduler driving both State<f32> (V1) and Signal<f32> (V2) values with duration and easing. Supports cancel, replace, zero-duration immediate set. Multiple independent animations. Signal<f32>::animate_to() and State<f32>::set_animated() both supported. Toggle, Accordion, ScrollArea, ProgressBar use Signal<f32>::animate_to(). Integrated into the widget tree's frame lifecycle.

**Dormancy.** Three activation states (Active, Dormant, Destroyed). Recursive dormancy/activation. visible_when binding for state-driven toggling. ComboBox and MenuBar pre-create their dropdown content as dormant subtrees, activating them via `ctx.activate()` when the menu opens.

**Window management.** WindowManager in fern-app with per-window WidgetTree. Window creation/closure via CommandContext (queued operations). Theme propagation to all windows. Event routing by winit window ID. FernWindowId abstraction.

**Accessibility.** AccessKit integration at the Widget trait level. AccessNodeBuilder with role, name, actions, disabled, bounds. sync_accessibility() generating TreeUpdate. AccessAction routing to target widgets. Focus tracking in TreeUpdate.

**Higher-level widgets (Milestones 3 and 4 complete).** Button, Panel, Card, Toolbar, StatusBar, Checkbox (two-state and tristate), RadioButton, Toggle/Switch, Slider (horizontal/vertical, stepped), SegmentedControl, ProgressBar (determinate and indeterminate), Badge, Accordion, Link, ScrollArea (overlay and permanent modes), ScrollBar, Tooltip, ComboBox (dropdown with keyboard navigation and type-ahead), MenuItem (with icons, shortcut labels auto-resolved from ShortcutMap, submenu support with hover delay and diagonal movement tolerance), MenuList (vertical container with keyboard highlight), MenuBar (horizontal menu bar with trailing slot for additional actions), MenuSeparator, ContextMenu (right-click with dynamic content).

**Examples.** simple_button (Milestone 1 demo). text_and_layout (Milestone 2 demo). widget_catalog (Milestone 3 demo — all Milestone 3 widgets showcased with theme switching, 766 lines). menus_and_dropdowns (Milestone 4 demo — ComboBox, ContextMenu, MenuBar with File/Edit/Format menus, 909 lines).

---

## Milestone 1: Button in a Window ✅

Completed. A window displaying a single themed button with click handling, hover/press states, text rendering, AccessKit accessibility, and keyboard activation. The `simple_button` example demonstrates this with comprehensive tests.

---

## Milestone 2: Text and Layout ✅

Completed. A window with multiple widgets in nested layouts (HStack-in-VStack, Spacer distribution), text rendering with multiple typography styles, and runtime theme switching (light/dark). The `text_and_layout` example demonstrates this with tests for layout correctness, theme swapping, and composite rebuild.

---

## Milestone 3: Core Widget Catalog ✅

Completed. All Milestone 3 widgets are implemented and tested. The milestone also included the V2 widget authoring model migration (architecture Section 28), which was driven by three problems identified during implementation: the Widget/CompositeWidget split forced wrong decisions, the RefCell<Option<State>> pattern was required by every stateful composite, and the four reactivity types confused widget authors.

**V2 migration completed alongside Milestone 3:**
- composite_widget.rs and composite_adapter.rs deleted. No CompositeWidget references remain.
- All 22 widget files use `impl Widget for` with the unified trait.
- No `RefCell<Option<State<T>>>` pattern remains anywhere.
- Every interactive widget uses Signal<T>: Button, Toggle, Checkbox, RadioButton, Slider, Accordion, Badge, Card, Link, SegmentedControl, ScrollArea, ScrollBar, ProgressBar.
- ScrollArea fully migrated to Signal<f32> for all six scroll state fields.
- Toggle fully Signal-ified (no Rc<Cell<>> remaining).
- ProgressBar uses Prop<T> for fill/track colors and Signal<f32> for indeterminate animation.
- AnimationScheduler supports both State<f32> and Signal<f32>; Toggle, Accordion, ScrollArea, ProgressBar use Signal<f32>::animate_to().
- All container primitives implement build() for PendingChild resolution.
- widget_catalog example uses exclusively `ctx.signal()` (14 calls, zero `ctx.state()`).

**Remaining V1 internals:**
- state.rs retained (758 lines) as internal binding infrastructure. Widget usage limited to BindingLevel enum (3 widget files), Reactive<bool> accepted by visible_when/enabled_when via bridge conversions.
- BuildContext retains V1 legacy methods (`ctx.state()`, `ctx.observe()`) marked as deprecated.
- ScrollBar uses Rc<Cell<>> for drag interaction state (legitimate low-level use).
- DragRecognizer and LongPressRecognizer not yet implemented in gesture system.

---

## Milestone 4: ScrollBar, ComboBox, Menus ✅

Completed. Overlay-based interactive widgets and the ScrollArea overlay/permanent mode refactor from Section 3.7. The `menus_and_dropdowns` example demonstrates the full set with File/Edit/Format menus, ComboBox, and context menu.

**Delivered:**
- **ScrollArea overlay mode (default).** Thin resting indicator (4px) expands to full interactive ScrollBar as an overlay on hover. Viewport width unchanged. `ScrollBar::overlay_mode(true)` with configurable `resting_thickness`.
- **ScrollArea permanent mode.** ScrollBar is a layout sibling of the content, reducing the viewport by the scroll bar's thickness. Selected via `ScrollBarStyle::Permanent`.
- **ComboBox** (902 lines). Non-generic, index-based selection via `Signal<Option<usize>>`. Dropdown content pre-created as dormant subtree, activated on open. Keyboard navigation (Arrow Up/Down, Enter, Escape), type-ahead filtering, `BelowPreferred` placement.
- **MenuItem** (910 lines). Non-generic, closure-based command erasure (same pattern as Button). Supports icons, shortcut labels, disabled state, submenu triggers. Automatic shortcut label lookup from ShortcutMap via `ctx.shortcut_label_for_any()`. Submenu opens on 200ms hover delay, closes on 150ms delay — provides diagonal movement tolerance across other menu items.
- **MenuList** (464 lines). Vertical container for MenuItem and MenuSeparator. KeyboardHighlightWrapper for focus visualization. Arrow Up/Down navigation.
- **MenuBar** (1,138 lines). Horizontal bar with dropdown menus. Trailing slot for additional actions. MenuContext coordinates open index, trigger focus, cross-menu Left/Right navigation.
- **MenuSeparator**. Themed 1px horizontal line with padding.
- **ContextMenu.** Right-click opens a MenuList overlay at pointer position via `OverlayPlacement::AtPointer`.
- **New EventContext operations.** `request_focus(id)` for transferring focus on menu open, `dismiss_all_overlays()` for closing the entire overlay stack, `cancel_delayed_overlay(id)` for aborting a pending submenu open when hover ends, `synthetic_click(id)` for keyboard Enter activation.
- **New overlay placement.** `BelowPreferred` flips to `Above` when there is insufficient space below the anchor.
- **widget_tree.rs refactor.** Split from a 3,621-line monolith into a 892-line main file plus eight implementation modules (accessibility_impl, event_dispatch_impl, focus_impl, layout_impl, overlay_impl, query_impl, rendering_impl, test_api). Internal refactor only — no API change.

**Not done (deferred).** The DragRecognizer is still not implemented. ScrollBar uses `on_pointer_event` with manual drag state tracking (Rc<Cell<>> for hovered, dragging, drag_start_pointer, drag_start_scroll, cached_bounds). This is legitimate low-level interaction state and is the same pattern Slider uses.

---

## Milestone 5: Tabs, SplitView, and Dialogs

**Goal:** Application structure widgets — tabbed interfaces, resizable panes, and modal/modeless dialogs.

**Delivers:**

TabWidget: composite with HStack of tab headers above a Switcher for content panes. Signal<usize> for selected index. The TabWidget delegates switching to the Switcher primitive (already implemented) rather than reimplementing the logic. Trailing slot for tab-level actions (add tab button, overflow menu). Keyboard: Arrow Left/Right between tab headers.

SplitView: Level 2 widget with draggable divider. Signal<f32> for split position. DragRecognizer on divider (this milestone adds DragRecognizer to the gesture system). CursorIcon::ColResize (horizontal) or RowResize (vertical). Configurable minimum pane sizes. Keyboard: focused divider adjustable with Arrow keys. Double-click resets to default.

Dialog: modal Panel via OverlayLayer::NativePopup or in-window overlay with scrim. Focus trapping within dialog widgets. Escape to dismiss. Title, content area, action bar with buttons.

Popover: interactive overlay anchored to trigger, arbitrary content, focus on show, ClickOutside dismissal.

Snackbar/Toast: SnackbarManager for queued auto-dismissing notifications. Animated slide-in/fade-out via Signal<f32>::animate_to().

Breadcrumb: HStack of clickable path segments separated by chevron icons.

**Blocked by:** DragRecognizer gesture — this milestone adds it. LongPressRecognizer (optional, not strictly required).

**Tests:**
- TabWidget switches visible pane on tab click; inactive panes are dormant
- TabWidget keyboard: Arrow Left/Right moves between tabs
- TabWidget uses Switcher internally (delegation, not reimplementation)
- SplitView divider drag updates Signal<f32> and resizes panes
- SplitView respects minimum pane sizes
- Dialog traps focus within its widget subtree
- Dialog Escape dismissal returns focus to parent window
- Modal dialog blocks input to parent window
- Popover shows on trigger click, dismisses on click outside
- Snackbar appears, auto-dismisses after configured duration
- AccessKit: TabList/Tab/TabPanel roles, Splitter role with numeric value, Dialog role with modal

---

## Milestone 6: Data-Driven Collections and Drag & Drop

**Goal:** Dynamic lists and trees backed by data models, with virtualization, selection, and the full drag-and-drop system from architecture Section 14.

**Delivers:**

ListModel<T>: concrete reactive list (Section 15.2 of architecture). Owns items as Vec<T> behind Rc<RefCell<>>. Mutations emit DataChange automatically. Cloneable for shared access.

ListDataSource trait: escape hatch for large/external datasets (Section 15.3). Callback-based item access. Implementor emits DataChange manually. Not related to ListModel by inheritance — two separate input paths on ListView.

Repeater: non-virtualized dynamic collection. Takes a ListModel<T> and a delegate closure. Creates one child subtree per item. Targeted arena mutations on DataChange notifications (no full rebuild).

ListView: virtualized scrollable list. Accepts ListModel<T> (common case) or ListDataSource (large datasets) through two constructors. Creates widget subtrees only for visible items plus buffer. Item lifecycle management based on scroll position.

SelectionModel: Signal<SelectionSet> utility. Single-select (click), toggle (Ctrl+click), range (Shift+click), select-all (Ctrl+A). Consumed by ListView.

TreeModel<T>: concrete reactive tree (Section 15.5). Owns hierarchy with NodeId-addressed nodes. Mutations emit TreeChange automatically. Cloneable for shared access.

TreeSlice<T>: per-view flattened projection of a TreeModel (Section 15.6). Owns expand/collapse state. Exposes flat visible-node list with depth. Emits DataChange. Created internally by TreeView via tree.create_slice(). Multiple TreeViews sharing the same TreeModel get independent expand states.

TreeView: hierarchical list with indent, expand/collapse arrows, virtualization. Backed by TreeModel<T>. Creates its own TreeSlice internally. Keyboard: Arrow Up/Down for focus, Arrow Right to expand, Arrow Left to collapse.

Drag and Drop (Section 14 of architecture). Full implementation of the drag-and-drop system:

- **DragPayload with typed MIME representations.** Multiple representations of the same content carried in a single payload (e.g., a file path as both `text/uri-list` and `text/plain`). Drop targets check accepted MIME types during hover without deserializing.
- **DragData trait.** Typed wrapper for intra-application payloads, avoiding raw byte manipulation for common cases (moving a `ProjectDto`, reordering a `ChapterNode`). Cross-application transfers serialize to MIME types; intra-application transfers keep typed Rust values.
- **DragSource trait.** Produces the `DragPayload` and the visual preview widget when a drag begins. Implemented by ListView items, TreeView items, and any custom widget via the attached `on_drag_start` handler.
- **DropTarget trait.** Declares accepted MIME types, evaluates hover (does the current payload match?), renders drop feedback (insertion line, highlight rectangle), and handles the drop by emitting a typed command. Implemented via attached `on_drag_hover` and `on_drop` handlers.
- **DropFeedback descriptors.** Widget-level rendering hints for drop targets: `InsertionLine { orientation, position }` for ordered lists, `HighlightRect { color }` for container drops, `NoFeedback` for rejecting payloads.
- **DragRecognizer in the gesture system.** Added in Milestone 5 for SplitView; Milestone 6 extends it with payload pickup (from DragSource) and drop handling (to DropTarget). The recognizer manages the drag state machine: press-threshold-hold-move-release, pointer capture for the duration, and cancellation on Escape.
- **Drag preview overlay.** The source widget's preview is rendered as a semi-transparent overlay following the pointer. Uses the existing overlay system with `OverlayLayer::InTree`, `OverlayPlacement::AtPointer`, and no dismiss behavior until the drag ends.
- **Intra-widget reordering.** ListView and TreeView implement both DragSource and DropTarget, producing insertion-line feedback and emitting typed reorder commands on drop. The ListModel/TreeModel `move_item` methods provide the underlying data mutation.
- **Inter-widget transfer.** Drag from one ListView to another (or from a TreeView to a trash button) works automatically when both widgets agree on a typed payload. No special wiring — the framework routes drops based on MIME type compatibility.
- **Cross-application transfer (fern-platform).** `PlatformDragBackend` trait with OS-specific implementations: `WaylandDragBackend` (wl_data_device), `X11DragBackend` (XDnD), `WindowsDragBackend` (OLE IDataObject/IDropTarget), `MacOsDragBackend` (NSPasteboard/NSDraggingSource). The backend is hidden behind the DragPayload API — widget authors write the same code regardless of platform.
- **Keyboard accessibility contract.** Every drag operation has a keyboard equivalent emitting the same command. ListView and TreeView support Alt+Arrow for reordering. Custom drag sources must provide a keyboard path as part of their implementation — this is a lint check, not a runtime requirement, but it is documented as a contract.

Retroactive integration: once ListView exists, ComboBox and MenuList gain a `max_visible_items` option that uses a virtualized scrollable list internally. This is tracked by TODO(milestone-6) comments in combo_box.rs and menu_list.rs.

**Blocked by:** ScrollBar/ScrollArea from Milestone 4 (done). DragRecognizer from Milestone 5. Platform-specific drag backends in fern-platform for cross-application transfer (intra-application DnD has no platform dependency).

**Tests:**
- ListModel push/remove/set emits correct DataChange notifications
- Repeater creates/destroys children on ListModel insert/remove
- ListView only instantiates visible items (arena size significantly smaller than total item count)
- ListView scroll creates entering items, destroys exiting items
- DataChange::ItemsInserted shifts visible items correctly
- DataChange::ItemsRemoved removes item and relayouts
- DataChange::ItemsMoved reorders without rebuild
- SelectionModel: click selects one, Ctrl+click toggles, Shift+click selects range
- TreeModel insert_child/remove emits correct TreeChange
- TreeSlice expand emits DataChange::ItemsInserted for newly visible children
- TreeSlice collapse emits DataChange::ItemsRemoved for hidden children
- Two TreeSlices on the same TreeModel have independent expand states
- TreeView expand/collapse toggles child visibility
- DragPayload with multiple MIME types is accepted by a target declaring any one of them
- DragSource produces preview widget; preview overlay follows pointer during drag
- DropTarget hover feedback: insertion line appears between ListView items at correct Y position
- DropTarget hover feedback: highlight rectangle appears on container drops
- ListView intra-widget reorder via drag emits `ListModel::move_item`
- TreeView intra-widget reorder via drag emits `TreeModel::move_node`
- Inter-widget drag from ListView A to ListView B transfers the typed payload
- Drop target that rejects payload (MIME mismatch) shows no feedback and does not accept drop
- Drag that leaves the window without a valid target cancels cleanly
- Escape during drag cancels and emits no command
- Keyboard Alt+Arrow on ListView/TreeView emits the same reorder command as a drag
- Cross-application drag from external file manager delivers `text/uri-list` payload to compatible drop targets (platform-specific integration test)
- AccessKit: List/ListItem with position_in_set/size_of_set, Tree/TreeItem with expanded/level

---

## Milestone 7: Internationalization

**Goal:** Full i18n support with Fluent translations, runtime language switching, and RTL layout. Scheduled before the Rich Text Editor so that editor labels, toolbar strings, command names, and accessibility descriptions are translatable from the first commit, rather than being retrofitted.

**Delivers:**

fern-i18n: Fluent bundle loading, tr! macro, locale management. FluentBundle-per-locale caching. Resource file discovery from application-configurable paths.

Runtime language switching: CommandContext::set_locale() triggers composite rebuild across all windows. Active widgets re-query tr! and get new strings.

RTL layout direction: HStack child reversal, Leading/Trailing resolution. The LayoutDirection enum already exists — this milestone adds the runtime switching and the widget-level reactivity.

Shortcut label localization: ShortcutFormatter in fern-i18n. Ctrl->Strg in German, Ctrl->Cmd (⌘) on macOS. MenuItem and Tooltip auto-query ShortcutFormatter rather than displaying the raw `Ctrl+X` string (replacing the current TODO in shortcut.rs).

Built-in FernUI accessibility string translations: default-shipped .ftl files for the accessibility strings that the framework generates (scroll bar, tab list, menu item, etc.) so that applications do not have to redefine common framework strings.

Locale environment propagation: `ctx.locale()` returns the active Locale. Widgets can react to locale changes via `ctx.effect(&locale, |_| ...)`.

**Blocked by:** Nothing. Can be developed in parallel with any other post-Milestone-6 work. The composite rebuild mechanism is already stable.

**Tests:**
- Language switch updates all visible text via composite rebuild
- RTL layout reverses HStack children
- Leading/Trailing resolves correctly per LayoutDirection
- Shortcut labels format correctly per platform and locale ("Ctrl+S" in en-US, "Strg+S" in de-DE, "⌘S" on macOS)
- tr! macro with plurals and gender produces correct output via Fluent's built-in support
- Missing translation key falls back gracefully (to the key itself or a developer-configured default)
- MenuItem shortcut labels auto-localize via ShortcutFormatter
- Accessibility strings on framework-provided widgets come from the shipped .ftl files

---

## Milestone 8: Rich Text Editor

**Goal:** A functional rich text editor using text-document and text-typeset, with formatting toolbar and context menu. All UI strings use the tr! macro from Milestone 7.

**Delivers:**

RichTextEditor widget (fern-widgets, behind rich-text feature flag). Integration of text-document's TextCursor with FernUI's event system. Keyboard input for insertion and deletion. Mouse click/drag for cursor positioning and selection. Multi-cursor support. Formatting toolbar connected via typed commands (Bold, Italic, Heading — all labels via tr!). Context menu: Cut, Copy, Paste, Select All (all labels via tr!). Text selection rendering via text-typeset's DecorationRect. Syntax highlighting via text-document's Highlighter trait. Undo/redo integration: widget-level typing coalescing. Scrolling via text-typeset's viewport-scoped rendering inside a ScrollArea. Canvas::draw_render_frame() embedding text-typeset's output.

Clipboard: platform-level read/write of text via fern-platform (arboard crate or direct integration). Cut/Copy/Paste (Ctrl+X/C/V, ⌘X/C/V on macOS). Plain text clipboard for this milestone — rich-format clipboard (RTF, HTML) is a post-milestone refinement.

**Blocked by:** Internationalization from Milestone 7 (for toolbar labels, context menu, accessibility strings). ScrollArea from Milestone 4 (done) for editor scrolling. ContextMenu from Milestone 4 (done) for right-click menu. Clipboard integration in fern-platform.

**Tests:**
- Typing produces correct document mutations
- Formatting commands apply to selection and are reflected in rendering
- Undo reverses last operation, Redo re-applies
- Undo coalescing: consecutive character insertions grouped into one undo step
- Selection rendering matches TextCursor range
- Context menu Cut/Copy/Paste work with clipboard
- Toolbar and menu labels respond to locale changes
- Highlighter colors propagate through text-typeset to render output
- RichTextEditor accepts externally-owned TextDocument reference
- Application retains full access to TextDocument API
- AccessKit: Role::MultilineTextInput with text value, caret position, selection

---

## Milestone 9: Text Input

**Goal:** Single-line plain text editing, derived from the Rich Text Editor by constraining formatting to a single paragraph of plain text. This reverses the common GUI evolution path (plain-to-rich) — in FernUI the rich editor is the fundamental widget, and TextInput is the constrained specialization.

**Delivers:**

TextInput: Level 2 widget built on RichTextEditor from Milestone 8, with the following constraints enforced at construction:
- Single paragraph (Enter key does not insert a newline; emits `on_submit` instead)
- Single line (the debatable constraint — configurable via `multiline(bool)` builder method; default is single-line)
- No rich formatting (Bold, Italic, Heading commands disabled at the command filter level)
- Plain text representation exposed via `Signal<String>` (two-way binding with the underlying TextDocument)

Cursor rendering, selection rendering, text selection interactions, and keyboard editing are all inherited from RichTextEditor — no reimplementation.

NumberInput/SpinBox: TextInput with increment/decrement buttons and numeric validation. Composition using TextInput + Buttons. Validation rejects non-numeric input at the command filter level.

Clipboard: already implemented in Milestone 8 — TextInput reuses it directly.

**IME support is deferred** to a post-milestone refinement. The IME composition window positioning, composition text rendering, and CJK input handling require platform-specific work in fern-platform. TextInput in this milestone targets Latin-script languages; IME for CJK, Arabic, and Indic scripts is tracked separately and can be added without changing the TextInput API.

**Blocked by:** RichTextEditor from Milestone 8.

**Tests:**
- Typing produces correct text in Signal<String>
- Cursor position updates on Arrow Left/Right, Home/End
- Shift+Arrow produces correct selection range
- Double-click selects word
- Ctrl+A selects all
- Backspace/Delete remove character at cursor or selected range
- Ctrl+Backspace removes word
- Cursor blinks at correct rate via tree.advance_time()
- Enter in single-line mode does not insert newline; fires on_submit
- Formatting commands (Bold, Italic) are filtered and do not affect the document
- Copy places selected text on clipboard; Paste inserts clipboard text at cursor
- Cut removes selected text and places on clipboard
- NumberInput rejects non-numeric typed input
- NumberInput increment/decrement buttons adjust value within bounds
- AccessKit: Role::TextInput with value, text_selection, caret position

---

## Milestone 10: Multi-Window and Platform Integration

**Goal:** Multiple windows with shared state, modal/modeless dialogs using platform windows, and native menu bar integration.

**Delivers:**

Multi-window: per-window WidgetTree with shared application state (theme, locale, shortcuts, data models). Modal dialog with OS-level parent relationship. Modeless dialog with independent interaction. The multi-window infrastructure already exists in fern-app's WindowManager — this milestone validates it with real use cases.

Native menu bar: NSMenu on macOS, widget-based MenuBar (from Milestone 4) inside the window on Windows and Linux. Declarative MenuBar description through FernApp builder. Abstraction over the platform difference: the application declares its menu structure once, and FernUI routes to native on macOS and to the in-window MenuBar elsewhere.

File dialog: native open/save via rfd crate or OS APIs. Async result via EventLoopProxy.

**Blocked by:** Platform-specific Cocoa/AppKit code for macOS menu bar (goes beyond winit).

**Tests:**
- Modal dialog blocks parent window input
- Modeless dialog operates independently
- Theme change propagates to all windows
- Locale change propagates to all windows
- Command from secondary window reaches handler with correct source_window
- Closing window cleans up resources
- Focus returns to parent after modal dismissal
- Native menu bar on macOS mirrors the declared MenuBar structure
- File dialog returns selected path asynchronously without blocking the event loop

---

## Summary

| # | Milestone | Status | Key Capability |
|---|-----------|--------|----------------|
| 1 | Button in a Window | ✅ Done | Full vertical slice, rendering pipeline |
| 2 | Text and Layout | ✅ Done | Layout engine, text, theme switching |
| 3 | Core Widget Catalog + V2 Migration | ✅ Done | Form controls, display, layout utilities, unified Widget trait, Signal<T> |
| 4 | ScrollBar, ComboBox, Menus | ✅ Done | Overlay-based interactive widgets, ScrollArea overlay/permanent modes, MenuBar |
| 5 | Tabs, SplitView, Dialogs | Next | Application structure, modal behavior, focus trapping, DragRecognizer |
| 6 | Data-Driven Collections and Drag & Drop | Planned | ListModel, TreeModel, ListView, TreeView, Repeater, SelectionModel, full DnD system |
| 7 | Internationalization | Planned | Fluent, tr!, RTL, shortcut localization (before rich text so labels translatable) |
| 8 | Rich Text Editor | Planned | text-document integration, formatting, undo, clipboard |
| 9 | Text Input | Planned | Plain-text specialization of RichTextEditor; IME deferred |
| 10 | Multi-Window and Platform | Planned | Native menus, file dialogs, multi-window |
