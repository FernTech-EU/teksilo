// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The **geometry constraint**: one closure that rewrites a gesture's proposed
//! geometry *before* anything is applied.
//!
//! Snap-to-grid, axis lock, page-bounds clamping and "this item may not leave
//! its lane" are all one hook. It is Qt's
//! `QGraphicsItem::itemChange(ItemPositionChange, value) -> value` in the shape
//! Rust's borrow rules allow: the scene is lent **read-only** and the decision
//! is *returned* rather than written.
//!
//! # The quantity is a frame, not a pointer
//!
//! A constraint is offered a [`TransformFrame`] — the scene-space box the
//! gesture's items occupy — and hands one back. That choice is the whole reason
//! the obvious snap-to-grid example is actually correct:
//!
//! * A **pointer** position carries the grab offset. Snapping the cursor to a
//!   25 dp grid leaves an item grabbed 37 dp from its corner sitting 37 dp
//!   off-grid, on every drag, for ever.
//! * A **`local_pos`** is stated in the item's *parent's* frame, so a
//!   constraint written against a scene grid silently means something else for
//!   a parented item.
//! * A **frame** is the box the user can see, in scene coordinates, for one
//!   item or for a whole selection. Snapping `proposed.rect.x` snaps the thing
//!   the user is looking at.
//!
//! # It runs on every route, and it cannot disagree with itself
//!
//! There is deliberately **no phase parameter**. The constraint is a pure
//! function of the proposal, re-run from scratch on every sample including the
//! release one, so the frame the last preview drew *is* the frame the commit
//! writes — by construction, not by convention. A hook that could snap loosely
//! while dragging and hard on release would be a hook that guarantees a jump at
//! the release, which is the one failure this whole design exists to remove.
//!
//! Four routes consult it, and they all consult the same closure:
//!
//! | route | who runs it |
//! | --- | --- |
//! | the selection transform controller (move / resize / rotate; pointer, keyboard and AT) | [`SceneView`](crate::SceneView) |
//! | the lightweight item drag | [`SceneView`](crate::SceneView) |
//! | the `Alt`+arrow keyboard nudge | [`SceneView`](crate::SceneView) |
//! | an app's own drag — a heavyweight card's `on_drag` handler | the app, via [`SceneModel::constrain_move`](crate::SceneModel::constrain_move) |
//!
//! The fourth is why the constraint lives on the **model** rather than on a
//! view: a card's widget is built by a per-view delegate and holds a
//! [`SceneModel`](crate::SceneModel) clone, never a `&SceneView`, so a
//! view-installed closure would be unreachable from the one tier the motivating
//! use cases (a corkboard of cards on a grid) actually live in. Geometry policy
//! is a property of the *document* — "this corkboard is on a 25 dp grid" — not
//! of a pane looking at it, and two panes onto one scene that snapped
//! differently would be a bug rather than a feature. Contrast
//! [`SceneView::focus_order`](crate::SceneView::focus_order),
//! [`SceneView::drag_mode`](crate::SceneView::drag_mode) and
//! [`SceneView::magnetism`](crate::SceneView::magnetism), which are per-view
//! because they say what *this pane* lets you do.
//!
//! # A programmatic write is never constrained
//!
//! [`SceneModel::set_local_pos`](crate::SceneModel::set_local_pos) and every
//! other mutator land **exactly** where they say. Snapping a document load, a
//! [`SceneListAdapter`](crate::SceneListAdapter) rebuild or a data-layer replay
//! is Qt's own best-known footgun with `itemChange`, and it is avoided here not
//! by where the closure is stored but by *who consults it*: the gesture paths
//! do, the mutators do not.
//!
//! # What it costs
//!
//! A scene with **no** constraint pays one `Option` test per gesture sample and
//! never builds a call. Measured on a 20 000-item scene, a pointer sample costs
//! the same with the mechanism present and absent — the difference is below the
//! run-to-run noise of the measurement.
//!
//! A scene **with** one pays for the closure, and the closure is a *pure
//! function re-run per query* rather than a per-gesture callback: one sample
//! that also lays out, paints and re-walks the accessibility tree asks it
//! several times (six, at the time of writing). That count is bounded and
//! independent of both the scene's size and the selection's — pinned by
//! `the_constraint_is_asked_a_bounded_number_of_times_per_sample` — but it is
//! not one. Keep the closure cheap; a constraint that must run an expensive
//! spatial query should memoise on `proposed` inside its own capture.
//!
//! Re-running rather than caching is deliberate. The answer has to be the same
//! for the chrome, the preview, the commit and the announcement, and the
//! cheapest way to guarantee that is for there to be nothing to invalidate.
//!
//! # Testing a policy
//!
//! A [`ProposedChange`] is built by the framework and is deliberately not
//! consumer-constructible. To exercise a policy closure without a widget tree,
//! drive it through the public door the app-owned drag uses — a bare
//! [`SceneModel`](crate::SceneModel) is enough:
//!
//! ```
//! # use teksilo_canvas::{Point, Rect, Vec2};
//! # use teksilo_scene::{ChangeVerdict, RectItem, SceneModel, TransformSource};
//! let model = SceneModel::new();
//! let a = model.add_item(RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)), Point::ZERO);
//! model.set_geometry_constraint(|c| {
//!     let t = c.translation();
//!     ChangeVerdict::Adjust(c.translated(Vec2::new(t.x, 0.0)))   // horizontal only
//! });
//!
//! let start = model.transform_frame(&[a]).expect("resolves");
//! let applied =
//!     model.constrain_move(&[a], start, Vec2::new(30.0, 12.0), TransformSource::Pointer);
//! assert_eq!(applied, Vec2::new(30.0, 0.0));
//! ```
//!
//! # What it may touch
//!
//! It may **read** the scene through the `&Scene` it is handed — a shared
//! borrow is already open, and shared plus shared is legal. It may **not**
//! write: a write needs the cell exclusively. That is enforced rather than
//! documented — [`SceneModel::write_guard`](crate::SceneModel::write_guard)
//! checks the constraint flag and panics naming this hook and what to do
//! instead (return the verdict), so the failure reads as the bug it is rather
//! than as `RefCell already borrowed`.
//!
//! # Do not capture a `SceneModel`
//!
//! [`ProposedChange::scene`] is the *whole* read surface — every query
//! [`SceneModel`](crate::SceneModel) offers is a shared borrow delegating to
//! the same [`Scene`], so a captured handle buys a constraint nothing it is
//! allowed to do.
//!
//! It costs something, though. The scene **owns** the closure
//! (`Rc<RefCell<Scene>>` → `Scene::geometry_constraint` → the closure), so a
//! closure holding a `SceneModel` clone closes that ring and nothing in it is
//! ever dropped: every item, every heavyweight payload, the journal and the
//! spatial index stay alive for the life of the process. It is the ordinary
//! `Rc` cycle, and it is invisible — the scene keeps working perfectly.
//!
//! Two safe forms, in order of preference:
//!
//! ```
//! # use teksilo_canvas::{Point, Rect};
//! # use teksilo_scene::{ChangeVerdict, RectItem, SceneModel};
//! # let model = SceneModel::new();
//! # let lane = model.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
//! // 1. Read through the scene you are handed. Nothing captured, nothing to leak.
//! model.set_geometry_constraint(move |c| match c.scene.scene_rect(lane) {
//!     Some(r) if r.contains(Point::new(c.proposed.rect.x, c.proposed.rect.y)) => {
//!         ChangeVerdict::Accept
//!     }
//!     _ => ChangeVerdict::Reject,
//! });
//!
//! // 2. When a policy object genuinely holds a model for its *other* work,
//! //    capture the weak handle and upgrade inside the call.
//! let weak = model.downgrade();
//! model.set_geometry_constraint(move |c| match weak.upgrade() {
//!     Some(m) if m.len() > 1 => ChangeVerdict::Accept,
//!     _ => ChangeVerdict::Adjust(c.start),
//! });
//! ```
//!
//! `a_constraint_capturing_a_strong_handle_leaks_the_scene` (in
//! `tests/constraint_lifetime.rs`, beside
//! `a_constraint_capturing_a_weak_handle_lets_the_scene_drop`) pins both halves
//! with a `Drop` sentinel.
//!
//! # What it may hand back
//!
//! An applicable frame: every field finite, neither extent negative. That is
//! not a formality. A one-character slip in the snap closure above —
//! `let grid = 0.0;` — makes `(x / grid).round() * grid` a `NaN`, and a `NaN`
//! frame applied verbatim gives the item a `NaN` position, an inverted
//! infinite AABB, and a permanent absence from hit-testing, from the marquee
//! and from every spatial query, with no panic to say so. So an
//! [`ChangeVerdict::Adjust`] whose frame cannot be applied is **refused**: the
//! sample behaves as [`ChangeVerdict::Reject`], and in a debug build it panics
//! naming the offending frame rather than losing the item quietly.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Transform2D, Vec2};

use crate::item::ItemId;
use crate::scene::Scene;
use crate::transform_session::{TransformFrame, TransformOp, TransformSource};

/// A geometry constraint: the closure installed by
/// [`SceneModel::set_geometry_constraint`](crate::SceneModel::set_geometry_constraint).
pub(crate) type GeometryConstraint = Rc<dyn Fn(&ProposedChange<'_>) -> ChangeVerdict>;

/// A change a gesture is about to make, offered to the geometry constraint.
///
/// Every field is a *question*, never a channel: the answer is the returned
/// [`ChangeVerdict`].
#[derive(Debug)]
#[non_exhaustive]
pub struct ProposedChange<'a> {
    /// The scene, read-only. Query siblings, guides, parents, magnets, flags —
    /// anything a constraint needs to decide.
    ///
    /// This is the whole read surface, and the reason a constraint never needs
    /// to capture a [`SceneModel`](crate::SceneModel): one that does closes a
    /// reference cycle through the closure the scene owns and leaks the scene.
    /// A closure that must reach the model holds a
    /// [`WeakSceneModel`](crate::WeakSceneModel). Writes panic; see the module
    /// header.
    pub scene: &'a Scene,
    /// The **roots** this gesture will change, pruned of any item whose
    /// ancestor is also in the set — a descendant moves because its root does,
    /// so it is deliberately not listed.
    ///
    /// The transform controller narrows this further to the roots carrying the
    /// operation's own flag
    /// ([`TransformOp::required_flag`](crate::TransformOp::required_flag)); the
    /// lightweight item drag lists the whole drag group, which is what it
    /// moves.
    ///
    /// There is no "primary": the subject of the decision is the frame, which
    /// is what makes the constraint grab-offset invariant. Per-item policy is
    /// still expressible — a one-item gesture is `items.len() == 1`.
    pub items: &'a [ItemId],
    /// What the gesture asks of them.
    pub op: TransformOp,
    /// The frame the gesture started from. Fixed for the gesture's life, so a
    /// constraint measures total travel rather than per-sample travel.
    pub start: TransformFrame,
    /// The frame about to be applied, **after** magnetism.
    ///
    /// Both frames' `rect`s are stated in the frame's **own** basis. For a
    /// single rotated item that is the item's basis, so a grid snap written
    /// against `rect.x` snaps along the item's axes; a multi-item frame is
    /// axis-aligned, so the same rule snaps along the scene's.
    pub proposed: TransformFrame,
    /// Which route produced it.
    pub source: TransformSource,
    /// Whether magnetism moved [`proposed`](Self::proposed) onto a magnet.
    ///
    /// The seam where the two snapping systems compose: returning
    /// [`ChangeVerdict::Accept`] when this is `true` lets an explicit magnet
    /// win over a standing rule. Overriding it is also allowed — and then the
    /// magnet's connection does **not** fire, because a magnet whose alignment
    /// was overruled did not connect anything.
    ///
    /// Only the lightweight item drag runs magnetism today, so this is `false`
    /// on the transform-controller, keyboard-nudge and app-driven routes.
    pub magnet_snapped: bool,
}

impl ProposedChange<'_> {
    /// The scene-space translation from [`start`](Self::start) to
    /// [`proposed`](Self::proposed).
    ///
    /// The quantity an axis lock or a grid snap works in. Exact for a
    /// [`TransformOp::Move`]; for a resize or a rotate it is the movement of
    /// the frame's origin, and the extent change is in the two frames
    /// themselves.
    pub fn translation(&self) -> Vec2 {
        let d = Point::new(
            self.proposed.rect.x - self.start.rect.x,
            self.proposed.rect.y - self.start.rect.y,
        );
        let p = Transform2D::rotate(self.start.rotation).apply_point(d);
        Vec2::new(p.x, p.y)
    }

    /// [`start`](Self::start) moved by a scene-space translation — the frame to
    /// hand back from [`ChangeVerdict::Adjust`] once a move has been rewritten.
    ///
    /// The exact inverse of [`translation`](Self::translation), so
    /// `c.translated(c.translation())` is `c.proposed` for a move.
    pub fn translated(&self, translation: Vec2) -> TransformFrame {
        let d = Transform2D::rotate(-self.start.rotation)
            .apply_point(Point::new(translation.x, translation.y));
        TransformFrame {
            rect: Rect::new(
                self.start.rect.x + d.x,
                self.start.rect.y + d.y,
                self.proposed.rect.width,
                self.proposed.rect.height,
            ),
            rotation: self.proposed.rotation,
            count: self.proposed.count,
        }
    }
}

/// What a geometry constraint decides.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ChangeVerdict {
    /// Apply [`ProposedChange::proposed`] unchanged.
    Accept,
    /// Apply this frame instead. The delta is **re-derived** from it, so the
    /// preview, the commit, the chrome and the announcement cannot disagree
    /// about what happened.
    Adjust(TransformFrame),
    /// Apply nothing: this sample leaves the items where the gesture found
    /// them.
    ///
    /// Exactly equivalent to `Adjust(change.start)`, and deliberately so — a
    /// "freeze at the last accepted sample" rejection would need history, and
    /// the next accepted sample would then jump by everything the frozen ones
    /// travelled. Stateless refusal means a rejected gesture shows nothing
    /// happening and resumes the moment the proposal becomes acceptable, and a
    /// gesture that *ends* rejected commits nothing at all — its delta is the
    /// identity, so
    /// [`TransformOutcome::Cancelled`](crate::TransformOutcome::Cancelled) is
    /// what `on_end` reports.
    Reject,
}

/// The constraint plus the scene borrow it reads, bundled for one call.
///
/// Built by the view (or by [`SceneModel`](crate::SceneModel)'s public doors)
/// under a live **shared** borrow and dropped before any write. `None` where no
/// constraint is installed, which is what makes an unconstrained scene pay one
/// `Option` test per sample and not one closure call.
pub(crate) struct ConstraintCall<'a> {
    scene: &'a Scene,
    constraint: GeometryConstraint,
}

impl<'a> ConstraintCall<'a> {
    /// A call, if `scene` carries a constraint.
    ///
    /// # Panics
    ///
    /// When a constraint is already running on this scene. Nothing in the
    /// framework nests one, so the only way to get here is a constraint that
    /// called [`SceneModel::constrain_move`](crate::SceneModel::constrain_move)
    /// or `constrain_frame` on its own model — which would recurse until the
    /// stack ran out. A named panic is a better answer than that.
    pub(crate) fn new(scene: &'a Scene) -> Option<Self> {
        let constraint = scene.geometry_constraint()?;
        assert!(
            !scene.in_geometry_constraint(),
            "a geometry constraint asked this scene to constrain something. \
             The constraint IS the answer — compute the frame you want and \
             return ChangeVerdict::Adjust(frame); calling constrain_move / \
             constrain_frame from inside one would recurse without end."
        );
        Some(Self { constraint, scene })
    }

    /// Run the constraint over one proposed frame and return the frame to
    /// apply.
    ///
    /// An [`ChangeVerdict::Adjust`] frame is **validated** before it is handed
    /// on; see [`frame_is_applicable`] and the module header for why an
    /// unvalidated one loses the item.
    pub(crate) fn frame(
        &self,
        op: TransformOp,
        items: &[ItemId],
        start: &TransformFrame,
        proposed: TransformFrame,
        source: TransformSource,
        magnet_snapped: bool,
    ) -> TransformFrame {
        let change = ProposedChange {
            scene: self.scene,
            items,
            op,
            start: *start,
            proposed,
            source,
            magnet_snapped,
        };
        let _guard = ConstraintGuard::enter(self.scene);
        match (self.constraint)(&change) {
            // `Accept` and `Reject` hand back a frame the framework built, so
            // there is nothing consumer-authored in them to check.
            ChangeVerdict::Accept => proposed,
            ChangeVerdict::Reject => *start,
            ChangeVerdict::Adjust(f) if frame_is_applicable(&f) => f,
            ChangeVerdict::Adjust(f) => refuse_inapplicable_frame(op, &f, start),
        }
    }
}

/// Whether a frame a constraint handed back can be applied at all.
///
/// Finite everywhere, and neither extent negative. Both halves are reachable
/// from ordinary arithmetic in a policy closure — a zero grid step gives `NaN`,
/// an unclamped "resize by the delta" gives a mirrored box — and both are
/// silent once stored: a `NaN` `local_pos` puts the item outside every query
/// (its scene rect becomes `(inf, inf, -inf, -inf)`, which intersects nothing),
/// and a negative extent inverts its AABB.
///
/// A zero extent is *allowed*: the resize path clamps at `min_size` rather than
/// mirroring, so a zero-extent frame is one a policy closure asked for — and
/// unlike a negative one it does not invert the item's AABB.
fn frame_is_applicable(f: &TransformFrame) -> bool {
    f.rect.x.is_finite()
        && f.rect.y.is_finite()
        && f.rect.width.is_finite()
        && f.rect.height.is_finite()
        && f.rotation.is_finite()
        && f.rect.width >= 0.0
        && f.rect.height >= 0.0
}

/// What an inapplicable [`ChangeVerdict::Adjust`] means: nothing happens this
/// sample, and in a debug build the policy hears about it.
///
/// Refusing is the only answer that cannot lose data. Clamping would invent a
/// geometry the app did not ask for and would keep the gesture running against
/// a rule that is broken; applying it removes the item from the document with
/// no panic and no event. Refusing leaves the items where the gesture found
/// them — exactly [`ChangeVerdict::Reject`], which is stateless, so the gesture
/// resumes the moment the closure returns something applicable again.
#[cold]
#[inline(never)]
fn refuse_inapplicable_frame(
    op: TransformOp,
    returned: &TransformFrame,
    start: &TransformFrame,
) -> TransformFrame {
    if cfg!(debug_assertions) {
        panic!(
            "a geometry constraint returned ChangeVerdict::Adjust({returned:?}) for a \
             {op:?}, and that frame cannot be applied: every field must be finite and \
             neither extent may be negative. The usual cause is arithmetic on a zero \
             or unset step — `(x / grid).round() * grid` is NaN when `grid` is 0.0. \
             Applying it would give the item a NaN position and an inverted AABB, \
             which removes it from hit-testing, from the marquee and from every \
             spatial query for good, so this sample is refused instead (as \
             ChangeVerdict::Reject). The proposal the framework offered was \
             applicable: {start:?} was the frame the gesture started from."
        );
    }
    *start
}

/// Constrain a pure **translation** of `items` and read the answer back as a
/// translation.
///
/// The one implementation shared by every move-only route — the lightweight
/// item drag, the `Alt`+arrow nudge and the public
/// [`SceneModel::constrain_move`](crate::SceneModel::constrain_move) — so an
/// app's own drag and the view's own drag cannot end up obeying two different
/// readings of one closure.
///
/// Only the returned frame's **origin** is read: a translation is all these
/// routes can express, so a constraint that also resizes the frame has that
/// part of its answer ignored rather than silently half-applied.
pub(crate) fn constrained_translation(
    call: &ConstraintCall<'_>,
    items: &[ItemId],
    start: &TransformFrame,
    translation: Vec2,
    source: TransformSource,
    magnet_snapped: bool,
) -> Vec2 {
    let to_frame =
        Transform2D::rotate(-start.rotation).apply_point(Point::new(translation.x, translation.y));
    let proposed = TransformFrame {
        rect: Rect::new(
            start.rect.x + to_frame.x,
            start.rect.y + to_frame.y,
            start.rect.width,
            start.rect.height,
        ),
        ..*start
    };
    let out = call.frame(
        TransformOp::Move,
        items,
        start,
        proposed,
        source,
        magnet_snapped,
    );
    let d = Transform2D::rotate(start.rotation).apply_point(Point::new(
        out.rect.x - start.rect.x,
        out.rect.y - start.rect.y,
    ));
    Vec2::new(d.x, d.y)
}

/// Raises the scene's "a constraint is running" flag for the duration of one
/// constraint call and lowers it again on the way out — including on an unwind,
/// which is why it is a guard and not a pair of statements.
///
/// The flag is what lets
/// [`SceneModel::write_guard`](crate::SceneModel::write_guard) name the caller's
/// mistake instead of reporting `RefCell already borrowed`.
///
/// A flag and not a counter: [`ConstraintCall::new`] refuses to build a second
/// call while one is running, so the only two states are "running" and "not",
/// and a counter would advertise a nesting the crate does not have.
struct ConstraintGuard<'a>(&'a Cell<bool>);

impl<'a> ConstraintGuard<'a> {
    fn enter(scene: &'a Scene) -> Self {
        let flag = scene.constraint_running_cell();
        debug_assert!(
            !flag.get(),
            "ConstraintGuard entered while a constraint was already running — \
             ConstraintCall::new is supposed to have refused that."
        );
        flag.set(true);
        Self(flag)
    }
}

impl Drop for ConstraintGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::RectItem;

    fn change<'a>(
        scene: &'a Scene,
        items: &'a [ItemId],
        start: TransformFrame,
        proposed: TransformFrame,
    ) -> ProposedChange<'a> {
        ProposedChange {
            scene,
            items,
            op: TransformOp::Move,
            start,
            proposed,
            source: TransformSource::Pointer,
            magnet_snapped: false,
        }
    }

    fn frame(x: f32, y: f32, rotation: f32) -> TransformFrame {
        TransformFrame {
            rect: Rect::new(x, y, 40.0, 20.0),
            rotation,
            count: 1,
        }
    }

    #[test]
    fn translation_and_translated_round_trip_unrotated() {
        let scene = Scene::new();
        let ids: Vec<ItemId> = Vec::new();
        let c = change(&scene, &ids, frame(10.0, 10.0, 0.0), frame(37.0, -4.0, 0.0));
        let t = c.translation();
        assert!((t.x - 27.0).abs() < 1e-4, "{t:?}");
        assert!((t.y + 14.0).abs() < 1e-4, "{t:?}");
        let back = c.translated(t);
        assert!((back.rect.x - c.proposed.rect.x).abs() < 1e-4);
        assert!((back.rect.y - c.proposed.rect.y).abs() < 1e-4);
    }

    #[test]
    fn translation_and_translated_round_trip_rotated() {
        let scene = Scene::new();
        let ids: Vec<ItemId> = Vec::new();
        let rot = std::f32::consts::FRAC_PI_4;
        let c = change(&scene, &ids, frame(10.0, 10.0, rot), frame(37.0, -4.0, rot));
        let t = c.translation();
        let back = c.translated(t);
        assert!((back.rect.x - c.proposed.rect.x).abs() < 1e-3, "{back:?}");
        assert!((back.rect.y - c.proposed.rect.y).abs() < 1e-3, "{back:?}");
    }

    #[test]
    fn reject_is_exactly_adjust_start() {
        let mut scene = Scene::new();
        let id = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
        scene.set_geometry_constraint(|_| ChangeVerdict::Reject);
        let start = frame(0.0, 0.0, 0.0);
        let rejected = {
            let call = ConstraintCall::new(&scene).expect("installed");
            call.frame(
                TransformOp::Move,
                &[id],
                &start,
                frame(100.0, 100.0, 0.0),
                TransformSource::Pointer,
                false,
            )
        };
        assert_eq!(rejected, start);

        scene.set_geometry_constraint(|c| ChangeVerdict::Adjust(c.start));
        let adjusted = {
            let call = ConstraintCall::new(&scene).expect("installed");
            call.frame(
                TransformOp::Move,
                &[id],
                &start,
                frame(100.0, 100.0, 0.0),
                TransformSource::Pointer,
                false,
            )
        };
        assert_eq!(rejected, adjusted);
    }

    #[test]
    fn the_guard_is_raised_only_inside_the_call() {
        let mut scene = Scene::new();
        scene.set_geometry_constraint(|c| {
            assert!(c.scene.in_geometry_constraint(), "depth raised inside");
            ChangeVerdict::Accept
        });
        assert!(!scene.in_geometry_constraint());
        let start = frame(0.0, 0.0, 0.0);
        {
            let call = ConstraintCall::new(&scene).expect("installed");
            let _ = call.frame(
                TransformOp::Move,
                &[],
                &start,
                frame(1.0, 1.0, 0.0),
                TransformSource::Pointer,
                false,
            );
        }
        assert!(!scene.in_geometry_constraint(), "lowered on the way out");
    }

    #[test]
    fn the_guard_is_lowered_when_the_constraint_panics() {
        let mut scene = Scene::new();
        scene.set_geometry_constraint(|_| panic!("policy blew up"));
        let start = frame(0.0, 0.0, 0.0);
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let call = ConstraintCall::new(&scene).expect("installed");
            call.frame(
                TransformOp::Move,
                &[],
                &start,
                frame(1.0, 1.0, 0.0),
                TransformSource::Pointer,
                false,
            )
        }));
        std::panic::set_hook(hook);
        assert!(caught.is_err());
        assert!(
            !scene.in_geometry_constraint(),
            "a panicking constraint must not strand the scene at depth"
        );
    }

    #[test]
    fn no_constraint_means_no_call() {
        let scene = Scene::new();
        assert!(ConstraintCall::new(&scene).is_none());
        assert!(!scene.has_geometry_constraint());
    }
}
