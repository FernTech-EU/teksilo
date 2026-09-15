// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared per-datum mark geometry, pointer hit-testing, the readout
//! state machine, synthetic accessibility-node emission, and
//! readout-card rendering for `BarChart` / `LineChart` / `PieChart`.
//!
//! Each chart's `paint()` computes a fresh `Vec<MarkGeometry>` describing
//! every visible datum's on-screen shape (a bar rect, a line-point, or a
//! pie/donut slice). The same vector drives four consumers: pointer
//! hit-testing ([`nearest_point`] / [`rect_hit_within`] /
//! [`slice_hit_within`]), the shared readout card
//! ([`draw_mark_tooltip`]), keyboard traversal ([`stepped_mark`]), and
//! per-datum AT nodes ([`emit_mark_node`]). `(series_id, point_idx)` —
//! [`MarkKey`] — is the natural key into a
//! [`teksilo_data::ChartSelection`], so no separate lookup structure is
//! needed.
//!
//! # The readout, and who retires it
//!
//! A chart's readout — the tooltip card plus the highlighted mark — is
//! keyed by one `Signal<Option<MarkKey>>` per chart, and every input
//! route writes that one signal. [`drive_readout`] owns the pointer
//! routes and [`drive_readout_keys`] the keyboard ones, so the three
//! chart kinds differ only in how a point resolves to a mark.
//!
//! The pointer rules are **kind-aware**, and the reason is that a finger
//! has no way to leave. A precise pointer sets the readout as it moves
//! and clears it by moving off or leaving the widget, which is what
//! `PointerLeave` is for; a contact never receives a `PointerLeave`
//! anywhere in the framework, so a readout a finger raised would stay up
//! for the rest of the program. So a coarse pointer gets two modes and
//! both of them end:
//!
//! * **Scrub** — press, then travel. The readout follows the contact for
//!   as long as the contact is down, and the lift retires it.
//! * **Pin** — press and release without travelling. The readout stays
//!   on the mark under the release, re-anchored to the mark now that the
//!   finger has gone, and is announced once. The next press retires or
//!   replaces it.
//!
//! A cancel retires it either way, because a revoked interaction is not
//! an inspection — for a chart the cancel funnel reaches, which is one
//! that holds the pointer. The readout claims no event on purpose, so a
//! chart with neither a selection nor any other pointer-facing handler is
//! not told, and the next press is what retires the stranded readout;
//! `docs/charts.md` §9 records that, and `bar_chart.rs` pins both halves.
//!
//! There is no timeout: nothing on `EventContext` reports the clock, a
//! chart runs no timer of its own, and with press, release and cancel all
//! owning a retire path a timeout would only ever fire against a readout
//! the user is still looking at.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Canvas, Point, Rect, StrokeStyle};
use teksilo_core::accessibility::{AccessNodeBuilder, SyntheticKind};
use teksilo_core::accesskit;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::pointer::hit_slop::HitSlop;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{BorderRecipe, BorderStyle, Theme};
use teksilo_core::widget::EventContext;
use teksilo_core::widget_id::WidgetId;
use teksilo_data::{ChartSelection, SeriesId};
use teksilo_tokens::{CornerRadius, InputTokens, PointerKind, TextStyle};

use crate::style as cs;
use crate::text::measure_text_width;

/// The identity of one datum: which series, which point in it.
///
/// Every readout, selection and AT node in the crate is keyed by this
/// pair rather than by an index into the paint-order mark vector, which
/// is rebuilt from scratch on every paint.
pub type MarkKey = (SeriesId, usize);

/// One datum's on-screen shape + identity, recomputed fresh on every
/// paint by every chart kind.
#[derive(Debug, Clone)]
pub struct MarkGeometry {
    pub series_id: SeriesId,
    pub point_idx: usize,
    pub series_name: String,
    pub category_label: String,
    pub value: f32,
    pub shape: MarkShape,
}

/// The on-screen shape of one mark. All coordinates are window-space
/// (the same space `paint()` draws into).
#[derive(Debug, Clone, Copy)]
pub enum MarkShape {
    /// A line-chart data point.
    Point { center: Point, radius: f32 },
    /// A bar-chart bar.
    Rect(Rect),
    /// A pie/donut slice. `start_rad`/`sweep_rad` are in the same
    /// screen-space angle convention as `f32::atan2(dy, dx)` (0 = the
    /// screen +x / 3 o'clock direction, increasing angle sweeps toward
    /// +y — visually clockwise on a y-down screen). `sweep_rad` may be
    /// negative for a counter-clockwise sweep.
    Slice {
        center: Point,
        inner_radius: f32,
        outer_radius: f32,
        start_rad: f32,
        sweep_rad: f32,
    },
}

impl MarkShape {
    /// Axis-aligned bounding rect in window space. Used both for AT node
    /// bounds ([`emit_mark_node`]) and as a general-purpose fallback.
    pub fn bounding_rect(&self) -> Rect {
        match *self {
            MarkShape::Point { center, radius } => Rect::new(
                center.x - radius,
                center.y - radius,
                radius * 2.0,
                radius * 2.0,
            ),
            MarkShape::Rect(r) => r,
            MarkShape::Slice {
                center,
                inner_radius,
                outer_radius,
                start_rad,
                sweep_rad,
            } => slice_bounding_rect(center, inner_radius, outer_radius, start_rad, sweep_rad),
        }
    }
}

/// Bounding box of an annular (or pie, when `inner_radius <= 0`) wedge.
/// Samples both endpoints of the sweep plus every cardinal angle
/// (0, π/2, π, 3π/2) the sweep crosses, at both radii — the exact set of
/// points that can extend the AA bbox beyond the endpoints alone.
fn slice_bounding_rect(
    center: Point,
    inner_radius: f32,
    outer_radius: f32,
    start_rad: f32,
    sweep_rad: f32,
) -> Rect {
    let two_pi = std::f32::consts::TAU;
    // Normalize to a non-negative sweep so cardinal-angle membership is
    // a simple `s <= a <= s + sw` test.
    let (s, sw) = if sweep_rad < 0.0 {
        (start_rad + sweep_rad, -sweep_rad)
    } else {
        (start_rad, sweep_rad)
    };
    if sw >= two_pi - 1e-4 {
        // Full circle/annulus — the outer radius alone bounds it.
        return Rect::new(
            center.x - outer_radius,
            center.y - outer_radius,
            outer_radius * 2.0,
            outer_radius * 2.0,
        );
    }

    let mut angles: Vec<f32> = vec![s, s + sw];
    for k in 0..4 {
        let cardinal = k as f32 * std::f32::consts::FRAC_PI_2;
        let mut a = cardinal;
        while a < s {
            a += two_pi;
        }
        if a <= s + sw {
            angles.push(a);
        }
    }

    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    let mut include = |p: Point| {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    };
    let radii = [inner_radius.max(0.0), outer_radius.max(0.0)];
    for &a in &angles {
        for &r in &radii {
            if r <= 0.0 {
                include(center);
            } else {
                include(Point::new(center.x + r * a.cos(), center.y + r * a.sin()));
            }
        }
    }
    Rect::new(
        min_x,
        min_y,
        (max_x - min_x).max(0.0),
        (max_y - min_y).max(0.0),
    )
}

/// Nearest [`MarkShape::Point`] mark to `p` by squared distance — no
/// radius cutoff (matches `LineChart`'s pre-refactor "always pick the
/// closest point" semantics; the caller gates on plot-rect containment
/// separately).
pub fn nearest_point(marks: &[MarkGeometry], p: Point) -> Option<usize> {
    let mut best_idx = None;
    let mut best_d2 = f32::INFINITY;
    for (i, m) in marks.iter().enumerate() {
        if let MarkShape::Point { center, .. } = m.shape {
            let dx = center.x - p.x;
            let dy = center.y - p.y;
            let d2 = dx * dx + dy * dy;
            if d2 < best_d2 {
                best_d2 = d2;
                best_idx = Some(i);
            }
        }
    }
    best_idx
}

/// First [`MarkShape::Rect`] mark containing `p` (`None` in a gap
/// between bars) — strict containment, the mouse's answer.
///
/// [`rect_hit_within`] is the same test with a tolerance; this is
/// `rect_hit_within(marks, p, 0.0)`, and the tolerance-free form is what
/// the containment-first phase of that function is pinned by, so it lives
/// here rather than being inlined into its one caller.
#[cfg(test)]
pub fn rect_hit(marks: &[MarkGeometry], p: Point) -> Option<usize> {
    rect_hit_within(marks, p, 0.0)
}

/// [`MarkShape::Rect`] mark for `p`, admitting a near miss of up to
/// `tolerance` dp.
///
/// Two phases, and the order is the whole of it. **Containment first**:
/// when `p` is inside a bar, that bar wins, and the answer is identical
/// to the one a zero tolerance gives — which is what keeps a precise
/// pointer's behaviour untouched no matter what tolerance a coarse one
/// asks for. Only when nothing contains `p` does the **nearest** bar
/// within `tolerance` take it.
///
/// Nearest, rather than first: inflating every bar by a tolerance makes
/// the inflated rectangles of adjacent bars overlap, and a first-match
/// scan through an overlap resolves by position in the mark vector —
/// i.e. by series and category order — regardless of which bar the press
/// was actually closer to. A press further than `tolerance` from every
/// bar still answers `None`.
pub fn rect_hit_within(marks: &[MarkGeometry], p: Point, tolerance: f32) -> Option<usize> {
    if let Some(i) = marks
        .iter()
        .position(|m| matches!(m.shape, MarkShape::Rect(r) if r.contains(p)))
    {
        return Some(i);
    }
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return None;
    }
    let mut best: Option<(usize, f32)> = None;
    for (i, m) in marks.iter().enumerate() {
        let MarkShape::Rect(r) = m.shape else {
            continue;
        };
        let d = teksilo_core::pointer::hit_slop::rect_distance(r, p);
        if d <= tolerance && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// [`MarkShape::Slice`] mark a point inspects, given the disc it was
/// drawn in, admitting a **radial** near miss of up to `tolerance` dp.
///
/// The tolerance is radial only, and deliberately so. Inside the disc,
/// some slice owns every bearing already, so an angular tolerance would
/// widen nothing — it would only make two adjacent slices both claim
/// their shared boundary, and the first one in paint order would win it
/// at every radius. So a press near a wedge's *arc* — its outer edge, or
/// the inner edge of a donut hole — reaches the wedge, and a press whose
/// bearing lies in a neighbour's sweep reaches the neighbour however
/// close to the boundary it falls.
///
/// `outer <= 0.0` (a disc with no geometry yet) answers `None`.
pub fn slice_hit_within(
    marks: &[MarkGeometry],
    center: Point,
    inner_radius: f32,
    outer_radius: f32,
    p: Point,
    tolerance: f32,
) -> Option<usize> {
    if outer_radius <= 0.0 {
        return None;
    }
    let tol = if tolerance.is_finite() && tolerance > 0.0 {
        tolerance
    } else {
        0.0
    };
    let dx = p.x - center.x;
    let dy = p.y - center.y;
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < (inner_radius - tol).max(0.0) || dist > outer_radius + tol {
        return None;
    }
    slice_hit(marks, dy.atan2(dx))
}

/// First [`MarkShape::Slice`] mark whose sweep contains `test_angle`
/// (same screen-space angle convention as [`MarkShape::Slice`]).
///
/// Angle only — the caller owns the radial band. [`slice_hit_within`]
/// pairs the two and is what the charts call.
pub fn slice_hit(marks: &[MarkGeometry], test_angle: f32) -> Option<usize> {
    marks.iter().position(|m| {
        if let MarkShape::Slice {
            start_rad,
            sweep_rad,
            ..
        } = m.shape
        {
            let (s, sw) = if sweep_rad < 0.0 {
                (start_rad + sweep_rad, -sweep_rad)
            } else {
                (start_rad, sweep_rad)
            };
            angle_in_sweep(test_angle, s, sw)
        } else {
            false
        }
    })
}

/// Whether `angle` (in 0..2π) lies inside `[start, start + sweep]`, both
/// normalized to 0..2π. `sweep` must be non-negative — callers with a
/// signed sweep normalize before calling (see [`slice_hit`]).
pub fn angle_in_sweep(angle: f32, start: f32, sweep: f32) -> bool {
    let two_pi = std::f32::consts::TAU;
    let s = start.rem_euclid(two_pi);
    let mut e = (start + sweep).rem_euclid(two_pi);
    let a = angle.rem_euclid(two_pi);
    if (sweep - two_pi).abs() < 1e-4 {
        return true;
    }
    if e < s {
        e += two_pi;
    }
    let a_lifted = if a < s { a + two_pi } else { a };
    a_lifted >= s && a_lifted <= e
}

/// How far a **miss** may be from a mark and still inspect it, in dp,
/// for the pointer that is asking.
///
/// This is the pointer's own hit-slop radius from
/// [`HitSlop::for_pointer`] — `0` for a mouse, `2` for a pen, `8` for a
/// finger, capped by the density's `slop_budget` — so a chart mark's
/// tolerance is the same number the framework's own miss-only slop pass
/// spends on a small control, and the mouse's is zero by arithmetic
/// rather than by a branch.
///
/// A mark is not a node, which is why the tolerance is spent here rather
/// than through `Widget::hit_distance`: all of a chart's marks live
/// inside the one chart node, the slop pass's product is a node id, and
/// a press inside the chart's own rectangle is not a miss.
pub(crate) fn mark_tolerance(kind: PointerKind, tokens: &InputTokens) -> f32 {
    HitSlop::for_pointer(kind, tokens).radius
}

/// The one sentence a mark is described by — the AT node's name, and the
/// announcement a pinned readout speaks.
///
/// Shared so a screen reader hears the same words whichever route
/// reached the datum.
pub(crate) fn mark_description(m: &MarkGeometry) -> String {
    format!("{}, {}: {}", m.series_name, m.category_label, m.value)
}

/// The mark a `(series, point)` key names, by identity rather than by
/// paint-order index.
pub(crate) fn mark_index_of(marks: &[MarkGeometry], key: MarkKey) -> Option<usize> {
    marks
        .iter()
        .position(|m| m.series_id == key.0 && m.point_idx == key.1)
}

/// Which mark a synthetic AT `NodeId` belongs to, for an
/// assistive-technology action arriving at `owner`.
///
/// `push_scene_child` derives a mark's node id from
/// `(owner, mark_element_id(..), ChartMark)`, and
/// [`mark_element_id`] hashes its inputs, so the map is only invertible
/// by recomputing it over the live marks — which is what this does.
pub(crate) fn mark_for_node(
    owner: WidgetId,
    marks: &[MarkGeometry],
    node: accesskit::NodeId,
) -> Option<MarkKey> {
    marks
        .iter()
        .find(|m| {
            teksilo_core::accessibility::synthetic_node_id(
                owner,
                mark_element_id(m.series_id, m.point_idx),
                SyntheticKind::ChartMark,
            ) == node
        })
        .map(|m| (m.series_id, m.point_idx))
}

// ---------------------------------------------------------------------------
// The readout state machine
// ---------------------------------------------------------------------------

/// The part of a chart's readout that is not the key: where the card is
/// anchored, and whether the live press has travelled.
///
/// Cheap to clone into a handler closure — both fields are handles.
#[derive(Clone)]
pub(crate) struct ReadoutState {
    /// The contact the card is being held clear of, while a **coarse**
    /// pointer owns the readout. `None` anchors the card on the mark,
    /// which is what a precise pointer always does and what a pinned
    /// readout reverts to once the finger has gone.
    pub contact: Signal<Option<Point>>,
    /// Whether the live press has moved since it went down — a scrub,
    /// not a tap. Decides which of the two coarse modes the lift ends.
    pub scrubbed: Rc<Cell<bool>>,
}

impl ReadoutState {
    pub fn new() -> Self {
        Self {
            contact: Signal::new(None),
            scrubbed: Rc::new(Cell::new(false)),
        }
    }
}

/// Write `key` (or nothing) into `readout`, anchored at `contact`.
///
/// Both signals are guarded against a no-op write so a move within one
/// mark costs no repaint.
fn set_readout(
    readout: &Signal<Option<MarkKey>>,
    key: Option<MarkKey>,
    state: &ReadoutState,
    contact: Option<Point>,
) {
    if readout.get() != key {
        readout.set(key);
    }
    if state.contact.get() != contact {
        state.contact.set(contact);
    }
}

/// Drive a chart's readout from one raw pointer event.
///
/// `resolve` answers which mark, if any, a **widget-local** point
/// inspects — it is where each chart kind's own geometry lives (the plot
/// rect for bars and lines, the annulus for a pie) and where the
/// kind-aware tolerance is spent. `describe` supplies the announcement
/// for a pinned readout.
///
/// Always returns [`EventResponse::Ignored`]: the readout observes the
/// pointer stream and never consumes it, so the chart's tap, its
/// ancestors' gestures and a pan claim above it all behave as though it
/// were not installed.
///
/// See the module header for the two coarse modes and why each of them
/// ends.
pub(crate) fn drive_readout(
    event: &WidgetEvent,
    ctx: &mut EventContext,
    readout: &Signal<Option<MarkKey>>,
    state: &ReadoutState,
    resolve: impl Fn(Point) -> Option<MarkKey>,
    describe: impl Fn(MarkKey) -> Option<String>,
) -> EventResponse {
    let coarse = ctx.pointer_kind().is_coarse();
    match event {
        WidgetEvent::PointerMove { position, .. } => {
            if coarse {
                state.scrubbed.set(true);
            }
            let contact = if coarse { Some(*position) } else { None };
            set_readout(readout, resolve(*position), state, contact);
        }
        // A press starts a fresh coarse gesture: the readout appears
        // under the contact straight away, so a hold shows it without
        // waiting for a move that a still finger never produces.
        WidgetEvent::PointerDown { position, .. } if coarse => {
            state.scrubbed.set(false);
            set_readout(readout, resolve(*position), state, Some(*position));
        }
        WidgetEvent::PointerUp { position, .. } if coarse => {
            if state.scrubbed.get() {
                set_readout(readout, None, state, None);
            } else {
                let key = resolve(*position);
                // Re-anchored on the mark, not on the release point: the
                // finger has gone, and the card is about to be read.
                set_readout(readout, key, state, None);
                if let Some(text) = key.and_then(&describe) {
                    ctx.announce(text);
                }
            }
        }
        // A revoked interaction is not an inspection.
        WidgetEvent::PointerCancel { .. } => set_readout(readout, None, state, None),
        WidgetEvent::PointerLeave { .. } => set_readout(readout, None, state, None),
        _ => {}
    }
    EventResponse::Ignored
}

// ---------------------------------------------------------------------------
// Keyboard traversal
// ---------------------------------------------------------------------------

/// One step of keyboard datum traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkStep {
    Next,
    Previous,
    First,
    Last,
}

/// The traversal a bare arrow / Home / End means, or `None` for anything
/// else.
///
/// Both axes step the same single sequence — the paint-order mark vector,
/// which is series-major and point-minor — because a chart's marks are
/// not a grid: a pie has one ring, a grouped bar chart has one bar per
/// (series, category) pair, and a line chart's points are per series. One
/// sequence is the only ordering all three share, and it is the order the
/// AT tree publishes its per-datum nodes in, so the keyboard and a screen
/// reader's own review agree.
///
/// A chord with any modifier is left alone, so nothing here can shadow a
/// registered shortcut.
pub(crate) fn mark_step_for_key(event: &WidgetEvent) -> Option<MarkStep> {
    let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
        return None;
    };
    if modifiers.ctrl() || modifiers.alt() || modifiers.shift() || modifiers.super_key() {
        return None;
    }
    match key {
        Key::ArrowRight | Key::ArrowDown => Some(MarkStep::Next),
        Key::ArrowLeft | Key::ArrowUp => Some(MarkStep::Previous),
        Key::Home => Some(MarkStep::First),
        Key::End => Some(MarkStep::Last),
        _ => None,
    }
}

/// The mark `step` reaches from `current`, or `None` when there are no
/// marks.
///
/// Clamps at both ends rather than wrapping: an arrow that runs off the
/// last datum and reappears at the first loses the reader's place, and a
/// chart's sequence has no cyclic meaning even for a pie (whose first
/// slice starts at the configured start angle, not at a boundary the
/// reader can feel).
pub(crate) fn stepped_mark(
    marks: &[MarkGeometry],
    current: Option<MarkKey>,
    step: MarkStep,
) -> Option<MarkKey> {
    if marks.is_empty() {
        return None;
    }
    let last = marks.len() - 1;
    let idx = match step {
        MarkStep::First => 0,
        MarkStep::Last => last,
        MarkStep::Next => match current.and_then(|k| mark_index_of(marks, k)) {
            Some(i) => (i + 1).min(last),
            None => 0,
        },
        MarkStep::Previous => match current.and_then(|k| mark_index_of(marks, k)) {
            Some(i) => i.saturating_sub(1),
            None => last,
        },
    };
    marks.get(idx).map(|m| (m.series_id, m.point_idx))
}

/// Drive a focused chart's readout from one key event.
///
/// Arrows / Home / End move the focused datum, set the readout to it (so
/// the same card a pointer raises is what a keyboard user reads) and
/// announce it. Enter and Space commit the focused datum to `selection`
/// when the chart has one, which is the keyboard equivalent of tapping
/// the mark.
///
/// Returns [`EventResponse::Handled`] only for a key it acted on.
pub(crate) fn drive_readout_keys(
    event: &WidgetEvent,
    ctx: &mut EventContext,
    marks: &[MarkGeometry],
    readout: &Signal<Option<MarkKey>>,
    focus: &Signal<Option<MarkKey>>,
    state: &ReadoutState,
    selection: Option<&ChartSelection>,
) -> EventResponse {
    if let Some(step) = mark_step_for_key(event) {
        let Some(key) = stepped_mark(marks, focus.get(), step) else {
            return EventResponse::Ignored;
        };
        if focus.get() != Some(key) {
            focus.set(Some(key));
        }
        set_readout(readout, Some(key), state, None);
        if let Some(m) = mark_index_of(marks, key).and_then(|i| marks.get(i)) {
            ctx.announce(mark_description(m));
        }
        return EventResponse::Handled;
    }
    if let WidgetEvent::KeyDown {
        key: Key::Enter | Key::Space,
        modifiers,
        ..
    } = event
        && !modifiers.ctrl()
        && !modifiers.alt()
        && let Some(selection) = selection
        && let Some((sid, idx)) = focus.get()
    {
        selection.select_point(sid, idx);
        return EventResponse::Handled;
    }
    EventResponse::Ignored
}

/// Forget the focused datum when the chart loses focus, and take the
/// readout with it if the keyboard is what raised it.
///
/// A focus ring left painted on an unfocused chart claims a keyboard
/// position the chart no longer has.
pub(crate) fn clear_readout_focus(
    readout: &Signal<Option<MarkKey>>,
    focus: &Signal<Option<MarkKey>>,
    state: &ReadoutState,
) {
    let focused = focus.get();
    if focused.is_some() {
        focus.set(None);
        if readout.get() == focused {
            set_readout(readout, None, state, None);
        }
    }
}

/// Stable per-`(series, point)` synthetic-node element id. `SeriesId`
/// has no public raw-integer accessor (it's an opaque SlotMap key
/// wrapper, deliberately not `Ord`/numeric), so this hashes the
/// `(SeriesId, usize)` pair with `DefaultHasher` — deterministic within
/// a process run (which is all `synthetic_node_id`'s stability contract
/// requires: the same mark must keep the same AT node id across repeated
/// `accessibility()` walks in one program execution).
pub(crate) fn mark_element_id(series_id: SeriesId, point_idx: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    series_id.hash(&mut hasher);
    point_idx.hash(&mut hasher);
    hasher.finish()
}

/// Emit one synthetic `SyntheticKind::ChartMark` AT child node for `m` on
/// `builder` (the chart widget's own accessibility builder), returning
/// the node id allocated for it.
///
/// The node advertises the two actions a datum can actually answer, and
/// the chart implements both — an advertised action nothing implements is
/// worse than a missing one, because the only way to discover it does
/// nothing is to invoke it.
///
/// * [`Click`](accesskit::Action::Click) inspects the datum: the readout
///   moves to it and, where the chart has a selection, the datum is
///   selected. It is the assistive-technology equivalent of tapping the
///   mark, which no screen-reader user can aim at otherwise — the marks
///   are all one node to the hit test.
/// * [`ScrollIntoView`](accesskit::Action::ScrollIntoView) brings the
///   datum into view: the readout moves to it, and any scroll container
///   above the chart is asked to reveal the mark's own rectangle. It is
///   meaningful because a chart is frequently a tile on a scrolling page
///   even though nothing scrolls *inside* the plot.
///
/// The bounds are the mark's own logical rectangle. The device scale
/// factor is applied once, by the root window node's transform, so a
/// scale factor must never appear here.
pub(crate) fn emit_mark_node(
    builder: &mut AccessNodeBuilder,
    m: &MarkGeometry,
) -> accesskit::NodeId {
    let element_id = mark_element_id(m.series_id, m.point_idx);
    let bounds = m.shape.bounding_rect();
    let name = mark_description(m);
    builder.push_scene_child(element_id, SyntheticKind::ChartMark, |child| {
        child.set_role(accesskit::Role::GraphicsObject);
        child.set_name(name);
        child.set_numeric_value(m.value as f64);
        child.add_action(accesskit::Action::Click);
        child.add_action(accesskit::Action::ScrollIntoView);
        child.inner_mut().set_bounds(accesskit::Rect {
            x0: bounds.x as f64,
            y0: bounds.y as f64,
            x1: (bounds.x + bounds.width) as f64,
            y1: (bounds.y + bounds.height) as f64,
        });
    })
}

/// Handle a `Click` / `ScrollIntoView` action addressed to one of
/// `owner`'s per-datum AT nodes.
///
/// Returns [`EventResponse::Handled`] when `node` named a live mark and
/// the action was one of the two [`emit_mark_node`] advertises.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_mark_action(
    action: accesskit::Action,
    node: accesskit::NodeId,
    owner: WidgetId,
    ctx: &mut EventContext,
    marks: &[MarkGeometry],
    readout: &Signal<Option<MarkKey>>,
    focus: &Signal<Option<MarkKey>>,
    state: &ReadoutState,
    selection: Option<&ChartSelection>,
) -> EventResponse {
    if !matches!(
        action,
        accesskit::Action::Click | accesskit::Action::ScrollIntoView
    ) {
        return EventResponse::Ignored;
    }
    let Some(key) = mark_for_node(owner, marks, node) else {
        return EventResponse::Ignored;
    };
    if focus.get() != Some(key) {
        focus.set(Some(key));
    }
    set_readout(readout, Some(key), state, None);
    match action {
        accesskit::Action::Click => {
            if let Some(selection) = selection {
                selection.select_point(key.0, key.1);
            }
        }
        _ => {
            if let Some(m) = mark_index_of(marks, key).and_then(|i| marks.get(i)) {
                ctx.ensure_visible(m.shape.bounding_rect());
            }
        }
    }
    EventResponse::Handled
}

/// Where the readout card of `size` goes: centred over the anchor, above
/// it by a gap, flipped below when that would clip the top of `plot`, then
/// clamped into `plot` on both axes.
///
/// `contact` is the finger that raised the readout, when one did — it
/// replaces the mark as the anchor and widens the gap to clear the contact
/// patch, because the card is painted inline by the chart and so never
/// passes through the overlay layer's own contact avoidance. `None` (a
/// mouse, a pen, the keyboard, or a pinned readout whose finger has
/// lifted) anchors on the mark with the 8 dp gap the card has always used.
///
/// Pure, so the placement is testable without a canvas.
pub(crate) fn mark_tooltip_rect(
    plot: Rect,
    anchor: Point,
    contact: Option<Point>,
    size: teksilo_canvas::Size,
) -> Rect {
    let (anchor, gap) = match contact {
        Some(c) => (
            c,
            teksilo_core::overlay::ASSUMED_CONTACT_PATCH.height * 0.5 + CARD_GAP,
        ),
        None => (anchor, CARD_GAP),
    };
    let mut tx = anchor.x - size.width * 0.5;
    let mut ty = anchor.y - size.height - gap;
    if ty < plot.y {
        ty = anchor.y + gap;
    }
    if tx < plot.x {
        tx = plot.x;
    }
    if tx + size.width > plot.right() {
        tx = plot.right() - size.width;
    }
    if ty + size.height > plot.bottom() {
        ty = plot.bottom() - size.height;
    }
    Rect::new(tx, ty, size.width, size.height)
}

/// Gap between the readout card and whatever it is anchored on.
const CARD_GAP: f32 = 8.0;

/// Draw the shared readout card used by all three chart kinds: `text`
/// centered above the anchor, flipped below / clamped horizontally and
/// vertically so it never clips outside `plot`.
///
/// Placement is [`mark_tooltip_rect`], including what `contact` means.
pub(crate) fn draw_mark_tooltip(
    canvas: &mut Canvas,
    theme: &Theme,
    plot: Rect,
    anchor: Point,
    contact: Option<Point>,
    text: &str,
    label_style: &TextStyle,
) {
    let text_w = measure_text_width(canvas, text, label_style);
    let size = teksilo_canvas::Size::new(
        text_w + cs::TOOLTIP_PADDING * 2.0,
        label_style.size * 1.4 + cs::TOOLTIP_PADDING,
    );
    let tip = mark_tooltip_rect(plot, anchor, contact, size);
    canvas.fill_rounded_rect(tip, CornerRadius::uniform(4.0), theme.colors.tooltip_bg);
    canvas.stroke_rounded_rect(
        tip,
        CornerRadius::uniform(4.0),
        theme.colors.tooltip_border,
        1.0,
    );

    let label_rect = Rect::new(
        tip.x + cs::TOOLTIP_PADDING,
        tip.y + (tip.height - label_style.size * 1.2) * 0.5,
        tip.width - cs::TOOLTIP_PADDING * 2.0,
        label_style.size * 1.2,
    );
    canvas.draw_text(text, label_rect, label_style, theme.colors.tooltip_text);
}

/// Resolve a [`BorderRecipe`] (from `ChartStyle::gridline`) plus an
/// optional per-axis dash override (`AxisConfig::gridline_dash`) into a
/// concrete [`StrokeStyle`] ready for `Canvas::stroke_path`. The dash
/// override wins over the recipe's own [`BorderStyle`] when set.
///
/// Gridlines are drawn via `stroke_path` (Tier 3, CPU-rasterized through
/// tiny-skia), whose `PathEntry.stroke_style` the path-atlas rasterizer
/// carries through to `tiny_skia::StrokeDash`. That used to be the *only*
/// draw path that honored dashing: `Canvas::draw_line` (Tier 1) baked a
/// plain `DecorationRect` and dropped the pattern silently. `Canvas` now
/// routes any dashing style to `stroke_path` itself (see
/// `StrokeStyle::is_dashed`), so calling it here is no longer load-bearing
/// — but it is still the honest description of what a dashed gridline is,
/// and it skips the routing check.
pub(crate) fn resolve_gridline_stroke(
    recipe: &BorderRecipe,
    dash_override: Option<(f32, f32)>,
) -> StrokeStyle {
    if let Some((dash, gap)) = dash_override {
        return StrokeStyle::dashed(recipe.width, dash, gap);
    }
    match recipe.style {
        BorderStyle::Solid => StrokeStyle::solid(recipe.width),
        BorderStyle::Dashed { dash, gap } => StrokeStyle::dashed(recipe.width, dash, gap),
        BorderStyle::Dotted { gap } => StrokeStyle::dotted(recipe.width, gap),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(series_id: SeriesId, point_idx: usize, shape: MarkShape) -> MarkGeometry {
        MarkGeometry {
            series_id,
            point_idx,
            series_name: "S".into(),
            category_label: "C".into(),
            value: 1.0,
            shape,
        }
    }

    fn fake_series_id() -> SeriesId {
        let model: teksilo_data::ChartModel<i32> = teksilo_data::ChartModel::new();
        model.add_series("s")
    }

    #[test]
    fn point_bounding_rect_is_square_around_center() {
        let shape = MarkShape::Point {
            center: Point::new(10.0, 20.0),
            radius: 3.0,
        };
        let r = shape.bounding_rect();
        assert_eq!((r.x, r.y, r.width, r.height), (7.0, 17.0, 6.0, 6.0));
    }

    #[test]
    fn rect_bounding_rect_is_self() {
        let rect = Rect::new(1.0, 2.0, 3.0, 4.0);
        let shape = MarkShape::Rect(rect);
        assert_eq!(shape.bounding_rect(), rect);
    }

    #[test]
    fn slice_bounding_rect_crosses_cardinal_angle() {
        // Sweep from -10deg to +10deg crosses the 0-rad cardinal angle
        // (screen +x / 3 o'clock). The bbox must reach exactly
        // center.x + outer_radius on the right edge — a sample at only
        // the two endpoints would fall short (cos(10deg) < 1.0).
        let center = Point::new(100.0, 100.0);
        let outer = 50.0;
        let start = (-10.0_f32).to_radians();
        let sweep = (20.0_f32).to_radians();
        let shape = MarkShape::Slice {
            center,
            inner_radius: 0.0,
            outer_radius: outer,
            start_rad: start,
            sweep_rad: sweep,
        };
        let r = shape.bounding_rect();
        assert!(
            (r.right() - (center.x + outer)).abs() < 1e-3,
            "bbox right edge should reach the cardinal-angle radius, got {:?}",
            r
        );
    }

    #[test]
    fn slice_bounding_rect_handles_negative_sweep() {
        let center = Point::new(0.0, 0.0);
        let shape = MarkShape::Slice {
            center,
            inner_radius: 0.0,
            outer_radius: 10.0,
            start_rad: 0.0,
            sweep_rad: (-20.0_f32).to_radians(),
        };
        let r = shape.bounding_rect();
        // Sweeping -20deg from 0 covers [-20deg, 0], right edge still at
        // full radius (0 rad is one endpoint).
        assert!((r.right() - 10.0).abs() < 1e-3);
    }

    #[test]
    fn nearest_point_ignores_non_point_shapes_and_no_radius_cutoff() {
        let id = fake_series_id();
        let marks = vec![
            mark(id, 0, MarkShape::Rect(Rect::new(0.0, 0.0, 5.0, 5.0))),
            mark(
                id,
                1,
                MarkShape::Point {
                    center: Point::new(100.0, 100.0),
                    radius: 3.0,
                },
            ),
        ];
        // Far away — still returns the only Point mark (no radius cutoff).
        let idx = nearest_point(&marks, Point::new(0.0, 0.0));
        assert_eq!(idx, Some(1));
    }

    #[test]
    fn rect_hit_finds_containing_bar_and_none_in_gap() {
        let id = fake_series_id();
        let marks = vec![
            mark(id, 0, MarkShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0))),
            mark(id, 1, MarkShape::Rect(Rect::new(20.0, 0.0, 10.0, 10.0))),
        ];
        assert_eq!(rect_hit(&marks, Point::new(5.0, 5.0)), Some(0));
        assert_eq!(rect_hit(&marks, Point::new(25.0, 5.0)), Some(1));
        assert_eq!(rect_hit(&marks, Point::new(15.0, 5.0)), None);
    }

    #[test]
    fn slice_hit_finds_slice_and_handles_negative_sweep() {
        let id = fake_series_id();
        let marks = [
            mark(
                id,
                0,
                MarkShape::Slice {
                    center: Point::ZERO,
                    inner_radius: 0.0,
                    outer_radius: 10.0,
                    start_rad: 0.0,
                    sweep_rad: std::f32::consts::FRAC_PI_2,
                },
            ),
            mark(
                id,
                1,
                MarkShape::Slice {
                    center: Point::ZERO,
                    inner_radius: 0.0,
                    outer_radius: 10.0,
                    start_rad: 0.0,
                    sweep_rad: -std::f32::consts::FRAC_PI_2,
                },
            ),
        ];
        assert_eq!(slice_hit(&marks[..1], std::f32::consts::FRAC_PI_4), Some(0));
        assert_eq!(
            slice_hit(&marks[1..2], -std::f32::consts::FRAC_PI_4),
            Some(0)
        );
    }

    #[test]
    fn angle_in_sweep_wraps_across_zero() {
        // start close to TAU, sweep pushes past the wrap point.
        let start = std::f32::consts::TAU - 0.1;
        let sweep = 0.3;
        assert!(angle_in_sweep(0.05, start, sweep));
        assert!(!angle_in_sweep(1.0, start, sweep));
    }

    #[test]
    fn mark_element_id_is_stable_and_differs_by_point() {
        let id = fake_series_id();
        let a = mark_element_id(id, 0);
        let b = mark_element_id(id, 0);
        let c = mark_element_id(id, 1);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn gridline_stroke_dash_override_wins_over_recipe_style() {
        let recipe = BorderRecipe {
            width: 1.0,
            color: teksilo_core::styles::RecipeColor::Static(teksilo_tokens::Color::BLACK),
            style: BorderStyle::Solid,
            position: teksilo_core::styles::BorderPosition::Center,
            sides: None,
        };
        let stroke = resolve_gridline_stroke(&recipe, Some((4.0, 2.0)));
        assert_eq!(stroke.dash_pattern, Some(vec![4.0, 2.0]));
    }

    #[test]
    fn gridline_stroke_falls_back_to_recipe_dashed_style() {
        let recipe = BorderRecipe {
            width: 1.0,
            color: teksilo_core::styles::RecipeColor::Static(teksilo_tokens::Color::BLACK),
            style: BorderStyle::Dashed {
                dash: 3.0,
                gap: 1.0,
            },
            position: teksilo_core::styles::BorderPosition::Center,
            sides: None,
        };
        let stroke = resolve_gridline_stroke(&recipe, None);
        assert_eq!(stroke.dash_pattern, Some(vec![3.0, 1.0]));
    }

    // ── kind-aware tolerance ──────────────────────────────────────────────

    #[test]
    fn a_mouse_earns_no_tolerance_and_a_finger_does() {
        let compact = InputTokens::for_density(teksilo_tokens::TargetDensity::Compact);
        let touch = InputTokens::for_density(teksilo_tokens::TargetDensity::Touch);
        // Zero for the mouse at every density — which is what makes every
        // tolerance-aware hit test below the identity for a mouse.
        assert_eq!(mark_tolerance(PointerKind::Mouse, &compact), 0.0);
        assert_eq!(mark_tolerance(PointerKind::Mouse, &touch), 0.0);
        assert!(mark_tolerance(PointerKind::Touch, &compact) > 0.0);
        assert!(
            mark_tolerance(PointerKind::Pen(teksilo_tokens::PenKind::Pen), &compact) > 0.0,
            "a pen occludes a little and earns a little"
        );
        assert!(
            mark_tolerance(PointerKind::Pen(teksilo_tokens::PenKind::Pen), &compact)
                < mark_tolerance(PointerKind::Touch, &compact),
            "and less than a finger"
        );
    }

    #[test]
    fn a_press_past_a_bars_edge_hits_it_only_within_the_tolerance() {
        let id = fake_series_id();
        let marks = vec![mark(
            id,
            0,
            MarkShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0)),
        )];
        // 3 dp past the right edge.
        let near = Point::new(13.0, 5.0);
        assert_eq!(rect_hit_within(&marks, near, 8.0), Some(0));
        assert_eq!(
            rect_hit_within(&marks, near, 0.0),
            None,
            "a precise pointer still needs to be inside the bar"
        );
        // 12 dp past it — beyond even a finger's reach.
        assert_eq!(rect_hit_within(&marks, Point::new(22.0, 5.0), 8.0), None);
    }

    #[test]
    fn a_tolerated_press_between_two_bars_takes_the_nearer_one() {
        let id = fake_series_id();
        let marks = vec![
            mark(id, 0, MarkShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0))),
            mark(id, 1, MarkShape::Rect(Rect::new(20.0, 0.0, 10.0, 10.0))),
        ];
        // Both bars are within 8 dp of both points, so a first-match scan
        // over the inflated rectangles would answer bar 0 for each of them.
        assert_eq!(rect_hit_within(&marks, Point::new(13.0, 5.0), 8.0), Some(0));
        assert_eq!(
            rect_hit_within(&marks, Point::new(17.0, 5.0), 8.0),
            Some(1),
            "nearest, not first — otherwise the mark vector's order decides"
        );
    }

    #[test]
    fn containment_still_wins_over_a_nearer_neighbours_edge() {
        let id = fake_series_id();
        // Bar 1 is a sliver immediately right of bar 0's right edge. A press
        // deep inside bar 0 is 9 dp from bar 1 and 0 dp from bar 0, so the
        // two phases agree here; a press 1 dp inside bar 0's right edge is
        // still inside bar 0 and must not be handed to the sliver.
        let marks = vec![
            mark(id, 0, MarkShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0))),
            mark(id, 1, MarkShape::Rect(Rect::new(10.0, 0.0, 1.0, 10.0))),
        ];
        assert_eq!(rect_hit_within(&marks, Point::new(9.0, 5.0), 8.0), Some(0));
    }

    #[test]
    fn a_non_finite_tolerance_falls_back_to_containment() {
        let id = fake_series_id();
        let marks = vec![mark(
            id,
            0,
            MarkShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0)),
        )];
        assert_eq!(
            rect_hit_within(&marks, Point::new(5.0, 5.0), f32::NAN),
            Some(0)
        );
        assert_eq!(
            rect_hit_within(&marks, Point::new(13.0, 5.0), f32::NAN),
            None
        );
    }

    /// Two quarter slices meeting at 3 o'clock's neighbour, so the boundary
    /// is unambiguous in the screen-space angle convention.
    fn two_quarter_slices(id: SeriesId) -> Vec<MarkGeometry> {
        vec![
            mark(
                id,
                0,
                MarkShape::Slice {
                    center: Point::new(100.0, 100.0),
                    inner_radius: 20.0,
                    outer_radius: 50.0,
                    start_rad: 0.0,
                    sweep_rad: std::f32::consts::FRAC_PI_2,
                },
            ),
            mark(
                id,
                1,
                MarkShape::Slice {
                    center: Point::new(100.0, 100.0),
                    inner_radius: 20.0,
                    outer_radius: 50.0,
                    start_rad: std::f32::consts::FRAC_PI_2,
                    sweep_rad: std::f32::consts::FRAC_PI_2,
                },
            ),
        ]
    }

    #[test]
    fn a_press_past_a_wedges_arc_reaches_it_only_within_the_tolerance() {
        let id = fake_series_id();
        let marks = two_quarter_slices(id);
        let center = Point::new(100.0, 100.0);
        let bearing = std::f32::consts::FRAC_PI_4;
        let at = |r: f32| Point::new(center.x + r * bearing.cos(), center.y + r * bearing.sin());
        assert_eq!(
            slice_hit_within(&marks, center, 20.0, 50.0, at(53.0), 8.0),
            Some(0)
        );
        assert_eq!(
            slice_hit_within(&marks, center, 20.0, 50.0, at(53.0), 0.0),
            None,
            "a precise pointer still needs to be inside the ring"
        );
        assert_eq!(
            slice_hit_within(&marks, center, 20.0, 50.0, at(62.0), 8.0),
            None
        );
        // The donut hole is the same rule inward.
        assert_eq!(
            slice_hit_within(&marks, center, 20.0, 50.0, at(17.0), 8.0),
            Some(0)
        );
        assert_eq!(
            slice_hit_within(&marks, center, 20.0, 50.0, at(17.0), 0.0),
            None
        );
    }

    #[test]
    fn a_tolerated_press_never_crosses_the_slice_boundary() {
        let id = fake_series_id();
        let marks = two_quarter_slices(id);
        let center = Point::new(100.0, 100.0);
        let quarter = std::f32::consts::FRAC_PI_2;
        // 3 dp beyond the outer arc — inside the radial tolerance — a hair
        // either side of the boundary the two wedges share.
        let at = |bearing: f32| {
            Point::new(
                center.x + 53.0 * bearing.cos(),
                center.y + 53.0 * bearing.sin(),
            )
        };
        assert_eq!(
            slice_hit_within(&marks, center, 20.0, 50.0, at(quarter - 0.02), 8.0),
            Some(0)
        );
        assert_eq!(
            slice_hit_within(&marks, center, 20.0, 50.0, at(quarter + 0.02), 8.0),
            Some(1),
            "the tolerance is radial only: a bearing inside a neighbour's \
             sweep belongs to the neighbour at every radius"
        );
    }

    #[test]
    fn a_disc_with_no_geometry_yet_hits_nothing() {
        let id = fake_series_id();
        let marks = two_quarter_slices(id);
        assert_eq!(
            slice_hit_within(
                &marks,
                Point::new(100.0, 100.0),
                0.0,
                0.0,
                Point::new(100.0, 100.0),
                8.0
            ),
            None
        );
    }

    // ── keyboard traversal ────────────────────────────────────────────────

    #[test]
    fn stepping_walks_the_paint_order_and_clamps_at_both_ends() {
        let id = fake_series_id();
        let marks: Vec<MarkGeometry> = (0..3)
            .map(|i| {
                mark(
                    id,
                    i,
                    MarkShape::Rect(Rect::new(i as f32 * 10.0, 0.0, 5.0, 5.0)),
                )
            })
            .collect();
        assert_eq!(stepped_mark(&marks, None, MarkStep::Next), Some((id, 0)));
        assert_eq!(
            stepped_mark(&marks, None, MarkStep::Previous),
            Some((id, 2))
        );
        assert_eq!(
            stepped_mark(&marks, Some((id, 0)), MarkStep::Next),
            Some((id, 1))
        );
        assert_eq!(
            stepped_mark(&marks, Some((id, 2)), MarkStep::Next),
            Some((id, 2)),
            "clamps rather than wrapping"
        );
        assert_eq!(
            stepped_mark(&marks, Some((id, 0)), MarkStep::Previous),
            Some((id, 0))
        );
        assert_eq!(
            stepped_mark(&marks, Some((id, 1)), MarkStep::First),
            Some((id, 0))
        );
        assert_eq!(
            stepped_mark(&marks, Some((id, 1)), MarkStep::Last),
            Some((id, 2))
        );
        assert_eq!(stepped_mark(&[], None, MarkStep::Next), None);
    }

    #[test]
    fn a_modified_arrow_is_not_a_traversal() {
        use teksilo_core::event::Modifiers;
        let bare = WidgetEvent::KeyDown {
            key: Key::ArrowRight,
            modifiers: Modifiers::NONE,
            text: None,
        };
        assert_eq!(mark_step_for_key(&bare), Some(MarkStep::Next));
        for m in [Modifiers::CTRL, Modifiers::ALT, Modifiers::SHIFT] {
            let chord = WidgetEvent::KeyDown {
                key: Key::ArrowRight,
                modifiers: m,
                text: None,
            };
            assert_eq!(
                mark_step_for_key(&chord),
                None,
                "a modified chord must be left to the shortcut system"
            );
        }
    }

    // ── AT node identity ──────────────────────────────────────────────────

    #[test]
    fn a_mark_is_recovered_from_the_at_node_id_it_emitted() {
        let id = fake_series_id();
        let marks = vec![
            mark(id, 0, MarkShape::Rect(Rect::new(0.0, 0.0, 5.0, 5.0))),
            mark(id, 1, MarkShape::Rect(Rect::new(10.0, 0.0, 5.0, 5.0))),
        ];
        let owner = WidgetId::default();
        let node = teksilo_core::accessibility::synthetic_node_id(
            owner,
            mark_element_id(id, 1),
            SyntheticKind::ChartMark,
        );
        assert_eq!(mark_for_node(owner, &marks, node), Some((id, 1)));
        // A datum that is no longer in the mark vector — the model changed
        // under a screen reader holding a stale node id — names nothing.
        let stale = teksilo_core::accessibility::synthetic_node_id(
            owner,
            mark_element_id(id, 99),
            SyntheticKind::ChartMark,
        );
        assert_eq!(mark_for_node(owner, &marks, stale), None);
        // The synthetic KIND is part of the identity, so a scene item's node
        // id at the same element id is not this mark.
        let other_kind = teksilo_core::accessibility::synthetic_node_id(
            owner,
            mark_element_id(id, 1),
            SyntheticKind::SceneItem,
        );
        assert_eq!(mark_for_node(owner, &marks, other_kind), None);
    }

    // ── readout card placement ────────────────────────────────────────────

    #[test]
    fn the_card_sits_just_above_the_mark_for_a_precise_pointer() {
        let plot = Rect::new(0.0, 0.0, 400.0, 300.0);
        let size = teksilo_canvas::Size::new(80.0, 20.0);
        let r = mark_tooltip_rect(plot, Point::new(200.0, 150.0), None, size);
        assert_eq!(r.bottom(), 142.0, "8 dp above the mark");
        assert!((r.x + r.width * 0.5 - 200.0).abs() < 1e-3, "centred on it");
    }

    #[test]
    fn the_card_clears_the_contact_patch_when_a_finger_raised_it() {
        let plot = Rect::new(0.0, 0.0, 400.0, 300.0);
        let size = teksilo_canvas::Size::new(80.0, 20.0);
        let contact = Point::new(120.0, 150.0);
        let r = mark_tooltip_rect(plot, Point::new(200.0, 150.0), Some(contact), size);
        assert!(
            (r.x + r.width * 0.5 - contact.x).abs() < 1e-3,
            "anchored on the contact, not on the mark"
        );
        let clearance = contact.y - r.bottom();
        assert!(
            clearance >= teksilo_core::overlay::ASSUMED_CONTACT_PATCH.height * 0.5,
            "the card must clear the half-patch the finger covers, got {clearance}"
        );
    }

    #[test]
    fn the_card_flips_below_rather_than_clipping_the_plots_top() {
        let plot = Rect::new(0.0, 100.0, 400.0, 300.0);
        let size = teksilo_canvas::Size::new(80.0, 20.0);
        let r = mark_tooltip_rect(plot, Point::new(200.0, 105.0), None, size);
        assert!(r.y > 105.0, "flipped below the mark, got {r:?}");
    }

    #[test]
    fn gridline_stroke_solid_has_no_dash_pattern() {
        let recipe = BorderRecipe {
            width: 1.0,
            color: teksilo_core::styles::RecipeColor::Static(teksilo_tokens::Color::BLACK),
            style: BorderStyle::Solid,
            position: teksilo_core::styles::BorderPosition::Center,
            sides: None,
        };
        let stroke = resolve_gridline_stroke(&recipe, None);
        assert_eq!(stroke.dash_pattern, None);
    }
}
