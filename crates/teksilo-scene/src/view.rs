// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`SceneView`] — the viewport widget that hosts a [`Scene`] and
//! places its items at scene coordinates.
//!
//! `SceneView` is the bridge between the model layer ([`Scene`] /
//! [`SceneModel`]) and the render/event pipeline. It
//! manages a pan/zoom/rotation camera, materialises heavyweight widgets
//! for delegated items, dispatches pointer events to lightweight item
//! handlers, and feeds synthetic AT nodes to AccessKit for every visible
//! lightweight item. Multiple `SceneView`s can share one `SceneModel` and
//! reconcile independently on every mutation.
//!
//! ## Composition
//!
//! - **Placement.** `place_children` plants each materialised
//!   heavyweight widget at its scene-space rect (composed from the
//!   item's `local_pos`, `transform`, and parent chain).
//! - **Paint bands.** Three passes: `paint` draws the `Under` lightweight
//!   items (backdrop), the arena child-walk draws the heavyweight widgets,
//!   then `post_paint` draws the `Over` lightweight items + marquee /
//!   foreground / debug overlays. `z` orders within each tier; the
//!   Under/Over band ([`Scene::set_layer`](crate::Scene::set_layer))
//!   chooses the side. See `docs/teksilo-scene.md` §"Z-order and paint bands".
//! - **View transform.** Pan / zoom / rotation are four animated
//!   `Signal<f32>`s on `SceneView`, composed into a derived
//!   `Signal<Transform2D>` bound via `BuildContext::set_content_transform`
//!   on the view itself. The render walker pushes that scope around
//!   the entire subtree, so every materialised widget is visually
//!   transformed; transform-aware hit-test routes pointer events
//!   through the same scope.
//! - **Spatial index.** `place_children` and the paint walk consult
//!   `Scene::items_in_rect(visible_region)` to skip off-screen items.
//! - **Idle gating.** Pan / zoom that's reached its terminal tick
//!   stops scheduling frames via the engine's per-node `paint_epoch`.
//!
//! ## Input wiring
//!
//! - **`on_scroll`** — trackpad two-finger pan (`ScrollDelta::Pixels`)
//!   and mouse wheel (`ScrollDelta::Lines`) animate the pan signals
//!   via `Easing::EaseOut`. Trackpad momentum events from winit
//!   arrive as further `Pixels` deltas; the existing animation
//!   pipeline turns this into smooth inertial fling without a custom
//!   recognizer.
//! - **`on_pinch`** — a pinch (`PinchPhase::Changed`) feeds `scale` into
//!   the zoom signal and `rotation` into the rotation signal, anchored
//!   around the gesture center so the scene point under the user's
//!   fingers stays put. Two producers reach the one handler: the OS
//!   trackpad stream and the two-contact touch recognizer, and both
//!   satisfy the contract on
//!   [`GestureEvent::PinchChanged`](teksilo_core::gesture::GestureEvent::PinchChanged)
//!   — `scale` is the factor **since the previous sample** (folded in with
//!   `zoom *= scale`) and `rotation` the **radian** delta since the previous
//!   sample (`rotation += delta`). Degrees never reach here: winit's unit is
//!   converted at the platform seam. `view::tests::touch_camera` holds both
//!   halves.
//! - **Reduced-motion** — at build time, captures
//!   [`BuildContext::prefers_reduced_motion`](teksilo_core::build_context::BuildContext::prefers_reduced_motion).
//!   When set, scroll handlers `set` the signals directly instead of
//!   `animate_to`-ing them; pinch is already instantaneous.
//! - **Drag-to-move** for items carrying `IS_DRAGGABLE`; **marquee**
//!   selection on the empty viewport surface (or under
//!   [`DragMode::ScrollHandDrag`](crate::DragMode), pan-on-drag).
//! - **Pan to scroll** — an interactive view declares a
//!   [`PanClaim`](teksilo_core::pointer::touch_action::PanClaim), so a finger's
//!   pan reaches the same `on_scroll` handler as a
//!   `ScrollSource::TouchPan` sample and moves the camera with no tween (a
//!   finger is already the animation), hard-clamped on the coast that follows
//!   the lift, and declined at a bound so the gesture chains to whatever
//!   scrolls outside the view. Deliberately **not** routed through
//!   `teksilo_widgets::common::scrollable`: that models a surface as an offset
//!   in `[0, max]`, and a scene's pan is negated, bounded by a zoom-dependent
//!   rectangle, and centre-pinned where that rectangle is smaller than the
//!   viewport. With selection or magnetism on, the view's own drag recognizer
//!   is the competitor a finger meets first and it wins at the smaller drag
//!   slop — the marquee, not the camera.
//!
//! ## Example
//!
//! ```rust
//! # use teksilo_scene::{Scene, SceneModel, SceneView, SceneSelectionMode, RectItem};
//! # use teksilo_canvas::{Point, Rect};
//! # use teksilo_tokens::Color;
//! // Build a shared model and add a lightweight rect item.
//! let model = SceneModel::new();
//! let local_bounds = Rect::new(0.0, 0.0, 120.0, 80.0);
//! let item_id = model.add_item(
//!     RectItem::new(local_bounds).fill(Color::from_rgb(0.2, 0.5, 0.8)),
//!     Point::new(50.0, 50.0), // local_pos in scene coords
//! );
//!
//! // Create viewports backed by that model; each has its own camera.
//! let _view_a = SceneView::with_model(model.clone())
//!     .selection_mode(SceneSelectionMode::Single)
//!     .default_size(800.0, 600.0)
//!     .initial_zoom(1.5);
//!
//! let _view_b = SceneView::with_model(model.clone())
//!     .interactive(false); // axis-chrome / overview pane
//!
//! // Both views see the item; the model remembers its local_pos.
//! assert!(model.local_pos(item_id).is_some());
//! ```

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{Point, Rect, Size, SizeProposal, Transform2D, Vec2};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, ScrollDelta, WidgetEvent};
use teksilo_core::gesture::PinchPhase;
use teksilo_core::overscroll::OverscrollBehavior;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::Easing;

use crate::item::ItemId;
use crate::magnet::{MagnetId, MagnetSnap, MagnetismConfig};
use crate::pick::{PaintKey, RANK_OVER};
use crate::scene::Scene;
use crate::scene_model::SceneModel;
use crate::shape::{ItemSelectionMode, ItemShape, SceneRegion};
use crate::transform::{anchor_pan_for_pinch, compose_view};
use teksilo_i18n::LocalizedString;

/// Logical pixels of pan applied per `ScrollDelta::Lines` notch.
/// Mirrors the convention used by `ScrollArea` (`line_height` ≈ 16 in
/// teksilo-widgets).
const DEFAULT_LINE_HEIGHT: f32 = 16.0;
const DEFAULT_PAN_DURATION: Duration = Duration::from_millis(120);
const DEFAULT_ZOOM_DURATION: Duration = Duration::from_millis(180);
const DEFAULT_MIN_ZOOM: f32 = 0.1;
const DEFAULT_MAX_ZOOM: f32 = 10.0;

/// Default [`SceneView::retention_margin`], in screen pixels.
///
/// 96 px is what a very fast fling covers in one frame at 120 Hz
/// (11 520 px/s), which is the quantity the margin has to beat: it exists so a
/// card is woken while it is still off screen rather than on the frame it
/// becomes visible.
pub const DEFAULT_RETENTION_MARGIN: f32 = 96.0;

/// Maximum movement, in **view pixels**, between PointerDown and PointerUp for
/// the gesture to count as a tap rather than a drag — the floor a precise
/// pointer gets.
///
/// Measured on screen rather than in scene units because the same hand movement
/// has to mean the same thing at every zoom. A scene-unit comparison divides
/// the tolerance by the zoom, so the tolerance shrinks as the view zooms out
/// and grows as it zooms in: far enough out, an ordinary click carries more
/// jitter than the tolerance allows and no item can be tapped at all; far
/// enough in, a deliberate small drag is reported as a tap. See
/// [`tap_tolerance_px`].
const TAP_MOVEMENT_THRESHOLD: f32 = 4.0;

/// The tap-versus-drag movement tolerance for `kind`, in view pixels.
///
/// A precise pointer keeps the view's own floor. A coarse one is allowed the
/// travel its gesture profile already calls a tap: a contact patch rolls as it
/// lifts, and the profile is where the framework states how much of that is
/// still one tap. Reads a build-time [`InputTokens`](teksilo_tokens::InputTokens)
/// snapshot, which cannot go
/// stale — a density change marks the tree for rebuild.
fn tap_tolerance_px(
    tokens: &teksilo_tokens::InputTokens,
    kind: teksilo_tokens::PointerKind,
) -> f32 {
    if kind.is_coarse() {
        TAP_MOVEMENT_THRESHOLD.max(tokens.profile(kind).tap_slop)
    } else {
        TAP_MOVEMENT_THRESHOLD
    }
}

/// How far a *missed* grab may be re-attributed to scene content of
/// `screen_size`, in view pixels.
///
/// Delegates to [`HitSlop::for_pointer`](teksilo_core::pointer::hit_slop::HitSlop::for_pointer),
/// which is the framework's one answer to this question, so the scene inherits
/// its two properties rather than restating them: the offer is zero for a mouse
/// at every density (the mouse profile's radius is `0.0`, and a pen's is small),
/// and it is zero for content already at least the density's target size on its
/// smaller axis. A
/// scene's content is mostly large, so in practice this widens exactly the
/// small items — a connector stroke, a port dot, a pin — that a finger cannot
/// otherwise land on.
fn grab_slop_px(
    tokens: &teksilo_tokens::InputTokens,
    kind: teksilo_tokens::PointerKind,
    screen_size: Size,
) -> f32 {
    teksilo_core::pointer::hit_slop::HitSlop::for_pointer(kind, tokens).outset_for(screen_size)
}

/// Distance from `p` to the nearest point of `r`; `0.0` when `p` is inside.
///
/// The slop pass settles overlapping candidates by distance rather than by
/// z-order, so a stroke 3 px away wins over a box whose *inflated* rectangle
/// the press also fell inside 8 px away.
fn distance_to_rect(p: Point, r: Rect) -> f32 {
    let dx = (r.x - p.x).max(p.x - (r.x + r.width)).max(0.0);
    let dy = (r.y - p.y).max(p.y - (r.y + r.height)).max(0.0);
    dx.hypot(dy)
}
/// Take the tightening intersection of two optional zoom ranges:
/// `(max(lo), min(hi))`. `None` on either side leaves the other
/// untouched; `None` on both returns `None`. Used to compose
/// Scene-level + view-level constraints — neither side can loosen.
fn intersect_zoom_range(
    a: Option<&std::ops::RangeInclusive<f32>>,
    b: Option<&std::ops::RangeInclusive<f32>>,
) -> Option<std::ops::RangeInclusive<f32>> {
    match (a, b) {
        (None, None) => None,
        (Some(r), None) | (None, Some(r)) => Some(r.clone()),
        (Some(a), Some(b)) => {
            let lo = a.start().max(*b.start());
            let hi = a.end().min(*b.end());
            // Guard against degenerate intersect: if the ranges
            // don't overlap (lo > hi), collapse to the tighter
            // side's lo so callers see a single allowed value
            // rather than NaN-clamping.
            Some(lo..=hi.max(lo))
        }
    }
}

/// Clamp a zoom factor through an optional range. `None` is the
/// identity — no clamp applied.
fn clamp_zoom(z: f32, range: Option<&std::ops::RangeInclusive<f32>>) -> f32 {
    match range {
        None => z,
        Some(r) => z.clamp(*r.start(), *r.end()),
    }
}

/// Take the tightening intersection of two optional pan-bounds
/// rects. `None` on either side leaves the other untouched; `None`
/// on both returns `None`. If both are `Some` and the rect
/// intersection is empty (no overlap), falls back to the first
/// (Scene-declared) bounds — the more authoritative side.
fn intersect_pan_bounds(scene: Option<Rect>, view: Option<Rect>) -> Option<Rect> {
    match (scene, view) {
        (None, None) => None,
        (Some(r), None) | (None, Some(r)) => Some(r),
        (Some(a), Some(b)) => {
            let x = a.x.max(b.x);
            let y = a.y.max(b.y);
            let right = a.right().min(b.right());
            let bottom = a.bottom().min(b.bottom());
            if right > x && bottom > y {
                Some(Rect::new(x, y, right - x, bottom - y))
            } else {
                Some(a)
            }
        }
    }
}

/// Clamp a pan vector against `bounds` so the visible scene region
/// (derived from `viewport` and `zoom`) stays inside the bounds rect.
/// When the rect is smaller than the visible viewport on an axis,
/// that axis is centered on the bounds rather than clamped.
///
/// `bounds` is in scene coords; `viewport` is the SceneView's
/// resolved size in screen pixels; `zoom` is the current zoom
/// factor. Returns `pan` unchanged when `bounds` is `None`.
fn clamp_pan_to_bounds(pan: Vec2, bounds: Option<&Rect>, viewport: Size, zoom: f32) -> Vec2 {
    let Some(b) = bounds else { return pan };
    if zoom <= 0.0 || viewport.width <= 0.0 || viewport.height <= 0.0 {
        return pan;
    }
    // visible_scene_x = [-pan.x / zoom, (viewport_w - pan.x) / zoom]
    // For visible to lie inside [b.x, b.right]:
    //   pan.x in [viewport_w - b.right * zoom, -b.x * zoom]
    let clamp_axis = |pan_c: f32, b_lo: f32, b_hi: f32, vp: f32| {
        let lo = vp - b_hi * zoom;
        let hi = -b_lo * zoom;
        if hi >= lo {
            pan_c.clamp(lo, hi)
        } else {
            // Bounds smaller than viewport on this axis — center.
            // visible_center = b_lo + (b_hi - b_lo)/2
            //                = (b_lo + b_hi) / 2
            //                = (vp/2 - pan_c) / zoom
            // → pan_c = vp/2 - (b_lo + b_hi)/2 * zoom
            vp / 2.0 - ((b_lo + b_hi) / 2.0) * zoom
        }
    };
    Vec2::new(
        clamp_axis(pan.x, b.x, b.right(), viewport.width),
        clamp_axis(pan.y, b.y, b.bottom(), viewport.height),
    )
}

/// One entry's measurement history inside one view. See
/// [`SceneView::measure_state`].
///
/// # What an oscillation is, and what an edit is
///
/// A measured size feeds back into the model and the model feeds the next
/// pass, so a body that cannot answer the same question twice would oscillate
/// for ever. Bounding that needs something that can tell an oscillation from
/// an ordinary edit — and the two are **indistinguishable from the numbers**.
/// `100 → 120 → 100` is an oscillation when the body is answering its own last
/// answer, and is a user typing a character that wraps and then deleting it
/// when it is not; a user who types and deletes the same character twice hands
/// a value-pattern detector a sequence bit-identical to a flip-flop's. Freezing
/// the second case leaves a card at a height its content abandoned, and — since
/// a card that writes nothing is never asked again — leaves it there for ever.
///
/// What separates them is not the values but **what drove the pass**. A body's
/// answer is a function of its content and of the width it is offered. The
/// width is held fixed here (a change to it retires the history outright), and
/// the content cannot change without something dirtying the card: a `Signal`
/// bound at `Relayout`, a rebuild, a theme swap, a fresh node. None of that
/// fires an [`ItemChange`](crate::ItemChange) the view could observe, but all of
/// it sets `needs_layout` on the card's own node — a relayout-level binding
/// marks its whole ancestor chain — and the arena clears those flags only
/// **after** the walk, so when `place_children` asks, the answer is still
/// there. `layout_impl::card_was_reached` is the one lookup that asks it.
///
/// This view's own write, by contrast, reaches nothing below it: it emits
/// [`MeasuredSizeChanged`](crate::ItemChange::MeasuredSizeChanged), the
/// observer bumps `measure_dirty`, and that signal is bound at `Relayout` on
/// the `SceneView` node — whose ancestors are marked, and whose descendants are
/// not. So on the pass a write causes, the card reads **clean**, and a clean
/// card that answers differently is reading back what was written to it. That
/// is not a guess about a pattern; it is the definition of the non-idempotence
/// [`SizePolicy`](crate::SizePolicy) warns about.
///
/// # What is bounded, and by what
///
/// Nothing is pinned to a value. A contradiction on a clean pass resolves to
/// the **larger** of the two answers (a box that is too big shows everything;
/// one that is too small cuts content off) and is counted as a strike. At
/// [`MAX_CONTRADICTIONS`] the entry stops taking new answers and re-states the
/// one the model already holds — which writes nothing, and silence is what ends
/// the chain, because a pass that writes nothing schedules no successor. So a
/// hopeless body costs a bounded number of passes **per external event**
/// instead of an unbounded number for ever, and an edited body is never bounded
/// at all: every one of its passes is externally driven, and an externally
/// driven pass always takes the answer it is given.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MeasureTrack {
    /// The width the last measurement was taken at. A width change retires the
    /// history — that is a different question, and its answer owes nothing to
    /// the previous one.
    width: f32,
    /// The size resolved last, which is also the size the model was told.
    last: Size,
    /// How many times this entry has contradicted itself, on passes nothing
    /// external drove, since the question it is being asked last changed. The
    /// two things that change the question are the two things that reset it: a
    /// different width, and a pass something outside this view drove.
    strikes: u8,
}

/// How far apart two measurements have to be to count as different.
///
/// Half a logical pixel: below what a reader can see, above the float noise a
/// shaper produces for the same text measured twice. Without it a body whose
/// height differs in the last bit writes on every pass, and every write is a
/// relayout.
const MEASURE_EPSILON: f32 = 0.5;

/// How many self-contradictions at one width an entry is allowed before this
/// view stops taking its answers for the rest of the chain.
///
/// Two, because the two strikes do different jobs and both are reachable. The
/// first resolves a flip-flop: taking the larger of the two answers lands on a
/// size the body itself named, and for a body that alternates between two
/// values that is already the fixed point — it agrees on the next pass and the
/// chain ends. The second catches the body the first cannot: one whose answer
/// *grows* with what it is given, for which "take the larger" is a ratchet, so
/// the only way to stop is to stop asking.
const MAX_CONTRADICTIONS: u8 = 2;

impl MeasureTrack {
    /// A fresh history whose first answer is `candidate`, taken at `width`.
    fn new(width: f32, candidate: Size) -> Self {
        MeasureTrack {
            width,
            last: candidate,
            strikes: 0,
        }
    }

    /// What this pass should place the card at, given a fresh measurement.
    ///
    /// `question_changed` answers "did anything but this view's own write reach
    /// the card since it was last measured" — see the type's documentation for
    /// why that is the whole discriminator. It is a closure because the two
    /// branches that decide without it (the width changed; the answer did not)
    /// are the ones taken on every ordinary pass, and neither should pay for an
    /// arena lookup.
    fn resolve(
        &mut self,
        width: f32,
        candidate: Size,
        question_changed: impl FnOnce() -> bool,
    ) -> Size {
        if (self.width - width).abs() > MEASURE_EPSILON {
            *self = MeasureTrack::new(width, candidate);
            return candidate;
        }
        if size_close(self.last, candidate) {
            return self.last;
        }
        if question_changed() {
            // A different question, so a different answer is not a
            // contradiction — it is the point.
            self.strikes = 0;
            self.last = candidate;
            return candidate;
        }
        self.strikes = self.strikes.saturating_add(1);
        if self.strikes >= MAX_CONTRADICTIONS {
            // Re-state what the model already holds. The caller writes nothing,
            // nothing schedules another pass, and the chain ends here.
            return self.last;
        }
        let settled = Size::new(
            self.last.width.max(candidate.width),
            self.last.height.max(candidate.height),
        );
        self.last = settled;
        settled
    }
}

/// Two sizes that no reader could tell apart. See [`MEASURE_EPSILON`].
fn size_close(a: Size, b: Size) -> bool {
    (a.width - b.width).abs() <= MEASURE_EPSILON && (a.height - b.height).abs() <= MEASURE_EPSILON
}

/// The single chokepoint for the pan-bounds clamp: take the tightening
/// intersection of the Scene-declared and view-override bounds, then
/// clamp `candidate` against it for the given `viewport` and `zoom`.
///
/// Every pan path (the camera API plus all five gesture handlers) ends
/// in this exact `intersect → clamp` pair. Funnelling them through one
/// function keeps a new pan path from silently skipping the intersection
/// or the clamp. Each site still computes its own `candidate` (the parts
/// that legitimately differ — `z_new` vs committed zoom for the
/// zoom-coupled sites, `animation_target` vs live pan for the tween-
/// chaining sites — stay at the call site).
fn clamp_pan(
    candidate: Vec2,
    scene_bounds: Option<Rect>,
    view_bounds: Option<Rect>,
    viewport: Size,
    zoom: f32,
) -> Vec2 {
    let effective = intersect_pan_bounds(scene_bounds, view_bounds);
    clamp_pan_to_bounds(candidate, effective.as_ref(), viewport, zoom)
}

/// Apply a [`PanAxes`](crate::scene::PanAxes) policy to a candidate pan:
/// the permitted axis takes `candidate`, the restricted axis is held at
/// `hold` (the gesture's reference pan). `Both` passes through; `None`
/// holds both.
///
/// Shared by the pinch and hand-drag sites, which hold the orthogonal
/// axis at the pan captured when the gesture began, and by the camera's
/// `gate_pan_target`, which holds it at the current pan. The wheel and
/// keyboard sites instead zero their input *deltas* (so an excluded axis
/// passes through to ancestor scrollables / arrow-key guards) and do not
/// call this.
fn apply_pan_axes(candidate: Vec2, hold: Vec2, axes: crate::scene::PanAxes) -> Vec2 {
    use crate::scene::PanAxes;
    match axes {
        PanAxes::Both => candidate,
        PanAxes::None => hold,
        PanAxes::Horizontal => Vec2::new(candidate.x, hold.y),
        PanAxes::Vertical => Vec2::new(hold.x, candidate.y),
    }
}

/// In-flight marquee box-select state. Tracked in scene
/// coordinates so pan/zoom mid-drag (e.g. the user holds shift
/// and scrolls while dragging) doesn't break the rectangle's
/// alignment with scene contents.
#[derive(Debug, Clone, Copy)]
pub(super) struct MarqueeState {
    origin: Point,
    current: Point,
    /// Whether the marquee is additive (Ctrl/Shift held at start).
    /// On commit, additive = `extend`; non-additive = `replace`.
    additive: bool,
}

impl MarqueeState {
    fn rect(self) -> Rect {
        let x = self.origin.x.min(self.current.x);
        let y = self.origin.y.min(self.current.y);
        let w = (self.origin.x - self.current.x).abs();
        let h = (self.origin.y - self.current.y).abs();
        Rect::new(x, y, w, h)
    }
}

/// A drag-to-move in flight: which lightweight item is being
/// translated, in scene coords. The committed delta on `Ended`
/// is `current_scene - anchor_scene`; that delta is applied to
/// the target item *and* every declared descendant via
/// `Scene::collect_descendants`.
#[derive(Debug, Clone, Copy)]
pub(super) struct DragTarget {
    item_id: ItemId,
    /// Scene-coord position where the drag started.
    anchor_scene: Point,
    /// Current scene-coord position (updated on each Moved /
    /// Ended). Allows paint to render the in-flight offset for
    /// live visual feedback.
    ///
    /// **A pointer position, not an item position.** Both anchors are the press
    /// point, and the pair is only ever read as a difference — the grab offset
    /// between the cursor and the item's own corner lives in neither of them.
    /// That is exactly why the geometry constraint is stated over
    /// [`start_frame`](Self::start_frame) instead: snapping a cursor to a grid
    /// leaves the item off-grid by the grab offset, for ever.
    current_scene: Point,
    /// The scene-space box the dragged group occupied when the drag started —
    /// [`Scene::transform_frame`](crate::Scene::transform_frame) over the whole
    /// `drag_group`, captured once at the press and fixed for the gesture.
    ///
    /// The `start` a geometry constraint measures against, and the quantity
    /// that makes a grid snap land the *item* on the grid.
    ///
    /// `None` when the scene declares **no** geometry constraint — capturing it
    /// costs a full prune of the selection and nothing else reads it, so a
    /// scene with no rule pays nothing for the mechanism, at the press as well
    /// as on every sample — or when the group resolved to no geometry at all,
    /// which, since the hit test had just found the grabbed item, means it was
    /// removed between the two. Either way no constraint runs for this drag.
    start_frame: Option<crate::transform_session::TransformFrame>,
}

/// The items a pointer drag of `grabbed` carries — see
/// [`SceneView::drag_group`], which is this function with the view's own
/// handles.
///
/// A free function because the drag closure owns clones rather than a `&self`,
/// and the closure's answer must be the *same* answer the commit and the paint
/// feedback get: a second implementation there would be a second definition of
/// "what is moving".
pub(super) fn drag_group_of(
    model: &SceneModel,
    selection: &crate::selection::SceneSelection,
    grabbed: ItemId,
) -> Vec<ItemId> {
    if !selection.is_selected(grabbed) {
        return vec![grabbed];
    }
    let selected = selection.selected();
    if selected.len() < 2 {
        return vec![grabbed];
    }
    model.selection_roots(&selected)
}

/// Snapshot of one item's hit-test geometry + handler closures used
/// by the SceneView's `on_pointer_event` dispatch path. Refreshed
/// per layout pass alongside `lightweight_bounds_snapshot`.
#[derive(Clone)]
struct HandlerSnapshotEntry {
    id: crate::item::ItemId,
    /// Scene-coord AABB used for broad-phase hit-test (normal items).
    scene_rect: Rect,
    /// Local→scene transform — used to inverse-project the
    /// scene-coord pointer into local coords for the narrow phase.
    /// Stored so the dispatch path doesn't have to re-walk the parent
    /// chain (which would need `&Scene`).
    scene_transform: teksilo_canvas::Transform2D,
    /// The item's geometry in local coordinates, cloned straight from
    /// [`SceneItem::shape`](crate::SceneItem::shape). One value, shared
    /// with every other query in the crate — the dispatch path and the
    /// eager `Scene::item_at` path cannot disagree about what an item is.
    shape: ItemShape,
    /// Where the entry sits in the view's single paint order. The snapshot is
    /// sorted by this descending, so the first shape match is the entry the
    /// user sees on top — band first, then z, then insertion.
    key: PaintKey,
    /// Whether this entry would *act* on a press ([`crate::pick::claims_press`]).
    /// Read only by the `Over`-band veto; the hit test itself still resolves
    /// the topmost **entry** and consults its handlers afterwards.
    claims_press: bool,
    /// Item-level handler closures, cloned at snapshot time. `None`
    /// when the item has no handler set installed.
    handlers: Option<Box<crate::item_handlers::SceneItemHandlerSet>>,
    /// `true` when the item carries `ItemFlags::IGNORES_TRANSFORMATIONS`.
    /// Dispatch routes hit-test through screen space: the visible
    /// area is `local_bounds` rooted at the screen-projected
    /// `scene_anchor`, and pan/zoom of the view don't change that
    /// area. `scene_rect` is meaningless for these items because
    /// they don't scale with zoom.
    ignores_xform: bool,
    /// For IGNORES items: the item's origin (local `(0,0)`) mapped
    /// to scene coords through the parent chain. The current view
    /// transform projects this to the screen-space anchor at
    /// dispatch time. For normal items, unused.
    scene_anchor: Point,
    /// For IGNORES items: the item's `local_bounds`. Combined with
    /// the screen anchor at dispatch time to form the screen-space
    /// AABB. For normal items, unused.
    local_bounds: Rect,
}

/// Hit-test geometry for one **draggable** lightweight item, snapshotted each
/// layout pass for the `on_drag` drag-start hit-test and the grab-cursor hover
/// check. Carries the item's [`ItemShape`] + transform (the same data
/// `HandlerSnapshotEntry` holds for tap/hover) so a press targets the item on
/// the item's **actual shape**, not merely its AABB — important for thin
/// draggable items (e.g. a connector path) whose bounding box is much larger than
/// the drawn stroke. z-sorted descending so the first shape match is the topmost.
///
/// A press that lands on no shape at all is offered to a *coarse* pointer's
/// bounded slop pass ([`GrabSlop`]), which widens each item's box rather than its
/// shape — but only by what a small target earns, so the middle of a large AABB
/// is still not a hit.
#[derive(Clone)]
struct DraggableSnapshotEntry {
    id: ItemId,
    scene_rect: Rect,
    scene_transform: teksilo_canvas::Transform2D,
    shape: ItemShape,
    ignores_xform: bool,
    scene_anchor: Point,
    local_bounds: Rect,
    /// Where the entry sits in the view's single paint order; the snapshot is
    /// sorted by this descending. See [`PaintKey`].
    key: PaintKey,
}

impl std::fmt::Debug for DraggableSnapshotEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DraggableSnapshotEntry")
            .field("id", &self.id)
            .field("scene_rect", &self.scene_rect)
            .finish_non_exhaustive()
    }
}

/// The topmost draggable item whose **shape** contains the pointer (narrow
/// phase), or the nearest one this pointer's slop reaches, or `None`.
///
/// Mirrors the `hit_handler_item` logic used for tap/hover dispatch: AABB
/// broad-phase, then inverse-project to local and consult the item's
/// [`ItemShape`], with a screen-space branch for `IGNORES_TRANSFORMATIONS`
/// items. `snap` must be sorted by [`PaintKey`] descending (topmost first). A
/// total miss falls through to `slop`'s miss-only pass, which is inert for a
/// mouse.
///
/// `floor` is the paint-order floor: entries below it are invisible to this
/// hit test. The drag path passes the key of whatever press claimant vetoed a
/// card, so a press that only reached this view because of that veto cannot
/// grab something the card was covering.
fn hit_draggable_item(
    snap: &[DraggableSnapshotEntry],
    screen_pt: Point,
    scene_pt: Point,
    view_xform: teksilo_canvas::Transform2D,
    slop: GrabSlop,
    floor: PaintKey,
) -> Option<ItemId> {
    let view_scale = view_xform.m[0].hypot(view_xform.m[1]);
    for entry in snap.iter() {
        if entry.key < floor {
            continue;
        }
        if entry.ignores_xform {
            let screen_anchor = view_xform.apply_point(entry.scene_anchor);
            let screen_rect = Rect::new(
                screen_anchor.x + entry.local_bounds.x,
                screen_anchor.y + entry.local_bounds.y,
                entry.local_bounds.width,
                entry.local_bounds.height,
            );
            if !screen_rect.contains(screen_pt) {
                continue;
            }
            let local_pt = Point::new(screen_pt.x - screen_anchor.x, screen_pt.y - screen_anchor.y);
            // Screen-anchored items ignore the view transform → unit scale.
            if entry.shape.contains(local_pt, 1.0) {
                return Some(entry.id);
            }
            continue;
        }
        if !entry.scene_rect.contains(scene_pt) {
            continue;
        }
        let local_pt = entry
            .scene_transform
            .inverse()
            .map(|inv| inv.apply_point(scene_pt))
            .unwrap_or(Point::ZERO);
        if entry.shape.contains(local_pt, view_scale) {
            return Some(entry.id);
        }
    }
    slop.nearest_draggable(snap, screen_pt, scene_pt, view_xform, floor.rank())
}

/// The topmost item under the pointer whose **shape** contains it, out of a
/// handler snapshot, or `None`.
///
/// Two hit spaces, one per item kind. A normal item is broad-phased against its
/// scene-coord AABB and narrow-phased by inverse-projecting the pointer into
/// item-local coordinates; an `IGNORES_TRANSFORMATIONS` item is pinned at a
/// screen position, so it is broad-phased against its projected screen rect and
/// narrow-phased at unit scale. `snap` must be sorted by [`PaintKey`]
/// descending, so the first containing entry is the topmost one.
///
/// `floor` is the paint-order floor, compared as a whole [`PaintKey`] so a
/// caller can say "strictly above *this* entry" and not only "at least this
/// rank". The dispatch site passes [`RANK_OVER`] (as a rank floor) when the
/// arena has given this pointer to a heavyweight card — only the `Over` band
/// outranks *every* card, so everything below it must stay invisible and let
/// the card have the event — and [`RANK_UNDER`](crate::pick::RANK_UNDER) when the view itself is the
/// target, which is precisely when no card won. The `accepts_child_hit` veto
/// passes one specific card's key, because an
/// [`Interleaved`](crate::SceneLayer::Interleaved) entry is above some cards
/// and below others and a rank cannot say which.
///
/// A total miss falls through to `slop`'s miss-only pass, which is inert for a
/// mouse.
fn hit_handler_index(
    snap: &[HandlerSnapshotEntry],
    screen_pt: Point,
    scene_pt: Point,
    view_xform: teksilo_canvas::Transform2D,
    slop: GrabSlop,
    floor: PaintKey,
) -> Option<usize> {
    // Logical view zoom (uniform scale of the linear part) — passed to each
    // item's shape test so a cosmetic (device-pixel) stroke's clickable band is
    // converted to scene coordinates at the current zoom.
    let view_scale = view_xform.m[0].hypot(view_xform.m[1]);
    for (index, entry) in snap.iter().enumerate() {
        if entry.key < floor {
            continue;
        }
        if entry.ignores_xform {
            let screen_anchor = view_xform.apply_point(entry.scene_anchor);
            let screen_rect = Rect::new(
                screen_anchor.x + entry.local_bounds.x,
                screen_anchor.y + entry.local_bounds.y,
                entry.local_bounds.width,
                entry.local_bounds.height,
            );
            if !screen_rect.contains(screen_pt) {
                continue;
            }
            let local_pt = Point::new(screen_pt.x - screen_anchor.x, screen_pt.y - screen_anchor.y);
            // Screen-anchored items ignore the view transform → unit scale.
            if entry.shape.contains(local_pt, 1.0) {
                return Some(index);
            }
            continue;
        }
        if !entry.scene_rect.contains(scene_pt) {
            continue;
        }
        let local_pt = entry
            .scene_transform
            .inverse()
            .map(|inv| inv.apply_point(scene_pt))
            .unwrap_or(Point::ZERO);
        if entry.shape.contains(local_pt, view_scale) {
            return Some(index);
        }
    }
    slop.nearest_handler_index(snap, screen_pt, scene_pt, view_xform, floor.rank())
}

/// [`hit_handler_index`] resolved to an owned entry, for the dispatch path,
/// which needs the handler closures and cannot hold the snapshot borrow across
/// a callback.
///
/// The index form is the one the `Over`-band veto uses: it asks only whether
/// the winner claims the press, and cloning an entry to answer that would put a
/// heap allocation (the boxed handler set) on every pointer sample — on a path
/// whose whole reason for existing is that it allocates nothing.
fn hit_handler_item(
    snap: &[HandlerSnapshotEntry],
    screen_pt: Point,
    scene_pt: Point,
    view_xform: teksilo_canvas::Transform2D,
    slop: GrabSlop,
    floor: PaintKey,
) -> Option<HandlerSnapshotEntry> {
    hit_handler_index(snap, screen_pt, scene_pt, view_xform, slop, floor)
        .map(|index| snap[index].clone())
}

/// The pointer-kind half of the grab hit test: a build-time
/// [`InputTokens`](teksilo_tokens::InputTokens)
/// snapshot plus the kind of the pointer the event being handled came from.
///
/// Carried as one value so the exact-pass hit tests take one extra argument
/// rather than two, and so the *miss-only* rule has a single place to live:
/// every method here runs after an exact pass has already missed, and none of
/// them can report a hit an exact pass had found.
#[derive(Debug, Clone, Copy)]
struct GrabSlop {
    tokens: teksilo_tokens::InputTokens,
    kind: teksilo_tokens::PointerKind,
}

impl GrabSlop {
    /// The slop a pointer of `kind` earns against `tokens`.
    fn new(tokens: teksilo_tokens::InputTokens, kind: teksilo_tokens::PointerKind) -> Self {
        Self { tokens, kind }
    }

    /// Whether this pointer can earn no widening at all — `true` for a mouse at
    /// every density, since the mouse profile's slop radius is `0.0`. Asked
    /// first in each pass, so a mouse never walks the snapshot.
    fn offers_nothing(&self) -> bool {
        teksilo_core::pointer::hit_slop::HitSlop::for_pointer(self.kind, &self.tokens).is_none()
    }

    /// The tap-versus-drag movement tolerance for this pointer, in view pixels.
    fn tap_tolerance_px(&self) -> f32 {
        tap_tolerance_px(&self.tokens, self.kind)
    }

    /// The nearest draggable item whose *inflated* bounds contain the press.
    ///
    /// Deliberately the inflated **box**, not the shape: an item whose shape is
    /// narrower than its box — a connector stroke, a diagonal path — is exactly
    /// the case this pass exists for, and re-testing the shape against a widened
    /// point would answer the same "no" the exact pass just did.
    fn nearest_draggable(
        &self,
        snap: &[DraggableSnapshotEntry],
        screen_pt: Point,
        scene_pt: Point,
        view_xform: teksilo_canvas::Transform2D,
        min_rank: u8,
    ) -> Option<ItemId> {
        if self.offers_nothing() {
            return None;
        }
        let view_scale = view_xform.m[0].hypot(view_xform.m[1]);
        let mut best: Option<(f32, ItemId)> = None;
        for entry in snap.iter() {
            if entry.key.rank() < min_rank {
                continue;
            }
            let Some(distance) = self.miss_distance(
                entry.ignores_xform,
                entry.scene_rect,
                entry.local_bounds,
                entry.scene_anchor,
                screen_pt,
                scene_pt,
                view_xform,
                view_scale,
            ) else {
                continue;
            };
            if best.is_none_or(|(best_d, _)| distance < best_d) {
                best = Some((distance, entry.id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// Distance from the press to `entry`'s bounds, in the space the comparison
    /// runs in, or `None` when the press is further away than the slop this
    /// entry earns.
    ///
    /// A screen-anchored item is measured in view pixels at unit scale; every
    /// other item is measured in **scene** units against its scene-space box,
    /// with the earned slop converted by the live zoom — so a rotated view needs
    /// no transformed-box arithmetic and the tolerance is still the same number
    /// of view pixels at every zoom.
    #[allow(clippy::too_many_arguments)]
    fn miss_distance(
        &self,
        ignores_xform: bool,
        scene_rect: Rect,
        local_bounds: Rect,
        scene_anchor: Point,
        screen_pt: Point,
        scene_pt: Point,
        view_xform: teksilo_canvas::Transform2D,
        view_scale: f32,
    ) -> Option<f32> {
        if ignores_xform {
            let anchor = view_xform.apply_point(scene_anchor);
            let screen_rect = Rect::new(
                anchor.x + local_bounds.x,
                anchor.y + local_bounds.y,
                local_bounds.width,
                local_bounds.height,
            );
            let slop = grab_slop_px(&self.tokens, self.kind, screen_rect.size());
            let distance = distance_to_rect(screen_pt, screen_rect);
            return (slop > 0.0 && distance <= slop).then_some(distance);
        }
        if !(view_scale.is_finite() && view_scale > 1e-6) {
            return None;
        }
        let screen_size = Size::new(
            scene_rect.width * view_scale,
            scene_rect.height * view_scale,
        );
        let slop_scene = grab_slop_px(&self.tokens, self.kind, screen_size) / view_scale;
        let distance = distance_to_rect(scene_pt, scene_rect);
        (slop_scene > 0.0 && distance <= slop_scene).then_some(distance)
    }

    /// The nearest item whose *inflated* bounds contain the press, out of a
    /// handler snapshot, as a position in it. The tap/hover twin of
    /// [`nearest_draggable`](Self::nearest_draggable), with the same miss-only
    /// and inflated-box rules.
    fn nearest_handler_index(
        &self,
        snap: &[HandlerSnapshotEntry],
        screen_pt: Point,
        scene_pt: Point,
        view_xform: teksilo_canvas::Transform2D,
        min_rank: u8,
    ) -> Option<usize> {
        if self.offers_nothing() {
            return None;
        }
        let view_scale = view_xform.m[0].hypot(view_xform.m[1]);
        let mut best: Option<(f32, usize)> = None;
        for (index, entry) in snap.iter().enumerate() {
            if entry.key.rank() < min_rank {
                continue;
            }
            let Some(distance) = self.miss_distance(
                entry.ignores_xform,
                entry.scene_rect,
                entry.local_bounds,
                entry.scene_anchor,
                screen_pt,
                scene_pt,
                view_xform,
                view_scale,
            ) else {
                continue;
            };
            if best.is_none_or(|(best_d, _)| distance < best_d) {
                best = Some((distance, index));
            }
        }
        best.map(|(_, index)| index)
    }

    /// The scene-unit radius within which a magnet handle may be grabbed.
    ///
    /// The declared [`MagnetismConfig::capture_px`](crate::MagnetismConfig::capture_px)
    /// is the whole radius for a mouse; every other pointer adds what a disc of
    /// that diameter earns from its own profile — nothing for a mouse, a pen's
    /// small allowance for a pen, the full offer for a finger — so a port dot
    /// stays grabbable without any app-side number. The four *snap-arrival* radii
    /// are left alone: they are how close a dragged thing has to come before it
    /// snaps, which is feel, not reach.
    fn magnet_grab_scene_radius(&self, capture_px: f32, zoom: f32) -> f32 {
        let diameter = (capture_px * 2.0).max(0.0);
        let px = capture_px + grab_slop_px(&self.tokens, self.kind, Size::new(diameter, diameter));
        px / zoom
    }
}

/// Visual debug overlays painted on top of normal scene rendering.
///
/// Every flag defaults to `false`. Use this to verify that culling /
/// hit-test / spatial-index / dragging are doing what you expect
/// while developing a scene-based feature; turn off before shipping.
///
/// Each flag adds a thin overlay paint with a distinct color so
/// multiple flags can be combined without visual confusion:
///
/// - [`item_bounds`](Self::item_bounds): green outline around every
///   visible scene item's `bounds_in_scene`.
/// - [`content_bounds`](Self::content_bounds): blue outline around
///   the scene's overall content extent (the union of all item
///   bounds).
/// - [`viewport`](Self::viewport): red outline around the visible
///   scene region (the cull rect — the inverse-projected viewport).
/// - [`selection_bounds`](Self::selection_bounds): orange outline
///   around every currently-selected item.
#[derive(Debug, Clone, Copy, Default)]
pub struct DebugOverlay {
    pub item_bounds: bool,
    pub content_bounds: bool,
    pub viewport: bool,
    pub selection_bounds: bool,
}

impl DebugOverlay {
    /// All overlays enabled. Useful to catch any anomaly visually.
    pub const ALL: DebugOverlay = DebugOverlay {
        item_bounds: true,
        content_bounds: true,
        viewport: true,
        selection_bounds: true,
    };

    /// Whether at least one debug overlay is enabled.
    pub fn is_active(&self) -> bool {
        self.item_bounds || self.content_bounds || self.viewport || self.selection_bounds
    }
}

/// Direction passed to a [`SceneView::focus_order`] callback when the
/// app wants to override the default Tab cycle.
///
/// `Forward` corresponds to Tab; `Backward` to Shift+Tab. The default
/// SceneView focus traversal is scene insertion order — apps that
/// need data-flow order (graph editor), story-order (corkboard with
/// Acts), chronological order (timeline), etc. install a callback
/// that receives the current focus and returns the next id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    /// Advance to the next item — corresponds to the Tab key.
    Forward,
    /// Retreat to the previous item — corresponds to Shift+Tab.
    Backward,
}

/// A pannable/zoomable viewport that renders a [`Scene`]'s items at scene
/// coordinates and routes user input (scroll, pinch, drag, keyboard) back into
/// the camera signals.
///
/// Construct with [`SceneView::new`] (single-view sugar: wraps a [`Scene`] in a
/// fresh [`SceneModel`]) or [`SceneView::with_model`] (multi-view: several
/// viewports share one [`SceneModel`] and each reconcile independently on every
/// mutation). Install a heavyweight builder for delegated items via
/// [`delegate_typed`](Self::delegate_typed). Add to a [`WidgetTree`](teksilo_core::widget_tree::WidgetTree)
/// like any other widget; gestures and camera animations are wired automatically
/// during [`build`](teksilo_core::widget::Widget::build).
///
/// See the [module-level documentation](crate) for the full composition model
/// and `docs/teksilo-scene.md` for an end-to-end guide.
pub struct SceneView {
    /// Shared, cloneable handle to the scene this view renders. Multiple
    /// `SceneView`s can hold clones of one [`SceneModel`] and reconcile
    /// independently on every mutation.
    model: SceneModel,
    /// Per-view heavyweight builder for `Delegated` items. Each view calls
    /// its own delegate with an item's type-erased payload to build a fresh
    /// `Widget` instance for **this** view's arena. Returns `None` to skip
    /// an item (e.g. a downcast miss). `None` (the field) = no delegate
    /// installed; only single-view `Once` widgets materialise.
    delegate: Option<Rc<dyn Fn(&dyn std::any::Any, ItemId) -> Option<Box<dyn Widget>>>>,
    /// Items whose payload changed since the last build (filled by the
    /// `item_change` observer on `ItemChange::PayloadChanged`). Drained at
    /// the top of `build`, where each is destroyed and re-materialised via the
    /// delegate. `Rc<RefCell>` so the observer closure can push without
    /// borrowing `model`.
    payload_dirty: Rc<RefCell<HashSet<ItemId>>>,
    /// Materialisation map populated during `build`. Stable across
    /// rebuilds — subsequent `build` calls just return the cached
    /// widget ids.
    materialized: HashMap<ItemId, WidgetId>,
    /// Reverse lookup populated alongside `materialized` so the
    /// per-frame `place_children` cull resolves
    /// `WidgetId → ItemId` in `O(1)`. Without it, scaling the demo
    /// to 5,000 cards would burn a full frame's budget on the
    /// per-child entry scan.
    widget_to_item: HashMap<WidgetId, ItemId>,
    /// The heavyweight cards this view publishes to assistive technology, as
    /// decided by the most recent `place_children`. `None` until the view has
    /// been laid out once — "no decision yet", which means publish everything.
    ///
    /// A **subset** of the cards the same pass kept alive, and deliberately so.
    /// Staying alive and being enumerated are two different questions with two
    /// different answers: [`retention_margin`](SceneView::retention_margin) is
    /// a lifecycle hint that keeps a card just off the edge warm so the camera
    /// never reaches a hole, while
    /// [`a11y_off_screen_mode`](SceneView::a11y_off_screen_mode) is the app's
    /// statement about how much of an off-screen scene a screen reader should
    /// be offered. A card in the margin band but outside the mode's region is
    /// alive — laid out, Tab-reachable, holding its state — and absent from
    /// the AT tree.
    ///
    /// The one exception is a card the user is *in the middle of*: it is
    /// pinned into this set wherever the camera goes, because the published
    /// tree names the focused node and a focus pointing at a node the walk did
    /// not emit is a broken tree, not a missing one.
    ///
    /// Written by the layout pass and read by the accessibility walk so the
    /// two cannot disagree about a card: the walk asks the recorded set rather
    /// than recomputing a predicate that might round differently or miss the
    /// pins. Paired with [`at_children`](Self::at_children), written in the
    /// same loop from the same decision.
    at_heavy: Rc<RefCell<Option<HashSet<ItemId>>>>,
    /// The same decision as [`at_heavy`](Self::at_heavy), as the ordered arena
    /// child list `Widget::accessibility_children` hands the framework walker.
    ///
    /// This is what actually suppresses a live-but-unenumerated card: the
    /// walker uses this list for BOTH the child push and the recursion, so a
    /// card left out of it is neither named by a parent nor emitted as a node
    /// — no orphan, no dangling child, nothing for the framework's strip to
    /// clean up after. Order is the arena's own child order (z-order), so what
    /// a screen reader reads is unchanged apart from the omissions.
    at_children: Rc<RefCell<Option<Vec<WidgetId>>>>,
    /// How far past the viewport edge, in **screen** pixels, a heavyweight
    /// card stays live. See [`SceneView::retention_margin`].
    retention_margin: f32,
    /// Live mirror of `bounds.origin` (the SceneView's screen-space
    /// position as decided by its parent layout). Updated in
    /// `place_children` and folded into the view-transform composition
    /// so a SceneView positioned at a non-zero parent offset still
    /// places its children correctly under pan / zoom / rotation.
    /// Without this, zoom would multiply `bounds.origin` and the
    /// content would visually drift away from the viewport.
    bounds_origin_signal: Signal<Vec2>,
    /// Fallback size when the parent's `SizeProposal` is unspecified
    /// on either axis.
    default_size: Size,
    /// When `true`, [`SceneView::layout_response`] returns the
    /// scene's `scene_rect_extent` as the view's wanted size — the
    /// view sizes itself to its scene. User pan / zoom / drag-to-
    /// move are still gated by [`Scene::pan_axes`] and
    /// [`Scene::is_zoomable`]; the default policy is "no pan, no
    /// zoom" because the entire scene is already on-screen.
    adopt_scene_size: bool,
    /// Drag-on-canvas behavior. Default `RubberBand` (item drag →
    /// move; empty area → marquee). `ScrollHandDrag` makes the
    /// canvas pan unconditionally on left-mouse drag; `NoDrag`
    /// disables the on-drag handler entirely.
    /// Drag mode (rubber-band marquee, scroll-hand pan, or no-drag).
    /// Reactive: gesture handlers read this per event, so mutating
    /// the signal at runtime (typically from a toolbar) flips
    /// behaviour on the next pointer event without rebuilding
    /// the view. `.drag_mode(mode)` writes to it directly;
    /// `.drag_mode(sig)` replaces the inner signal with an
    /// app-owned one so toolbars can share state with the view.
    drag_mode: Signal<crate::item_handlers::DragMode>,
    /// Per-layout snapshot of (id, scene_rect, handlers) for items
    /// that have a handler set installed. Used by the
    /// `on_pointer_event` closure to dispatch hover / tap / context
    /// menu without borrowing `&self.scene`. Refreshed in
    /// `layout_response`.
    handler_snapshot: Rc<RefCell<Vec<HandlerSnapshotEntry>>>,
    /// Currently-hovered item id, used to dispatch `on_hover(false)`
    /// when the pointer leaves it.
    hovered_item: Rc<Cell<Option<crate::item::ItemId>>>,
    /// Last press recorded for tap detection: `(press point in VIEW pixels,
    /// item_id, button)`. Cleared on PointerUp / PointerLeave.
    ///
    /// View pixels rather than scene units so the tap-versus-drag tolerance is
    /// the same physical distance at every zoom, and so a zoom that changes
    /// between the press and the release cannot make the two points
    /// incomparable — see [`TAP_MOVEMENT_THRESHOLD`].
    pending_tap: Rc<
        Cell<
            Option<(
                Point,
                crate::item::ItemId,
                teksilo_core::event::PointerButton,
            )>,
        >,
    >,
    /// Latest viewport size observed during layout. Cached so
    /// imperative methods like [`SceneView::fit_to_content`] can
    /// reason about the visible rectangle without re-running layout.
    /// `Rc<Cell>` so event-handler closures (e.g. Ctrl+wheel zoom-
    /// about-viewport-center) can read it without touching `&mut self`.
    /// Last viewport size resolved by `layout_response`. Stored as a
    /// `Signal` (not `Cell`) so derived signals like
    /// [`viewport_in_scene_signal`](Self::viewport_in_scene_signal)
    /// can react to viewport changes. Writes are gated by an
    /// equality check at the call site to avoid notifying on
    /// unchanged values.
    last_viewport: Signal<Size>,

    // --- View transform state ---------------------------------
    pan_x: Signal<f32>,
    pan_y: Signal<f32>,
    zoom: Signal<f32>,
    rotation: Signal<f32>,

    // --- View configuration ----------------------------------------
    /// View-level *tightening* override on the underlying
    /// [`Scene`]'s zoom range. The effective clamp applied at
    /// gesture / set_zoom / pan_to time is the intersection of
    /// `Scene::current_zoom_range()` and this override (see
    /// `effective_zoom_range`). `None` means the view does not
    /// constrain zoom; the default is `Some(0.1..=10.0)` so
    /// existing callers see the historical clamp behaviour.
    zoom_range_override: Signal<Option<std::ops::RangeInclusive<f32>>>,
    /// View-level *tightening* override on the underlying
    /// [`Scene`]'s pan bounds. The effective clamp is the rect
    /// intersection of `Scene::current_pan_bounds()` and this
    /// override. `None` (the default) leaves pan unconstrained
    /// from the view side.
    pan_bounds_override: Signal<Option<Rect>>,
    pan_anim_duration: Duration,
    zoom_anim_duration: Duration,
    line_height: f32,
    /// Whether a wheel that the scene can't absorb (already clamped at its
    /// `pan_bounds`) chains to an ancestor scrollable, or is contained.
    overscroll_behavior: OverscrollBehavior,

    // --- A11y configuration — visual-default path ----------------------------------
    a11y_off_screen_mode: crate::a11y::A11yOffScreenMode,

    // --- A11y configuration — logical structural API ----------------------------------
    /// Cooperative (default) vs StrictlyParallel.
    a11y_mode: crate::a11y::A11yMode,
    /// SceneView's own arena `WidgetId`, captured during the first
    /// `build()`. Needed by `a11y_redirect_descendant` to compute
    /// the synthetic `NodeId` of a declared logical parent group
    /// (the hash key is `(self_id, group_id, SyntheticKind::SceneGroup)`).
    /// `Cell` because the trait method is `&self`.
    self_widget_id: Cell<Option<WidgetId>>,

    // --- Interactivity ------------------------------------------------
    /// When `false`, `build()` skips registering scroll / pinch /
    /// keyboard handlers and does not mark the SceneView focusable.
    /// Programmatic `pan_to` / `zoom_to` still work — this only
    /// gates user-driven navigation. Used by chart-style nested
    /// scenes where the outer container is purely decorative
    /// (axis chrome around an inner data SceneView).
    interactive: bool,

    // --- Selection -----------------------------------------
    /// Reactive selection state. Defaults to `SceneSelectionMode::None`
    /// (no selection wired). Apps opt in via
    /// [`selection_mode`](Self::selection_mode); marquee + click-to-
    /// select then activate.
    selection: crate::selection::SceneSelection,
    /// In-flight marquee state: scene-coord origin + current. While
    /// `Some`, `paint` overlays a semi-transparent rect.
    /// `Rc<Cell>` so the on_drag closure (which only borrows
    /// `&self` shape via the closure's capture) can mutate it.
    marquee: Rc<Cell<Option<MarqueeState>>>,
    /// Pending marquee commit: set by the on_drag closure on
    /// `DragPhase::Ended`, consumed at the start of the next
    /// `place_children` (which has direct `&self.scene` access
    /// via `self`). This indirection avoids forcing `Scene` into
    /// an `Rc<RefCell>`.
    pending_marquee_commit: Rc<RefCell<Option<(SceneRegion, ItemSelectionMode, bool)>>>,
    /// Which rule the rubber band picks items by. Default
    /// [`ItemSelectionMode::IntersectsItemShape`] — see
    /// [`SceneView::marquee_selection_mode`].
    marquee_mode: ItemSelectionMode,
    /// In-flight drag-to-move state: which item is being dragged
    /// and the scene-coord anchor where the drag started. The
    /// total scene-coord delta is computed at `Ended` from
    /// `current - anchor` and posted to `pending_item_move`.
    /// `Rc<Cell>` so the on_drag closure can mutate via `&self`.
    drag_target: Rc<Cell<Option<DragTarget>>>,
    /// Pending drag-to-move commit: `(target_id, delta)` set by
    /// the on_drag `Ended` branch, drained in `build`. The drain
    /// code translates the target item AND every descendant
    /// (declared via `Scene::set_item_parent`) by the same delta
    /// — so a labelled rectangle (Rect parent + TextItem child)
    /// moves as one unit, QGraphicsScene-style.
    pending_item_move: Rc<Cell<Option<(ItemId, Vec2)>>>,
    /// Snapshot of **draggable** lightweight scene items + their narrow-phase
    /// hit geometry, used by the on_drag drag-start hit-test and the grab-cursor
    /// hover check. Refreshed in `place_children` each layout pass — the
    /// snapshot stays consistent within a single drag and refreshes between
    /// drags via the spatial-index mutation triggering relayout. Avoids forcing
    /// `Scene` into an `Rc<RefCell>`.
    lightweight_bounds_snapshot: Rc<RefCell<Vec<DraggableSnapshotEntry>>>,
    /// Bumped by the on_drag closure on `Ended` after posting a
    /// `pending_item_move`. SceneView binds to this at
    /// `BindingLevel::Rebuild` in `build`, so the next build
    /// cycle drains the pending move and calls
    /// `Scene::set_local_pos` (which requires `&mut self.scene`,
    /// only available inside `build`). Without this signal, the
    /// move was queued but never applied — items "snapped back"
    /// to their original positions on drag release.
    reconcile_dirty: Signal<u64>,

    /// Bumped by the `item_change_signal` observer on an
    /// [`ItemChange::AppearanceChanged`](crate::ItemChange::AppearanceChanged).
    /// Bound at `BindingLevel::RepaintOnly` in `build`, so a lightweight item's
    /// live colour/style change repaints the view (re-running `paint_band` →
    /// `item.paint`) **without** a relayout or rebuild — the cheap path for a
    /// pure appearance mutation.
    appearance_dirty: Signal<u64>,

    /// Bumped by the `item_change_signal` observer on an
    /// [`ItemChange::MeasuredSizeChanged`](crate::ItemChange::MeasuredSizeChanged).
    /// Bound at `BindingLevel::Relayout` in `build`.
    ///
    /// **Relayout, not rebuild**, and that is the whole point of the separate
    /// signal. A measured size moves geometry and nothing else: nothing was
    /// materialised, nothing reaped, no delegate needs re-running, and the
    /// change is counted out of `structural_version` so it buys no AccessKit
    /// re-walk either. Answering a paragraph's re-wrap with a full
    /// `SceneView::build()` — three observers re-installed, every drain re-run,
    /// the heavyweight list re-derived and re-sorted — is the storm this exists
    /// to avoid, and a note page produces one per line break.
    ///
    /// The view that *made* the write is notified too, and that is deliberate:
    /// it is what makes a **second** view of the same model relayout at all.
    /// The writer's extra pass is bounded by the equality guard in
    /// `Scene::set_measured_size` — it measures the same height, writes
    /// nothing, and stops.
    measure_dirty: Signal<u64>,

    /// Per-item measurement history for the non-[`Fixed`](crate::SizePolicy)
    /// size policies, and the convergence guard.
    ///
    /// Per **view**, not per model: the measurement comes from a widget
    /// instance, and `delegate_typed` builds one instance per view.
    ///
    /// The last answer, the width it was taken at, and how often the body has
    /// contradicted itself since anything but this view last reached it. A
    /// body whose `layout_response` is not a function of its width alone would
    /// otherwise write A, then B, then A forever, each write a relayout. See
    /// [`MeasureTrack`] for how an oscillation is told apart from an edit, and
    /// [`SizePolicy`](crate::SizePolicy) for the obligation itself.
    ///
    /// Behind an `Rc` because two things hold it: the (`&self`) layout pass
    /// writes it, and the `item_change_signal` observer clears an entry's
    /// history when its policy changes or its item is removed — a closure that
    /// outlives the `&self` that installed it.
    measure_state: Rc<RefCell<HashMap<ItemId, MeasureTrack>>>,

    /// Latest pointer position seen on the SceneView (screen-space).
    /// Updated via an on_pointer_event handler in `build`. Used by
    /// Ctrl+wheel zoom to zoom-about-pointer instead of zoom-about-
    /// viewport-center, which is the natural feel users expect (the
    /// scene point under the cursor stays put).
    /// `None` until the first pointer event arrives — Ctrl+wheel
    /// before any pointer event falls back to viewport center.
    cursor_pos: Rc<Cell<Option<Point>>>,

    /// App-supplied focus-order callback. When set, the public
    /// [`next_focus`](Self::next_focus) /
    /// [`previous_focus`](Self::previous_focus) accessors route
    /// through it instead of falling back to insertion order.
    /// `Rc<dyn Fn>` so callers can clone the SceneView while
    /// keeping the closure shared.
    focus_order_callback:
        Option<Rc<dyn Fn(&Scene, FocusDirection, Option<ItemId>) -> Option<ItemId>>>,

    /// Whether this SceneView is logically nested inside another
    /// (chart-style outer chrome + inner data scene, or a preview
    /// pane inside a parent scene). Default `false` — every
    /// SceneView reports itself as a top-level `Role::Pane`. When
    /// `true`, the AT walker reports `Role::Region` instead so
    /// screen readers don't announce redundant landmarks.
    a11y_nested: bool,
    /// Optional label announced as the SceneView's own AT name.
    /// When set, becomes the logical region name (e.g. "Chart
    /// data area" for an inner chart SceneView). Default `None`
    /// — the SceneView has no explicit name.
    a11y_label: Option<LocalizedString>,
    /// Debug overlay configuration. Default: all flags `false`
    /// — no debug paint. When any flag is set, the SceneView
    /// paints visual diagnostics (item bounding boxes,
    /// content extent, viewport rect, etc.) on top of normal
    /// scene rendering. Use to verify culling, hit-test, and
    /// spatial-index behavior; intended for development only,
    /// don't ship with this on.
    debug_overlay: DebugOverlay,

    // --- Cached derived signals ---------------------------------------
    /// `view_transform` as a derived `Signal<Transform2D>`,
    /// constructed once in `new()` and reused across rebuilds.
    /// Exposed via [`view_transform_signal`](Self::view_transform_signal)
    /// so consumers (e.g. axis labels in a parent SceneView) can
    /// bind to it reactively without taking a snapshot every paint.
    view_transform_signal: Signal<Transform2D>,

    // --- Background / foreground paint hooks --------------------------
    /// App-supplied closure painted **before** the items walk. The
    /// canvas already has the view-transform scope pushed, so the
    /// closure paints in scene coords. The `Rect` argument is the
    /// scene-coord visible region — useful for "every-N-units" tiled
    /// backgrounds (graph-paper grids, ruled lines, dot grids) so the
    /// closure only emits geometry the user can actually see.
    background_paint: Option<Rc<dyn Fn(&mut teksilo_canvas::Canvas, &PaintContext, Rect)>>,
    /// App-supplied closure painted **after** the items walk and the
    /// marquee, but before the debug overlay. Same coordinate
    /// conventions as `background_paint`. Used for scene-coord
    /// chrome that should ride over content (rulers, snap-line
    /// indicators, drop hints).
    foreground_paint: Option<Rc<dyn Fn(&mut teksilo_canvas::Canvas, &PaintContext, Rect)>>,

    // --- Item-coordinate paint cache ----------------------------------
    /// Per-item paint cache for items that opted into
    /// [`CacheMode::ItemCoordinate`](crate::cache::CacheMode::ItemCoordinate).
    /// Keyed by `ItemId`; the entry stores a [`RenderFrame`](teksilo_canvas::RenderFrame)
    /// recorded in the item's local coordinates and replayed via
    /// `Canvas::draw_render_frame` when valid. Invalidated by an
    /// observer on [`Scene::item_change_signal`](crate::Scene::item_change_signal):
    /// `LocalBoundsChanged` / `OpacityChanged` / `Removed` for an id
    /// drop that id's entry. Apps that mutate item-internal state
    /// outside of `Scene` mutators must call
    /// [`SceneView::invalidate_item_cache`] to evict.
    pub(crate) item_cache: Rc<RefCell<crate::cache::ItemCoordinateCache>>,
    /// The viewport in scene coordinates, republished every `place_children`
    /// and every `paint`, so a child paint node can read it.
    ///
    /// A `SceneBandProxy` or a `WetLayerNode` knows its own rect and not the
    /// viewport's, and an item reads the region as a hint to skip geometry it
    /// cannot show. See [`ScenePaintBridge`](paint_node::ScenePaintBridge).
    published_visible_region: Rc<Cell<Rect>>,
    /// Every child's [`PaintKey`], as of the last build.
    ///
    /// Read on the pointer's hot path by
    /// [`accepts_child_hit`](SceneView::accepts_child_hit), which runs inside
    /// the router where the scene's `RefCell` may already be held by an
    /// `ItemChange` observer — so the key has to be here rather than looked up
    /// in the model. Written from `build`, which is also where the child order
    /// is decided from the same values, so the list the arena paints and the
    /// keys this compares are one snapshot.
    child_keys: Rc<RefCell<HashMap<WidgetId, PaintKey>>>,
    /// One paint node per [`SceneLayer::Interleaved`](crate::SceneLayer) item,
    /// keyed by item — the lightweight twin of `materialized`.
    interleaved_nodes: HashMap<ItemId, WidgetId>,
    /// The wet surface, if one is installed. Its node is always the last child
    /// of this view. See [`WetLayer`](paint_node::WetLayer).
    wet_layer: Option<paint_node::WetLayer>,
    /// The wet surface's node, minted in `build`.
    wet_node: Option<WidgetId>,
    /// This view's mount of [`wet_layer`](Self::wet_layer): the node above plus
    /// the window its tree belongs to.
    ///
    /// Held **strongly** here and weakly by the layer, which is what makes one
    /// layer in several views work without a `Drop` impl on this type: a
    /// destroyed view drops its mount and the layer's entry for it expires.
    wet_mount: Option<Rc<paint_node::WetMount>>,
    /// RAII guard for the cache-invalidation observer wired in
    /// `build()`. Held by `Self` so the observer's lifetime tracks
    /// the SceneView's; dropping it on a fresh `build()` un-installs
    /// the previous observer before re-installing.
    _item_cache_observer: RefCell<Option<teksilo_core::signal::ObserverHandle>>,
    /// RAII guard for the logical-AT-structure observer wired in `build()`.
    /// Held by `Self` so a re-build un-installs the previous observer before
    /// re-installing. Drives a reconcile pass on `Scene::a11y_change_signal`
    /// (group / parent / relation / live / landmark / category mutations),
    /// which don't flow through `item_change_signal`.
    _a11y_observer: RefCell<Option<teksilo_core::signal::ObserverHandle>>,
    /// [`Scene::mutation_version`] as of the end of the build that last
    /// requested an AccessKit re-walk. `None` until the first build. `build()`
    /// re-walks AT only when the version has advanced past this since the last
    /// walk (a structural / geometry / a11y mutation), so a `build()` driven
    /// purely by per-frame dynamic-bounds churn does not re-walk AT 60×/s.
    last_at_version: Option<u64>,
    /// Whether [`Scene::refresh_dynamic_bounds`] reported a change on the
    /// *previous* build. The `true → false` edge (an animation settling) walks
    /// the final animated bounds into AT once — the one AT update the
    /// version-delta gate would otherwise miss while suppressing the churn.
    dynamic_churning: bool,

    // --- The above-a-card veto ----------------------------------------
    /// Whether the scene currently holds at least one hit-testable entry that
    /// **claims the press** ([`crate::pick::claims_press`]) and could be
    /// painted above a card — the [`Over`](crate::SceneLayer::Over) band, which
    /// is above every card, or the
    /// [`Interleaved`](crate::SceneLayer::Interleaved) band, which shares the
    /// cards' rank and is above the ones with a lower `z`. Refreshed while the
    /// handler snapshot is rebuilt.
    ///
    /// An `Under` item can never outrank a card, so this is the only reason
    /// this view would ever reject one of its own children — which is why every
    /// scene whose foreground is decorative pays one `bool` read per
    /// hit-tested child and nothing else.
    over_claimants: Rc<Cell<bool>>,
    /// Memo for [`topmost_press_claimant`], valid for one hit walk:
    /// `(snapshot generation, view transform, scene point) -> answer`.
    ///
    /// The arena asks [`accepts_child_hit`](SceneView::accepts_child_hit) once
    /// per hit-tested child, so without this a 200-card scene would rescan the
    /// snapshot 200 times per pointer sample. **The floor is deliberately not
    /// part of the key.** Every child asks about a different floor — its own
    /// [`PaintKey`] — so a memo keyed by it would miss on every single child
    /// and cost exactly the rescans it exists to prevent. What the scan answers
    /// is floor-free ("what is the topmost press claimant at this point?"), and
    /// the per-child floor is applied to that answer by one `PaintKey` compare
    /// outside the memo. See [`press_claimant_above`] for why the two forms are
    /// equivalent.
    ///
    /// The **whole** view transform is part of the key, not just the projected
    /// point. A screen-anchored (`IGNORES_TRANSFORMATIONS`) claimant is tested
    /// against its own projected anchor, which is a different point from the one
    /// being asked about, so two transforms agreeing at the query point can
    /// still disagree about the claimant. Six `f32` compares are cheaper than
    /// that being true only by luck.
    veto_memo: Rc<Cell<Option<(u64, [f32; 6], Point, Option<PaintKey>)>>>,
    /// How many times [`topmost_press_claimant`] has actually walked the
    /// handler snapshot, over this view's whole life.
    ///
    /// A memo is only worth what it hits, and "it hits" is not something
    /// reading the code can establish — the previous version of this memo was
    /// keyed by a value that differed on every call and therefore never hit at
    /// all, while looking exactly like a working memo. So the number is
    /// published rather than inferred, and
    /// `tests/veto_scaling_probe.rs` gates it: one hit test over *n*
    /// overlapping cards must scan once, not *n* times.
    veto_scans: Rc<Cell<u64>>,
    /// Bumped every time [`SceneView::child_paint_key`] is asked, which is the
    /// *other* half of the veto's cost and was invisible for exactly as long as
    /// the scan count was the only number published.
    ///
    /// The scan count cannot see it: the two halves fail independently, so
    /// breaking either alone leaves `veto_scaling_probe` green. This one gates
    /// the order of the two questions — the claimant is resolved first and the
    /// child's key looked up only against an actual answer, so a scene with no
    /// press claimant at all pays for **no** probes however many children the
    /// arena asks about. Evaluating the key as an argument instead reverses
    /// that, and nothing behavioural changes.
    key_probes: Rc<Cell<u64>>,
    /// Bumped every time either hit snapshot is written — rebuilt whole or
    /// patched per item. The first field of [`SceneView::veto_memo`]'s key, and
    /// therefore what retires that memo; a pass that leaves the snapshots alone
    /// (every pan sample) leaves it alone too.
    snapshot_generation: Rc<Cell<u64>>,
    /// What has happened to the model since the two hit snapshots above were
    /// last made current — the record that lets a layout pass patch them per
    /// item, or skip them entirely, instead of rebuilding from `scene.ids()`.
    ///
    /// Written by this view's `item_change_signal` observer (installed in
    /// `build`), read by the layout pass. See
    /// [`hit_snapshot`] for why that is sound.
    hit_sync: Rc<RefCell<hit_snapshot::HitSnapshotSync>>,
    /// The paint-order floor the arena's verdict established for the press
    /// currently in flight — [`RANK_OVER`] when a heavyweight card was on top
    /// at the press point (or an `Over` claimant vetoed one), [`RANK_UNDER`](crate::pick::RANK_UNDER)
    /// otherwise. Written by the `PointerDown` arm, read by the drag.
    ///
    /// The drag needs it and cannot derive it. `on_pointer_event` runs with the
    /// router's `dispatch_target` in hand; a recognized gesture does not always
    /// (two of its dispatch paths build a context with no target at all), and
    /// the press that armed the drag is anyway the event whose verdict the grab
    /// should follow. Recording it is what makes the drag and the tap answer
    /// from **one** paint order rather than two — and it is the reason a press
    /// on a card no longer grabs a connector the card was covering.
    ///
    /// One slot, and one press is what a `SceneView` arbitrates: the node-level
    /// `MultiContact` default is `First`, which refuses a second contact's
    /// gesture arena while the first is live, so only one press can be driving
    /// a drag here. Two pointers of different kinds can still *deliver* presses
    /// to this handler (a mouse and a pen), and the later one would overwrite
    /// the slot — which costs at most one drag starting with the floor the other
    /// press recorded, on a view that can only be dragging for one of them.
    /// Reset by the release and by the cancel arm.
    press_floor: Rc<Cell<PaintKey>>,

    // --- Magnetism -----------------------------------------------------
    /// Per-view magnetism config (predicate, on_connect, feedback, …).
    /// `None` = magnetism off for this view: no snap, no feedback, no
    /// magnet AT nodes. Shared `Rc` so the drag / key closures hold a clone.
    magnetism: Option<Rc<MagnetismConfig>>,
    /// In-flight port-drag (grabbed a magnet handle, dragging a wire).
    /// `RefCell` (not `Cell`) because `PortDragState` carries a non-`Copy`
    /// `Rc` payload.
    port_drag: Rc<RefCell<Option<magnetism::PortDragState>>>,
    /// Active item-drag snap (the dragged item's magnet aligned onto a
    /// target). Drives feedback and the connection fired on release.
    item_snap: Rc<RefCell<Option<MagnetSnap>>>,
    /// Whether the keyboard connect mode is active (entered via the
    /// config's connect key while the view is focused).
    magnet_connect_mode: Rc<Cell<bool>>,
    /// The keyboard-focused magnet in connect mode (virtual focus: the
    /// SceneView keeps real arena focus and points `active_descendant`
    /// at this magnet's synthetic AT node).
    magnet_focus: Rc<Cell<Option<MagnetId>>>,
    /// The keyboard-activated source magnet awaiting a target.
    magnet_pending: Rc<Cell<Option<MagnetId>>>,

    // --- Selection transform controller --------------------------------
    /// Per-view transform-controller config (handles, constraints, hooks).
    /// `None` = no controller: no frame, no handles, no group move, no
    /// accessibility nodes, and every pointer rule is exactly what it was.
    transform: Option<Rc<crate::transform_session::TransformConfig>>,
    /// The controller's live state — the in-flight session, the keyboard mode,
    /// the two tick signals and the published session signal. Shared `Rc` so
    /// the drag / key / paint closures hold a clone.
    transform_rt: Rc<crate::transform_session::TransformRuntime>,
}

impl std::fmt::Debug for SceneView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Manual impl: `focus_order_callback` is `Rc<dyn Fn>` and
        // therefore not `Debug`. Render it as a presence flag instead.
        f.debug_struct("SceneView")
            .field("model", &self.model)
            .field("materialized_count", &self.materialized.len())
            .field("default_size", &self.default_size)
            .field("interactive", &self.interactive)
            .field("zoom_range_override", &self.zoom_range_override.get())
            .field("pan_bounds_override", &self.pan_bounds_override.get())
            .field("a11y_mode", &self.a11y_mode)
            .field("a11y_off_screen_mode", &self.a11y_off_screen_mode)
            .field("selection_mode", &self.selection.mode())
            .field("focus_order_callback", &self.focus_order_callback.is_some())
            .field("a11y_nested", &self.a11y_nested)
            .field("a11y_label", &self.a11y_label)
            .field("transform_controller", &self.transform.is_some())
            .field("debug_overlay", &self.debug_overlay)
            .finish_non_exhaustive()
    }
}

// --------------------------------------------------------------------------
// The `Over`-band veto — one order, two pickers, one answer
// --------------------------------------------------------------------------

/// Every piece of in-flight pointer-interaction state a revoked contact must
/// give back, in one place.
///
/// A [`PointerCancel`](teksilo_core::event::WidgetEvent::PointerCancel) is
/// terminal — no `PointerUp` follows and nothing may activate — and it reaches
/// a `SceneView` twice: once as `DragPhase::Cancelled` (the recognizer's own
/// unwind, emitted only for a raw pointer cancel, since arbitration loss resets
/// an arena silently) and once as the node's `on_pointer_cancel`. Both hand
/// over to this, so there is one list of what a cancel drops rather than two
/// lookalike ones that can drift.
///
/// Clearing twice is a no-op, which is what makes the double delivery safe.
#[derive(Clone)]
pub(super) struct DragUnwind {
    pub(super) drag_target: Rc<Cell<Option<DragTarget>>>,
    pub(super) pending_item_move: Rc<Cell<Option<(ItemId, Vec2)>>>,
    pub(super) marquee: Rc<Cell<Option<MarqueeState>>>,
    pub(super) pending_marquee_commit: Rc<RefCell<Option<(SceneRegion, ItemSelectionMode, bool)>>>,
    pub(super) port_drag: Rc<RefCell<Option<magnetism::PortDragState>>>,
    pub(super) item_snap: Rc<RefCell<Option<MagnetSnap>>>,
    /// The transform controller, when one is installed. A revoked contact drops
    /// its live session the same way it drops a drag target, and for the same
    /// reason: the preview is re-derived from it on every paint and every
    /// relayout, so a session left standing would draw the selection at the
    /// transformed position for ever while the model went on saying nothing had
    /// happened.
    ///
    /// The whole **driver** and not just its
    /// [`TransformRuntime`](crate::transform_session::TransformRuntime),
    /// because dropping the session is only a third of what ending a gesture
    /// means. `TransformRuntime::abort` did that third; the edge auto-pan went
    /// on tweening (for up to thirty seconds, with no input and with
    /// `edge_panning` still `true`), and `on_end` — documented as "fired once
    /// when a gesture finishes, committed **or cancelled**" — never fired, so
    /// an app that opens an overlay on `on_start` leaked it every time the OS
    /// revoked the contact. One cancel path, reached from here too.
    transform: Option<super::view::transform::TransformDriver>,
}

impl DragUnwind {
    /// Drop everything, reporting whether anything was actually there.
    ///
    /// `drag_target` is the one that used to survive a cancel and matters most:
    /// `paint` translates the grabbed item — and its whole drag group — by
    /// `current - anchor` on every frame for as long as it is set, so leaving it
    /// behind drew the item at the dragged position *permanently*, while the
    /// model went on saying it had never moved. `pending_item_move` is the
    /// commit that would otherwise have landed on the next rebuild; a cancel is
    /// terminal, so no `Ended` can have queued it and it belongs to the gesture
    /// that was just taken away.
    pub(super) fn clear(&self, ctx: &mut teksilo_core::widget::EventContext) -> bool {
        // Bitwise OR, not `||`: every one of these has to run.
        self.drag_target.take().is_some()
            | self.pending_item_move.take().is_some()
            | self.marquee.take().is_some()
            | self.pending_marquee_commit.replace(None).is_some()
            | self.port_drag.replace(None).is_some()
            | self.item_snap.replace(None).is_some()
            | self
                .transform
                .as_ref()
                .is_some_and(|driver| driver.cancel(ctx))
    }
}

/// Everything the **hover episode** opened, and the one place it is closed.
///
/// The view raises three things while a hovering pointer is inside it, and each
/// one outlives the dispatch that raised it: an item's `on_hover(true)`, the
/// cursor (a `set_cursor` override, which by contract stands until the same
/// handler takes it back), and the item tooltip — on either of its two paths,
/// armed-and-counting-down or already shown. Every one of them is re-decided
/// only by another hovering move *inside* the view, so a pointer that goes away
/// takes the deciding move with it.
///
/// It is shared rather than written twice because two different events end an
/// episode and they must not drift: the pointer leaving (`on_hover(false)` on
/// the view's own node) and the pointer being revoked (`on_pointer_cancel`).
/// The cancel arm unwinds this **and** [`DragUnwind`]; a departure deliberately
/// does not, which is the one place the two legitimately differ — see the call
/// sites.
pub(super) struct HoverUnwind {
    // Private to `view`, which is enough: the only consumer is
    // `view::gestures_impl`, a child module, and `HandlerSnapshotEntry` is
    // itself module-private — re-exporting the field at `pub(super)` would
    // leak a name the rest of the crate cannot spell.
    hovered_item: Rc<Cell<Option<crate::item::ItemId>>>,
    handler_snapshot: Rc<RefCell<Vec<HandlerSnapshotEntry>>>,
    cursor_pos: Rc<Cell<Option<Point>>>,
    /// The view's tooltip body, the key both retraction calls are made on.
    tooltip_content_id: WidgetId,
}

impl HoverUnwind {
    /// Close the episode: unhover the item, retract the tooltip on both paths,
    /// put the cursor down, forget where the pointer was.
    ///
    /// The two tooltip calls are both unconditional and neither substitutes for
    /// the other: `cancel_delayed_overlay` drops a show that is still queued on
    /// the tree (which would otherwise surface *after* the pointer had gone,
    /// anchored to a view it is no longer over), and `dismiss_overlay_by_content`
    /// takes down one already on the stack. A hover tip and a held tip each
    /// reach exactly one of them.
    ///
    /// The cursor is set to [`CursorIcon::Default`](teksilo_core::widget::CursorIcon::Default)
    /// rather than released. A
    /// release restores what the *current* hover chain declared, and at this
    /// moment that chain is the one being torn down — the destination's own
    /// `PointerEnter` has not run yet. Writing `Default` is the same answer the
    /// framework's own leave arm gives a node that declared a cursor, and the
    /// destination's declaration lands after ours and wins.
    pub(super) fn clear(&self, ctx: &mut teksilo_core::widget::EventContext) {
        if let Some(prev) = self.hovered_item.take()
            && let Some(entry) = self.handler_snapshot.borrow().iter().find(|e| e.id == prev)
            && let Some(h) = entry.handlers.as_deref()
            && let Some(cb) = h.on_hover.as_ref()
        {
            cb(false, ctx);
        }
        ctx.cancel_delayed_overlay(self.tooltip_content_id);
        ctx.dismiss_overlay_by_content(self.tooltip_content_id);
        self.cursor_pos.set(None);
        ctx.set_cursor(teksilo_core::widget::CursorIcon::Default);
    }
}

impl SceneView {
    /// This view's [`DragUnwind`] handles, cloned for a handler closure.
    pub(super) fn drag_unwind(&self) -> DragUnwind {
        DragUnwind {
            drag_target: self.drag_target.clone(),
            pending_item_move: self.pending_item_move.clone(),
            marquee: self.marquee.clone(),
            pending_marquee_commit: self.pending_marquee_commit.clone(),
            port_drag: self.port_drag.clone(),
            item_snap: self.item_snap.clone(),
            transform: self.transform_driver(),
        }
    }

    /// This view's [`HoverUnwind`] handles, cloned for a handler closure.
    pub(super) fn hover_unwind(&self, tooltip_content_id: WidgetId) -> HoverUnwind {
        HoverUnwind {
            hovered_item: self.hovered_item.clone(),
            handler_snapshot: self.handler_snapshot.clone(),
            cursor_pos: self.cursor_pos.clone(),
            tooltip_content_id,
        }
    }
}

/// The state the `Over`-band veto reads, borrowed rather than cloned.
///
/// Both callers hold these already: the `Widget` impl has `&self`, and the drag
/// closure captured its own clones at build time. Passing borrows keeps the
/// arena's per-child call free of refcount traffic.
struct VetoState<'a> {
    over_claimants: &'a Cell<bool>,
    veto_memo: &'a Cell<Option<(u64, [f32; 6], Point, Option<PaintKey>)>>,
    veto_scans: &'a Cell<u64>,
    snapshot_generation: &'a Cell<u64>,
    handler_snapshot: &'a RefCell<Vec<HandlerSnapshotEntry>>,
    view_transform: &'a Signal<Transform2D>,
}

/// The key of the topmost entry **above `floor`** whose shape contains
/// `scene_pt`, if that entry **claims the press** — otherwise `None`.
///
/// This is the whole occlusion rule in one query, and it is deliberately "the
/// topmost entry above the floor, *if* it claims" rather than "the topmost
/// claimant above the floor":
///
/// * it mirrors `hit_handler_item`, which resolves the topmost **entry** and
///   only then looks for handlers — so the veto can never disagree with the
///   dispatch that follows it;
/// * a decorative item painted over an interactive one blocks it, exactly as it
///   does today, instead of being stepped over.
///
/// `floor` is what makes it answerable for
/// [`Interleaved`](crate::SceneLayer::Interleaved). The `Over` band is above
/// *every* card, so a rank sufficed; an interleaved entry is above some cards
/// and below others, and only a whole [`PaintKey`] can say which. The per-child
/// caller ([`SceneView::accepts_child_hit`]) passes that child's own key, so a
/// card painted over an interleaved claimant keeps its press and one painted
/// under it yields — which is the same rule the eye applies.
///
/// # Why the floor is applied here and not inside the scan
///
/// The scan is [`topmost_press_claimant`], which takes no floor, and the floor
/// is one `PaintKey` compare against its answer. The two forms are the same
/// answer because the snapshot is sorted by key descending: the first entry the
/// scan meets is the topmost containing one *overall*, so
///
/// * if that entry's key is at or above `floor`, it is also the first one a
///   floored scan would have met — same entry, same verdict;
/// * if it is below `floor`, then every containing entry is below `floor`
///   (they are all below *it*), and a floored scan finds nothing — which is the
///   `None` the compare produces.
///
/// The slop pass cannot break the equivalence either: it is miss-only and this
/// call site hands it the mouse profile, whose slop radius is zero, so it never
/// reports anything (see the `GrabSlop` construction in
/// [`topmost_press_claimant`]).
///
/// This split is not a micro-optimisation. The floor **differs on every call** —
/// it is the asking child's own key — so folding it into the memo key makes the
/// memo miss once per child, which is exactly the rescan the memo exists to
/// prevent. Keeping the scan floor-free is what makes one hit test cost one
/// scan at any card count.
fn press_claimant_above(
    state: VetoState<'_>,
    floor: PaintKey,
    scene_pt: Point,
) -> Option<PaintKey> {
    topmost_press_claimant(state, scene_pt).filter(|top| *top >= floor)
}

/// The key of the topmost entry whose shape contains `scene_pt`, if that entry
/// **claims the press** — otherwise `None`. Memoised for one hit walk.
///
/// Floor-free by design: see [`press_claimant_above`], which applies a caller's
/// floor to this answer.
///
/// Reads only the per-layout `handler_snapshot` — never the
/// [`SceneModel`]. A hit test runs inside the router, where
/// an `ItemChange` observer may already hold the model's `RefCell`, and
/// re-entering it there is a panic.
fn topmost_press_claimant(state: VetoState<'_>, scene_pt: Point) -> Option<PaintKey> {
    if !state.over_claimants.get() {
        return None;
    }
    let view_xform = state.view_transform.get();
    // The second space. A screen-anchored (`IGNORES_TRANSFORMATIONS`) entry —
    // the flag whose own doc names "annotation pins … fixed-pixel-size badges
    // over moving content", i.e. the most likely real `Over` claimant after a
    // halo — hit-tests where it is *drawn*, not where its scene anchor is. The
    // view owns the transform, so both spaces are recoverable from the one
    // point the arena hands over.
    let screen_pt = view_xform.apply_point(scene_pt);
    let generation = state.snapshot_generation.get();
    if let Some((memo_gen, memo_xform, memo_pt, answer)) = state.veto_memo.get()
        && memo_gen == generation
        && memo_xform == view_xform.m
        && memo_pt == scene_pt
    {
        return answer;
    }
    state.veto_scans.set(state.veto_scans.get().wrapping_add(1));
    let snapshot = state.handler_snapshot.borrow();
    let answer = hit_handler_index(
        &snapshot,
        screen_pt,
        scene_pt,
        view_xform,
        // The veto is a question about what is painted where, not about what a
        // finger can *reach*: widening it would let an `Over` claimant punch a
        // hole in a card several device pixels away from anything the user can
        // see. The slop pass is unreachable from here anyway — the view's own
        // bounds always accept, so a hit test inside it is never the miss that
        // pass exists to re-attribute.
        GrabSlop::new(
            teksilo_tokens::InputTokens::default(),
            teksilo_tokens::PointerKind::Mouse,
        ),
        // "Admit everything": the answer is floor-free, and a caller's floor is
        // applied to it afterwards.
        PaintKey::bottom(),
    )
    .filter(|index| snapshot[*index].claims_press)
    .map(|index| snapshot[index].key);
    drop(snapshot);
    state
        .veto_memo
        .set(Some((generation, view_xform.m, scene_pt, answer)));
    answer
}

impl SceneView {
    /// A child's paint key, as of the last build.
    ///
    /// [`PaintKey::rank_floor(RANK_WIDGET)`](PaintKey::rank_floor) — the bottom
    /// of the card band — for a child this view did not place, and for the
    /// window between a child being added and the next build publishing its
    /// key. That is the permissive answer, and the same one every card got
    /// before keys were compared per child.
    pub(super) fn child_paint_key(&self, child: WidgetId) -> PaintKey {
        self.key_probes.set(self.key_probes.get().wrapping_add(1));
        self.child_keys
            .borrow()
            .get(&child)
            .copied()
            .unwrap_or_else(|| PaintKey::rank_floor(crate::pick::RANK_WIDGET))
    }

    /// [`topmost_press_claimant`] against this view's own state — the
    /// floor-free form, for a caller that has a floor it would rather not
    /// compute unless there is an answer to compare it with.
    ///
    /// [`accepts_child_hit`](SceneView::accepts_child_hit) is that caller: its
    /// floor is a `HashMap` lookup per child, and in a scene with no press
    /// claimant (or with the pointer nowhere near one) there is nothing for it
    /// to be compared against.
    pub(super) fn topmost_press_claimant(&self, scene_pt: Point) -> Option<PaintKey> {
        topmost_press_claimant(self.veto_state(), scene_pt)
    }

    fn veto_state(&self) -> VetoState<'_> {
        VetoState {
            over_claimants: &self.over_claimants,
            veto_memo: &self.veto_memo,
            veto_scans: &self.veto_scans,
            snapshot_generation: &self.snapshot_generation,
            handler_snapshot: &self.handler_snapshot,
            view_transform: &self.view_transform_signal,
        }
    }

    /// How many times this view has walked its handler snapshot to answer the
    /// above-a-card veto, since it was created.
    ///
    /// The veto's memo is asked once per hit-tested child and answers from one
    /// scan; this is the number that says so. A hit test over *n* overlapping
    /// cards must advance it by **one**, not by *n* — the regression the
    /// `veto_scaling_probe` test gates, and one that is invisible to every
    /// other observable the crate publishes (the verdicts are identical either
    /// way; only the cost moves).
    #[doc(hidden)]
    pub fn veto_snapshot_scans(&self) -> u64 {
        self.veto_scans.get()
    }

    /// How many times the veto has looked a child's [`PaintKey`] up.
    ///
    /// The companion to [`veto_snapshot_scans`](Self::veto_snapshot_scans), and
    /// necessary because the two halves of the veto's cost fail independently:
    /// a memo keyed on something that differs per child makes the *scan* count
    /// follow the card count, while evaluating the child's key before the
    /// claimant query can decline it makes *this* one follow it. Each is
    /// invisible to the other's test, and neither changes a verdict.
    ///
    /// A scene with no press claimant under the pointer must leave this at
    /// zero however many children the arena asks about.
    #[doc(hidden)]
    pub fn veto_key_probes(&self) -> u64 {
        self.key_probes.get()
    }

    /// The items a pointer drag of `grabbed` carries.
    ///
    /// A drag of an item that is part of a multi-selection moves the **whole
    /// selection**, the same contract `Alt+Arrow` keeps — a corkboard user who
    /// rubber-bands five cards and then drags one expects five to move, and the
    /// keyboard alternative has to agree with the pointer or WCAG 2.5.7's
    /// "dragging movements" equivalence is a fiction.
    ///
    /// A selected item that is a **descendant** of another selected item is
    /// dropped: its `local_pos` is relative to that ancestor, so moving both
    /// would translate it twice.
    ///
    /// Derived rather than snapshotted at grab time, so the live paint feedback
    /// and the commit cannot disagree about which items are moving.
    pub(super) fn drag_group(&self, grabbed: ItemId) -> Vec<ItemId> {
        drag_group_of(&self.model, &self.selection, grabbed)
    }

    /// The paint-order floor for a dispatch that is running on this view.
    ///
    /// A [`RANK_OVER`] rank floor when the arena gave this pointer to one of
    /// our heavyweight children (we are previewing, not targeted) — only the
    /// `Over` band outranks **every** card, so everything below it must stand
    /// aside and let the card have the event.
    /// [`PaintKey::bottom`] when we are the target, which is exactly the case
    /// where no card won: the arena has already applied `hit_shape`,
    /// `hit_transparent`, `event_pass_through` **and**
    /// [`accepts_child_hit`](SceneView::accepts_child_hit) — so an
    /// [`Interleaved`](crate::SceneLayer::Interleaved) claimant that vetoed the
    /// cards beneath it has already produced this branch, and reading the
    /// verdict here is what stops the two pickers re-deriving it differently.
    ///
    /// The coarse `RANK_OVER` floor in the first branch is deliberate and
    /// slightly conservative: an interleaved entry that is above the winning
    /// card but does **not** claim the press is treated, for hover and cursor,
    /// the way an `Under` entry under that card is. It could not have won the
    /// press (it does not claim), and narrowing the floor to the winning card's
    /// own key would mean resolving *which* card won from a `dispatch_target`
    /// that may be one of its descendants.
    ///
    /// A context with no verdict at all (a gesture timer, a hand-built
    /// `EventContext` in a test) is treated as "we are the target", which is
    /// the permissive, pre-existing behaviour.
    fn dispatch_floor(ctx: &teksilo_core::widget::EventContext, self_id: WidgetId) -> PaintKey {
        match ctx.dispatch_target() {
            Some(target) if target != self_id => PaintKey::rank_floor(RANK_OVER),
            _ => PaintKey::bottom(),
        }
    }
}

mod a11y_impl;
mod build_impl;
mod builder_impl;
mod camera_impl;
mod gestures_impl;
mod hit_snapshot;
mod layout_impl;
mod magnetism;
mod paint_impl;
pub(crate) mod paint_node;
mod transform;
mod widget_trait;

/// Union an iterator of axis-aligned rectangles into a single
/// bounding rectangle. Returns `None` if the iterator is empty.
fn union_rects(mut rects: impl Iterator<Item = Rect>) -> Option<Rect> {
    let first = rects.next()?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.right();
    let mut max_y = first.bottom();
    for r in rects {
        if r.x < min_x {
            min_x = r.x;
        }
        if r.y < min_y {
            min_y = r.y;
        }
        if r.right() > max_x {
            max_x = r.right();
        }
        if r.bottom() > max_y {
            max_y = r.bottom();
        }
    }
    Some(Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
}

#[cfg(test)]
mod tests;
