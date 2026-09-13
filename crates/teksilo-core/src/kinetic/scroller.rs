// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The two driver types: one scrollable's physics, and the tree's fling pump.
//!
//! [`KineticScroller`] is what a scrollable surface owns. It holds the offset,
//! the range, a [`VelocityTracker`], and at most one running simulation, and it
//! answers three questions: where does a drag put the content
//! ([`pan`](KineticScroller::pan)), does a release deserve a coast
//! ([`fling`](KineticScroller::fling)), and where is the coast now
//! ([`tick`](KineticScroller::tick)).
//!
//! [`FlingDriver`] is the tree-level pump for the *other* fling shape: a coast
//! that is re-dispatched as scroll deltas so it can chain outward when the
//! target reaches its end. It keeps one simulation pair per target widget and
//! hands out per-tick deltas.
//!
//! Neither type reads a clock. Time arrives as an [`EventTime`] from the tree's
//! one [`InputClock`](crate::pointer::clock::InputClock), which is what lets a
//! whole fling be replayed deterministically in a headless test.
//!
//! # Reduced motion
//!
//! `prefers-reduced-motion` is not advisory here. With it on:
//!
//! - a fling **does not coast** — it collapses to a settle that completes on
//!   the very next [`tick`](KineticScroller::tick), so the content arrives at a
//!   legal offset in one step rather than sliding;
//! - the rubber band is **hard-clamped** — the content stops at the boundary
//!   instead of following the finger past it and springing back;
//! - [`FlingDriver::start`] is a no-op, so nothing is re-dispatched.
//!
//! Reference: `docs/kinetic-scrolling.md`.

use std::time::Duration;

use teksilo_canvas::{Point, Vec2};
use teksilo_tokens::{GestureProfile, OverscrollStyle, ScrollPhysics, ScrollPhysicsTokens};

use crate::frame_tick_scheduler::{FrameTickScheduler, FrameTickSubscription};
use crate::overscroll::SCROLL_MOVE_EPSILON;
use crate::pointer::{EventTime, ScrollPhase};
use crate::widget_id::WidgetId;

use super::simulation::{
    BouncingSimulation, ClampingSimulation, ScrollSimulation, rubber_band_inverse_with,
    rubber_band_with,
};
use super::velocity::VelocityTracker;

/// The tick cadence a running simulation asks for — 60 Hz, the same
/// `16_667 µs` the [`FrameTickScheduler`] and the animation schedulers use, so
/// a fling folds into the existing wake budget rather than adding a cadence of
/// its own.
pub const FLING_FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

/// The viewport extent a scroller assumes before anyone tells it otherwise.
///
/// Only the rubber band reads it (the resistance curve is a fraction of the
/// viewport). A caller that never calls
/// [`set_viewport`](KineticScroller::set_viewport) and never selects
/// [`OverscrollStyle::RubberBand`] is unaffected by this number.
pub const DEFAULT_VIEWPORT_EXTENT: f32 = 400.0;

// ---------------------------------------------------------------------------
// ScrollStep
// ---------------------------------------------------------------------------

/// What one pan or one tick did to the content.
///
/// `#[non_exhaustive]`: a later field (a phase, a settled flag) must not break
/// a destructuring call site.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScrollStep {
    /// The scroll offset, already clamped into the scroller's range.
    pub offset: Point,
    /// How far past the range the content is being held, per axis, **after**
    /// the rubber band. Always zero under [`OverscrollStyle::Clamp`] and under
    /// reduced motion.
    pub overscroll: Vec2,
    /// Whether each axis `(x, y)` took any of the requested movement.
    ///
    /// This is the boundary hand-off signal: an axis that absorbed nothing has
    /// nothing left to give, so the event belongs to an ancestor scrollable.
    /// An axis that was asked for nothing reports `false` — a zero delta is not
    /// a claim.
    pub absorbed: (bool, bool),
}

impl ScrollStep {
    /// Whether either axis took any of the movement.
    pub fn absorbed_any(&self) -> bool {
        self.absorbed.0 || self.absorbed.1
    }
}

// ---------------------------------------------------------------------------
// KineticScroller
// ---------------------------------------------------------------------------

/// A running coast, or the one-step settle that reduced motion substitutes for
/// it.
#[derive(Debug)]
enum Animation {
    /// Reduced motion: clamp into range on the next tick and stop.
    Settle,
    /// A real coast, one simulation per axis.
    Fling {
        start: EventTime,
        x: Option<Box<dyn ScrollSimulation>>,
        y: Option<Box<dyn ScrollSimulation>>,
    },
}

/// One scrollable surface's physics: offset, range, velocity and coast.
///
/// The offset it reports is *visual*: in range it is the scroll offset, and
/// out of range it is the boundary plus a rubber-banded overscroll, which
/// [`ScrollStep`] splits into its two fields so a consumer can scroll the
/// content and translate the overscroll indicator separately.
///
/// ```
/// use teksilo_canvas::{Point, Vec2};
/// use teksilo_core::kinetic::KineticScroller;
/// use teksilo_core::pointer::EventTime;
/// use teksilo_tokens::{GestureProfile, OverscrollStyle};
///
/// let mut s = KineticScroller::new(OverscrollStyle::Clamp);
/// s.set_range(0.0, 500.0);
///
/// let step = s.pan(EventTime::from_millis(0), Point::new(0.0, 0.0), Vec2::new(0.0, 40.0));
/// assert_eq!(step.offset.y, 40.0);
/// assert!(step.absorbed.1);
///
/// // A release below the profile's floor is not a fling.
/// assert!(!s.fling(Vec2::new(0.0, 10.0), &GestureProfile::TOUCH));
/// assert!(!s.is_animating());
/// ```
#[derive(Debug)]
pub struct KineticScroller {
    style: OverscrollStyle,
    tokens: ScrollPhysicsTokens,
    /// Visual content offset — boundary plus overscroll when out of range.
    position: Point,
    range_x: (f32, f32),
    range_y: (f32, f32),
    viewport: Vec2,
    reduced_motion: bool,
    /// The host backend delivers its own momentum stream (macOS / a precision
    /// trackpad), so Teksilo must not add one.
    os_momentum: bool,
    tracker: VelocityTracker,
    animation: Option<Animation>,
    /// Last time anything moved — the base for [`next_deadline`] and the
    /// implicit release time of [`fling`].
    ///
    /// [`next_deadline`]: Self::next_deadline
    /// [`fling`]: Self::fling
    last_time: EventTime,
}

impl KineticScroller {
    /// A scroller with the shipped physics tokens and the given overscroll
    /// style.
    ///
    /// The style also picks the fling simulation, because the two are one
    /// decision: [`Clamp`](OverscrollStyle::Clamp) means Android's
    /// [`ClampingSimulation`] and a hard boundary,
    /// [`RubberBand`](OverscrollStyle::RubberBand) means Flutter's
    /// [`BouncingSimulation`] and a spring. A surface that clamped on drag but
    /// bounced on fling would read as two different scrollables.
    pub fn new(style: OverscrollStyle) -> Self {
        Self::with_tokens(style, &ScrollPhysicsTokens::DEFAULT)
    }

    /// [`new`](Self::new) against a theme's own physics tokens.
    pub fn with_tokens(style: OverscrollStyle, tokens: &ScrollPhysicsTokens) -> Self {
        Self {
            style,
            tokens: *tokens,
            position: Point::ZERO,
            range_x: (0.0, 0.0),
            range_y: (0.0, 0.0),
            viewport: Vec2::new(DEFAULT_VIEWPORT_EXTENT, DEFAULT_VIEWPORT_EXTENT),
            reduced_motion: false,
            os_momentum: false,
            tracker: VelocityTracker::new(),
            animation: None,
            last_time: EventTime::ZERO,
        }
    }

    // --- configuration ---------------------------------------------------

    /// Set the scrollable range on **both** axes.
    ///
    /// A surface that scrolls on one axis only gives the other a zero-width
    /// range (`min == max`), which pins it. Per-axis ranges go through
    /// [`set_range_x`](Self::set_range_x) / [`set_range_y`](Self::set_range_y).
    pub fn set_range(&mut self, min: f32, max: f32) {
        self.set_range_x(min, max);
        self.set_range_y(min, max);
    }

    /// Set the horizontal range.
    pub fn set_range_x(&mut self, min: f32, max: f32) {
        self.range_x = normalize(min, max);
        self.position.x = self.position.x.clamp(self.range_x.0, self.range_x.1);
    }

    /// Set the vertical range.
    pub fn set_range_y(&mut self, min: f32, max: f32) {
        self.range_y = normalize(min, max);
        self.position.y = self.position.y.clamp(self.range_y.0, self.range_y.1);
    }

    /// The viewport size, which sets how much the rubber band resists.
    ///
    /// Only read under [`OverscrollStyle::RubberBand`]; the curve is expressed
    /// as a fraction of the viewport, so a tall list resists over a longer
    /// travel than a short one, exactly as iOS does.
    pub fn set_viewport(&mut self, size: Vec2) {
        self.viewport = Vec2::new(
            size.x.max(SCROLL_MOVE_EPSILON),
            size.y.max(SCROLL_MOVE_EPSILON),
        );
    }

    /// Turn `prefers-reduced-motion` on or off. See the module docs for
    /// exactly what it changes.
    ///
    /// Turning it on mid-coast collapses the coast to a settle rather than
    /// letting it play out.
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
        if reduced && matches!(self.animation, Some(Animation::Fling { .. })) {
            self.animation = Some(Animation::Settle);
        }
    }

    /// Whether reduced motion is on.
    pub fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    /// Declare that the host backend delivers its own momentum stream.
    ///
    /// This is the macOS switch, and the reason
    /// [`fling_for_phase`](Self::fling_for_phase) exists — see its docs.
    /// A backend sets it from its `reports_os_momentum` capability.
    pub fn set_os_momentum(&mut self, os_momentum: bool) {
        self.os_momentum = os_momentum;
    }

    /// Whether the host backend delivers its own momentum stream.
    pub fn os_momentum(&self) -> bool {
        self.os_momentum
    }

    /// The current visual offset, clamped into range.
    pub fn offset(&self) -> Point {
        Point::new(
            self.position.x.clamp(self.range_x.0, self.range_x.1),
            self.position.y.clamp(self.range_y.0, self.range_y.1),
        )
    }

    /// Place the content at `offset` without animating. Clamps into range.
    pub fn set_offset(&mut self, offset: Point) {
        self.position = Point::new(
            offset.x.clamp(self.range_x.0, self.range_x.1),
            offset.y.clamp(self.range_y.0, self.range_y.1),
        );
    }

    /// The velocity tracker fed by [`pan`](Self::pan), for a caller that wants
    /// to seed its own fling.
    ///
    /// It follows the **pointer**, not the content. A surface that drags its
    /// content in the opposite direction to the finger (the usual arrangement:
    /// finger up, offset down) negates before passing it to
    /// [`fling`](Self::fling), which takes its velocity in offset space.
    pub fn pointer_velocity(&self) -> Vec2 {
        self.tracker.velocity()
    }

    // --- dragging --------------------------------------------------------

    /// Apply one drag step.
    ///
    /// `position` is the pointer, recorded for the velocity estimate;
    /// `delta` is the requested change in **scroll offset** (positive = the
    /// offset grows), which the caller has already derived from the pointer
    /// motion under its own sign convention.
    ///
    /// A pan cancels any running coast — the user has grabbed the content.
    pub fn pan(&mut self, time: EventTime, position: Point, delta: Vec2) -> ScrollStep {
        self.tracker.add(time, position);
        self.last_time = time;
        self.animation = None;

        let before = self.position;
        self.position.x = self.drag_axis(self.position.x, delta.x, self.range_x, self.viewport.x);
        self.position.y = self.drag_axis(self.position.y, delta.y, self.range_y, self.viewport.y);

        self.step_from(before)
    }

    /// One axis of a drag, in visual space.
    ///
    /// Under a hard clamp this is a clamp. Under the rubber band it is a
    /// three-step round trip: recover how far the finger had already travelled
    /// past the edge (the inverse curve), add this step's raw travel, and
    /// re-damp. Going through the raw travel rather than damping the damped
    /// value is what makes the resistance a function of total travel, which is
    /// what makes a drag that reverses retrace its own curve instead of
    /// jumping.
    fn drag_axis(&self, current: f32, delta: f32, range: (f32, f32), extent: f32) -> f32 {
        if !delta.is_finite() {
            return current;
        }
        let (min, max) = range;
        let clamped = current.clamp(min, max);

        if self.hard_clamps() {
            return (clamped + delta).clamp(min, max);
        }

        let factor = self.tokens.rubber_band_factor;
        let raw_before = rubber_band_inverse_with(current - clamped, extent, factor);
        let virtual_position = clamped + raw_before + delta;

        if virtual_position < min {
            min + rubber_band_with(virtual_position - min, extent, factor)
        } else if virtual_position > max {
            max + rubber_band_with(virtual_position - max, extent, factor)
        } else {
            virtual_position
        }
    }

    /// Whether overscroll is refused outright — either because the surface
    /// clamps, or because reduced motion has turned the bounce off.
    fn hard_clamps(&self) -> bool {
        self.reduced_motion || self.style == OverscrollStyle::Clamp
    }

    // --- flinging --------------------------------------------------------

    /// Start a coast from a release velocity, in **scroll-offset space**.
    ///
    /// Returns `false` and changes nothing when the release is below
    /// [`GestureProfile::min_fling_velocity`] — that is the whole point of the
    /// floor, so a slow release settles where the finger left it rather than
    /// drifting. A release above
    /// [`GestureProfile::max_fling_velocity`] is scaled down to it, direction
    /// preserved.
    ///
    /// The release instant is the last time this scroller saw a
    /// [`pan`](Self::pan); use [`fling_at`](Self::fling_at) to state it
    /// explicitly.
    ///
    /// **On a host that delivers its own momentum**, call
    /// [`fling_for_phase`](Self::fling_for_phase) instead — see its docs.
    pub fn fling(&mut self, velocity: Vec2, profile: &GestureProfile) -> bool {
        self.fling_at(self.last_time, velocity, profile)
    }

    /// [`fling`](Self::fling) with an explicit release instant.
    pub fn fling_at(&mut self, time: EventTime, velocity: Vec2, profile: &GestureProfile) -> bool {
        let magnitude = (velocity.x * velocity.x + velocity.y * velocity.y).sqrt();
        if !magnitude.is_finite() || magnitude < profile.min_fling_velocity {
            return false;
        }

        let velocity = if magnitude > profile.max_fling_velocity && magnitude > 0.0 {
            let scale = profile.max_fling_velocity / magnitude;
            Vec2::new(velocity.x * scale, velocity.y * scale)
        } else {
            velocity
        };

        self.last_time = time;

        if self.reduced_motion {
            // No coast: the next tick puts the content at a legal offset and
            // the animation is over.
            self.animation = Some(Animation::Settle);
            return true;
        }

        self.animation = Some(Animation::Fling {
            start: time,
            x: self.axis_simulation(self.position.x, velocity.x, self.range_x),
            y: self.axis_simulation(self.position.y, velocity.y, self.range_y),
        });
        true
    }

    /// Whether a release at `phase` should start a Teksilo fling.
    ///
    /// **This is the macOS double-momentum guard.** macOS (and a precision
    /// trackpad on any host that forwards its phases) simulates the coast
    /// itself and delivers it as a stream of
    /// [`ScrollPhase::Momentum`] deltas. Running a Teksilo fling on top of that
    /// means two independent simulations moving the same content, which reads
    /// as a scroll that accelerates when the finger lifts. The rule:
    ///
    /// | phase | flings? | why |
    /// | --- | --- | --- |
    /// | [`Ended`](ScrollPhase::Ended) | only if the host does **not** report its own momentum | the fingers lifted and nobody else is animating |
    /// | [`Fling`](ScrollPhase::Fling) | yes | the host reported a release velocity and handed the coast to us |
    /// | [`Momentum`](ScrollPhase::Momentum), [`MomentumEnded`](ScrollPhase::MomentumEnded) | never | the host is already animating; this is the double-momentum bug |
    /// | [`Discrete`](ScrollPhase::Discrete), [`Began`](ScrollPhase::Began), [`Changed`](ScrollPhase::Changed), [`Cancelled`](ScrollPhase::Cancelled) | never | not a release |
    ///
    /// Set [`set_os_momentum`](Self::set_os_momentum) from the backend's
    /// `reports_os_momentum` capability and then route every release through
    /// [`fling_for_phase`](Self::fling_for_phase) rather than through
    /// [`fling`](Self::fling); then no call site has to remember the table.
    pub fn should_fling_for_phase(&self, phase: ScrollPhase) -> bool {
        match phase {
            ScrollPhase::Ended => !self.os_momentum,
            ScrollPhase::Fling => true,
            _ => false,
        }
    }

    /// [`fling`](Self::fling), but only when
    /// [`should_fling_for_phase`](Self::should_fling_for_phase) says the phase
    /// warrants one. Returns `false` and starts nothing otherwise.
    ///
    /// This is the entry point a scroll handler should use: it makes the
    /// double-momentum rule impossible to forget, because the phase is already
    /// in hand at every release site.
    pub fn fling_for_phase(
        &mut self,
        phase: ScrollPhase,
        velocity: Vec2,
        profile: &GestureProfile,
    ) -> bool {
        self.should_fling_for_phase(phase) && self.fling(velocity, profile)
    }

    /// The per-axis simulation a fling starts, or `None` for an axis with
    /// nothing to do.
    fn axis_simulation(
        &self,
        position: f32,
        velocity: f32,
        range: (f32, f32),
    ) -> Option<Box<dyn ScrollSimulation>> {
        let (min, max) = range;
        let in_range = position >= min && position <= max;
        if velocity == 0.0 && in_range {
            return None;
        }
        match self.style {
            OverscrollStyle::Clamp => Some(Box::new(ClampingSimulation::new(
                position,
                velocity,
                min,
                max,
                &self.tokens,
            ))),
            OverscrollStyle::RubberBand => Some(Box::new(BouncingSimulation::with_tokens(
                position,
                velocity,
                min,
                max,
                min,
                max,
                &self.tokens,
            ))),
        }
    }

    // --- ticking ---------------------------------------------------------

    /// Advance a running coast to `now`.
    ///
    /// `None` when nothing is animating. The returned step's `absorbed` says
    /// whether each axis actually moved this tick, so a chaining consumer can
    /// tell a live coast from one that is spinning against a boundary.
    pub fn tick(&mut self, now: EventTime) -> Option<ScrollStep> {
        let before = self.position;
        let finished = match self.animation.as_ref() {
            None => return None,
            Some(Animation::Settle) => {
                self.position = self.offset();
                true
            }
            Some(Animation::Fling { start, x, y }) => {
                let t = now.saturating_since(*start);
                let mut done = true;
                if let Some(sim) = x {
                    self.position.x = sim.position(t);
                    done &= sim.is_done(t);
                }
                if let Some(sim) = y {
                    self.position.y = sim.position(t);
                    done &= sim.is_done(t);
                }
                done
            }
        };

        if finished {
            // Snap exactly onto the boundary. A no-op in range, and it removes
            // the sub-pixel residue a spring leaves behind.
            self.position = self.offset();
            self.animation = None;
        }

        self.last_time = now;
        Some(self.step_from(before))
    }

    /// Stop any coast where it is. The offset is left untouched, including an
    /// overscrolled one — a caller that wants it clamped calls
    /// [`set_offset`](Self::set_offset) with the current
    /// [`offset`](Self::offset).
    pub fn stop(&mut self) {
        self.animation = None;
    }

    /// Whether a coast (or a pending reduced-motion settle) is running.
    pub fn is_animating(&self) -> bool {
        self.animation.is_some()
    }

    /// When [`tick`](Self::tick) next wants to be called, or `None` when
    /// nothing is animating.
    ///
    /// One frame after the last movement. The tree folds this into its
    /// `WaitUntil` alongside every other input deadline.
    pub fn next_deadline(&self) -> Option<EventTime> {
        self.animation
            .as_ref()
            .and_then(|_| self.last_time.checked_add(FLING_FRAME_INTERVAL))
    }

    /// Build the step describing the move from `before` to the current
    /// position.
    fn step_from(&self, before: Point) -> ScrollStep {
        let offset = self.offset();
        ScrollStep {
            offset,
            overscroll: Vec2::new(self.position.x - offset.x, self.position.y - offset.y),
            absorbed: (
                axis_absorbed(before.x, self.position.x),
                axis_absorbed(before.y, self.position.y),
            ),
        }
    }
}

/// An axis absorbed movement if the content actually moved by more than
/// [`SCROLL_MOVE_EPSILON`].
///
/// Deliberately *not* "was this axis asked for anything": an axis asked for
/// nothing has absorbed nothing, and an axis asked for 50 dp that could only
/// give a rounding error has absorbed nothing either. Both must chain, and both
/// answer the same question, so there is one test rather than two.
fn axis_absorbed(before: f32, after: f32) -> bool {
    (after - before).abs() > SCROLL_MOVE_EPSILON
}

/// An inverted range means the content fits: pin the offset at `min`.
fn normalize(min: f32, max: f32) -> (f32, f32) {
    if !min.is_finite() || !max.is_finite() {
        return (0.0, 0.0);
    }
    if min > max { (min, min) } else { (min, max) }
}

// ---------------------------------------------------------------------------
// FlingDriver
// ---------------------------------------------------------------------------

/// Resolve [`ScrollPhysics::Platform`] against the host OS.
///
/// Bouncing on the Apple platforms, clamping everywhere else — the two
/// conventions users of each platform already have in their hands.
pub fn resolve_platform_physics(physics: ScrollPhysics) -> ScrollPhysics {
    match physics {
        ScrollPhysics::Platform => {
            if cfg!(any(target_os = "macos", target_os = "ios")) {
                ScrollPhysics::Bouncing
            } else {
                ScrollPhysics::Clamping
            }
        }
        other => other,
    }
}

/// One widget's live coast.
#[derive(Debug)]
struct FlingEntry {
    id: WidgetId,
    start: EventTime,
    /// Last time this entry produced a delta — the base for its deadline.
    last: EventTime,
    /// Integrated position at `last`, so each tick reports a delta rather than
    /// an absolute the receiver would have to difference itself.
    last_position: Vec2,
    x: Box<dyn ScrollSimulation>,
    y: Box<dyn ScrollSimulation>,
    /// Keeps the owner waking the event loop while the coast runs. `None` when
    /// the driver was built without a scheduler (a headless test).
    _tick: Option<FrameTickSubscription>,
}

/// The tree-level fling pump: coasts that are **re-dispatched** as scroll
/// deltas rather than applied to one scroller's own offset.
///
/// The distinction from [`KineticScroller`] is chaining. A scroller's fling
/// belongs to that scroller and stops at its bounds. A driver's fling belongs
/// to the *tree*: each tick it hands out a delta for the target widget, and if
/// the target declines the delta because it has reached its end, the caller
/// chains it to an ancestor exactly as it would chain a wheel event. That is
/// what makes a flick that runs out of inner list scroll the outer one.
///
/// The driver holds simulations only. Bounds live with the widgets that
/// receive the deltas, so the simulations here are unbounded coasts.
///
/// # Where the tree's one lives
///
/// [`WidgetTree`](crate::WidgetTree) builds one in its constructor from its own
/// [`FrameTickScheduler`] and drives it from `tick_flings_with_ops`, on the
/// pass a host already runs each wake. Its
/// [`next_deadline`](Self::next_deadline) is folded into
/// `WidgetTree::next_input_deadline`, and from there into the one
/// `ControlFlow::WaitUntil`. A caller that wants a coast of its own — a
/// headless test, a surface driving its own release — can still build one
/// directly: it reaches for no tree.
#[derive(Debug)]
pub struct FlingDriver {
    entries: Vec<FlingEntry>,
    tokens: ScrollPhysicsTokens,
    /// Cloned from the tree's scheduler when there is one. `FrameTickScheduler`
    /// is an `Rc` handle, so this shares the tree's table rather than making a
    /// second one — the module docs' "do not invent a second scheduler" rule,
    /// enforced by construction.
    scheduler: Option<FrameTickScheduler>,
    reduced_motion: bool,
}

impl Default for FlingDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl FlingDriver {
    /// A driver with no scheduler — nothing is kept awake, which is what a
    /// headless test wants.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            tokens: ScrollPhysicsTokens::DEFAULT,
            scheduler: None,
            reduced_motion: false,
        }
    }

    /// A driver that keeps its targets waking the event loop through the
    /// tree's existing per-frame scheduler.
    pub fn with_scheduler(scheduler: FrameTickScheduler) -> Self {
        Self {
            scheduler: Some(scheduler),
            ..Self::new()
        }
    }

    /// Use a theme's own physics tokens.
    pub fn set_tokens(&mut self, tokens: &ScrollPhysicsTokens) {
        self.tokens = *tokens;
    }

    /// Turn `prefers-reduced-motion` on or off.
    ///
    /// With it on, [`start`](Self::start) is a no-op and any running coast is
    /// dropped: a re-dispatched fling has no "settle" to collapse to, because
    /// the target already has the content where the finger left it.
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
        if reduced {
            self.entries.clear();
        }
    }

    /// Begin coasting `target` at `velocity` (logical pixels per second, in
    /// scroll-offset space) from `at`.
    ///
    /// Replaces any coast already running for the same target. A zero or
    /// non-finite velocity, or reduced motion, starts nothing.
    pub fn start(
        &mut self,
        target: WidgetId,
        velocity: Vec2,
        physics: ScrollPhysics,
        at: EventTime,
    ) {
        self.stop(target);
        if self.reduced_motion
            || !velocity.x.is_finite()
            || !velocity.y.is_finite()
            || (velocity.x == 0.0 && velocity.y == 0.0)
        {
            return;
        }

        let physics = resolve_platform_physics(physics);
        let make = |v: f32| -> Box<dyn ScrollSimulation> {
            match physics {
                ScrollPhysics::Bouncing => Box::new(BouncingSimulation::with_tokens(
                    0.0,
                    v,
                    f32::MIN / 4.0,
                    f32::MAX / 4.0,
                    f32::MIN / 4.0,
                    f32::MAX / 4.0,
                    &self.tokens,
                )),
                // `Platform` is already resolved above; treat it as clamping so
                // the match stays total without an unreachable arm.
                ScrollPhysics::Clamping | ScrollPhysics::Platform => Box::new(
                    ClampingSimulation::new(0.0, v, f32::MIN / 4.0, f32::MAX / 4.0, &self.tokens),
                ),
            }
        };

        self.entries.push(FlingEntry {
            id: target,
            start: at,
            last: at,
            last_position: Vec2::ZERO,
            x: make(velocity.x),
            y: make(velocity.y),
            _tick: self.scheduler.as_ref().map(|s| s.subscribe(target)),
        });
    }

    /// Advance every coast to `now` and report the deltas to dispatch.
    ///
    /// A coast that produced no visible movement this tick is omitted rather
    /// than reported as a zero delta, so a caller can dispatch the result
    /// verbatim. Finished coasts emit their last delta and are then dropped,
    /// which releases their frame-tick subscription.
    pub fn tick(&mut self, now: EventTime) -> Vec<(WidgetId, Vec2)> {
        let mut out = Vec::new();
        self.entries.retain_mut(|entry| {
            let t = now.saturating_since(entry.start);
            let position = Vec2::new(entry.x.position(t), entry.y.position(t));
            let delta = Vec2::new(
                position.x - entry.last_position.x,
                position.y - entry.last_position.y,
            );
            entry.last_position = position;
            entry.last = now;

            if delta.x.abs() > SCROLL_MOVE_EPSILON || delta.y.abs() > SCROLL_MOVE_EPSILON {
                out.push((entry.id, delta));
            }
            !(entry.x.is_done(t) && entry.y.is_done(t))
        });
        out
    }

    /// Stop `target`'s coast, if it has one. Idempotent.
    ///
    /// A press on a coasting surface calls this: catching a flying list is a
    /// reflex users have, and a coast that ignored the press would read as
    /// broken.
    pub fn stop(&mut self, target: WidgetId) {
        self.entries.retain(|e| e.id != target);
    }

    /// Stop every coast.
    pub fn stop_all(&mut self) {
        self.entries.clear();
    }

    /// Whether `target` is coasting.
    pub fn is_flinging(&self, target: WidgetId) -> bool {
        self.entries.iter().any(|e| e.id == target)
    }

    /// How many coasts are running.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is coasting.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// When [`tick`](Self::tick) next wants to be called — the earliest
    /// deadline across every running coast, or `None` when there are none.
    pub fn next_deadline(&self) -> Option<EventTime> {
        self.entries
            .iter()
            .filter_map(|e| e.last.checked_add(FLING_FRAME_INTERVAL))
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;
    use crate::widget_tree::WidgetTree;

    fn touch() -> GestureProfile {
        GestureProfile::TOUCH
    }

    fn scroller(style: OverscrollStyle) -> KineticScroller {
        let mut s = KineticScroller::new(style);
        s.set_range_y(0.0, 1000.0);
        s.set_range_x(0.0, 0.0);
        s.set_viewport(Vec2::new(400.0, 600.0));
        s
    }

    fn two_ids() -> (WidgetId, WidgetId) {
        let mut tree = WidgetTree::new();
        (tree.add(FillWidget::new()), tree.add(FillWidget::new()))
    }

    // --- panning ---------------------------------------------------------

    #[test]
    fn a_pan_inside_the_range_moves_one_for_one() {
        let mut s = scroller(OverscrollStyle::Clamp);
        let step = s.pan(EventTime::ZERO, Point::ZERO, Vec2::new(0.0, 120.0));
        assert_eq!(step.offset.y, 120.0);
        assert_eq!(step.overscroll, Vec2::ZERO);
        assert_eq!(step.absorbed, (false, true), "only y was asked for");
    }

    /// The boundary hand-off signal: an axis that cannot move reports that it
    /// absorbed nothing, which is what lets the event chain outward.
    #[test]
    fn a_clamped_axis_at_the_boundary_absorbs_nothing() {
        let mut s = scroller(OverscrollStyle::Clamp);
        s.set_offset(Point::new(0.0, 1000.0));
        let step = s.pan(EventTime::ZERO, Point::ZERO, Vec2::new(0.0, 50.0));
        assert_eq!(step.offset.y, 1000.0);
        assert!(!step.absorbed.1, "there was nowhere to go");
        assert!(!step.absorbed_any());
    }

    /// Under the rubber band the same push does move the content — just far
    /// less than it was asked for — so the axis has absorbed it and the event
    /// does not chain.
    #[test]
    fn a_rubber_band_axis_absorbs_at_the_boundary_with_falling_gain() {
        let mut s = scroller(OverscrollStyle::RubberBand);
        s.set_offset(Point::new(0.0, 1000.0));

        let first = s.pan(EventTime::from_millis(0), Point::ZERO, Vec2::new(0.0, 50.0));
        assert_eq!(first.offset.y, 1000.0, "the offset stays at the bound");
        assert!(first.overscroll.y > 0.0, "the overscroll takes the travel");
        assert!(
            first.overscroll.y < 50.0,
            "and resists: {} of 50",
            first.overscroll.y
        );
        assert!(first.absorbed.1);

        // A second identical push moves it less than the first did — the gain
        // falls with the travel already spent.
        let second = s.pan(
            EventTime::from_millis(16),
            Point::ZERO,
            Vec2::new(0.0, 50.0),
        );
        let first_gain = first.overscroll.y;
        let second_gain = second.overscroll.y - first.overscroll.y;
        assert!(
            second_gain < first_gain,
            "gain must fall: {first_gain} then {second_gain}"
        );
    }

    /// Dragging back out of the overscroll must retrace the same curve, not
    /// jump — which is why the drag works through the inverse rubber band.
    #[test]
    fn a_reversed_drag_retraces_the_rubber_band() {
        let mut s = scroller(OverscrollStyle::RubberBand);
        s.set_offset(Point::new(0.0, 1000.0));
        let out = s.pan(EventTime::from_millis(0), Point::ZERO, Vec2::new(0.0, 80.0));
        let back = s.pan(
            EventTime::from_millis(16),
            Point::ZERO,
            Vec2::new(0.0, -80.0),
        );
        assert!(
            back.overscroll.y.abs() < 0.5,
            "returned to {} rather than 0 after retracing {}",
            back.overscroll.y,
            out.overscroll.y
        );
        assert!(
            (back.offset.y - 1000.0).abs() < 1e-3,
            "the offset stayed at the bound, got {}",
            back.offset.y
        );
    }

    /// Reduced motion turns the bounce off entirely: the boundary is hard.
    #[test]
    fn reduced_motion_hard_clamps_the_rubber_band() {
        let mut s = scroller(OverscrollStyle::RubberBand);
        s.set_reduced_motion(true);
        s.set_offset(Point::new(0.0, 1000.0));
        let step = s.pan(EventTime::ZERO, Point::ZERO, Vec2::new(0.0, 200.0));
        assert_eq!(
            step.overscroll,
            Vec2::ZERO,
            "no rubber band under reduced motion"
        );
        assert_eq!(step.offset.y, 1000.0);
    }

    #[test]
    fn a_pan_cancels_a_running_coast() {
        let mut s = scroller(OverscrollStyle::Clamp);
        s.pan(EventTime::from_millis(0), Point::ZERO, Vec2::new(0.0, 10.0));
        assert!(s.fling(Vec2::new(0.0, 2000.0), &touch()));
        assert!(s.is_animating());
        s.pan(EventTime::from_millis(20), Point::ZERO, Vec2::new(0.0, 5.0));
        assert!(!s.is_animating(), "grabbing the content stops the coast");
    }

    // --- flinging --------------------------------------------------------

    /// The floor exists so a slow release settles where it is. Below it,
    /// nothing starts at all.
    #[test]
    fn below_the_minimum_velocity_a_fling_starts_nothing() {
        let profile = touch();
        let mut s = scroller(OverscrollStyle::Clamp);
        s.set_offset(Point::new(0.0, 100.0));

        let below = profile.min_fling_velocity - 1.0;
        assert!(!s.fling(Vec2::new(0.0, below), &profile));
        assert!(!s.is_animating(), "and starts no animation");
        assert_eq!(s.next_deadline(), None, "and asks for no wake-up");
        assert_eq!(s.offset().y, 100.0, "and does not move the content");

        assert!(
            s.fling(Vec2::new(0.0, profile.min_fling_velocity), &profile),
            "exactly at the floor is a fling"
        );
    }

    #[test]
    fn a_fling_above_the_maximum_velocity_is_clamped_not_refused() {
        let profile = touch();
        let mut fast = scroller(OverscrollStyle::Clamp);
        let mut capped = scroller(OverscrollStyle::Clamp);

        assert!(fast.fling(Vec2::new(0.0, profile.max_fling_velocity * 10.0), &profile));
        assert!(capped.fling(Vec2::new(0.0, profile.max_fling_velocity), &profile));

        let at = EventTime::from_millis(500);
        let a = fast.tick(at).unwrap().offset.y;
        let b = capped.tick(at).unwrap().offset.y;
        assert!(
            (a - b).abs() < 1.0,
            "a 10x over-speed release must behave exactly like the cap: {a} vs {b}"
        );
    }

    /// The headline reduced-motion guarantee.
    #[test]
    fn reduced_motion_settles_in_one_tick() {
        let mut s = scroller(OverscrollStyle::Clamp);
        s.set_reduced_motion(true);
        s.set_offset(Point::new(0.0, 400.0));

        assert!(
            s.fling(Vec2::new(0.0, 6000.0), &touch()),
            "the release is accepted"
        );
        assert!(s.is_animating(), "so there is exactly one step pending");

        let step = s.tick(EventTime::from_millis(16)).expect("one step");
        assert_eq!(step.offset.y, 400.0, "settled where the finger left it");
        assert!(!s.is_animating(), "and it is over after that single tick");
        assert_eq!(s.tick(EventTime::from_millis(32)), None);
    }

    /// Turning reduced motion on mid-coast must collapse it, not let it run.
    #[test]
    fn reduced_motion_collapses_a_coast_already_in_flight() {
        let mut s = scroller(OverscrollStyle::Clamp);
        assert!(s.fling(Vec2::new(0.0, 4000.0), &touch()));
        s.tick(EventTime::from_millis(16));
        s.set_reduced_motion(true);
        s.tick(EventTime::from_millis(32));
        assert!(!s.is_animating());
    }

    #[test]
    fn a_clamping_coast_runs_then_stops_at_the_boundary() {
        let mut s = scroller(OverscrollStyle::Clamp);
        assert!(s.fling(Vec2::new(0.0, 4000.0), &touch()));

        let mut last = 0.0;
        for frame in 1..200 {
            let Some(step) = s.tick(EventTime::from_millis(frame * 16)) else {
                break;
            };
            assert!(step.offset.y >= last - 1e-3, "the coast reversed");
            assert!(step.offset.y <= 1000.0, "the coast passed the boundary");
            assert_eq!(step.overscroll, Vec2::ZERO, "clamping never overscrolls");
            last = step.offset.y;
        }
        assert!(!s.is_animating(), "the coast must end");
        assert_eq!(s.offset().y, 1000.0, "resting on the boundary");
    }

    /// A bouncing coast into the boundary overshoots (a non-zero overscroll)
    /// and then comes back to rest exactly on it.
    #[test]
    fn a_bouncing_coast_overshoots_then_rests_on_the_boundary() {
        let mut s = scroller(OverscrollStyle::RubberBand);
        s.set_offset(Point::new(0.0, 900.0));
        assert!(s.fling(Vec2::new(0.0, 3000.0), &touch()));

        let mut peak: f32 = 0.0;
        for frame in 1..400 {
            let Some(step) = s.tick(EventTime::from_millis(frame * 16)) else {
                break;
            };
            peak = peak.max(step.overscroll.y);
        }
        assert!(
            peak > 0.0,
            "a bouncing coast must overshoot, peaked at {peak}"
        );
        assert!(!s.is_animating());
        assert_eq!(s.offset().y, 1000.0);
    }

    #[test]
    fn the_deadline_is_one_frame_out_and_only_while_animating() {
        let mut s = scroller(OverscrollStyle::Clamp);
        assert_eq!(s.next_deadline(), None);

        s.pan(
            EventTime::from_millis(100),
            Point::ZERO,
            Vec2::new(0.0, 5.0),
        );
        assert!(s.fling(Vec2::new(0.0, 3000.0), &touch()));
        assert_eq!(
            s.next_deadline(),
            EventTime::from_millis(100).checked_add(FLING_FRAME_INTERVAL)
        );

        s.stop();
        assert_eq!(s.next_deadline(), None);
    }

    // --- the macOS momentum guard ----------------------------------------

    /// The double-momentum rule, stated as a table so a reader can check the
    /// whole policy at once.
    #[test]
    fn the_phase_guard_refuses_every_phase_the_host_is_already_animating() {
        let mut s = scroller(OverscrollStyle::Clamp);

        // A host that animates its own momentum: only an explicit `Fling`
        // hands us the coast.
        s.set_os_momentum(true);
        for phase in [
            ScrollPhase::Discrete,
            ScrollPhase::Began,
            ScrollPhase::Changed,
            ScrollPhase::Ended,
            ScrollPhase::Momentum,
            ScrollPhase::MomentumEnded,
            ScrollPhase::Cancelled,
        ] {
            assert!(
                !s.should_fling_for_phase(phase),
                "{phase:?} must not fling on a host with its own momentum"
            );
        }
        assert!(s.should_fling_for_phase(ScrollPhase::Fling));

        // A host that does not: the lift is ours to animate.
        s.set_os_momentum(false);
        assert!(s.should_fling_for_phase(ScrollPhase::Ended));
        assert!(s.should_fling_for_phase(ScrollPhase::Fling));
        for phase in [
            ScrollPhase::Discrete,
            ScrollPhase::Began,
            ScrollPhase::Changed,
            ScrollPhase::Momentum,
            ScrollPhase::MomentumEnded,
            ScrollPhase::Cancelled,
        ] {
            assert!(
                !s.should_fling_for_phase(phase),
                "{phase:?} is not a release"
            );
        }
    }

    /// The guard has to actually gate the fling, not merely report on it —
    /// this is the call site that makes the rule unforgettable.
    #[test]
    fn fling_for_phase_starts_nothing_on_a_momentum_delta() {
        let mut s = scroller(OverscrollStyle::Clamp);
        s.set_os_momentum(true);

        assert!(!s.fling_for_phase(ScrollPhase::Momentum, Vec2::new(0.0, 4000.0), &touch()));
        assert!(!s.is_animating(), "the classic double-momentum bug");

        assert!(!s.fling_for_phase(ScrollPhase::Ended, Vec2::new(0.0, 4000.0), &touch()));
        assert!(
            !s.is_animating(),
            "the OS is about to send its own momentum"
        );

        assert!(s.fling_for_phase(ScrollPhase::Fling, Vec2::new(0.0, 4000.0), &touch()));
        assert!(s.is_animating(), "an explicit fling hands the coast to us");
    }

    /// The tracker follows the pointer, and a fling site reads it there.
    #[test]
    fn panning_feeds_the_velocity_tracker() {
        let mut s = scroller(OverscrollStyle::Clamp);
        for i in 0..8u64 {
            let y = i as f32 * -6.0;
            s.pan(
                EventTime::from_millis(i * 10),
                Point::new(0.0, y),
                Vec2::new(0.0, -y),
            );
        }
        let v = s.pointer_velocity();
        assert!(
            (v.y + 600.0).abs() < 10.0,
            "the pointer was moving at -600 dp/s, tracker says {}",
            v.y
        );
    }

    // --- FlingDriver -----------------------------------------------------

    #[test]
    fn the_driver_hands_out_deltas_until_the_coast_is_spent() {
        let (a, _) = two_ids();
        let mut d = FlingDriver::new();
        d.start(
            a,
            Vec2::new(0.0, 3000.0),
            ScrollPhysics::Clamping,
            EventTime::ZERO,
        );
        assert!(d.is_flinging(a));

        let mut total = 0.0f32;
        let mut frames = 0;
        for frame in 1..400 {
            let out = d.tick(EventTime::from_millis(frame * 16));
            for (id, delta) in out {
                assert_eq!(id, a);
                assert!(delta.y >= -1e-3, "a coast must not reverse");
                total += delta.y;
                frames += 1;
            }
            if d.is_empty() {
                break;
            }
        }
        assert!(
            frames > 10,
            "the coast should last more than {frames} frames"
        );
        assert!(d.is_empty(), "and must end");
        // Android's getSplineFlingDistance(3000) — the integrated deltas are
        // the same travel the simulation promises.
        let expected =
            super::super::simulation::fling_distance(3000.0, &ScrollPhysicsTokens::DEFAULT);
        assert!(
            (total - expected).abs() < 2.0,
            "integrated {total}, simulation says {expected}"
        );
    }

    #[test]
    fn stopping_a_target_ends_only_its_coast() {
        let (a, b) = two_ids();
        let mut d = FlingDriver::new();
        d.start(
            a,
            Vec2::new(0.0, 3000.0),
            ScrollPhysics::Clamping,
            EventTime::ZERO,
        );
        d.start(
            b,
            Vec2::new(0.0, 3000.0),
            ScrollPhysics::Clamping,
            EventTime::ZERO,
        );
        assert_eq!(d.len(), 2);

        d.stop(a);
        assert!(!d.is_flinging(a));
        assert!(d.is_flinging(b));
        d.stop(a);
        assert_eq!(d.len(), 1, "stopping twice is idempotent");

        d.stop_all();
        assert!(d.is_empty());
    }

    #[test]
    fn restarting_a_target_replaces_its_coast() {
        let (a, _) = two_ids();
        let mut d = FlingDriver::new();
        d.start(
            a,
            Vec2::new(0.0, 3000.0),
            ScrollPhysics::Clamping,
            EventTime::ZERO,
        );
        d.start(
            a,
            Vec2::new(0.0, 1000.0),
            ScrollPhysics::Clamping,
            EventTime::ZERO,
        );
        assert_eq!(d.len(), 1, "one coast per target");
    }

    #[test]
    fn a_zero_velocity_start_is_a_no_op() {
        let (a, _) = two_ids();
        let mut d = FlingDriver::new();
        d.start(a, Vec2::ZERO, ScrollPhysics::Clamping, EventTime::ZERO);
        assert!(d.is_empty());
        d.start(
            a,
            Vec2::new(f32::NAN, 0.0),
            ScrollPhysics::Clamping,
            EventTime::ZERO,
        );
        assert!(d.is_empty());
    }

    #[test]
    fn reduced_motion_stops_the_driver_entirely() {
        let (a, _) = two_ids();
        let mut d = FlingDriver::new();
        d.start(
            a,
            Vec2::new(0.0, 3000.0),
            ScrollPhysics::Clamping,
            EventTime::ZERO,
        );
        d.set_reduced_motion(true);
        assert!(d.is_empty(), "a running coast is dropped");

        d.start(
            a,
            Vec2::new(0.0, 3000.0),
            ScrollPhysics::Clamping,
            EventTime::ZERO,
        );
        assert!(d.is_empty(), "and no new one starts");
        assert_eq!(d.next_deadline(), None);
    }

    #[test]
    fn the_driver_deadline_is_the_earliest_across_its_coasts() {
        let (a, b) = two_ids();
        let mut d = FlingDriver::new();
        assert_eq!(d.next_deadline(), None);

        d.start(
            a,
            Vec2::new(0.0, 3000.0),
            ScrollPhysics::Clamping,
            EventTime::from_millis(10),
        );
        d.start(
            b,
            Vec2::new(0.0, 3000.0),
            ScrollPhysics::Clamping,
            EventTime::from_millis(40),
        );
        assert_eq!(
            d.next_deadline(),
            EventTime::from_millis(10).checked_add(FLING_FRAME_INTERVAL)
        );
    }

    /// The driver integrates on the tree's existing per-frame path rather than
    /// running a cadence of its own: a coast holds one subscription for its
    /// target, and releases it when the coast ends.
    #[test]
    fn a_coast_holds_a_frame_tick_subscription_for_its_target() {
        let (a, _) = two_ids();
        let scheduler = FrameTickScheduler::new();
        let mut d = FlingDriver::with_scheduler(scheduler.clone());
        assert_eq!(scheduler.subscriber_count(), 0);

        d.start(
            a,
            Vec2::new(0.0, 3000.0),
            ScrollPhysics::Clamping,
            EventTime::ZERO,
        );
        assert_eq!(scheduler.subscriber_count(), 1);

        d.stop(a);
        assert_eq!(
            scheduler.subscriber_count(),
            0,
            "stopping releases the wake"
        );
    }

    #[test]
    fn platform_physics_resolves_to_a_concrete_family() {
        assert_ne!(
            resolve_platform_physics(ScrollPhysics::Platform),
            ScrollPhysics::Platform
        );
        assert_eq!(
            resolve_platform_physics(ScrollPhysics::Clamping),
            ScrollPhysics::Clamping
        );
        assert_eq!(
            resolve_platform_physics(ScrollPhysics::Bouncing),
            ScrollPhysics::Bouncing
        );
        let expected = if cfg!(any(target_os = "macos", target_os = "ios")) {
            ScrollPhysics::Bouncing
        } else {
            ScrollPhysics::Clamping
        };
        assert_eq!(resolve_platform_physics(ScrollPhysics::Platform), expected);
    }
}
