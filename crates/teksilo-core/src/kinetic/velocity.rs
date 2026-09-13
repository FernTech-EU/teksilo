// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The velocity tracker: how fast was the pointer going when it lifted?
//!
//! A fling is only as good as the release velocity that seeds it, and the
//! naive answer — last position minus previous position, over the gap between
//! them — is the wrong one. It reads whatever jitter the final sample happened
//! to carry, and it reports a large velocity for a pointer that has been
//! sitting still for 200 ms and then twitched one pixel.
//!
//! So Teksilo uses the algorithm Android and Flutter both use: keep a short
//! ring of recent samples, throw away the ones that are too old or that sit on
//! the far side of a pause, and fit a **degree-2 least-squares polynomial**
//! through what is left. The linear coefficient of that fit is the velocity;
//! the fit's coefficient of determination is a confidence a consumer can use to
//! discount a noisy estimate.
//!
//! # The constants, and where they come from
//!
//! Every one is Flutter's `VelocityTracker`
//! (`packages/flutter/lib/src/gestures/velocity_tracker.dart`), which in turn
//! took them from Android's `VelocityTracker`/`LeastSquaresVelocityTrackerStrategy`:
//!
//! | constant | value | Flutter name |
//! | --- | --- | --- |
//! | [`HISTORY_SIZE`] | 20 samples | `_historySize` |
//! | [`HORIZON`] | 100 ms | `_horizonMilliseconds` |
//! | [`MIN_SAMPLE_SIZE`] | 3 samples | `_minSampleSize` |
//! | [`STOP_GAP`] | 40 ms | `_assumePointerMoveStoppedMilliseconds` |
//! | fit degree | 2 | `LeastSquaresSolver(...).solve(2)` |
//!
//! They are load-bearing rather than decorative. The 100 ms horizon is what
//! makes a long slow drag ending in a flick report the flick and not the drag.
//! The 40 ms gap is what makes a pointer that stopped, waited, then lifted
//! report *zero* rather than the velocity it had before the pause. The degree-2
//! fit is what lets a decelerating finger report the velocity it actually had
//! at release rather than its average over the window.
//!
//! # The gap rule
//!
//! Teksilo, like Flutter, implements "a 40 ms gap clears the history" by
//! **stopping the backwards walk** at the gap rather than by erasing storage.
//! The two are equivalent for every observable: samples on the far side of a
//! pause are never fitted across, and within 100 ms the horizon would have
//! dropped them anyway. Stopping the walk is strictly better, because a tracker
//! that erased its ring would also lose samples a *later* estimate could still
//! legitimately use.
//!
//! # Coalescing
//!
//! [`VelocityTracker::add_coalesced`] exists because a 1000 Hz digitizer's
//! samples arrive from the OS as a batch, and a backend that forwarded only the
//! batch's newest position would decimate the stream by a factor of sixteen at
//! 60 Hz. It adds every point in order, so the estimate is identical to the one
//! produced by adding the same points one at a time — which is exactly the
//! property its test asserts.
//!
//! Reference: `docs/kinetic-scrolling.md`.

use std::time::Duration;

use teksilo_canvas::{Point, Vec2};

use crate::pointer::EventTime;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// How many samples the ring holds. Flutter `_historySize`.
pub const HISTORY_SIZE: usize = 20;

/// Samples older than this are not fitted. Flutter `_horizonMilliseconds`.
pub const HORIZON: Duration = Duration::from_millis(100);

/// Below this many usable samples no estimate is produced at all. Flutter
/// `_minSampleSize`.
pub const MIN_SAMPLE_SIZE: usize = 3;

/// A gap at least this long between two consecutive samples means the pointer
/// stopped; the walk ends there rather than fitting across the pause. Flutter
/// `_assumePointerMoveStoppedMilliseconds`.
pub const STOP_GAP: Duration = Duration::from_millis(40);

/// The polynomial degree fitted through the samples. Flutter passes `2` to
/// `LeastSquaresSolver.solve`.
const DEGREE: usize = 2;

/// Number of polynomial coefficients — `DEGREE + 1`.
const N: usize = DEGREE + 1;

// ---------------------------------------------------------------------------
// Estimate
// ---------------------------------------------------------------------------

/// What [`VelocityTracker::estimate`] produces.
///
/// `#[non_exhaustive]` so a later field (Flutter also carries the offset
/// covered by the fitted window) cannot break a `match` or a struct literal at
/// a call site.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct VelocityEstimate {
    /// The fitted velocity, in logical pixels per second, on both axes.
    pub pixels_per_second: Vec2,
    /// The fit's coefficient of determination (R²), `0.0..=1.0` for a sane
    /// fit. `1.0` means the samples lie exactly on the fitted curve. A
    /// consumer that wants to refuse a noisy flick compares against this
    /// rather than re-deriving one.
    pub confidence: f32,
    /// How much time the fitted window spans — newest sample minus oldest
    /// *used* sample, which is at most [`HORIZON`].
    pub duration: Duration,
}

// ---------------------------------------------------------------------------
// The tracker
// ---------------------------------------------------------------------------

/// One sample in the ring.
#[derive(Copy, Clone, Debug)]
struct Sample {
    time: EventTime,
    position: Point,
}

/// A ring of recent pointer positions that answers "how fast, right now?".
///
/// One per gesture (or per [`crate::pointer::PointerId`]) — a tracker fed from
/// two contacts reports nonsense. [`clear`](Self::clear) resets it for reuse.
///
/// ```
/// use std::time::Duration;
/// use teksilo_canvas::Point;
/// use teksilo_core::kinetic::VelocityTracker;
/// use teksilo_core::pointer::EventTime;
///
/// let mut t = VelocityTracker::new();
/// // 600 dp/s downward, sampled at 100 Hz.
/// for i in 0..8 {
///     t.add(EventTime::from_millis(i * 10), Point::new(0.0, i as f32 * 6.0));
/// }
/// let v = t.estimate().expect("eight samples inside the horizon");
/// assert!((v.pixels_per_second.y - 600.0).abs() < 6.0);
/// ```
#[derive(Clone, Debug)]
pub struct VelocityTracker {
    /// The ring. `samples[index]` is always the newest.
    samples: [Option<Sample>; HISTORY_SIZE],
    /// Index of the newest sample. Incremented **before** each write, exactly
    /// as Flutter's `_index` is, so a fresh tracker's first write lands at 1.
    index: usize,
}

impl Default for VelocityTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl VelocityTracker {
    /// An empty tracker.
    pub const fn new() -> Self {
        Self {
            samples: [None; HISTORY_SIZE],
            index: 0,
        }
    }

    /// Record one position.
    ///
    /// Samples may arrive slightly out of order (a coalesced packet whose
    /// timestamps predate the last one processed); the walk uses
    /// [`EventTime::saturating_since`] in both directions, so an inverted pair
    /// reads as "no time passed" rather than underflowing.
    pub fn add(&mut self, time: EventTime, position: Point) {
        self.index += 1;
        if self.index == HISTORY_SIZE {
            self.index = 0;
        }
        self.samples[self.index] = Some(Sample { time, position });
    }

    /// Record a whole batch of coalesced positions, oldest first.
    ///
    /// Equivalent to calling [`add`](Self::add) once per element — which is
    /// the point: a 1000 Hz pen's batch must not be reduced to its newest
    /// point before the fit sees it.
    pub fn add_coalesced(&mut self, samples: &[(EventTime, Point)]) {
        for &(time, position) in samples {
            self.add(time, position);
        }
    }

    /// Forget every sample.
    pub fn clear(&mut self) {
        self.samples = [None; HISTORY_SIZE];
        self.index = 0;
    }

    /// The fitted release velocity, or `None`.
    ///
    /// `None` means the tracker cannot honestly answer: fewer than
    /// [`MIN_SAMPLE_SIZE`] samples survive the horizon and the stop-gap walk,
    /// or the fit is degenerate (every sample at the same instant). A caller
    /// that wants a number regardless uses [`velocity`](Self::velocity).
    pub fn estimate(&self) -> Option<VelocityEstimate> {
        let newest = self.samples[self.index]?;

        // Walk backwards from the newest sample for as long as the samples
        // represent continuous motion. `time` holds negative ages in
        // milliseconds, matching Flutter's `time.add(-age)`.
        let mut xs = [0.0f64; HISTORY_SIZE];
        let mut ys = [0.0f64; HISTORY_SIZE];
        let mut ws = [0.0f64; HISTORY_SIZE];
        let mut ts = [0.0f64; HISTORY_SIZE];
        let mut count = 0usize;

        let mut index = self.index;
        let mut previous = newest;
        let mut oldest = newest;

        while count < HISTORY_SIZE {
            let Some(sample) = self.samples[index] else {
                break;
            };

            let age = newest.time.saturating_since(sample.time);
            // Absolute gap: samples can arrive out of order.
            let gap = if sample.time >= previous.time {
                sample.time.saturating_since(previous.time)
            } else {
                previous.time.saturating_since(sample.time)
            };
            previous = sample;
            if age > HORIZON || gap > STOP_GAP {
                break;
            }

            oldest = sample;
            xs[count] = f64::from(sample.position.x);
            ys[count] = f64::from(sample.position.y);
            ws[count] = 1.0;
            ts[count] = -(age.as_secs_f64() * 1000.0);

            index = if index == 0 { HISTORY_SIZE } else { index } - 1;
            count += 1;
        }

        if count < MIN_SAMPLE_SIZE {
            return None;
        }

        let fx = least_squares_fit(&ts[..count], &xs[..count], &ws[..count])?;
        let fy = least_squares_fit(&ts[..count], &ys[..count], &ws[..count])?;

        // The fit's independent variable is milliseconds, so the linear
        // coefficient is px/ms; ×1000 makes it px/s. Flutter does exactly
        // this at the `VelocityEstimate` construction site.
        Some(VelocityEstimate {
            pixels_per_second: Vec2::new(
                (fx.coefficients[1] * 1000.0) as f32,
                (fy.coefficients[1] * 1000.0) as f32,
            ),
            confidence: (fx.confidence * fy.confidence) as f32,
            duration: newest.time.saturating_since(oldest.time),
        })
    }

    /// The fitted velocity, or [`Vec2::ZERO`] when there is no estimate.
    ///
    /// The shape a fling site wants: "no usable samples" and "the pointer was
    /// not moving" both mean *do not fling*, and both should read as zero.
    pub fn velocity(&self) -> Vec2 {
        self.estimate()
            .map(|e| e.pixels_per_second)
            .unwrap_or(Vec2::ZERO)
    }

    /// How many samples the ring currently holds (test/diagnostic helper).
    pub fn sample_count(&self) -> usize {
        self.samples.iter().filter(|s| s.is_some()).count()
    }
}

// ---------------------------------------------------------------------------
// Least squares
// ---------------------------------------------------------------------------

/// The result of a weighted polynomial fit.
struct PolynomialFit {
    /// `coefficients[k]` multiplies `x^k`.
    coefficients: [f64; N],
    /// Coefficient of determination, `1 - SSerr / SStot`.
    confidence: f64,
}

/// Weighted least-squares polynomial fit of [`DEGREE`], by QR decomposition
/// via the Gram–Schmidt process.
///
/// A direct port of Flutter's `LeastSquaresSolver.solve`
/// (`packages/flutter/lib/src/gestures/lsq_solver.dart`), which is itself a
/// port of Android's `LeastSquaresVelocityTrackerStrategy`. Two deliberate
/// deviations, both documented at their site:
///
/// - the arithmetic is `f64` rather than `f32`, because a degree-2 Vandermonde
///   matrix built from millisecond-scale abscissae is ill-conditioned enough
///   that single precision visibly moves the answer;
/// - a singular `R` diagonal returns `None` instead of dividing, so a
///   degenerate window can never hand a NaN velocity to the physics.
fn least_squares_fit(x: &[f64], y: &[f64], w: &[f64]) -> Option<PolynomialFit> {
    let m = x.len();
    debug_assert!(m == y.len() && m == w.len());
    // Flutter: `if (degree > x.length) return null` — not enough data to fit.
    // The upper bound is this file's own: the scratch matrices are fixed-size,
    // so a caller cannot ask for a fit wider than the ring.
    if !(N..=HISTORY_SIZE).contains(&m) {
        return None;
    }

    // Expand X into the weighted Vandermonde matrix A (n rows of m columns).
    let mut a = [[0.0f64; HISTORY_SIZE]; N];
    for h in 0..m {
        a[0][h] = w[h];
        for i in 1..N {
            a[i][h] = a[i - 1][h] * x[h];
        }
    }

    // Gram–Schmidt: A = Q R.
    let mut q = [[0.0f64; HISTORY_SIZE]; N];
    let mut r = [[0.0f64; N]; N];
    for j in 0..N {
        q[j] = a[j];
        for i in 0..j {
            let qi = q[i];
            let qj = q[j];
            let dot: f64 = (0..m).map(|h| qj[h] * qi[h]).sum();
            for h in 0..m {
                q[j][h] -= dot * qi[h];
            }
        }

        let norm = (0..m).map(|h| q[j][h] * q[j][h]).sum::<f64>().sqrt();
        // Linearly dependent or zero rows — no solution. The explicit NaN
        // arm matters: every comparison against NaN is false, so a bare
        // `norm < 1e-6` would let a NaN norm through and hand a NaN velocity
        // to the physics.
        if norm.is_nan() || norm < 1e-6 {
            return None;
        }

        let inverse_norm = 1.0 / norm;
        for value in q[j].iter_mut().take(m) {
            *value *= inverse_norm;
        }
        for i in 0..N {
            r[j][i] = if i < j {
                0.0
            } else {
                let qj = q[j];
                let ai = a[i];
                (0..m).map(|h| qj[h] * ai[h]).sum()
            };
        }
    }

    // Solve R B = Qᵀ W Y by back substitution.
    let mut wy = [0.0f64; HISTORY_SIZE];
    for h in 0..m {
        wy[h] = y[h] * w[h];
    }

    let mut coefficients = [0.0f64; N];
    for i in (0..N).rev() {
        let qi = q[i];
        let mut c: f64 = (0..m).map(|h| qi[h] * wy[h]).sum();
        for j in (i + 1..N).rev() {
            c -= r[i][j] * coefficients[j];
        }
        // Singular diagonal; see the deviation note in the doc comment. The
        // NaN arm is explicit for the same reason as the norm check above.
        let diagonal = r[i][i];
        if diagonal.is_nan() || diagonal.abs() < 1e-12 {
            return None;
        }
        coefficients[i] = c / diagonal;
    }

    // Confidence = 1 - SSerr / SStot.
    let y_mean = y.iter().sum::<f64>() / m as f64;
    let mut sum_squared_error = 0.0f64;
    let mut sum_squared_total = 0.0f64;
    for h in 0..m {
        let mut term = 1.0f64;
        let mut err = y[h] - coefficients[0];
        for c in coefficients.iter().take(N).skip(1) {
            term *= x[h];
            err -= term * c;
        }
        sum_squared_error += w[h] * w[h] * err * err;
        let v = y[h] - y_mean;
        sum_squared_total += w[h] * w[h] * v * v;
    }
    let confidence = if sum_squared_total <= 1e-6 {
        1.0
    } else {
        1.0 - (sum_squared_error / sum_squared_total)
    };

    if !coefficients.iter().all(|c| c.is_finite()) {
        return None;
    }

    Some(PolynomialFit {
        coefficients,
        confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed `count` samples at `hz` moving at `velocity` dp/s on y.
    fn constant_velocity(velocity: f32, hz: u64, count: u64) -> VelocityTracker {
        let mut t = VelocityTracker::new();
        let step_ms = 1000 / hz;
        for i in 0..count {
            let seconds = (i * step_ms) as f32 / 1000.0;
            t.add(
                EventTime::from_millis(i * step_ms),
                Point::new(0.0, velocity * seconds),
            );
        }
        t
    }

    /// The headline accuracy claim: a clean constant-velocity stream is
    /// estimated within 1 %.
    #[test]
    fn a_constant_velocity_stream_is_estimated_within_one_percent() {
        for v in [120.0f32, 600.0, 2400.0, -1800.0] {
            let t = constant_velocity(v, 100, 8);
            let e = t.estimate().expect("eight samples inside the horizon");
            let error = (e.pixels_per_second.y - v).abs() / v.abs();
            assert!(
                error < 0.01,
                "v={v}: estimated {} ({}% off)",
                e.pixels_per_second.y,
                error * 100.0
            );
            assert!(
                e.pixels_per_second.x.abs() < 1.0,
                "a y-only stream must report ~0 on x, got {}",
                e.pixels_per_second.x
            );
        }
    }

    /// An exact straight line fits exactly, so the coefficient of
    /// determination is 1.
    #[test]
    fn a_straight_line_fits_with_full_confidence() {
        let t = constant_velocity(500.0, 100, 6);
        let e = t.estimate().unwrap();
        assert!(
            (e.confidence - 1.0).abs() < 1e-3,
            "confidence {} for an exact line",
            e.confidence
        );
    }

    #[test]
    fn fewer_than_three_samples_yields_none() {
        let mut t = VelocityTracker::new();
        assert_eq!(t.estimate(), None, "an empty tracker has no estimate");

        t.add(EventTime::from_millis(0), Point::new(0.0, 0.0));
        assert_eq!(t.estimate(), None, "one sample is not an estimate");

        t.add(EventTime::from_millis(10), Point::new(0.0, 6.0));
        assert_eq!(t.estimate(), None, "two samples are not an estimate");

        t.add(EventTime::from_millis(20), Point::new(0.0, 12.0));
        assert!(t.estimate().is_some(), "three samples are the minimum");
    }

    /// A pause longer than [`STOP_GAP`] means the pointer stopped. Everything
    /// before the pause is excluded, so two post-pause samples leave the
    /// tracker below the minimum and it reports nothing.
    #[test]
    fn a_forty_millisecond_gap_clears_the_history() {
        let mut t = VelocityTracker::new();
        for i in 0..6 {
            t.add(
                EventTime::from_millis(i * 5),
                Point::new(0.0, i as f32 * 30.0),
            );
        }
        assert!(t.estimate().is_some(), "six dense samples estimate fine");

        // 50 ms of nothing, then two more samples.
        t.add(EventTime::from_millis(75), Point::new(0.0, 150.0));
        t.add(EventTime::from_millis(80), Point::new(0.0, 150.0));
        assert_eq!(
            t.estimate(),
            None,
            "the pre-gap samples must not be fitted across"
        );
    }

    /// The pause rule must not fire on a gap that is merely long-ish: 39 ms is
    /// still continuous motion.
    #[test]
    fn a_gap_under_the_stop_threshold_keeps_the_history() {
        let mut t = VelocityTracker::new();
        t.add(EventTime::from_millis(0), Point::new(0.0, 0.0));
        t.add(EventTime::from_millis(39), Point::new(0.0, 39.0));
        t.add(EventTime::from_millis(78), Point::new(0.0, 78.0));
        let e = t.estimate().expect("39 ms gaps are continuous motion");
        assert!(
            (e.pixels_per_second.y - 1000.0).abs() < 10.0,
            "got {}",
            e.pixels_per_second.y
        );
    }

    /// Samples older than the 100 ms horizon are dropped even without a gap.
    #[test]
    fn the_horizon_drops_stale_samples() {
        let mut t = VelocityTracker::new();
        // 30 samples at 200 Hz spans 145 ms — more than the horizon.
        for i in 0..30u64 {
            t.add(
                EventTime::from_millis(i * 5),
                Point::new(0.0, i as f32 * 5.0),
            );
        }
        let e = t.estimate().unwrap();
        assert!(
            e.duration <= HORIZON,
            "the fitted window spans {:?}, past the {HORIZON:?} horizon",
            e.duration
        );
    }

    /// The reason `add_coalesced` exists: a batch must produce the same answer
    /// as the same points delivered singly, so a dense pen stream is never
    /// decimated by the delivery path.
    #[test]
    fn add_coalesced_matches_adding_one_at_a_time() {
        // 1000 Hz for 12 ms — the shape a digitizer batch arrives in.
        let batch: Vec<(EventTime, Point)> = (0..12u64)
            .map(|i| {
                (
                    EventTime::from_millis(i),
                    Point::new(i as f32 * 1.5, i as f32 * -2.5),
                )
            })
            .collect();

        let mut one_at_a_time = VelocityTracker::new();
        for &(time, position) in &batch {
            one_at_a_time.add(time, position);
        }

        let mut coalesced = VelocityTracker::new();
        coalesced.add_coalesced(&batch);

        assert_eq!(
            coalesced.estimate(),
            one_at_a_time.estimate(),
            "a coalesced batch must not be decimated"
        );
    }

    #[test]
    fn clear_forgets_everything() {
        let mut t = constant_velocity(500.0, 100, 6);
        assert!(t.estimate().is_some());
        t.clear();
        assert_eq!(t.estimate(), None);
        assert_eq!(t.sample_count(), 0);
    }

    /// `velocity()` is the "give me a number" shape: no estimate reads as
    /// zero, which at a fling site means *do not fling*.
    #[test]
    fn velocity_reports_zero_when_there_is_no_estimate() {
        let t = VelocityTracker::new();
        assert_eq!(t.velocity(), Vec2::ZERO);
    }

    /// The ring holds 20; a longer stream keeps the newest 20 and still
    /// answers.
    #[test]
    fn the_ring_wraps_without_losing_the_newest_samples() {
        let mut t = VelocityTracker::new();
        for i in 0..60u64 {
            t.add(
                EventTime::from_millis(i * 4),
                Point::new(0.0, i as f32 * 4.0),
            );
        }
        assert_eq!(t.sample_count(), HISTORY_SIZE);
        let e = t.estimate().unwrap();
        assert!(
            (e.pixels_per_second.y - 1000.0).abs() < 10.0,
            "got {}",
            e.pixels_per_second.y
        );
    }

    /// Every sample at the same instant is a degenerate fit; the solver must
    /// decline rather than produce a NaN or an infinite velocity.
    #[test]
    fn a_degenerate_window_declines_rather_than_returning_nan() {
        let mut t = VelocityTracker::new();
        for i in 0..6 {
            t.add(EventTime::ZERO, Point::new(i as f32, 0.0));
        }
        assert_eq!(t.estimate(), None);
        assert_eq!(t.velocity(), Vec2::ZERO);
    }

    /// A decelerating finger should report the velocity it had *at release*,
    /// not its average over the window — which is precisely what the degree-2
    /// term buys over a straight two-point difference.
    #[test]
    fn a_decelerating_stream_reports_the_release_velocity() {
        // y(t) = 1000t - 2000t², so dy/dt at t = 0 is 1000 and at t = 0.05 s
        // it is 800. Samples run 0..50 ms and the fit is anchored at the
        // newest sample, so the reported velocity is the 800 end.
        let mut t = VelocityTracker::new();
        for i in 0..6u64 {
            let s = i as f32 * 0.01;
            t.add(
                EventTime::from_millis(i * 10),
                Point::new(0.0, 1000.0 * s - 2000.0 * s * s),
            );
        }
        let e = t.estimate().unwrap();
        assert!(
            (e.pixels_per_second.y - 800.0).abs() < 8.0,
            "expected the release velocity 800, got {}",
            e.pixels_per_second.y
        );
    }

    /// The velocity-tracker table in `docs/kinetic-scrolling.md` §2 is this
    /// module's constants.
    ///
    /// Before this, `HISTORY_SIZE` and `HORIZON` were used symbolically by the
    /// tests around it and their published values — 20 samples, 100 ms — were
    /// asserted nowhere, so retuning either would have left the document
    /// describing a tracker that no longer existed. `DEGREE` is private, which
    /// is why the check lives here rather than in `tests/`.
    #[test]
    fn the_documented_velocity_constants_are_the_shipped_ones() {
        const PAGE: &str = "kinetic-scrolling.md";
        let rows = crate::kinetic::doc_table::rows(PAGE, "### Velocity tracker");
        let mut seen = Vec::new();
        for row in &rows {
            let key = row[0].trim_matches('`').to_string();
            let cell = &row[1];
            match key.as_str() {
                "HISTORY_SIZE" => {
                    crate::kinetic::doc_table::assert_value(PAGE, cell, HISTORY_SIZE as f64, &key)
                }
                "HORIZON" => crate::kinetic::doc_table::assert_value(
                    PAGE,
                    cell,
                    HORIZON.as_millis() as f64,
                    &key,
                ),
                "MIN_SAMPLE_SIZE" => crate::kinetic::doc_table::assert_value(
                    PAGE,
                    cell,
                    MIN_SAMPLE_SIZE as f64,
                    &key,
                ),
                "STOP_GAP" => crate::kinetic::doc_table::assert_value(
                    PAGE,
                    cell,
                    STOP_GAP.as_millis() as f64,
                    &key,
                ),
                "fit degree" => {
                    crate::kinetic::doc_table::assert_value(PAGE, cell, DEGREE as f64, &key)
                }
                other => panic!(
                    "docs/kinetic-scrolling.md publishes a velocity row {other:?} that \
                     this test does not check"
                ),
            }
            seen.push(key);
        }
        assert_eq!(
            seen.len(),
            5,
            "the velocity table lost a row: {seen:?}. The prose below it says \
             \"Flutter took all five from Android\"."
        );
    }
}
