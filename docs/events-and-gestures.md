<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Events and Gestures

**Companion to:** [architecture.md](architecture.md)
**Scope:** How input becomes widget behavior in Teksilo — attached handlers, preview/bubble dispatch, gesture recognizers, and the `EventContext` deferred-operations pattern.

---

## 1. What we designed for

The event system has to handle three unrelated things cleanly:

1. **Raw input** from the platform — pointer moves, key presses, scroll, IME composition, trackpad pinch.
2. **Recognized gestures** composed from raw events — tap, double-tap, long-press, drag, swipe.
3. **Accessibility actions** — a screen reader or automation tool asking the widget to do something (click, set value, set selection) without any pointer or keyboard at all.

The V1 design unified these behind a single `fn event(&mut self, event: &WidgetEvent, ctx: &mut EventContext) -> EventResponse` method on every `Widget`. Every widget wrote one giant `match` statement on the event enum. This worked, but it forced a pile of incidental complexity:

- Gesture recognizers had to be instantiated per-widget by hand.
- The `RefCell<Option<State<T>>>` pattern was mandatory to mutate state created during `build()` from inside `event()`.
- Composition was painful: wrapping a widget and also listening for taps meant the wrapper had to re-dispatch the inner's events manually.
- Unused handler slots still cost the dispatcher a virtual call per widget per event.

V2 replaces the single method with attached handlers. Widget *builders* register typed closures for the specific events they care about; the framework stores those closures on the arena node and dispatches them automatically. The `Widget` trait itself has no `event()` method anymore.

## 2. Preview and bubble — the two-pass model

Every event that targets a specific widget (via hit testing for pointer events, via the focused widget for keyboard events, via the target node for AccessKit actions) travels through the tree twice:

- **Preview pass:** root → target. Each ancestor gets a chance to consume the event before the target sees it. A `MenuList` overlay uses the preview pass to intercept Arrow keys before any menu item sees them; a modal scrim uses it to swallow pointer events that fall outside the modal. Preview handlers return `EventResponse::Handled` to stop the pass.
- **Bubble pass:** target → root. The target handles the event first; if it returns `Ignored`, the event walks up the parent chain until something handles it or the root is reached. This is how a `Button`'s `.on_key(Key::Space)` handler can be registered on the button itself, but Ctrl+S falls through to a root-level `Action`.

```text
     root
      │   preview: root first
      ↓
   ancestor
      │
      ↓
    parent
      │
      ↓
   target ← event fires here
      │   bubble: target first
      ↑
    parent
      │
      ↑
   ancestor
      │
      ↑
     root
```

Implementation is a single walk per pass in [widget_tree/event_dispatch_impl.rs](../crates/teksilo-core/src/widget_tree/event_dispatch_impl.rs): `dispatch_to_widget_returning_handled(target, &event)` collects ancestors, runs preview top-down, then runs bubble target-up, returning on the first `Handled`.

The framework decides what "target" means per event type:

- **Pointer events** (`PointerDown`, `PointerMove`, `PointerUp`, `PointerEnter`, `PointerLeave`) — hit-tested against layout bounds. The deepest hit wins. Preview runs from the root to that hit; bubble walks back up.
- **Scroll events** — hit-tested at the pointer position; bubble to the nearest `on_scroll` that returns `Handled` (a scroll container typically).
- **KeyDown / KeyUp / IME** — routed to the focused widget. Preview from root down, bubble focused-widget up.
- **AccessKit actions** — routed to the target node directly. No pointer, no focus — the platform's AccessKit request carries a `NodeId`. No preview pass; handler runs on the target only, then bubbles.

There is no "capture phase" distinct from preview, no event replay, no explicit listener list. The tree structure is the listener list.

## 3. Attached handlers

Widget builders register event handlers via blanket-implemented methods on the [`WidgetBuilder`](../crates/teksilo-core/src/widget_builder.rs) trait. Every widget gets them for free:

```rust
ctx.add(
    MinSize::new(48.0, 48.0).child(content)
        .on_tap(|event, ctx| {
            // event is &TapEvent { position, button, modifiers }
            ctx.send_intent(AppIntent::Clicked);
        })
        .on_hover(move |entered, _ctx| {
            interaction.set(if entered {
                InteractionState::Hovered
            } else {
                InteractionState::Idle
            });
        })
        .focusable(true)
        .cursor(CursorIcon::Pointer)
);
```

Under the hood, the builder wraps the widget in a `WidgetWithHandlers<W>` that carries a `HandlerSet`. On arena insertion the handlers are moved onto the `WidgetNode`; the wrapper evaporates. At dispatch time the framework looks up the relevant closure on the node and calls it with the event data and an `EventContext`. Absent handlers are `None` and cost nothing.

### 3.1 Handler catalogue

[`event_handlers.rs`](../crates/teksilo-core/src/event_handlers.rs) defines the full set. Summarized:

| Handler | Fires when | Signature (simplified) |
|---|---|---|
| `on_tap` | A single primary-button tap completes | `FnMut(&TapEvent, &mut EventContext)` |
| `on_double_tap` | Two taps within 300 ms, within 10 px | same |
| `on_triple_tap` | Three taps within the recognizer window | same |
| `on_long_press` | Pointer held past the long-press threshold | same |
| `on_hover` | Pointer enters / leaves the widget's bounds | `FnMut(bool, &mut EventContext)` |
| `on_focus` | Widget gains or loses focus | `FnMut(bool, &mut EventContext)` |
| `on_key` | Focused widget receives a `KeyDown` / `KeyUp` | `FnMut(&WidgetEvent, &mut EventContext) -> EventResponse` |
| `on_scroll` | Scroll event hits the widget | same |
| `on_pointer_event` | Low-level pointer escape hatch (any `Pointer*` variant) | same |
| `on_drag` | Gesture-based drag — `Started`, `Moved*`, `Ended` phases | `FnMut(DragPhase, &mut EventContext)` |
| `on_swipe` | One-shot swipe with direction + velocity | `FnMut(SwipeDirection, f32, &mut EventContext)` |
| `on_pinch` | OS trackpad magnify / rotate phases | `FnMut(PinchPhase, &mut EventContext)` |
| `on_drag_hover` | DnD payload hovers over the widget | `FnMut(&DragPayload, Point, &mut EventContext) -> DropFeedback` |
| `on_drag_leave` | Drag leaves the widget (target change, drop, cancel, or source destroyed) | `FnMut(&mut EventContext)` |
| `on_drag_tick` | Per-frame tick while the widget is the current drop target | `FnMut(Point, &mut EventContext)` |
| `on_drop` | DnD payload released on the widget | `FnMut(DragPayload, Point, &mut EventContext) -> bool` |
| `on_access_action` | AccessKit action request targets the widget | `FnMut(accesskit::Action, &mut EventContext) -> EventResponse` |
| `on_access_action_request` | Full AccessKit action with payload (`SetTextSelection`, `SetValue`, `SetScrollOffset`) | see source |

### 3.1.1 `TapEvent` — button + modifiers in the callback

The four click-style handlers (`on_tap` / `on_double_tap` / `on_triple_tap` / `on_long_press`) all receive a borrowed [`TapEvent`](../crates/teksilo-core/src/gesture.rs):

```rust
#[non_exhaustive]
pub struct TapEvent {
    pub position: Point,         // widget-local coords
    pub button: PointerButton,   // which button finalised the gesture
    pub modifiers: Modifiers,    // held at the finalising event
}
```

This lets a single handler discriminate by mouse button and modifier without falling back to `on_pointer_event`:

```rust
.on_tap(|event, ctx| match (event.button, event.modifiers) {
    (PointerButton::Primary, Modifiers::SHIFT) => extend_selection(ctx),
    (PointerButton::Primary, Modifiers::CTRL)  => toggle_selection(ctx),
    (PointerButton::Primary, _)                => set_selection(ctx),
    (PointerButton::Secondary, _)              => show_quick_actions(ctx),
    _ => {}
})
```

Modifiers come from the finalising event — `Up` for `on_tap` / `on_double_tap` / `on_triple_tap`, the held `Down` for `on_long_press` (which recognises on a timer before any `Up`). The struct is `#[non_exhaustive]` so future fields don't break match patterns or constructors.

> **Coordinate space (framework invariant).** Every pointer / gesture position delivered to a handler — `on_tap` / `on_double_tap` / `on_triple_tap` / `on_long_press`, `on_drag` (`DragPhase`), and `on_pointer_event` (`PointerDown` / `Move` / `Up`) — is in that handler's **widget-local** space (relative to the node's top-left, with any ancestor `Scale` / `Rotate` transform undone). The framework converts once at dispatch via `WidgetArena::local_pointer_position`; widgets must **not** subtract their own bounds origin. (A `content_transform` node such as `SceneView` is the one exception: it owns its view transform and receives positions in its parent-effective space.) The drag-and-drop drop callbacks (`on_drop` / `on_drag_hover` / `on_drag_tick`) and the `context_menu` factory are dispatched on a separate path and likewise receive widget-local / window-local positions as documented at their own call sites.

### 3.1.2 Button-acceptance filter — default Primary, opt-in to more

Each of the four recognizers defaults to [`ButtonMask::PRIMARY`] — left-click only. A right-click on a `Button`, `Checkbox`, `MenuItem`, etc. does **not** activate the widget; it can still open a context menu via `.context_menu(...)` or be handled directly via `on_pointer_event`. Multi-tap recognizers further require every tap in the sequence to use the same button — mixed-button sequences fail rather than spuriously firing.

To opt a handler into a wider button set, call the matching `accept_*_buttons(...)` knob:

```rust
Button::new(lit!("Action"))
    .accept_tap_buttons(ButtonMask::PRIMARY | ButtonMask::SECONDARY)
    .on_tap(|event, ctx| match event.button {
        PointerButton::Primary   => primary_action(ctx),
        PointerButton::Secondary => alt_action(ctx),
        _ => {}
    });
```

`ButtonMask` exposes the obvious constants and bitwise operators; `ButtonMask::ALL` is the catch-everything shorthand and `accept_any_button()` on the recognizer types is the equivalent. The same family of knobs exists for double-tap (`accept_double_tap_buttons`), triple-tap (`accept_triple_tap_buttons`), and long-press (`accept_long_press_buttons`).

The `PointerButton` enum covers `Primary`, `Secondary`, `Middle`, plus `Back` and `Forward` (mouse 4 / 5). Platforms that don't surface the auxiliary buttons simply never emit them.

### 3.1.3 Other flag-like attachments

Plus a handful of flag-like attachments that don't take event-data closures:

| Flag | Purpose |
|---|---|
| `.focusable(true)` | Opt the node into tab order |
| `.tab_index(n)` | Explicit tab index, **scoped to the nearest `FocusScope`** (see §6) — `Some` sorts before unindexed, ascending |
| `.cursor(CursorIcon::Pointer)` | Cursor when pointer is over the widget |
| `.clips_children(true)` | Scissor clipping to bounds (ScrollArea, MaxSize) |
| `.context_menu(factory)` | Right-click overlay factory — see §3.1.4 |

### 3.1.4 Context-menu factory — `Fn(Point, &mut EventContext) -> Option<Box<dyn Widget>>`

Right-click handling lives at a different tier from the four tap-family hooks. Instead of a recognizer-driven callback, the framework wires a single **factory** that produces the menu widget on demand. When the user right-clicks, the framework's `show_context_menu_for` walks up the parent chain looking for the nearest ancestor with a factory installed, calls it with the click position (widget-local) plus a full `EventContext`, and:

- Mounts the returned widget as an `OverlayLayer::InTree` overlay anchored at the factory-owning widget, placed at the click position.
- Dismisses pre-existing overlays first.
- Saves the previously-focused widget for restoration when the menu dismisses.
- Focuses the menu content so keyboard navigation works immediately.

```rust
.context_menu(|position, ctx| {
    // Use `position` to identify what was right-clicked (a row in a
    // list, a node in a tree, an item under a hit-test).
    let row = pick_row_at(position.y)?;
    // Use `ctx` to read window state, query app-state, send intents,
    // or update Signals before the menu mounts.
    ctx.send_intent(AppIntent::TelemetryRightClick { row_id: row.id });
    Some(Box::new(build_menu_for(row)))
})
```

The factory is `Fn` (re-entrant) and called fresh on every right-click — the menu's enabled / disabled flags read live state at the moment it opens, so a "Paste" item correctly greys out when the clipboard becomes empty between two right-clicks.

**Returning `None` declines the click** and the framework continues walking up the parent chain to the next ancestor with a factory. This lets a widget conditionally suppress its own menu without uninstalling the factory:

```rust
.context_menu(|_, _| if disabled.get() { None } else { Some(build_menu()) })
```

A factory that always returns `None` produces no menu and no fall-through visible effect — the right-click is consumed silently.

### 3.2 HandlerSet — handlers from inside `build()`

Attached handlers via `WidgetBuilder` methods only work on *child* widgets (`ctx.add(MinSize::new().on_tap(...))`). A composite widget that wants to install handlers on *itself* (typical for focusable containers that should swallow keyboard events) uses `HandlerSet` + `ctx.apply_self_handlers`:

```rust
fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    let handlers = HandlerSet::new()
        .focusable(self.enabled)
        .cursor(CursorIcon::Pointer)
        .on_tap(move |_event, ctx| { /* ... */ })
        .on_key(move |event, ctx| {
            if let WidgetEvent::KeyDown { key: Key::Space, .. } = event {
                // handle activation
                return EventResponse::Handled;
            }
            EventResponse::Ignored
        });
    ctx.apply_self_handlers(handlers);
    // ... then add children ...
}
```

Multiple `apply_self_handlers` calls across a widget's `build()` chain merge via `HandlerSet::merge`; two `on_tap` closures both run. This lets a composite widget compose its own behavior with a base trait's contributed handlers.

### 3.3 Why attached handlers won

The `event()`-method vs attached-handlers tradeoff flipped once three things became clear:

- **Gesture auto-wiring.** When a widget attaches `on_tap`, the framework instantiates a `TapRecognizer` in the node's gesture arena on the fly. The widget author never touches the recognizer. Under the V1 model, every tappable widget had to declare the recognizer by hand.
- **Cheap composition.** Wrapping a widget inside a `MinSize` and also listening for taps on the outer wrapper used to require a second widget with a custom `event()` impl. Now it's `.child(content).on_tap(...)` — the `MinSize` doesn't need to know its parent wrote a tap handler, because the handler is on the *wrapper's* node, not inside `MinSize`.
- **Mutation without `RefCell<Option<State>>`.** V1's `event(&mut self, ...)` required `&mut` access to state built in `build(&self)`, forcing the `RefCell<Option<State<T>>>` pattern. Attached handlers close over `Signal<T>` clones — the signal itself is clone-friendly and internally cells its own storage, so the handler closure doesn't need `&mut self`.

## 4. Gesture recognizers — composition with backpressure

[`gesture.rs`](../crates/teksilo-core/src/gesture.rs) defines the recognizer state machines. Each is a pure, platform-free value type that consumes `RawPointerEvent::{Down, Move, Up}` and emits `GestureResult::{Pending, Recognized(GestureEvent), Failed}`.

Built-in recognizers (the four click-style ones default to `ButtonMask::PRIMARY` — call `.accept_buttons(...)` / `.accept_any_button()` to widen):

- `TapRecognizer` — fires on a down-up without movement past the tap-slop threshold. Down/Up button must match.
- `DoubleTapRecognizer` — two taps within 300 ms. Both taps must use the same button.
- `TripleTapRecognizer` — three. Same button across all three.
- `LongPressRecognizer` — pointer held past ~500 ms. Modifiers are captured at `Down`.
- `DragRecognizer` — emits `DragStarted` once the pointer moves past the drag-start threshold, then `DragMoved` per move, then `DragEnded` on pointer-up.
- `SwipeRecognizer` — pointer moves fast enough to qualify as a swipe in one of four cardinal directions.

`PinchRecognizer` is *not* in the list because on desktop the OS delivers `TouchpadMagnify` / `RotationGesture` events directly (winit passes them through); the framework turns those into `PinchPhase` events without needing a recognizer.

### 4.1 GestureArena — cooperating and competing

When a widget attaches multiple gesture handlers (`on_tap` + `on_long_press`), both recognizers run in parallel on the same event stream via `GestureArena`. The arena's rules:

- Each recognizer sees every raw event until it returns `Recognized` or `Failed`.
- When one recognizes, competing recognizers whose `resets_on_peer_recognition` flag is set get reset (`DoubleTapRecognizer` peers-reset when `TapRecognizer` alone fires — so a single tap doesn't arm a phantom "missing second tap" in the double-tap recognizer).
- Cooperative recognizers (tap and triple-tap, for instance) run to completion side-by-side.

Widget authors never touch the arena directly. Attaching handlers via `WidgetBuilder` or `HandlerSet` auto-wires the recognizers and the arena on the node.

### 4.2 Cross-widget tap/drag disambiguation — drag observers

The `GestureArena` is **per-widget**; there is no cross-widget arena. That
leaves one gap: a descendant's `on_tap` installs a `TapRecognizer` that
**captures** the pointer on `PointerDown`, which would otherwise route every
following `PointerMove`/`PointerUp` to the descendant alone — so an *ancestor*
that wants to start a drag (a `SceneView` behind tappable cards, a draggable
container wrapping tappable rows) would never see the move and could never
begin its drag.

The framework closes this without cross-arena arbitration, by leaning on the
existing **`active_drag` takeover** (an in-flight `start_drag` is consulted
*before* capture routing). On `PointerDown`, after the normal dispatch, if a
descendant captured the pointer the framework **arms drag observers**: it walks
the captured widget's strict ancestors and, for each one that carries an
`on_drag` / `on_swipe` recognizer, feeds the down event into *that ancestor's
own* gesture arena (no `on_pointer_event`, no second capture). On each
subsequent `PointerMove` (while no drag is yet active) it advances those
observers; the moment one recognizes a drag it calls `start_drag`, and the
`active_drag` takeover pulls the pointer away from the descendant. If no
ancestor drag fires, the descendant's tap completes normally on `PointerUp`.

Two consequences worth knowing:

- A widget that has its **own** `on_drag` (a slider, a DnD row) is left
  untouched — the arming step skips a captured widget that already carries a
  drag recognizer, so only *pure-tap* descendants inside a draggable ancestor
  change behavior.
- A quick press-release on the card is still a **tap** (no move crossed the
  drag threshold); only a press-and-drag escalates to the ancestor. This is
  exactly what makes "drag from on top of a select-only scene card starts a
  marquee, click selects it" work (see
  [teksilo-scene.md](teksilo-scene.md) "Drag mode").

## 5. `EventContext` — the deferred-operations pattern

Handlers don't mutate the tree directly. They request mutations on their `EventContext` and the framework applies them after the dispatch finishes:

```rust
pub struct EventContext {
    // tree structure
    tree_mutations: Vec<TreeMutation>,        // SetDormant / Activate / Destroy
    // focus
    focus_requests: Vec<WidgetId>,
    // overlays
    overlay_requests: Vec<OverlayRequest>,
    overlay_dismissals: Vec<OverlayId>,
    delayed_overlay_requests: Vec<...>,
    timed_overlay_requests: Vec<...>,
    dismiss_all_overlays: bool,
    dismiss_top: bool,
    // modals
    modal_requests: Vec<ModalRequest>,
    dismiss_modal: bool,
    // repaint / layout
    repaint_requests: Vec<WidgetId>,
    // intents + shortcuts
    pending_intents: Vec<Intent>,
    pending_key_capture: Option<KeyCaptureSlot>,
    pending_shortcut_mutations: Vec<ShortcutMutation>,
    // window-level
    theme_request: Option<Theme>,
    locale_request: Option<String>,
    close_window_requested: bool,
    // cursor
    cursor_request: Option<CursorIcon>,
    // frame loop
    frame_requested: bool,
    // ...
}
```

This single-pass deferral matters for two reasons:

- **Safety.** A handler that destroys its own widget, then inspects state on that widget, would crash. Deferring the destroy until after the handler returns avoids use-after-free without runtime cost.
- **Ordering.** Multiple handlers along the bubble path can each queue mutations; the framework applies them in a well-defined order (intents first, then tree mutations, then repaints). A widget author doesn't have to reason about mid-handler tree shape changes.

### 5.1 Ambient ops available from any handler

Via `EventContext`, any handler can:

- `ctx.set_theme(theme)` — swap the app theme; all windows rebuild.
- `ctx.set_locale(id)` — switch i18n locale; dirty-marks locale-bound signals.
- `ctx.close_window()` — request the owning window close.
- `ctx.request_focus(widget_id)` — programmatic focus transfer (overlay content on open, first error field on submit).
- `ctx.dismiss_all_overlays()` — useful after menu item activation.
- `ctx.send_intent(AppIntent::X)` — fire a typed intent; framework walks source → root invoking any matching `Action`. See [shortcut-intent-action.md](shortcut-intent-action.md).
- `ctx.request_frame()` — ask the event loop to pump one more frame (caret blink restart, drag auto-scroll, pending document events).
- `ctx.app_state::<T>()` — look up an app-scoped value registered on `TeksiloAppBuilder` by `TypeId`.

These are the methods that make it possible to build app-level behavior (menu routing, theme switching, shortcut rebind UIs) without any global statics or hardcoded backchannels.

## 6. Focus management

Focus is a single `Option<WidgetId>` stored on the tree. Tab / Shift+Tab moves it across widgets whose node has `focusable = true`. The framework publishes three signals that widgets can observe:

- The currently focused node id (read via `tree.focused()`).
- **Focus origin** — `Keyboard` (tab/shift-tab/programmatic) or `Pointer` (tap) or `Programmatic`. Used to paint a focus ring only on keyboard focus by default; pointer focus typically omits the ring per Int UI style.
- **Focus-gained / focus-lost** events dispatched to widgets via `on_focus(gained: bool, ctx)`.

Programmatic focus transfer goes through `ctx.request_focus(id)`. The framework also exposes `first_focusable_descendant(id)` for modal openers (dialogs that should land focus on the primary action button — it returns the widget Tab would land on *first*, respecting the scope rules below) and `ScrollIntoView` synthesized on focus change so that tab-focusing an offscreen widget scrolls the nearest clipping ancestor to reveal it.

Focus cleanup on destroy is automatic: destroying a focused widget clears focus; the next input event that requires focus routes to the nearest focusable ancestor or root.

### 6.1 Traversal scopes (`FocusScope`)

Tab order is **not** one flat global ring — it is a tree of **traversal scopes**. Every focusable widget belongs to its nearest enclosing [`FocusScope`](../crates/teksilo-widgets/src/focus_scope.rs); the whole window (or, while a centered modal is open, that modal's content) is an implicit root scope. Within a scope, members — focusable leaves *and* nested scopes, each counted as one unit — are ordered by **scoped `tab_index`** (then document order). Because `tab_index` is compared only among siblings of the same scope, two sibling scopes that both number their children `1, 2, 3` never interleave. This is Teksilo's analogue of Flutter `FocusTraversalGroup` / WPF `KeyboardNavigation.TabNavigation`.

A scope is declared by wrapping a subtree in the layout-transparent `FocusScope` wrapper, which carries a `TraversalScopePolicy` governing what Tab does at the scope's ends:

| Policy | At the scope boundary |
|---|---|
| `Continue` | Tab flows **out** into the enclosing scope's next member. Groups + scopes `tab_index` numbering without trapping focus — e.g. dock panels in a continuous Tab order. |
| `Cycle` | Tab **wraps** within the scope and never leaves via keyboard — modal dialogs only. |

```rust
// teksu!: a modal dialog whose Tab order is confined to its own content
FocusScope(TraversalScopePolicy::Cycle) {
    Button::new(lit!("OK"))
    Button::new(lit!("Cancel"))
}
// builder form
FocusScope::new(TraversalScopePolicy::Cycle).child(dialog_body)
```

**Not for popovers or menus.** A non-modal overlay is dismissed when keyboard
focus leaves it (see *Overlays follow focus out*, below) — the behaviour ARIA's
Disclosure and Menu patterns call for, and what stops an open panel from
covering the focus ring that just left it (WCAG 2.2 SC 2.4.11). `Cycle`-wrapping
one traps focus so that dismissal never fires.

The root scope is implicitly `Cycle` (whole-tree last↔first wrap, the historical behavior). A **centered modal overlay** folds into the same mechanism: its content subtree becomes the root `Cycle` scope, so Tab is confined to the modal with no special-case code. The `FocusScope` node itself is forced non-focusable (it is a boundary, never a Tab stop). A subtree with no `FocusScope` behaves exactly like the old flat wrapping ring.

> **Not to be confused with `view_focus_*`.** `BuildContext::begin_view_focus` / `view_focus_active` (formerly the `focus_scope` chrome API) is an unrelated build-time mechanism that tracks "does this data view's subtree hold focus" to drive selection chrome and focus rings. It has nothing to do with Tab traversal. Traversal scopes are the `FocusScope` widget + `set_traversal_scope`.

Implemented in [`cycle_focus`](../crates/teksilo-core/src/widget_tree/focus_impl.rs) (the recursive scope-tree walk) and [`set_traversal_scope`](../crates/teksilo-core/src/widget_tree.rs) (the node marker, directly usable from headless tests).

### Overlays follow focus out

A **non-modal overlay is dismissed when keyboard focus leaves it.** Menus, popovers, dropdown panels and suggestion lists do not contain focus; Tab is an exit gesture for all of them, and the panel goes when focus does. Widgets get this for free — there is nothing to wire, and nothing to wrap.

This is what the patterns those surfaces implement actually specify. ARIA APG's Menu pattern is unqualified: Tab "moves focus out of the `menu` or `menubar`, and closes all menus and submenus" — only the arrows navigate within. A popover implements Disclosure, which mandates no containment. The alternative, trapping, is *legal* (WCAG 2.1.2 is satisfied by Escape alone) but unsupported by any of those patterns, and it leaves the real defect in place: an open panel sitting over the focus ring that just left it, which is WCAG 2.2 SC 2.4.11 Focus Not Obscured (Minimum), Level AA.

An overlay is eligible when it is **positioned at its anchor** — `Below`, `Above`, `TrailingEdge`, `AtPointer`, `NearAnchor`, `BelowPreferred`. Those hang off a control, so "focus left that control" means something. The viewport-placed variants are excluded, and deliberately: `Centered` is the modal (the one surface whose pattern *does* contain focus), `FullViewport` is its scrim, and `BottomCenter` / `ViewportCorner` are notifications. A snackbar is shown from a focused button and leaves it focused — an anchor-aware rule that did not exclude it would tear the snackbar down on the user's very next keystroke, overriding both its timer and `.persistent()`. A toast's lifetime belongs to its timer, never to where the keyboard happens to be.

Two further exclusions: an overlay already fading out (dismissing it again collapses the tween it is mid-way through), and tooltips, which `tooltip_focus_leave_outside` owns end-to-end with a deliberately wider test — it keeps a tip alive while focus rests on its *anchor*, the normal state of a focus-promoted tip.

`DismissBehavior` is **not** consulted. It selects which of Escape / click-outside / hover-out apply, an orthogonal axis: a popover that opted out of click-outside did not thereby ask to survive being tabbed away from.

An overlay's **anchor counts as part of it.** A non-searchable `ComboBox` keeps focus on its trigger the whole time its dropdown is open, and a `SearchField` keeps it in the text input while suggestions float below — focus is never inside the overlay, so a content-only test would conclude nothing was ever open. Arriving *on* the anchor still counts as leaving, though, so Shift+Tab off the front of a popover closes it and lands on the trigger, where Escape would have left you.

Nested overlays close as a cascade: the walk goes *up* `parent_overlay` and dismisses the outermost eligible level, which takes every level below it — APG's plural "all menus and submenus" — while stopping at a host surface so a menu never drags its hosting dialog, composite tooltip or revealed menubar down with it.

Implemented in [`dismiss_overlays_left_by_focus`](../crates/teksilo-core/src/widget_tree/overlay_impl.rs), called from `focus_with_origin_ops` — the single funnel every focus change passes through, so Tab, click-to-focus, AccessKit and `ctx.request_focus` are all covered by one mechanism.

## 6.5 Drag-and-drop lifecycle

Target-side handlers fire in a strict order. A widget that accepts drops should assume this sequence and own the cleanup of any feedback state it sets:

1. **`on_drag_hover(payload, pos, ctx) -> DropFeedback`** — fires on every `PointerMove` while this widget is the drop target (pointer inside its bounds and the framework picked it via `find_drop_target_at_or_above`). The widget typically stashes its own feedback state (an insertion line y, a highlight rect) and returns the matching `DropFeedback` descriptor. `pos` is in **target-local** coordinates — origin at the target widget's top-left — so drop-index math can reuse the same coordinate system as the target's own `bounds` and `paint` layout.
2. **`on_drag_tick(local_pos, ctx)`** — fires once per layout pass while the widget is the current drop target. Use for per-frame behaviours that must keep progressing when the pointer is stationary: viewport-edge auto-scroll (linear ramp inside an edge zone), spring-loaded folder expansion after a dwell time. Receives the pointer position in widget-local coordinates.
3. **`on_drag_leave(ctx)`** — fires exactly once when this widget stops being the drop target. The framework emits it for **all four** leave scenarios: pointer moved to a different target, drop completed (on this or another target), Escape-cancelled, or the drag source was destroyed mid-drag. Widgets MUST clear any feedback state they set in `on_drag_hover` here — the framework does not touch widget-owned state.
4. **`on_drop(payload, pos, ctx) -> bool`** — fires on `PointerUp` only if this widget is the drop target at the release position. Already preceded by `on_drag_leave` (so feedback is cleared by the time the drop handler decides acceptance). Returns `true` if accepted.

Framework guarantees the ordering: `on_drag_leave` runs before `on_drop` on the same widget for a successful drop, and before `cleanup_drag_preview` for cancels. The `DragPreview` overlay (created via `EventContext::start_drag_with_preview`) follows the pointer throughout and is dismissed by the framework in all paths — widgets don't manage it.

## 7. Synthetic events

The framework dispatches a few synthetic events the widget code doesn't see from the platform:

- **`PointerEnter` / `PointerLeave`.** Derived from `PointerMove` by comparing the hit target frame-over-frame. A widget moving out from under a stationary pointer still gets `PointerLeave` — the hit target changed even if the pointer didn't.
- **`FocusGained` / `FocusLost`.** Issued when focus moves.
- **`ScrollIntoView { target }`.** Issued by the focus system after a focus change to a widget outside the viewport. Nearest clipping ancestor handles it by adjusting its scroll offset.
- **Synthetic clicks.** `ctx.synthetic_click(id)` dispatches a simulated tap at the widget's center — used by AccessKit action routing (`Action::Click`), menu item activation, and some shortcut-triggered activations that want to go through the full tap path.

## 8. Testing

Events are synthesizable from tests without a real platform:

```rust
let mut tree = WidgetTree::new();
let btn_id = tree.add(Button::new(lit!("OK")).on_activate_fn(|ctx| {
    ctx.send_intent(AppIntent::Confirm);
}));
tree.layout(SizeProposal::exact(200.0, 100.0));

// Synthesize a pointer tap at the button's center:
let bounds = tree.bounds(btn_id);
tree.dispatch_event(WidgetEvent::PointerDown {
    position: bounds.center(),
    button: PointerButton::Primary,
    modifiers: Modifiers::NONE,
});
tree.dispatch_event(WidgetEvent::PointerUp {
    position: bounds.center(),
    button: PointerButton::Primary,
    modifiers: Modifiers::NONE,
});
```

For gesture-level assertions the `test_api` module on `WidgetTree` exposes helpers like `synthesise_tap(id)` that run the preview-bubble walk with a fabricated event. Timing-sensitive recognizers (double-tap, long-press) use the tree's simulated clock — `advance_time(Duration)` in tests.

No Xvfb, no GPU, no display server required.

## 9. Design rules in one list

- Widget authors register typed closures per event type; no monolithic `event()` method.
- Events travel preview (root → target) then bubble (target → root); first `Handled` stops the pass.
- Attach handlers on children with `.on_foo(…)` via `WidgetBuilder`; attach on self with `HandlerSet` + `ctx.apply_self_handlers`.
- Gesture recognizers are auto-wired from attached handlers; `GestureArena` arbitrates cooperation and reset.
- Handlers express mutations by calling methods on `EventContext`; the framework applies them after dispatch.
- `ctx.send_intent(X)` is the single way to request app-level behavior from a handler; `ctx.set_theme / set_locale / close_window / request_focus / dismiss_all_overlays` cover the framework-level ambient ops.
- Focus is a single optional WidgetId; transfers happen via `ctx.request_focus(id)`; Tab/Shift+Tab walks a tree of `FocusScope`s (scoped `tab_index`, per-scope `Continue`/`Cycle` policy), defaulting to a flat document-order ring when no scopes are present.
- Everything is headless-testable — dispatch synthetic events, advance the simulated clock, inspect the tree.

---

## See also

- [animation.md](animation.md) — `Signal<f32>::animate_to` and the scheduler. Handlers that kick off motion (toggle thumb, accordion height, snackbar slide-in) call `animate_to` on animation-capable signals; the docs here and there are two halves of the "handler runs → something moves" path.
- [shortcut-intent-action.md](shortcut-intent-action.md) — how intents travel source → root and fire `Action`s; rebindable keystrokes via `ShortcutRegistry`.
- [architecture.md §22 Window Management](architecture.md) — modal-vs-modeless, window focus routing.
- [architecture.md §13 Overlay System](architecture.md) — overlay stack, click-outside, Escape cascade, focus-restore on dismiss.
- [crates/teksilo-core/src/event_handlers.rs](../crates/teksilo-core/src/event_handlers.rs) — `EventHandlers` struct.
- [crates/teksilo-core/src/widget_builder.rs](../crates/teksilo-core/src/widget_builder.rs) — blanket-impl builder methods.
- [crates/teksilo-core/src/gesture.rs](../crates/teksilo-core/src/gesture.rs) — recognizer state machines.
- [crates/teksilo-core/src/widget_tree/event_dispatch_impl.rs](../crates/teksilo-core/src/widget_tree/event_dispatch_impl.rs) — dispatch walk.
- [crates/teksilo-core/src/widget.rs](../crates/teksilo-core/src/widget.rs) — `EventContext`.
- [crates/teksilo-widgets/src/focus_scope.rs](../crates/teksilo-widgets/src/focus_scope.rs) — the `FocusScope` traversal-scope wrapper (§6.1).
- [crates/teksilo-core/src/widget_tree/focus_impl.rs](../crates/teksilo-core/src/widget_tree/focus_impl.rs) — `cycle_focus` scope-tree traversal, `set_traversal_scope`, `view_focus_*` chrome signals.
