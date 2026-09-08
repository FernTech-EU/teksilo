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

The two AccessKit slots are **layered, not alternatives**. Every installed one
fires for a single dispatched action — both shapes, and within each shape both
the app-installed handler and the widget's own — and the action counts as
handled if any of them says so. They have different owners:
`on_access_action_request` is what a widget reaches for when it needs
`target_node` or the payload (`Slider`, `SpinBox`, `TextInputField`,
`CodeEditor`, `TabBar` all do), while `.on_access_action(..)` is the
application's hook. The dispatcher used to *prefer* the payload shape when it
was set, which did not choose between two handlers for one job — it silently
disabled the app's handler on exactly the widgets that had migrated.

### 3.1.1 `TapEvent` — button + modifiers in the callback

The four click-style handlers (`on_tap` / `on_double_tap` / `on_triple_tap` / `on_long_press`) all receive a borrowed [`TapEvent`](../crates/teksilo-core/src/gesture.rs):

```rust
#[non_exhaustive]
pub struct TapEvent {
    pub position: Point,         // widget-local coords
    pub button: PointerButton,   // which button finalised the gesture
    pub modifiers: Modifiers,    // held at the finalising event
    pub pointer: PointerInfo,    // which pointer — mouse, finger, stylus
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

[`gesture.rs`](../crates/teksilo-core/src/gesture.rs) defines the recognizer state machines. Each is a pure, platform-free value type that consumes `RawPointerEvent::{Down, Move, Up, Cancel}` and emits `GestureResult::{Pending, Recognized(GestureEvent), Failed}`. Every raw event carries the [`PointerInfo`](../crates/teksilo-core/src/pointer.rs) that produced it and the [`EventTime`](../crates/teksilo-core/src/pointer.rs) the backend stamped it with.

Built-in recognizers (the four click-style ones default to `ButtonMask::PRIMARY` — call `.accept_buttons(...)` / `.accept_any_button()` to widen):

- `TapRecognizer` — fires on a down-up without movement past the tap-slop threshold. Down/Up button must match.
- `DoubleTapRecognizer` — two taps inside the profile's multi-tap window. Both taps must use the same button.
- `TripleTapRecognizer` — three. Same button across all three.
- `LongPressRecognizer` — pointer held past the profile's long-press hold. Modifiers are captured at `Down`.
- `DragRecognizer` — emits `DragStarted` once the pointer moves past the drag-start threshold, then `DragMoved` per move, then `DragEnded` on pointer-up.
- `SwipeRecognizer` — pointer moves fast enough to qualify as a swipe in one of four cardinal directions.

`PinchRecognizer` is *not* in the list because on desktop the OS delivers `TouchpadMagnify` / `RotationGesture` events directly (winit passes them through); the framework turns those into `PinchPhase` events without needing a recognizer.

### 4.1 GestureArena — cooperating and competing

When a widget attaches multiple gesture handlers (`on_tap` + `on_long_press`), both recognizers run in parallel on the same event stream via `GestureArena`. The arena's rules:

- Each recognizer sees every raw event until it returns `Recognized` or `Failed`.
- When one recognizes, competing recognizers whose `resets_on_peer_recognition` flag is set get reset (`DoubleTapRecognizer` peers-reset when `TapRecognizer` alone fires — so a single tap doesn't arm a phantom "missing second tap" in the double-tap recognizer).
- Cooperative recognizers (tap and triple-tap, for instance) run to completion side-by-side.

Widget authors never touch the arena directly. Attaching handlers via `WidgetBuilder` or `HandlerSet` auto-wires the recognizers and the arena on the node.

### 4.1.1 `RecognizerContext` — where thresholds and time come from

A recognizer holds **no** thresholds and reads **no** clock. Both arrive per call, in a
[`RecognizerContext`](../crates/teksilo-core/src/gesture/config.rs):

```rust
pub struct RecognizerContext<'a> {
    pub now: EventTime,            // the tree's one input clock, never Instant::now()
    pub profile: GestureProfile,   // the tuning for *this* pointer's kind
    pub local_bounds: Rect,        // the owning node, origin at zero
    pub pointer: PointerInfo,      // who is pointing
    pub streak: &'a TapStreak,     // the node's tap count — see below
}
```

`profile` is `theme.input.profile(pointer.kind)`: the mouse column is byte-for-byte the
constants Teksilo shipped before the touch programme (5 dp tap and drag slop, 10 dp
multi-tap slop, 300 ms multi-tap window, 500 ms long press, 200 dp/s and 30 dp for a
swipe), so a mouse behaves exactly as it did. A finger reads the touch column instead —
18 dp of slop, 40 dp of multi-tap slop — without any recognizer knowing that a finger
exists. Retuning `Theme::input` retunes every recognizer live, because the context is
rebuilt per dispatch rather than captured.

The builder overrides (`TapRecognizer::max_distance`, `DragRecognizer::threshold`,
`LongPressRecognizer::min_duration`, `DoubleTapRecognizer::max_interval`, …) still work
and still win over the profile. They are the exception, not the road: a widget that pins
a threshold pins it for every pointer kind.

`now` is an [`EventTime`](../crates/teksilo-core/src/pointer.rs) — a duration since the
tree epoch, produced by the tree's one `InputClock`. A test installs a `ManualClock` and
a long press, a double-tap window or a fling resolves exactly when the test says, with no
sleeping. `Instant::now()` is banned from the gesture layer outside its own test blocks,
and a source scan (`no_wall_clock_in_gestures`) fails the build if it reappears.

### 4.1.2 `GestureArenaSet` — one arena per contact

A `GestureArena` follows **one** press. That was enough while the only pointer was a
mouse; a touchscreen delivers two presses that overlap in time and each needs its own
recognizer state. So a node carries a
[`GestureArenaSet`](../crates/teksilo-core/src/gesture/arena_set.rs): the recognizer
*list* decided once when its handlers are read (as `GestureProto` factories), one arena
instantiated lazily per live `PointerId`, and the node's `TapStreak`.

A contact stops being followed the moment it lifts or is cancelled — touch ids are minted
per press, so a set that kept them would grow without bound.

### 4.1.3 `TapStreak` — the tap count lives on the node

Counting taps used to live inside `DoubleTapRecognizer` and `TripleTapRecognizer`. It
cannot stay there: on a touchscreen the second tap of a double tap is a **different**
`PointerId` — a different contact, a different arena — so tap one's count would be
destroyed before tap two arrived, and touch double-tap would be structurally impossible.

The count therefore lives on the node, in a `TapStreak` that outlives every contact. The
arena set advances it once per qualifying release, *before* the recognizers see the
event; the recognizers only read `cx.streak.count()` and fire at two and three.

**Continuation rule.** A tap continues the streak when **all** of these hold, and starts a
new streak (count 1) otherwise:

1. it landed on the same node — true by construction, the streak *is* the node's;
2. it used the same button as the previous tap;
3. `now - last_up <= profile.multi_tap_interval`;
4. `distance(press point, last_position) <= profile.multi_tap_slop`;
5. no other gesture completed on the node in between — a drag, a swipe or a long press
   finishing resets the streak.

Condition 4 measures press to press. (The pre-P06 recognizers measured release to
release; the two differ only by the within-tap travel, which is itself bounded by the tap
slop.) A release that strayed further than `multi_tap_slop` from its own press does not
count as a tap at all: it neither advances the streak nor breaks it, which is what lets
"tap, press-and-scrub-and-release, tap" still read as a double tap. A streak of three
restarts at one, so a long burst keeps producing alternating double and triple taps.

An explicit `max_interval` / `max_distance` on a multi-tap recognizer can only ever
**narrow** the window — the streak is shared by every recognizer on the node and uses the
profile.

### 4.1.4 The two cancels

`GestureArenaSet` revokes in two grains, and the distinction is load-bearing:

- **`cancel(pointer)`** — terminal for that contact. Every recognizer it was feeding is
  cancelled, the contact stops being followed, and the node's streak is broken. This is
  what the system-level revocations raise: the window lost focus, a modal opened over the
  interaction, the OS took the pointer (`CancelReason::{WindowDeactivated, ModalOpened,
  Platform, …}`).
- **`cancel_taps(pointer)`** — kills only the *tap family* (tap, double tap, triple tap,
  long press) and leaves everything else running. A drag the same press started keeps
  reporting.

The second is what WCAG 2.2 SC 2.5.2 ("Pointer Cancellation") needs: sliding a finger off
a control must abort its **activation** without aborting the **drag** the same press is
driving. Recognizers declare which family they are in via
`GestureRecognizer::tap_family`.

A `RawPointerEvent::Cancel` fed through the set does the same for the recognizers that
care: a running drag emits `DragCancelled` (so its handler can unwind) rather than a
`DragEnded` at wherever the pointer happened to be, and `DragPhase`/`PinchPhase` gained a
matching `Cancelled` arm. Both enums are now `#[non_exhaustive]`.

### 4.1.5 Multi-contact policy

```rust
pub enum MultiContact { First, All }   // default: First
```

Declared per node with `.multi_contact(..)` on `HandlerSet`, `WidgetWithHandlers` or the
`WidgetBuilder` trait. Under `First` — the default, and what every widget written before
the touch programme assumes — a node serves its first contact and an **extra** contact is
*terminated at that node*: not delivered to it, and not bubbled to an ancestor either.

That last half is the point, and its scope is exactly the arena. Two fingers landing on
one button fire **one** tap: the second contact never gets the button's press and never
reaches an ancestor's arena to complete a tap there instead
(`two_fingers_on_one_button_fire_one_tap`).

What it does **not** do is silence the second contact. Capture, arbitration and press
ownership are per pointer, so the refused contact still opens a `PointerSequence` of its
own, still enrols an enclosing pan claimant, and still wins it at `pan_slop` — a second
finger inside a scroll area scrolls it, while the first goes on holding the button
(`a_second_finger_on_a_button_in_a_scroller_pans_the_scroller`). That is what the
platforms do, and it is not what reading `First` as "the extra contact is ignored" would
predict. Both tests are in `crates/teksilo-core/tests/arbitration_matrix.rs`.

`All` opts a genuine multi-touch surface (a pinch-zoom canvas, a piano keyboard) into
per-contact arenas.

### 4.2 The pointer sequence — cross-widget arbitration

The `GestureArena` is **per-widget**; there is no cross-widget arena. That
leaves a gap: a descendant's `on_tap` installs a `TapRecognizer` that
**captures** the pointer on `PointerDown`, which would otherwise route every
following `PointerMove`/`PointerUp` to the descendant alone — so an *ancestor*
that wants to start a drag (a `SceneView` behind tappable cards, a draggable
container wrapping tappable rows) would never see the move and could never
begin its drag. A scroll container, a widget driving its whole interaction
from an explicit `capture_pointer()`, and a wrapper claiming a press from the
preview pass all have the same problem, and the same right to compete for it.

The framework closes all of that with **one arbitration object per live
pointer**: a [`PointerSequence`](../crates/teksilo-core/src/gesture/sequence.rs),
stored on that pointer's entry in the `PointerTable`, carrying the frozen hit
path, the frozen `TouchAction`, every enrolled competitor, and the winner once
one is decided.

```rust
pub struct PointerSequence {
    pointer: PointerInfo,
    path: Vec<WidgetId>,              // frozen at press, target -> root
    touch_action: TouchAction,        // frozen at press
    dead_zone_boundary: Option<WidgetId>,
    members: Vec<SequenceMember>,     // innermost first
    winner: Option<WidgetId>,
    capture: Option<WidgetId>,
    press_origin: Point,
    last_position: Point,
    started_at: EventTime,
    pressed_owner: Option<WidgetId>,
}

pub struct SequenceMember {
    pub id: WidgetId,
    pub role: MemberRole,             // Gesture | Pan(PanClaim) | RawDrag | RawPreview
    pub eligible_at: Option<EventTime>,
    pub state: MemberState,           // Possible | Held | Rejected | Won
}
```

Read it with `WidgetTree::sequence_members(PointerId)` and
`WidgetTree::sequence_winner(PointerId)`.

#### The ordered decision procedure

**At press**, the router hit-tests to a target and then, in order:

1. Freezes the hit path (target → root) and the effective `TouchAction`
   (`effective_touch_action`, the root-to-target intersection). Both are frozen
   for the life of the press: a rebuild mid-gesture cannot change who was
   competing, and `EventContext::touch_action()` reports the frozen value from
   inside every handler.
2. Records the **innermost `gesture_dead_zone`** node on that path as the
   enrolment boundary. Nothing at or above it may be enrolled — for a mouse
   exactly as for a finger.
3. Enrols the **pan claimants** on the path (`pan_candidates`), innermost
   first. Only for a direct pointer, only where the claim's device mask admits
   it, and only on an axis the frozen `TouchAction` still permits.

**Then**, in this order:

4. **The raw-preview pass runs FIRST and keeps its ROOT-FIRST order.** The
   first ancestor whose `on_pointer_event` answers `Handled` claims the press
   as a `RawPreview` winner. This order is load-bearing — `rich_text/mouse.rs`
   relies on an outer wrapper seeing a press before an inner one — so
   previewers are deliberately *not* folded into the innermost-first member
   order.
5. **An explicit `capture_pointer()` from an undecided sequence is an
   arbitration act**, not plumbing: the caller is enrolled as a `RawDrag`
   member, and for a precise pointer with no eligible pan competitor the
   sequence is decided there and then. Three shipped widgets drive their whole
   interaction this way — the splitter handle, the dock resize handle and the
   table column grip all answer `Ignored` from `on_pointer_event`, capture, and
   work from `PointerMove` with no recognizer at all. The framework's own
   captures (the gesture arena's Down..Up window, the drag pipeline's) go
   through a private implicit door and stake no claim.
6. Direct pointers take an implicit capture on the bubble target; mouse capture
   stays explicit, exactly as before.
7. **On move while undecided: timers before positional thresholds**, then
   members innermost-first.
   - a `RawDrag` member wins past the sequence's latch slop;
   - a `Gesture` member wins when its own recognizer recognizes — for a mouse,
     at `drag_slop`;
   - a `Pan` member wins only on an axis the frozen `TouchAction` permits and
     only past `pan_slop`, and a diagonal tie resolves by dominant axis then
     innermost;
   - a member with `DragActivation::AfterLongPress` cannot win before its timer
     and **self-rejects** the instant the press leaves the tap boundary;
   - `slop_precise` applies **only** to a direct pointer under a frozen
     `TouchAction::NONE`. A precise pointer always uses `profile.drag_slop`.
   A **`Gesture`** member that took the press (the `pressed_owner`) is driven
   by the ordinary capture route and **stops the walk**: nothing above the
   innermost drag may win, which is the pre-existing "the innermost drag owns
   the gesture" rule. A `RawDrag` and a `Pan` member of that same node are
   exempt, because this walk is the only place either is ever evaluated — so
   stopping there would leave a scroll container that also owns the press
   arena, which is every text surface, unable ever to pan itself. A
   `RawPreview` stops the walk with the `Gesture`, and never reaches it in
   practice: a preview claim decides the sequence as it is enrolled, and a
   decided sequence yields no candidates.
8. **On up**: the release sweep. Every member still following the press is fed
   the terminating `Up` so its recognizer clears the origin it recorded — which
   is what stops an ancestor `DragRecognizer` from staying armed and starting a
   phantom drag on the next *hover* move — and the pressed owner's own arena
   resolves its tap through the normal capture dispatch.

A member that loses keeps its handlers and loses only its **recognizers**: the
router silences the arena of a rejected member, of every non-winner once a
winner exists, of a peer while another member is holding, and of a member whose
deferral timer has not yet elapsed. Losing an arbitration is not the same as
being removed from the tree.

#### Why the mouse is unchanged

`GestureProfile::pan_slop` is `None` for a mouse and `PanClaim::devices`
defaults to direct pointers, so **no pan member is ever eligible for a mouse**.
Every mouse sequence is therefore decided at press (an explicit capture, a
preview claim) or arbitrated exactly as the old drag-observer mechanism
arbitrated it: ancestors innermost-first, each latching at its own `drag_slop`,
which on the mouse profile is the 5.0 dp it has always been. On touch the same
widget defers by `drag_slop` (18) and still beats a scroller, because
`pan_slop` (36) is larger.

Two consequences worth knowing, unchanged from before:

- A widget with its **own** `on_drag` (a slider, a DnD row) is untouched — it
  is the innermost member and stops the walk, so only *pure-tap* descendants
  inside a draggable ancestor change behaviour.
- A quick press-release on a card is still a **tap**; only a press-and-drag
  escalates to the ancestor. That is what makes "drag from on top of a
  select-only scene card starts a marquee, click selects it" work (see
  [teksilo-scene.md](teksilo-scene.md) "Drag mode").

#### `TapBoundary` — one predicate, three consumers

Where a press stops being a tap is decided once, by
`TapBoundary::for_pointer(&pointer, &profile)`:

| Pointer | Boundary |
| --- | --- |
| Precise (mouse, pen) | `Radius(profile.tap_slop)` — 5 dp for a mouse, exactly as before. |
| Coarse (finger) | `Bounds` — the pressed node's own rect. |

A finger's reported centre wanders several device pixels while resting on the
control it is pressing, so `tap_slop` must **not** independently cancel a coarse
tap that never left its target. The same predicate is (a) what `TapRecognizer`
fails on, (b) what the router uses to fire `GestureArenaSet::cancel_taps` — once
per press — when the pointer slides off, and (c) what will clear the framework's
press visual. WCAG 2.2 SC 2.5.2's "slide off to abort" is exactly rule (b): the
activation is abandoned, and a drag the same press started is not.

#### Handler-side arbitration API

| Call | Meaning |
| --- | --- |
| `ctx.claim_gesture()` | This node owns the press; every peer is cancelled. |
| `ctx.reject_gesture()` | This node withdraws; its peers carry on. |
| `ctx.hold_gesture()` | Defer this node's own answer — **no peer may win while it holds**, on the sample path and on the gesture timer alike. The silence is not a queue: a peer's gesture that ripens inside the hold is *dropped*, and the release does not deliver it late. Auto-releases at `profile.max_hold` (250 ms in every shipped profile). For an *application* recognizer awaiting an answer it does not have yet; the framework never holds. |
| `ctx.release_gesture()` | End the hold. |
| `ctx.owns_pointer()` | Whether this node still holds the pointer's capture. A widget driving an interaction from `PointerMove` should gate on it: capture is an arbitration act, so a widget that lost the press must stop driving even though its own state says it started one. |

#### `gesture_dead_zone` stays itself

`.gesture_dead_zone(true)` (and the [`DeadZone`](../crates/teksilo-widgets/src/primitives/dead_zone.rs)
wrapper) marks a subtree whose presses may not enrol any ancestor as a
`Gesture`, `Pan` or `RawDrag` member — kind-independently.

It is **not** sugar for `.touch_action(TouchAction::NONE)`. A mouse ignores
touch actions entirely, so the substitution would delete the mouse behaviour
the flag exists for; and on a direct pointer it would drop the latch to
`slop_precise` and turn the dead zone's own regression — "a jittery click on a
header button must not drag the panel" — into a 2 px hair trigger.

`DeadZone` also installs a no-op `on_tap` / `on_drag` pair. That is **not**
redundant with the boundary and is deliberately kept: the flag governs
*enrolment*, and a press landing on the wrapper's own bare area (a gap between
the controls it wraps) needs something to take the press so it never reaches
the ancestor's recognizer at all. The absorbing `on_tap` is what gives the
wrapper an arena, which stops the bubble; the boundary is what stops the
ancestors from competing anyway.

#### `sequence_members` in tests

The observable form of the arbitration, and the successor to the deleted
`armed_drag_observers()`:

```rust
tree.pointer_down_button(button_center, PointerButton::Primary);
assert!(
    tree.sequence_members(PointerId::MOUSE).is_empty(),
    "a dead zone blocks the draggable ancestor from competing",
);
```

#### The arbitration matrix

`crates/teksilo-core/tests/arbitration_matrix.rs` is the whole decision
procedure written out as a table: pointer kind × frozen `TouchAction` × member
set × movement vector → the named winner and each loser's exact cancel. The
table below **is** that file's fixtures — `the_documented_table_matches_the_fixtures`
fails if the two drift, and prints the replacement.

<!-- BEGIN GENERATED ARBITRATION MATRIX -->
<!-- Generated by crates/teksilo-core/tests/arbitration_matrix.rs. Do not edit by hand: `the_documented_table_matches_the_fixtures` fails when the two drift, and `TEKSILO_BLESS=1 cargo test -p teksilo-core --test arbitration_matrix` rewrites this region. -->

Each row is a **core-only fixture** reproducing the named widget's arbitration shape — the handlers it installs, the claims it declares and the capture it takes — not the widget itself, which lives in a crate `teksilo-core` cannot depend on. `ScrollArea` — and `TabBar`, through the scroll area it wraps its header row in — is the only production declarer of a `PanClaim`; every other row's claimant is the enclosing scroller the real shape would sit in, written out. So read a row as *what the framework does with this shape*.

| Scenario | Pointer | Frozen | Members at press (innermost first) | Movement → winner | Losers cancelled | What it pins |
| --- | --- | --- | --- | --- | --- | --- |
| list row | mouse | `AUTO` | `row` Gesture/Possible | (+0, +4) → —; (+0, +5) → `row` | — | a mouse never enrols a pan claimant, so the row's own drag is the only competitor and it latches at 5 dp. Three redundant gates enforce the no-enrolment half — `begin_sequence`'s is_direct guard, PanClaim::devices' DIRECT default and the mouse profile's absent pan_slop — and no single deletion reddens it: removing the is_direct guard moves nothing in teksilo-core at all, and removing the DIRECT default moves only the pen rows. It covers the rule, not any one implementation of it; see the header for which of the three have been mutated |
| list row | touch | `AUTO` | `row` Gesture/Possible, `scroller` Pan/Possible | (+0, +17) → —; (+0, +19) → `row` | `scroller` PeerClaimed | the reorder wins at drag_slop (18) before the scroller's pan_slop (36) is reached, and the claimant is told |
| slider thumb | touch | `NONE` | `slider` Gesture/Possible | (+0, +17) → —; (+0, +19) → `slider` | — | TouchAction::NONE keeps the enclosing claimant out of the member list entirely — there is no loser to cancel. Two independent gates enforce it (`pan_candidates`' axis filter and `PointerSequence::pan_is_eligible`'s), so this row goes red only when *both* are removed: it covers the rule, not either implementation of it |
| text selection | mouse | `PAN_Y` | — | (+0, +20) → —; (+0, +60) → — | — | a mouse selection is owned by the capture, not by the arbitration: no competitor is ever enrolled. Redundant gates enforce that — `begin_sequence`'s is_direct guard, PanClaim::devices' DIRECT default and the mouse profile's absent pan_slop — so the row covers the *rule*. The row on which a device set and a profile's pan_slop are each singly observable is `text selection · pen`, on a pointer those two gates admit |
| text selection | touch | `PAN_Y` | `scroller` Pan/Possible | (+0, +20) → —; (+0, +40) → `scroller` | — | a frozen PAN_Y admits the claim it agrees with: the pan wins at 36 dp and the editor — the captor, but not a member — is never cancelled |
| text selection | pen | `PAN_Y` | `scroller` Pan/Possible | (+0, +6) → —; (+0, +9) → `scroller` | — | what this row alone asserts is the **enrolment** — that a pen puts the claimant in the member list at all, which PanClaim::devices' DIRECT default decides — and the threshold it then pans at, bracketed by the two steps: past 6 dp and by 9, i.e. GestureProfile::PEN's own pan_slop and not a finger's 36. Either gate reddens it, identically — it says a gate moved, not which — and it is not the sole witness to either: the `column grip · pen` row and `a_pen_loses_an_explicit_capture_to_a_drag_capable_ancestor` read the same two through the DragActivation they resolve on the capture-versus-ancestor path, and neither asserts this row's threshold |
| scene marquee | mouse | `AUTO` | `container` Gesture/Possible | (+4, +0) → —; (+5, +0) → `container` | — | the marquee reaches through the tapped card's capture and latches at 5 dp; the card is not a member, so it is not cancelled. Nothing above it claims a pan, so — unlike the other mouse rows — the empty pan half is the fixture's doing, not the mouse's |
| scene marquee | touch | `AUTO` | `container` Gesture/Possible | (+17, +0) → —; (+19, +0) → `container` | — | with no pan claimant above it, DragActivation::Auto resolves to Immediate and the marquee latches at drag_slop |
| scene marquee in a scroller | touch | `AUTO` | `container` Gesture/Possible, `scroller` Pan/Possible | (+0, +20) → —; (+0, +37) → `scroller` | `container` PeerClaimed | an eligible pan defers the marquee to AfterLongPress, so it cannot win at 19 dp the way the unscrolled marquee does; the pan takes the press at 36 and cancels it |
| scene marquee in a scroller, press at the edge | touch | `AUTO` | `container` Gesture/Possible, `scroller` Pan/Possible | (+0, +20) → —; (+0, +37) → `scroller` | — | a coarse pointer's tap boundary is the member's own bounds, not a slop radius: 20 dp off a press near the edge leaves them, the deferred marquee withdraws itself, and a member that withdrew is never cancelled |
| drag region | mouse | `AUTO` | `region` Gesture/Possible | (+4, +0) → —; (+5, +0) → `region` | — | the window move starts from DragPhase::Started, never from the press: an implicit arena capture decides nothing |
| drag region | touch | `AUTO` | `region` Gesture/Possible | (+17, +0) → —; (+19, +0) → `region` | — | same rule for a finger, at the finger's slop |
| tab strip vs tab drag | mouse | `AUTO` | `tab` Gesture/Possible | (+4, +0) → —; (+5, +0) → `tab` | — | the horizontal twin of the list row: the tab's own drag is the only mouse competitor, and the same three redundant gates keep the strip's claim out — the rule, not one implementation of it |
| tab strip vs tab drag | touch | `AUTO` | `tab` Gesture/Possible, `strip` Pan/Possible | (+17, +0) → —; (+19, +0) → `tab` | `strip` PeerClaimed | dragging a tab beats scrolling the strip on the same axis, for the same reason a reorder beats a list scroll |
| column grip | mouse | `AUTO` | `grip` RawDrag/Won, `ancestor` Gesture/Rejected | (+4, +0) → `grip`; (+40, +0) → `grip` | — | an explicit capture by an *indirect* pointer decides at the press; the ancestor is enrolled already-rejected and so is never cancelled. The strip's claim is absent for the same three redundant gates the other mouse rows name |
| column grip | touch | `AUTO` | `grip` RawDrag/Possible, `strip` Pan/Possible, `ancestor` Gesture/Possible | (+17, +0) → —; (+19, +0) → `grip` | `strip` PeerClaimed, `ancestor` PeerClaimed | a contact's explicit capture does not decide at the press: the grip latches at latch_slop, which under AUTO is 18, and both live competitors are told |
| column grip declaring NONE | touch | `NONE` | `grip` RawDrag/Possible, `ancestor` Gesture/Possible | (+1, +0) → —; (+3, +0) → `grip` | `ancestor` PeerClaimed | under a frozen NONE a direct pointer's RawDrag drops to the jitter floor (2 dp) — the only production reader of PointerSequence::latch_slop's precise branch |
| column grip | pen | `AUTO` | `grip` RawDrag/Possible, `strip` Pan/Possible, `ancestor` Gesture/Possible | (+1, +0) → —; (+3, +0) → `grip` | `strip` PeerClaimed | a pen is precise but *direct*, so the press-time decision does not fire and the grip latches on travel instead; the eligible pan defers the ancestor, which both leaves the grip room to latch and withdraws the ancestor — unannounced — as the press leaves its 2 dp tap boundary |
| column grip declaring NONE | pen | `NONE` | `grip` RawDrag/Possible, `ancestor` Gesture/Possible | (+1, +0) → —; (+3, +0) → `ancestor` | `grip` PeerClaimed | **recorded defect, not a rule.** NONE removes the pan competitor and so re-resolves the ancestor from AfterLongPress to Immediate; the two slops that then compete are the grip's latch_slop — slop_precise, 2 dp, under a frozen NONE — and the ancestor's DragRecognizer threshold, PEN.drag_slop, also 2 dp, so the tie goes to dispatch order and the ordinary bubble reaches the ancestor first. See `a_pen_loses_an_explicit_capture_to_a_drag_capable_ancestor` |
| column grip declaring MANIPULATION | touch | `MANIPULATION` | `grip` RawDrag/Possible, `strip` Pan/Possible, `ancestor` Gesture/Possible | (+1, +0) → —; (+3, +0) → —; (+19, +0) → `grip` | `strip` PeerClaimed, `ancestor` PeerClaimed | latch_slop's precise branch keys on NONE **exactly**, not on any action narrower than AUTO: MANIPULATION forbids nothing the pan arbitration reads, so the grip stays at 18 dp and 3 dp of travel decides nothing. Widen that condition to `!= AUTO` and this row — alone — goes red at step 1 |
| two-axis scroller under PAN_X | touch | `PAN_X` | — | (+30, +0) → —; (+40, +0) → — | — | a claim is **excluded entirely, never narrowed**: a two-axis scroller inside a PAN_X region pans on neither axis, not on the permitted one. The only row that separates `pan_candidates`' axis filter from `PointerSequence::pan_is_eligible`'s — the latter asks whether *any* claimed axis survives and would admit this claim, so deleting the former alone reddens this row and no other |
| two-axis scroller under PAN | touch | `PAN` | `scroller` Pan/Possible | (+30, +0) → —; (+40, +0) → `scroller` | — | the control for the PAN_X row: the same claim, the same fixture, the same travel along the same axis — admitted, because PAN carries both pan bits, and the one thing that differs is the frozen action. Without it the row above would pass just as well if `pan_candidates` returned nothing at all |
| nested previewers | mouse | `AUTO` | `outer` RawPreview/Won | (+40, +0) → `outer` | — | the raw-preview pass is root-first and the first Handled claims the press, so the inner previewer never runs |
| nested previewers | touch | `AUTO` | `outer` RawPreview/Won | (+40, +0) → `outer` | — | a preview claim is unconditional: unlike an explicit capture it decides for a coarse pointer too |
<!-- END GENERATED ARBITRATION MATRIX -->

### 4.3 Touch action and pan claims

Two node properties declare what a **direct pointer** (touch, pen) may do to a
subtree, independently of whether the widget has attached any handler at all.
The types, the builders, and the two path folds that read them live in
[`crates/teksilo-core/src/pointer/touch_action.rs`](../crates/teksilo-core/src/pointer/touch_action.rs).
Both are read **at press**, folded once and frozen onto the press's
[`PointerSequence`](#42-the-pointer-sequence--cross-widget-arbitration) —
`TouchAction` gates which axes a pan member may win on and is what
`EventContext::touch_action()` reports; `PanClaim` is what enrols a scroll
container as a competitor in the first place.

**`TouchAction`** — the CSS `touch-action` model. A node declares one; the
effective value for a target is the *intersection* of its own declaration
with every ancestor's, root to target (`WidgetTree::effective_touch_action`,
in `widget_tree/pointer_state.rs`) — an ancestor can only narrow what a
descendant permits, never widen it.

| Constant | Permits |
| --- | --- |
| `TouchAction::AUTO` (default) | Everything — pan on both axes, pinch-zoom, and any other default touch behavior. |
| `TouchAction::NONE` | Nothing — the subtree reserves every contact for its own gesture handling. |
| `TouchAction::PAN_X` | Horizontal panning only. |
| `TouchAction::PAN_Y` | Vertical panning only. |
| `TouchAction::PAN` | Panning on either axis (`PAN_X \| PAN_Y`). |
| `TouchAction::PINCH_ZOOM` | Pinch-to-zoom only. |
| `TouchAction::MANIPULATION` | Panning and pinch-zoom, no other default gesture (`PAN \| PINCH_ZOOM`). |

Set it with `.touch_action(TouchAction::…)`, available on `WidgetBuilder`,
`HandlerSet`, and `WidgetWithHandlers` — the same three surfaces every other
node-level property (`gesture_dead_zone`, `event_pass_through`, …) is set
through.

**`PanClaim`** — a node's declaration that it is a **pan surface**: it wants
to consume a direct pointer's drag as content panning. Declared
*independently* of `TouchAction` — a scrollable states "I pan" via
`PanClaim` regardless of what its own `touch_action` permits; `TouchAction`
is what the arbitration consults to decide whether a claim further down the
chain is still reachable. `WidgetTree::pan_candidates` collects
every claim from a target up to the root, **innermost first** — the order a
boundary pan will chain along once nested scrollables hand off at their
edges.

A scroll container **declares itself** — `.scroll_container(PanAxes::…)` —
rather than being inferred from the presence of an `on_scroll` handler.
That inference would be wrong more often than not: `SpinBox` increments its
value on wheel, `TabBar` remaps wheel to horizontal tab scroll, and
`SceneView` zooms on Ctrl-wheel — every one of those has an `on_scroll`
handler and none of them is a pan. `.scroll_container(axes)` is sugar for a
`PanClaim` with `devices: PointerKindMask::DIRECT` and `kinetic: true`; reach
for the `.pan_claim(PanClaim { .. })` escape hatch directly for a claim that
isn't kinetic, or that widens/narrows the device mask.

**A mouse ignores `TouchAction` entirely.** It has no contact patch to
restrict, and it already scrolls with the wheel rather than by dragging
content — there is nothing for a mouse to read here. This is also why
`PanClaim::devices` defaults to `PointerKindMask::DIRECT` (touch + pen) and
never `PointerKindMask::MOUSE`: however a widget declares its pan claim, a
mouse drag is never read as a pan.

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

An assistive technology's `Action::Focus` takes the same walk, but only from a node that **advertises the action**. A composite publishes one AT node on a root that is not itself focusable — a `SpinBox`, a `ComboBox`, a `DateEdit` keeps focus on an inner leaf — and each adds `Action::Focus` in its own `accessibility()`, so the walk lands where the keys go. Everything else (a `Panel`, a `GroupBox`, a landmark, a label) is not focusable and offers no `Focus`; walking in from *any* non-focusable node moved the keyboard onto the first control inside it — a node the technology could have named itself and did not — and then reported success. Such a node now reports the action unhandled instead.

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
- [crates/teksilo-core/src/pointer/touch_action.rs](../crates/teksilo-core/src/pointer/touch_action.rs) — `TouchAction` / `PanAxes` / `PanClaim` (§4.3).
- [crates/teksilo-core/src/widget_tree/pointer_state.rs](../crates/teksilo-core/src/widget_tree/pointer_state.rs) — `effective_touch_action` / `pan_candidates`, the two path folds (§4.3).
- [crates/teksilo-core/src/widget_tree/event_dispatch_impl.rs](../crates/teksilo-core/src/widget_tree/event_dispatch_impl.rs) — dispatch walk.
- [crates/teksilo-core/src/widget.rs](../crates/teksilo-core/src/widget.rs) — `EventContext`.
- [crates/teksilo-widgets/src/focus_scope.rs](../crates/teksilo-widgets/src/focus_scope.rs) — the `FocusScope` traversal-scope wrapper (§6.1).
- [crates/teksilo-core/src/widget_tree/focus_impl.rs](../crates/teksilo-core/src/widget_tree/focus_impl.rs) — `cycle_focus` scope-tree traversal, `set_traversal_scope`, `view_focus_*` chrome signals.
