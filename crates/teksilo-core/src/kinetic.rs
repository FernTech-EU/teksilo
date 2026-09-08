// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Kinetic scrolling: velocity, fling physics, rubber band — computed once.
//!
//! Every pixel-offset scrollable in this workspace hand-rolled the same
//! boundary clamp followed by the same 150 ms ease-out tween, with no velocity
//! behind it, no fling, and no rubber band. Adding real physics to each would
//! have meant one integrator, one tolerance constant and one place to forget
//! `prefers-reduced-motion` per surface — a failure mode this codebase already
//! demonstrates, with `EDGE = 32` and `MAX_VELOCITY = 12` copied across five
//! widgets that have since drifted apart. They share one behaviour now; see
//! `docs/kinetic-scrolling.md` for which surfaces, and for the one that does
//! not (a terminal scrolls a scrollback ring by whole lines, not a pixel
//! offset with a maximum).
//!
//! So the physics lives here, once, as pure computation. Nothing in this module
//! owns a widget, reads a clock or touches the arena; time arrives as an
//! [`EventTime`](crate::pointer::EventTime) and the answers are numbers.
//!
//! # The four pieces
//!
//! | | | |
//! | --- | --- | --- |
//! | [`VelocityTracker`] | how fast was the pointer going? | Flutter / Android least-squares fit |
//! | [`ClampingSimulation`] | where does a fling go, hard-stopping at the edge? | Android `OverScroller` |
//! | [`BouncingSimulation`] | …and where does it go if the edge gives? | Flutter `BouncingScrollSimulation` |
//! | [`rubber_band`] | how far does content move when *dragged* past the edge? | iOS / Flutter friction curve |
//!
//! Two driver types put them to work: [`KineticScroller`], which one scrollable
//! surface owns, and [`FlingDriver`], the tree-level pump for a coast that is
//! re-dispatched as scroll deltas so it can chain outward at a boundary.
//!
//! # Everything is reproduced, nothing is invented
//!
//! Every constant is traceable to Android's `OverScroller.java` or to Flutter's
//! `scroll_simulation.dart` / `velocity_tracker.dart` / `spring_simulation.dart`,
//! and is cited at its definition together with the arithmetic that produces
//! it. Scroll feel is muscle memory; a curve that is merely plausible reads as
//! broken, and "plausible" cannot be reviewed. A reader must be able to put
//! this code next to the upstream source and check it line by line — which is
//! why Android's spline is rebuilt by the same bisection rather than
//! approximated, and why the tests assert hand-derived reference values rather
//! than goldens this implementation produced.
//!
//! The three deliberate divergences from upstream are each documented at their
//! site: the `f64` least-squares arithmetic, the underdamped spring's decay
//! rate, and the signed spring hand-off velocity.
//!
//! # Reduced motion
//!
//! [`KineticScroller::set_reduced_motion`] collapses a fling to an immediate
//! clamped settle and hard-clamps the rubber band. That is the *scrolling* half
//! of the accessibility rule; the magnifier, the toolbar entrance, the tooltip
//! fade, the scrollbar reveal and the density relayout each belong to the
//! package that owns that surface.
//!
//! # The macOS momentum rule
//!
//! macOS simulates momentum itself and delivers it as
//! [`ScrollPhase::Momentum`](crate::pointer::ScrollPhase::Momentum) deltas.
//! Starting a Teksilo fling on top of that is the classic double-momentum bug —
//! two simulations moving one surface, so the content speeds up when the finger
//! lifts. [`KineticScroller::fling_for_phase`] is the guard: route every release
//! through it and the rule cannot be forgotten, because the phase is already in
//! hand at every release site.
//!
//! Reference: `docs/kinetic-scrolling.md`.

pub mod scroller;
pub mod simulation;
pub mod velocity;

pub use scroller::{
    DEFAULT_VIEWPORT_EXTENT, FLING_FRAME_INTERVAL, FlingDriver, KineticScroller, ScrollStep,
    resolve_platform_physics,
};
pub use simulation::{
    BouncingSimulation, ClampingSimulation, SETTLE_DISTANCE_TOLERANCE, SETTLE_VELOCITY_TOLERANCE,
    ScrollSimulation, fling_distance, fling_duration, rubber_band, rubber_band_inverse,
    rubber_band_inverse_with, rubber_band_with,
};
pub use velocity::{
    HISTORY_SIZE, HORIZON, MIN_SAMPLE_SIZE, STOP_GAP, VelocityEstimate, VelocityTracker,
};
