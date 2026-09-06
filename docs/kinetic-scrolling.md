<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Kinetic Scrolling

Velocity tracking, fling physics and the rubber band, implemented once in
[`teksilo-core::kinetic`](../crates/teksilo-core/src/kinetic.rs).

Fourteen surfaces in this workspace hand-roll the same boundary clamp followed
by the same 150 ms ease-out tween. None of them tracks velocity, so none of them
can fling, and none has a rubber band. Adding physics to each would mean
fourteen integrators, fourteen tolerance constants, and fourteen places to
forget `prefers-reduced-motion` — a failure mode this codebase already
demonstrates, with `EDGE = 32` and `MAX_VELOCITY = 12` copied across five
widgets that have since drifted apart. So the physics lives in one module, as
pure computation, and the surfaces drive it.

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

---

## 6. Testing

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

## 7. Status

The module is pure computation and is **not yet wired into the widget tree**.
Nothing constructs a `FlingDriver` on a tree, no scrollable owns a
`KineticScroller`, and `WidgetTree::next_input_deadline` does not yet fold
`next_deadline()` into the event loop's `WaitUntil`. Both driver types are
deliberately self-contained — each owns its own deadline, and the driver takes a
`FrameTickScheduler` by clone rather than reaching for a tree — so installation
is a construction and a few call sites, not a redesign.
