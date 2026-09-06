<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Touch & pen

Teksilo is a desktop framework whose input model was, until now, a mouse: one
pointer, always hovering, always precise, always present. Touchscreens and
digitizers break all four of those assumptions at once. This document describes
the vocabulary the framework uses to stop assuming them.

It is written alongside the migration, so it grows as the packages land. What is
here now is what exists now: the pointer model, the clock, the trace switch, and
the platform translator that turns an OS touch packet into pointer samples.

> **Status.** A mouse behaves exactly as it always has. The platform layer can
> now *produce* touch samples, but nothing dispatches them yet: the app event
> loop still feeds the tree the single-`WidgetEvent` mouse path, and the gesture
> recognizers still see the same stream they always did. Pen is not translated
> at all.

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

## 5. Platform capabilities

The translator that turns winit packets into samples lives in
`teksilo-platform/src/event_translation.rs`; the seam it sits behind is
`PointerBackend` in `teksilo-platform/src/pointer_backend.rs`. A backend
declares what it can report through `BackendCaps`, and **every `false` below is
a platform fact read out of winit 0.30's source, not a to-do**.

### 5.1 The matrix

| | Windows | macOS | Wayland | X11 |
| --- | --- | --- | --- | --- |
| touch at all | yes | **no** | yes | yes |
| `reports_cancel` | no | no | **yes** | no |
| `reports_pressure` | yes (`WM_POINTER`) | — | no | no |
| `reports_tilt` / `twist` / pen kind / palm | no | — | no | no |
| `reports_scroll_phase` | no | **yes** | yes | no |
| `reports_os_momentum` | no | **yes** | no | no |
| `reports_os_pinch` | no | **yes** | no | no |
| `synthesises_mouse_from_touch` | no | — | no | **yes** |
| `touch_window_drag` | no | no | **no** | no |
| `osk` | `ViaAccessibility` | `None` | `None` | `None` |

`BackendCaps::for_platform(PlatformKind, WindowSystem)` is the machine-readable
form, and it is a *pure function* — so every row above is asserted from any
host, including the two a Linux CI runner cannot boot.

`WindowSystem::Unknown` is exactly the set {Windows, macOS, headless}, because
`window_system_for_display_handle` only ever answers `Wayland` or `X11` from a
live Linux/BSD handle. That is what makes `Unknown` a safe default for the
dual-stream suppressors: none of those three platforms promotes touch to mouse.

### 5.2 What each backend actually does

**Windows.** winit calls `RegisterTouchWindow(hwnd, TWF_WANTPALM)` and answers
`WM_TOUCH` *and* the `WM_POINTER*` family, returning 0 without calling
`DefWindowProc`. There is therefore **no mouse promotion to fight** — and, until
this package, a Teksilo window received literally nothing for a finger.
`WM_TOUCH` reports no pressure ("WM_TOUCH doesn't support pressure information",
winit's own comment); the `WM_POINTER*` path normalises
`POINTER_TOUCH_INFO::pressure` over `1..=1024`. Tilt, twist, eraser and the palm
flag all exist in `POINTER_PEN_INFO` and `TOUCH_FLAG_PALM`, and winit 0.30
surfaces none of them.

**macOS.** Delivers **no touch at all** — `WindowEvent::Touch` is documented
"macOS: Unsupported". The trackpad arrives as `PinchGesture` /
`RotationGesture` / `DoubleTapGesture`, and a two-finger pan as `MouseWheel`
pixel deltas. This is the one platform where the OS owns the momentum, which is
why `reports_os_momentum` exists at all: a framework fling added on top of
AppKit's would double the coast.

**Wayland.** The only desktop backend that emits `TouchPhase::Cancelled`
(`wl_touch.cancel`). No pressure, no tool axes.

**X11.** XI2 touch. winit filters *emulated button* events by
`XIPointerEmulated`, but it synthesises a `CursorMoved` of its **own** for the
first concurrently-active contact — "Only the first concurrently active touch ID
moves the mouse cursor" — on every phase of that contact, at the contact's
location, through `util::VIRTUAL_CORE_POINTER`: the very device a real mouse
uses. The two are therefore indistinguishable at this layer, and the translator
suppresses the emulated stream wholesale while a contact is live. `force` is
`None // TODO`, and `TouchPhase::Cancelled` is never emitted.

### 5.3 The X11 residual

winit emits its synthetic `CursorMoved` **before** the `Touch` packet that
establishes the contact. So the very first move of a touch session that follows
more than 150 ms of quiet leaks exactly one mouse sample, at the touch-down
point. Closing it would need one event of lookahead, which would cost every real
X11 mouse move a frame of latency — so it is documented rather than paid for,
and pinned by a test
(`the_x11_phantom_motion_does_not_double_the_stream`).

### 5.4 Wayland cannot drag a window with a finger

`touch_window_drag` is `false` everywhere, and on Wayland that is worth stating
plainly rather than leaving as a gap: `xdg_toplevel::move` needs a serial from
an input event on a toplevel the compositor agrees the client owns, and winit
0.30's `drag_window` harvests a **pointer** serial internally. A finger cannot
reach it however the app asks. A custom title bar therefore stays mouse-only
under winit 0.30, and any touch-drag affordance has to be an in-app one.

### 5.5 The kill switch

`InputTokens::touch_enabled` is honoured **at the translator**, the first point
at which a finger becomes a Teksilo concept. With it `false` a touch packet
yields no sample at all: no `PointerId` is minted, no contact is tracked, no
suppressor arms, and the mouse translation is byte-for-byte identical. That is
the programme's rollback switch, and it has to sit at the producer for the
rollback to be total.

### 5.6 The winit 0.31 mapping

winit 0.31 replaces `WindowEvent::Touch` with a unified pointer API. The seam is
shaped so that upgrade is one package:

| winit 0.30 | winit 0.31 | reaches Teksilo as |
| --- | --- | --- |
| `WindowEvent::Touch { phase, id, location, force }` | `PointerEntered` / `PointerMoved` / `PointerButton` / `PointerLeft` with `PointerSource::Touch { finger_id, force }` | `PointerSample` with `PointerKind::Touch` |
| `Touch::id: u64` (reused after a lift) | `FingerId` | a fresh `PointerId` per press, either way |
| `CursorMoved` | `PointerMoved` with `PointerSource::Mouse` | `PointerSample` with `PointerKind::Mouse` |
| `MouseInput` | `PointerButton` | `PointerSample` with a `button` |
| — (no pen) | `PointerSource::Tablet` + the `TabletTool*` family | `PointerKind::Pen(..)` and the tilt/twist axes |
| `MouseWheel { phase: TouchPhase }` | `MouseWheel` with an explicit phase | `ScrollSample::phase` |

Two things the upgrade fixes for free: pen becomes reachable, and the scroll
phase stops being inferred. One thing it does not: winit 0.30's macOS backend
**collapses** `NSEvent`'s `phase` and `momentumPhase` into one `TouchPhase`, so
the OS's momentum arrives as a second `Started → Moved → Ended` run with nothing
in the event to distinguish it. The translator separates the two with a 100 ms
handoff window, and that heuristic is a winit-0.30 artefact that should be
revisited against 0.31's own phase reporting.

### 5.7 The conformance suite

`crates/teksilo-platform/tests/backend_conformance.rs` runs *recorded* winit
vectors — a Windows two-finger `WM_TOUCH` sequence, an X11 first touch with its
phantom `CursorMoved`, a macOS wheel-with-momentum ordering, an OS contact id
reused across two taps, a Wayland cancel — through six invariants:

1. **Identity is unique across OS id reuse.**
2. **Cancel completeness** — every `Down` is terminated by exactly one `Up` or
   one `Cancel`, never both, never neither.
3. **Time is monotone** within a stream.
4. **Primacy and hover** — at most one live pointer *of a kind* is primary
   (W3C `isPrimary`; the mouse is always primary in its own stream, and the
   cross-stream arbitration is the tree's pointer table, not the platform
   layer's), and no direct pointer ever hovers.
5. **One stream per contact** — the X11 double-stream case.
6. **Well-formed scroll phases** — `Began → Changed* → Ended`, `Momentum*` only
   after an `Ended`.

Nothing in it touches an OS: the vectors are `winit::event::WindowEvent` values
written out by hand from a reading of the backend that would produce them, so it
runs with no display, no touchscreen and no GPU. A future backend — winit 0.31,
a replay backend over a captured trace, a platform Teksilo has not met — earns
its trust by passing the same six.

---

## 6. What is not here yet

Deliberately, and in this order: wiring the app event loop to the multi-sample
translator (nothing dispatches a touch sample yet — the platform layer only
*produces* them), pen translation, gesture arbitration keyed by pointer, the
kinetic scrolling core, the density sweep across the widget catalogue, and touch
text editing. Each has its own package; this file grows with them.

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
