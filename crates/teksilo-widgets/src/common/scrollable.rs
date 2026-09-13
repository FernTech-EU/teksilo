// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! One scroll handler for every scrollable surface.
//!
//! The scrollable widgets in this crate each hand-rolled the same `on_scroll`
//! body: convert a [`ScrollDelta`] to pixels, clamp each axis, animate or set,
//! and answer `Ignored` at a hard boundary so the event chains to an ancestor.
//! They agreed on the arithmetic and disagreed on everything around it — which
//! of them honoured `Contain`, which animated, which read a line height. This
//! module is that body written once, plus the two things none of them had: a
//! finger's pan, and the rubber band that pan needs at the edge.
//!
//! [`ScrollArea`](crate::ScrollArea) is the reference adopter and the one to
//! copy. The data views ([`ListView`](crate::ListView),
//! [`TreeView`](crate::TreeView), [`GridView`](crate::GridView),
//! [`TableView`](crate::TableView),
//! [`TreeTableView`](crate::TreeTableView)) and the three text surfaces
//! ([`RichTextEditor`](crate::rich_text::RichTextEditor), [`CodeEditor`](crate::CodeEditor)
//! and [`LogView`](crate::LogView)) install it too. A widget that handles a
//! wheel without owning a scroll offset — `SpinBox` steps a number, `TabBar`
//! remaps a notch sideways — is not a scrollable and does not appear here.
//!
//! # The two paths
//!
//! [`handle_scroll_event`] branches on
//! [`EventContext::scroll_source`](teksilo_core::widget::EventContext::scroll_source),
//! not on the phase:
//!
//! * **Everything except [`ScrollSource::TouchPan`]** — a wheel notch, a
//!   trackpad stream, a programmatic scroll — takes the path this crate has
//!   always taken: clamp against the *animation target* rather than the
//!   rendered offset (so a mid-tween boundary chains correctly), then either
//!   tween over [`ScrollHandlingOptions::smooth_duration`] or set outright,
//!   per axis and only where the clamp moved.
//! * **[`ScrollSource::TouchPan`]** — a pan synthesised from a direct pointer
//!   by the router, and the coast that follows it — goes through a
//!   [`KineticScroller`], which is what supplies the rubber band. A pan never
//!   tweens: a finger is already the animation.
//!
//! Splitting on the source and not the phase is what makes the migration safe.
//! A legacy [`WidgetEvent::Scroll`] reports [`ScrollSource::Wheel`], so every
//! existing call site and every existing test keeps the path it had. The
//! distinction is load-bearing rather than cosmetic, and is pinned by
//! `a_trackpad_stream_takes_the_wheel_path_and_not_the_kinetic_one`
//! (`tests/scrollables_touch.rs`): a trackpad stream carries the same
//! `Began`/`Changed`/`Ended` phases a synthesised pan does, so a phase test
//! would route a pointing device into the kinetic path and start tracking
//! velocity for a contact that will never lift.
//!
//! # Who owns what
//!
//! The scroller belongs to the widget, not to this module: it is the widget
//! that knows its viewport (which is the only thing the rubber-band curve reads
//! beyond the range) and the widget whose layout pass is where that number
//! becomes available. So the surface owns an `Rc<RefCell<KineticScroller>>`,
//! calls [`set_viewport`](KineticScroller::set_viewport) from its own layout,
//! and hands the handle to [`handle_scroll_event`] on every event. The range is
//! read from the [`ScrollableAxes`] signals each time, so it is never stale.
//!
//! The **coast** is not owned here at all. A release hands its velocity to the
//! tree's `FlingDriver`, which re-dispatches it as
//! [`ScrollPhase::Fling`] deltas along the same claimant chain the pan walked —
//! that is what makes a flick that runs out of an inner list scroll the outer
//! one. A fling delta therefore arrives here as an ordinary positive-or-negative
//! offset change and is applied with a **hard clamp**: the driver's simulation
//! is unbounded and stopping it at the edge is this surface's job, not the
//! band's.
//!
//! # Adoption
//!
//! ```ignore
//! let axes = ScrollableAxes::new(scroll_x, scroll_y, max_x, max_y);
//! let behavior = ScrollableBehavior::new(axes)
//!     .with_scroller(self.scroller.clone())
//!     .axes(PanAxes::BOTH)
//!     .smooth(self.smooth_scrolling)
//!     .line_height(self.line_height)
//!     .reduced_motion(ctx.prefers_reduced_motion());
//! let handlers = behavior.install(HandlerSet::new());
//! ```
//!
//! `install` attaches both halves: the `on_scroll` handler *and* the
//! [`PanClaim`] that makes the node a pan claimant in the first place. A
//! surface that installs the handler without the claim is a surface a finger
//! cannot scroll, which is the bug this module exists to stop shipping.
//!
//! Reference: `docs/kinetic-scrolling.md`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{Point, Vec2};
use teksilo_core::OverscrollBehavior;
use teksilo_core::event::{EventResponse, ScrollDelta, WidgetEvent};
use teksilo_core::kinetic::KineticScroller;
use teksilo_core::pointer::touch_action::{Axis, PanAxes, PanClaim};
use teksilo_core::pointer::{EventTime, ScrollPhase, ScrollSource};
use teksilo_core::signal::Signal;
use teksilo_core::widget::EventContext;
use teksilo_core::widget_builder::HandlerSet;
use teksilo_tokens::{Easing, OverscrollStyle, PointerKindMask, ScrollPhysicsTokens};

use crate::common::scroll::{scroll_clamp_axis, scroll_response};

/// The 150 ms ease-out every smooth-scrolling surface in this crate uses for a
/// wheel notch. Unchanged by the touch programme — a wheel still feels the way
/// it always did.
pub const SMOOTH_SCROLL_DURATION: Duration = Duration::from_millis(150);

// ---------------------------------------------------------------------------
// ScrollableAxes
// ---------------------------------------------------------------------------

/// The reactive state one scrollable surface scrolls: where it is on each axis,
/// how far it can go, and how far past the end it is currently being held.
///
/// Every field is a `Signal`, and they are the *shared* ones — the same handles
/// a `ScrollBar` reads and an `ensure_visible` writes — so this type is a view
/// onto the widget's state rather than a second copy of it. Cloning is cloning
/// handles.
///
/// An offset must be [`Signal::new_animated`] on a surface that turns
/// [`ScrollHandlingOptions::smooth`] on **and gives that axis a range**:
/// `animate_to` on a plain signal panics. An axis with a permanently zero
/// range is never written by either path, so a surface that scrolls on one
/// axis only may leave the other plain.
#[derive(Clone, Debug)]
pub struct ScrollableAxes {
    /// Horizontal offset, `0.0` at the leading edge.
    pub x: Signal<f32>,
    /// Vertical offset, `0.0` at the top.
    pub y: Signal<f32>,
    /// Largest legal [`x`](Self::x) — content width minus viewport width, never
    /// below zero.
    pub max_x: Signal<f32>,
    /// Largest legal [`y`](Self::y).
    pub max_y: Signal<f32>,
    /// How far past the range the content is being held right now, per axis,
    /// after the rubber band. Always `ZERO` under [`OverscrollStyle::Clamp`],
    /// under reduced motion, and outside a live pan.
    ///
    /// Published for a surface that wants to draw the stretch or the glow; the
    /// offset itself never leaves the range, so a surface that ignores this
    /// signal is still correct.
    pub overscroll: Signal<Vec2>,
}

impl ScrollableAxes {
    /// Both axes, with a fresh overscroll signal.
    pub fn new(x: Signal<f32>, y: Signal<f32>, max_x: Signal<f32>, max_y: Signal<f32>) -> Self {
        Self {
            x,
            y,
            max_x,
            max_y,
            overscroll: Signal::new(Vec2::ZERO),
        }
    }

    /// A vertical-only surface: the horizontal axis is pinned at zero with a
    /// zero range, so nothing can ever move it.
    ///
    /// The pinned axis is a plain signal. Both paths write an axis only when
    /// its clamp actually moved, and an axis whose range is zero and whose
    /// offset is already zero never moves — so the tween that would panic on a
    /// plain signal is unreachable here.
    pub fn vertical(y: Signal<f32>, max_y: Signal<f32>) -> Self {
        Self::new(Signal::new(0.0), y, Signal::new(0.0), max_y)
    }

    /// A horizontal-only surface. The pinned axis is plain, for the reason
    /// given on [`vertical`](Self::vertical).
    pub fn horizontal(x: Signal<f32>, max_x: Signal<f32>) -> Self {
        Self::new(x, Signal::new(0.0), max_x, Signal::new(0.0))
    }

    /// Write an offset back, notifying only on a real change.
    ///
    /// `Signal::set` notifies unconditionally, and a pan delivers a sample per
    /// frame; re-dirtying a scrollable for a movement smaller than
    /// [`SCROLL_MOVE_EPSILON`](teksilo_core::SCROLL_MOVE_EPSILON) would cost a
    /// relayout per frame for a picture that cannot change.
    fn publish(&self, offset: Point) {
        if (self.x.get() - offset.x).abs() > f32::EPSILON {
            self.x.set(offset.x);
        }
        if (self.y.get() - offset.y).abs() > f32::EPSILON {
            self.y.set(offset.y);
        }
    }

    /// Write the overscroll back, notifying only on a real change.
    fn publish_overscroll(&self, overscroll: Vec2) {
        if self.overscroll.get() != overscroll {
            self.overscroll.set(overscroll);
        }
    }
}

// ---------------------------------------------------------------------------
// ScrollHandlingOptions
// ---------------------------------------------------------------------------

/// Everything [`handle_scroll_event`] needs to know that is not state.
///
/// Snapshot, not signals: a scrollable builds one of these in `build()`, where
/// the theme and the reduced-motion preference are in scope, and the handler
/// closure captures it. Both change through a rebuild, which is the level a
/// density switch and a preference change already mark.
#[derive(Clone, Copy, Debug)]
pub struct ScrollHandlingOptions {
    /// Pixels one [`ScrollDelta::Lines`] unit is worth.
    pub line_height: f32,
    /// Whether a wheel notch tweens to its target instead of jumping. Never
    /// consulted on the pan path — a finger is already the animation.
    ///
    /// With this on, every [`ScrollableAxes`] offset **that has a range** must
    /// be [`Signal::new_animated`] — `animate_to` panics on a plain signal.
    /// The qualifier is load-bearing, not a caveat: an axis whose maximum is
    /// permanently zero is never written by either path, which is exactly why
    /// [`ScrollableAxes::vertical`] and [`ScrollableAxes::horizontal`] pin
    /// their unused axis with a plain `Signal::new(0.0)` and why `ListView`,
    /// `TreeView` and `GridView` each pair `ScrollableAxes::vertical` with a
    /// `smooth_scrolling` that defaults to `true`. The rule is stated once on
    /// [`ScrollableAxes`] itself; this is the same rule.
    pub smooth: bool,
    /// How long that tween lasts.
    pub smooth_duration: Duration,
    /// Whether a boundary scroll chains outward ([`OverscrollBehavior::Chain`])
    /// or is absorbed ([`OverscrollBehavior::Contain`]).
    pub overscroll_behavior: OverscrollBehavior,
    /// Which overscroll feel this surface asks for when [`rubber_band`] is on.
    ///
    /// [`rubber_band`]: Self::rubber_band
    pub overscroll_style: OverscrollStyle,
    /// Which axes a finger may pan. An axis outside this set takes no movement
    /// from a pan (the wheel path is unaffected — a wheel has always reached
    /// every axis the range allows).
    pub axes: PanAxes,
    /// Whether this surface follows the finger past its own end.
    ///
    /// **Off by default, and that is the load-bearing default.** A band that
    /// engages absorbs the movement, so a nested list that rubber-banded at its
    /// end would never hand the gesture to the container around it. The band
    /// belongs to the outermost surface of a scroll chain; everything inside it
    /// clamps and chains, which is also what every desktop toolkit does.
    pub rubber_band: bool,
    /// Which pointer kinds may pan this surface. Defaults to
    /// [`PointerKindMask::DIRECT`] — a mouse scrolls with its wheel and must
    /// never be treated as a panning pointer.
    pub pan_devices: PointerKindMask,
    /// `prefers-reduced-motion`, snapshotted at build. Hard-clamps the band.
    pub reduced_motion: bool,
    /// The theme's scroll-physics constants. Only the rubber-band friction
    /// factor is read here — the fling's constants belong to the tree's
    /// driver, which reads them from the same tokens.
    pub physics: ScrollPhysicsTokens,
}

impl Default for ScrollHandlingOptions {
    fn default() -> Self {
        Self {
            line_height: 20.0,
            smooth: true,
            smooth_duration: SMOOTH_SCROLL_DURATION,
            overscroll_behavior: OverscrollBehavior::Chain,
            overscroll_style: OverscrollStyle::RubberBand,
            axes: PanAxes::BOTH,
            rubber_band: false,
            pan_devices: PointerKindMask::DIRECT,
            reduced_motion: false,
            physics: ScrollPhysicsTokens::DEFAULT,
        }
    }
}

impl ScrollHandlingOptions {
    /// The style the scroller is actually configured with: the requested one
    /// when the surface opted into the band, a hard clamp otherwise.
    fn effective_style(&self) -> OverscrollStyle {
        if self.rubber_band {
            self.overscroll_style
        } else {
            OverscrollStyle::Clamp
        }
    }
}

// ---------------------------------------------------------------------------
// handle_scroll_event
// ---------------------------------------------------------------------------

/// Apply one scroll event to `axes`, and answer the boundary question.
///
/// `Handled` means an axis absorbed some of the movement. `Ignored` means it
/// absorbed none, which is the signal that sends the event to the next
/// container outward — along the pan claimant chain for a finger, up the
/// ordinary bubble for a wheel. [`OverscrollBehavior::Contain`] turns a
/// declined *scroll* into `Handled`; it never contains the gesture's
/// end-of-stream bookkeeping, which every claimant on a chain must see.
///
/// Returns `Ignored` unchanged for any event that is not a
/// [`WidgetEvent::Scroll`], so a caller can chain its own arms after it.
pub fn handle_scroll_event(
    event: &WidgetEvent,
    axes: &ScrollableAxes,
    scroller: &Rc<RefCell<KineticScroller>>,
    options: &ScrollHandlingOptions,
    ctx: &mut EventContext,
) -> EventResponse {
    let WidgetEvent::Scroll {
        delta,
        phase,
        window_position,
        pointer,
        ..
    } = event
    else {
        return EventResponse::Ignored;
    };

    let (dx, dy) = match delta {
        ScrollDelta::Lines { x, y } => (x * options.line_height, y * options.line_height),
        ScrollDelta::Pixels { x, y } => (*x, *y),
    };

    if ctx.scroll_source() == ScrollSource::TouchPan {
        pan_step(
            axes,
            scroller,
            options,
            *phase,
            Vec2::new(dx, dy),
            // Window-space on purpose: `KineticScroller::pan`'s tracker follows
            // the pointer, and a frame that moved with the widget being
            // measured would fold that widget's own motion into the velocity.
            window_position.unwrap_or(Point::ZERO),
            pointer.time,
        )
    } else {
        wheel_step(axes, options, dx, dy)
    }
}

/// The pre-touch path, preserved exactly.
///
/// The base is the animation **target** rather than the rendered offset, so a
/// notch that arrives mid-tween accumulates onto where the previous notch was
/// heading and a boundary reached by the tween still chains.
fn wheel_step(
    axes: &ScrollableAxes,
    options: &ScrollHandlingOptions,
    dx: f32,
    dy: f32,
) -> EventResponse {
    let max_y = axes.max_y.get();
    let max_x = axes.max_x.get();
    let cur_y = axes.y.get();
    let cur_x = axes.x.get();
    let base_y = axes.y.animation_target().unwrap_or(cur_y);
    let base_x = axes.x.animation_target().unwrap_or(cur_x);

    let (target_x, moved_x) = scroll_clamp_axis(base_x, dx, max_x);
    let (target_y, moved_y) = scroll_clamp_axis(base_y, dy, max_y);

    // Per axis, and only when that axis' clamp actually moved. Writing an
    // unmoved axis costs a notification for a value that did not change —
    // which every scrollable in this crate but `ScrollArea` guarded against by
    // hand, two of them with the guard's reason written at the site. It would
    // also put a tween on an axis a surface may legitimately keep as a plain
    // signal, where `animate_to` panics.
    if moved_x {
        if options.smooth {
            axes.x
                .animate_to(target_x, options.smooth_duration, Easing::EaseOut);
        } else {
            axes.x.set(target_x);
        }
    }
    if moved_y {
        if options.smooth {
            axes.y
                .animate_to(target_y, options.smooth_duration, Easing::EaseOut);
        } else {
            axes.y.set(target_y);
        }
    }

    scroll_response(
        moved_x || moved_y,
        options.overscroll_behavior == OverscrollBehavior::Contain,
    )
}

/// The finger's path: the scroller decides where the content goes, and how far
/// past the end it is being held.
fn pan_step(
    axes: &ScrollableAxes,
    scroller: &Rc<RefCell<KineticScroller>>,
    options: &ScrollHandlingOptions,
    phase: ScrollPhase,
    delta: Vec2,
    position: Point,
    time: EventTime,
) -> EventResponse {
    let contain = options.overscroll_behavior == OverscrollBehavior::Contain;
    let mut s = scroller.borrow_mut();

    s.set_reduced_motion(options.reduced_motion);
    s.set_range_x(0.0, axes.max_x.get());
    s.set_range_y(0.0, axes.max_y.get());

    // Re-seed from the signals when somebody else moved the offset — a scroll
    // bar drag, an `ensure_visible`, a keyboard page. Comparing against what
    // the scroller last published rather than assigning unconditionally is
    // what lets a rubber band accumulate across samples: `set_offset` clamps,
    // so an unconditional re-seed would erase the overscroll every frame.
    let published = s.offset();
    let (sx, sy) = (axes.x.get(), axes.y.get());
    if (published.x - sx).abs() > f32::EPSILON || (published.y - sy).abs() > f32::EPSILON {
        s.set_offset(Point::new(sx, sy));
    }

    // An axis this surface does not pan on takes nothing, so the event chains
    // on it. `TouchAction` has already filtered what the *gesture* may do; this
    // is the surface's own narrower say (a horizontal tab strip inside a
    // vertical list claims X only).
    let dx = if options.axes.contains(Axis::X) {
        delta.x
    } else {
        0.0
    };
    let dy = if options.axes.contains(Axis::Y) {
        delta.y
    } else {
        0.0
    };

    match phase {
        // The gesture is over. Release the band — the content returns to a
        // legal offset and the overscroll signal to zero — and decline, so the
        // walk carries the same `Ended` to every claimant outward. An end of
        // stream is bookkeeping, not movement: a claimant that answered
        // `Handled` here would leave the containers around it holding a band
        // nobody ever told them to let go of, and `Contain` has nothing to
        // contain.
        ScrollPhase::Ended | ScrollPhase::MomentumEnded | ScrollPhase::Cancelled => {
            let settled = s.offset();
            s.set_offset(settled);
            drop(s);
            axes.publish(settled);
            axes.publish_overscroll(Vec2::ZERO);
            EventResponse::Ignored
        }
        // A coast. The tree's `FlingDriver` integrates an *unbounded*
        // simulation and hands out its per-tick deltas, so the boundary is
        // enforced here, with a hard clamp and no band: a coast that reaches
        // the end must decline and chain, not slide on with decreasing gain.
        ScrollPhase::Fling | ScrollPhase::Momentum => {
            let base = s.offset();
            let (nx, moved_x) = scroll_clamp_axis(base.x, dx, axes.max_x.get());
            let (ny, moved_y) = scroll_clamp_axis(base.y, dy, axes.max_y.get());
            let settled = Point::new(nx, ny);
            s.set_offset(settled);
            drop(s);
            axes.publish(settled);
            axes.publish_overscroll(Vec2::ZERO);
            scroll_response(moved_x || moved_y, contain)
        }
        // The finger is down and moving.
        _ => {
            let step = s.pan(time, position, Vec2::new(dx, dy));
            drop(s);
            axes.publish(step.offset);
            axes.publish_overscroll(step.overscroll);
            scroll_response(step.absorbed_any(), contain)
        }
    }
}

/// Rewrite a Shift+wheel notch into a horizontal one, or decline.
///
/// A vertical-only wheel held with Shift scrolls a horizontally-scrollable
/// surface sideways — the convention `TabBar` established in this crate and
/// every desktop toolkit shares. The transform is a *delta* rewrite, which
/// [`handle_scroll_event`] cannot express because it reads the delta off the
/// event; a surface that wants it builds the rewritten event here and hands
/// that to the shared handler from its [`ScrollableBehavior::before`] arm, so
/// the arithmetic is still written once.
///
/// Declines — returning `None`, meaning "no remap, treat this event as it
/// came" — for anything but a wheel-family [`WidgetEvent::Scroll`] held with
/// Shift whose horizontal component is zero. Two of those clauses carry the
/// rule rather than the example:
///
/// * A delta with a real horizontal component is a trackpad's own two-axis
///   stream, and rewriting it would throw the axis the user actually moved.
/// * A [`ScrollSource::TouchPan`] is never remapped. A finger has no Shift
///   key, so the modifier could only arrive from a keyboard held during a
///   pan, and turning that pan sideways is not what the hand asked for.
pub fn shift_wheel_remap(event: &WidgetEvent, ctx: &EventContext) -> Option<WidgetEvent> {
    let WidgetEvent::Scroll {
        delta,
        modifiers,
        window_position,
        phase,
        pointer,
    } = event
    else {
        return None;
    };
    if !modifiers.shift() || ctx.scroll_source() == ScrollSource::TouchPan {
        return None;
    }
    let remapped = match delta {
        ScrollDelta::Lines { x, y } if x.abs() < f32::EPSILON => {
            ScrollDelta::Lines { x: *y, y: 0.0 }
        }
        ScrollDelta::Pixels { x, y } if x.abs() < f32::EPSILON => {
            ScrollDelta::Pixels { x: *y, y: 0.0 }
        }
        _ => return None,
    };
    Some(WidgetEvent::Scroll {
        delta: remapped,
        modifiers: *modifiers,
        window_position: *window_position,
        phase: *phase,
        pointer: *pointer,
    })
}

// ---------------------------------------------------------------------------
// ScrollableBehavior
// ---------------------------------------------------------------------------

/// The whole of what a widget must do to become scrollable, as one value it
/// installs onto its [`HandlerSet`].
///
/// Two halves, and both matter. The `on_scroll` handler is the arithmetic;
/// the [`PanClaim`] is what puts the node on the claimant chain a synthesised
/// pan walks. Installing one without the other yields a surface that scrolls on
/// a wheel and ignores a finger, which is exactly the state this crate was in
/// before this module.
pub struct ScrollableBehavior {
    axes: ScrollableAxes,
    scroller: Rc<RefCell<KineticScroller>>,
    options: ScrollHandlingOptions,
    #[allow(clippy::type_complexity)]
    before: Option<Rc<dyn Fn(&WidgetEvent, &mut EventContext) -> Option<EventResponse>>>,
    #[allow(clippy::type_complexity)]
    after: Option<Rc<dyn Fn(&WidgetEvent, EventResponse, &mut EventContext)>>,
}

impl std::fmt::Debug for ScrollableBehavior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollableBehavior")
            .field("options", &self.options)
            .field("has_before", &self.before.is_some())
            .field("has_after", &self.after.is_some())
            .finish()
    }
}

impl ScrollableBehavior {
    /// A behaviour over `axes`, with a scroller of its own.
    ///
    /// A surface that must reach the scroller from its layout pass (to publish
    /// its viewport, which is what the rubber-band curve is a fraction of)
    /// keeps its own handle and passes it to
    /// [`with_scroller`](Self::with_scroller) instead, so the physics survives
    /// a rebuild.
    pub fn new(axes: ScrollableAxes) -> Self {
        Self {
            axes,
            scroller: Rc::new(RefCell::new(KineticScroller::new(OverscrollStyle::Clamp))),
            options: ScrollHandlingOptions::default(),
            before: None,
            after: None,
        }
    }

    /// Use the caller's scroller rather than the one [`new`](Self::new) made.
    pub fn with_scroller(mut self, scroller: Rc<RefCell<KineticScroller>>) -> Self {
        self.scroller = scroller;
        self
    }

    /// Which axes a finger may pan.
    pub fn axes(mut self, axes: PanAxes) -> Self {
        self.options.axes = axes;
        self
    }

    /// Whether a boundary scroll chains outward or is absorbed.
    pub fn overscroll(mut self, behavior: OverscrollBehavior) -> Self {
        self.options.overscroll_behavior = behavior;
        self
    }

    /// Follow the finger past the end with decreasing gain. See
    /// [`ScrollHandlingOptions::rubber_band`] for why this is off by default.
    pub fn rubber_band(mut self, on: bool) -> Self {
        self.options.rubber_band = on;
        self
    }

    /// Which overscroll feel to use when the band is on.
    pub fn overscroll_style(mut self, style: OverscrollStyle) -> Self {
        self.options.overscroll_style = style;
        self
    }

    /// Whether a wheel notch tweens to its target.
    pub fn smooth(mut self, on: bool) -> Self {
        self.options.smooth = on;
        self
    }

    /// How long that tween lasts.
    pub fn smooth_duration(mut self, duration: Duration) -> Self {
        self.options.smooth_duration = duration;
        self
    }

    /// Pixels one line of a [`ScrollDelta::Lines`] notch is worth.
    pub fn line_height(mut self, pixels: f32) -> Self {
        self.options.line_height = pixels;
        self
    }

    /// Which pointer kinds may pan this surface.
    pub fn pan_devices(mut self, devices: PointerKindMask) -> Self {
        self.options.pan_devices = devices;
        self
    }

    /// `prefers-reduced-motion`, read from the build context.
    pub fn reduced_motion(mut self, reduced: bool) -> Self {
        self.options.reduced_motion = reduced;
        self
    }

    /// The theme's scroll-physics constants, for the rubber-band curve.
    pub fn physics(mut self, physics: ScrollPhysicsTokens) -> Self {
        self.options.physics = physics;
        self
    }

    /// An arm the installed handler runs **first**, for every event.
    ///
    /// The answer is an `Option`, and the two halves of it are different
    /// questions:
    ///
    /// * `None` — "not mine". The shared treatment then runs on the **same,
    ///   unmodified** event. This is what an observing arm returns: a surface
    ///   doing per-scroll bookkeeping before the delta lands looks, records,
    ///   and declines.
    /// * `Some(r)` — "this event is mine, and `r` is the surface's answer to
    ///   it". The shared treatment does not run at all. Both a surface's own
    ///   scroll-adjacent events (`ScrollIntoView` is the usual one) and an arm
    ///   that *rewrote* the event and fed the rewrite to
    ///   [`handle_scroll_event`] itself take this branch — including when the
    ///   rewrite could not move, where the answer is `Some(Ignored)` so the
    ///   whole original event chains outward.
    ///
    /// That last case is why this is an `Option` and not an [`EventResponse`].
    /// A `Handled`-means-short-circuit rule cannot express "I consumed this
    /// event and the answer is `Ignored`", so a remapping arm — the tables'
    /// Shift+wheel — would fall through and have the shared handler apply the
    /// *original* delta on top of the remapped one it just declined.
    pub fn before(
        mut self,
        arm: impl Fn(&WidgetEvent, &mut EventContext) -> Option<EventResponse> + 'static,
    ) -> Self {
        self.before = Some(Rc::new(arm));
        self
    }

    /// An arm the installed handler runs **last**, once the delta has landed.
    ///
    /// It sees the event and the answer the surface is about to give, and can
    /// change neither: contradicting the boundary answer is how a chain stops
    /// working. It is for the bookkeeping a surface can only do *after* the
    /// offset moved — asking for a repaint being the one that matters, on a
    /// surface whose offset signals are read at paint rather than bound to the
    /// node.
    ///
    /// It runs on every path, including the one where a
    /// [`before`](Self::before) arm claimed the event — a `before` arm that
    /// answers `Handled` has moved the offset itself, which is exactly the
    /// case this arm exists to notice.
    pub fn after(
        mut self,
        arm: impl Fn(&WidgetEvent, EventResponse, &mut EventContext) + 'static,
    ) -> Self {
        self.after = Some(Rc::new(arm));
        self
    }

    /// The scroller this behaviour will use, for a surface that must publish
    /// its viewport into it from layout.
    pub fn scroller(&self) -> Rc<RefCell<KineticScroller>> {
        self.scroller.clone()
    }

    /// The options this behaviour resolved to. Exposed for a surface that
    /// wants to answer the same boundary question from a second call site
    /// (a keyboard page, an AT scroll action).
    pub fn options(&self) -> ScrollHandlingOptions {
        self.options
    }

    /// Attach the pan claim and the scroll handler to `handlers`.
    pub fn install(self, handlers: HandlerSet) -> HandlerSet {
        let Self {
            axes,
            scroller,
            options,
            before,
            after,
        } = self;

        // A scroller is built around its style, so the resolved style is
        // stamped in here — at build, where the theme and the preference that
        // decide it are in scope — rather than re-asserted per event. The
        // handle is the caller's, so a surface that publishes its viewport
        // from layout keeps writing to the right object; the range and the
        // offset are re-read from the signals on the next sample either way.
        {
            let mut s = scroller.borrow_mut();
            *s = KineticScroller::with_tokens(options.effective_style(), &options.physics);
            s.set_reduced_motion(options.reduced_motion);
        }

        let handlers = if options.axes == PanAxes::NONE {
            handlers
        } else {
            handlers.pan_claim(PanClaim {
                axes: options.axes,
                devices: options.pan_devices,
                kinetic: true,
            })
        };

        handlers.on_scroll(move |event, ctx| {
            // `before` answering `Some` means it OWNS this event: the shared
            // treatment is skipped whatever the answer is. Skipping only on
            // `Handled` would run the shared handler on the original event
            // after a remapping arm had already consumed it — which is how a
            // Shift+wheel notch a `Chain` table could not absorb sideways
            // ended up scrolling its rows vertically as well. (`Ignored` is
            // the boundary answer only under `Chain`; a `Contain` surface
            // answered `Handled` and short-circuited even before this.)
            let response = match before.as_ref().and_then(|before| before(event, ctx)) {
                Some(response) => response,
                None => handle_scroll_event(event, &axes, &scroller, &options, ctx),
            };
            if let Some(after) = &after {
                after(event, response, ctx);
            }
            response
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_canvas::{Size, SizeProposal};
    use teksilo_core::build_context::BuildContext;
    use teksilo_core::event::Modifiers;
    use teksilo_core::pointer::clock::ManualClock;
    use teksilo_core::pointer::{
        BackendDeviceKey, PointerId, PointerIdAllocator, PointerInfo, PointerPhase, PointerSample,
    };
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_core::widget_id::WidgetId;
    use teksilo_core::widget_tree::WidgetTree;

    // -- fixture ---------------------------------------------------------

    /// A minimal scrollable: nothing but a [`ScrollableBehavior`] on a leaf
    /// that fills whatever it is proposed. Everything a real surface adds —
    /// content, bars, viewport metrics — is beside the point here.
    #[derive(Debug)]
    struct Surface {
        axes: ScrollableAxes,
        scroller: Rc<RefCell<KineticScroller>>,
        options: ScrollHandlingOptions,
        pan_axes: PanAxes,
        viewport: f32,
        /// One already-registered child, for the nested fixture.
        child: Option<WidgetId>,
    }

    impl Surface {
        fn new(axes: ScrollableAxes) -> Self {
            Self {
                axes,
                scroller: Rc::new(RefCell::new(KineticScroller::new(OverscrollStyle::Clamp))),
                options: ScrollHandlingOptions {
                    smooth: false,
                    ..Default::default()
                },
                pan_axes: PanAxes::BOTH,
                viewport: 200.0,
                child: None,
            }
        }
    }

    impl Widget for Surface {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let behavior = ScrollableBehavior::new(self.axes.clone())
                .with_scroller(self.scroller.clone())
                .axes(self.pan_axes)
                .overscroll(self.options.overscroll_behavior)
                .rubber_band(self.options.rubber_band)
                .overscroll_style(self.options.overscroll_style)
                .smooth(self.options.smooth)
                .smooth_duration(self.options.smooth_duration)
                .line_height(self.options.line_height)
                .pan_devices(self.options.pan_devices)
                .reduced_motion(self.options.reduced_motion);
            ctx.apply_self_handlers(behavior.install(HandlerSet::new()));
            self.scroller
                .borrow_mut()
                .set_viewport(Vec2::new(self.viewport, self.viewport));
            self.child.into_iter().collect()
        }

        fn children(&self) -> Vec<WidgetId> {
            self.child.into_iter().collect()
        }

        fn place_children(
            &self,
            bounds: teksilo_canvas::Rect,
            _proposal: SizeProposal,
            children: &mut [teksilo_core::widget::WidgetPlacement],
            _ctx: &LayoutContext,
        ) {
            for child in children.iter_mut() {
                child.origin = bounds.origin();
                child.size = bounds.size();
            }
        }

        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            Size::new(
                proposal.width.unwrap_or(self.viewport),
                proposal.height.unwrap_or(self.viewport),
            )
            .into()
        }
    }

    /// A tree holding one `Surface`, laid out 200 × 200 with the pointer parked
    /// inside it so a positionless wheel event has somewhere to go.
    struct Fixture {
        tree: WidgetTree,
        id: WidgetId,
        axes: ScrollableAxes,
    }

    fn fixture(build: impl FnOnce(&mut Surface)) -> Fixture {
        let axes = ScrollableAxes::vertical(Signal::new_animated(0.0), Signal::new(1000.0));
        let mut surface = Surface::new(axes.clone());
        build(&mut surface);
        let mut tree = WidgetTree::new();
        let id = tree.add(surface);
        tree.layout(SizeProposal::exact(200.0, 200.0));
        tree.pointer_move(Point::new(100.0, 100.0));
        Fixture { tree, id, axes }
    }

    /// Two `Surface`s, one inside the other, so a boundary answer is
    /// observable as movement on the container rather than as a return value
    /// no public API hands back.
    struct Nested {
        tree: WidgetTree,
        inner: ScrollableAxes,
        outer: ScrollableAxes,
    }

    fn nested(inner_max: f32, outer_max: f32, contain: bool) -> Nested {
        let inner_axes =
            ScrollableAxes::vertical(Signal::new_animated(0.0), Signal::new(inner_max));
        let outer_axes =
            ScrollableAxes::vertical(Signal::new_animated(0.0), Signal::new(outer_max));
        let mut inner = Surface::new(inner_axes.clone());
        if contain {
            inner.options.overscroll_behavior = OverscrollBehavior::Contain;
        }
        let mut outer = Surface::new(outer_axes.clone());
        let mut tree = WidgetTree::new();
        let inner_id = tree.add(inner);
        outer.child = Some(inner_id);
        let outer_id = tree.add(outer);
        let _ = outer_id;
        tree.layout(SizeProposal::exact(200.0, 200.0));
        tree.pointer_move(Point::new(100.0, 100.0));
        Nested {
            tree,
            inner: inner_axes,
            outer: outer_axes,
        }
    }

    fn wheel(f: &mut Fixture, dy: f32) {
        f.tree.dispatch_event(WidgetEvent::scroll(
            ScrollDelta::Pixels { x: 0.0, y: dy },
            Modifiers::NONE,
        ));
    }

    fn contact_id(raw: u64) -> PointerId {
        let alloc = PointerIdAllocator::global();
        let device = BackendDeviceKey::new(0x5C40);
        let id = alloc.begin(device, raw);
        alloc.end(device, raw);
        id
    }

    fn contact(id: PointerId, phase: PointerPhase, at: Point) -> PointerSample {
        PointerSample {
            pointer: PointerInfo::touch(id, EventTime::ZERO),
            phase,
            position: at,
            button: None,
            modifiers: Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    fn pan_slop() -> f32 {
        teksilo_core::gesture::default_profile(teksilo_tokens::PointerKind::Touch)
            .pan_slop
            .expect("a touch profile pans")
    }

    /// Press at `from` and drag the finger by `dy`, crossing the pan slop
    /// first so the claim is taken. Returns the finger's final position.
    fn drag(tree: &mut WidgetTree, id: PointerId, from: Point, dy: f32) -> Point {
        tree.dispatch_pointer(contact(id, PointerPhase::Down, from));
        let arm = Point::new(from.x, from.y + pan_slop().copysign(dy) + dy.signum());
        tree.dispatch_pointer(contact(id, PointerPhase::Move, arm));
        let at = Point::new(from.x, arm.y + (dy - (arm.y - from.y)));
        tree.dispatch_pointer(contact(id, PointerPhase::Move, at));
        at
    }

    // -- the wheel path --------------------------------------------------

    /// A line notch is worth the line height; a pixel notch is itself.
    #[test]
    fn a_line_notch_is_worth_the_line_height_and_a_pixel_notch_is_itself() {
        let mut f = fixture(|s| s.options.line_height = 17.0);
        f.tree.dispatch_event(WidgetEvent::scroll(
            ScrollDelta::Lines { x: 0.0, y: 3.0 },
            Modifiers::NONE,
        ));
        assert_eq!(f.axes.y.get(), 51.0, "3 lines × 17 dp");
        wheel(&mut f, 9.0);
        assert_eq!(f.axes.y.get(), 60.0, "a pixel delta is not scaled");
    }

    /// A wheel event at a hard boundary is declined, so it bubbles to the
    /// container around it; `Contain` absorbs the same event and the container
    /// never sees it. Nothing moves inside either way.
    #[test]
    fn a_clamped_wheel_chains_unless_contained() {
        let mut n = nested(100.0, 1000.0, false);
        n.tree.dispatch_event(WidgetEvent::scroll(
            ScrollDelta::Pixels { x: 0.0, y: 500.0 },
            Modifiers::NONE,
        ));
        assert_eq!(n.inner.y.get(), 100.0, "the inner surface reached its end");
        n.tree.dispatch_event(WidgetEvent::scroll(
            ScrollDelta::Pixels { x: 0.0, y: 50.0 },
            Modifiers::NONE,
        ));
        assert_eq!(
            n.outer.y.get(),
            50.0,
            "…and the next notch went to the container"
        );

        let mut n = nested(100.0, 1000.0, true);
        n.tree.dispatch_event(WidgetEvent::scroll(
            ScrollDelta::Pixels { x: 0.0, y: 500.0 },
            Modifiers::NONE,
        ));
        n.tree.dispatch_event(WidgetEvent::scroll(
            ScrollDelta::Pixels { x: 0.0, y: 50.0 },
            Modifiers::NONE,
        ));
        assert_eq!(
            n.inner.y.get(),
            100.0,
            "Contain absorbs, it does not scroll"
        );
        assert_eq!(n.outer.y.get(), 0.0, "…and nothing reaches the container");
    }

    /// The same rule for a finger, along the claimant chain: the whole event
    /// goes outward at the boundary, with no residual left behind.
    #[test]
    fn a_boundary_pan_hands_the_whole_event_outward() {
        let mut n = nested(0.0, 1000.0, false);
        drag(&mut n.tree, contact_id(20), Point::new(100.0, 150.0), -60.0);
        assert_eq!(
            n.inner.y.get(),
            0.0,
            "the inner surface had nothing to give"
        );
        assert!(
            n.outer.y.get() > 0.0,
            "so the container took the pan: {}",
            n.outer.y.get()
        );
    }

    /// A smooth notch aims a tween, and the next notch accumulates onto that
    /// target rather than onto the frame the tween has reached — which is what
    /// makes a fast series of notches travel the sum of its deltas.
    #[test]
    fn smooth_notches_accumulate_on_the_animation_target() {
        let mut f = fixture(|s| s.options.smooth = true);
        wheel(&mut f, 40.0);
        assert_eq!(f.axes.y.animation_target(), Some(40.0));
        wheel(&mut f, 40.0);
        assert_eq!(
            f.axes.y.animation_target(),
            Some(80.0),
            "the second notch aims past the first"
        );
    }

    // -- the pan path ----------------------------------------------------

    /// A finger scrolls the surface, and the content moves against the finger.
    #[test]
    fn a_finger_pans_the_surface() {
        let mut f = fixture(|_| {});
        drag(&mut f.tree, contact_id(1), Point::new(100.0, 150.0), -60.0);
        assert!(
            f.axes.y.get() > 0.0,
            "dragging the finger up scrolls down: {}",
            f.axes.y.get()
        );
    }

    /// A pan never tweens, whatever `smooth` says: the content is under the
    /// finger, so it is already the animation.
    #[test]
    fn a_pan_never_tweens() {
        let mut f = fixture(|s| s.options.smooth = true);
        drag(&mut f.tree, contact_id(2), Point::new(100.0, 150.0), -60.0);
        assert!(f.axes.y.get() > 0.0);
        assert_eq!(f.axes.y.animation_target(), None);
    }

    /// A fast release hands off to the tree's coast, and the coast keeps the
    /// surface moving after the finger is gone.
    #[test]
    fn a_release_flings_and_the_coast_keeps_scrolling() {
        let mut f = fixture(|_| {});
        let clock = Rc::new(ManualClock::new(EventTime::ZERO));
        f.tree.set_input_clock(clock.clone());

        let finger = contact_id(3);
        let from = Point::new(100.0, 180.0);
        f.tree
            .dispatch_pointer(contact(finger, PointerPhase::Down, from));
        let mut y = from.y;
        for step in 1..=5 {
            clock.set(EventTime::from_millis(step * 4));
            y -= 20.0;
            f.tree
                .dispatch_pointer(contact(finger, PointerPhase::Move, Point::new(from.x, y)));
        }
        clock.set(EventTime::from_millis(24));
        f.tree
            .dispatch_pointer(contact(finger, PointerPhase::Up, Point::new(from.x, y)));

        assert!(f.tree.is_flinging(f.id), "a fast release starts a coast");
        let at_release = f.axes.y.get();
        f.tree.advance_time(Duration::from_millis(100));
        assert!(
            f.axes.y.get() > at_release,
            "the coast moved the surface: {at_release} -> {}",
            f.axes.y.get()
        );
    }

    /// Reduced motion turns the coast off outright: the surface stops exactly
    /// where the finger left it.
    #[test]
    fn reduced_motion_collapses_the_fling_to_where_the_finger_left_it() {
        let mut f = fixture(|s| s.options.reduced_motion = true);
        f.tree.set_accessibility_preferences(false, true, 1.0);
        let clock = Rc::new(ManualClock::new(EventTime::ZERO));
        f.tree.set_input_clock(clock.clone());

        let finger = contact_id(4);
        let from = Point::new(100.0, 180.0);
        f.tree
            .dispatch_pointer(contact(finger, PointerPhase::Down, from));
        let mut y = from.y;
        for step in 1..=5 {
            clock.set(EventTime::from_millis(step * 4));
            y -= 20.0;
            f.tree
                .dispatch_pointer(contact(finger, PointerPhase::Move, Point::new(from.x, y)));
        }
        clock.set(EventTime::from_millis(24));
        f.tree
            .dispatch_pointer(contact(finger, PointerPhase::Up, Point::new(from.x, y)));

        assert!(!f.tree.is_flinging(f.id), "reduced motion starts no coast");
        let at_release = f.axes.y.get();
        f.tree.advance_time(Duration::from_millis(200));
        assert_eq!(
            f.axes.y.get(),
            at_release,
            "and nothing moves it afterwards"
        );
    }

    /// With the band on, a pan past the end keeps following the finger with
    /// decreasing gain, publishes the excess as overscroll — and the offset
    /// itself never leaves the range. The release puts it back.
    #[test]
    fn the_rubber_band_holds_past_the_end_and_releases_on_the_lift() {
        let mut f = fixture(|s| {
            s.axes.max_y.set(100.0);
            s.options.rubber_band = true;
            s.options.overscroll_style = OverscrollStyle::RubberBand;
        });
        f.axes.y.set(100.0);

        let finger = contact_id(5);
        let at = drag(&mut f.tree, finger, Point::new(100.0, 180.0), -80.0);
        assert_eq!(f.axes.y.get(), 100.0, "the offset stays inside the range");
        let held = f.axes.overscroll.get().y;
        assert!(held > 0.0, "the band is holding the content past the end");
        assert!(
            held < 80.0,
            "…with decreasing gain, so less far than the finger travelled: {held}"
        );

        f.tree
            .dispatch_pointer(contact(finger, PointerPhase::Up, at));
        assert_eq!(
            f.axes.overscroll.get(),
            Vec2::ZERO,
            "the lift releases the band"
        );
        assert_eq!(f.axes.y.get(), 100.0);
    }

    /// Reduced motion hard-clamps the band: the pan stops at the end and there
    /// is no overscroll to release.
    #[test]
    fn reduced_motion_hard_clamps_the_band() {
        let mut f = fixture(|s| {
            s.axes.max_y.set(100.0);
            s.options.rubber_band = true;
            s.options.overscroll_style = OverscrollStyle::RubberBand;
            s.options.reduced_motion = true;
        });
        f.axes.y.set(100.0);
        drag(&mut f.tree, contact_id(6), Point::new(100.0, 180.0), -80.0);
        assert_eq!(f.axes.overscroll.get(), Vec2::ZERO);
        assert_eq!(f.axes.y.get(), 100.0);
    }

    /// An axis the surface does not claim takes nothing from a finger.
    #[test]
    fn a_pan_on_an_unclaimed_axis_takes_nothing() {
        let mut f = fixture(|s| s.pan_axes = PanAxes::X);
        drag(&mut f.tree, contact_id(7), Point::new(100.0, 150.0), -60.0);
        assert_eq!(f.axes.y.get(), 0.0, "the vertical axis is not this one's");
    }

    /// A mouse is not a panning pointer. It has no `pan_slop` at all, so a
    /// press-and-drag with a mouse button scrolls nothing — the wheel is its
    /// scroll device.
    #[test]
    fn a_mouse_drag_does_not_pan() {
        let mut f = fixture(|_| {});
        f.tree.dispatch_event(WidgetEvent::pointer_down(
            Point::new(100.0, 150.0),
            teksilo_core::event::PointerButton::Primary,
            Modifiers::NONE,
        ));
        f.tree.pointer_move(Point::new(100.0, 60.0));
        assert_eq!(f.axes.y.get(), 0.0);
    }

    /// An offset moved by somebody else — a scroll bar, an `ensure_visible` —
    /// is picked up by the next pan sample rather than being overwritten by
    /// the stale position the scroller was still holding.
    #[test]
    fn an_externally_moved_offset_is_picked_up_by_the_next_pan() {
        let mut f = fixture(|_| {});
        let finger = contact_id(8);
        let at = drag(&mut f.tree, finger, Point::new(100.0, 150.0), -40.0);
        let after_pan = f.axes.y.get();
        assert!(after_pan > 0.0);

        // A scroll bar drag writes the shared signal directly.
        f.axes.y.set(500.0);
        f.tree.dispatch_pointer(contact(
            finger,
            PointerPhase::Move,
            Point::new(at.x, at.y - 10.0),
        ));
        assert!(
            f.axes.y.get() > 500.0,
            "the pan continued from where the bar left it, not from {after_pan}"
        );
    }

    // -- install ---------------------------------------------------------

    /// `install` attaches both halves. Without the claim a finger has nothing
    /// to catch, so the proof is that a real contact scrolls the surface —
    /// which is exactly what the two pan tests above already exercise. What
    /// this one pins is the *other* direction: a surface that claims no axis
    /// stays wheel-scrollable and catches no finger.
    #[test]
    fn a_surface_with_no_claim_still_scrolls_on_a_wheel() {
        let mut f = fixture(|s| s.pan_axes = PanAxes::NONE);
        wheel(&mut f, 40.0);
        assert_eq!(f.axes.y.get(), 40.0);
        drag(&mut f.tree, contact_id(9), Point::new(100.0, 150.0), -60.0);
        assert_eq!(f.axes.y.get(), 40.0, "no claim, no pan");
    }

    /// The `before` arm runs first for every event: `Handled` short-circuits,
    /// `Ignored` observes and falls through.
    #[test]
    fn the_before_arm_observes_then_falls_through() {
        #[derive(Debug)]
        struct Observed {
            axes: ScrollableAxes,
            seen: Rc<Cell<usize>>,
        }
        impl Widget for Observed {
            fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
                let seen = self.seen.clone();
                let behavior = ScrollableBehavior::new(self.axes.clone())
                    .smooth(false)
                    .before(move |event, _ctx| {
                        if matches!(event, WidgetEvent::Scroll { .. }) {
                            seen.set(seen.get() + 1);
                        }
                        // Observes and declines, so the shared handler still
                        // runs on this event.
                        None
                    });
                ctx.apply_self_handlers(behavior.install(HandlerSet::new()));
                Vec::new()
            }
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> LayoutResponse {
                Size::new(
                    proposal.width.unwrap_or(200.0),
                    proposal.height.unwrap_or(200.0),
                )
                .into()
            }
        }

        use std::cell::Cell;
        let axes = ScrollableAxes::vertical(Signal::new_animated(0.0), Signal::new(1000.0));
        let seen = Rc::new(Cell::new(0));
        let mut tree = WidgetTree::new();
        tree.add(Observed {
            axes: axes.clone(),
            seen: seen.clone(),
        });
        tree.layout(SizeProposal::exact(200.0, 200.0));
        tree.pointer_move(Point::new(100.0, 100.0));
        tree.dispatch_event(WidgetEvent::scroll(
            ScrollDelta::Pixels { x: 0.0, y: 25.0 },
            Modifiers::NONE,
        ));
        assert_eq!(seen.get(), 1, "the arm saw the scroll");
        assert_eq!(axes.y.get(), 25.0, "observing does not stop the delta");
    }

    /// The scroller handle survives `install`: a viewport published from the
    /// surface's own layout reaches the object the handler will read.
    #[test]
    fn the_callers_scroller_handle_survives_install() {
        let axes = ScrollableAxes::vertical(Signal::new_animated(0.0), Signal::new(1000.0));
        let scroller = Rc::new(RefCell::new(KineticScroller::new(OverscrollStyle::Clamp)));
        let behavior = ScrollableBehavior::new(axes)
            .with_scroller(scroller.clone())
            .rubber_band(true)
            .overscroll_style(OverscrollStyle::RubberBand);
        assert!(Rc::ptr_eq(&behavior.scroller(), &scroller));
        let _ = behavior.install(HandlerSet::new());
        scroller.borrow_mut().set_range_y(0.0, 100.0);
        scroller.borrow_mut().set_offset(Point::new(0.0, 100.0));
        // The style `install` stamped in is the one the band needs: a drag past
        // the end is followed rather than refused.
        let step = scroller
            .borrow_mut()
            .pan(EventTime::ZERO, Point::ZERO, Vec2::new(0.0, 40.0));
        assert!(
            step.overscroll.y > 0.0,
            "install kept the rubber-band style"
        );
    }

    /// A rebuild with the band turned off leaves the scroller clamping, so the
    /// two knobs cannot drift apart across a rebuild.
    #[test]
    fn install_without_the_band_leaves_the_scroller_clamping() {
        let axes = ScrollableAxes::vertical(Signal::new_animated(0.0), Signal::new(1000.0));
        let scroller = Rc::new(RefCell::new(KineticScroller::new(
            OverscrollStyle::RubberBand,
        )));
        let _ = ScrollableBehavior::new(axes)
            .with_scroller(scroller.clone())
            .rubber_band(false)
            .install(HandlerSet::new());
        scroller.borrow_mut().set_range_y(0.0, 100.0);
        scroller.borrow_mut().set_offset(Point::new(0.0, 100.0));
        let step = scroller
            .borrow_mut()
            .pan(EventTime::ZERO, Point::ZERO, Vec2::new(0.0, 40.0));
        assert_eq!(step.overscroll, Vec2::ZERO);
    }
}
