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

> **Status.** A mouse behaves exactly as it always has. The tree now routes
> touch and pen samples end to end — arbitration, pan delivery, cancellation,
> hit targeting and the press — but the **app event loop** is not yet wired to
> the multi-sample translator, so a running application still feeds the tree the
> single-`WidgetEvent` mouse path. Everything below the ingress doors is live
> and tested; the last hop from the platform layer into the loop is not.

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

| | pen at all | tool kind | pressure | tilt | twist | contact patch |
| --- | --- | --- | --- | --- | --- | --- |
| Wayland | `zwp_tablet_v2` | yes | yes | yes | yes | — |
| Windows | `WM_POINTER*` subclass | yes | yes | yes | yes | **yes** (touch) |
| X11 | — | no | no | no | no | no |
| macOS | — | no | no | no | no | no |

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
- `pen/wayland.rs` keeps all its logic in `ToolState`, which knows nothing about
  wayland-client and is driven by recorded `zwp_tablet_tool_v2` sequences. The
  `Dispatch` impls do one thing: translate a protocol event into a `ToolEvent`
  and hand it over.
- The translator's proximity machine is tested through a scripted `PenSource`,
  so hover, contact, buttons, tool change and `cancel_all` are all covered with
  no device at all.

What none of that covers is the OS boundary itself: that the subclass installs
alongside AccessKit's, that a real compositor's tablet seat behaves as the
protocol says, that a Wacom's `dwTime` and frame cadence are what the
documentation claims. Those belong on a hardware checklist, not in CI.

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
rather than inferred; the list of conversions still owed names the package that
owes each one.

#### Kept on press, with the reason

| site | what it does on press | why it stays |
| --- | --- | --- |
| [`title_bar/resize_strip.rs:116`](../crates/teksilo-widgets/src/title_bar/resize_strip.rs) | `host.begin_resize(edge)` | The OS owns the gesture from the press onward. `begin_resize` hands the pointer to the compositor's own interactive-resize loop, which never delivers the release back to us — there is no release to move the actuation to. |
| [`splitter/handle.rs:266`](../crates/teksilo-widgets/src/splitter/handle.rs) | captures the pointer, records the drag origin and the pane pair | A continuous manipulator: the value it produces **is** the press position, and everything after the press is measured from it. Deferring to the release would mean the divider only ever jumped, never dragged. |
| [`docking/resize_handle.rs:235`](../crates/teksilo-widgets/src/docking/resize_handle.rs) | captures, records the drag offset | The Splitter handle's twin, and the same reason. |
| [`table_view/header.rs:707`](../crates/teksilo-widgets/src/table_view/header.rs) | latches a column resize / reorder grip | Same family: the grip's whole output is the delta from the press point. |
| [`spin_box/step_button.rs:252`](../crates/teksilo-widgets/src/spin_box/step_button.rs) | steps once, then arms hold-to-repeat | Qt's `QAbstractSpinBox` convention, and the auto-repeat needs a press to start counting from. A step is cheap and reversible; a release-only step would make the repeat impossible to express. |
| [`primitives/text_input_field/mouse.rs:25`](../crates/teksilo-widgets/src/primitives/text_input_field/mouse.rs), [`rich_text/mouse.rs:251`](../crates/teksilo-widgets/src/rich_text/mouse.rs), [`code_editor/mouse.rs:46`](../crates/teksilo-widgets/src/code_editor/mouse.rs) | places the caret and latches a selection anchor | The mouse-only selection latches. A drag-select is a continuous manipulator whose anchor is the press point, and every desktop text surface behaves this way. Touch text selection is a different gesture set entirely and is owed by the touch-text package, which will not reuse this path. |
| [`password_field.rs:527`](../crates/teksilo-widgets/src/password_field.rs) | starts a hold-to-reveal | The gesture *is* "while held". There is nothing to defer. |
| [`grid_view/body_pane.rs:335`](../crates/teksilo-widgets/src/grid_view/body_pane.rs), [`list_view/body_pane.rs:309`](../crates/teksilo-widgets/src/list_view/body_pane.rs), [`tree_view/body_pane.rs:298`](../crates/teksilo-widgets/src/tree_view/body_pane.rs), [`table_view/body_pane.rs:392`](../crates/teksilo-widgets/src/table_view/body_pane.rs), [`tree_table_view/body_pane.rs:601`](../crates/teksilo-widgets/src/tree_table_view/body_pane.rs) | Ctrl- and Shift-modified row selection | The **mouse-only** selection latches: an accelerator-click extends a selection whose anchor is the press, and a Shift-drag range needs the press to anchor from. The unmodified press on an already-selected row is *already* deferred to the release (`data_views::deferred_select`) so grabbing a multi-selection drags the whole set; the remaining press-time paths are the modified ones. |
| [`grid_view.rs:1274`](../crates/teksilo-widgets/src/grid_view.rs) | records the modifiers the marquee will use | Not an actuation. It reads the press so the drag that may follow knows whether it is additive; the marquee itself starts from `DragPhase::Started`. |
| [`button.rs:128`](../crates/teksilo-widgets/src/button.rs) | sets `InteractionState::Pressed` | Not an actuation either — a press *visual*, which is what §7.1 exists to govern. The button family's own bookkeeping is left in place for now; converting it is the controls sweep's job. |

#### Owed to a later package

| site | what it does on press | owed by |
| --- | --- | --- |
| [`widget_tree/pointer_router.rs:529`](../crates/teksilo-core/src/widget_tree/pointer_router.rs) — `handle_click_outside` | dismisses every click-outside overlay | **P17.** A finger that presses outside a popover and slides back in has not dismissed it; the dismissal belongs on the release, with the same "landed where it started" guard focus now uses. |
| [`widget_tree/pointer_router.rs:823`](../crates/teksilo-core/src/widget_tree/pointer_router.rs) — the `PointerButton::Secondary` arm | opens the context menu | **P29.** For a coarse pointer there is no secondary button to press: the gesture is a long press, and the menu should open from the long-press recognizer instead of from a button a finger does not have. |
| [`title_bar/drag_region.rs:155`](../crates/teksilo-widgets/src/title_bar/drag_region.rs) | `host.show_window_menu(position)` on a Secondary press | **P29.** The same conversion, for the OS window menu. (The window *move* on this node is not a press-time actor at all — it starts from `DragPhase::Started`, `drag_region.rs:131`.) |
| `data_views::deferred_select::on_down` — the `command()` and `shift()` arms, at the five body panes listed above | modified row selection | **P23.** Listed in both tables on purpose: the *mouse* behaviour is kept and justified above; what P23 owes is the touch gesture set that replaces it, since a finger has no Ctrl and no Shift. |

#### Already release-driven, and worth recording

Three widgets the design expected to find on the press turned out to be on the
release or the drag already, and need no conversion:

* the **ScrollBar** — the thumb latches from `DragPhase::Started`
  ([`scroll_bar.rs:502`](../crates/teksilo-widgets/src/scroll_bar.rs)) and a
  track click is an `on_tap` (`scroll_bar.rs:546`);
* the **Slider** — likewise, `DragPhase::Started` at
  [`slider.rs:358`](../crates/teksilo-widgets/src/slider.rs) and `on_tap` at
  `slider.rs:380`;
* **menu triggers, submenu opening and tab activation** — all `on_tap` or
  `on_hover` ([`menu_bar/trigger.rs:74`](../crates/teksilo-widgets/src/menu_bar/trigger.rs),
  [`menu_item/widget_impl.rs:577`](../crates/teksilo-widgets/src/menu_item/widget_impl.rs),
  [`tab_widget/header.rs:732`](../crates/teksilo-widgets/src/tab_widget/header.rs)),
  so their remaining touch problem is the hover-only half, not the press-time
  half.


---

## 8. What is not here yet

Deliberately, and in this order: wiring the app event loop to the multi-sample
translator (nothing dispatches a touch or pen sample yet — the platform layer
only *produces* them), the kinetic scrolling core, the density sweep across the
widget catalogue, and touch text editing. Each has its own package; this file
grows with them.

The cancel teardown is complete: the framework press signal clears there
(§7.1) and a fling this pointer was driving stops there, both at the point the
funnel marks.

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
