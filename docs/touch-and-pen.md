<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Touch & pen

Teksilo is a desktop framework whose input model was, until now, a mouse: one
pointer, always hovering, always precise, always present. Touchscreens and
digitizers break all four of those assumptions at once. This document describes
the vocabulary the framework uses to stop assuming them.

This page is the **model**: identity, the clock, the sample doors, the platform
seam, the pen, the press, the budget. Three companions carry the rest — the
dispatch and arbitration procedure is in
[Events & gestures](events-and-gestures.md), the sizing and hit-target side is in
[Density & targets](density-and-targets.md), and the obligations a widget author
signs up to are a numbered contract in
[porting-widgets-to-the-pointer-model.md](porting-widgets-to-the-pointer-model.md).

> **Status.** A mouse behaves exactly as it always has. The path is connected end
> to end: the app event loop routes `Touch`, `CursorMoved`, `MouseInput`,
> `MouseWheel` and the OS gesture family through the platform backend into the
> tree's sample doors, hands `CursorLeft` straight to the tree (there is no
> sample for a boundary crossing), drains the pen shim once per event-loop turn,
> and revokes every live pointer — in the tree and at the translator both — when
> a window is deactivated or occluded. A window closing drains the translator
> too, so the process-global pointer identities its contacts held are returned.
> Above it, a finger scrolls, selects on release, reorders behind a hold, selects
> text with handles and a magnifier, drags to and from the OS, pinches a scene,
> and reaches a 24 dp target; a pen draws with pressure and tilt and hovers.
>
> **Two things a running application still does not do right.** It does not
> switch density because a finger arrived
> (`DensityPolicy::FollowLastPointer` and `Environment::prefers_touch` have an
> ingress and no writer — what a stray tap should cost, whether a pen is coarse,
> and when hysteresis commits are unanswered). And it does not paint an overscroll
> (`ScrollableAxes::overscroll` publishes the value; nothing renders a stretch or
> a glow). §9 lists what a headless host cannot check and §10 is the standing
> ledger of what needs an owner's decision.
>
> The pinch payload the two producers hand over is now one contract, stated on
> `GestureEvent::PinchChanged`: `scale` is the factor **since the previous
> sample** and `rotation` the twist since the previous sample in **radians**. A
> touchscreen spread to twice the starting span therefore leaves the zoom at
> exactly twice rather than compounding into `max_zoom`, and a one-degree trackpad
> twist turns the content one degree rather than ~57 — winit reports degrees and
> the platform translator converts them at the seam. The one ingress both
> producers reach is described in
> [kinetic-scrolling.md §7](kinetic-scrolling.md); the payload contract lives on
> the event itself.

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

### 1.3 Primary, and the two other things it is not

Three notions used to be one field. They are not interchangeable, and the
framework keeps them apart on purpose.

| | What it is | Who decides | How many at once |
| --- | --- | --- | --- |
| `PointerInfo::primary` | The **W3C Pointer Events Level 3 per-kind flag**: every mouse event is primary, and so is the first touch of a sequence | the backend that produced the sample | one *per kind* — on a hybrid machine a mouse and a first touch are both primary |
| `PointerTable::primary()` | **Teksilo's** single pointer: the one backing `WidgetTree::hovered()` and `last_pointer_position()` | the table, by election | exactly one, mouse preferred, else the oldest live pointer |
| `PointerTable::hover_owner()` | The most recent **hovering-capable** pointer — a mouse, or a pen in proximity | the table, on each hovering sample | at most one; often none (a touch-only device has none at all) |

`PointerInfo::touch(..)` deliberately constructs a **non**-primary pointer:
primacy is a fact about the whole set of live pointers, so only the table can
decide it, never a constructor.

### 1.3.1 A contact never produces hover

**`on_hover` and every hover signal are hover-owner-only.** Enter/leave, the
cursor shape, tooltip dwell and `hover_within` all follow the hover owner and
nothing else; a touch contact is never the hover owner, and a finger arriving
beside a hovering mouse leaves every one of them exactly where it was.

This is a rule, not an omission. A finger has no hover state to report: were a
contact to write hover, every hover affordance would fire on tap and then stay
lit after the lift, because there is no "moved away" event to turn it off. The
affordances that today depend on hover get their **own** touch routes; they do
not get them by pretending a finger hovers.

When two hovering-capable pointers move in the same frame — a mouse and a pen —
the **later sample wins** the role, and the displaced owner is sent a
`PointerLeave` for the widget it was over, so nothing stays lit for a pointer
that is no longer pointing at it.

### 1.3.2 Capture is per pointer

`EventContext::capture_pointer()` captures **the pointer whose sample the
handler is serving**, and that capture is released only by *that* pointer's Up
or Cancel. Two contacts pressing two widgets therefore hold two independent
captures, and one lifting can no longer steal the other's stream — which is
exactly what a single `pointer_captured_by` used to do. `capture_pointer_id(id)`
names a different pointer explicitly; `owns_pointer()` answers whether the
handler's own widget already holds the capture of the pointer it is serving.

Nothing changes for a mouse: there is one mouse, and `capture_pointer()`
captures it.

**Localisation follows the captor's *current* bounds.** A captured widget is
re-localised against its live arena rectangle on every event, not against the
rectangle it had when the capture began — so a slider inside a container that
slides keeps reporting sensible widget-local coordinates instead of coordinates
relative to where it used to be.

### 1.3.3 The contact cap

The table holds **ten** pointers: the maximum simultaneous contacts a Windows
digitiser and a Wayland `wl_touch` seat both report. An eleventh arrival is
refused at the door and produces no event at all, rather than evicting one of
the ten — evicting a live contact mid-gesture is how a pinch turns into a
fling. So is any sample the backend flagged as a palm
(`PointerInfo::palm`). Both refusals are traced (§4), because a dropped sample
that leaves no evidence is the hardest input bug there is.

### 1.3.4 A nested dispatch waits its turn

A handler that dispatches while a dispatch is in flight — a synthetic click, an
assistive-technology action re-entering the door — is **queued**, not run
inline. Run inline it would unwind the pointer state the outer sample is still
standing on, and the outer handler would return to a tree whose hover, capture
and table entries had all moved under it.

The queue is drained, faithfully (event *and* input snapshot), the moment the
outer dispatch completes, so from a caller's side nothing changed: the queue is
empty again before the top-level `dispatch_*` call returns.

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

The shared epoch is what makes the two axes *comparable*; what makes them one is
that they change hands together — `advance_time` takes the tree off the wall
clock before it moves anything, and `resume_real_time` puts **both** axes back
on it in one call. While time is taken over,
`input_now()` is a reading of `sim_clock` and the animation scheduler is ticked
at `sim_clock`, so a deadline can only be reached by advancing the clock — never
by the test taking a long time, and never *not* reached because it did.

That is one flag, `sim_time_frozen`, and it is **not a latch**. A headless test
never gives time back, so for a test it behaves like one. A host sharing a live
tree with a real event loop — the debug automation bridge — calls
`WidgetTree::resume_real_time()` when the operation ends, and real time drives
the tree again until the next advance. Leaving it frozen would stop the attached
window measuring another gesture, and stop its animations advancing at all.

The two axes are handed over and handed back differently, because they carry
different state:

* **The input axis carries no stored instants, so the *reading* is carried.**
  Taking it over continues it from the reading the wall clock had at the switch
  rather than restarting it at the simulated clock's own offset, so a sample
  stamped before the switch stays in the past instead of landing in the virtual
  future where no interval measured from it could elapse. Handing it back cannot
  simply drop that anchor: an axis advanced by more than the wall clock moved
  meanwhile reads *ahead* of the raw clock, and dropping it would step
  `input_now()` backwards over times already handed out. So the gap is measured
  — afresh at each hand-back, against that hand-back's own readings, and floored
  at zero for the case where the raw clock is already the later of the two — and
  carried in `sim_input_offset`, which every later reading adds. `instant_for` subtracts it again when it reports a deadline to the event
  loop, which is what keeps a `WaitUntil` in the future rather than in the past,
  where the loop would spin on it.
* **The animation scheduler stores absolute instants, so *they* are carried.**
  Each switch rebases every instant the scheduler holds by the gap between the
  two clocks (`AnimationScheduler::rebase`), and the reading is then simply
  whichever clock is in charge. Without the rebase both directions fail, in
  opposite ways: taking time over would measure a wall-clock-stamped animation
  against a simulated clock far behind it and clamp its elapsed time to zero for
  good, and handing it back would measure a simulated-clock-stamped one against
  the wall clock and complete it on the first real frame.

This is not a nicety. The animation scheduler already had to be rescued from the
two-clock version of this bug (see `WidgetTree::animation_clock`): while real
time and simulated time advanced independently, every animation armed after real
time overtook simulated time had a start in the scheduler's future and froze
completely — a failure that tracked machine load rather than behaviour. Giving
the input layer its own unrelated epoch would have re-created exactly that,
one subsystem over.

See [automation-mcp.md](automation-mcp.md) § *Time, and handing it back* for the
hand-back's call sites on the bridge side.

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

#### The contract, for a widget author

**`PointerCancel` is terminal.** No `PointerUp` follows it for that pointer;
the framework swallows one that arrives anyway, because handing a release back
to a widget that has already been told to let go would resurrect an interaction
that no longer exists.

**Release your own state.** The framework releases what it owns — the pointer
capture, the arbitration, the drag session, the recognizers — and nothing else.
Anything the *press* latched is yours to undo: a selection anchor, a grabbed
divider's origin, a drop-target highlight, a `PendingTextDrag`, a preview
overlay you mounted yourself. A widget that only clears such state on `Up`
keeps it forever the first time an interaction is revoked.

**Expect no `Up`, and do not treat the cancel as one.** A drag that ends in a
cancel must not drop; a press that ends in a cancel must not activate. That is
the whole reason the two events are distinct.

Attach with `.on_pointer_cancel(|pointer, reason, ctx| …)`, on `WidgetBuilder`,
`HandlerSet` and `WidgetWithHandlers`. It is a notification, not a route: it has
no return value and cannot consume anything. A widget that drives the whole
pointer stream from `on_pointer_event` sees the cancel there too.

#### One funnel

Every revocation goes through `WidgetTree::cancel_pointer(pointer, reason,
ops)`, which is **always queued** behind the sample being dispatched and
**no-ops if the interaction finished** in the meantime. It then tears down in
one order: every competitor's recognizer state, the arbitration, the capture,
the drag session, the table entry (for a contact — a hovering pointer keeps
its entry, exactly as it does across an `Up`), and the `PointerCancel` last, to
a tree that has already let go. `cancel_all_pointers` and
`cancel_pointers_in_subtree` are the same act in bulk;
`EventContext::cancel_pointer_sequence(reason)` is a widget's own door into it.

**Two granularities, and they are not the same act.** Cancelling a *pointer*
ends the interaction. Revoking a *sequence member* ends one competitor's claim
while the pointer stays alive and its winner keeps going — that is what a peer
claim does, and confusing the two would make a losing ancestor drag kill the tap
that beat it.

#### The taxonomy

| `CancelReason` | Raised by | Delivered to | What the widget must do |
| --- | --- | --- | --- |
| `Platform` | A `PointerPhase::Cancel` sample from the backend — `wl_touch.cancel`, `WM_POINTERCAPTURECHANGED`, a compositor grab | The captor, else the last widget that accepted one of this pointer's events | Release everything the press latched. The contact is gone; nothing further will arrive. |
| `WindowDeactivated` | `WidgetTree::set_window_active(false)` | The captor | Drop the grab. The user is releasing the button over another window and this one will never hear about it. |
| `Occluded` | Reserved for the platform layer's occlusion path | The captor | As `WindowDeactivated`. |
| `ModalOpened` | A `Centered` overlay opening (`show_overlay*`) | Every live pointer's captor | Abandon the press: the surface is behind a scrim and the `Up` will land on the modal. |
| `SubtreeParked` | `WidgetTree::park_subtree` — an explicit `set_dormant`, a `visible_when` gate closing, an `EventContext::set_dormant` | Pointers whose captor is inside the parked subtree | Release the press. Dormancy is invisible to dispatch, so no further event can reach you. |
| `WidgetDestroyed` | A sequence *member* destroyed mid-press (member-level), or the sequence *winner* destroyed (pointer-level) | The dead member, if it still exists; else nobody | Nothing, usually — the widget is going away. |
| `CaptureOrphaned` | The captor destroyed mid-press | The last widget that accepted one of this pointer's events | Nothing, usually. The capture is given back so the window is not stranded. |
| `OsDragStarted` | An in-app drag escalating past the window edge into a native OS drag | The drag's source widget | Stop tracking the pointer. The OS owns it; the drag's outcome arrives separately through `on_drag_ended`. |
| `ExternalDndTakeover` | Reserved for the inbound external-DnD path | The captor | As `OsDragStarted`. |
| `PeerClaimed` | Another member of the sequence won arbitration | That member alone — **member-level** | Undo whatever the press provisionally started. The pointer is alive and belongs to someone else now. |
| `OverlayDismissed` | An overlay being torn down, for pointers anchored inside it | The captor inside the dismissed overlay | Release the press. **Exempt**: a pointer whose press has already finished — see below. |
| `MultiContactIgnored` | Reserved for `MultiContact::First` (P13) | The extra contact's target | Ignore the second finger. |
| `ContactCapExceeded` | Refused at `PointerTable::begin`, before any event exists | Nobody | — |
| `PalmRejected` | Refused at `PointerTable::begin`, before any event exists | Nobody | — |
| `Deactivated` | A catch-all for a revocation that fits nothing above | The captor | Release the press. |

The reserved rows name variants whose producer belongs to a package that has
not landed. They are in the enumeration because the taxonomy is meant to be
fixed before the producers exist, not grown one boolean at a time.

#### The exemption: a tap that closes what it was tapped in

Tapping a menu item whose own handler closes its menu must complete the tap.
The item asks for the overlay teardown, the teardown raises
`cancel_pointers_in_subtree(menu, OverlayDismissed)`, and that pointer is
sitting inside the menu — so without an exemption the item would cancel its own
activation.

It does not, because by the time an item's `on_tap` runs, that pointer has no
press left to revoke: the release sweep closes the sequence *before* the `Up` is
delivered. `cancel_pointer` no-ops for a pointer with no live sequence and no
capture, and skips a sequence that is already inside its terminal dispatch
(`PointerSequence::is_terminating`). Both conditions say the same thing — there
is nothing left to take away.

#### The `set_dormant` audit

Parking is invisible to hit-testing and to dispatch, so a widget parked
mid-interaction keeps whatever the press latched and never receives another
event. `WidgetArena::set_dormant` therefore **returns the whole parked subtree**,
and every caller that could park a live pointer goes through
`WidgetTree::park_subtree`, which cancels first and parks second. The complete
caller set in `teksilo-core`:

| Site | Route |
| --- | --- |
| `widget_tree/layout_impl.rs` — the per-layout visibility pass (`visible_when` closing) | `park_subtree_with_ops` |
| `widget_tree/test_api.rs` — `WidgetTree::set_dormant`, which `BuildContext::set_dormant` calls | `park_subtree` |
| `widget_tree/pointer_router.rs` — `TreeMutation::SetDormant`, from `EventContext::set_dormant` | `park_subtree` |
| `widget_tree/overlay_impl.rs` — `dormant_dismissed_content`, an overlay being torn down | `cancel_pointers_in_subtree(OverlayDismissed)` then `arena.set_dormant` — a more specific reason than `SubtreeParked`, and the one the exemption above is written against |
| `widget_tree/overlay_impl.rs` — tooltip content parked at registration | Bare `arena.set_dormant`. **Cannot contain a pointer**: the content is created and parked in the same call, before it has ever been laid out or hit-tested. |

A caller that discards the returned ids and does not appear above is a silent
leak.

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
| `osk` | `Explicit` | `None` | `None` | `None` |

The `osk` row is the one capability with a whole page behind it — what
`Explicit` obliges a backend to do, and why three of four platforms answer
`None` for reasons that are not laziness. See
[Soft keyboard](soft-keyboard.md).

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
reused across two taps, a Wayland cancel — plus recorded pen sessions driven
through `poll_pen`, one of them a **coalesced batch** arriving in a single drain
with the device's own stamps on it, through six invariants:

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

## 6. Pen and stylus

winit 0.30 exposes **no pen API at all**. On Windows its `WM_POINTER` arm
already decodes pen packets and hands the app nothing; on Wayland
`zwp_tablet_v2` is never bound. A stylus therefore reaches a winit 0.30 client
either as a mouse without axes (Windows, via the OS's own promotion) or as
nothing whatsoever (Wayland).

Waiting for winit 0.31 would make pen this programme's one genuine deferral, so
Teksilo ships two shims now, behind a seam that is deleted at the upgrade:
`teksilo-platform/src/pen.rs`, with `pen/{wayland, windows, null}.rs` beside it.

### 6.1 The seam

```text
  OS                     PenSource::poll        TranslationState::poll_pen
  zwp_tablet_tool_v2 ─┐
  WM_POINTER* ────────┼─▶  Vec<PenPacket>  ─▶  Vec<PointerSample>
  (nothing) ──────────┘                         PointerKind::Pen(tool)
```

A `PenSource` is **pulled**, not pushed: both backends buffer packets off the
event path — a Wayland dispatch thread, a Win32 subclass procedure — and the
caller drains them once per event-loop turn. That keeps the OS callbacks free of
Teksilo state, and it hands the translator the caller's clock rather than a
device one, which is what the one-clock rule (§2) demands.

**A drain is not one instant.** The packets in it are separate digitizer frames
that happened at separate times, and a poll that stamps every one of them with
its single `now` — which is what `poll_pen` did — collapses a whole stroke onto
one timestamp. Everything computed from the gaps between samples is then
computed from zero: velocity, any temporal smoothing, and the per-sample time
offsets an ink representation stores.

Each packet therefore carries the device's own millisecond counter in
`PenPacket::device_time_ms` — Wayland's `frame` time, Win32's `dwTime` — and
`pen::back_date` turns a drained batch into one `EventTime` per packet:

- The **newest packet is `now`**. It is the one the poll's clock actually
  describes, so a batch of one is stamped `now` exactly, which is what every
  packet used to get.
- Each earlier packet sits at the device's own delta before the one after it,
  read with `wrapping_sub` so a `u32` counter rolling over inside the batch
  costs nothing. A delta long enough to be the counter running *backwards*
  falls through to the step below rather than back-dating the stroke into the
  last century.
- Where a packet reports no device clock at all, the step is one
  `PEN_POLL_INTERVAL` divided evenly across the batch — so the run still fits
  inside the window it was buffered in, and separate frames are still separate
  instants.
- One clamp on top: **no sample is stamped earlier than the translator's
  previous `now`**. A shim filling its buffer from its own thread can hand over
  a packet the device stamped before the last drain returned, and this window
  has already told the tree that time had reached `now`. Invariant 3 of the
  conformance suite (§5.7) — time is monotone — is a promise to every consumer
  downstream.

The device counters themselves never escape the platform layer. Their epoch is
unknown — Wayland's is the compositor's, Win32's is `GetTickCount`'s — so as an
absolute each is a lie dressed as precision, and only their *differences* are
ever read. That is why `device_time_ms` is a raw `u32` and not an `EventTime`,
and why `TranslationState::translate_pen_packet` takes the tree-timeline time
as an argument instead of reading one off the packet: the caller owns the
clock, always.

A `PenPacket` is a **level, not an edge**: it describes the tool's whole state
at one instant, and the translator derives the transitions by comparing
consecutive packets. Both backends produce that shape naturally (Wayland
accumulates axes and commits them on `frame`; Win32 fills one `POINTER_PEN_INFO`
per message), and it means a dropped packet costs a sample rather than leaving a
button stuck down.

`InputTokens::touch_enabled` deliberately does **not** gate the pen: a stylus is
not a finger, and rolling touch back must not take pen input with it. The pen's
off switch is not installing a source.

### 6.2 The support matrix

| | pen at all | tool kind | pressure | tilt | twist | contact patch | per-sample time | coalesced packets |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Wayland | `zwp_tablet_v2` | yes | yes | yes | yes | — | `frame`'s `time` | one packet per `frame`, all drained |
| Windows | `WM_POINTER*` subclass | yes | yes | yes | yes | **yes** (touch) | `dwTime` | `GetPointerPenInfoHistory` |
| X11 | — | no | no | no | no | no | no | no |
| macOS | — | no | no | no | no | no | no | no |

The last two columns are what makes the stylus rate real rather than nominal.
Wayland delivers one packet per `frame` into an accumulating queue and the poll
drains all of them, so the full 200-360 Hz stream already reaches the widget
tree. Windows delivers one *message* per display frame with the rest folded into
its history buffer, which is why the shim reads that buffer (§6.6). Both then
get their own timeline (§6.1).

A source folds its capabilities into the window's `BackendCaps`, raising
`reports_pen_kind` / `reports_pressure` / `reports_tilt` / `reports_twist`. It
only ever *raises* them: a shim adds what winit lacks, it never takes a
capability away. X11 and macOS get `NullPenSource`, whose `PenCaps::NONE` leaves
every row `false` — the absence is **declared**, not faked, so a consumer asking
"does this window report tilt?" reads `false` instead of a zero that might have
been a measurement.

Both could grow a shim — X11 has XInput2 valuators, macOS has `NSEvent`'s
`tabletPoint` / `tabletProximity` subtypes. Neither is written, for the same
reason: winit 0.31 supplies both, and a shim written now would be deleted before
it earned its maintenance. The two that *are* written cover the platforms where
a stylus is common and the data is already flowing past the app unread.

### 6.3 Proximity is a first-class state

A pen in proximity with **no contact** is a hovering pointer. It moves, it
drives hover visuals, tooltips and the cursor exactly as a mouse does, and it
does so with `down: false`. That is why `PointerKind::hovers()` is true for
`Pen` and false for `Touch`, and it is the single biggest behavioural difference
between a stylus and a finger.

The translator's machine, per packet, comparing against the session's previous
state:

| transition | sample |
| --- | --- |
| out of range → in range | `Move` (a hover: no buttons, `down` false) |
| position changed | `Move` |
| tip touched down | `Down` with `Primary` |
| tip lifted | `Up` with `Primary` |
| barrel pressed / released | `Down` / `Up` with `Secondary` |
| second barrel | `Down` / `Up` with `Middle` |
| in range → out of range | `Cancel` |
| tool changed mid-session | `Cancel`, then a fresh session |

Within one packet the `Move` is emitted **first**, so a press always lands at a
position the consumer has already seen.

One `PointerId` is minted per **proximity session**, not per tip contact: the
tip touching and lifting inside that span are button transitions on one pointer.
That is the W3C model, and it is what lets a hovering stylus keep a tooltip open
across a tap. A tool change inside a session — the stylus flipped to its eraser
— ends the session and starts a new one, because a drawing surface is entitled
to treat the eraser as a different pointer.

**Why leaving proximity is a `Cancel`.** `PointerPhase` has no *leave*, and a
tool going out of range completes nothing: the completion, if there was one, was
the tip's `Up`, already delivered. `Cancel` is the phase that says "this
pointer's life ended without completing an interaction" — and it is right for
the down case too, where a stylus yanked off the tablet mid-stroke must not read
as a deliberate lift.

Two consequences for the conformance suite (§5.7), both owed by the package that
routes pen samples into the tree rather than by the platform layer:

- Invariant 4 says *"a direct pointer never hovers, so a buttonless move is
  impossible"*. That is touch-shaped, and `PointerKind::hovers()` already
  contradicts it for `Pen`. It needs to read "a **coarse** pointer never
  hovers".
- Invariant 2 counts one terminator per `Down`. A pen session that draws and
  then leaves emits `Down → Up → Cancel` for one id, which is correct for a
  hovering-capable pointer and two terminators by that counting.

Neither bites today: pen samples arrive through `TranslationState::poll_pen`,
not through `PointerBackend::translate`, and the suite drives only the latter.

### 6.4 Buttons and the eraser, normatively

- Pen **contact** is `PointerButton::Primary`. This is not cosmetic: every
  `accept_buttons()` recognizer in the framework gates on `ButtonMask::PRIMARY`,
  so a stylus reporting anything else would be invisible to tap, drag and
  long-press alike.
- The **barrel** button is `Secondary`, matching W3C Pointer Events (pen barrel
  → `button` 2).
- A second barrel button, where the hardware has one (`BTN_STYLUS2`), is
  `Middle`.
- The **eraser is a tool kind** (`PenKind::Eraser`), never a button. Windows
  reports it as `PEN_FLAG_INVERTED` (stylus flipped) or `PEN_FLAG_ERASER`
  (eraser button on a stylus that has one); both fold to the tool before the
  packet leaves the source. `PenButtons::ERASER` exists only so a backend can
  carry the raw bit faithfully, and it never becomes a dispatched button.

### 6.5 Wayland: `zwp_tablet_v2`

The connection model is the one `external_dnd/wayland.rs` proved: wrap winit's
live `wl_display` with `Backend::from_foreign_display`, run `registry_queue_init`
on it, bind `zwp_tablet_manager_v2`, ask for the tablet seat, and drain **our**
queue from a dedicated thread.

The rule that comes with it, restated because breaking it aborts the process:
**never read the socket**. winit's event loop is its sole reader; a second
reader (`blocking_dispatch` → `prepare_read` / `read_events`) is fatal in
libwayland. The multi-queue model buffers events for our objects whenever
*anyone* reads, so `dispatch_pending` on a short interval is both correct and
sufficient. The pen thread polls every **4 ms** where the drag backend polls
every 8: a stylus is a continuous input a user watches ink follow, and 4 ms is a
quarter of a 60 Hz frame.

**But only while a tool exists.** Binding the protocol says nothing about the
hardware: a compositor advertises `zwp_tablet_manager_v2` whether or not a
digitizer is attached — the machine this was written on advertises it at
version 2 with a touchpad, a keyboard and no tablet at all — so on a modern
desktop most windows get a shim and most of those will never see a packet. At 4
ms that is 250 timer wakeups a second per window for the life of the
application, on a machine with no stylus in the building, which is exactly the
idle cost the rest of the framework is built to avoid. The protocol answers the
question itself: a tablet seat announces `tool_added` before any tool can be in
proximity, so `WaylandPenSource::poll_interval` stands the thread down to **250
ms** until one is announced and returns it to 4 ms the moment one is — at bind
time for an already-plugged tablet, within one idle interval for one plugged in
later, which is well before a hand can reach the pen. Nothing is dropped in that
window; the events are buffered in our queue and dispatched at the next look.

**The `frame` carries the time.** `tablet-v2.xml` defines `frame`'s `time`
argument as "the time of the event with millisecond granularity", and since a
`frame` is exactly what commits a `PenPacket`, that stamp is the packet's. It
reaches `PenPacket::device_time_ms` and is read only as a difference (§6.1). The
one packet with no frame behind it is the leave a `removed` tool synthesises —
a removed tool never frames again, so the leave is committed on the spot,
carrying the last frame's stamp. A tool removed **before** it ever framed
carries no stamp because it produces no packet: nothing framed, so no hover was
ever announced, and `commit`'s `!in_proximity && !reported_proximity` guard
retracts nothing rather than synthesising an undated leave.

Motion arrives in **surface-local** coordinates, which on Wayland are already
logical — the compositor has divided by the buffer scale. So this arm passes
positions straight through, where the Windows arm reads screen *physical* pixels
and divides. winit does the same conversion in the other direction (it
*multiplies* surface-local by the scale factor to report physical), which is why
the two arms look inconsistent and are not.

**Tools that are not pens.** `zwp_tablet_tool_v2::Type` names eight tools; two
are not styluses, and Teksilo drops both rather than mislabel them:

- `Finger` is a finger on the tablet surface. It is a touch contact, it already
  arrives as one through `wl_touch`, and calling it `PointerKind::Pen` would
  give a fingertip a stylus's tuning — tight slop, no hit outset,
  precise-pointer affordances — which is exactly backwards.
- `Mouse` is a puck-style tablet mouse: an indirect device with no tip pressure,
  whose events do not reach `wl_pointer`. `PenKind` has no variant for it, and
  forcing one would report an indirect pointer as a direct one. **This is a
  known gap**: a tablet mouse produces no Teksilo input at all under the shim.

`Lens` — the other puck — *is* mapped, to `PenKind::Lens`: it is an
absolute-positioning tool on the tablet surface, which is what "direct" means
here. `wheel`, `ring`, `strip` and `slider` are out of scope; the pad half of
the protocol is bound only far enough that a tablet with a button pad does not
take the app down (an unhandled `new_id` event is a runtime panic in
wayland-client, not a silent drop).

### 6.6 Windows: a `WM_POINTER*` subclass

winit 0.30 already receives `WM_POINTERDOWN` / `WM_POINTERUPDATE` /
`WM_POINTERUP`, calls `GetPointerType`, and turns the result into a `Touch` or a
mouse event. What it never does is call `GetPointerPenInfo` — so the pressure,
tilt, rotation and eraser bits the digitizer is already sending arrive at the
window and are dropped. The shim reads them off the same messages, one subclass
earlier.

**It is a tap, never a filter.** Every message is passed on with
`DefSubclassProc`, unconditionally, including the ones we read. Nothing changes
what winit sees, so the mouse and touch streams are byte-for-byte what they were
and the shim can be removed with no behavioural diff. `EnableMouseInPointer` is
deliberately *not* called: it would route the mouse through the pointer family
too and change winit's own input path.

**Coexisting with the two other subclasses.** An HWND in a Teksilo app can carry
three at once — AccessKit's (`WM_GETOBJECT`), the custom title bar's (`WM_NC*`,
`WM_DPICHANGED`) and this one (`WM_POINTER*`). They coexist because each obeys
the same two rules:

1. **A unique subclass id**, from a process-wide counter in a private range
   (the title bar's starts at `0xFE_111_000`, the pen's at `0xFE_112_000`).
   Win32 keys the chain on `(proc, id)`, and a collision silently replaces
   another subclass's entry.
2. **Always chain.** Every path ends in `DefSubclassProc`, so the rest of the
   chain and finally winit's own window procedure run exactly as before.

Message disjointness makes that safe rather than merely polite: the three sets
do not overlap. A subclass that will not install is logged once and degrades to
the null source — a missing pen is a missing capability, never a broken window.

**Touch contact geometry comes along for free.** `WM_TOUCH` — the path winit
0.30 takes for fingers — carries no contact area. `POINTER_TOUCH_INFO::rcContact`
does, and both the palm heuristic and finger-avoiding overlay placement want it.
The same subclass records the patch per contact id, and the translator folds it
into `PointerAxes::contact` for the matching winit `Touch`. This is the one
place a *pen* source answers a question about a finger, and it is worth the
oddity: nothing else in winit 0.30 can answer it.

**A message is not a packet.** Windows coalesces the digitizer packets that
arrive between two window messages and reports how many in
`POINTER_INFO::historyCount`; `GetPointerPenInfo` — the singular form — hands
back only the newest of them. A shim built on that alone is capped at the window
message rate, roughly the display's, no matter how fast the tablet is, and every
intermediate packet's pressure and tilt goes with it. So the shim calls
`GetPointerPenInfoHistory` whenever the count says there is more, and appends the
whole run.

Two details of that API are worth stating because getting either wrong is
silent:

- **`historyCount` is read from the `POINTER_INFO`**, not guessed. `1` means
  nothing was coalesced (and `0` is what a driver that does not fill the field
  leaves behind); either way the extra call is skipped, because it would buy one
  duplicate packet. Above that, the count is clamped to
  `MAX_PEN_HISTORY_ENTRIES` before it sizes a buffer — a length a driver writes
  is not a length to allocate from.
- **The array comes back newest-first.** `PenSource::poll` promises oldest-first,
  because the caller reads a drained batch as one forward timeline and back-dates
  it from the last entry. `decode::decode_pen_history` is the reversal between
  the two conventions, and it is the single most reversible mistake in the file:
  get it wrong and every stroke is drawn backwards, with its pressure ramp
  inverted and its velocity pointing the wrong way, which looks like a rendering
  bug rather than a sort order. Four tests fail if the `reverse()` goes away.

Each history entry is a whole `POINTER_PEN_INFO` with its own `dwTime`, its own
pressure and its own tilt — which is the entire reason to make the extra call.
The leave path (`WM_POINTERLEAVE`) deliberately reads no history: a leave is one
transition, not a stroke, and replaying its coalesced positions would say the
tool left several times.

**Normalisations.** `POINTER_PEN_INFO::pressure` is `0..=1024` (`0 → 0.0`,
`512 → 0.5`, `1024 → 1.0`); `tiltX` / `tiltY` are `-90..=90` degrees;
`rotation` is `0..=359` degrees and becomes `twist`. An axis whose `PEN_MASK`
bit is clear is `None`, never a zero pretending to be a measurement — except
pressure, which becomes `1.0` while the tip is down, because a tip in contact
has *some* pressure and `0.0` would make a pressure-driven brush paint nothing
on hardware with no sensor. Tilt is both-or-neither: half a pair is worse than
none, since a brush angle computed from a real `tilt_x` and a zeroed `tilt_y` is
confidently wrong.

### 6.7 Testing a shim on a machine that cannot run it

Neither shim can be exercised on a Linux CI runner with no tablet, so the
acceptance for the platform arms is that they compile for their target and that
the *decoding* is tested target-independently:

- `pen/windows/decode.rs` is compiled on **every** target and parses a
  `&[u8]`, so recorded layouts decode on a host with no Windows. The live path
  hands it the bytes the OS just wrote, so tested code and shipped code are one.
  The offsets are Microsoft's field order laid out by the Windows x64 C ABI —
  *derived*, not captured — and they are not taken on trust: compiled **for**
  Windows, the module asserts every offset against `windows-rs`'s own
  `POINTER_PEN_INFO` with `offset_of!`, so a wrong constant is a build failure
  on the platform that matters.
  The same split carries the coalescing history: `decode_pen_history` takes the
  `&[u8]` the OS would have filled and returns decoded entries, so its
  newest-first-to-oldest-first reversal, its short-buffer handling and its
  clamped request size are all exercised from Linux against synthesised
  `POINTER_PEN_INFO` images. What the Windows target contributes is the other
  jaw of the pincer: `offset_of!(POINTER_INFO, historyCount) == HISTORY_COUNT`
  is a `const` assertion, so `cargo check --target x86_64-pc-windows-msvc`
  fails on a wrong offset from a host with no Windows on it.
- `pen/wayland.rs` keeps all its logic in `ToolState`, which knows nothing about
  wayland-client and is driven by recorded `zwp_tablet_tool_v2` sequences.
  Below it, `translate_tool_event` is tested **at the protocol layer**, against
  real `zwp_tablet_tool_v2::Event` values built in the test: the enum is plain
  data, so a `Frame`, a `Motion`, a `Button` or a `Type` can be constructed on a
  host with no compositor and pushed through the decode. That test exists
  because the decode is not a variant rename — it moves *numbers*, and a dropped
  field compiles and runs. It is exactly how the frame stamp went missing: the
  arm read `E::Frame { .. }` and the compositor's own millisecond clock never
  reached the packet, so a whole drained batch claimed one instant. Every arm
  that reads a field is now pinned, including the ones deliberately dropped
  (`distance`, `slider`, `wheel`, the identity burst), so a drop stays a
  decision rather than becoming an accident.
  The one exception is `proximity_in`, which carries live `zwp_tablet_v2` and
  `wl_surface` proxies: a `Proxy` cannot exist without a connection, so that arm
  has no off-device test and would need a fake Wayland server to get one. It is
  the safest arm to leave uncovered — `ToolEvent::ProximityIn` has no default
  for `surface`, so a decode that stopped reading it would not compile, and one
  that read the *wrong* surface would put every stroke in the wrong window. What
  *consumes* the id is covered.
  What is still untested by construction is the glue in the `Dispatch` impls:
  look the tool up by `ObjectId`, call `ToolState::apply`, drop a removed tool.
  It transforms nothing, and reaching it needs a proxy.
- The translator's proximity machine is tested through a scripted `PenSource`,
  so hover, contact, buttons, tool change and `cancel_all` are all covered with
  no device at all — and so is the batch timeline: a scripted drain carrying the
  device stamps a digitizer would produce must come out strictly increasing, at
  the device's own spacing, with the newest sample on the poll's clock.
- `pen::back_date` is a pure function over `&[Option<u32>]`, so the placement
  rule itself — wrap, backwards stamp, missing clock, equal stamps, the epoch
  and `now` clamps — is unit-tested with no digitizer, no shim and no
  translator.

What none of that covers is the OS boundary itself: that the subclass installs
alongside AccessKit's, that a real compositor's tablet seat behaves as the
protocol says, that a Wacom's `dwTime` and frame cadence are what the
documentation claims, and that `GetPointerPenInfoHistory` really orders its
array the way Microsoft documents. Those belong on a hardware checklist, not in
CI.

### 6.8 Both shims have an expiry date

winit 0.31 supersedes them with its own `TabletTool*` events and
`PointerSource::Tablet`. At that upgrade `pen/wayland.rs` and `pen/windows.rs`
are **deleted**, `create_pen_source` answers `NullPenSource` everywhere, and the
translator reads the pen off winit like every other device. Nothing above the
seam changes: `PenPacket`, the proximity machine and the button semantics are
Teksilo's, not winit's.

---

## 7. The press

A mouse press and a finger press are not the same act. A mouse is placed
exactly and stays where it is put; a finger lands on an estimate, wanders while
it rests, and is very often the beginning of a scroll rather than the beginning
of a choice. Three consequences run through this section: the framework, not
each widget, decides when a control looks pressed; a direct pointer's focus is
decided by its **release**; and the surviving press-time actuations are named
and justified rather than left to accumulate.

### 7.1 The framework press

The router keeps one record per live press, keyed by the node whose gesture
arena took it — the node whose `on_tap` would fire. Recipes read it as one
signal:

```rust
// in build()
let pressed = ctx.pressed_signal();   // Signal<bool>, bind at RepaintOnly
```

and the tree answers three separate questions about it, because a press and a
press *visual* are not the same thing:

| question | true when |
| --- | --- |
| `WidgetTree::pressed_by(id) -> Option<PointerId>` | the node is held, wherever the pointer has since moved |
| `WidgetTree::press_is_inside(id)` | …and the pointer has not left the press's tap boundary |
| `WidgetTree::is_pressed(id)` | …and the press-feedback delay has elapsed |

`is_pressed` is what `pressed_signal` mirrors and what a recipe paints. From
inside a handler the same three are on `EventContext`, for the pointer being
dispatched: `is_pressed()`, `press_is_inside()`, `press_pending()`.

A press visual says "release here and this control acts", so it answers to the
same buttons the activation does: the union of the owner's own click-style
`ButtonMask`s — `PRIMARY` unless the widget widened it with
`accept_tap_buttons` and friends. A middle-click, or a right-click on a node
with no context menu, therefore lights nothing up; a widget that widened its
mask lights up on the buttons it widened to. A node whose arena also *drags* or
*swipes* — neither recognizer carries a mask, both act on whatever button they
are given — has nothing to say against any button and keeps its visual for all
of them. A finger and a pen tip report no button at all and are lowered to
`Primary`, so the gate is a mouse-and-barrel-button concern only.

The record itself is opened for **every** button, because it also holds the
focusable a direct pointer defers to its release (§7.2). A press on a button the
control cannot act on owns no visual and still chooses what it lands on.

The state moved out of the widgets because four of its rules are invisible from
inside a handler:

* **Slide-off.** WCAG 2.2 SC 2.5.2 asks that a press travelling off its target
  be abandonable. "Off its target" is the pointer's `TapBoundary` — a 5 dp
  radius for a mouse, the node's own bounds for a finger, because a finger's
  reported centre wanders several device pixels while resting inside the
  control it is pressing. It is the same predicate that fails the tap, so the
  activation and the visual can never disagree.
* **Re-entry.** Sliding back on restores the visual. The abort gesture is
  reversible right up to the release.
* **A peer claim.** When a pan claimant or an ancestor drag wins the
  arbitration, the pressed control loses the press and is never sent a release
  to clear itself from. Only the arbitration knows.
* **The feedback delay.** Inside a pan claimant the visual is withheld for
  `GestureProfile::press_feedback_delay` (100 ms — Flutter `kPressTimeout` /
  Android `ViewConfiguration.getTapTimeout()`), so a finger resting on a list
  row does not flash the row before the pan has been ruled out. **Only inside a
  claimant**: a control nothing can scroll out from under has no ambiguity to
  wait out and lights up at once. A mouse never waits at all — an indirect
  pointer opens no pan session, so the delay cannot reach it.

A cancel clears the visual as part of the ordered teardown. A second contact
never takes over a node's visual: under `MultiContact::First` it is terminated
before it reaches the arena, and under any other policy the first contact keeps
the boolean — otherwise the first release would clear a visual the second is
still holding.

`teksilo-widgets`' `common/interaction.rs` is the consumer side: `bind_pressed`
mirrors the framework press onto a control's own signal (keeping its identity,
so a Tier-3 style that captured it goes on working), and `press_shows_now` /
`press_survives` are for a control that keeps its own state machine and needs to
consult the framework from inside handlers it already has.

### 7.2 Focus lands on the release, for a direct pointer

A mouse focuses on press, exactly as it always has. A finger and a pen do not:
their focus is assigned on the `PointerUp`, guarded by **"the release landed on
the same focusable as the press"**. A finger that presses one control, slides
onto its neighbour and lifts has activated nothing, and must move focus nowhere
— the rule the tap recognizer applies to activation, applied to focus so the
two cannot disagree.

`FocusOrigin` grew a device to say which of these happened:

```rust
#[non_exhaustive]
pub enum FocusOrigin {
    Keyboard,
    Pointer(PointerKind),
    Programmatic,
    Accessibility,
}
```

`is_pointer()` replaces the `== FocusOrigin::Pointer` comparisons; `pointer_kind()`
answers with the device. A control deriving its *own* origin from hover or from
the input-modality signal — it knows only "not the keyboard" — writes
`FocusOrigin::POINTER`, which is `Pointer(PointerKind::Unknown)`, rather than
naming a device it never saw.

`focus_visible` is one tree-level signal, not a per-node flag, so the assignment
itself declares the modality: `Keyboard` and `Accessibility` reveal the ring,
`Pointer(_)` hides it, and `Programmatic` declares nothing at all — a scripted
focus leaves the ring where the user's last real interaction left it, which is
what `:focus-visible` does for `element.focus()`. The keystroke *also* still
sets the signal at the dispatch root, because a keystroke that moves no focus
must still reveal the ring.

`Accessibility` is new, and fixes a real bug: an assistive `Action::Focus` used
to route through `Programmatic`, so a screen-reader user who had clicked
anything got an invisible focus for the rest of the session.

### 7.3 The press-time actuation census

What still happens on `PointerDown`, and why. Every entry was read in the source
rather than inferred. The citations are **file plus the call named in the middle
column** — that call is what to grep for; line numbers were removed because they
rot within a couple of packages while the verdicts stay true.

#### Kept on press, with the reason

| site | what it does on press | why it stays |
| --- | --- | --- |
| [`title_bar/resize_strip.rs`](../crates/teksilo-widgets/src/title_bar/resize_strip.rs) | `host.begin_resize(edge)` | The OS owns the gesture from the press onward. `begin_resize` hands the pointer to the compositor's own interactive-resize loop, which never delivers the release back to us — there is no release to move the actuation to. |
| [`splitter/handle.rs`](../crates/teksilo-widgets/src/splitter/handle.rs) | captures the pointer, records the drag origin and the pane pair | A continuous manipulator: the value it produces **is** the press position, and everything after the press is measured from it. Deferring to the release would mean the divider only ever jumped, never dragged. |
| [`docking/resize_handle.rs`](../crates/teksilo-widgets/src/docking/resize_handle.rs) | captures, records the drag offset | The Splitter handle's twin, and the same reason. |
| [`table_view/header.rs`](../crates/teksilo-widgets/src/table_view/header.rs) | latches a column resize / reorder grip | Same family: the grip's whole output is the delta from the press point. |
| [`spin_box/step_button.rs`](../crates/teksilo-widgets/src/spin_box/step_button.rs) | steps once, then arms hold-to-repeat | Qt's `QAbstractSpinBox` convention, and the auto-repeat needs a press to start counting from. A step is cheap and reversible; a release-only step would make the repeat impossible to express. |
| [`primitives/text_input_field/mouse.rs`](../crates/teksilo-widgets/src/primitives/text_input_field/mouse.rs), [`rich_text/mouse.rs`](../crates/teksilo-widgets/src/rich_text/mouse.rs), [`code_editor/mouse.rs`](../crates/teksilo-widgets/src/code_editor/mouse.rs) | places the caret and latches a selection anchor | The **mouse-only** selection latches, and they are gated on a precise pointer. A drag-select is a continuous manipulator whose anchor is the press point, and every desktop text surface behaves this way. A direct pointer takes a different path entirely — the press commits nothing, the caret lands on a release that still belongs to it, and a hold selects the word and raises the handles. See [Touch text editing](text-touch-editing.md). |
| [`password_field.rs`](../crates/teksilo-widgets/src/password_field.rs) | starts a hold-to-reveal | The gesture *is* "while held". There is nothing to defer. |
| [`grid_view/body_pane.rs`](../crates/teksilo-widgets/src/grid_view/body_pane.rs), [`list_view/body_pane.rs`](../crates/teksilo-widgets/src/list_view/body_pane.rs), [`tree_view/body_pane.rs`](../crates/teksilo-widgets/src/tree_view/body_pane.rs), [`table_view/body_pane.rs`](../crates/teksilo-widgets/src/table_view/body_pane.rs), [`tree_table_view/body_pane.rs`](../crates/teksilo-widgets/src/tree_table_view/body_pane.rs) | Ctrl- and Shift-modified row selection, **for a precise pointer only** | The mouse's accelerator-click extends a selection whose anchor is the press, and a Shift-drag range needs the press to anchor from. Its one deferral is the unmodified press on an already-selected row, so grabbing a multi-selection drags the whole set. A **direct** pointer defers all three decisions to the release, because a finger has no Ctrl, no Shift, and its press is the opening sample of a possible scroll — see the conversion table below. |
| [`grid_view.rs`](../crates/teksilo-widgets/src/grid_view.rs) | records the modifiers the marquee will use | Not an actuation. It reads the press so the drag that may follow knows whether it is additive; the marquee itself starts from `DragPhase::Started`. |
| [`button.rs`](../crates/teksilo-widgets/src/button.rs) | sets `InteractionState::Pressed` | Not an actuation — a press *visual*, which is what §7.1 governs, and it is no longer the widget's own bookkeeping: `bind_press_interaction` mirrors the framework's `ctx.pressed_signal()` onto the button's `InteractionState`, so the router decides when the visual is up (including withholding it for the feedback delay inside a pan claimant). The remaining `set(Pressed)` calls in that file are the keyboard path, where Space and Enter have no pointer to ask about. |

#### Converted, and what each became

Nothing on this list is still owed. Each row records what the conversion turned
out to be, because in two cases it was not what was planned.

| site | what it did on press | what it does now |
| --- | --- | --- |
| [`widget_tree/pointer_router.rs`](../crates/teksilo-core/src/widget_tree/pointer_router.rs) — `handle_click_outside` | dismissed every click-outside overlay | **Converted.** `handle_click_outside` no longer exists: `arm_outside_press_dismissal` *arms* on the Down and `commit_outside_press_dismissal` commits on the Up, per pointer. For a direct pointer the arming Down is also **suppressed beneath**, so nothing under the overlay activates, and a press that slides off or is cancelled aborts the arm having delivered nothing. A mouse is unchanged. |
| [`widget_tree/pointer_router.rs`](../crates/teksilo-core/src/widget_tree/pointer_router.rs) — the `PointerButton::Secondary` arm | opens the context menu | **Closed by P29**, though not as written: the Secondary arm is left exactly as it is (a mouse still has that button) and the long press is a **fourth, additive** route into the same `show_context_menu_for`. It is not a long-press *recognizer* either — see [`touch_route`](../crates/teksilo-core/src/widget_tree/touch_route.rs) for why a recognizer cannot reach a disabled node. |
| [`title_bar/drag_region.rs`](../crates/teksilo-widgets/src/title_bar/drag_region.rs) | `host.show_window_menu(position)` on a Secondary press | **Already closed** before P29 looked at it: the node carries an `on_long_press` that calls `show_window_menu`, and its module header documents it. A widget's own `on_long_press` takes precedence over the tree route, so the two do not collide. (The window *move* on this node is not a press-time actor at all — it starts from `DragPhase::Started`.) |
| `data_views::deferred_select::on_down` — the `command()` and `shift()` arms, at the five body panes listed above | modified row selection | **Converted, and further than planned.** For a **direct** pointer `on_down` now applies *nothing at all*: it decides which of `Collapse` / `Toggle` / `Extend` the modifiers chose, parks it, and `on_up` applies it only if `release_completes_the_press` — so a pan commits nothing a release on that row would have committed. The mouse keeps its press-time behaviour, justified above, except for the pre-existing multi-selection collapse, which was already deferred. |

#### Already release-driven, and worth recording

Three widgets the design expected to find on the press turned out to be on the
release or the drag already, and need no conversion:

* the **ScrollBar** — the thumb latches from `DragPhase::Started`
  ([`scroll_bar.rs`](../crates/teksilo-widgets/src/scroll_bar.rs)) and a
  track click is an `on_tap` (`scroll_bar.rs`);
* the **Slider** — likewise, `DragPhase::Started` at
  [`slider.rs`](../crates/teksilo-widgets/src/slider.rs) and `on_tap` at
  `slider.rs`;
* **menu triggers, submenu opening and tab activation** — all `on_tap` or
  `on_hover` ([`menu_bar/trigger.rs`](../crates/teksilo-widgets/src/menu_bar/trigger.rs),
  [`menu_item/widget_impl.rs`](../crates/teksilo-widgets/src/menu_item/widget_impl.rs),
  [`tab_widget/header.rs`](../crates/teksilo-widgets/src/tab_widget/header.rs)),
  so their remaining touch problem is the hover-only half, not the press-time
  half.


---

## 8. Performance budget

A frame is 16.6 ms. Ten fingers on a digitizer reporting at 120 Hz is ten
samples per frame before any coalescing, and dispatch is the part of the frame
that happens *before* layout, paint and present get their turn. So the budget is
stated per sample rather than per frame, and it is stated at the contact cap,
because ten is the worst case the pointer table will admit.

Two numbers, and they are not the same kind of thing.

| | value | what it is |
| --- | --- | --- |
| **Hard budget** | ≤ 25 µs per pointer sample at ten contacts | a build gate — CI fails |
| **Advisory** | ≤ 5 % against a mouse baseline | a number to read — nothing fails |

25 µs leaves a hundred samples of headroom inside one frame. The 5 % is not a
gate because it cannot be one: at microsecond scale a shared CI runner swings
further than that between two runs of *unchanged* code — the mouse baseline in
this file's own two consecutive local runs moved 20 % — so a job enforcing it
would fail on the weather, and a red build nobody believes is worse than no
build at all.

### 8.1 The separation is structural, not a convention

The gate is the bench binary's **exit status**. The advisories are **stdout**.

```console
$ cargo bench -p teksilo-core --bench pointer_dispatch -- --gate-only
gate      pointer_move touch x10 depth12: 9.44 µs/sample (budget 25.00 µs) -> PASS
advisory  pointer_move touch x1 depth12: 9.42 µs/sample vs 9.77 µs baseline (…) -> -3.5 % (informational, margin 5 %)
advisory  hit_test touch miss, 400 candidates: 9.80 µs/sample vs 3.68 µs baseline (…) -> +166.4 % (informational, margin 5 %)
$ echo $?
0
```

That last advisory is 166 % over its baseline and the build is green, which is
the point: `Report::verdict` in `crates/teksilo-core/benches/budget.rs` takes
`&self` and reads only `Report::gate`. `Report::advisories` is a separate field
and there is no path from it to the exit status —
`advisories_never_move_the_verdict` and
`a_healthy_advisory_cannot_rescue_a_breached_gate` in `benches/budget_gate.rs`
pin both directions.

`TEKSILO_DISPATCH_BUDGET_NS` tightens the budget for a local run and is clamped
to the published 25 µs, so it can rehearse a breach but never excuse one.

### 8.2 What is measured

`crates/teksilo-core/benches/pointer_dispatch.rs`, under criterion.

| bench | shape | what it is for |
| --- | --- | --- |
| `pointer_move/mouse_depth12` | one mouse, twelve-deep tree | the baseline every advisory is read against |
| `pointer_move/touch_1_contact_depth12` | one finger, same tree | what one contact costs |
| `pointer_move/touch_10_contacts_depth12` | ten fingers, same tree | **the gate's subject** |
| `hit_test/slop_miss_touch_400_candidates` | a probe in a gutter, 400 eligible targets | the miss-only slop pass, in its expensive case — a *hit* short-circuits, a miss walks and sorts the whole subtree |
| `hit_test/slop_miss_mouse_400_candidates` | the same probe, precise pointer | the "a mouse pays nothing new" claim, measured rather than asserted |
| `touch_action/tap_declared_path/{2,20}` | a tap where every node declares | the two path folds, `effective_touch_action` and `pan_candidates` |
| `fling/tick` | one coast tick, chained scroll and all | the per-frame cost of momentum |
| `workspace/{mouse_move,touch_10_contacts_move}` | an IDE-shaped tree | the composite, under live pan claimants |

Every contact in a multi-contact fixture is checked to have actually been
admitted before anything is timed: a sample the table refuses returns before any
work happens, so one contact over the cap would have the gate dividing by more
work than it did.

### 8.3 Running it

```console
$ cargo bench -p teksilo-core --bench pointer_dispatch                  # measure, then gate  (~100 s)
$ cargo bench -p teksilo-core --bench pointer_dispatch -- --gate-only   # gate alone          (~1 s)
$ cargo test  -p teksilo-core --benches                                 # the gate's own tests + a smoke run
$ cargo bench -p teksilo-core --bench pointer_dispatch -- --save-baseline p01
$ cargo bench -p teksilo-core --bench pointer_dispatch -- --baseline p01
```

The last two are criterion's own baseline machinery, and they are how a mouse
path is compared against a recorded SHA. A stored baseline is a directory of
measurements taken on one machine; it belongs in that machine's `target/`, never
in the repository, because a baseline recorded on one host says nothing on
another.

The gate does its own timing rather than parsing criterion's estimates.
criterion is the instrument you reach for when a number moved and you want to
know why — distributions, outliers, baselines. The gate answers one question in
about a second, from a median of block-timed rounds, and stays correct wherever
`CRITERION_HOME` happens to point.

### 8.4 What the numbers look like

One workstation, one run, September 2026. Absolute values are a property of the
machine; the *ratios* are the part worth reading.

| bench | per sample |
| --- | --- |
| `pointer_move` mouse, depth 12 | 11.8 µs |
| `pointer_move` touch ×1, depth 12 | 9.4 µs |
| `pointer_move` touch ×10, depth 12 | 9.7 µs |
| `hit_test` slop miss, touch, 400 candidates | 10.1 µs |
| `hit_test` slop miss, mouse, 400 candidates | 4.2 µs |
| `touch_action` tap, declared path of 2 | 4.2 µs |
| `touch_action` tap, declared path of 20 | 21.3 µs |
| `fling` tick | 0.6 µs |
| `workspace` mouse move | 5.7 µs |
| `workspace` touch ×10 move | 4.5 µs |

Three things fall out of that table.

**A tenth contact costs no more than the first.** Per-sample cost is flat from
one contact to ten, which is what the per-pointer entry model was for: nothing
in the dispatch path iterates the other live pointers.

**Depth, not node count, is what dispatch costs.** The workspace is roughly
six hundred nodes and is *cheaper per sample* than the forty-node twelve-deep
fixture, because its root-to-target path is half as long. The declared-path pair
says the same thing louder: 2 deep is 4.2 µs, 20 deep is 21.3 µs.

**The slop pass is the one genuinely expensive mechanism**, and only on a miss:
6 µs over 400 candidates, against a precise pointer that skips it entirely. It
is bounded by the subtree it searches, so the number to watch is not this one
but what happens when someone runs it over a list of ten thousand rows.

Two operational cautions. First, the headroom here is 2.5×; a CI runner more
than 2.5× slower than this workstation will fail the gate for being slow rather
than for a regression, so the gate wants a machine class it can hold to.
Second, criterion reports `pointer_move/touch_10_contacts_depth12` **per
iteration** — ten samples — so its number is ten times the gate's; the `thrpt`
line beside it is the per-sample figure.

### 8.5 Two things the benchmark could not do as written

A21 asks the composite to be "a docking workspace with a virtualized
`TableView`". It cannot be. `DockingLayout` and `TableView` live in
`teksilo-widgets`, which depends on `teksilo-core`; a dev-dependency the other
way would invert the crate graph for the sake of a benchmark. The shape is
reproduced from core's own parts instead — a rail, two docked panels, forty
realized table rows by eight columns, each panel a live pan claimant — because
forty rows is what a virtualized body actually mounts and the work the
dispatcher does over them is the same work either way.

A21 also asks for "the touch-action fold on a 20-deep path" as a thing of its
own. `WidgetTree::effective_touch_action` and `WidgetTree::pan_candidates` are
both `pub(crate)`, so a bench — which compiles as a separate crate — can only
reach them through the press that runs them. The bench therefore measures a tap
at two depths and reads the difference, which includes the preview and bubble
walks over the same lengthening path. That is a fair measure of what a deep path
costs and an imperfect one of what the folds alone cost.

---

## 9. What is reviewed rather than tested

The migration is complete: the scrollable, control, menu, data-view, text and
chrome sweeps have all landed, and touch text editing has a host in every editing
surface. What is left on this page is not a list of missing features but a list of
**claims a headless Linux CI host cannot check**, which is what a hardware
sign-off is for. The two things a running application still does not do right are
in the Status note at the top of this page, and §10 is the standing ledger of
everything else that needs an owner's decision.

The app event loop **is** wired: a `WindowEvent::Touch` handed to
`TeksiloAppHandler::window_event` reaches the widget tree as a touch sample.
What that costs to *state* is worth stating too. Every decision in the input
half of a loop turn lives in `teksilo-app`'s `input_loop` module, where a
headless test drives it; the winit callbacks that *call* those decisions need an
`&ActiveEventLoop`, which nothing in the workspace can construct — so they are
witnessed instead by `teksilo-app`'s `app::winit_loop_tests`, which builds a
real event loop off the main thread, pumps it with `run_app_on_demand`, and
hands the real handler hand-built events from inside a callback. That test is
`#[ignore]`d (it needs a display server and a wgpu adapter), it is Linux-only
(it uses winit's X11 extension traits), and CI runs it under Xvfb with openbox,
beside the X11 protocol tests.

These call sites stay **reviewed rather than tested**. Most carry a platform
answer that is a constant no Linux host can vary, so a test run there cannot
tell a right answer from a missing one:

- `create_window`'s first safe-area read — zero on everything but a macOS
  camera-housing window.
- the `WindowOps::soft_keyboard_support` override — `None` on every desktop
  but Windows, which is also the trait's default, so deleting the override
  changes no value this host can observe.
- the on-screen-keyboard poll, and its apply — `Explicit` is a Windows-only row.
- the pen pump's per-turn call — X11 has no pen path, so no shim is ever
  installed on the host CI runs on.
- the close path's contact release — what it frees is a process-global identity
  allocator whose state has no observable side once the window and its tree are
  gone.
- the Wayland tablet seat publishing tool presence (`sync_tool_presence` on the
  `ToolAdded` / `ToolRemoved` arms) — the poll-rate rule it feeds is covered,
  but the arms that call it need a compositor with a tablet manager, so
  deleting the call leaves the suite green while a real stylus drops to the
  idle rate.
- the `proximity_in` arm of `translate_tool_event` — it carries two live
  protocol objects (`zwp_tablet_v2`, `wl_surface`) and a `Proxy` cannot exist
  without a connection, so the line reading `surface.id().protocol_id()` has no
  off-device test short of a fake Wayland server. Measured, not assumed:
  deleting the arm leaves the suite green. It is the **only** arm of either the
  decode or the fold for which that is still true — every other arm of
  `translate_tool_event`, `ToolState::apply` and `ToolState::commit` was mutated
  one at a time and each reddens a named test. The mitigations are that the
  field is not droppable (`ToolEvent::ProximityIn` has no default for `surface`,
  so a decode that stopped reading it would not compile) and that reading the
  *wrong* surface puts every stroke in the wrong window, which is loud rather
  than silent; the routing that consumes the id is covered by
  `a_tool_over_a_sibling_window_is_not_ours` and
  `a_new_proximity_session_starts_clean`.

External drag-and-drop adds its own, all in the same class — the answer is a
protocol behaviour no headless host can produce:

- **the Wayland touch-down serial** (`external_dnd/wayland.rs`, `DndState::begin_outbound`).
  The rule that a finger's drag is started with a `wl_touch::down` serial rather
  than a `wl_pointer::button` one is a pure function and is tested; whether a
  compositor then accepts the request is only visible against a real one. The
  failure mode to look for is the *silent* one: no drag starts and no terminal
  event arrives.
- **the revised Wayland accept** (`DndState::revise_accept`) and **the revised
  `XdndStatus`** (`DndThread::revise_inbound_accept`). That the widget's verdict
  reaches the setter is tested at the seam; that the *cursor* then changes over a
  refusing target needs a real source application. GTK and Qt both track the
  latest status, which is the premise.
- **the inbound `Cancelled` post** from a backend whose own outbound drag ends
  over one of this app's windows. The platform-independent net for the same
  condition — a re-entered session whose process-wide stash has gone — *is*
  tested, so a missing post degrades to "cleared on the next layout pass" rather
  than to a stuck session.

And one that is a *pinned premise* rather than an untested line: X11's outbound
drag reads the core pointer's button mask, which answers for a finger only
because X11 promotes a pointer-emulating touch onto that pointer. A test beside
the backend asserts the capability row that says so, so a platform change fails
an assertion — but the promotion itself is what a touchscreen on X11 has to
confirm.

These are the lines a hardware sign-off has to look at by hand.

The cancel teardown is complete: the framework press signal clears there
(§7.1) and a fling this pointer was driving stops there, both at the point the
funnel marks.

### Telemetry: nothing is emitted

The pointer, gesture, density and kinetic subsystems emit **no telemetry**, and no
framework crate ships a telemetry manifest — the only `events.yaml` in the
workspace belongs to `examples/telemetry_codegen`, and this programme left its
schema untouched. The framework's one outbound event remains `intent.dispatched`,
whose `IntentSource` names how a command was reached and has no pointer-kind
variant: a tap from a finger and a click from a mouse both report `Handler`. See
[telemetry.md §2.8](telemetry.md).

## 10. Open findings

§9 is a different list from this one and the distinction is load-bearing. There,
every item is a claim **this host cannot check** — a platform constant, a protocol
behaviour — and the answer exists on some machine. Here, every item is a question
**nobody has answered**: an unread token, a missing route, a contract that is
wrong and worked around. No amount of hardware settles these; they need a ruling.

They are collected on this page because the Status note at the top already sends a
reader here for what the pointer model does not do, and because a finding recorded
only in a package report is a finding that evaporates. One of them — the trackpad
rotation sign — *is* answerable on hardware and appears in
[touch-verification.md §12](touch-verification.md) as an explicit check; it is
listed here too so that the ledger is complete and so that closing it there closes
it here.

Each entry carries the measurement that establishes it. Where a stated *reason*
was found to be wrong, the correction is part of the entry: a finding with the
wrong cause attached sends the next person to the wrong file.

### 10.1 Dead API — declared, never read

Each of these is a public or projected surface with no production reader. The
decision each needs is the same one: **wire it, or delete it.** Retuning one and
expecting a behaviour to change is the failure mode they share.

- **`InputTokens::pen_hit_slop`.** Programme-introduced, and the only reader of
  the identifier anywhere is a test. The pen's outset is served by
  `GestureProfile::PEN.hit_slop`, which carries the same value and is what
  `HitSlop::for_pointer` consults. The two are pinned together by
  `the_unread_pen_outset_agrees_with_the_pen_profile` so a reader who finds them
  disagreeing is not left guessing which the framework honours — but the field
  itself still has no effect.
- **`TouchSelection::report_ime_area`. Closed: deleted, and a real gap beside it
  fixed.** The method had no caller and is gone. The reasoning that found it
  harmless was right about presses and wrong about drags: every stack does report
  from its own *touch* path — the direct-pointer arm of `mouse.rs` in each of
  `primitives/text_input_field`, `rich_text` and `code_editor`, plus a second
  reporter in each `place_caret_at` — but **dragging the caret handle went through
  none of them**. `TouchSelection::update_drag` moves the caret with
  `source.set_selection(moving..moving)`, which nothing downstream sees, so a
  finger that dragged the caret to a new position and then typed Japanese, Chinese
  or Korean got its candidate window at the old one. Each delegate now reports
  from its own stack's reporter — `TouchTextSurface::report_ime_area` for the two
  multi-line editors, `FieldTouch::report_ime_area` for the single-line family —
  gated on the core predicate `text_touch::drag_moves_the_caret`, which replaces
  the deleted method: the controller answers *when* a caret moved, and the host
  answers *what* to report, because only the host's reporter holds that stack's
  focus / read-only / layout guard and the dedup that stops an input method
  feeding an unchanged rectangle back as a fresh empty preedit. The terminal is
  exempt and doubly so: the crate owns no IME machinery, and a terminal answers
  `is_editable() == false`, so it is never offered a caret handle to drag.
  (`docs/soft-keyboard.md` asserted something different again — that the stacks
  report from their *keyboard* paths and that a touch-placed caret leaves a stale
  rectangle standing — and has been corrected.)
- **`WidgetTree::touch_pinch_active`.** No production reader; every call is a test.
  The pinch *state* behind it is live — the arbiter drives contact tracking and
  emission — so what is dead is the query. Worth being precise about the
  consequence: it is **not** established by this measurement that nothing
  suppresses the per-contact pan sessions during a two-finger pinch. Suppression
  could come from the claim chain rather than from this flag, and an earlier note
  that read the unread accessor as proof of an arbitration gap overstated it.
- **Projected recipe fields with no reader.** Counted twice before and wrong both
  times — four, then six — because both counts were spot checks rather than a
  sweep. The sweep is now in
  [density-projection-gaps.md](density-projection-gaps.md), which enumerates every
  one, cites its raw-const use site by file and function, says what it would
  become at Comfortable and Touch, and is held to the set by a guard test so a
  newly added projected field with no reader is caught rather than joining the
  pile. Four of them were configured by a shipped preset and are now wired —
  `TableRecipe::cell_padding_horizontal`/`_vertical`,
  `CalendarRecipe::nav_arrow_size` and
  `SearchFieldRecipe::row_padding_horizontal`/`_vertical` — through defaulted
  metrics accessors on `TableStyle`, `CalendarStyle` and `SearchFieldStyle`.
  `MenuItemRecipe::item_height`, which the earlier list included, *does* have a
  reader (`RecipeMenuItemStyle::metrics` → the menu list's own metrics) and never
  belonged on it.
- **`KineticScroller::pointer_velocity`.** No production caller. A fling is seeded
  in core from the pan recognizer's own window-space tracker, not from here. It is
  a public accessor whose only correct coordinate frame is the one the
  `window_position` naming rule preserves; harmless today by construction rather
  than by design.
- **`SceneItemHandlerSet::on_double_tap`.** A public builder with no dispatcher
  anywhere in `teksilo-scene`.
- **A `MultiContact::All` on a reporting surface.** Measured twice, in two
  unrelated places: the debug inspector's pointer-watch overlay and the touch
  playground's pointer pad both had one, and deleting it changed nothing. A
  handler that answers `EventResponse::Ignored` never becomes the arena owner, so
  there is no live arena for the default `First` to refuse a second contact from.
  Both declarations are gone and the behaviour is pinned by a test instead. This
  is a note for widget authors rather than a defect: declaring `All` on a surface
  that consumes nothing is a claim, not a mechanism.

### 10.2 Missing routes and accessors

- **A plain `ScrollArea` has no keyboard scroll route at all.** `ScrollBar`'s
  arrow / `Home` / `End` / `Page` arms are installed on a node built
  `HandlerSet::new().focusable(false)` in `ScrollBar::build`, and that node also
  calls `set_hidden()` in its own `accessibility`, so it is unreachable by
  keyboard *and* invisible to assistive technology. `ScrollArea` installs no key
  handler of its own — the only `on_key` in the whole file is a test fixture.
  **Pre-existing, not programme-introduced, and not a touch matter**, but a
  WCAG 2.1.1 gap on any scroll region whose content is not itself focusable — a
  long read-only text panel, most obviously. Two things narrow it and neither
  closes it: `ScrollArea::accessibility` does advertise `ScrollUp` / `ScrollDown`
  / `ScrollLeft` / `ScrollRight` per overflowing axis and answers them in its
  `on_access_action`, so an AT client can scroll it; and `ListView`, `TreeView` and
  `TableView` each install their own `on_key`, so the gap does not reach them. The
  decision is an owner's: make the bar focusable, or give `ScrollArea` an
  `on_key`.
- **Nothing a widget can reach reports the tree's live pointers, or the hover
  owner.** `WidgetTree::hover_owner` and the pointer table behind it are the
  tree's. `EventContext` describes only the sample being dispatched;
  `LayoutContext` carries focus, shortcuts and overlays; `BuildContext` carries
  neither. The closest thing that exists is `EventContext::tree_pointer_position`,
  and it is not a substitute: it reports the table's *elected* primary, which
  prefers the mouse, so on a machine with both it answers for the wrong device.
  Measured by two tools that wanted it and could not have it — the debug
  inspector's Pointers tab, whose live half needs an armed full-window probe that
  takes the application's input while it is up, and the touch playground, whose
  inspector is therefore a *pad* the user touches on purpose. One accessor (or a
  pointer-table field on the layout extras) would make both passive.
- **`EventContext::set_input_density` does not exist.** An application can only
  switch density from a handler by doing both halves itself: re-project the theme
  through `ctx.set_theme`, then write a signal its own root binds at
  `BindingLevel::Rebuild`. That works only for an app that owns its root — the
  widget previewer and the touch playground both do it, and the widget catalog
  takes a `--density` flag instead precisely because a tab cannot rebuild the
  other twenty. `WidgetTree::set_input_density` does both halves in one call and
  nothing on `EventContext` reaches it; it should be parked like `set_theme` and
  drained into every window.
- **Scene item context menus by hold are not shipped.** Attaching `on_long_press`
  to a `SceneView` puts a long-press recognizer in the view's own arena, which
  cannot address a lightweight item because an item has no `WidgetId`. Needs a core
  hook. The hover-affordance census row stays open with it.
- **`CancelReason` has no variant for "an embedded native surface owns this
  pointer now".** The webview uses the documented catch-all `Deactivated`;
  `NativeSurfaceTakeover` would be the honest name, and `Platform` would
  misattribute the producer.
- **`density::grab_outset(visual, kind, tokens)` exists three times privately** —
  in `splitter/handle.rs`, `docking/resize_handle.rs` and
  `title_bar/resize_strip.rs`. It wants hoisting into core; a fourth copy was
  deleted rather than added.

### 10.3 Contracts that are wrong, and worked around

- **`TouchSelection::update_drag` hit-tests the raw contact position.** A
  selection handle's disc sits *above* the glyph row it marks, and the core takes
  no account of the rise — so every host is handed a sample a dozen dp off the
  text. Both shipped hosts compensate entirely host-side, capturing the grab
  offset when the handle drag begins, which is correct for them and leaves the
  **contract** wrong for the next one. A core fix is `update_drag` taking the caret
  the handle marks, or `HandleDrag` carrying the grab offset. Until then the
  adoption checklist is the only defence, which is why
  [touch-verification.md §10](touch-verification.md) F4 checks for exactly this
  offset by hand.
- **`take_handler_set` does not merge the inner widget's handler set.** It is the
  one `Widget` method deliberately not forwarded by `WidgetWithHandlers` — it
  exists so the arena can lift off the handlers the builder chain just attached,
  which live on the wrapper. But now that the branch types delegate the whole
  trait, `WidgetWithHandlers<TeksiBranch<WidgetWithHandlers<Button>, _>>` is
  reachable, which is what `teksu!` produces from a builder method applied to an
  `if`/`else` whose arms carry handlers — and the inner arm's handlers are
  silently dropped. Fixing it needs a per-field precedence decision between two
  handler sets: an owner's call.
- **`TextSurface::allows_copy` for a text-field handle returns the raw opt-in**
  rather than the resolved permission, so a *revealed* password field greys out
  Edit ▸ Copy while `Ctrl+C` works. Pre-existing.
- **`PointerInfo::primary`'s own field doc is false.** It says "Exactly one live
  pointer is primary; a mouse always wins the role" — which is verbatim the rule
  for `PointerTable::primary`, the table's *election*, sitting on the W3C
  *per-kind* flag. On a hybrid machine a mouse and a first touch are both primary,
  and the table never rewrites `info.primary`: its `elect` writes only its own
  field, and the only writers of the flag are the producers in the platform
  translator. The three notions are separated correctly in `pointer/table.rs`'s
  module docs and nowhere at the field. `PointerInfo::touch`'s constructor doc is
  false in a second way, claiming the table decides primacy "not the
  constructor" — the table decides *its* election; the producer decides the flag.
  Both corrected by this package; recorded here because the shape of the error —
  a rule written at the wrong one of three similarly-named things — is the one to
  watch for.

### 10.4 Known defects carried behind an `#[ignore]`

Two, both in the data views, both measured, both needing a ruling rather than a
patch. They are the only `#[ignore]`s the programme added.

- **A deferred drag is revoked if the first sample after the hold leaves the
  pressed row.** A coarse pointer's press boundary is the pressed node's own
  bounds, and ending the press takes the drag member with it. The touch drag slop
  is 18 dp and a default tree row is 28, so on short rows a real finger's first
  reported sample is as likely as not to be outside, and the reorder silently
  becomes a scroll. `a_finger_reorder_survives_its_first_sample_leaving_the_row`
  is `#[ignore]`d against it and carries the numbers. The grid's marquee is
  unaffected: its press is captured by the body pane, whose bounds are the whole
  viewport.
- **An application that wraps a data view in anything tappable takes the row's
  release.** The wrapper captures the press, the release is dispatched to the
  wrapper and bubbled wrapper→root, and the row never sees the release its press
  deferred. The obvious fix — the press absorber applied unconditionally at the
  four row sites — was applied and measured: it greens this and breaks
  `TableView`'s *cell* selection, because the row's new arena takes the press the
  cell needs on its own release. So it needs a ruling on which node owns a press
  when an application wraps a data view, plus a cell-level absorber.
  `a_finger_tap_on_a_list_row_under_a_tappable_ancestor_still_selects_it` is
  `#[ignore]`d against it.

### 10.5 Coverage boundaries

- **The target-size gate is green for three named fixture lists, not for the
  framework.** The stock widget catalog's list (in `teksilo-target-conformance`),
  `teksilo-charts`' and `teksilo-scene`'s; `teksilo-inspector`,
  `teksilo-terminal`, `teksilo-preview-ui` and `teksilo-webview` own targets and
  have none. The widget list sweeps all four shipped presets at three densities;
  charts and scene sweep Int UI alone, each for a written reason. Of the three
  Compact-visible under-floor exceptions
  [density-inventory.md](density-inventory.md) §0 enumerates, the gate reaches
  none. The boundary is written out crate by crate, with the feasibility of each
  missing list, in
  [accessibility-internal-audit.md](accessibility-internal-audit.md) §3.7 and in
  the shared walker's own module docs. **Do not read a green gate as "the framework
  conforms."**
- **The fixture lists measure every label through a flat line height.** Nothing in
  them installs a text backend, and the two headless measurers — `TextWidget`'s own
  8-dp-per-character fallback and `MockTextBackend` — both report one fixed line
  height whatever the theme's typography asks for. Every text-derived dimension in
  the census is therefore that constant rather than the theme's line box, which is
  how a menu row comes out under the floor in three presets and at its declared
  height in the fourth. The allow-list entry on the menu row says so at the point
  it matters; the general consequence is that a fixture measuring a control sized
  by its label is measuring the harness's metrics as much as the widget's.
  Unowned, and the remedy (a backend that reads the theme) would re-measure every
  pin in the list.
- **A window's top resize ring swallows the top band of its own content at Touch.**
  The 6 dp strip's coarse `hit_outset` reaches roughly a `target_size` into the
  window, so a control laid out against the top edge is unreachable by a finger.
  Present under every preset; the gate reports it under Material 3 alone, because
  that is the preset whose larger Touch target moves the control's *centre* inside
  the ring — under the others the audit files the same row as shadowed by another
  target rather than judging it. Same owner as A10's outset-versus-target
  precedence, and the same finding as the docking gutter already on the list.
- **A `SearchField` with an empty query reports a 16 × 16 clear slot reaching
  22 × 24 at Compact, under all four presets.** Measured, not described: the
  `search_field/empty` fixture produces exactly that row, and the figure above is
  read off the gate rather than written beside it. The same slot with a query
  typed reaches 24 × 24 and does not appear at all — `HitTarget::active`
  withdraws the *outset* when the affordance is hidden, and nothing withdraws the
  *target*. What is left is a live node that keeps its `on_tap` and its
  `CursorIcon::Pointer`: a press on an empty field's trailing 16 dp is swallowed
  by an affordance that is not painted and does nothing, shows a hand while it
  does it, and eats the caret placement the press was aiming at. Now an
  allow-list entry with an owner rather than a bullet here, because the question
  is answerable: stop being a target when inactive. It is not a one-line change —
  an outset must be declared by the node that *takes* the press, so the handler
  cannot simply move onto the `visible_when`-gated glyph inside, and `HitTarget`
  has no reactive way to drop a handler — which is why it is escalated rather
  than fixed in passing.
- **Fluent pins its icon button at 32 dp and its menu row at 33 / 33 / 40 across
  the whole ladder.** Both clear the 24 dp AA floor at every density, so neither is
  a gate failure; both are the same class as the macOS constants that *were* under
  it, and both would become failures the day the floor moved. Recorded, not gated.
- **`WidgetTree::set_input_density` destroys a subtree rooted in a layout
  primitive.** Measured twice, most recently on a `Padding → HStack → [TextWidget,
  Button]` root: six nodes to two. Every fixture list builds *at* a density and so
  never meets it, but `audit_at_density` is public and a caller can walk into it.
  Unowned.
- **`DensityPolicy::FollowLastPointer` and `Environment::prefers_touch` have an
  ingress and no writer.** Honouring the policy needs decisions nobody has taken:
  what a stray tap should cost, whether a pen is coarse, and when hysteresis
  commits. The three doc sites say what is true.
- **`ScrollableAxes::overscroll` publishes a value nothing renders.** No stretch,
  no glow. The value is correct and the paint does not exist.

### 10.6 Not ours, but in the way

- **`cargo check --workspace --all-features` cannot build on any revision,
  including `main`.** `teksilo-text` embeds seven feature-gated fallback font
  faces and only two of them — the Arabic and Hebrew ones, which are on by
  default — were ever committed. The Thai face, the Devanagari one and the three
  CJK ones are named by `include_bytes!` and absent from the tree on every branch,
  so five of those features have never been buildable by anyone who clones this
  repository. Verified against `main`, so not branch-introduced. **Deliberately not
  added to any gate.** It needs either the fonts committed or the features gated
  behind a build-time probe.
- **The trackpad rotation sign may be inverted, and only macOS hardware can say.**
  `Transform2D::rotate` maps a positive angle to a clockwise turn in y-down screen
  space, and the touchscreen arm is self-consistent with that — its `atan2` is in
  the same space. AppKit documents `NSEvent.rotation` as positive for
  *counter*-clockwise, and winit documents no sign convention at all for its
  rotation gesture, so the source tree cannot settle whether the platform seam
  should negate. It was deliberately not guessed at. Trackpad-only,
  magnitude-independent, and orthogonal to the degrees-versus-radians defect that
  *is* fixed. The check is [touch-verification.md §12](touch-verification.md) H4,
  and it closes this entry either way.
- **`rustfmt` joins backslash-continued string literals** when the join fits,
  dropping the continuation and leaving its indent as runs of spaces *inside* the
  literal — which silently mangles a multi-line assertion message. Not a pointer
  matter at all; recorded because it bit this programme twice and the damage looks
  like a bad assertion rather than a formatter artefact.

## See also

- [porting-widgets-to-the-pointer-model.md](porting-widgets-to-the-pointer-model.md)
  — the numbered contract, if you are porting or writing a widget.
- [Events & gestures](events-and-gestures.md) — dispatch, recognizers, the
  sequence and the ordered decision procedure.
- [Density & targets](density-and-targets.md) — the ladders, the hit mechanisms,
  the gesture-profile tables.
- [Kinetic scrolling](kinetic-scrolling.md), [Touch text
  editing](text-touch-editing.md), [Soft keyboard](soft-keyboard.md),
  [Data views under a finger](data-view-touch.md).
- [Explore by touch](a11y/explore-by-touch.md) and [single-pointer alternatives to
  dragging](a11y/non-drag-alternatives.md) — the accessibility halves.
- The inventories: [widget pointer inventory](widget-pointer-inventory.md),
  [density inventory](density-inventory.md), [hover-affordance
  census](hover-affordance-census.md), [drag-operation
  census](drag-operation-census.md).

[`PointerInfo`]: https://docs.rs/teksilo-core
[`PointerIdAllocator`]: https://docs.rs/teksilo-core
[`EventTime`]: https://docs.rs/teksilo-core
[`InputClock`]: https://docs.rs/teksilo-core
[`MonotonicClock`]: https://docs.rs/teksilo-core
[`ManualClock`]: https://docs.rs/teksilo-core
[`CancelReason`]: https://docs.rs/teksilo-core
