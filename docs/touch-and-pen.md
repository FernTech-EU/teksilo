# Touch & pen

Teksilo is a desktop framework whose input model was, until now, a mouse: one
pointer, always hovering, always precise, always present. Touchscreens and
digitizers break all four of those assumptions at once. This document describes
the vocabulary the framework uses to stop assuming them.

It is written alongside the migration, so it grows as the packages land. What is
here now is what exists now: the pointer model, the clock, and the trace switch.

> **Status.** A mouse behaves exactly as it always has. Nothing yet produces a
> touch or pen sample: the platform backends still deliver mouse events, and the
> gesture recognizers still see the same stream they always did. What this
> package adds is the *vocabulary* those samples will arrive in.

---

## 1. The pointer model

### 1.1 What a pointer is

Every input sample carries a [`PointerInfo`]: who is pointing, with what, and
when.

```rust
pub struct PointerInfo {
    pub id: PointerId,
    pub kind: PointerKind,      // Mouse | Touch | Pen(PenKind) | Unknown
    pub primary: bool,
    pub buttons: ButtonMask,
    pub axes: PointerAxes,      // pressure, tilt, twist, contact patch
    pub time: EventTime,
}
```

`PointerKind` lives in `teksilo-tokens` rather than `teksilo-core`, because the
token structs name it: a `GestureProfile` is selected *by* pointer kind, and
`teksilo-tokens` must stay a leaf with no core edge. Everything else in the
pointer vocabulary lives in `teksilo_core::pointer`.

The distinction the framework actually reasons about is not the device name but
two independent properties of it:

| | **precise** | **coarse** |
| --- | --- | --- |
| **indirect** (a cursor stands in for the hand) | mouse, trackpad | — |
| **direct** (the finger occludes what it points at) | stylus | finger |

`is_direct()`, `is_coarse()`, `is_precise()` and `hovers()` on `PointerKind`
answer those questions; almost no code should be matching on the kind itself.
Direct pointers earn a hit outset and larger gesture slop because they occlude
their own target. Only `hovers()` pointers can ever fire a hover affordance —
which is why a design that hides an action until hover is *unreachable* with a
finger, not merely inconvenient.

### 1.2 Identity is minted per press

```rust
pub struct PointerId(NonZeroU64);   // process-unique, monotonic
```

A `PointerId` is minted for each **press**, not for each device, by the
process-global [`PointerIdAllocator`]:

```rust
let alloc = PointerIdAllocator::global();
let id = alloc.begin(device, os_touch_id);   // on Down
let id = alloc.get(device, os_touch_id);     // on Move
alloc.end(device, os_touch_id);              // on Up / Cancel
```

This is not ceremony. **winit reuses `Touch::id`**: lift a finger and press
again and the second contact can arrive carrying the identifier the first one
left behind. A table keyed on the raw OS id therefore attributes the new
contact's samples to the old contact's half-finished gesture — a bug that
reproduces roughly one press in three and looks like nothing at all in the
platform log. Minting a fresh, monotonic id per Down defeats it outright, which
is why there is deliberately **no generation counter**: it would be a second
mechanism for the thing the allocator already guarantees.

The mapping key is `(BackendDeviceKey, u64)` and not the OS id alone, because a
machine with a touchscreen *and* a digitizer will happily report contact `0` on
both at the same moment.

`PointerId::MOUSE` is the one exception: a stable reserved id (1) that every
synthesized mouse event uses. A mouse is singular by construction, so it needs
no per-press identity, and the legacy `dispatch_event` path stays
allocation-free.

### 1.3 Primary

`primary` is the W3C Pointer Events compatibility concept: exactly one live
pointer is primary, and it is the one that drives the singular legacy signals a
widget written before multi-touch will read. A mouse always wins the role.
`PointerInfo::touch(..)` deliberately constructs a **non**-primary pointer:
primacy is a fact about the whole set of live pointers, so only the pointer
table can decide it, never a constructor.

### 1.4 Pressure

`effective_pressure()` implements the W3C Pointer Events Level 3 rule: report
what the device said; failing that, `0.5` while any button is held and `0.0`
otherwise. A pressure-sensitive surface therefore has a defined answer for a
mouse without special-casing one.

---

## 2. One clock

### 2.1 The rule

> **`Instant` never enters a recognizer.**

Every deadline the input layer owns — a long press, a double-tap window, a
fling's decay, a press-feedback delay — is an [`EventTime`] read from the tree's
one [`InputClock`]. `EventTime` is a `Duration` since the tree's epoch, never an
`Instant`.

A recognizer that calls `Instant::now()` cannot be driven by a test, and a
recognizer that cannot be driven by a test is one whose timing is only ever
exercised by sleeping — which is how a suite acquires tests that pass on an idle
laptop and fail on a loaded CI runner.

### 2.2 The epoch is shared

A `WidgetTree` already had a simulated clock: `sim_clock`, which
`advance_time` moves and which the animation scheduler is ticked against. Its
zero is the `Instant` captured when the tree is built.

The input clock is seeded from **that same `Instant`**:

```rust
let epoch = Instant::now();
sim_clock: epoch,
input_clock: Rc::new(MonotonicClock::new(epoch)),
```

So `EventTime::ZERO` and the tree's initial `simulated_now()` name one moment,
and the input timeline and the animation timeline are one axis rather than two.
One `advance_time` call therefore moves gestures, long-press deadlines,
tooltips, overlays *and* animations to the same virtual now.

This is not a nicety. The animation scheduler already had to be rescued from the
two-clock version of this bug (see `WidgetTree::animation_clock`): while real
time and simulated time advanced independently, every animation armed after real
time overtook simulated time had a start in the scheduler's future and froze
completely — a failure that tracked machine load rather than behaviour. Giving
the input layer its own unrelated epoch would have re-created exactly that,
one subsystem over.

`WidgetTree::input_clock().epoch()` returns the anchoring `Instant` for a clock
that has one, and a test pins the shared origin:

```rust
assert_eq!(tree.input_clock().epoch(), Some(tree.simulated_now()));
```

### 2.3 Choosing a clock

| clock | `now()` | for |
| --- | --- | --- |
| [`MonotonicClock`] | `Instant::now() - epoch` | a real window. The default. |
| [`ManualClock`] | whatever it was last set to | a headless test driving time explicitly. |

```rust
let manual = Rc::new(ManualClock::new(EventTime::ZERO));
tree.set_input_clock(manual.clone());
manual.advance(Duration::from_millis(600));   // the long press is now due
```

Both are held as `Rc<dyn InputClock>`, so a tree's clock can be swapped at any
point. `WidgetTree::input_now()` is the short way to read it.

---

## 3. Samples: the two ingress doors

Input reaches the tree through two doors:

```rust
tree.dispatch_pointer(PointerSample { .. });
tree.dispatch_scroll(ScrollSample { .. });
// …and the `_with_ops` twins, which give handlers the multi-window API.
```

A `PointerSample` is one OS packet: a `PointerInfo`, a
`PointerPhase` (`Down | Move | Up | Cancel`), a position, the button that
changed, the modifiers, and any intermediate positions the OS batched into
`coalesced`. A `ScrollSample` adds a `ScrollPhase`
(`Discrete | Began | Changed | Ended | Momentum | MomentumEnded | Fling |
Cancelled`) and a `ScrollSource` (`Wheel | Trackpad | TouchPan | Programmatic`).

Both default to what a mouse has always meant: `ScrollPhase::Discrete` from
`ScrollSource::Wheel` is a wheel notch.

`dispatch_event(WidgetEvent)` remains public and keeps working; it defaults to a
mouse pointer, which is what it has always been. At this stage the two new doors
**lower** onto it: a sample becomes the `WidgetEvent` it describes and takes the
existing route. What has changed is that the vocabulary exists, that a handler
can read it, and that scroll routing now has a position to route by.

### 3.1 Scroll routing

`ScrollSample::position` decides where a scroll goes:

- `Some(p)` — hit-test `p`.
- `None` — the hovered widget, else the focused one. Today's rule.

A mouse wheel carries no position, so **this is a no-op for a mouse** (and a
test pins that). It exists for the synthesised pan a direct pointer will
produce: a contact never writes hover, so a positionless pan would route to
whatever the mouse last touched, or nowhere at all.

### 3.2 Reading the sample from a handler

```rust
.on_scroll(|_event, ctx| {
    if ctx.pointer_kind().is_coarse() { /* a finger is panning */ }
    match ctx.scroll_phase() { ScrollPhase::Momentum => …, _ => … }
    let where_ = ctx.pointer_position();
    EventResponse::Handled
})
```

`EventContext` exposes `pointer()`, `pointer_kind()`, `pointer_position()`,
`scroll_phase()` and `scroll_source()`. Outside a pointer or scroll dispatch — a
gesture timer, an assistive-technology action, a hand-built test context — they
report the mouse at the epoch, which is the same answer such a handler got
before pointers were distinguishable.

### 3.3 Cancellation

`WidgetEvent::PointerCancel { position, reason, pointer }` says the system took
the interaction away, as against `PointerUp`, which says the user finished it.
Conflating the two is how a drag whose window lost focus ends up *dropped*
wherever the pointer happened to be.

[`CancelReason`] enumerates the taxonomy in full — `Platform`,
`WindowDeactivated`, `ModalOpened`, `SubtreeParked`, `CaptureOrphaned`,
`PalmRejected` and the rest — so that the set is fixed before the producers
exist. **Nothing emits one yet.**

---

## 4. Tracing

An input bug is a *sequence* bug: the sample that mattered is three packets
back, and by the time a widget misbehaves the evidence is gone. Rather than take
on a logging dependency, the input layer carries one environment variable.

```console
$ TEKSILO_TRACE_INPUT=samples cargo run -p widget-catalog
[teksilo input] Down PointerId(2) at Point { x: 120.0, y: 44.0 } buttons=ButtonMask(1) t=EventTime(1.284s)
[teksilo input] Move PointerId(2) at Point { x: 121.0, y: 51.0 } buttons=ButtonMask(1) t=EventTime(1.301s)
[teksilo input] scroll Lines { x: 0.0, y: -1.0 } Discrete/Wheel at None
```

| value | traces |
| --- | --- |
| `samples` | raw pointer and scroll samples entering the tree |
| `gestures` | recognizer transitions, arbitration, cancellations |
| `all` | both |

Anything else, including an unset variable, is off.

The variable is read once through a `OnceLock`, and the `trace_input!` macro
**guards its arguments** — with tracing off, a trace call formats nothing,
allocates nothing, and does not evaluate its argument expressions. That is what
makes it acceptable to leave one on the per-sample path.

```rust
teksilo_core::trace_input!(Samples, "down {:?} at {:?}", id, position);
```

---

## 5. What is not here yet

Deliberately, and in this order: the per-pointer state table (so two contacts
can be tracked at once), gesture arbitration keyed by pointer, `TouchAction`
declarations, the kinetic scrolling core, the platform touch backends, the
density sweep across the widget catalogue, and touch text editing. Each has its
own package; this file grows with them.

See also: [Density & targets](density-and-targets.md), the
[widget pointer inventory](widget-pointer-inventory.md), the
[hover-affordance census](hover-affordance-census.md), and the
[drag-operation census](drag-operation-census.md).

[`PointerInfo`]: https://docs.rs/teksilo-core
[`PointerIdAllocator`]: https://docs.rs/teksilo-core
[`EventTime`]: https://docs.rs/teksilo-core
[`InputClock`]: https://docs.rs/teksilo-core
[`MonotonicClock`]: https://docs.rs/teksilo-core
[`ManualClock`]: https://docs.rs/teksilo-core
[`CancelReason`]: https://docs.rs/teksilo-core
