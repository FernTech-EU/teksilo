<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Drag and Drop

**Companion to:** [architecture.md §14](architecture.md), [events-and-gestures.md](events-and-gestures.md)
**Scope:** The full DnD lifecycle — source-side handlers, target-side handlers, preview overlay, coordinate conventions, auto-scroll / spring-loaded folders, keyboard equivalence, and how `ListView` / `TreeView` use it.

---

## 1. What DnD means here

Three distinct user stories share the same mechanics in Bastyde:

1. **Intra-widget reordering** — drag a row inside a list or a node inside a tree. No serialisation; the payload is a typed Rust value.
2. **Inter-widget transfer** — drag a row from one list into another, or drop a file shortcut onto a bookmarks bar. Also a typed payload, possibly with a MIME-annotated byte representation for adapter layers.
3. **External (OS) drops** — accept files / text / URLs dragged in from a file manager or another app. Built on the same primitives plus a per-OS `ExternalDndBackend`. **Inbound is shipped** on every desktop target (macOS verified; Windows OLE, Wayland `wl_data_device` and X11 XDND cfg-gated) — see [§11](#11-external-os-drag-and-drop--drops-from-outside-the-app) and the `DropZone` widget.
4. **External (OS) export** — drag a file / text / URL *out* of a Bastyde window into another application. **Shipped** on every desktop target (macOS and Wayland verified; Windows OLE and X11 XDND cfg-gated) — the source needs no new API: a normal `start_drag` whose payload carries MIME data auto-escalates to a native OS drag when the pointer leaves the window. See [§11.5](#115-outbound-export-app--os).

All flows reuse the same payload type, the same handler set, and the same gesture recognizer. Only the source of the events differs (in-app gesture vs. OS backend).

## 2. Payload — `DragPayload`

Every drag carries a [`DragPayload`](../crates/bastyde-core/src/drag_payload.rs). It can hold:

- **A typed Rust value** — stored via `DragPayload::typed(value)`, retrieved via `payload.get_typed::<T>()` / `take_typed::<T>()` or probed with `has_typed::<T>()`.
- **Zero or more MIME-annotated byte representations** — added via `with_mime(mime_type, bytes)`, queried via `mime_types()` / `get_mime(mime)`. Populated for external (OS) drops (e.g. `text/uri-list`, `text/plain`) alongside the typed `files()` / `text()` / `uris()` accessors; see [§11](#11-external-os-drag-and-drop--drops-from-outside-the-app).

Typed payloads are fast-path: no serialisation, sender and receiver just agree on a Rust type. Drop targets check acceptance during hover without touching the bytes:

```rust
// On the source:
let payload = DragPayload::typed(MyRowId(i));
ctx.start_drag_with_preview(source_id, payload, preview_widget);

// On the target:
handlers = handlers.on_drag_hover(move |payload, pos, _ctx| {
    if payload.has_typed::<MyRowId>() {
        DropFeedback::InsertionLine { y: insertion_y, width: w }
    } else {
        DropFeedback::NoFeedback
    }
});
```

## 3. Starting a drag — source side

A widget becomes a drag source by attaching `on_drag`. The framework auto-wires a `DragRecognizer` (press → 5 px threshold → recognise) into the widget's gesture arena and fires `on_drag` with a `DragPhase`:

```rust
use bastyde::core::gesture::DragPhase;

handlers = handlers.on_drag(move |phase, ctx| {
    if let DragPhase::Started { .. } = phase {
        ctx.start_drag_with_preview(
            self_id,
            DragPayload::typed(RowRef { index: i }),
            Box::new(build_preview(i)),
        );
    }
});
```

Two source APIs on [`EventContext`](../crates/bastyde-core/src/widget.rs):

| Call | Effect |
|---|---|
| `start_drag(source, payload)` | Start a drag with no visible preview. Cursor still turns into `Grabbing`; target-side feedback still fires. Useful for "abstract" drags where the row itself doesn't move (e.g. colour pickers). |
| `start_drag_with_preview(source, payload, Box<dyn Widget>)` | Same, plus a floating overlay that tracks the pointer. `ListView` / `TreeView` use this — they re-invoke their delegate for the dragged row and wrap it in a raised panel (see [crates/bastyde-widgets/src/drag_preview.rs](../crates/bastyde-widgets/src/drag_preview.rs)). |

Three cursor / capture invariants the framework guarantees for the source:

- `current_cursor` switches to `CursorIcon::Grabbing` at drag start and resets to `Default` on drop / cancel / source-destroyed.
- Pointer capture is installed on the source's wrapper automatically via the recognizer's `ctx.capture_pointer()` call. The source keeps receiving `PointerMove` / `PointerUp` even when the cursor leaves its bounds.
- `Escape` cancels: the preview overlay is dismissed and `on_drag_leave` fires on the current target before the session is cleared.

## 4. Dropping — target side

A widget becomes a drop target by attaching at least `on_drag_hover` or `on_drop`. Target hit-testing uses [`find_drop_target_at_or_above`](../crates/bastyde-core/src/widget_tree/drag_drop_impl.rs): hit-test the pointer position, walk up until a node with either handler is found.

Target-side handlers fire in this strict order, each at most once per role per drag:

### 4.1 `on_drag_hover(payload, local_pos, ctx) -> DropFeedback`

Fires on every `PointerMove` while this widget is the current drop target. Two jobs:

1. **Decide acceptance.** Inspect `payload` via `has_typed` / `mime_types`. If this target doesn't want this payload, return `DropFeedback::NoFeedback` (and make sure your `on_drop` also rejects it — nothing enforces consistency).
2. **Provide visual feedback.** Return `DropFeedback::InsertionLine { y, width }` (between-items insertion) or `DropFeedback::HighlightRect { rect, color }` (drop-into container). The framework stores the descriptor on the active session for anything that wants to render it; the widget itself is responsible for actually drawing the feedback in its `paint()`.

```rust
handlers = handlers.on_drag_hover(move |payload, pos, _ctx| {
    if !payload.has_typed::<RowRef>() {
        feedback_signal.set(None);
        return DropFeedback::NoFeedback;
    }
    let insertion_y = compute_insertion_y(pos, scroll.get(), item_height);
    feedback_signal.set(Some((insertion_y, content_width)));
    DropFeedback::InsertionLine { y: insertion_y, width: content_width }
});
```

**Coordinates are target-local** — origin at the target widget's top-left, in logical pixels. Same coordinate system as the target's own `bounds` / `paint`, so drop-index math doesn't have to know where the widget sits in the window. (Before we fixed this the indicator was offset by the header height; see the regression test `on_drag_hover_and_on_drop_receive_widget_local_coordinates` in [drag_drop_impl.rs](../crates/bastyde-core/src/widget_tree/drag_drop_impl.rs).)

### 4.2 `on_drag_tick(local_pos, ctx)`

Fires once per layout pass while this widget is the current drop target. Use it for **per-frame behaviours that must keep progressing when the pointer is stationary**:

- **Viewport-edge auto-scroll.** When the pointer dwells inside, say, the top 32 px of a scrollable target, the widget nudges its own scroll signal down a fixed delta each frame. `ListView` / `TreeView` ship this — see the `on_drag_tick` handler block in [list_view.rs](../crates/bastyde-widgets/src/list_view.rs).
- **Spring-loaded folders.** `TreeView` records which flat row the pointer sits over in `on_drag_hover`, together with the time it first saw it. `on_drag_tick` checks elapsed time against `SPRING_DELAY_MS = 700`; after the dwell, a collapsed branch auto-expands so the user can drop into its children.

```rust
handlers = handlers.on_drag_tick(move |pos, _ctx| {
    // edge-scroll (top/bottom 32 px ramp, max 12 px/frame)
    // spring-open (700 ms dwell → expand(node))
});
```

`on_drag_tick` is the only DnD hook that isn't event-driven. The framework fires it from [`WidgetTree::layout`](../crates/bastyde-core/src/widget_tree/layout_impl.rs) itself, right after the animation scheduler tick.

### 4.3 `on_drag_leave(ctx)`

Fires exactly once when this widget stops being the current drop target. The framework emits it for **all four** leave scenarios:

| Scenario | Trigger |
|---|---|
| Pointer moved to a different target | `handle_drag_move` detects `prev_target != drop_target` |
| Drop completed on this or another target | `handle_drag_drop`, before `on_drop` runs |
| Drag cancelled | `Escape` key, or `EventContext::cancel_drag()` |
| Source destroyed mid-drag | `revalidate_interaction_state` after the arena loses the source widget |

**Widgets MUST clear their feedback state in `on_drag_leave`.** The framework owns the session state but never touches widget-owned `Signal`s or `Cell`s. `ListView` / `TreeView` clear their `drop_feedback` signal here; that's what makes the insertion line vanish the instant the pointer exits.

```rust
let feedback_for_leave = self.drop_feedback.clone();
handlers = handlers.on_drag_leave(move |_ctx| {
    feedback_for_leave.set(None);
});
```

### 4.4 `on_drop(payload, local_pos, ctx) -> bool`

Fires on `PointerUp` only if this widget is the drop target at the release position. Already preceded by `on_drag_leave`, so by the time `on_drop` runs the feedback state is cleared. Return `true` if the drop was accepted.

```rust
handlers = handlers.on_drop(move |mut payload, pos, _ctx| {
    if let Some(drag_data) = payload.take_typed::<RowRef>() {
        apply_reorder(drag_data, pos);
        true
    } else {
        false
    }
});
```

Whether the drop is "accepted" has no framework-observable side effect today — the payload is dropped (Rust `Drop`) regardless, and no user-visible state hangs off the return. The `bool` is an extension point for future listener APIs.

### 4.5 Drop-target bubbling — nested targets

When drop targets nest (a per-row `DropTarget` inside a reorderable
`ListView`, a cell target inside a table), a hover doesn't stop at the deepest
one. The framework walks **up** from the hit target through successive drop
targets, firing each one's `on_drag_hover`, and stops at the first that
**engages** — returns a non-`NoFeedback` response (`is_engaged()`):

- A target that returns `DropFeedback::NoFeedback` does **not** want this
  payload, so the drag **bubbles** to the next drop target above it. This is
  what lets a reorderable view behind a per-row `DropTarget` still receive a
  drag the row rejected.
- If an ancestor engages, every rejecting target passed on the way up is
  cleared (`on_drag_leave`) so none leaves a stuck "forbidden" border — the
  drag is accepted above them.
- If **nothing** engages, the drag is genuinely rejected: the *deepest* drop
  target keeps its own reject affordance and becomes the tracked target;
  ancestors above it are cleared.

A target with an `on_drop` but no `on_drag_hover` engages **optimistically**
(`Accept`, no visual), so it can still receive the drop — `on_drop` makes the
final call on release. The same engage-or-bubble walk runs on `PointerUp`, so
the drop lands on whichever target the hover settled on.

## 5. The preview overlay

When a drag starts with `start_drag_with_preview`, the framework:

1. Inserts the preview widget as a root via `add_boxed` — which runs `build()`, so composite preview widgets actually instantiate their child subtrees. (Plain `arena.insert` doesn't run build; using it here leaves the preview rendering an empty widget, which is what "no floating indicator" looked like before we fixed it.)
2. Creates an overlay with `OverlayLayer::InTree` + `OverlayPlacement::AtPointer(Point::ZERO)`.
3. Marks the preview content `needs_layout` so the next layout pass runs `position_overlays` and actually positions the overlay at the pointer rather than leaving it at `(0, 0)`.

On every `PointerMove` during drag, [`handle_drag_move`](../crates/bastyde-core/src/widget_tree/drag_drop_impl.rs) calls `overlay_manager.update_placement(AtPointer(position))` **and** marks the preview content `needs_layout` again — without the dirty mark, `layout()` short-circuits (`any_needs_layout()` is false) and the overlay stays pinned at its previous position.

Cleanup: `cleanup_drag_preview()` dismisses the overlay and destroys its content subtree. It runs on drop, Escape cancel, and explicit `cancel_drag`.

### The [`DragPreview`](../crates/bastyde-widgets/src/drag_preview.rs) wrapper

`ListView` / `TreeView` don't hand the raw delegate widget to `start_drag_with_preview` — they wrap it in a small `DragPreview` composite that:

- Applies a fixed `(width, height)` so a `Spacer` inside the delegate doesn't collapse under the overlay's unbounded proposal.
- Wraps the inner in `Panel::new().background(SurfaceRole::Raised).corner_radius(6.0)` so the floating row reads as picked-up against the window.

Custom widgets can use `DragPreview` too, but it's `pub(crate)` today — if you need one, either re-implement the pattern locally or open a PR to make it public.

## 6. Scrolling during a drag

Two related interactions, both handled by the framework:

### 6.1 Mouse-wheel scroll over a drop target

While `active_drag` is `Some`, `WidgetEvent::Scroll` is routed to the drag session's `current_target` instead of the normally-hovered widget. The drop target's `on_scroll` handler (e.g. `ListView`'s internal one) fires, updating its scroll signal. The framework then synthesises a re-hover at the stationary pointer so drop-index math, feedback line, and preview placement all refresh against the new scroll offset. Implementation: the `WidgetEvent::Scroll` arm of the `active_drag.is_some()` match in [`dispatch_event`](../crates/bastyde-core/src/widget_tree/event_dispatch_impl.rs).

### 6.2 Viewport-edge auto-scroll

See §4.2 — this is a per-widget behaviour, not a framework one. The widget implements it in `on_drag_tick`.

## 7. Keyboard equivalence

Every drag operation should have a keyboard-accessible equivalent that emits the same semantic command. `ListView` / `TreeView` implement this via `Alt+Arrow` (in their `on_key` handler), calling the same `ListModel::move_item` / `TreeModel::move_node` that `on_drop` would call. The semantic operation is decoupled from the input gesture.

This is a **contract**, not a framework feature — nothing forces a custom drop target to provide a keyboard path. For anything accessible, you have to.

## 8. Lifecycle summary — one drag, one diagram

```text
                        ┌─────────────────────────────────────────────┐
source widget            │                                             │
───────────              │                                             ▼
  on_drag(Started)  →  start_drag[_with_preview]  →  preview overlay created
                                                        (AtPointer)
  ── cursor switches to Grabbing, pointer_captured_by = source widget ──

drop target (whichever widget find_drop_target_at_or_above returns):

  PointerMove ─► on_drag_hover(payload, LOCAL pos) ─► DropFeedback
                                │
                                ├─► on_drag_tick(LOCAL pos)   (each layout pass)
                                │         ↳ edge-scroll, spring-open
                                │
   target changes ─────────────►│ on_drag_leave (prev target)
                                │
  Escape / cancel  ─────────────►│ on_drag_leave (current target)
                                │ cleanup_drag_preview
                                │ pointer_captured_by = None
                                │ current_cursor = Default
                                │
  PointerUp on target  ─────────► on_drag_leave (current target)
                                 on_drop(payload, LOCAL pos)
                                 cleanup_drag_preview
                                 pointer_captured_by = None
                                 current_cursor = Default

scroll wheel during drag:
  Scroll ─► routed to active_drag.current_target.on_scroll
        └─► synthesised re-hover so feedback refreshes
```

## 9. `ListView` / `TreeView` as drop targets — how they wire it up

Both widgets combine every primitive above, but the **acceptance decision and
the commit are owned by the backing data source**, not the view (see
[data-source.md §3](data-source.md)). The view supplies geometry and rendering;
the source answers `can_accept` / `accept_drop`. Reading
[list_view.rs](../crates/bastyde-widgets/src/list_view.rs) and
[tree_view.rs](../crates/bastyde-widgets/src/tree_view.rs) as reference examples:

- `drop_feedback` signal (bound at `BindingLevel::RepaintOnly`) — set by `on_drag_hover`, cleared by `on_drag_leave`. Reading it in `paint()` is enough; the binding dirties the widget when the signal changes.
- `on_drag` (per item wrapper) — fires only when the source's `drag(key)` returns `CanDrag`; emits the shared **public** [`RowDragData<T> { source: ViewId, rows: Vec<usize>, items: Option<Vec<T>> }`](../crates/bastyde-widgets/src/data_views.rs) typed payload (one type for all five data views) and a `DragPreview` built by re-invoking the delegate for the dragged row. `rows` is the selection-aware dragged set; `items` is `Some` only when the view opted into [`.exportable(..)`](#12-dragging-rows-out-of-a-data-view--cross-widget-export). The identity is a kind-tagged, process-global [`ViewId`](../crates/bastyde-widgets/src/data_views.rs) so a foreign drag is never misread as a same-view reorder.
- `on_drag_hover` (on the list/tree itself) — computes the geometric `(target, position)` from local Y + scroll offset, asks the source `can_accept`, and sets the feedback signal to match the verdict (`Accept` → insertion line, `Reject` → suppress, `Redirect` → snap). `TreeView` also records the hovered node + timestamp for spring-load. The row under a given `y` is resolved by the same `PrefixSumOffsets::row_at` a click uses, so the two agree even at a zero-height row boundary — see [table-view.md "Which row a `y` coordinate resolves to"](table-view.md) for the degenerate-height tie-break `row_at` applies.
- `on_drag_tick` — edge auto-scroll (linear ramp inside a 32 px zone, max 12 px/frame). `TreeView` additionally checks the spring-load timer and expands the hovered branch after 700 ms.
- `on_drag_leave` — clears the feedback signal and the spring-load timer.
- `on_drop` — re-queries `can_accept`; if not `Reject`, routes the commit to the source's `accept_drop`. A same-view `RowDragData` is a `DragSource::SameView` the source applies (a `ListModel` reorders in place — one row via `move_item`, a multi-row block via `move_items`; a `TreeModel`-backed source `move_node`s with the cycle guard); a cross-view or OS payload arrives as `DragSource::Foreign { payload }` at the *same* `accept_drop`, which downcasts it. The same-view reorder only runs when the view is `reorderable`.
- `on_key` — Alt+ArrowUp / Alt+ArrowDown synthesize the same `RowDragData` and route it through `accept_drop`, so the keyboard contract travels the identical path.

## 10. Testing

Everything is headless. The key harness helpers (on `WidgetTree`):

| Helper | Purpose |
|---|---|
| `dispatch_event(WidgetEvent::PointerDown/Move/Up/...)` | Feed raw events |
| `advance_time(Duration)` | Advance the sim clock (used by tooltip / long-press delays) |
| `overlay_manager().len()` / `.overlay(id)` | Inspect active overlays (incl. drag preview) |
| `widget_as_any(id)` | Downcast a widget for test introspection (via the `Widget::as_any` hook) |
| `active_drag.is_some()` (pub(crate) — in bastyde-core tests only) | Check whether a session is live |

Common patterns from the existing suite:

- **Core-level lifecycle tests** ([drag_drop_impl.rs tests module](../crates/bastyde-core/src/widget_tree/drag_drop_impl.rs)) — use `FillWidget` / `InsetWidget` / `StackWidget` with handlers attached directly, drive events with `tree.dispatch_event(...)`, assert via `Rc<Cell<u32>>` counters and `tree.active_drag`. The `on_drag_leave_*` tests are the canonical examples.
- **Widget-level integration tests** ([list_view.rs](../crates/bastyde-widgets/src/list_view.rs), [tree_view.rs](../crates/bastyde-widgets/src/tree_view.rs) tests modules) — build a real `ListView`/`TreeView` with a `ListModel` / `TreeModel`, run the full gesture chain via a `drag_item` helper, assert the model's observable state (`with_item`, `root_count`) and the feedback signal via the `widget_as_any` downcast.
- **Drag across a rebuild** — `drag_survives_rebuild_triggered_by_selection` in `list_view.rs` pins the scenario where clicking a row triggers a selection-driven rebuild between `PointerDown` and the first `PointerMove`. The drag must still complete.
- **External handlers survive rebuild** — `external_handlers_survive_rebuild` in [widget_builder.rs](../crates/bastyde-core/src/widget_builder.rs) pins the handler-bucket invariant: closures attached via `SomeWidget::new().on_tap(...)` must keep firing after the widget rebuilds in place.

See [events-and-gestures.md §8](events-and-gestures.md) for the general testing patterns.

## 11. External (OS) drag-and-drop — across the app boundary

Files dragged from the file manager, or text / URLs dragged from another
application, enter through a **platform backend** and then reuse the *entire*
in-app pipeline above. There is no separate handler surface: an OS drop is just
a `DragPayload` with `origin() == DragOrigin::External`, dispatched through the
same `on_drag_hover` / `on_drag_leave` / `on_drop`. The reverse direction —
dragging *out* of the app — is covered in [§11.5](#115-outbound-export-app--os).

### 11.1 What the payload carries

For external drags, `DragPayload` exposes typed accessors instead of (or
alongside) the `text/uri-list` etc. MIME bytes:

```rust
fn on_drop(payload: DragPayload, _pos, ctx) -> bool {
    if payload.is_external() {
        for path in payload.files() { import(path); }     // &[PathBuf]
        if let Some(text) = payload.text() { paste(text); } // Option<&str>
        for url in payload.uris() { open(url); }           // &[String] (non-file)
    }
    true
}
```

`EventContext::drag_is_external()` is the same query for the `on_drag_leave` /
`on_drag_tick` handlers, which don't receive the payload.

### 11.2 The backend trait

[`ExternalDndBackend`](../crates/bastyde-platform/src/external_dnd.rs) registers
the app as the OS drop target for a window and, for each phase
(`Entered { data, position }` / `Moved` / `Left` / `Dropped { data, position }`),
posts an `ExternalDndEventPayload` through `AppEventPoster::post_external` — the
same channel file dialogs use. `bastyde-app` routes it to the window's tree and
drives `WidgetTree::{begin,update,end,cancel}_external_drag`, which construct a
`DragSession` (with `source_widget = None`, `is_external = true`, no pointer
capture, no preview overlay) and run the normal `handle_drag_move` /
`handle_drag_drop` path.

Apps opt in with `BastydeAppBuilder::install_external_dnd()`. Each window is
registered on creation and revoked on close.

### 11.3 Per-platform status

| Platform | Inbound backend | Outbound (export) | Notes |
|---|---|---|---|
| **macOS** | `NSDraggingDestination` on a transparent overlay `NSView` | `NSDraggingSource` on the same overlay | Full position + files + text + URLs. Both directions verified. |
| **Windows** | OLE `IDropTarget` (`RevokeDragDrop` winit's, then `RegisterDragDrop` ours) | OLE `IDropSource` + `DoDragDrop` (deferred off the dispatch that armed it) | Inbound: full position + formats. |
| **Wayland** | `wl_data_device` from the seat | `wl_data_source` + `start_drag` | No winit conflict (winit leaves Wayland DnD unimplemented). Both directions verified. |
| **X11** | XDND v5 via an `XdndProxy` helper window | XDND source: owns `XdndSelection`, polls the pointer, serves the selection (incl. `INCR`) | Full position + arbitrary MIME types. See §11.3.1. |

winit's own `DroppedFile` / `HoveredFile` events are *not* used: they carry no
cursor position, files only, and nothing on Wayland — insufficient for a
drop-zone widget that must hit-test position.

#### 11.3.1 X11: why a proxy window, and why no pointer grab

Two things about X11 shape the backend, and both are worth knowing before
reading [`external_dnd/x11.rs`](../crates/bastyde-platform/src/external_dnd/x11.rs).

**Inbound needs `XdndProxy`.** XDND messages are `ClientMessage`s sent with an
empty event mask, which the X protocol delivers *only* to the client that
created the destination window. winit created the toplevel and pumps its own
connection, and exposes no hook into its X event stream (`WindowExtX11` is an
empty trait) — so a second connection cannot see them, and winit's own built-in
XDND handling (files only, no position) cannot be turned off. The spec's own
answer is `XdndProxy`: a window may name another window that "should be checked
for `XdndAware` and should receive all the client messages". Bastyde creates a
1×1 `InputOnly` helper window on its own connection, marks it `XdndAware` and
self-pointing `XdndProxy` (the spec's stale-proxy guard), and points the
toplevel's `XdndProxy` at it. GTK 3/4, Qt 5/6 and Java/AWT all honour this with
the same validation, covering every mainstream toolkit and file manager.

**Outbound needs no pointer grab.** An XDND source conventionally grabs the
pointer to keep receiving motion over other applications' windows. Bastyde
cannot: X11 pointer grabs are exclusive per client, and the `ButtonPress` that
started the drag already gave winit's connection an implicit grab lasting until
release — `GrabPointer` from the backend would return `AlreadyGrabbed` *every*
time, not occasionally. It does not need one: `QueryPointer` is unaffected by
grabs and reports both position and button state, so the drag is driven by
polling the backend's own connection while the button is held.

Coordinates are converted root-physical → window-logical using the scale factor
pushed down by the app layer (`ExternalDndGuard::set_scale_factor`), since X11
has no per-window DPI to query the way Win32's `GetDpiForWindow` does.

Two protocol details are easy to get wrong and are worth stating explicitly.
When a target names an `XdndProxy`, only the *address* changes: messages go to
the proxy but must still name the real window in the `window` field, or a proxy
fronting several windows cannot route the drop (the bug Chromium tracks as
crbug.com/41278320). Conversely, target→source replies (`XdndStatus`,
`XdndFinished`) name the **source** — the recipient — with our own window in
`data[0]`; GTK routes replies by `xclient.window` and discards anything else.

Target resolution is cached on the root-child that the `QueryPointer` each tick
already performs reports, so staying over one window costs a single round trip
rather than the dozens a full tree descent plus per-ancestor property reads
would.

**Known limitation.** A source that ignores `XdndProxy` — a hand-rolled XDND
client; no mainstream toolkit does — reaches winit's built-in handler instead,
whose events Bastyde does not consume, so such a drop is ignored rather than
delivered. Raw Xt/Motif clients speak `_MOTIF_DRAG_*`, not XDND, and were never
reachable.

### 11.4 The `DropZone` widget

[`DropZone`](../crates/bastyde-widgets/src/drop_zone.rs) is the ready-made
"drop files here" target: hover accept/reject highlight, `accept_extensions`
filter, `allow_multiple` policy, `on_files_dropped` / `on_text_dropped` /
`on_urls_dropped` callbacks, and a keyboard-operable **Browse…** button (the
WCAG 2.1.1 equivalent, since an OS drag can't be keyboard-initiated). It is a
Tier-3 themable widget (`DropZoneStyle`) and announces hover / success /
rejection through a `Live::Polite` status line. Demo: `cargo run -p file-drop`.

**Accessibility note.** AccessKit models no drag/drop `Action`, and ARIA's
`aria-grabbed` / `aria-dropeffect` are deprecated. The supported pattern is
therefore live-region announcements (the status line) plus the always-present
keyboard fallback (Browse) — not a synthetic drag action.

### 11.5 Outbound export (app → OS)

Dragging a file / text / URL **out** of a Bastyde window into another
application. The model is **unified, escalate-at-boundary**: a drag is not
pre-committed to "internal" or "external" — the destination decides. `DragPayload`
already carries both representations (a typed `Box<dyn Any>` fast-path *and*
`mime_data`), so **the source side needs no new API** — a drag becomes
OS-exportable simply by populating `mime_data` (via `DragPayload::with_mime`):

```rust
row.on_drag(|phase, ctx| {
    if let DragPhase::Started { .. } = phase {
        let uri_list = format!("file://{path}\r\n");
        ctx.start_drag(
            row_id,
            DragPayload::typed(item).with_mime("text/uri-list", uri_list.into_bytes()),
        );
    }
});
```

Bastyde runs its normal in-app drag (preview overlay, `on_drag_hover` feedback).
**When the pointer leaves the window** carrying an OS-exportable payload, the
framework escalates to a native OS drag (`WidgetTree::try_escalate_to_os_drag` →
`WindowOps::begin_os_drag` → the backend's `ExternalDndGuard::begin_drag`). Drops
that never leave the window keep the typed fast-path untouched.

**Completion — `on_drag_ended`.** A single source-side hook fires once per drag,
with a `DropOutcome`:

```rust
row.on_drag_ended(|outcome, ctx| match outcome {
    DropOutcome::InApp { accepted } => { /* dropped on an in-app target */ }
    DropOutcome::OsCopy            => { /* exported to another app (copy) */ }
    DropOutcome::OsMove            => { /* exported as a move */ }
    DropOutcome::Cancelled         => { /* Escape / dropped on nothing / OS rejected */ }
});
```

The advertised OS operation is **Copy only** — never `Move` — so the destination
can't physically relocate a dragged file off disk; move-out would be an explicit
opt-in, not the baseline.

**Typed re-entry + cross-window DnD.** Once escalated, the OS owns the drag, but
the original typed payload is parked in an app-global stash for the drag's
lifetime. If the OS drag wanders back over **any** window of the app — the source
window *or another one* — that window recovers the typed payload and presents it
as a normal internal drag (so `get_typed::<T>()` works), while also exposing the
`files()` / `text()` / `uris()` view derived from the MIME (so `DropZone`-style
targets accept it too). This is what enables drag-and-drop **between two windows
of the same app**. Limitation: an in-app drop that crossed the window boundary
reports `OsCopy` (the OS's view), not `InApp` — drops that never left report
`InApp`.

**Per-platform:** macOS uses `NSDraggingSource` + `beginDraggingSessionWithItems:event:source:`
(triggering event from `NSApp.currentEvent`); Wayland uses `wl_data_source` +
`wl_data_device.start_drag` with a button-press serial captured from a `wl_pointer`
bound on the DnD thread (the `Drop` handler skips the pipe-read for a self-drag to
avoid a single-thread deadlock); X11 owns `XdndSelection` from its proxy window
and polls the pointer rather than grabbing it (§11.3.1). No target declines
(`begin_drag` returns
`false`) and the framework keeps the in-app drag alive. Demo: the "Drag OUT" rows
and "Internal drop target" in `cargo run -p file-drop`.

### 11.6 The `DropTarget` widget

[`DropTarget`](../crates/bastyde-widgets/src/drop_target.rs) is the *wrapping*
counterpart to `DropZone`: instead of being a standalone "drop here" placeholder,
it turns **any existing widget subtree** into a drop target without replacing its
look. The wrapped child fills the bounds and stays fully visible; the highlight is
a **border** stroked over the child (never an opaque fill that would hide it), plus
an optional popup hint card centered in the zone. It reacts to **both** internal
(typed `DragPayload`) and external (OS) drops through the same
`on_drag_hover` / `on_drag_leave` / `on_drop` pipeline.

```rust
// Wrap a panel; accept image files; show a hint while an accepted drag hovers.
DropTarget::new()
    .child(my_panel)
    .hint(TextWidget::new(lit!("Drop your image here")))
    .accept_external_extensions(["png", "jpg", "jpeg"])
    .on_drop(|payload, _pos, _ctx| { import(payload.files()); true });

// Typed internal drag — recovers the value even after an OS round-trip or
// across windows (§11.5 typed re-entry), since it rides the unchanged
// target-side pipeline.
DropTarget::new()
    .child(project_card)
    .on_drop_typed::<ProjectRef>(|project, _pos, ctx| {
        ctx.send_intent(AppIntent::Link(project));
        true
    });
```

**Accept filtering** (last-call-wins; default = accept everything once `on_drop`
is set): `accept_any`, `accept_external` / `accept_external_files` /
`accept_external_text` / `accept_external_extensions([…])`, `accept_typed::<T>()`,
or `accept_when(|payload| …)` for full control. The external-extension filters
mirror `DropZone`'s Wayland-aware split — optimistic at hover (file bytes haven't
arrived yet, only advertised formats), real check at drop. `on_drop` re-checks the
filter before invoking the callback (the hover gate is visual only; the framework
still routes the drop to the target).

**Caller-observable state.** `targeted_signal(Signal<bool>)` (SwiftUI's
`isTargeted` pattern — `true` only while an *accepted* drag hovers) and
`drag_state_signal(Signal<DropTargetDragState>)` (full `Idle` / `HoverAccept` /
`HoverReject`) let the surrounding UI drive its own visuals. `on_drop_typed::<T>`
implicitly sets `accept_typed::<T>()` and hands the extracted `T` to the callback.

#### Multi-zone drops

Beyond one whole-bounds target, a `DropTarget` can expose up to five
independently enable-able **regions** — `DropRegion::{Center, Top, Bottom,
Leading, Trailing}` — each with its own optional hint, and route the drop by
*where* the pointer released. This is the reusable form of `DockingLayout`'s
hand-computed five-zone drag-to-dock overlay (its `compute_drop_zone` /
`DockDropOverlay`); the pure hit-test (`region_at`) and geometry (`region_rect`)
live in `bastyde-core::styles`.

```rust
DropTarget::new()
    .child(editor_pane)
    // The four SIDE zones share one factor (0.1..=1.0): the fraction of the
    // axis each edge strip occupies. 0.2 = fifth (default), 0.5 = bisect.
    .zone_size_factor(0.25)
    .region(DropRegion::Center,   |z| z.hint(TextWidget::new(lit!("Add as tab"))))
    .region(DropRegion::Leading,  |z| z.hint(TextWidget::new(lit!("Split left"))))
    .region(DropRegion::Trailing, |z| z.hint(TextWidget::new(lit!("Split right"))))
    // Region-aware drop (wins over on_drop); also `.active_region_signal(..)`.
    .on_region_drop(|region, payload, _pos, ctx| { route(region, payload); true });
```

- Declaring **any** region switches the target to exactly the declared regions;
  declaring none keeps the `Center`-only whole-bounds default (`.hint(w)` is
  sugar for `.region(DropRegion::Center, |z| z.hint(w))`).
- Each zone takes a reactive `z.enabled(signal)` (default `true`): a bound
  `Signal<bool>` disables the zone **live, without a rebuild** — it stops
  hit-testing (its strip falls through to the next-priority enabled zone, or
  `Center`, or rejects), never highlights, and never shows its hint.
- `region_at` classifies the *target-local* pointer (§4.1) against the currently
  **enabled** zones — side zones are `size_factor`-thick strips tested in
  leading→trailing→top→bottom priority; a middle covered by no enabled zone
  resolves to `Center` when enabled, else the drop is **rejected** (the hover
  never engages there, and `on_region_drop` only ever receives an enabled region).
- The active zone highlights (**centre → frame only** so the wrapped content
  shows through; an **edge strip → translucent fill + accent frame**) and only
  that zone's hint appears, centered within the zone rect. Accept uses this
  per-zone overlay; a reject paints a full-bounds error border.
- `Leading` / `Trailing` map to left / right — the framework surfaces no writing
  direction on the layout context yet, so RTL mirroring is a follow-up.

**Styling.** Tier-3 `DropTargetStyle` (default `RecipeDropTargetStyle`); per-call
`DropTarget::style(…)` or theme-wide `theme.style_slots.drop_target`.
`DropTargetVariant` (`Default` 2 px / `Prominent` 3 px / `Subtle` 1 px / `None`)
sets the highlight-frame weight. Each hint is gated with `visible_when` on a
derived "is *this* region the active accepted-hover?" signal, so an inactive
zone's hint is culled from paint **and** the accessibility tree; `Live::Polite`
on the card announces it appearing.

**Accessibility.** `Role::Group`. Unlike `DropZone`, `Live` is *not* placed on the
group itself (that would announce every change to the wrapped child) — it is scoped
to each hint card. There is no Browse fallback: `DropTarget` wraps arbitrary content,
which provides its own keyboard affordances.

Demo: the "Internal drop target" panel in `cargo run -p file-drop` is a single-zone
`DropTarget` recovering a typed `String`; `cargo run -p drag-and-drop` adds a
multi-zone target (leading = play next / centre = add / trailing = favourite, each
with its own hint).

## 12. Dragging rows OUT of a data view — cross-widget export

Sections 9 and 4 cover a row **reordering within its own view** and a source
owning `can_accept` / `accept_drop`. This section is the other direction:
letting a user drag row(s) **out** of a `ListView` / `TreeView` / `TableView` /
`TreeTableView` / `GridView` and drop them **elsewhere** — on a
[`DropTarget`](#116-droptarget), a [`DropZone`](#114-the-dropzone-widget),
another data view, or the OS. All five views share one opt-in builder surface.

### 12.1 The payload — `RowDragData<T>`

Every data-view row drag carries the public, generic
[`RowDragData<T>`](../crates/bastyde-widgets/src/data_views.rs):

```rust
pub struct RowDragData<T: 'static> {
    pub source: ViewId,          // kind-tagged, process-global identity
    pub rows: Vec<usize>,        // origin's flat visible indices (drag-start)
    pub items: Option<Vec<T>>,   // clones — Some only for an export drag
}
```

It occupies the single typed slot of the [`DragPayload`](#2-payload--dragpayload)
and serves both audiences: the origin's own erased classifier reads
`source` + `rows` to recognise a same-view reorder; a **foreign** receiver reads
`items`. A plain `.reorderable(true)` drag carries `items == None`, so a
reorder-only view is never accidentally consumed elsewhere — a receiver gates on
[`RowDragData::is_export()`](../crates/bastyde-widgets/src/data_views.rs).

### 12.2 Send side — opting rows into export

| Builder (on every data view) | Effect |
|---|---|
| `.exportable(DragTransferMode)` (`where T: Clone`) | Carry `items` clones so a foreign target gets typed rows; also makes rows a drag source **without** `reorderable`. `Move` removes the origin rows once a foreign target accepts them; `Copy` keeps them. |
| `.export_external(\|&[T]\| -> Vec<(String, Vec<u8>)>)` (`where T: Clone`) | Additionally advertise MIME (`text/plain`, `text/uri-list`, an app `application/x-…`) so a `DropZone` / the OS can take the drag. Implies `.exportable`. |
| `.on_rows_transferred_out(\|&[usize], ctx\|)` | Override the `Move` removal (rows are delivered **descending** so index-by-index removal stays valid). Default: the source's `on_drag_out`. |

The dragged set is **selection-aware**: pressing an already-selected row keeps
the whole multi-selection (the collapse-to-one is deferred to a release without
a drag), so dragging one member of a selection exports them all. Rows whose item
isn't resident (a lazy `Loading` row) are dropped from the export so `rows` and
`items` stay aligned.

### 12.3 Receive side

- **A `DropTarget` / `DropZone` / any widget elsewhere** — already works: name the same `T` and read it:

  ```rust
  DropTarget::new()
      .accept_when(|p| p.get_typed::<RowDragData<Chapter>>().is_some_and(|d| d.is_export()))
      .on_drop(|mut p, _pos, _ctx| {
          if let Some(items) = p.take_typed::<RowDragData<Chapter>>().and_then(|d| d.items) {
              trash.extend(items); return true;
          }
          false
      })
      .child(trash_bin);
  ```

- **Another data view** — two ways: (a) a **custom `ListDataSource`/`TreeDataSource`** whose `can_accept`/`accept_drop` inspect the `DragSource::Foreign { payload }` (§9); or (b) the zero-custom-source sugar `.accept_foreign_rows(true)` + `.on_rows_received(\|Vec<T>, insertion_index, ctx\|)` on the receiving view, which inserts the dropped items into your model.
- **`TreeTableView`** is not source-pluggable (it wraps a concrete `SortFilterTreeModel<T>`), so it exposes a raw escape hatch in addition to the typed sugar: `.on_foreign_drop(\|&DragPayload, target: NodeId, DropPosition, ctx\| -> bool)`.

### 12.4 Completion (move-vs-copy)

The framework delivers the outcome to the source's `on_drag_ended`. The view sets
a `self_reorder_flag` when *it* handled a same-view reorder, so a `Move` only
removes rows that a **foreign** target accepted — never double-removing after an
own reorder. **Move caveats:** removal fires for an in-app drop *in the same
window* (`DropOutcome::InApp { accepted: true }`) or a genuine OS move; shipped
OS backends advertise **copy only**, so a drag exported to another application or
another window is a *copy* (the origin row is kept — see §13). For a
`ListModel`-backed view (key == index) the move-out removes by drag-start
indices; mutate a shared model mid-drag and use `.on_rows_transferred_out` with
your own stable identity.

### 12.5 Correctness notes

`ViewId` is a process-global, kind-tagged id (so a `ListView` and a `TreeView`
can never collide and misread a foreign drag as a same-view reorder). Multi-row
same-view reorder lands the block **contiguously** (`ListModel::move_items`
emits `ItemsMoved` so index selection follows; trees re-anchor and drop
descendants of another dragged node, and reject a drop *into* a dragged
subtree). See the integration tests in
[list_view.rs](../crates/bastyde-widgets/src/list_view.rs) (`exportable_*`,
`accept_foreign_rows_receives_from_another_view`,
`two_views_over_same_model_do_not_spuriously_reorder`).

## 13. Non-goals — what DnD does NOT do yet

- **Cross-window / re-entry *move* semantics.** A drop that crossed the window boundary reports `OsCopy`, never `OsMove`/`InApp` — the source can't know to delete its item. True app-internal move across windows would need a private-MIME handshake beyond the current Copy-only export. (A data-view `.exportable(Move)` drag therefore behaves as a copy across the window boundary — see §12.4.)
- **A drag icon on X11.** XDND has no drag image in the wire protocol; GTK and Qt each create their own override-redirect window and reposition it per motion, which needs an ARGB visual and a running compositor to avoid drawing a black rectangle. Bastyde changes the cursor instead, so `DragImageData` is ignored on X11.
- **Non-`XdndProxy` X11 sources.** See §11.3.1 — a source that ignores the proxy reaches winit's built-in handler and its drop is not delivered. No mainstream toolkit is affected.
- **`Opacity` primitive for previews.** The current `DragPreview` uses a raised surface — no transparency. Opacity is a separate widget-primitive enhancement.
- **Public `preview_builder(..)` on ListView / TreeView.** Today the preview is always a delegate-built `DragPreview`. Apps that need a differently-styled preview have to re-implement the full reorderable widget or wait for the builder API.

## See also

- [architecture.md §14 Drag and Drop](architecture.md) — the design rationale and the three DnD scenarios.
- [events-and-gestures.md §4 Gesture recognizers](events-and-gestures.md) — how `DragRecognizer` fits into the gesture arena.
- [data-models.md §8 Drag-and-drop integration](data-models.md) — how `ListModel::move_item` / `TreeModel::move_node` / `DataChange::ItemsMoved` / `TreeChange::NodeMoved` plug into the drop handlers.
- [shortcut-intent-action.md](shortcut-intent-action.md) — when a drop should fire a typed `Intent` instead of mutating a model directly.
- [crates/bastyde-core/src/drag_payload.rs](../crates/bastyde-core/src/drag_payload.rs), [drag_state.rs](../crates/bastyde-core/src/drag_state.rs) — the framework types.
- [crates/bastyde-widgets/src/list_view.rs](../crates/bastyde-widgets/src/list_view.rs), [tree_view.rs](../crates/bastyde-widgets/src/tree_view.rs), [drag_preview.rs](../crates/bastyde-widgets/src/drag_preview.rs) — the canonical widget integrations.
- [crates/bastyde-platform/src/external_dnd.rs](../crates/bastyde-platform/src/external_dnd.rs) — the external (OS) drag backend trait (`ExternalDndGuard::begin_drag` for outbound), handle, and macOS / Wayland / no-op / memory backends; [external_dnd/macos.rs](../crates/bastyde-platform/src/external_dnd/macos.rs) (`NSDraggingSource`), [external_dnd/wayland.rs](../crates/bastyde-platform/src/external_dnd/wayland.rs) (`wl_data_source`); [drop_zone.rs](../crates/bastyde-widgets/src/drop_zone.rs) — the standalone `DropZone` widget; [drop_target.rs](../crates/bastyde-widgets/src/drop_target.rs) — the wrapping `DropTarget` widget (§11.6).
- Outbound escalation + typed re-entry live in [crates/bastyde-core/src/widget_tree/drag_drop_impl.rs](../crates/bastyde-core/src/widget_tree/drag_drop_impl.rs) (`try_escalate_to_os_drag`, `handle_os_drag_ended`, the global typed-payload stash); `DropOutcome` / `OutboundDragData` / `DragImageData` / `DragPayload::{to_outbound,is_os_exportable,enrich_external_from_mime}` in [drag_payload.rs](../crates/bastyde-core/src/drag_payload.rs); `WindowOps::begin_os_drag` in [window/ops.rs](../crates/bastyde-core/src/window/ops.rs).
- [examples/drag_and_drop](../examples/drag_and_drop/) — runnable in-app DnD demo; [examples/file_drop](../examples/file_drop/) — external (OS) drop demo.
