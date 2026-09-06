// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The two scroll physics, and the rubber band.
//!
//! A fling is a closed-form function of time: given a release position and a
//! release velocity, [`ScrollSimulation`] answers where the content is at any
//! `t`, how fast it is going, and whether it has come to rest. Nothing here
//! holds a clock, a widget or a frame; the driver in
//! [`scroller`](super::scroller) advances `t` and reads the answers.
//!
//! Two families ship, because the two desktop conventions genuinely differ and
//! neither is a tuning of the other:
//!
//! - [`ClampingSimulation`] — **Android `OverScroller`**. A friction spline
//!   that decelerates to a stop, hard-stopping at the boundary. This is the
//!   Windows / GTK / Android feel, and it is what
//!   [`OverscrollStyle::Clamp`](teksilo_tokens::OverscrollStyle::Clamp)
//!   selects.
//! - [`BouncingSimulation`] — **Flutter `BouncingScrollSimulation`**, i.e. the
//!   iOS `UIScrollView` feel. Exponential decay while the content is in range,
//!   handing off to a spring the moment it crosses a boundary, so the content
//!   overshoots and is pulled back. Selected by
//!   [`OverscrollStyle::RubberBand`](teksilo_tokens::OverscrollStyle::RubberBand).
//!
//! [`rubber_band`] is the third piece and belongs to neither: it is the
//! *drag-time* companion to the bouncing simulation, the falling-gain curve
//! that makes content resist the finger past the edge. The fling never uses
//! it — past the edge, the spring is the return.
//!
//! # Why the constants are reproduced rather than invented
//!
//! Every number below is traceable to Android's `OverScroller.java` or
//! Flutter's `scroll_simulation.dart` / `spring_simulation.dart`, and each is
//! cited at its definition with the arithmetic that produces it. Scroll feel is
//! a thing users have twenty years of muscle memory for; a curve that is merely
//! *plausible* reads as wrong, and there is no way to review "plausible". A
//! reader must be able to put this file next to the upstream source and check
//! it line by line, which is why the Android spline is rebuilt by the same
//! bisection rather than approximated by a cubic fit.
//!
//! Reference: `docs/kinetic-scrolling.md`.

use std::sync::OnceLock;
use std::time::Duration;

use teksilo_tokens::ScrollPhysicsTokens;

// ---------------------------------------------------------------------------
// The trait
// ---------------------------------------------------------------------------

/// A closed-form scroll animation over one axis.
///
/// Implementors are pure: `position(t)` for the same `t` always answers the
/// same thing, and nothing is mutated by asking. That is what lets a test
/// evaluate a whole fling without a frame loop, and what lets the driver skip
/// frames without drift.
///
/// `Debug` is a supertrait so the driver types that box a simulation stay
/// printable — the same bargain [`Widget`](crate::widget::Widget) makes.
pub trait ScrollSimulation: std::fmt::Debug {
    /// Where the content is at `t`, in logical pixels.
    fn position(&self, t: Duration) -> f32;
    /// How fast the content is moving at `t`, in logical pixels per second.
    fn velocity(&self, t: Duration) -> f32;
    /// Whether the content has come to rest at or before `t`.
    fn is_done(&self, t: Duration) -> bool;
}

// ---------------------------------------------------------------------------
// Shared tolerances
// ---------------------------------------------------------------------------

/// Below this displacement the content is considered to have arrived, in
/// logical pixels.
///
/// Flutter derives its scroll tolerance as
/// `Tolerance(distance: 1.0 / (devicePixelRatio * 10.0), …)`
/// (`ScrollPhysics.tolerance` via `ScrollContext`). Teksilo works in *logical*
/// pixels, so the right substitution is `devicePixelRatio == 1`, giving 0.1.
/// Flutter's `Tolerance.defaultTolerance` (1e-3) is deliberately **not** used:
/// it is a generic physics tolerance, and at scroll scale it keeps a
/// simulation nominally alive for seconds after the last visible movement.
pub const SETTLE_DISTANCE_TOLERANCE: f32 = 0.1;

/// Below this speed the content is considered stopped, in logical pixels per
/// second.
///
/// Same derivation: Flutter's `Tolerance(velocity: 1.0 / (0.050 *
/// devicePixelRatio), …)` at `devicePixelRatio == 1` is 20 px/s — about a
/// third of a pixel per 60 Hz frame.
pub const SETTLE_VELOCITY_TOLERANCE: f32 = 20.0;

/// Turn a `Duration` into seconds without losing sub-millisecond resolution.
fn secs(t: Duration) -> f32 {
    t.as_secs_f32()
}

/// Normalise a possibly-inverted range. An empty range (`min > max`) means the
/// content fits and there is nowhere to scroll, which pins the offset at `min`.
fn normalize(min: f32, max: f32) -> (f32, f32) {
    if min > max { (min, min) } else { (min, max) }
}

// ---------------------------------------------------------------------------
// Android OverScroller — the clamping spline
// ---------------------------------------------------------------------------

/// `SensorManager.GRAVITY_EARTH`, in m/s². Android `OverScroller`'s
/// `mPhysicalCoeff` opens with this.
const GRAVITY_EARTH: f64 = 9.806_65;

/// Inches per metre, spelled as Android spells it (`39.37f`, not 39.3701).
const INCHES_PER_METER: f64 = 39.37;

/// Pixels per inch at Android's density 1.0.
///
/// Android computes `ppi = displayMetrics.density * 160`. Teksilo's logical
/// pixel *is* the density-1 Android pixel — that is what "dp" means — so the
/// right substitution is density 1.0, i.e. 160 ppi. Flutter makes the same
/// substitution when it hardcodes `61774.04968` (= 9.80665 × 39.37 × 160) in
/// `ClampingScrollSimulation._decelerationForFriction`.
const PPI_AT_DENSITY_ONE: f64 = 160.0;

/// Android's "look and feel tuning" factor, the trailing `0.84f` of
/// `mPhysicalCoeff`.
const PHYSICAL_TUNING: f64 = 0.84;

/// `mPhysicalCoeff` = 9.80665 × 39.37 × 160 × 0.84 ≈ 51890.2017.
fn physical_coeff() -> f64 {
    GRAVITY_EARTH * INCHES_PER_METER * PPI_AT_DENSITY_ONE * PHYSICAL_TUNING
}

/// Number of spline samples. Android `NB_SAMPLES`.
const NB_SAMPLES: usize = 100;

/// Android `START_TENSION`.
const START_TENSION: f64 = 0.5;

/// Android `END_TENSION`.
const END_TENSION: f64 = 1.0;

/// Safety valve on the bisection below. Android has none; a bisection that
/// fails to converge in a GUI framework is a hang, and 64 halvings take an
/// `f64` interval far below the 1e-5 acceptance band, so the cap can only ever
/// fire on a pathological float.
const MAX_BISECTION_STEPS: usize = 64;

/// Android's `SPLINE_POSITION` table, rebuilt by the same bisection.
///
/// From the static initialiser of `OverScroller.SplineOverScroller`
/// (`frameworks/base/core/java/android/widget/OverScroller.java`): for each of
/// 100 evenly spaced `alpha`, bisect for the `x` whose tension-blended
/// parameter equals `alpha`, then record the position that `x` implies.
/// `SPLINE_POSITION[NB_SAMPLES]` is pinned to exactly 1.0, which is what makes
/// a fling land on its computed distance to the pixel.
///
/// Android's companion `SPLINE_TIME` table is **not** reproduced: it serves
/// `startScroll`'s viscous-fluid interpolator, not the fling, and nothing in
/// Teksilo reads it.
///
/// The tension constants use `ScrollPhysicsTokens::DEFAULT.clamping_inflexion`
/// (0.35, Android's `INFLEXION`). The table is built once per process and is
/// *not* rebuilt when a theme overrides that token — Android hardcodes it too,
/// and the token's live effect is on the deceleration formula below, which is
/// where it actually changes the feel.
fn spline_position_table() -> &'static [f32; NB_SAMPLES + 1] {
    static TABLE: OnceLock<[f32; NB_SAMPLES + 1]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let inflexion = f64::from(ScrollPhysicsTokens::DEFAULT.clamping_inflexion);
        let p1 = START_TENSION * inflexion;
        let p2 = 1.0 - END_TENSION * (1.0 - inflexion);

        let mut table = [0.0f32; NB_SAMPLES + 1];
        // `x_min` deliberately carries between iterations, as in Android: the
        // solution is monotone in `alpha`, so the previous answer is a valid
        // lower bound for the next.
        let mut x_min = 0.0f64;
        for (i, slot) in table.iter_mut().take(NB_SAMPLES).enumerate() {
            let alpha = i as f64 / NB_SAMPLES as f64;
            let mut x_max = 1.0f64;
            let mut x = 0.0f64;
            let mut coef = 0.0f64;
            for _ in 0..MAX_BISECTION_STEPS {
                x = x_min + (x_max - x_min) / 2.0;
                coef = 3.0 * x * (1.0 - x);
                let tx = coef * ((1.0 - x) * p1 + x * p2) + x * x * x;
                if (tx - alpha).abs() < 1e-5 {
                    break;
                }
                if tx > alpha {
                    x_max = x;
                } else {
                    x_min = x;
                }
            }
            *slot = (coef * ((1.0 - x) * START_TENSION + x) + x * x * x) as f32;
        }
        // Both ends are pinned. Android pins only the far one
        // (`SPLINE_POSITION[NB_SAMPLES] = 1.0`), which is what makes a fling
        // land exactly on its computed distance; the near end is left at
        // whatever the bisection's 1e-5 acceptance band produced, about
        // 2.3e-5. That residue is invisible in Android because it starts the
        // fling from the current scroll position rather than evaluating the
        // curve at t = 0 — but a `ScrollSimulation` is asked for `position(0)`
        // directly, and 2.3e-5 of a 2157 dp fling is a 0.05 dp jump at the
        // instant the finger lifts. Pinning the near end is the same move
        // Android already makes at the far one, for the same reason.
        table[0] = 0.0;
        table[NB_SAMPLES] = 1.0;
        table
    })
}

/// Android's `update()` interpolation: the fraction of the total distance
/// covered at `frac ∈ [0, 1]`, and the local slope that yields the velocity.
fn spline_coefficients(frac: f32) -> (f32, f32) {
    let table = spline_position_table();
    // At or past the end — and for NaN, which the explicit arm catches since
    // every comparison against NaN is false: fully travelled, zero slope.
    if frac.is_nan() || frac >= 1.0 {
        return (1.0, 0.0);
    }
    let frac = frac.max(0.0);
    let index = (NB_SAMPLES as f32 * frac) as usize;
    if index >= NB_SAMPLES {
        return (1.0, 0.0);
    }
    let t_inf = index as f32 / NB_SAMPLES as f32;
    let t_sup = (index + 1) as f32 / NB_SAMPLES as f32;
    let d_inf = table[index];
    let d_sup = table[index + 1];
    let velocity_coef = (d_sup - d_inf) / (t_sup - t_inf);
    let distance_coef = d_inf + (frac - t_inf) * velocity_coef;
    (distance_coef, velocity_coef)
}

/// Android `getSplineDeceleration(velocity)` — `ln(INFLEXION · |v| /
/// (friction · mPhysicalCoeff))`.
fn spline_deceleration(velocity: f64, friction: f64, inflexion: f64) -> f64 {
    (inflexion * velocity.abs() / (friction * physical_coeff())).ln()
}

/// How long a fling at `velocity` lasts, in seconds.
///
/// Android `getSplineFlingDuration`: `exp(l / (DECELERATION_RATE − 1))`.
/// Returns zero for a velocity that cannot fling (zero, non-finite, or a
/// non-positive friction token).
pub fn fling_duration(velocity: f32, tokens: &ScrollPhysicsTokens) -> Duration {
    let Some(l) = spline_l(velocity, tokens) else {
        return Duration::ZERO;
    };
    let rate = f64::from(tokens.clamping_deceleration_rate);
    if rate <= 1.0 {
        return Duration::ZERO;
    }
    let seconds = (l / (rate - 1.0)).exp();
    if seconds.is_finite() && seconds > 0.0 {
        Duration::from_secs_f64(seconds.min(60.0))
    } else {
        Duration::ZERO
    }
}

/// How far a fling at `velocity` travels, in logical pixels (unsigned).
///
/// Android `getSplineFlingDistance`: `friction · mPhysicalCoeff ·
/// exp(DECELERATION_RATE / (DECELERATION_RATE − 1) · l)`.
pub fn fling_distance(velocity: f32, tokens: &ScrollPhysicsTokens) -> f32 {
    let Some(l) = spline_l(velocity, tokens) else {
        return 0.0;
    };
    let rate = f64::from(tokens.clamping_deceleration_rate);
    if rate <= 1.0 {
        return 0.0;
    }
    let friction = f64::from(tokens.clamping_friction);
    let distance = friction * physical_coeff() * (rate / (rate - 1.0) * l).exp();
    if distance.is_finite() && distance > 0.0 {
        distance as f32
    } else {
        0.0
    }
}

/// The shared `l` of the two formulas above, or `None` when the inputs cannot
/// produce a fling.
fn spline_l(velocity: f32, tokens: &ScrollPhysicsTokens) -> Option<f64> {
    let friction = f64::from(tokens.clamping_friction);
    let inflexion = f64::from(tokens.clamping_inflexion);
    if !velocity.is_finite() || velocity == 0.0 || friction <= 0.0 || inflexion <= 0.0 {
        return None;
    }
    let l = spline_deceleration(f64::from(velocity), friction, inflexion);
    l.is_finite().then_some(l)
}

/// Android `OverScroller`'s friction fling, bounded by a scroll range.
///
/// The content decelerates along Android's spline and stops dead at `min` or
/// `max` — no overshoot, no bounce. This is the [`Clamp`] half of the pair, and
/// the physics every Teksilo scrollable gets unless its theme or its owner asks
/// for the other one.
///
/// [`Clamp`]: teksilo_tokens::OverscrollStyle::Clamp
///
/// ```
/// use std::time::Duration;
/// use teksilo_core::kinetic::{ClampingSimulation, ScrollSimulation};
/// use teksilo_tokens::ScrollPhysicsTokens;
///
/// let tokens = ScrollPhysicsTokens::DEFAULT;
/// let sim = ClampingSimulation::new(0.0, 4000.0, -1.0e6, 1.0e6, &tokens);
/// // Android's getSplineFlingDistance(4000) — see the tests for the arithmetic.
/// assert!((sim.position(sim.duration()) - 2156.95).abs() < 1.0);
/// assert!(sim.is_done(sim.duration()));
/// ```
#[derive(Clone, Debug)]
pub struct ClampingSimulation {
    start: f32,
    /// Signed total travel; the spline covers exactly this by `duration`.
    distance: f32,
    duration: Duration,
    duration_secs: f32,
    min: f32,
    max: f32,
}

impl ClampingSimulation {
    /// A fling released at `position` with `velocity`, confined to
    /// `[min, max]`.
    ///
    /// Pass a very wide range for an unbounded coast — that is what
    /// [`FlingDriver`](super::FlingDriver) does, since the widget receiving the
    /// re-dispatched deltas owns the real bounds.
    pub fn new(
        position: f32,
        velocity: f32,
        min: f32,
        max: f32,
        tokens: &ScrollPhysicsTokens,
    ) -> Self {
        let (min, max) = normalize(min, max);
        let duration = fling_duration(velocity, tokens);
        let magnitude = fling_distance(velocity, tokens);
        let distance = if velocity < 0.0 {
            -magnitude
        } else {
            magnitude
        };
        Self {
            start: position,
            distance,
            duration,
            duration_secs: duration.as_secs_f32(),
            min,
            max,
        }
    }

    /// How long the fling runs before the spline itself is spent. The
    /// simulation may finish earlier by hitting a bound.
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// The signed distance the spline covers over its whole duration.
    pub fn distance(&self) -> f32 {
        self.distance
    }

    /// Where the spline would be, ignoring the bounds.
    fn unbounded_position(&self, t: Duration) -> f32 {
        if self.duration_secs <= 0.0 {
            return self.start + self.distance;
        }
        let frac = secs(t) / self.duration_secs;
        let (distance_coef, _) = spline_coefficients(frac);
        self.start + self.distance * distance_coef
    }
}

impl ScrollSimulation for ClampingSimulation {
    fn position(&self, t: Duration) -> f32 {
        self.unbounded_position(t).clamp(self.min, self.max)
    }

    fn velocity(&self, t: Duration) -> f32 {
        if self.duration_secs <= 0.0 || self.is_done(t) {
            return 0.0;
        }
        let frac = secs(t) / self.duration_secs;
        let (_, velocity_coef) = spline_coefficients(frac);
        velocity_coef * self.distance / self.duration_secs
    }

    fn is_done(&self, t: Duration) -> bool {
        if t >= self.duration {
            return true;
        }
        // A bound reached mid-flight ends the fling: clamping physics does not
        // bounce, so there is nothing left to animate. Only the bound *in the
        // direction of travel* counts — a fling released while already sitting
        // on the near bound is starting, not finishing.
        let raw = self.unbounded_position(t);
        if self.distance > 0.0 {
            raw >= self.max
        } else if self.distance < 0.0 {
            raw <= self.min
        } else {
            true
        }
    }
}

// ---------------------------------------------------------------------------
// Flutter's friction simulation
// ---------------------------------------------------------------------------

/// Flutter `FrictionSimulation` with `constantDeceleration: 0`.
///
/// `x(t) = p + v·(dragᵗ − 1)/ln(drag)`, `dx(t) = v·dragᵗ`. The drag constant is
/// `bouncing_decay_per_second` — 0.135, which Flutter documents as
/// `UIScrollView.decelerationRate.normal` (0.998) raised to the 1000th power,
/// i.e. iOS's per-millisecond retention re-expressed per second.
#[derive(Clone, Copy, Debug)]
struct FrictionSimulation {
    drag: f32,
    drag_log: f32,
    position: f32,
    velocity: f32,
}

impl FrictionSimulation {
    fn new(drag: f32, position: f32, velocity: f32) -> Self {
        // A drag outside (0, 1) has no decay; pin it to something inert rather
        // than producing an infinite or growing coast.
        let drag = if drag.is_finite() && drag > 0.0 && drag < 1.0 {
            drag
        } else {
            ScrollPhysicsTokens::DEFAULT.bouncing_decay_per_second
        };
        Self {
            drag,
            drag_log: drag.ln(),
            position,
            velocity,
        }
    }

    fn x(&self, t: f32) -> f32 {
        self.position + self.velocity * (self.drag.powf(t) - 1.0) / self.drag_log
    }

    fn dx(&self, t: f32) -> f32 {
        self.velocity * self.drag.powf(t)
    }

    /// Where the coast ends if nothing interrupts it — Flutter's `finalX`.
    fn final_x(&self) -> f32 {
        self.position - self.velocity / self.drag_log
    }

    /// When the coast passes `x`, or `INFINITY` if it never does. Flutter's
    /// `timeAtX`.
    fn time_at_x(&self, x: f32) -> f32 {
        if x == self.position {
            return 0.0;
        }
        let final_x = self.final_x();
        let unreachable = if self.velocity > 0.0 {
            x < self.position || x > final_x
        } else {
            x > self.position || x < final_x
        };
        if self.velocity == 0.0 || unreachable {
            return f32::INFINITY;
        }
        ((self.drag_log * (x - self.position) / self.velocity + 1.0).ln() / self.drag_log).max(0.0)
    }

    fn is_done(&self, t: f32) -> bool {
        self.dx(t).abs() < SETTLE_VELOCITY_TOLERANCE
    }
}

// ---------------------------------------------------------------------------
// Flutter's spring simulation
// ---------------------------------------------------------------------------

/// The closed-form solution of a damped harmonic oscillator, in the three
/// regimes Flutter's `_SpringSolution` distinguishes.
#[derive(Clone, Copy, Debug)]
enum SpringSolution {
    /// `cmk > 0` — the regime Teksilo's shipped tokens select (damping ratio
    /// 1.1). Flutter `_OverdampedSolution`.
    Overdamped { r1: f32, r2: f32, c1: f32, c2: f32 },
    /// `cmk == 0`. Flutter `_CriticalSolution`.
    Critical { r: f32, c1: f32, c2: f32 },
    /// `cmk < 0` — reachable only if an app sets a damping ratio below 1.0.
    ///
    /// **Deliberate divergence from Flutter**: Flutter spells the decay rate
    /// `-(damping / 2.0 * mass)`, i.e. `−(c/2)·m`, where the damped-oscillator
    /// solution calls for `−c/(2m)`. The two agree only at `mass == 1`, and
    /// Flutter's default spring has `mass == 1`, which is why the divergence
    /// has survived there. Teksilo's spring has `mass == 0.5`, so reproducing
    /// the expression would give a visibly wrong decay; the textbook form is
    /// used instead. Not exercised by the shipped tokens.
    Underdamped { w: f32, r: f32, c1: f32, c2: f32 },
}

impl SpringSolution {
    fn new(mass: f32, stiffness: f32, damping: f32, distance: f32, velocity: f32) -> Self {
        let cmk = damping * damping - 4.0 * mass * stiffness;
        if cmk > 0.0 {
            let root = cmk.sqrt();
            let r1 = (-damping - root) / (2.0 * mass);
            let r2 = (-damping + root) / (2.0 * mass);
            let c2 = (velocity - r1 * distance) / (r2 - r1);
            let c1 = distance - c2;
            SpringSolution::Overdamped { r1, r2, c1, c2 }
        } else if cmk < 0.0 {
            let w = (4.0 * mass * stiffness - damping * damping).sqrt() / (2.0 * mass);
            // See the variant's docs for why this is not Flutter's expression.
            let r = -damping / (2.0 * mass);
            SpringSolution::Underdamped {
                w,
                r,
                c1: distance,
                c2: (velocity - r * distance) / w,
            }
        } else {
            let r = -damping / (2.0 * mass);
            SpringSolution::Critical {
                r,
                c1: distance,
                c2: velocity - r * distance,
            }
        }
    }

    fn x(&self, t: f32) -> f32 {
        match *self {
            SpringSolution::Overdamped { r1, r2, c1, c2 } => {
                c1 * (r1 * t).exp() + c2 * (r2 * t).exp()
            }
            SpringSolution::Critical { r, c1, c2 } => (c1 + c2 * t) * (r * t).exp(),
            SpringSolution::Underdamped { w, r, c1, c2 } => {
                (r * t).exp() * (c1 * (w * t).cos() + c2 * (w * t).sin())
            }
        }
    }

    fn dx(&self, t: f32) -> f32 {
        match *self {
            SpringSolution::Overdamped { r1, r2, c1, c2 } => {
                c1 * r1 * (r1 * t).exp() + c2 * r2 * (r2 * t).exp()
            }
            SpringSolution::Critical { r, c1, c2 } => {
                let power = (r * t).exp();
                r * (c1 + c2 * t) * power + c2 * power
            }
            SpringSolution::Underdamped { w, r, c1, c2 } => {
                let power = (r * t).exp();
                let cos = (w * t).cos();
                let sin = (w * t).sin();
                power * (c2 * w * cos - c1 * w * sin) + r * power * (c2 * sin + c1 * cos)
            }
        }
    }
}

/// Flutter `ScrollSpringSimulation`: pulls the content back to `end`.
#[derive(Clone, Copy, Debug)]
struct SpringSimulation {
    end: f32,
    solution: SpringSolution,
}

impl SpringSimulation {
    fn new(tokens: &ScrollPhysicsTokens, start: f32, end: f32, velocity: f32) -> Self {
        let mass = if tokens.spring_mass > 0.0 {
            tokens.spring_mass
        } else {
            ScrollPhysicsTokens::DEFAULT.spring_mass
        };
        let stiffness = if tokens.spring_stiffness > 0.0 {
            tokens.spring_stiffness
        } else {
            ScrollPhysicsTokens::DEFAULT.spring_stiffness
        };
        // Flutter `SpringDescription.withDampingRatio`:
        // `damping = ratio · 2 · sqrt(mass · stiffness)`.
        let damping = tokens.spring_damping_ratio * 2.0 * (mass * stiffness).sqrt();
        Self {
            end,
            solution: SpringSolution::new(mass, stiffness, damping, start - end, velocity),
        }
    }

    fn x(&self, t: f32) -> f32 {
        self.end + self.solution.x(t)
    }

    fn dx(&self, t: f32) -> f32 {
        self.solution.dx(t)
    }

    fn is_done(&self, t: f32) -> bool {
        self.solution.x(t).abs() < SETTLE_DISTANCE_TOLERANCE
            && self.solution.dx(t).abs() < SETTLE_VELOCITY_TOLERANCE
    }
}

// ---------------------------------------------------------------------------
// The bouncing simulation
// ---------------------------------------------------------------------------

/// Flutter `BouncingScrollSimulation` — the iOS `UIScrollView` fling.
///
/// While the content is inside `[min, max]` it coasts on exponential decay.
/// The moment the coast would cross a bound, the simulation hands off to a
/// spring anchored at that side's rest target, so the content overshoots and is
/// pulled back. A release that *starts* outside the range skips the coast and
/// springs immediately.
///
/// # The two pairs of bounds
///
/// `min`/`max` and `leading`/`trailing` are different questions, and conflating
/// them is the reason both are parameters:
///
/// - **`min` / `max`** — the *scroll extent*: the offsets past which the
///   content is overscrolled. This is where the coast gives way to the spring.
/// - **`leading` / `trailing`** — the spring's *rest targets*: where
///   overscrolled content comes to rest. Normally `leading == min` and
///   `trailing == max`, which is what [`within`](Self::within) fills in. They
///   differ when the resting place is not the crossing place — content that
///   snaps to a sticky header, or to a paged boundary, past the edge it
///   crossed.
///
/// ```
/// use std::time::Duration;
/// use teksilo_core::kinetic::{BouncingSimulation, ScrollSimulation};
///
/// // Released mid-range with plenty of room: pure exponential decay.
/// let sim = BouncingSimulation::within(0.0, 1000.0, -1.0e6, 1.0e6);
/// // Flutter FrictionSimulation: x(1s) = v·(0.135 − 1)/ln(0.135).
/// assert!((sim.position(Duration::from_secs(1)) - 431.96).abs() < 0.5);
/// ```
#[derive(Clone, Copy, Debug)]
pub struct BouncingSimulation {
    friction: FrictionSimulation,
    spring: Option<SpringSimulation>,
    /// Seconds at which the spring takes over. `INFINITY` = never; `-INFINITY`
    /// = from the start (the release was already out of range).
    spring_time: f32,
}

impl BouncingSimulation {
    /// The common case: the spring rests exactly at the bound it crossed.
    pub fn within(position: f32, velocity: f32, min: f32, max: f32) -> Self {
        Self::new(position, velocity, min, max, min, max)
    }

    /// A bouncing fling with explicit spring rest targets, using the shipped
    /// physics tokens. See the type docs for what the two pairs mean.
    pub fn new(
        position: f32,
        velocity: f32,
        min: f32,
        max: f32,
        leading: f32,
        trailing: f32,
    ) -> Self {
        Self::with_tokens(
            position,
            velocity,
            min,
            max,
            leading,
            trailing,
            &ScrollPhysicsTokens::DEFAULT,
        )
    }

    /// [`new`](Self::new) against a theme's own physics tokens.
    pub fn with_tokens(
        position: f32,
        velocity: f32,
        min: f32,
        max: f32,
        leading: f32,
        trailing: f32,
        tokens: &ScrollPhysicsTokens,
    ) -> Self {
        let (min, max) = normalize(min, max);
        let friction =
            FrictionSimulation::new(tokens.bouncing_decay_per_second, position, velocity);

        // Released already out of range: spring from here, no coast at all.
        if position < min {
            return Self {
                friction,
                spring: Some(SpringSimulation::new(tokens, position, leading, velocity)),
                spring_time: f32::NEG_INFINITY,
            };
        }
        if position > max {
            return Self {
                friction,
                spring: Some(SpringSimulation::new(tokens, position, trailing, velocity)),
                spring_time: f32::NEG_INFINITY,
            };
        }

        // In range: coast, and hand off only if the coast would leave it.
        let final_x = friction.final_x();
        if velocity > 0.0 && final_x > max {
            let t = friction.time_at_x(max);
            if t.is_finite() {
                let transfer = cap_magnitude(friction.dx(t), MAX_SPRING_TRANSFER_VELOCITY);
                return Self {
                    friction,
                    spring: Some(SpringSimulation::new(tokens, max, trailing, transfer)),
                    spring_time: t,
                };
            }
        } else if velocity < 0.0 && final_x < min {
            let t = friction.time_at_x(min);
            if t.is_finite() {
                let transfer = cap_magnitude(friction.dx(t), MAX_SPRING_TRANSFER_VELOCITY);
                return Self {
                    friction,
                    spring: Some(SpringSimulation::new(tokens, min, leading, transfer)),
                    spring_time: t,
                };
            }
        }

        Self {
            friction,
            spring: None,
            spring_time: f32::INFINITY,
        }
    }

    /// When the spring takes over, in seconds; `INFINITY` when it never does.
    pub fn spring_time(&self) -> f32 {
        self.spring_time
    }

    /// Whether `t` is in the spring phase, and the time offset to apply.
    fn phase(&self, t: f32) -> (bool, f32) {
        if self.spring.is_some() && t > self.spring_time {
            let offset = if self.spring_time.is_finite() {
                self.spring_time
            } else {
                0.0
            };
            (true, offset)
        } else {
            (false, 0.0)
        }
    }
}

/// Flutter `BouncingScrollSimulation.maxSpringTransferVelocity`: however fast
/// the content is going when it reaches the edge, the spring is never handed
/// more than this, so a violent flick does not launch the content into the
/// next county before it comes back.
///
/// Flutter applies it with a bare `math.min`, which reads as a cap on a
/// quantity it has already made positive; Teksilo caps the **magnitude** of the
/// signed velocity instead, so the leading and trailing hand-offs are one
/// expression and the spring is always launched in the direction the content
/// was actually travelling.
const MAX_SPRING_TRANSFER_VELOCITY: f32 = 5000.0;

/// `v`, with its magnitude limited to `limit` and its sign preserved.
fn cap_magnitude(v: f32, limit: f32) -> f32 {
    v.clamp(-limit, limit)
}

impl ScrollSimulation for BouncingSimulation {
    fn position(&self, t: Duration) -> f32 {
        let t = secs(t);
        match self.phase(t) {
            (true, offset) => self
                .spring
                .expect("phase() only reports the spring when there is one")
                .x(t - offset),
            (false, _) => self.friction.x(t),
        }
    }

    fn velocity(&self, t: Duration) -> f32 {
        let t = secs(t);
        match self.phase(t) {
            (true, offset) => self
                .spring
                .expect("phase() only reports the spring when there is one")
                .dx(t - offset),
            (false, _) => self.friction.dx(t),
        }
    }

    fn is_done(&self, t: Duration) -> bool {
        let t = secs(t);
        match self.phase(t) {
            (true, offset) => self
                .spring
                .expect("phase() only reports the spring when there is one")
                .is_done(t - offset),
            (false, _) => self.friction.is_done(t),
        }
    }
}

// ---------------------------------------------------------------------------
// Rubber band
// ---------------------------------------------------------------------------

/// How far content actually moves when the finger drags it `offset` past the
/// edge of a viewport `extent` pixels long.
///
/// This is the *drag-time* half of the bouncing feel; the fling never uses it
/// (past the edge, the spring is the return). It is what makes the content
/// resist: the first pixel past the boundary moves the content about half a
/// pixel, and the resistance grows until the content asymptotically refuses to
/// move at all.
///
/// # The formula, and why the two upstream statements agree
///
/// Flutter's `BouncingScrollPhysics` states the rule as a *gain*:
/// `frictionFactor(f) = 0.52 · (1 − f)²`, where `f` is how far past the edge
/// the content already is as a fraction of the viewport. Integrating that gain
/// over the finger's travel `x`:
///
/// ```text
///   du/dx = k·(1 − u)² / extent          u = damped/extent, k = 0.52
///   ⇒ 1/(1 − u) − 1 = k·x/extent
///   ⇒ damped = extent · (1 − 1/(1 + k·x/extent))
/// ```
///
/// which is exactly the closed form iOS `UIScrollView` is documented to use,
/// `(1 − 1/(x·c/d + 1))·d`, with Flutter's `c = 0.52` in place of Apple's 0.55.
/// So the two upstream descriptions are one curve, and this function is it.
///
/// # Guarantees
///
/// - `rubber_band(0, e) == 0` — the identity at zero, so a drag that has not
///   left the range is untouched.
/// - Monotone non-decreasing in `offset`, and odd: `f(−x) == −f(x)`.
/// - `|rubber_band(x, e)| < e` for every finite `x` — the content can never be
///   dragged a full viewport past the edge.
/// - A non-finite `offset`, or a non-positive or non-finite `extent`, yields
///   zero rather than a NaN. (A NaN here would propagate straight into a
///   layout offset; the workspace has two recorded bugs of exactly that shape,
///   see `docs/property-testing.md`.)
pub fn rubber_band(offset: f32, extent: f32) -> f32 {
    rubber_band_with(
        offset,
        extent,
        ScrollPhysicsTokens::DEFAULT.rubber_band_factor,
    )
}

/// [`rubber_band`] against a theme's own `rubber_band_factor`.
pub fn rubber_band_with(offset: f32, extent: f32, factor: f32) -> f32 {
    if offset.is_nan()
        || !extent.is_finite()
        || extent <= 0.0
        || !factor.is_finite()
        || factor <= 0.0
    {
        return 0.0;
    }
    if offset.is_infinite() {
        // The limit of the curve, so the function stays continuous rather than
        // falling off a cliff at infinity.
        return extent.copysign(offset);
    }
    // In `f64`: `1 - 1/(1 + small)` cancels catastrophically in single
    // precision, and the error lands squarely on the gain near the origin —
    // which is the one place the curve's agreement with Flutter's
    // `frictionFactor` is checkable.
    let x = f64::from(offset.abs());
    let extent = f64::from(extent);
    let factor = f64::from(factor);
    let damped = (extent * (1.0 - 1.0 / (factor * x / extent + 1.0))) as f32;
    if offset < 0.0 { -damped } else { damped }
}

/// The inverse of [`rubber_band`]: how far the finger travelled to produce a
/// damped displacement of `damped`.
///
/// Needed whenever a drag *resumes* from a position the physics put there — a
/// fling that left the content overscrolled, then a finger grabbing it — since
/// the drag has to continue from the right point on the curve rather than
/// restart at the origin of it.
///
/// ```text
///   damped = e·(1 − 1/(k·x/e + 1))   ⇒   x = e·damped / (k·(e − damped))
/// ```
///
/// Saturates: `|damped|` is treated as at most `extent·(1 − 1e-4)` so the pole
/// at the asymptote cannot return an infinity.
pub fn rubber_band_inverse(damped: f32, extent: f32) -> f32 {
    rubber_band_inverse_with(
        damped,
        extent,
        ScrollPhysicsTokens::DEFAULT.rubber_band_factor,
    )
}

/// [`rubber_band_inverse`] against a theme's own `rubber_band_factor`.
pub fn rubber_band_inverse_with(damped: f32, extent: f32, factor: f32) -> f32 {
    if !damped.is_finite()
        || !extent.is_finite()
        || extent <= 0.0
        || !factor.is_finite()
        || factor <= 0.0
    {
        return 0.0;
    }
    let y = f64::from(damped.abs().min(extent * (1.0 - 1e-4)));
    let extent = f64::from(extent);
    let factor = f64::from(factor);
    let raw = (extent * y / (factor * (extent - y))) as f32;
    if damped < 0.0 { -raw } else { raw }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> ScrollPhysicsTokens {
        ScrollPhysicsTokens::DEFAULT
    }

    // --- The spline table -------------------------------------------------

    /// Android pins both ends of `SPLINE_POSITION`, and the curve between them
    /// is a distance fraction, so it must be monotone and stay in `[0, 1]`.
    #[test]
    fn the_spline_table_runs_from_zero_to_one_monotonically() {
        let table = spline_position_table();
        // Both ends pinned — see the table's construction for why the near
        // one is pinned here but not in Android.
        assert_eq!(table[0], 0.0, "SPLINE_POSITION[0]");
        assert_eq!(table[NB_SAMPLES], 1.0, "SPLINE_POSITION[NB_SAMPLES]");
        // The pin must not be papering over a wrong curve: the first sample
        // Android computes is ~2.3e-5, so the second entry has to be small.
        assert!(table[1] < 0.05, "SPLINE_POSITION[1] = {}", table[1]);
        for w in table.windows(2) {
            assert!(
                w[1] >= w[0],
                "the distance fraction went backwards: {} then {}",
                w[0],
                w[1]
            );
        }
        assert!(table.iter().all(|d| (0.0..=1.0).contains(d)));
    }

    // --- Closed-form Android reference values -----------------------------

    /// The one input for which Android's spline formulas have an *exact*
    /// answer, so it needs no tolerance argument at all.
    ///
    /// `getSplineDeceleration(v) = ln(INFLEXION·|v| / (friction·mPhysicalCoeff))`
    /// is zero exactly when `INFLEXION·v == friction·mPhysicalCoeff`. At that
    /// velocity:
    ///
    /// - `getSplineFlingDuration = exp(0 / (DECELERATION_RATE − 1)) = 1` second;
    /// - `getSplineFlingDistance = friction·mPhysicalCoeff · exp(0) =
    ///   friction·mPhysicalCoeff`.
    ///
    /// With Teksilo's tokens: `mPhysicalCoeff = 9.80665 × 39.37 × 160 × 0.84 =
    /// 51890.2017`, so `friction·mPhysicalCoeff = 0.015 × 51890.2017 =
    /// 778.3530`, and the velocity is `778.3530 / 0.35 = 2223.8658` dp/s.
    #[test]
    fn the_spline_formulas_match_android_at_their_exact_point() {
        let t = tokens();
        // 0.015 × 51890.2017312 = 778.353025968 in exact arithmetic; the
        // token is an `f32`, whose nearest value to 0.015 is 0.0149999997, so
        // the product lands 1.7e-8 relative below the decimal.
        let scaled_friction = f64::from(t.clamping_friction) * physical_coeff();
        assert!(
            (scaled_friction - 778.353_025_968).abs() < 1e-4,
            "friction · mPhysicalCoeff = {scaled_friction}, expected 778.353025968"
        );

        let velocity = (scaled_friction / f64::from(t.clamping_inflexion)) as f32;
        assert!(
            (velocity - 2223.8658).abs() < 0.01,
            "the exact-point velocity is {velocity}"
        );

        let duration = fling_duration(velocity, &t).as_secs_f64();
        assert!(
            (duration - 1.0).abs() < 1e-4,
            "getSplineFlingDuration must be exactly 1 s here, got {duration}"
        );
        let distance = f64::from(fling_distance(velocity, &t));
        assert!(
            (distance - scaled_friction).abs() < 0.05,
            "getSplineFlingDistance must be friction·mPhysicalCoeff = {scaled_friction}, got {distance}"
        );
    }

    /// A second Android reference point, worked out by hand so a reader can
    /// re-derive it from `OverScroller.java` without running anything.
    ///
    /// For `v = 4000` dp/s, friction 0.015, `mPhysicalCoeff = 51890.2017`:
    ///
    /// ```text
    ///   l        = ln(0.35 × 4000 / 778.3530) = ln(1.7986697) = 0.5870473
    ///   DECEL    = ln(0.78)/ln(0.9)           = 2.3582018
    ///   duration = exp(l / (DECEL − 1))       = exp(0.4322239) = 1.54068 s
    ///   distance = 778.3530 × exp(DECEL/(DECEL−1) × l)
    ///            = 778.3530 × exp(1.0192697)  = 2156.95 dp
    /// ```
    ///
    /// The identity `distance == friction·mPhysicalCoeff · duration^DECEL`
    /// falls straight out of the two formulas and is asserted alongside, so a
    /// future edit cannot break one without breaking the other.
    #[test]
    fn a_four_thousand_dp_per_second_fling_matches_the_hand_derivation() {
        let t = tokens();
        let duration = fling_duration(4000.0, &t).as_secs_f64();
        assert!(
            (duration - 1.540_68).abs() < 1e-3,
            "getSplineFlingDuration(4000) = {duration}, expected 1.54068 s"
        );

        let distance = f64::from(fling_distance(4000.0, &t));
        assert!(
            (distance - 2156.95).abs() < 1.0,
            "getSplineFlingDistance(4000) = {distance}, expected 2156.95 dp"
        );

        let scaled_friction = f64::from(t.clamping_friction) * physical_coeff();
        let rate = f64::from(t.clamping_deceleration_rate);
        let implied = scaled_friction * duration.powf(rate);
        assert!(
            (implied - distance).abs() < 0.5,
            "distance {distance} must equal friction·coeff·duration^DECEL = {implied}"
        );
    }

    /// `ln(0.78) / ln(0.9)`, the exponent every clamping formula shares.
    #[test]
    fn the_deceleration_rate_token_is_the_android_ratio() {
        let expected = 0.78f64.ln() / 0.9f64.ln();
        assert!(
            (f64::from(tokens().clamping_deceleration_rate) - expected).abs() < 1e-5,
            "DECELERATION_RATE token vs ln(0.78)/ln(0.9) = {expected}"
        );
    }

    // --- ClampingSimulation ----------------------------------------------

    #[test]
    fn a_clamping_fling_lands_on_its_computed_distance() {
        let t = tokens();
        let sim = ClampingSimulation::new(0.0, 4000.0, -1.0e6, 1.0e6, &t);
        let end = sim.position(sim.duration());
        assert!(
            (end - 2156.95).abs() < 1.0,
            "landed at {end}, expected 2156.95"
        );
        assert!(sim.is_done(sim.duration()));
        assert_eq!(sim.position(Duration::ZERO), 0.0, "starts where released");
    }

    /// The spline's fraction is monotone, so the fling never reverses.
    #[test]
    fn a_clamping_fling_is_monotone_and_decelerating() {
        let t = tokens();
        let sim = ClampingSimulation::new(0.0, 3000.0, -1.0e6, 1.0e6, &t);
        let mut previous = sim.position(Duration::ZERO);
        let mut previous_speed = sim.velocity(Duration::ZERO);
        for ms in (0..1600).step_by(16) {
            let now = Duration::from_millis(ms);
            let p = sim.position(now);
            let v = sim.velocity(now);
            assert!(
                p >= previous - 1e-3,
                "reversed at {ms} ms: {previous} → {p}"
            );
            assert!(
                v <= previous_speed + 1.0,
                "accelerated at {ms} ms: {previous_speed} → {v}"
            );
            previous = p;
            previous_speed = v;
        }
    }

    /// Clamping physics stops dead at the boundary: no overshoot, and the
    /// simulation reports itself finished the moment it arrives.
    #[test]
    fn a_clamping_fling_stops_dead_at_the_boundary() {
        let t = tokens();
        let sim = ClampingSimulation::new(0.0, 4000.0, 0.0, 100.0, &t);
        let late = Duration::from_millis(900);
        assert_eq!(sim.position(late), 100.0, "must not pass the boundary");
        assert!(sim.is_done(late), "arriving at the bound ends the fling");
        assert_eq!(sim.velocity(late), 0.0, "and leaves no residual velocity");
    }

    #[test]
    fn a_zero_velocity_clamping_fling_is_already_over() {
        let t = tokens();
        let sim = ClampingSimulation::new(42.0, 0.0, -1.0e6, 1.0e6, &t);
        assert_eq!(sim.duration(), Duration::ZERO);
        assert_eq!(sim.position(Duration::ZERO), 42.0);
        assert!(sim.is_done(Duration::ZERO));
    }

    #[test]
    fn a_negative_clamping_fling_travels_backwards() {
        let t = tokens();
        let sim = ClampingSimulation::new(0.0, -4000.0, -1.0e6, 1.0e6, &t);
        let end = sim.position(sim.duration());
        assert!(
            (end + 2156.95).abs() < 1.0,
            "landed at {end}, expected -2156.95"
        );
    }

    // --- Closed-form Flutter reference values -----------------------------

    /// Flutter `FrictionSimulation` with `drag = 0.135`:
    /// `x(t) = p + v·(0.135ᵗ − 1)/ln(0.135)` and `dx(t) = v·0.135ᵗ`.
    ///
    /// With `p = 0`, `v = 1000`, `ln(0.135) = −2.0024805`:
    ///
    /// ```text
    ///   x(1 s)  = 1000 × (0.135 − 1) / −2.0024805 = 431.964 dp
    ///   dx(1 s) = 1000 × 0.135                    = 135 dp/s
    ///   finalX  = −v / ln(0.135) = 1000 / 2.0024805 = 499.381 dp
    /// ```
    #[test]
    fn the_in_range_coast_matches_flutters_friction_simulation() {
        let ln_drag = 0.135f64.ln();
        assert!(
            (ln_drag + 2.002_480_5).abs() < 1e-6,
            "ln(0.135) = {ln_drag}, expected -2.0024805"
        );

        let sim = BouncingSimulation::within(0.0, 1000.0, -1.0e6, 1.0e6);
        let x1 = sim.position(Duration::from_secs(1));
        assert!(
            (x1 - 431.964).abs() < 0.05,
            "x(1 s) = {x1}, expected 431.964"
        );

        let v1 = sim.velocity(Duration::from_secs(1));
        assert!((v1 - 135.0).abs() < 0.05, "dx(1 s) = {v1}, expected 135");

        // The coast's asymptote, approached but never crossed.
        let far = sim.position(Duration::from_secs(30));
        assert!(
            (far - 499.381).abs() < 0.05,
            "finalX = {far}, expected 499.381"
        );
    }

    /// Flutter `_OverdampedSolution` with Teksilo's shipped spring: mass 0.5,
    /// stiffness 100, damping ratio 1.1.
    ///
    /// ```text
    ///   damping = 1.1 × 2 × √(0.5 × 100) = 15.556349
    ///   cmk     = damping² − 4·m·k = 242 − 200 = 42            (overdamped)
    ///   r1 = (−15.556349 − √42) / (2 × 0.5) = −22.037090
    ///   r2 = (−15.556349 + √42) / (2 × 0.5) =  −9.075609
    ///   for distance 50, velocity 0:
    ///     c2 = (0 − r1·50)/(r2 − r1) = 1101.8545 / 12.961481 = 85.00991
    ///     c1 = 50 − c2                                        = −35.00991
    ///   x(0.1) = c1·e^(0.1·r1) + c2·e^(0.1·r2)
    ///          = −35.00991 × 0.1103939 + 85.00991 × 0.4035080
    ///          = 30.437
    /// ```
    #[test]
    fn the_settle_spring_matches_flutters_overdamped_solution() {
        let t = tokens();
        let damping = t.spring_damping_ratio * 2.0 * (t.spring_mass * t.spring_stiffness).sqrt();
        assert!(
            (damping - 15.556_349).abs() < 1e-3,
            "damping = {damping}, expected 15.556349"
        );
        let cmk = damping * damping - 4.0 * t.spring_mass * t.spring_stiffness;
        assert!(
            cmk > 0.0,
            "the shipped ratio 1.1 must be overdamped, cmk={cmk}"
        );
        assert!((cmk - 42.0).abs() < 1e-2, "cmk = {cmk}, expected 42");

        // Released 50 dp past the trailing edge with no velocity.
        let sim = BouncingSimulation::within(150.0, 0.0, 0.0, 100.0);
        assert_eq!(sim.spring_time(), f32::NEG_INFINITY, "springs immediately");
        assert!(
            (sim.position(Duration::ZERO) - 150.0).abs() < 1e-3,
            "starts where released"
        );
        let x = sim.position(Duration::from_millis(100));
        assert!(
            (x - 130.437).abs() < 0.05,
            "x(0.1 s) = {x}, expected 100 + 30.437"
        );
    }

    /// Flutter `FrictionSimulation.timeAtX`:
    /// `t = ln(ln(drag)·(x − p)/v + 1) / ln(drag)`. With `p = 0`, `v = 1000`,
    /// `x = 100`: `t = ln(1 − 0.20024805)/−2.0024805 = 0.111600 s`.
    #[test]
    fn the_coast_hands_off_to_the_spring_at_the_boundary_crossing() {
        let sim = BouncingSimulation::within(0.0, 1000.0, -1.0e6, 100.0);
        let handoff = sim.spring_time();
        assert!(
            (handoff - 0.111_600).abs() < 1e-4,
            "handoff at {handoff} s, expected 0.111600"
        );
        // At the handoff the two phases agree on the position — that is what
        // makes the seam invisible.
        let at = sim.position(Duration::from_secs_f32(handoff));
        assert!((at - 100.0).abs() < 0.1, "position at handoff = {at}");
    }

    /// A bouncing fling that never reaches a bound is pure friction and
    /// declares itself done once it drops below the scroll velocity tolerance.
    #[test]
    fn an_in_range_bouncing_fling_settles_without_a_spring() {
        let sim = BouncingSimulation::within(0.0, 300.0, -1.0e6, 1.0e6);
        assert_eq!(sim.spring_time(), f32::INFINITY, "no boundary is crossed");
        assert!(!sim.is_done(Duration::ZERO), "300 dp/s is still moving");
        assert!(
            sim.is_done(Duration::from_secs(2)),
            "must be below {SETTLE_VELOCITY_TOLERANCE} dp/s after 2 s"
        );
    }

    /// The overshoot must come back. A fling into a bound overshoots, then the
    /// spring returns the content to rest at the bound.
    #[test]
    fn an_overscrolling_bouncing_fling_overshoots_then_returns() {
        let sim = BouncingSimulation::within(0.0, 2000.0, 0.0, 100.0);
        let peak = (0..200)
            .map(|i| sim.position(Duration::from_millis(i * 10)))
            .fold(f32::MIN, f32::max);
        assert!(peak > 100.0, "the content must overshoot, peaked at {peak}");

        let rest = sim.position(Duration::from_secs(3));
        assert!(
            (rest - 100.0).abs() < 1.0,
            "must come back to the bound, rested at {rest}"
        );
        assert!(sim.is_done(Duration::from_secs(3)));
    }

    /// A damping ratio just over 1 is chosen so the return never overshoots
    /// *back* past the bound — an oscillating scroll edge reads as a bug.
    #[test]
    fn the_return_does_not_oscillate_back_past_the_bound() {
        let sim = BouncingSimulation::within(150.0, 0.0, 0.0, 100.0);
        for i in 0..300 {
            let p = sim.position(Duration::from_millis(i * 10));
            assert!(
                p >= 100.0 - SETTLE_DISTANCE_TOLERANCE,
                "undershot to {p} at {} ms",
                i * 10
            );
        }
    }

    /// The rest target need not be the bound that was crossed — the reason
    /// `leading`/`trailing` are separate parameters from `min`/`max`.
    #[test]
    fn a_spring_can_rest_somewhere_other_than_the_crossed_bound() {
        // Crosses at 100, but snaps to rest at 120.
        let sim = BouncingSimulation::new(150.0, 0.0, 0.0, 100.0, 0.0, 120.0);
        let rest = sim.position(Duration::from_secs(3));
        assert!(
            (rest - 120.0).abs() < 0.5,
            "expected the snap target 120, rested at {rest}"
        );
    }

    // --- Rubber band ------------------------------------------------------

    /// `rubber_band(100, 400) = 400·(1 − 1/(0.52 × 100/400 + 1))
    ///                        = 400 × 0.13/1.13 = 46.0177`.
    #[test]
    fn the_rubber_band_matches_its_closed_form() {
        let y = rubber_band(100.0, 400.0);
        assert!((y - 46.0177).abs() < 1e-3, "got {y}, expected 46.0177");
        assert_eq!(rubber_band(-100.0, 400.0), -y, "the curve is odd");
    }

    /// The gain at the origin is Flutter's `frictionFactor(0) = 0.52` — the
    /// bridge between the two upstream statements of the same curve.
    #[test]
    fn the_initial_gain_is_the_flutter_friction_factor() {
        let extent = 800.0;
        let gain = rubber_band(0.01, extent) / 0.01;
        assert!(
            (gain - 0.52).abs() < 1e-3,
            "d(damped)/d(offset) at 0 = {gain}, expected 0.52"
        );
    }

    /// The `0.52·(1 − f)²` form must hold at every point of the curve, not just
    /// at the origin — that is the whole claim that the integral is right.
    #[test]
    fn the_gain_everywhere_is_the_flutter_friction_factor_of_the_current_fraction() {
        let extent = 500.0;
        for raw in [10.0f32, 60.0, 150.0, 400.0, 900.0] {
            // A forward difference measures the gain at the midpoint of the
            // step, so the friction factor is evaluated there too — otherwise
            // the O(h) truncation swamps the agreement being tested.
            let h = 0.5;
            let gain = (rubber_band(raw + h, extent) - rubber_band(raw, extent)) / h;
            let fraction = rubber_band(raw + h / 2.0, extent) / extent;
            let expected = 0.52 * (1.0 - fraction).powi(2);
            assert!(
                (gain - expected).abs() < 1e-3,
                "at raw={raw}: measured gain {gain}, frictionFactor {expected}"
            );
        }
    }

    #[test]
    fn the_rubber_band_is_the_identity_at_zero_and_never_reaches_the_extent() {
        assert_eq!(rubber_band(0.0, 400.0), 0.0);
        for raw in [1.0f32, 100.0, 10_000.0, 1.0e9] {
            let y = rubber_band(raw, 400.0);
            assert!(y < 400.0, "raw={raw} produced {y}, at or past the extent");
            assert!(y > 0.0);
        }
    }

    #[test]
    fn the_rubber_band_refuses_degenerate_inputs_without_producing_nan() {
        for (offset, extent) in [
            (f32::NAN, 400.0f32),
            (100.0, f32::NAN),
            (100.0, 0.0),
            (100.0, -5.0),
            (100.0, f32::INFINITY),
        ] {
            let y = rubber_band(offset, extent);
            assert_eq!(y, 0.0, "rubber_band({offset}, {extent}) = {y}");
        }
        assert_eq!(rubber_band(f32::INFINITY, 400.0), 400.0);
        assert_eq!(rubber_band(f32::NEG_INFINITY, 400.0), -400.0);
    }

    #[test]
    fn the_rubber_band_inverse_undoes_the_rubber_band() {
        let extent = 640.0;
        for raw in [0.0f32, 3.0, 55.0, 200.0, 1200.0] {
            let back = rubber_band_inverse(rubber_band(raw, extent), extent);
            assert!(
                (back - raw).abs() <= 1e-2 * raw.max(1.0),
                "raw={raw} round-tripped to {back}"
            );
        }
        assert_eq!(rubber_band_inverse(0.0, extent), 0.0);
        // Saturated input must stay finite rather than hitting the pole.
        assert!(rubber_band_inverse(extent, extent).is_finite());
        assert!(rubber_band_inverse(extent * 10.0, extent).is_finite());
    }
}
