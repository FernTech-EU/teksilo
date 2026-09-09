<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Kinetic Scrolling

Velocity tracking, fling physics and the rubber band, implemented once in
[`teksilo-core::kinetic`](../crates/teksilo-core/src/kinetic.rs).

Every pixel-offset scrollable in this workspace hand-rolled the same boundary
clamp followed by the same 150 ms ease-out tween. None of them tracked velocity, so
none could fling, and none had a rubber band. Adding physics to each would mean
one integrator, one tolerance constant and one place to forget
`prefers-reduced-motion` **per surface** — a failure mode this codebase already
demonstrates, with `EDGE = 32` and `MAX_VELOCITY = 12` copied across five
widgets that have since drifted apart. So the physics lives in one module, as
pure computation, and the surfaces drive it — through
[`ScrollableBehavior`](../crates/teksilo-widgets/src/common/scrollable.rs), which
is the one `on_scroll` body they now share. §10 is the adoption recipe.

Nine surfaces install it — `ScrollArea`, the five data views (`ListView`,
`TreeView`, `GridView`, `TableView`, `TreeTableView`) and the three text
surfaces (`RichTextEditor`, `CodeEditor`, `LogView`) — and `SceneView` takes
the claim from it while keeping its own handler, for the reason in §10.1. No
number here counts the *pre*-migration hand-rollers: the inventory that
produced the original count included `MenuList` and the `TabBar` strip, and
§9 records what the migration found about those two — neither owns an
`on_scroll` at all.

Nothing here owns a widget, reads a clock, or touches the arena. Time arrives as
an [`EventTime`](../crates/teksilo-core/src/pointer.rs) from the tree's single
`InputClock`, which is what lets a whole fling be replayed deterministically in a
headless test.

---

## 1. The two physics, and when each applies

Two families ship. They are genuinely different conventions, not tunings of one
curve, and the choice is made once per scrollable by its
[`OverscrollStyle`](../crates/teksilo-tokens/src/input.rs):

| | `ClampingSimulation` | `BouncingSimulation` |
| --- | --- | --- |
| Upstream | Android `OverScroller` | Flutter `BouncingScrollSimulation` (iOS `UIScrollView`) |
| Selected by | `OverscrollStyle::Clamp` | `OverscrollStyle::RubberBand` |
| In-range motion | friction spline | exponential decay |
| At the boundary | stops dead | overshoots, spring pulls it back |
| Feels like | Windows, GTK, Android | macOS, iOS |

`ScrollPhysics::Platform` — the default — resolves to `Bouncing` on macOS and
iOS and to `Clamping` everywhere else, through
[`resolve_platform_physics`](../crates/teksilo-core/src/kinetic/scroller.rs). An
app that wants one feel on every OS states `Clamping` or `Bouncing` outright.

**The style picks both the drag behaviour and the fling simulation, because they
are one decision.** A surface that clamped under the finger but bounced on
release would read as two different scrollables sharing a rectangle.

### The third piece: the rubber band

[`rubber_band`](../crates/teksilo-core/src/kinetic/simulation.rs) belongs to
neither simulation. It is the *drag-time* companion to the bouncing feel — the
falling-gain curve that makes content resist a finger past the edge. The fling
never calls it: past the edge, the spring **is** the return.

Flutter states the rule as a gain, `frictionFactor(f) = 0.52·(1 − f)²`, where `f`
is how far past the edge the content already is as a fraction of the viewport.
iOS is documented with a closed form, `(1 − 1/(x·c/d + 1))·d`. These are the same
curve: integrating Flutter's gain over the finger's travel yields iOS's closed
form exactly, with `c = 0.52` in place of Apple's 0.55. The derivation is written
out at the function, and a unit test measures the gain at five points along the
curve and checks it against `0.52·(1 − f)²` — so the claim that the two upstream
statements agree is asserted, not just asserted-in-prose.

`rubber_band_inverse` recovers the finger travel from a damped displacement. A
drag that *resumes* from an overscrolled position (a fling left the content past
the edge, then a finger grabbed it) has to continue from the right point on the
curve rather than restart at its origin.

---

## 2. The constants

Every value is traceable to upstream and is cited at its definition together
with the arithmetic that produces it. Scroll feel is muscle memory; a curve that
is merely plausible reads as broken, and "plausible" cannot be reviewed.

### Velocity tracker

Declared in
[`kinetic/velocity.rs`](../crates/teksilo-core/src/kinetic/velocity.rs).

| Constant | Value | Source |
| --- | --- | --- |
| `HISTORY_SIZE` | 20 samples | Flutter `VelocityTracker._historySize` |
| `HORIZON` | 100 ms | Flutter `_horizonMilliseconds` |
| `MIN_SAMPLE_SIZE` | 3 samples | Flutter `_minSampleSize` |
| `STOP_GAP` | 40 ms | Flutter `_assumePointerMoveStoppedMilliseconds` |
| fit degree | 2 | Flutter `LeastSquaresSolver(…).solve(2)` |

Flutter took all five from Android's `LeastSquaresVelocityTrackerStrategy`.

They are load-bearing, not decorative. The 100 ms horizon is what makes a long
slow drag ending in a flick report the flick rather than the drag. The 40 ms gap
is what makes a pointer that stopped, waited, then lifted report *zero* rather
than the velocity it had before the pause. The degree-2 fit is what lets a
decelerating finger report the velocity it actually had at release rather than
its average over the window — the reason a two-point difference is not good
enough.

Fewer than three usable samples yields `None`, not a guess.

### Clamping physics

Declared in
[`ScrollPhysicsTokens`](../crates/teksilo-tokens/src/input.rs), consumed in
[`kinetic/simulation.rs`](../crates/teksilo-core/src/kinetic/simulation.rs).

| Constant | Value | Source |
| --- | --- | --- |
| `clamping_deceleration_rate` | `ln(0.78)/ln(0.9)` ≈ 2.3582018 | Android `OverScroller.DECELERATION_RATE` |
| `clamping_inflexion` | 0.35 | Android `OverScroller.INFLEXION` |
| `clamping_friction` | 0.015 | Android `ViewConfiguration.getScrollFriction()` |
| `START_TENSION` | 0.5 | Android `OverScroller.START_TENSION` |
| `END_TENSION` | 1.0 | Android `OverScroller.END_TENSION` |
| `NB_SAMPLES` | 100 | Android `OverScroller.NB_SAMPLES` |
| gravity | 9.80665 m/s² | Android `SensorManager.GRAVITY_EARTH` |
| inches per metre | 39.37 | Android `mPhysicalCoeff` |
| pixels per inch | 160 | Android density 1.0 — see below |
| tuning factor | 0.84 | Android `mPhysicalCoeff` |

`mPhysicalCoeff = 9.80665 × 39.37 × 160 × 0.84 = 51890.2017`.

**Why 160 ppi.** Android computes `ppi = displayMetrics.density × 160`. Teksilo's
logical pixel *is* the density-1 Android pixel — that is what "dp" means — so the
substitution is density 1.0. Flutter makes the same substitution when it
hardcodes `61774.04968` (= 9.80665 × 39.37 × 160).

Android's `SPLINE_POSITION` table is **rebuilt by the same bisection** rather
than approximated by a cubic fit, so a reader can put the code next to
`OverScroller.java` and check it line by line. Android's companion `SPLINE_TIME`
table is not reproduced: it serves `startScroll`'s viscous-fluid interpolator,
not the fling.

### Bouncing physics

| Constant | Value | Source |
| --- | --- | --- |
| `bouncing_decay_per_second` | 0.135 | Flutter `BouncingScrollSimulation` — `UIScrollView.decelerationRate.normal` (0.998) to the 1000th power |
| `spring_mass` | 0.5 | Flutter `SpringDescription` |
| `spring_stiffness` | 100 | Flutter `SpringDescription` |
| `spring_damping_ratio` | 1.1 | just overdamped — the return never oscillates back past the edge |
| `maxSpringTransferVelocity` | 5000 dp/s | Flutter `BouncingScrollSimulation.maxSpringTransferVelocity` |
| `rubber_band_factor` | 0.52 | Flutter `BouncingScrollPhysics.frictionFactor` (iOS uses 0.55) |

The 1.1 damping ratio makes the spring **overdamped** (`cmk = damping² − 4·m·k =
242 − 200 = 42 > 0`), which is deliberate: a scroll edge that springs back and
overshoots *inward* reads as a bug. A unit test asserts the return is monotone.

### Settle tolerances

| Constant | Value | Source |
| --- | --- | --- |
| `SETTLE_DISTANCE_TOLERANCE` | 0.1 dp | Flutter `ScrollPhysics.tolerance` `1/(devicePixelRatio × 10)` at dpr 1 |
| `SETTLE_VELOCITY_TOLERANCE` | 20 dp/s | Flutter `ScrollPhysics.tolerance` `1/(0.050 × devicePixelRatio)` at dpr 1 |
| `FLING_FRAME_INTERVAL` | 16 667 µs | 60 Hz, matching `FrameTickScheduler` and the animation schedulers |

Flutter's generic `Tolerance.defaultTolerance` (1e-3) is deliberately **not**
used: at scroll scale it keeps a simulation nominally alive for seconds after
the last visible movement.

### Fling velocity gates

Read from the active
[`GestureProfile`](../crates/teksilo-tokens/src/input.rs), not from the physics
tokens, because they are properties of the input device rather than of the
surface:

| Constant | Value | Source |
| --- | --- | --- |
| `min_fling_velocity` | 50 dp/s | Android `ViewConfiguration.MINIMUM_FLING_VELOCITY` |
| `max_fling_velocity` | 8000 dp/s | Android `ViewConfiguration.MAXIMUM_FLING_VELOCITY` |

A release **below** the floor returns `false` and starts nothing at all — that is
the point of the floor, so a slow release settles where the finger left it
rather than drifting. A release **above** the ceiling is scaled down to it,
direction preserved: clamped, never refused.

### The three deliberate divergences from upstream

Each is documented at its site in the source, with its reason:

1. **`f64` least-squares arithmetic.** A degree-2 Vandermonde matrix built from
   millisecond-scale abscissae is ill-conditioned enough that single precision
   visibly moves the answer. Flutter uses `double` for the same reason;
   Teksilo's public surface stays `f32`.
2. **The underdamped spring's decay rate.** Flutter spells it
   `-(damping / 2.0 * mass)` — that is `−(c/2)·m`, where the damped-oscillator
   solution calls for `−c/(2m)`. The two agree only at `mass == 1`, which is
   Flutter's default and why the divergence has survived there. Teksilo's spring
   has `mass == 0.5`, so the textbook form is used. Not reachable with the
   shipped tokens (ratio 1.1 is overdamped).
3. **The signed spring hand-off velocity.** Flutter caps the hand-off with a
   bare `math.min` on a quantity it has already made positive; Teksilo caps the
   *magnitude* of the signed velocity, so the leading and trailing hand-offs are
   one expression and the spring is always launched in the direction the content
   was actually travelling.

Plus one pin: `SPLINE_POSITION[0]` is set to exactly 0. Android pins only the far
end (`[NB_SAMPLES] = 1.0`), leaving the near end at the bisection's ~2.3e-5
residue — invisible there because Android starts a fling from the current scroll
position, but a `ScrollSimulation` is asked for `position(0)` directly, and
2.3e-5 of a 2157 dp fling is a 0.05 dp jump at the instant the finger lifts.

---

## 3. Reduced motion

`prefers-reduced-motion` is not advisory here. With
`KineticScroller::set_reduced_motion(true)`:

- **A fling does not coast.** It collapses to a settle that completes on the very
  next `tick`, so the content arrives at a legal offset in one step rather than
  sliding. Turning the preference on *mid-coast* collapses the running coast
  too, rather than letting it play out.
- **The rubber band is hard-clamped.** The content stops at the boundary instead
  of following the finger past it and springing back. The reported overscroll is
  always zero.
- **`FlingDriver::start` is a no-op**, and a running coast is dropped. A
  re-dispatched fling has no settle to collapse to, because the target already
  has the content where the finger left it.

That is the **scrolling** half of the accessibility rule, and the only half this
module owns. The magnifier's appearance and follow, the selection toolbar's
entrance, the long-press tooltip fade, the scrollbar reveal fade, the overlay
reposition on a soft-keyboard inset, and the density-switch relayout each belong
to the package that owns that surface.

---

## 4. The macOS momentum rule

macOS — and a precision trackpad on any host that forwards its phases —
simulates the coast itself and delivers it as a stream of
`ScrollPhase::Momentum` deltas. Starting a Teksilo fling on top of that is the
classic **double-momentum bug**: two independent simulations moving the same
content, so the content *speeds up* when the finger lifts.

The guard is a phase-aware entry point, so that no call site has to remember the
rule — the phase is already in hand at every release:

```rust
scroller.set_os_momentum(caps.reports_os_momentum);   // once, from the backend
// …then, at every release:
scroller.fling_for_phase(sample.phase, velocity, profile);
```

`fling_for_phase` starts a coast only when `should_fling_for_phase` allows it:

| `ScrollPhase` | Flings? | Why |
| --- | --- | --- |
| `Ended` | only if the host does **not** report its own momentum | the fingers lifted and nobody else is animating |
| `Fling` | yes | the host reported a release velocity and handed the coast to us |
| `Momentum` | **never** | the host is already animating — this is the bug |
| `MomentumEnded` | **never** | same |
| `Discrete`, `Began`, `Changed`, `Cancelled` | never | not a release |

`should_fling_for_phase` is public, for a consumer that must ask before it has a
velocity. Calling plain `fling` bypasses the guard; do that only where the phase
is already known to be a release Teksilo owns.

---

## 5. The two driver types

### `KineticScroller` — one surface's physics

Owned by a scrollable. Holds the offset, the range, a `VelocityTracker`, and at
most one running simulation.

```rust
let mut s = KineticScroller::new(OverscrollStyle::RubberBand);
s.set_range_y(0.0, content_height - viewport_height);
s.set_viewport(viewport_size);          // only the rubber band reads this
s.set_reduced_motion(ctx.prefers_reduced_motion());

// While the finger is down:
let step = s.pan(time, pointer_position, offset_delta);
// step.offset      — clamped into the range
// step.overscroll  — how far past it, after the rubber band
// step.absorbed    — per axis: did this axis take any of the movement?

// On release:
if s.fling_for_phase(phase, velocity, profile) { /* schedule ticks */ }

// Each frame while animating:
while let Some(step) = s.tick(now) { … }
```

**`ScrollStep::absorbed` is the boundary hand-off signal.** An axis that absorbed
nothing has nothing left to give, so the event belongs to an ancestor
scrollable. It answers one question — did the content actually move by more than
`SCROLL_MOVE_EPSILON`? — which is why an axis asked for nothing and an axis asked
for 50 dp that could only give a rounding error both report `false`. Both must
chain.

**Sign conventions.** `pan`'s `delta` is a change in *scroll offset* (positive =
the offset grows); `position` is the *pointer*, recorded only for the velocity
estimate. `pointer_velocity()` therefore follows the finger, and a surface that
drags its content opposite to the finger negates before passing it to `fling`,
which takes its velocity in offset space.

`next_deadline()` is one frame after the last movement, or `None` when nothing is
animating.

### `FlingDriver` — the tree's fling pump

The other fling shape: a coast **re-dispatched** as scroll deltas rather than
applied to one scroller's own offset.

The distinction is chaining. A scroller's fling belongs to that scroller and
stops at its bounds. A driver's fling belongs to the tree: each tick it hands out
a delta for the target widget, and if the target declines the delta because it
has reached its end, the caller chains it to an ancestor exactly as it would
chain a wheel event. That is what makes a flick that runs out of inner list
scroll the outer one.

The driver holds simulations only — bounds live with the widgets that receive
the deltas, so its simulations are unbounded coasts.

It integrates on the **existing** per-frame path rather than running a cadence of
its own: `FlingDriver::with_scheduler(tree_scheduler.clone())` takes one
`FrameTickSubscription` per coasting target and releases it when the coast ends.
`FrameTickScheduler` is an `Rc` handle, so the clone shares the tree's table
rather than creating a second one.

The tree builds one at construction and drives it from `tick_flings_with_ops`,
which rides the pass a host already runs each wake (`tick_gestures_with_ops`) —
a coast is an input deadline like a long press, and giving it a second call site
would mean every host had to learn one. `WidgetTree::advance_time` ticks it too,
so a headless test moves a fling with the same call that moves an animation.

---

## 6. The hand-off rule

A coast, and the pan that launches it, are delivered along the **claimant
chain**: the frozen `pan_candidates` list, from the container that took the claim
outward, and nothing else. `teksilo-core::widget_tree::pan_arbiter` owns the
walk. Three rules govern the boundary, and all three exist to keep an existing
contract rather than to invent one.

### Whole events, never remainders

Per event, delivery is all-or-nothing, exactly as the wheel already is:
`scroll_response` answers `Handled` if the axis absorbed anything and `Ignored`
at a hard boundary
([`common/scroll.rs`](../crates/teksilo-widgets/src/common/scroll.rs)). On
`Ignored` the router re-delivers **the same whole event** to the next container
outward.

There is **no fractional residual and no back-channel**. Both are tempting, and
both are wrong here:

- `EventResponse` is binary. It cannot carry "I took 30 of your 50 pixels", so a
  residual would mean a new return type on every scroll handler in the
  workspace — a delta-path change in every scrollable at once.
- What it would buy is invisible. The difference between handing the ancestor
  the whole 50 and handing it the leftover 20 is one frame of one gesture, at a
  boundary the user is already crossing.

So the chain hands over *events*. A container that declines one is offered the
next one first again, because the claim never left it.

### Nothing is told a lie

The claimant the chain moves past receives **no `PointerCancel` and no
pan-ended**. It did not lose the gesture; it declined one event. Cancelling it
would tear down its own gesture state in the middle of a drag it is still
winning, and a terminal phase would make it release a rubber band it is still
holding.

`OverscrollBehavior::Contain` — declared on the node via
`.overscroll_behavior(..)`, read by the chain — **stops** the walk even when the
claimant absorbed nothing, so a self-contained panel never lets a boundary pan
escape into the page behind it.

### Claimants only

The chain visits the `PanClaim` holders and **never the generic bubble**. This is
not an optimisation. The bubble path from a nested list to the window root passes
through nodes that handle `on_scroll` without being scroll containers at all: a
`SpinBox` increments its value on wheel, a `TabBar` remaps wheel to horizontal
tab scrolling. A boundary pan that reached either would change a number or switch
a tab, silently, because the user ran out of list. `PanClaim` is the declaration
that tells them apart, and the chain reads nothing else.

Wheel and trackpad keep `ScrollDelivery::Bubble` — the route they have always
had. Only `ScrollSource::TouchPan` chains along claimants, and a mouse never
forms a pan claim, so **mouse wheel chaining is unchanged**.

A fling chains identically, because it is dispatched through the same door
(`dispatch_scroll`, with `ScrollPhase::Fling`) and walks the same frozen list. A
coast the whole chain declines is stopped: it has nothing left to move.

---

## 7. One pinch ingress

`WidgetTree::dispatch_os_gesture(gesture, at, ops)` is the only way a
`GestureEvent::Pinch*` reaches a widget. Two producers arrive there:

- the OS trackpad stream (`PinchGesture` / `RotationGesture`), which winit
  reports without a position — `at` is `None` and the route falls back to the
  hover owner's last position, then the hovered widget, then the focused one,
  then the root;
- `TouchPinchRecognizer` (`gesture/pinch.rs`), which derives the same three
  phases from two contacts under a `PINCH_ZOOM`-permitting subtree and supplies
  the contact midpoint as `at`.

One ingress is the point. `SceneView::on_pinch` used to be reachable on macOS
and nowhere else, and a second, touch-only event family would have doubled the
handler surface and guaranteed the same drift again. A test asserts the two
streams are *identical*, not merely similar.

Third and later contacts are ignored: rotation through three moving points has no
unique rigid transform, so the recognizer takes the two **earliest** contacts and
says so rather than averaging something. A contact leaving mid-pinch ends the
gesture — promoting a spare into its slot would teleport the centre and the span.

---

## 8. Testing

Unit tests assert **hand-derived closed-form reference values**, not goldens this
implementation produced, so a reader can re-derive them from upstream without
running anything. The two anchor cases:

- **The exact point.** `getSplineDeceleration` is zero exactly when
  `INFLEXION·v == friction·mPhysicalCoeff`, i.e. at `v = 2223.8658` dp/s. There
  the duration is exactly 1 second and the distance is exactly
  `friction·mPhysicalCoeff = 778.3530` dp — no tolerance argument needed.
- **`v = 4000` dp/s.** `l = ln(1.7986697) = 0.5870473`, duration
  `= exp(0.4322239) = 1.54068` s, distance `= 778.3530 × exp(1.0192697) = 2156.95`
  dp. The identity `distance == friction·mPhysicalCoeff · duration^DECEL` falls
  out of the two formulas and is asserted alongside, so a future edit cannot
  break one without breaking the other.

Flutter's side is anchored the same way: `x(1 s) = 431.964`, `dx(1 s) = 135`,
`finalX = 499.381` for the friction coast at 1000 dp/s; and
`x(0.1 s) = end + 30.437` for the overdamped spring released 50 dp out at rest.

Property tests live in
[`crates/teksilo-core/tests/prop_kinetic.rs`](../crates/teksilo-core/tests/prop_kinetic.rs)
and follow [property-testing.md](property-testing.md) — including its **safe run
protocol** (`--no-run`, then the binary under `ulimit -v`) and its rule that
dependent generator inputs must be coupled with `prop_flat_map`, so an extent of
1e6 can never pair with a step of 1e-6.

---

## 9. Status

The physics is wired at the tree level and, since P21, at the widget level too —
first for `ScrollArea`, and since P22 for every other scrollable in the crate.

Landed:

- the tree owns a `FlingDriver`, built in `WidgetTree::new` from the tree's own
  `FrameTickScheduler`, driven from `tick_flings_with_ops` (and from
  `advance_time` in tests);
- `WidgetTree::next_input_deadline` folds the driver's `next_deadline()` in
  beside the gesture deadlines, and `next_timer_deadline` folds *that* into the
  one `ControlFlow::WaitUntil`;
- a finger's pan is synthesised as `Scroll { source: TouchPan }` and delivered
  along the claimant chain, with the release handing its velocity to the driver;
- `teksilo-widgets`'
  [`common::scrollable`](../crates/teksilo-widgets/src/common/scrollable.rs)
  folds the hand-rolled `on_scroll` bodies into one, and **`ScrollArea`
  owns a `KineticScroller`** — the rubber band, the boundary answer and the
  offset a pan is holding all come from it;
- every other scrollable in the crate installs it too: the five data views
  (`ListView`, `TreeView`, `GridView`, `TableView`, `TreeTableView`) and the
  three text surfaces (`RichTextEditor`, `CodeEditor`, `LogView`). `SceneView`
  keeps its own handler and gains a claim and a `TouchPan` branch — its camera
  is not an offset in `[0, max]` (§10.1);
- `ScrollBar` reaches its thumb with a finger: a 48 dp `hit_outset` over an
  unchanged 8–12 dp paint, a density-following minimum thumb length, the thumb
  and its paging track published through `target_regions`, and a reveal an
  overlay bar can be shown by while a pan runs.

What the migration found, which is not what was expected of it:

- **`MenuList` and the `TabBar` strip own no `on_scroll` at all.** Each wraps
  its scrollable region in a `ScrollArea` and inherits everything from it, as
  every other widget with a scrollable region does — through a `ScrollArea`, a
  `MenuList` or a data view, none of which owns an `on_scroll` of its own. The tab strip's wheel remap is an `on_pointer_event` **preview**
  arm, which a synthesised pan cannot reach — the claimant walk is a direct
  dispatch per claimant with no preview pass — so a vertical finger pan over a
  horizontal strip goes to the container around it rather than being turned
  sideways, which is the behaviour wanted and it is free.
- **`SpinBox` needs no `touch_action(PAN_Y)`.** Its correct behaviour already
  holds by omission: the claimant chain visits only `pan_candidates` and never
  the generic bubble, so a boundary pan cannot reach its wheel handler. Adding
  `PAN_Y` would be a *narrowing* — it would additionally forbid a horizontal
  pan and a pinch through the field — for a guarantee that is already
  structural. Witnessed by
  `a_finger_pan_over_a_hover_wheel_spin_box_scrolls_its_container` in
  `spin_box/tests.rs`, which sets `WheelMode::Hover` so the box's `on_scroll`
  actually runs for anything that reaches it; its older sibling
  `a_finger_pan_over_the_spin_box_scrolls_its_container` shows the pan
  arriving at the scroller but cannot see the claim, because the default
  `WheelMode::Focused` declines first in a fixture that never focuses the
  field.
- **A `before` arm that *rewrote* an event could not say so.** `install` ran
  the shared handler whenever the arm answered anything but `Handled`, so the
  tables' Shift+wheel remap — which builds a horizontal event and feeds it to
  `handle_scroll_event` itself — fell through and had the shared handler apply
  the **original** vertical notch on top, on any `Chain` table (the default)
  whose columns fit or which was already at its horizontal end — `Ignored` is
  the boundary answer only under `Chain`, so a `Contain` table was never
  exposed. `before` now answers
  `Option<EventResponse>`: `None` is "not mine, run the shared treatment on
  this same event", `Some(r)` is "mine, and `r` is the answer" — including
  `Some(Ignored)`, which is what chains the whole original notch outward.
  Pinned by `a_shift_notch_a_table_cannot_take_sideways_does_not_scroll_its_rows`
  in `teksilo-widgets/tests/scrollables_touch.rs`.
- **A claim on the node that owns the press arena did not win.** The router's
  stop rule broke its candidate walk at the press owner whatever the member's
  role, so a text surface — which owns the arena through its tap recognizers
  *and* carries the claim — scrolled neither itself nor the page behind it.
  Fixed in `advance_sequence` by exempting a `Pan` member from the stop; see
  §10.1.
- **A pan committed what a *press* on the row would have committed, and no
  release-time predicate can reach that.** The selection a data-view row makes
  is written on `PointerDown`: a finger pan starting on an unselected row left
  the selection on that row, and a pan over a cell-selecting `TableView`
  selected the cell under the finger while the view scrolled. The fix is to move
  the write, not to guard it — `data_views::deferred_select` records a direct
  pointer's decision and applies it on the release, where
  `release_completes_the_press` can refuse it; a precise pointer still selects
  on press, which is the desktop click convention. So the true claim about a pan
  is that it commits nothing a **click** would have committed, whichever half of
  the click the widget used to act on.
  `teksilo-widgets/tests/data_view_selection.rs`.
- **A pan committed what a release on the row would have committed.** The five
  data views' row bodies act on `PointerUp` from a **raw** `on_pointer_event`
  arm — `TreeView`'s chevron toggle, and the deferred collapse of a
  multi-selection in all five — and each carried the same premise in a comment:
  that a drag pre-empts it, because "once `active_drag` is set, `PointerUp` is
  routed to `handle_drag_drop` and never reaches this widget". True of a drag,
  false of a **won pan claim**, which raises no `active_drag`: a 120 dp finger
  pan over 200 collapsed branch rows scrolled the tree *and* expanded the row it
  started on (`visible_count` 200 → 201), and a short pan on a fully-selected
  view collapsed the selection to whichever row the finger began on. Nor can the
  row rely on being told: it is enrolled as a sequence member only when it
  carries a drag, and the loser's `CancelReason::PeerClaimed` is *member*-level
  by design (the pointer stays alive so its winner can finish), so the release
  arrives either way. Fixed in the widgets, not the router — a framework-wide
  swallow of the release would strand the *teardown* other raw `PointerUp` arms
  do there — with one predicate, `data_views::release_completes_the_press` over
  `EventContext::press_is_inside`. Question 5 of
  `teksilo-widgets/tests/scrollables_touch.rs`.
- **A drag on the node that captures the press beats every claim, and is
  invisible to `DragActivation`.** Measured as "a multi-select `GridView` does
  not pan at all" — one 120 dp pan over the same grid: no selection model or one
  in `Single` mode → offset 157 of a 2808 range; `Multi` → 0; `Multi` plus
  `.marquee_selection(false)` → 157 again. The cause is neither the claim losing
  nor the marquee running instead; both were ruled out, and an early draft that
  blamed a slop race was wrong. It is the **capture dispatch**: the router
  delivers a move to the captor *before* it advances the arbitration, so a drag
  on that node latches at `drag_slop` (18 dp) and decides the sequence, after
  which the candidate walk yields nothing and the claim is never evaluated at
  `pan_slop` (36 dp). The marquee then declined the press for landing on a tile.
  The drag won and did nothing.

  The same chain would have stopped a finger scrolling **any** reorderable data
  view, latent only because all five default `reorderable: false`. And it is why
  `DragActivation::AfterLongPress` could not have helped as written: the policy
  is read in exactly one place, the ancestor walk of the tree's sequence
  enrolment, so it binds a drag only on a node that *strictly encloses* the
  captor.

  Fixed in the widgets, structurally and with no framework change: a drag-source
  row and `GridView`'s body pane take a no-op tap
  (`data_views::press_absorber`) so the press is captured *inside* the claimant,
  and the drag moves onto a `data_views::DragSurface` that encloses it. Every
  grid tile takes the tap too, drag or no drag — the pane's own arena would
  otherwise capture a press made on a tile, and a release goes to the captor and
  bubbles outward from there, never down to the tile. A mouse resolves `Auto` to
  `Immediate` and still latches at 5 dp; a finger resolves it to
  `AfterLongPress`, so a pan wins and a held contact reorders or marquees.
  `teksilo-widgets/tests/data_view_drag.rs`.
- **A finger could not pan a table from its column-header strip**, and nothing
  said so. The header cell answered `Handled` to the `PointerDown`, which the
  root-first preview pass reads as a **preview claim** — step 1 of the decision
  procedure — and which therefore decides the sequence for that cell. Measured
  on a table scrolled to 400: the offset stayed 400, and stayed 400 with every
  column `reorderable(false)` too, so it was not the 5 dp reorder escalation.
  The cell needed no claim (its sort rides the framework's press record, its
  reorder escalates from `PointerMove`, and the resize grip takes an explicit
  capture), so it answers `Ignored` and the strip pans: 557 on the same gesture.
  That fixed the vertical axis and exposed the horizontal one — the cell keeps
  receiving moves while the contact is over it, so a swipe *along* the strip still
  escalated the column reorder at 5 dp (`max_scroll_x` 512, `scroll_x` 0, columns
  swapped). A raw `start_drag` is not a sequence member and has no
  `DragActivation` to resolve, so the cell now reads a `long_press` recognizer
  instead: a swipe pans, a held contact reorders.

Still to come:

- `CodeEditor` and `LogView` have no finger-pan test of their own. All three
  text surfaces install one `common::text_scroll::text_surface_behavior`, and
  `a_finger_pans_the_rich_text_editor` (`rich_text/tests.rs`) covers that
  helper — but a build site that dropped the call, or passed
  `PanAxes::NONE`, would be caught for the rich editor and for neither of the
  other two. The obstacle is the fixture, not the assertion: a headless text
  surface needs its engine viewport seeded and a pump before it has anything to
  scroll, and that scaffolding exists only in `rich_text/tests.rs` today;
- `Terminal` (`teksilo-terminal`) still hand-rolls its scroll. It is a
  different shape from every surface here — its offset is a scrollback ring
  position quantised to whole lines rather than a pixel offset with a maximum,
  it forwards the wheel to the child process under mouse reporting, and it
  always answers `Handled` so it never chains — so it is deferred rather than
  forced through `ScrollableAxes`;
- the platform layer does not yet produce `ScrollSource::TouchPan` samples of
  its own; the core door (`dispatch_scroll` with that source, which derives its
  own chain from the sample's position) is open and waiting for it;
- nothing paints the overscroll yet. `ScrollableAxes::overscroll` publishes it,
  and the release is an immediate clamped settle rather than a spring: a
  surface's `KineticScroller` is driven by events, not by a per-frame
  subscription of its own, so it has no tick to spring on. A stretch or glow
  renderer is what would make a spring visible, and is what will want one.

---

## 10. Adopting `ScrollableBehavior`

[`common::scrollable`](../crates/teksilo-widgets/src/common/scrollable.rs) is the
widget-side half. A surface adopts it in four steps, and `ScrollArea`
([`scroll_area.rs`](../crates/teksilo-widgets/src/scroll_area.rs)) is the
reference to copy.

**1. Own a scroller.** One `Rc<RefCell<KineticScroller>>` field, built in the
widget's constructor rather than in `build`, so the physics survives a rebuild:

```rust
scroller: Rc::new(RefCell::new(KineticScroller::new(OverscrollStyle::Clamp))),
```

**2. Publish the viewport from layout.** The rubber-band curve is a fraction of
the viewport, and the layout pass is the only place that number exists. The cell
is a `RefCell` precisely so a `&self` `place_children` can write it:

```rust
self.scroller
    .borrow_mut()
    .set_viewport(Vec2::new(viewport_width, viewport_height));
```

**3. Install the behaviour in `build`.** Both halves at once — the handler and
the pan claim:

```rust
let axes = ScrollableAxes { x, y, max_x, max_y, overscroll };
let behavior = ScrollableBehavior::new(axes)
    .with_scroller(self.scroller.clone())
    .axes(PanAxes::BOTH)
    .overscroll(self.overscroll_behavior)
    .smooth(self.smooth_scrolling)
    .line_height(self.line_height)
    .reduced_motion(ctx.prefers_reduced_motion())
    .physics(ctx.theme().input.scroll_physics);
ctx.apply_self_handlers(behavior.install(HandlerSet::new()));
```

**4. Keep your own scroll-adjacent arms in `.before(..)`.** It runs first for
every event and answers an `Option`, which is the difference between owning an
event and merely watching one. Return `None` to observe and fall through — the
shared handler then runs on that same event, which is how a surface does
bookkeeping ahead of a delta it still wants applied. Return `Some(response)` to
claim the event: the shared handler does not run at all, and `response` is the
surface's answer. `Some(Handled)` is the usual claim (a `ScrollIntoView`
reveal); `Some(Ignored)` is for an arm that consumed the event and could not
move — the tables' Shift+wheel remap at its horizontal end — where the whole
original event must chain outward and must **not** be applied a second time on
the way past.

### The four rules that are easy to get wrong

**An offset must be `Signal::new_animated`** on a surface with `smooth` on *and
a range on that axis*: `animate_to` on a plain `Signal<f32>` panics. Both paths
write an axis only when its clamp actually moved, so an axis whose range is
permanently zero is never written and may stay plain — which is what lets a
vertical-only view pin its horizontal axis, and what lets the three text
surfaces keep the plain signals they have always had.

**The claim is not optional.** `install` attaches it, but a surface that hand-
rolls the handler and forgets `.scroll_container(..)` / `.pan_claim(..)` scrolls
on a wheel and ignores a finger entirely, because `pan_candidates` is a list of
claimants and it would not be on it.

**The rubber band is off by default, and should stay off on anything nested.** A
band absorbs the movement, so a surface that follows the finger past its own end
can never hand the gesture to the container around it. The band belongs to the
outermost surface of a scroll chain.

**The end of a gesture is not movement.** `ScrollPhase::Ended` carries a zero
delta and is answered `Ignored` even under `Contain`, so the claimant walk
carries it to every container outward and each releases its own band. A claimant
that answered `Handled` there would leave the ones around it holding a band
nobody told them to let go of.

### What the surface does *not* do

It does not run the coast. A release hands its velocity to the tree's
`FlingDriver` (§5), whose deltas come back as `ScrollPhase::Fling` samples on the
same chain. The surface applies them with a **hard clamp** — the driver's
simulation is unbounded, so stopping at the boundary and declining, which is what
chains the coast outward, is the surface's job.

It also does not read a clock, subscribe to frame ticks, or animate. Everything
it does happens inside an event.

## 10.1. One surface the helper does not fit, and the rule that nearly cost three more

**A claim on the node that owns the press arena wins.**
`advance_sequence` (`teksilo-core`, `widget_tree/pointer_state.rs`) stops its
candidate walk at the node whose arena took the press — but only for a
`Gesture` member, whose recognizer the ordinary capture dispatch is already
driving and above which nothing may win, which is the "innermost drag owns the
gesture" rule. A `Pan` member is exempt, as `RawDrag` already was, because that
walk is the *only* place either is ever evaluated: a pan's product is a
synthesised `Scroll`, not a `GestureEvent` fed to an arena. Stopping at it
would have left the claim permanently inert on any node that is both the press
owner and the claimant — and, because the walk stops rather than continuing,
nothing outside it would be offered the gesture either. The five data views
were never exposed (their claim sits on the container and the press on a row);
the three text surfaces are exactly that shape, since their double- and
triple-tap recognizers put the arena on the very node that carries the claim,
and for one package a finger on them scrolled nothing at all. Pinned by
`a_pan_claimant_that_owns_the_press_arena_pans_itself` and
`an_editing_surface_at_its_boundary_still_hands_the_pan_outward`
(`teksilo-core`, `widget_tree/pan_arbiter/tests.rs`), by
`a_claim_on_the_node_that_owns_the_press_wins`
(`teksilo-widgets/tests/scrollables_touch.rs`) and by
`a_finger_pans_the_rich_text_editor`.

What the exemption did **not** change is the third shape that same press owner
can be in: raising a **drag** of its own. `RichTextEditor` does exactly that
for a press inside an existing selection that travels far enough
(`ctx.start_drag` from its `PointerMove` handler, `rich_text/mouse.rs`). The
walk looks as though it would now name the owner the winner — that is what its
`won || active_drag.is_some()` arm reads like once the stop is gone — but the
router enters the walk only while no drag is in flight, and `start_drag` is
applied the moment the handler returns. So the drag closes the door on the
arbitration instead of deciding it: nobody wins, nobody is rejected, nobody is
cancelled, and nothing scrolls. Pinned by
`a_drag_raised_by_the_press_owner_takes_the_press_out_of_the_arbitration`
(`teksilo-core`, `widget_tree/pan_arbiter/tests.rs`).

**`SceneView`'s camera is not an offset.** It keeps its own handler with a
`ScrollSource::TouchPan` branch beside the wheel one, and takes only the claim
from this module. `ScrollableAxes` models a surface as an offset in `[0, max]`
per axis; a scene's `pan` is *added* by the view transform rather than
subtracted (so its delta is negated), its legal region is an arbitrary
rectangle that moves with the zoom, and where that rectangle is smaller than
the viewport on an axis the rule is a centring pin rather than a clamp — which
no `[min, max]` range can express. An unbounded scene has no range at all. The
policy is still this module's: no tween under a finger, a hard clamp on a
coast, and `Ignored` at the boundary and at the end of the stream.
