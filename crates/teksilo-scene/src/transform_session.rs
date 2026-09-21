// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The selection **transform controller**: group move, group resize and
//! group rotate, with a frame and handles drawn over the selection.
//!
//! # The one design choice everything else follows from
//!
//! A transform is a **per-view, model-free session**. It reads
//! [`SceneSelection`](crate::SceneSelection), it paints a frame, and it
//! mutates the scene **exactly once** — at the end of the gesture — through a
//! single [`Scene::apply_transform_delta`](crate::Scene::apply_transform_delta).
//! Nothing is written while the pointer moves.
//!
//! Three things fall out of that and none of them had to be built:
//!
//! * **Cancel is `session = None`.** There is no rollback, so there is no
//!   rollback to get wrong — and the crate has already shipped one drag whose
//!   cancel arm left a stranded translation behind.
//! * **One gesture is one reversible step.** `apply_transform_delta` is the
//!   only write, so the transaction boundary is structural rather than a
//!   convention someone has to remember. (The *history* that makes use of it
//!   belongs to the data layer, not here — this crate builds the mechanism and
//!   stops.)
//! * **A pointer sample costs a relayout, not N model writes.** A ten-item
//!   selection emits zero `ItemChange`s until the release.
//!
//! # What a resize does, and why there is only one answer
//!
//! The **commit** writes `local_bounds`, and the item reflows into the new box.
//! Not a visual scale that is later baked: a heavyweight card relayouts, a
//! [`RectItem`](crate::RectItem) redraws, a [`PathItem`](crate::PathItem) fits
//! its geometry to the rectangle. A heavyweight entry's `local_bounds` *is* its
//! layout size, so there is no second route for it, and since `PathItem` grew a
//! real fit there is no second route for the lightweight tier either.
//!
//! ## The preview is not that, on the lightweight tier
//!
//! While the pointer is down nothing is written, so a resize can only be
//! *shown*, and the two tiers show it differently:
//!
//! | tier | how the preview is shown | what the user sees |
//! | --- | --- | --- |
//! | heavyweight card | the placement rectangle is the previewed one, so the widget lays out at the new size | a live reflow; text re-wraps as the handle moves |
//! | lightweight item | the preview affine is composed into the item's `local → scene` | a **visual scale**; strokes thicken, glyphs stretch |
//!
//! Both land on the same `local_bounds` write at the release, and both preview
//! the same *outline* — the box never jumps — but a wrapped
//! [`TextItem`](crate::TextItem) re-wraps at the instant the handle is let go,
//! and a stroked item's hairline snaps back to its real width. That is the
//! honest description of the current mechanism and it is pinned by
//! `a_lightweight_resize_previews_the_box_it_commits`.
//!
//! Making the lightweight preview a reflow too needs a channel the paint pass
//! does not have: an item reflows through `SceneItem::set_local_bounds`, which
//! takes `&mut self`, and the preview must not write the model — that is what
//! makes "cancel is `session = None`" true. A previewed-bounds field on
//! `SceneItemPaintContext` would close it; until then the translation and the
//! rotation halves of the preview are exact on both tiers and the scale half is
//! exact only on the heavyweight one.
//!
//! # What a rotate does, and what it refuses
//!
//! It post-rotates the item's own `Transform2D` and orbits its `local_pos`
//! about the pivot. It is refused for a **heavyweight** entry, and the reason
//! is not that the framework cannot hit-test a rotated widget — it can, a self
//! transform scope inverse-transforms before testing. It is that `SceneView`
//! sizes a card from the AABB of its transformed bounds, so a rotation there
//! inflates the layout box and rotates nothing visible. Giving cards a real
//! per-child rotation scope is a change to how they are placed and to how their
//! accessibility rectangles are derived; it is not a line in this module.
//!
//! # Where the gesture is picked up, and the one honest limitation
//!
//! A `SceneView` sees a press over a heavyweight child only when that child
//! does not claim it — a card that calls `capture_pointer` or carries its own
//! `on_drag` (a `Splitter` handle, a `SpinBox` step button, a `TextInput`'s
//! selection drag) wins its own press, by design. So **body drag over a card is
//! best-effort**, and the reliable route is the controller's own chrome: the
//! frame outline is drawn `padding` **outside** the selection, and both the
//! frame band and every handle therefore sit on pixels the card does not own.
//! Grab the frame, not the card.
//!
//! # The three routes, and what a screen reader can reach
//!
//! The pointer drags a handle. The keyboard enters a roving mode with
//! [`TransformConfig::transform_key`] (`t` by default), `Tab`s between handles
//! and drives the focused one with the arrows. Assistive technology has neither
//! a pointer nor arrow keys — it has *verbs* — so each handle publishes the
//! verbs that suit what it is:
//!
//! | handle | AccessKit role | value | `Increment` / `Decrement` | custom actions |
//! | --- | --- | --- | --- | --- |
//! | an edge midpoint | `Slider` | that edge's coordinate | moves **that** coordinate by one | — |
//! | the rotate puck | `Slider` | the angle in degrees | turns it by one degree | — |
//! | a corner | `Button` | none — it is two numbers | moves **both** of its coordinates by one | one per direction |
//! | the frame band | `Button` | none | moves the whole selection on both axes | one per direction |
//!
//! One rule covers the whole table: **a verb moves every coordinate the handle
//! drives, and leaves the rest alone.** See [`TransformHandle::at_steps`] for
//! why a corner answers a one-dimensional verb with a diagonal, and
//! [`TransformStep`] for the four named actions that give one axis at a time.
//!
//! Every route ends in the same [`LiveSession`] and the same commit, so
//! `docs/a11y/non-drag-alternatives.md`'s "the alternative must make the same
//! model change the drag makes" is structural rather than asserted. The names
//! are [`TransformLabels`], which takes `tr!`.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Canvas, Point, Rect, Transform2D, Vec2};
use teksilo_core::event::Key;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{CursorIcon, EventContext, PaintContext};
use teksilo_core::widget_id::WidgetId;

use crate::item::ItemId;

/// One grab affordance on the selection frame.
///
/// [`Move`](Self::Move) is the frame band itself (and the body of a selected
/// item); [`Rotate`](Self::Rotate) is the detached puck above the top edge; the
/// other eight are the resize anchors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum TransformHandle {
    /// The frame band — drag it to move the whole selection.
    Move,
    /// Top-leading corner.
    TopLeading,
    /// Top edge midpoint.
    Top,
    /// Top-trailing corner.
    TopTrailing,
    /// Leading edge midpoint.
    Leading,
    /// Trailing edge midpoint.
    Trailing,
    /// Bottom-leading corner.
    BottomLeading,
    /// Bottom edge midpoint.
    Bottom,
    /// Bottom-trailing corner.
    BottomTrailing,
    /// The rotate puck, above the top edge.
    Rotate,
}

/// Every handle, in the order the keyboard route rotates through them.
pub(crate) const HANDLE_ORDER: [TransformHandle; 10] = [
    TransformHandle::Move,
    TransformHandle::TopLeading,
    TransformHandle::Top,
    TransformHandle::TopTrailing,
    TransformHandle::Trailing,
    TransformHandle::BottomTrailing,
    TransformHandle::Bottom,
    TransformHandle::BottomLeading,
    TransformHandle::Leading,
    TransformHandle::Rotate,
];

impl TransformHandle {
    /// Bit position in a [`TransformHandleSet`], and the AccessKit element id
    /// of this handle's node (offset by one so the frame keeps id `0`).
    pub(crate) const fn index(self) -> u16 {
        match self {
            Self::Move => 0,
            Self::TopLeading => 1,
            Self::Top => 2,
            Self::TopTrailing => 3,
            Self::Leading => 4,
            Self::Trailing => 5,
            Self::BottomLeading => 6,
            Self::Bottom => 7,
            Self::BottomTrailing => 8,
            Self::Rotate => 9,
        }
    }

    /// Which operation dragging this handle performs.
    pub const fn op(self) -> TransformOp {
        match self {
            Self::Move => TransformOp::Move,
            Self::Rotate => TransformOp::Rotate,
            _ => TransformOp::Resize,
        }
    }

    /// How this handle drives the frame's leading/trailing extent.
    fn x_role(self) -> Option<SideRole> {
        match self {
            Self::TopLeading | Self::Leading | Self::BottomLeading => Some(SideRole::Min),
            Self::TopTrailing | Self::Trailing | Self::BottomTrailing => Some(SideRole::Max),
            _ => None,
        }
    }

    /// How this handle drives the frame's top/bottom extent.
    fn y_role(self) -> Option<SideRole> {
        match self {
            Self::TopLeading | Self::Top | Self::TopTrailing => Some(SideRole::Min),
            Self::BottomLeading | Self::Bottom | Self::BottomTrailing => Some(SideRole::Max),
            _ => None,
        }
    }

    /// The pointer cursor for this handle, in an **unrotated** frame. A rotated
    /// frame keeps these — the four diagonal / axis cursors do not have enough
    /// resolution to track an arbitrary angle, and guessing wrong is worse than
    /// a cursor that is merely approximate.
    pub const fn cursor(self) -> CursorIcon {
        match self {
            Self::Move => CursorIcon::Move,
            Self::TopLeading | Self::BottomTrailing => CursorIcon::NwseResize,
            Self::TopTrailing | Self::BottomLeading => CursorIcon::NeswResize,
            Self::Top | Self::Bottom => CursorIcon::RowResize,
            Self::Leading | Self::Trailing => CursorIcon::ColResize,
            Self::Rotate => CursorIcon::Grab,
        }
    }

    /// The untranslated English name this handle publishes to assistive
    /// technology when the app installs no [`TransformLabels`] of its own.
    pub(crate) const fn default_label(self) -> &'static str {
        match self {
            Self::Move => "Move selection",
            Self::TopLeading => "Resize top leading",
            Self::Top => "Resize top",
            Self::TopTrailing => "Resize top trailing",
            Self::Leading => "Resize leading",
            Self::Trailing => "Resize trailing",
            Self::BottomLeading => "Resize bottom leading",
            Self::Bottom => "Resize bottom",
            Self::BottomTrailing => "Resize bottom trailing",
            Self::Rotate => "Rotate selection",
        }
    }

    /// Whether this handle's state is **one** number or two.
    ///
    /// An edge midpoint carries the coordinate of the edge it drags; the rotate
    /// puck carries an angle. Both are one number, so both are a `Slider` with
    /// a `numeric_value`, and `Increment` / `Decrement` mean exactly "make that
    /// number bigger / smaller".
    ///
    /// A corner carries two coordinates and the frame band carries the whole
    /// selection's position, so neither has a single value to announce. They
    /// are `Button`s, and their one-dimensional verbs move **both** coordinates
    /// — see [`at_steps`](Self::at_steps).
    ///
    /// Three things read this and must agree: the published role, whether a
    /// `numeric_value` is set, and what one assistive-technology verb does.
    pub const fn is_scalar(self) -> bool {
        matches!(
            self,
            Self::Top | Self::Bottom | Self::Leading | Self::Trailing | Self::Rotate
        )
    }

    /// The steps one assistive-technology `Increment` (or `Decrement`) is made
    /// of, as directions the keyboard route already understands.
    ///
    /// **One rule covers every handle: a step moves each coordinate the handle
    /// drives by `+1` (`Increment`) or `-1` (`Decrement`), and leaves the rest
    /// alone.**
    ///
    /// For a [scalar](Self::is_scalar) handle that is the coordinate its
    /// `numeric_value` announces, which is what makes the slider contract true:
    /// `Increment` on "Resize bottom" raises the bottom edge's `y`, and the
    /// announced number goes up by one. Before this existed every verb was a
    /// horizontal nudge, so `Top` and `Bottom` reported "handled" and moved
    /// nothing at all — the item's height could not be changed by any
    /// assistive-technology route.
    ///
    /// For a corner the same rule gives the diagonal: `Increment` on "Resize
    /// top leading" moves that corner's `x` **and** `y` up by one, which is
    /// exactly `Increment` on "Resize top" plus `Increment` on "Resize
    /// leading" — the two edges the corner sits between, together. That is the
    /// only motion a corner can express with a one-dimensional verb, and it
    /// keeps the corner reachable when a configuration offers corners and
    /// nothing else. A user who needs one axis at a time reaches it through the
    /// four named custom actions a non-scalar handle also publishes (see
    /// [`TransformStep`]); the frame band works the same way.
    pub const fn at_steps(self, increment: bool) -> &'static [TransformStep] {
        const IN_X: &[TransformStep] = &[TransformStep::Trailing];
        const DE_X: &[TransformStep] = &[TransformStep::Leading];
        const IN_Y: &[TransformStep] = &[TransformStep::Down];
        const DE_Y: &[TransformStep] = &[TransformStep::Up];
        const IN_XY: &[TransformStep] = &[TransformStep::Trailing, TransformStep::Down];
        const DE_XY: &[TransformStep] = &[TransformStep::Leading, TransformStep::Up];
        match self {
            // The puck's number is an angle, and the keyboard route reads a
            // trailing/leading arrow as clockwise/anticlockwise — so the same
            // pair that raises an `x` raises a rotation.
            Self::Leading | Self::Trailing | Self::Rotate => {
                if increment {
                    IN_X
                } else {
                    DE_X
                }
            }
            Self::Top | Self::Bottom => {
                if increment {
                    IN_Y
                } else {
                    DE_Y
                }
            }
            _ => {
                if increment {
                    IN_XY
                } else {
                    DE_XY
                }
            }
        }
    }
}

/// One directional nudge on a transform handle.
///
/// The vocabulary the controller's non-pointer routes are stated in: the
/// keyboard's four arrows, the two halves of an assistive-technology
/// `Increment` / `Decrement`, and the four **custom actions** a corner or the
/// frame band publishes so that a screen-reader user can drive one axis at a
/// time. Named for the frame's own axes, like every other name in this crate —
/// `Leading` is `-x` and `Trailing` is `+x`, whatever the reading direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum TransformStep {
    /// Decrease the driven vertical coordinate.
    Up,
    /// Increase it.
    Down,
    /// Decrease the driven horizontal coordinate.
    Leading,
    /// Increase it.
    Trailing,
}

impl TransformStep {
    /// Every step, in the order a non-scalar handle publishes its custom
    /// actions. The position in this array **is** the
    /// [`accesskit::CustomAction`] id, so it is part of the published
    /// contract: reorder it and an assistive technology holding a stale tree
    /// invokes the wrong direction.
    pub const ALL: [TransformStep; 4] = [Self::Up, Self::Down, Self::Leading, Self::Trailing];

    /// This step's index in [`ALL`](Self::ALL).
    pub const fn index(self) -> usize {
        match self {
            Self::Up => 0,
            Self::Down => 1,
            Self::Leading => 2,
            Self::Trailing => 3,
        }
    }

    /// The step at `index`, or `None` — the inverse of
    /// [`index`](Self::index), used to resolve a custom-action id arriving
    /// from an assistive technology.
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Up),
            1 => Some(Self::Down),
            2 => Some(Self::Leading),
            3 => Some(Self::Trailing),
            _ => None,
        }
    }

    /// The arrow key this step is the name of. One vocabulary, so the keyboard
    /// route and the assistive-technology route cannot drift apart.
    pub(crate) const fn key(self) -> Key {
        match self {
            Self::Up => Key::ArrowUp,
            Self::Down => Key::ArrowDown,
            Self::Leading => Key::ArrowLeft,
            Self::Trailing => Key::ArrowRight,
        }
    }

    /// This step as a one-element slice, so a custom action and a standard
    /// verb reach [`TransformDriver::access_action`] through one path.
    ///
    /// [`TransformDriver::access_action`]: crate::SceneView
    pub(crate) const fn as_slice(self) -> &'static [TransformStep] {
        match self {
            Self::Up => &[Self::Up],
            Self::Down => &[Self::Down],
            Self::Leading => &[Self::Leading],
            Self::Trailing => &[Self::Trailing],
        }
    }

    /// The untranslated English name this step publishes when the app installs
    /// no [`TransformLabels`] of its own. The handle's own name supplies the
    /// subject, so a screen reader reads "Resize top leading, Step up".
    pub(crate) const fn default_label(self) -> &'static str {
        match self {
            Self::Up => "Step up",
            Self::Down => "Step down",
            Self::Leading => "Step leading",
            Self::Trailing => "Step trailing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SideRole {
    Min,
    Max,
}

/// What one gesture asks of the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TransformOp {
    /// Translate the selection.
    Move,
    /// Change the selection's extent.
    Resize,
    /// Turn the selection about its centre.
    Rotate,
}

impl TransformOp {
    /// The per-item flag an item must carry to take part in this operation.
    pub const fn required_flag(self) -> crate::flags::ItemFlags {
        match self {
            Self::Move => crate::flags::ItemFlags::IS_DRAGGABLE,
            Self::Resize => crate::flags::ItemFlags::IS_RESIZABLE,
            Self::Rotate => crate::flags::ItemFlags::IS_ROTATABLE,
        }
    }
}

/// A bitset of [`TransformHandle`]s — Konva's `enabledAnchors`, in the house
/// style of [`ItemFlags`](crate::ItemFlags).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransformHandleSet(u16);

impl TransformHandleSet {
    /// No handles at all.
    pub const NONE: Self = Self(0);
    /// The frame band only.
    pub const MOVE: Self = Self(1 << 0);
    /// The four diagonal corners.
    pub const CORNERS: Self = Self((1 << 1) | (1 << 3) | (1 << 6) | (1 << 8));
    /// The four edge midpoints.
    pub const EDGES: Self = Self((1 << 2) | (1 << 4) | (1 << 5) | (1 << 7));
    /// Corners and edges.
    pub const ALL_RESIZE: Self = Self(Self::CORNERS.0 | Self::EDGES.0);
    /// The rotate puck only.
    pub const ROTATE: Self = Self(1 << 9);
    /// Everything — the default.
    pub const ALL: Self = Self(Self::MOVE.0 | Self::ALL_RESIZE.0 | Self::ROTATE.0);

    /// This set plus `h`.
    pub const fn with(self, h: TransformHandle) -> Self {
        Self(self.0 | (1 << h.index()))
    }

    /// This set minus `h`.
    pub const fn without(self, h: TransformHandle) -> Self {
        Self(self.0 & !(1 << h.index()))
    }

    /// Whether `h` is in the set.
    pub const fn contains(self, h: TransformHandle) -> bool {
        (self.0 & (1 << h.index())) != 0
    }

    /// Raw bits (debug / serialization).
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Construct from raw bits.
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }
}

impl Default for TransformHandleSet {
    fn default() -> Self {
        Self::ALL
    }
}

impl std::ops::BitOr for TransformHandleSet {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// What one in-flight gesture asks of the selection, stated **once** in scene
/// coordinates.
///
/// The preview paints it, the commit applies it, the scene's geometry
/// constraint rewrites it and
/// the announcement describes it — so those four can never disagree.
///
/// The affine it denotes is, innermost first: move the pivot to the origin,
/// rotate into the frame's basis, scale, rotate back out, rotate by
/// [`rotation`](Self::rotation), move the pivot back, translate. The scale is
/// therefore taken **along the frame's own axes**, which is what makes a
/// rotated single-item frame resize exactly rather than shear.
///
/// `#[non_exhaustive]`: the crate hands this *to* consumer code — a
/// [`ProposedChange`](crate::ProposedChange) reads it and
/// [`TransformConfig::on_end`] records it — and the affine it denotes may grow
/// a term. Build one with [`new`](Self::new), [`IDENTITY`](Self::IDENTITY) or
/// [`between`](Self::between); the fields stay public and assignable.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TransformDelta {
    /// Scene-space point every scale and rotation is taken about.
    pub pivot: Point,
    /// The frame's angle, in radians — the basis the [`scale`](Self::scale) is
    /// expressed in.
    pub basis: f32,
    /// Multiplier along the frame's axes. Never negative: a resize stops at
    /// the controller's `min_size` rather than mirroring, because a
    /// negative-extent `Rect` is silently un-hit-testable and un-indexable.
    pub scale: Vec2,
    /// Additive rotation, in radians.
    pub rotation: f32,
    /// Scene-space translation, applied last.
    pub translation: Vec2,
}

impl TransformDelta {
    /// The delta that changes nothing.
    pub const IDENTITY: Self = Self {
        pivot: Point { x: 0.0, y: 0.0 },
        basis: 0.0,
        scale: Vec2 { x: 1.0, y: 1.0 },
        rotation: 0.0,
        translation: Vec2 { x: 0.0, y: 0.0 },
    };

    /// A delta stated field by field — the constructor
    /// [`#[non_exhaustive]`](Self) takes the place of a struct literal for.
    ///
    /// The one an app reaches for to apply a programmatic transform through
    /// [`Scene::apply_transform_delta`](crate::Scene::apply_transform_delta)
    /// without driving a gesture. See the type's own documentation for the
    /// order the five terms compose in.
    pub fn new(pivot: Point, basis: f32, scale: Vec2, rotation: f32, translation: Vec2) -> Self {
        Self {
            pivot,
            basis,
            scale,
            rotation,
            translation,
        }
    }

    /// Whether applying this delta would leave every item where it is.
    pub fn is_identity(&self) -> bool {
        (self.scale.x - 1.0).abs() < 1e-6
            && (self.scale.y - 1.0).abs() < 1e-6
            && self.rotation.abs() < 1e-6
            && self.translation.x.abs() < 1e-4
            && self.translation.y.abs() < 1e-4
    }

    /// The scene-space affine this delta denotes.
    pub fn to_scene_transform(&self) -> Transform2D {
        Transform2D::translate(-self.pivot.x, -self.pivot.y)
            .then(&Transform2D::rotate(-self.basis))
            .then(&Transform2D::scale(self.scale.x, self.scale.y))
            .then(&Transform2D::rotate(self.basis))
            .then(&Transform2D::rotate(self.rotation))
            .then(&Transform2D::translate(self.pivot.x, self.pivot.y))
            .then(&Transform2D::translate(
                self.translation.x,
                self.translation.y,
            ))
    }

    /// The delta that takes `start` to `end` about `pivot`.
    ///
    /// The inverse of [`frame_after`](Self::frame_after), and the reason a
    /// [geometry constraint](crate::ProposedChange) can hand back an adjusted
    /// *frame* and have the applied delta follow it exactly.
    pub fn between(start: &TransformFrame, end: &TransformFrame, pivot: Point) -> Self {
        let sx = if start.rect.width.abs() > 1e-6 {
            end.rect.width / start.rect.width
        } else {
            1.0
        };
        let sy = if start.rect.height.abs() > 1e-6 {
            end.rect.height / start.rect.height
        } else {
            1.0
        };
        let basis = start.rotation;
        let mut probe = Self {
            pivot,
            basis,
            scale: Vec2::new(sx, sy),
            rotation: end.rotation - start.rotation,
            translation: Vec2::ZERO,
        };
        // Whatever the pivot-and-scale part did not account for is the
        // translation. Measured on the frame's own origin rather than derived
        // algebraically, so the round-trip is exact by construction.
        let predicted = probe.frame_after(start);
        let d = Vec2::new(end.rect.x - predicted.rect.x, end.rect.y - predicted.rect.y);
        // `d` is in frame coordinates; the delta's translation is in scene
        // coordinates.
        let scene_d = Transform2D::rotate(end.rotation).apply_point(Point::new(d.x, d.y));
        probe.translation = Vec2::new(scene_d.x, scene_d.y);
        probe
    }

    /// The frame `start` becomes under this delta.
    pub fn frame_after(&self, start: &TransformFrame) -> TransformFrame {
        let new_rotation = start.rotation + self.rotation;
        // Work in the *start* frame's basis, then restate the result in the
        // new one. A pure rotation leaves the rectangle alone and only turns
        // the basis, which is exactly what a rotated selection should do.
        let pivot_f = Transform2D::rotate(-self.basis).apply_point(self.pivot);
        let x0 = pivot_f.x + (start.rect.x - pivot_f.x) * self.scale.x;
        let y0 = pivot_f.y + (start.rect.y - pivot_f.y) * self.scale.y;
        let w = start.rect.width * self.scale.x;
        let h = start.rect.height * self.scale.y;
        // The translation is a scene vector; express it in the *new* basis,
        // which is the basis `rect` is stated in after the rotation.
        let t = Transform2D::rotate(-new_rotation)
            .apply_point(Point::new(self.translation.x, self.translation.y));
        TransformFrame {
            rect: Rect::new(x0 + t.x, y0 + t.y, w, h),
            rotation: new_rotation,
            count: start.count,
        }
    }
}

/// The box the handles are drawn on.
///
/// [`rect`](Self::rect) is stated in the frame's **own** basis: a point `p` of
/// it sits at `Rot(rotation) · p` in scene coordinates.
///
/// `#[non_exhaustive]`: the crate hands this *to* consumer code and asks for it
/// back — a [`ProposedChange`](crate::ProposedChange) is handed the proposed
/// frame and may answer with an adjusted one — so it is both a receive type and
/// a construct type. Build one with [`new`](Self::new).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TransformFrame {
    /// The union of the selection, in the frame's own (unrotated) basis.
    pub rect: Rect,
    /// The frame's angle, in radians. Non-zero only for a single-item
    /// selection that carries its own rotation — a multi-item union has no
    /// well-defined angle, so its frame is axis-aligned.
    pub rotation: f32,
    /// How many selection roots the frame encloses.
    pub count: usize,
}

impl TransformFrame {
    /// A frame stated field by field — the constructor
    /// [`#[non_exhaustive]`](Self) takes the place of a struct literal for.
    ///
    /// A geometry constraint that wants to adjust the frame it was handed
    /// normally derives one from that frame rather than building it here; this
    /// is for the constraint that states its answer outright, and for a
    /// consumer's own tests.
    pub fn new(rect: Rect, rotation: f32, count: usize) -> Self {
        Self {
            rect,
            rotation,
            count,
        }
    }

    /// Map a point of this frame's basis into scene coordinates.
    pub fn to_scene(&self, p: Point) -> Point {
        Transform2D::rotate(self.rotation).apply_point(p)
    }

    /// Map a scene point into this frame's basis.
    pub fn from_scene(&self, p: Point) -> Point {
        Transform2D::rotate(-self.rotation).apply_point(p)
    }

    /// The frame's centre, in scene coordinates.
    pub fn centre_scene(&self) -> Point {
        self.to_scene(Point::new(
            self.rect.x + self.rect.width * 0.5,
            self.rect.y + self.rect.height * 0.5,
        ))
    }

    /// The outline the chrome is drawn on: [`rect`](Self::rect) grown by
    /// `padding` scene units on every side.
    pub fn outline(&self, padding: f32) -> Rect {
        Rect::new(
            self.rect.x - padding,
            self.rect.y - padding,
            self.rect.width + padding * 2.0,
            self.rect.height + padding * 2.0,
        )
    }

    /// Where `handle` sits, in this frame's basis.
    ///
    /// `padding` and `rotate_offset` are scene units (a caller converts from
    /// screen pixels by dividing by the live view scale, so the chrome keeps a
    /// constant on-screen size at any zoom).
    pub fn handle_point(&self, handle: TransformHandle, padding: f32, rotate_offset: f32) -> Point {
        let r = self.outline(padding);
        let cx = r.x + r.width * 0.5;
        let cy = r.y + r.height * 0.5;
        match handle {
            TransformHandle::Move => Point::new(cx, cy),
            TransformHandle::TopLeading => Point::new(r.x, r.y),
            TransformHandle::Top => Point::new(cx, r.y),
            TransformHandle::TopTrailing => Point::new(r.right(), r.y),
            TransformHandle::Leading => Point::new(r.x, cy),
            TransformHandle::Trailing => Point::new(r.right(), cy),
            TransformHandle::BottomLeading => Point::new(r.x, r.bottom()),
            TransformHandle::Bottom => Point::new(cx, r.bottom()),
            TransformHandle::BottomTrailing => Point::new(r.right(), r.bottom()),
            TransformHandle::Rotate => Point::new(cx, r.y - rotate_offset),
        }
    }
}

/// Whether the items follow the gesture or only the frame does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LivePreview {
    /// Items follow the gesture. One relayout of this view per pointer sample.
    #[default]
    Live,
    /// Only the frame moves; the items jump at the commit.
    Ghost,
}

/// Which route produced a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransformSource {
    /// A pointer drag on the chrome or on a selected item's body.
    Pointer,
    /// The keyboard transform mode, or an assistive-technology action.
    Keyboard,
}

/// How a session finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransformOutcome {
    /// The delta was written to the scene.
    Committed,
    /// Nothing was written.
    Cancelled,
}

/// A snapshot of the live session, handed to the three event hooks and
/// published by
/// [`SceneView::transform_session_signal`](crate::SceneView::transform_session_signal).
///
/// `#[non_exhaustive]`: the crate hands this *to* consumer code — it is the
/// argument of all three hooks — and a session may learn to carry more. Build
/// one with [`new`](Self::new), which is what a consumer's own test of its
/// `on_end` needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TransformSession {
    /// The selection **roots** this gesture will actually change — pruned of
    /// any item whose ancestor is also in the set (moving the ancestor already
    /// moves it) and filtered to those carrying the operation's flag.
    pub items: Rc<[ItemId]>,
    /// The handle being driven.
    pub handle: TransformHandle,
    /// The frame as it was when the gesture started.
    pub start_frame: TransformFrame,
    /// The frame now, **after** the scene's geometry constraint.
    pub frame: TransformFrame,
    /// The delta now, in agreement with [`frame`](Self::frame).
    pub delta: TransformDelta,
    /// Which route produced it.
    pub source: TransformSource,
}

impl TransformSession {
    /// A session snapshot stated field by field — the constructor
    /// [`#[non_exhaustive]`](Self) takes the place of a struct literal for.
    ///
    /// The controller builds its own; this exists so a consumer can drive its
    /// `on_start` / `on_change` / `on_end` from a test without a live view.
    pub fn new(
        items: Rc<[ItemId]>,
        handle: TransformHandle,
        start_frame: TransformFrame,
        frame: TransformFrame,
        delta: TransformDelta,
        source: TransformSource,
    ) -> Self {
        Self {
            items,
            handle,
            start_frame,
            frame,
            delta,
            source,
        }
    }

    /// The operation this session performs.
    pub fn op(&self) -> TransformOp {
        self.handle.op()
    }
}

/// Everything the chrome painter is told. Scene coordinates throughout — the
/// view transform is already on the canvas, exactly as for
/// [`MagnetismConfig::feedback`](crate::MagnetismConfig::feedback).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TransformChrome {
    /// The frame to draw (the live preview frame during a gesture).
    pub frame: TransformFrame,
    /// The outline rectangle, in the frame's basis — `frame.rect` grown by the
    /// configured padding.
    pub outline: Rect,
    /// Every visible handle and its scene-space position.
    pub handles: Vec<(TransformHandle, Point)>,
    /// Half the handle's on-screen size, in **scene** units.
    pub handle_half: f32,
    /// The live view scale — divide a screen-pixel constant by it to get a
    /// scene-unit one.
    pub view_scale: f32,
    /// The handle under the pointer, if any.
    pub hovered: Option<TransformHandle>,
    /// The handle being dragged, if a gesture is live.
    pub active: Option<TransformHandle>,
    /// The handle the keyboard transform mode has roving focus on.
    pub keyboard_focus: Option<TransformHandle>,
}

/// The assistive-technology names the controller publishes.
///
/// Defaults are the crate's own untranslated English, the same way a magnet's
/// node is named "Connection point". Every setter takes
/// `impl Into<Prop<String>>`, so an app hands it `tr!(…)` and the names follow
/// the locale.
#[derive(Clone)]
pub struct TransformLabels {
    frame: Prop<String>,
    handles: [Option<Prop<String>>; 10],
    steps: [Option<Prop<String>>; 4],
}

impl std::fmt::Debug for TransformLabels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransformLabels")
            .field("frame", &self.frame.get())
            .field(
                "overridden_handles",
                &self.handles.iter().filter(|h| h.is_some()).count(),
            )
            .field(
                "overridden_steps",
                &self.steps.iter().filter(|s| s.is_some()).count(),
            )
            .finish()
    }
}

impl Default for TransformLabels {
    fn default() -> Self {
        Self::new()
    }
}

impl TransformLabels {
    /// English defaults.
    pub fn new() -> Self {
        Self {
            frame: Prop::Static("Selection".to_string()),
            handles: Default::default(),
            steps: Default::default(),
        }
    }

    /// The name of the frame node itself.
    pub fn frame(mut self, label: impl Into<Prop<String>>) -> Self {
        self.frame = label.into();
        self
    }

    /// The name of one handle's node.
    pub fn handle(mut self, handle: TransformHandle, label: impl Into<Prop<String>>) -> Self {
        self.handles[handle.index() as usize] = Some(label.into());
        self
    }

    /// Resolve the frame's name now.
    pub fn frame_name(&self) -> String {
        self.frame.get()
    }

    /// The name of one per-axis custom action, published by every handle whose
    /// verb is two-dimensional — the four corners and the frame band. The
    /// handle's own name supplies the subject, so this names only the
    /// direction ("Step up", not "Move the selection up").
    pub fn step(mut self, step: TransformStep, label: impl Into<Prop<String>>) -> Self {
        self.steps[step.index()] = Some(label.into());
        self
    }

    /// Resolve one handle's name now.
    pub fn handle_name(&self, handle: TransformHandle) -> String {
        match &self.handles[handle.index() as usize] {
            Some(p) => p.get(),
            None => handle.default_label().to_string(),
        }
    }

    /// Resolve one step's name now.
    pub fn step_name(&self, step: TransformStep) -> String {
        match &self.steps[step.index()] {
            Some(p) => p.get(),
            None => step.default_label().to_string(),
        }
    }

    /// Register every label that is actually bound, so a reactive one reaches
    /// the published tree.
    ///
    /// The names are resolved at **walk** time, which is what keeps a `tr!`
    /// label locale-reactive without a rebuild — but only because a locale
    /// switch dirties the accessibility tree by itself. A plain
    /// `Signal<String>` has nothing doing that for it, so without this a
    /// renamed handle would go on announcing its old name until something else
    /// happened to dirty the tree.
    pub(crate) fn register_bindings(
        &self,
        widget_id: WidgetId,
        registry: &teksilo_core::binding::BindingRegistry,
        level: teksilo_core::binding::BindingLevel,
    ) {
        self.frame.register_if_bound(widget_id, registry, level);
        for label in self.handles.iter().chain(self.steps.iter()).flatten() {
            label.register_if_bound(widget_id, registry, level);
        }
    }
}

/// Per-view transform-controller configuration, installed via
/// [`SceneView::transform_controller`](crate::SceneView::transform_controller).
///
/// Built the way [`MagnetismConfig`](crate::MagnetismConfig) is: `Rc`'d
/// closures so the view can clone it into every handler, `Prop`-accepting
/// reactive knobs, one `enabled` signal.
#[derive(Clone)]
pub struct TransformConfig {
    pub(crate) handles: TransformHandleSet,
    pub(crate) keep_ratio: Prop<bool>,
    pub(crate) centered_scaling: Prop<bool>,
    pub(crate) rotation_snaps: Rc<[f32]>,
    pub(crate) rotation_snap_tolerance: f32,
    pub(crate) padding_px: f32,
    pub(crate) handle_px: f32,
    pub(crate) min_size: (f32, f32),
    pub(crate) enabled: Signal<bool>,
    pub(crate) body_drag: bool,
    pub(crate) live_preview: LivePreview,
    pub(crate) transform_key: Key,
    pub(crate) edge_pan_px: f32,
    pub(crate) edge_pan_speed: f32,
    pub(crate) labels: TransformLabels,
    pub(crate) on_start: Option<Rc<dyn Fn(&TransformSession, &mut EventContext)>>,
    pub(crate) on_change: Option<Rc<dyn Fn(&TransformSession, &mut EventContext)>>,
    pub(crate) on_end: Option<Rc<dyn Fn(&TransformSession, TransformOutcome, &mut EventContext)>>,
    pub(crate) chrome: Option<Rc<dyn Fn(&mut Canvas, &PaintContext, &TransformChrome)>>,
}

impl std::fmt::Debug for TransformConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransformConfig")
            .field("handles", &self.handles)
            .field("keep_ratio", &self.keep_ratio.get())
            .field("centered_scaling", &self.centered_scaling.get())
            .field("rotation_snaps", &self.rotation_snaps.len())
            .field("padding_px", &self.padding_px)
            .field("handle_px", &self.handle_px)
            .field("min_size", &self.min_size)
            .field("body_drag", &self.body_drag)
            .field("live_preview", &self.live_preview)
            .field("edge_pan_px", &self.edge_pan_px)
            .field("has_custom_chrome", &self.chrome.is_some())
            .field("enabled", &self.enabled.get())
            .finish()
    }
}

impl Default for TransformConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Default frame padding, in screen pixels. Chosen so the handle discs clear
/// the selection's own bounds: a card never owns the pixels a handle is drawn
/// on, which is what makes the chrome grabbable over a heavyweight child.
pub const DEFAULT_PADDING_PX: f32 = 6.0;
/// Default handle size, in screen pixels.
pub const DEFAULT_HANDLE_PX: f32 = 9.0;
/// How far above the frame the rotate puck floats, in screen pixels.
pub const ROTATE_OFFSET_PX: f32 = 22.0;
/// Default distance from the viewport edge at which a pointer gesture starts
/// panning the view, in screen pixels.
pub const DEFAULT_EDGE_PAN_PX: f32 = 28.0;
/// Default auto-pan speed, in screen pixels per second.
pub const DEFAULT_EDGE_PAN_SPEED: f32 = 700.0;

impl TransformConfig {
    /// A controller with every handle, no ratio lock, corner-anchored scaling,
    /// 6 px padding, 9 px handles, a 4 × 4 minimum, body drag on, live preview,
    /// `t` for the keyboard route and edge auto-pan on.
    pub fn new() -> Self {
        Self {
            handles: TransformHandleSet::ALL,
            keep_ratio: Prop::Static(false),
            centered_scaling: Prop::Static(false),
            rotation_snaps: Rc::from(Vec::new()),
            rotation_snap_tolerance: 5.0_f32.to_radians(),
            padding_px: DEFAULT_PADDING_PX,
            handle_px: DEFAULT_HANDLE_PX,
            min_size: (4.0, 4.0),
            enabled: Signal::new(true),
            body_drag: true,
            live_preview: LivePreview::Live,
            transform_key: Key::Character('t'),
            edge_pan_px: DEFAULT_EDGE_PAN_PX,
            edge_pan_speed: DEFAULT_EDGE_PAN_SPEED,
            labels: TransformLabels::new(),
            on_start: None,
            on_change: None,
            on_end: None,
            chrome: None,
        }
    }

    /// Which handles the frame offers. Konva's `enabledAnchors`.
    ///
    /// A handle in the set is still hidden when the selection cannot honour
    /// it — see
    /// [`SceneView::transform_controller`](crate::SceneView::transform_controller)
    /// for the rule. What is drawn is exactly what will happen.
    pub fn handles(mut self, set: TransformHandleSet) -> Self {
        self.handles = set;
        self
    }

    /// Keep the selection's aspect ratio through a resize. Konva's `keepRatio`.
    pub fn keep_ratio(mut self, on: impl Into<Prop<bool>>) -> Self {
        self.keep_ratio = on.into();
        self
    }

    /// Scale about the frame's centre rather than the opposite anchor.
    /// Konva's `centeredScaling`.
    pub fn centered_scaling(mut self, on: impl Into<Prop<bool>>) -> Self {
        self.centered_scaling = on.into();
        self
    }

    /// Absolute angles, in radians, a rotation snaps to when it comes within
    /// [`rotation_snap_tolerance`](Self::rotation_snap_tolerance).
    pub fn rotation_snaps(mut self, radians: impl IntoIterator<Item = f32>) -> Self {
        self.rotation_snaps = Rc::from(radians.into_iter().collect::<Vec<_>>());
        self
    }

    /// How close a rotation has to get to a snap before it takes it. Default 5°.
    pub fn rotation_snap_tolerance(mut self, radians: f32) -> Self {
        self.rotation_snap_tolerance = radians.max(0.0);
        self
    }

    /// How far outside the selection the frame is drawn, in screen pixels.
    /// Konva's `padding`.
    ///
    /// This is load-bearing, not decoration: it is what puts the frame band and
    /// the handles on pixels a heavyweight card does not own, and therefore
    /// what makes them grabbable over one. Setting it below
    /// `handle_px / 2` lets a handle overlap a card, and a card that claims its
    /// own press will then take the grab.
    pub fn padding(mut self, screen_px: f32) -> Self {
        self.padding_px = screen_px.max(0.0);
        self
    }

    /// The on-screen size of a handle, in pixels. Default 9.
    pub fn handle_px(mut self, px: f32) -> Self {
        self.handle_px = px.max(1.0);
        self
    }

    /// The smallest frame a resize will produce, in scene units.
    pub fn min_size(mut self, w: f32, h: f32) -> Self {
        self.min_size = (w.max(0.01), h.max(0.01));
        self
    }

    /// Whether a press on the body of a selected, movable item starts a group
    /// move. Default on.
    ///
    /// Best-effort over the heavyweight tier by construction: a card that
    /// claims its own press — one carrying `capture_pointer` or its own
    /// `on_drag` — wins it, and this view never sees the gesture. The frame
    /// band is the route that always works.
    pub fn body_drag(mut self, on: bool) -> Self {
        self.body_drag = on;
        self
    }

    /// Whether the items follow the gesture or only the frame does.
    pub fn live_preview(mut self, mode: LivePreview) -> Self {
        self.live_preview = mode;
        self
    }

    /// The key that enters the keyboard transform mode while the view is
    /// focused. Default `t`, mirroring magnetism's `m`.
    pub fn transform_key(mut self, key: Key) -> Self {
        self.transform_key = key;
        self
    }

    /// How close to the viewport edge a pointer gesture has to get before the
    /// view starts panning, in screen pixels. Zero turns auto-pan off.
    pub fn edge_pan_px(mut self, px: f32) -> Self {
        self.edge_pan_px = px.max(0.0);
        self
    }

    /// Auto-pan speed, in screen pixels per second.
    pub fn edge_pan_speed(mut self, px_per_second: f32) -> Self {
        self.edge_pan_speed = px_per_second.max(1.0);
        self
    }

    /// The names the frame and its handles publish to assistive technology.
    pub fn labels(mut self, labels: TransformLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Set the enabled state, statically or reactively.
    pub fn enabled(mut self, on: impl Into<Prop<bool>>) -> Self {
        self.enabled = on.into().as_signal();
        self
    }

    /// The reactive enabled signal, for a toolbar to read or bind.
    pub fn enabled_signal(&self) -> Signal<bool> {
        self.enabled.clone()
    }

    /// Whether the controller is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.get()
    }

    /// Fired once when a gesture begins.
    pub fn on_start(mut self, f: impl Fn(&TransformSession, &mut EventContext) + 'static) -> Self {
        self.on_start = Some(Rc::new(f));
        self
    }

    /// Fired on every pointer sample and every keyboard step.
    pub fn on_change(mut self, f: impl Fn(&TransformSession, &mut EventContext) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// Fired once when a gesture finishes, committed or cancelled.
    ///
    /// The scene has already been written when the outcome is
    /// [`TransformOutcome::Committed`], so this is the hook an app records the
    /// delta from — a reversible-edit layer lives above this crate, never
    /// inside it.
    pub fn on_end(
        mut self,
        f: impl Fn(&TransformSession, TransformOutcome, &mut EventContext) + 'static,
    ) -> Self {
        self.on_end = Some(Rc::new(f));
        self
    }

    /// Replace the built-in chrome painter. Paints in scene coordinates.
    pub fn chrome(
        mut self,
        f: impl Fn(&mut Canvas, &PaintContext, &TransformChrome) + 'static,
    ) -> Self {
        self.chrome = Some(Rc::new(f));
        self
    }

    /// Whether the handle is offered at all by this configuration.
    pub(crate) fn offers(&self, handle: TransformHandle) -> bool {
        self.handles.contains(handle)
    }
}

/// The live, per-view state of one gesture.
///
/// Deliberately not a snapshot of the numbers: it stores the **screen** point
/// the pointer is at and re-projects it through the live view transform every
/// time the delta is read. That is what makes edge auto-pan fall out — the view
/// slides under a stationary pointer and the selection keeps following the
/// scene point beneath it — and it is why nothing here needs a frame tick of
/// its own.
#[derive(Debug, Clone)]
pub(crate) struct LiveSession {
    pub roots: Rc<[ItemId]>,
    pub handle: TransformHandle,
    pub start_frame: TransformFrame,
    /// Scene point the gesture was grabbed at. Fixed for the gesture's life.
    pub anchor_scene: Point,
    /// Screen point the pointer is at now. `None` for a keyboard session.
    pub current_screen: Option<Point>,
    /// Accumulated keyboard step, in the frame's basis. Ignored for a pointer
    /// session.
    pub keyboard_step: Vec2,
    /// Accumulated keyboard rotation, in radians.
    pub keyboard_rotation: f32,
    pub source: TransformSource,
    /// Whether any root is aspect-locked, resolved once at grab time.
    pub aspect_locked: bool,
}

/// The two numbers a resolved session yields, always in agreement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ResolvedSession {
    pub frame: TransformFrame,
    pub delta: TransformDelta,
}

impl LiveSession {
    /// Resolve the gesture into a frame and a delta, applying the config's
    /// constraints and the scene's geometry constraint.
    ///
    /// `view_scale` converts the screen-pixel padding into scene units;
    /// `to_scene` projects the live pointer position; `constraint` is the
    /// scene's [`ProposedChange`](crate::ProposedChange) hook, already bundled
    /// with the shared scene borrow it reads (`None` when none is installed,
    /// which is why an unconstrained scene pays one `Option` test here).
    ///
    /// **Deterministic and stateless**, which is what keeps the preview and the
    /// commit in agreement: the constraint sees the same proposal on the
    /// release sample as on the one before it, so the frame the chrome last
    /// drew is the frame that gets written. There is no phase to branch on and
    /// deliberately so — a hook that could snap loosely while dragging and hard
    /// on release would be a hook that guarantees a jump at the release.
    pub fn resolve(
        &self,
        cfg: &TransformConfig,
        view_scale: f32,
        to_scene: impl Fn(Point) -> Point,
        constraint: Option<&crate::constrain::ConstraintCall<'_>>,
    ) -> ResolvedSession {
        let pad = cfg.padding_px / view_scale.max(1e-3);
        let current_scene = match self.current_screen {
            Some(p) => to_scene(p),
            None => self.anchor_scene,
        };
        let mut delta = self.raw_delta(cfg, pad, current_scene);
        let mut frame = delta.frame_after(&self.start_frame);
        if let Some(call) = constraint {
            let adjusted = call.frame(
                self.handle.op(),
                &self.roots,
                &self.start_frame,
                frame,
                self.source,
                // Magnetism does not run on the transform-controller route; see
                // `ProposedChange::magnet_snapped`.
                false,
            );
            if adjusted != frame {
                frame = adjusted;
                delta = TransformDelta::between(&self.start_frame, &frame, delta.pivot);
            }
        }
        ResolvedSession { frame, delta }
    }

    /// The delta before the geometry constraint sees it.
    fn raw_delta(&self, cfg: &TransformConfig, pad: f32, current_scene: Point) -> TransformDelta {
        let basis = self.start_frame.rotation;
        let centre = self.start_frame.centre_scene();
        match self.handle {
            TransformHandle::Move => {
                let t = match self.source {
                    TransformSource::Pointer => Vec2::new(
                        current_scene.x - self.anchor_scene.x,
                        current_scene.y - self.anchor_scene.y,
                    ),
                    // A keyboard step is stated in the frame's basis, so it
                    // moves the selection along its own axes.
                    TransformSource::Keyboard => {
                        let p = Transform2D::rotate(basis)
                            .apply_point(Point::new(self.keyboard_step.x, self.keyboard_step.y));
                        Vec2::new(p.x, p.y)
                    }
                };
                TransformDelta {
                    pivot: centre,
                    basis,
                    scale: Vec2::new(1.0, 1.0),
                    rotation: 0.0,
                    translation: t,
                }
            }
            TransformHandle::Rotate => {
                let raw = match self.source {
                    TransformSource::Pointer => {
                        let a = angle_of(self.anchor_scene, centre);
                        let b = angle_of(current_scene, centre);
                        normalize_angle(b - a)
                    }
                    TransformSource::Keyboard => self.keyboard_rotation,
                };
                let snapped = snap_rotation(cfg, basis, raw);
                TransformDelta {
                    pivot: centre,
                    basis,
                    scale: Vec2::new(1.0, 1.0),
                    rotation: snapped,
                    translation: Vec2::ZERO,
                }
            }
            h => self.resize_delta(cfg, pad, current_scene, h),
        }
    }

    fn resize_delta(
        &self,
        cfg: &TransformConfig,
        pad: f32,
        current_scene: Point,
        handle: TransformHandle,
    ) -> TransformDelta {
        let basis = self.start_frame.rotation;
        let c0 = self.start_frame.rect;
        let to_frame = Transform2D::rotate(-basis);
        // The pointer's travel, expressed along the frame's own axes.
        let d = match self.source {
            TransformSource::Pointer => {
                let a = to_frame.apply_point(self.anchor_scene);
                let b = to_frame.apply_point(current_scene);
                Vec2::new(b.x - a.x, b.y - a.y)
            }
            TransformSource::Keyboard => self.keyboard_step,
        };
        let centered = cfg.centered_scaling.get();
        let x_role = handle.x_role();
        let y_role = handle.y_role();

        // The pivot: the frame's centre under centred scaling, otherwise the
        // side opposite the one being dragged. The padding does not enter here
        // — the pointer is grabbing a padded corner, but what is being scaled
        // is the content box, and both corners carry the same padding so the
        // offset cancels.
        let pivot_f = Point::new(
            match (centered, x_role) {
                (true, _) | (_, None) => c0.x + c0.width * 0.5,
                (false, Some(SideRole::Min)) => c0.right(),
                (false, Some(SideRole::Max)) => c0.x,
            },
            match (centered, y_role) {
                (true, _) | (_, None) => c0.y + c0.height * 0.5,
                (false, Some(SideRole::Min)) => c0.bottom(),
                (false, Some(SideRole::Max)) => c0.y,
            },
        );
        let _ = pad;

        let mut sx = axis_scale(x_role, c0.x, c0.right(), pivot_f.x, d.x);
        let mut sy = axis_scale(y_role, c0.y, c0.bottom(), pivot_f.y, d.y);

        if cfg.keep_ratio.get() || self.aspect_locked {
            // Whichever axis the pointer actually pushed hardest decides, and
            // the other follows. An edge handle drives both axes, which is
            // Konva's behaviour and the only thing that keeps the ratio.
            let s = match (x_role, y_role) {
                (Some(_), Some(_)) => {
                    if d.x.abs() >= d.y.abs() {
                        sx
                    } else {
                        sy
                    }
                }
                (Some(_), None) => sx,
                (None, Some(_)) => sy,
                (None, None) => 1.0,
            };
            sx = s;
            sy = s;
        }

        // The floor. A resize stops here rather than mirroring: `Rect::contains`
        // is false for every point of a negative-extent rectangle, so a flipped
        // box would be silently un-hit-testable and un-indexable.
        //
        // It applies **only to an axis this handle drives**. Applied to both
        // unconditionally it inflated the other one: an item already thinner
        // than `min_size` has a floor scale above 1 on that axis, so dragging
        // the `Bottom` edge of a 2 x 100 hairline widened it to 4 — a
        // horizontal change from a purely vertical gesture, on the first sample,
        // silently. `axis_scale` returns exactly 1.0 for an axis with no role,
        // so "does this handle drive it" is `role.is_some()` and nothing more.
        let min_sx = if c0.width.abs() > 1e-6 {
            cfg.min_size.0 / c0.width
        } else {
            1.0
        };
        let min_sy = if c0.height.abs() > 1e-6 {
            cfg.min_size.1 / c0.height
        } else {
            1.0
        };
        if cfg.keep_ratio.get() || self.aspect_locked {
            // One scale drives both axes here, so both floors bear on it and
            // the tighter wins. Clamping the two apart is the ratio lock
            // quietly coming undone at the minimum size.
            let floor = min_sx.max(min_sy);
            if x_role.is_some() || y_role.is_some() {
                sx = sx.max(floor);
                sy = sy.max(floor);
            }
        } else {
            if x_role.is_some() {
                sx = sx.max(min_sx);
            }
            if y_role.is_some() {
                sy = sy.max(min_sy);
            }
        }

        TransformDelta {
            pivot: self.start_frame.to_scene(pivot_f),
            basis,
            scale: Vec2::new(sx, sy),
            rotation: 0.0,
            translation: Vec2::ZERO,
        }
    }

    /// Build the public snapshot from a resolution.
    pub fn snapshot(&self, resolved: ResolvedSession) -> TransformSession {
        TransformSession {
            items: self.roots.clone(),
            handle: self.handle,
            start_frame: self.start_frame,
            frame: resolved.frame,
            delta: resolved.delta,
            source: self.source,
        }
    }
}

/// How far one side moves as a multiple of its distance from the pivot.
fn axis_scale(role: Option<SideRole>, min: f32, max: f32, pivot: f32, d: f32) -> f32 {
    let (side, sign) = match role {
        None => return 1.0,
        Some(SideRole::Min) => (min, 1.0),
        Some(SideRole::Max) => (max, 1.0),
    };
    let old = side - pivot;
    if old.abs() < 1e-6 {
        return 1.0;
    }
    let new = old + d * sign;
    new / old
}

fn angle_of(p: Point, centre: Point) -> f32 {
    (p.y - centre.y).atan2(p.x - centre.x)
}

fn normalize_angle(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut r = a % tau;
    if r > std::f32::consts::PI {
        r -= tau;
    } else if r < -std::f32::consts::PI {
        r += tau;
    }
    r
}

/// Pull an additive rotation onto the nearest configured absolute snap, when it
/// is within tolerance of one.
fn snap_rotation(cfg: &TransformConfig, basis: f32, raw: f32) -> f32 {
    if cfg.rotation_snaps.is_empty() {
        return raw;
    }
    let absolute = basis + raw;
    let mut best: Option<(f32, f32)> = None;
    for snap in cfg.rotation_snaps.iter().copied() {
        let diff = normalize_angle(snap - absolute).abs();
        if diff <= cfg.rotation_snap_tolerance && best.is_none_or(|(_, bd)| diff < bd) {
            best = Some((snap, diff));
        }
    }
    match best {
        Some((snap, _)) => snap - basis,
        None => raw,
    }
}

/// The selection resolved for the chrome: the roots the frame encloses, the
/// frame itself, and the handles that frame may offer.
///
/// One value because those are one question, asked together by everything that
/// draws, grabs or announces the frame. Asking them one at a time cost the
/// **free hover** - every `PointerMove` an application makes over the view -
/// thirteen descendant-pruning passes over the selection per sample: one for
/// the frame, six for the handle set, six more for the second handle set the
/// band test needed. Each pass is `Scene::selection_roots`, so on a large
/// selection the hover was the single most expensive thing a `SceneView` did,
/// and no functional test could see it.
#[derive(Debug, Clone)]
pub(crate) struct SelectionChrome {
    /// The box the selection's roots make. Not flag-filtered: the frame shows
    /// what is *selected*, and each operation then decides which of those it
    /// can honour.
    pub frame: TransformFrame,
    /// The handles this frame offers: configured **and** honourable.
    pub handles: Rc<[TransformHandle]>,
    /// Whether the selection can honour each operation at all, in
    /// [`HANDLE_ORDER`]'s own op vocabulary: move, resize, rotate.
    ///
    /// Kept beside `handles` rather than derived from it, because the two
    /// answer different questions: a config may decline to *offer* a resize
    /// handle on a selection that would happily resize.
    pub supports_move: bool,
    /// See [`supports_move`](Self::supports_move).
    pub supports_resize: bool,
    /// See [`supports_move`](Self::supports_move).
    pub supports_rotate: bool,
}

/// What a cached [`SelectionChrome`] was computed against.
///
/// Three counters, and between them they cover everything the answer reads.
/// `model` is [`Scene::item_change_version`](crate::Scene), which every
/// geometry, flag and structure mutation advances **under the exclusive borrow
/// that made it** - so a value read under a shared borrow describes the model
/// as it stands, whether or not the matching notifications have fanned out yet.
/// The other two are signal write generations, monotone by contract. The
/// remaining inputs - which handles the config offers, its padding - are fixed
/// at build time.
///
/// Nothing invalidates this by hand. A memo with an invalidation call site is a
/// memo with a missed invalidation call site; this one is simply wrong-keyed or
/// right-keyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChromeKey {
    /// `Scene::item_change_version`.
    pub model: u64,
    /// The selection signal's write generation.
    pub selection: u64,
    /// The `enabled` signal's write generation.
    pub enabled: u64,
}

/// The view-side runtime state of the controller, kept out of `SceneView`'s
/// field list so the controller can be reasoned about as one thing.
pub(crate) struct TransformRuntime {
    /// The live gesture, if one is running.
    pub session: std::cell::RefCell<Option<LiveSession>>,
    /// Posted at the end of a committed gesture, drained by `build()` where
    /// `&mut Scene` is reachable.
    pub pending_commit: std::cell::RefCell<Option<(Vec<ItemId>, TransformDelta)>>,
    /// Whether the keyboard transform mode is active.
    pub keyboard_mode: Cell<bool>,
    /// The handle the keyboard mode has roving focus on.
    pub keyboard_focus: Cell<Option<TransformHandle>>,
    /// Whether an edge auto-pan tween is this controller's to stop.
    ///
    /// Without it the "stop panning" path would touch `pan_x` / `pan_y` on
    /// every sample of every gesture — and those are bound at `Relayout`, so a
    /// controller with auto-pan configured but never triggered would schedule a
    /// relayout each sample for nothing, and worse, would make the one that
    /// *should* come from the controller's own tick indistinguishable from it.
    pub edge_panning: Cell<bool>,
    /// The handle under the pointer, for the chrome and the cursor.
    pub hovered: Cell<Option<TransformHandle>>,
    /// Where focus was when a **pointer** gesture took it, so the gesture can
    /// give it back.
    ///
    /// The steal is not cosmetic — `Esc` is delivered to the focused widget, so
    /// without it the cancel route would be unreachable for the whole of a
    /// mouse or touch gesture. But a caret in a card's `TextInput` is the user's
    /// place in their own work, and a drag is not a request to leave it. Set
    /// only for [`TransformSource::Pointer`]; a keyboard or AT route already
    /// has the focus it needs.
    pub focus_before: Cell<Option<WidgetId>>,
    /// Bumped on every session change; bound at `BindingLevel::Relayout` so
    /// both tiers re-derive their preview from the same value.
    ///
    /// This is the trigger the design needs and a plain `Cell` cannot be:
    /// `place_children` re-runs on a relayout, and the heavyweight tier's
    /// preview lives there. Without it the frame and the lightweight items
    /// would follow the pointer while the cards sat still until the commit.
    pub tick: Signal<u64>,
    /// Bumped only on discrete steps — session start, session end, keyboard
    /// roving — and bound at `BindingLevel::AccessibilityOnly`.
    ///
    /// Deliberately **not** bumped per pointer sample: a full accessibility
    /// re-walk at pointer rate would cost the live set on every sample to move
    /// ten nodes, and no screen-reader user is driving a pointer drag. The
    /// commit's own model change re-walks at the end.
    pub at_tick: Signal<u64>,
    /// The public session signal.
    pub published: Signal<Option<TransformSession>>,
    /// The last [`SelectionChrome`] and the [`ChromeKey`] it was computed
    /// against. `Some(key, None)` memoises "this selection has no frame",
    /// which is the commonest state of all - nothing selected - and the one a
    /// naive memo would recompute on every sample.
    pub chrome_memo: std::cell::RefCell<Option<(ChromeKey, Option<SelectionChrome>)>>,
    /// How many times the chrome was actually *computed* rather than reused.
    ///
    /// The memo's own witness, and the only thing that can tell a working memo
    /// from one whose key is stale on every read. A hover costs no model work
    /// at all now, so nothing else a test can observe changes when the memo is
    /// deleted.
    #[cfg(test)]
    pub chrome_computes: Cell<u64>,
}

impl std::fmt::Debug for TransformRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransformRuntime")
            .field("has_session", &self.session.borrow().is_some())
            .field("keyboard_mode", &self.keyboard_mode.get())
            .field("keyboard_focus", &self.keyboard_focus.get())
            .finish()
    }
}

impl Default for TransformRuntime {
    fn default() -> Self {
        Self {
            session: std::cell::RefCell::new(None),
            pending_commit: std::cell::RefCell::new(None),
            keyboard_mode: Cell::new(false),
            keyboard_focus: Cell::new(None),
            edge_panning: Cell::new(false),
            hovered: Cell::new(None),
            focus_before: Cell::new(None),
            tick: Signal::new(0),
            at_tick: Signal::new(0),
            published: Signal::new(None),
            chrome_memo: std::cell::RefCell::new(None),
            #[cfg(test)]
            chrome_computes: Cell::new(0),
        }
    }
}

impl TransformRuntime {
    /// Drop the live gesture and repaint. There is nothing to roll back — that
    /// is the whole point of the model-free session.
    ///
    /// Returns the session that was dropped, if any.
    pub fn abort(&self) -> Option<LiveSession> {
        let had = self.session.borrow_mut().take();
        if had.is_some() {
            self.published.set(None);
            self.bump_all();
        }
        had
    }

    pub fn bump(&self) {
        self.tick.set(self.tick.get().wrapping_add(1));
    }

    pub fn bump_all(&self) {
        self.bump();
        self.at_tick.set(self.at_tick.get().wrapping_add(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(x: f32, y: f32, w: f32, h: f32) -> TransformFrame {
        TransformFrame {
            rect: Rect::new(x, y, w, h),
            rotation: 0.0,
            count: 1,
        }
    }

    #[test]
    fn identity_delta_is_identity() {
        assert!(TransformDelta::IDENTITY.is_identity());
        assert!(TransformDelta::IDENTITY.to_scene_transform().is_identity());
    }

    #[test]
    fn a_translation_moves_a_point_by_itself() {
        let d = TransformDelta {
            translation: Vec2::new(10.0, -4.0),
            ..TransformDelta::IDENTITY
        };
        let p = d.to_scene_transform().apply_point(Point::new(1.0, 1.0));
        assert!((p.x - 11.0).abs() < 1e-4 && (p.y + 3.0).abs() < 1e-4);
    }

    #[test]
    fn a_scale_about_a_pivot_leaves_the_pivot_alone() {
        let d = TransformDelta {
            pivot: Point::new(5.0, 5.0),
            scale: Vec2::new(2.0, 3.0),
            ..TransformDelta::IDENTITY
        };
        let t = d.to_scene_transform();
        let p = t.apply_point(Point::new(5.0, 5.0));
        assert!((p.x - 5.0).abs() < 1e-4 && (p.y - 5.0).abs() < 1e-4);
        let q = t.apply_point(Point::new(7.0, 6.0));
        assert!((q.x - 9.0).abs() < 1e-4, "{q:?}");
        assert!((q.y - 8.0).abs() < 1e-4, "{q:?}");
    }

    #[test]
    fn scale_is_taken_along_the_frames_own_axes() {
        // A frame rotated 90°: scaling its "width" must stretch along scene y.
        let d = TransformDelta {
            pivot: Point::ZERO,
            basis: std::f32::consts::FRAC_PI_2,
            scale: Vec2::new(2.0, 1.0),
            ..TransformDelta::IDENTITY
        };
        let p = d.to_scene_transform().apply_point(Point::new(0.0, 10.0));
        assert!(p.x.abs() < 1e-3, "{p:?}");
        assert!((p.y - 20.0).abs() < 1e-3, "{p:?}");
    }

    #[test]
    fn frame_after_and_between_round_trip() {
        let start = frame(10.0, 20.0, 100.0, 50.0);
        let d = TransformDelta {
            pivot: Point::new(10.0, 20.0),
            basis: 0.0,
            scale: Vec2::new(1.5, 0.5),
            rotation: 0.0,
            translation: Vec2::new(7.0, -3.0),
        };
        let end = d.frame_after(&start);
        let back = TransformDelta::between(&start, &end, d.pivot);
        let end2 = back.frame_after(&start);
        assert!((end2.rect.x - end.rect.x).abs() < 1e-3, "{end2:?} {end:?}");
        assert!((end2.rect.y - end.rect.y).abs() < 1e-3);
        assert!((end2.rect.width - end.rect.width).abs() < 1e-3);
        assert!((end2.rect.height - end.rect.height).abs() < 1e-3);
    }

    #[test]
    fn round_trip_holds_for_a_rotated_frame() {
        let start = TransformFrame {
            rect: Rect::new(-50.0, -25.0, 100.0, 50.0),
            rotation: 0.6,
            count: 1,
        };
        let d = TransformDelta {
            pivot: Point::new(3.0, -2.0),
            basis: 0.6,
            scale: Vec2::new(1.25, 0.8),
            rotation: 0.0,
            translation: Vec2::new(4.0, 9.0),
        };
        let end = d.frame_after(&start);
        let back = TransformDelta::between(&start, &end, d.pivot);
        let end2 = back.frame_after(&start);
        assert!((end2.rect.x - end.rect.x).abs() < 1e-2, "{end2:?} {end:?}");
        assert!((end2.rect.y - end.rect.y).abs() < 1e-2);
        assert!((end2.rect.width - end.rect.width).abs() < 1e-2);
        assert!((end2.rect.height - end.rect.height).abs() < 1e-2);
    }

    fn session(handle: TransformHandle, start: TransformFrame, anchor: Point) -> LiveSession {
        LiveSession {
            roots: Rc::from(Vec::new()),
            handle,
            start_frame: start,
            anchor_scene: anchor,
            current_screen: Some(anchor),
            keyboard_step: Vec2::ZERO,
            keyboard_rotation: 0.0,
            source: TransformSource::Pointer,
            aspect_locked: false,
        }
    }

    #[test]
    fn dragging_the_bottom_trailing_corner_scales_from_the_top_leading_one() {
        let cfg = TransformConfig::new();
        let start = frame(0.0, 0.0, 100.0, 100.0);
        let mut s = session(
            TransformHandle::BottomTrailing,
            start,
            Point::new(100.0, 100.0),
        );
        s.current_screen = Some(Point::new(150.0, 200.0));
        let r = s.resolve(&cfg, 1.0, |p| p, None);
        assert!((r.delta.scale.x - 1.5).abs() < 1e-4, "{:?}", r.delta);
        assert!((r.delta.scale.y - 2.0).abs() < 1e-4, "{:?}", r.delta);
        assert!((r.delta.pivot.x).abs() < 1e-4 && (r.delta.pivot.y).abs() < 1e-4);
        assert!((r.frame.rect.width - 150.0).abs() < 1e-3);
        assert!((r.frame.rect.height - 200.0).abs() < 1e-3);
        assert!(r.frame.rect.x.abs() < 1e-3 && r.frame.rect.y.abs() < 1e-3);
    }

    #[test]
    fn dragging_the_leading_edge_holds_the_trailing_one() {
        let cfg = TransformConfig::new();
        let start = frame(0.0, 0.0, 100.0, 100.0);
        let mut s = session(TransformHandle::Leading, start, Point::new(0.0, 50.0));
        s.current_screen = Some(Point::new(40.0, 50.0));
        let r = s.resolve(&cfg, 1.0, |p| p, None);
        assert!((r.frame.rect.x - 40.0).abs() < 1e-3, "{:?}", r.frame);
        assert!((r.frame.rect.right() - 100.0).abs() < 1e-3, "{:?}", r.frame);
        assert!((r.frame.rect.height - 100.0).abs() < 1e-3);
    }

    #[test]
    fn a_resize_stops_at_min_size_instead_of_mirroring() {
        let cfg = TransformConfig::new().min_size(10.0, 10.0);
        let start = frame(0.0, 0.0, 100.0, 100.0);
        let mut s = session(
            TransformHandle::BottomTrailing,
            start,
            Point::new(100.0, 100.0),
        );
        // Dragged far past the opposite corner.
        s.current_screen = Some(Point::new(-400.0, -400.0));
        let r = s.resolve(&cfg, 1.0, |p| p, None);
        assert!(r.frame.rect.width >= 10.0 - 1e-3, "{:?}", r.frame);
        assert!(r.frame.rect.height >= 10.0 - 1e-3);
        assert!(r.delta.scale.x > 0.0 && r.delta.scale.y > 0.0);
        assert!((r.frame.rect.width - 10.0).abs() < 1e-3);
    }

    #[test]
    fn a_min_size_never_inflates_an_axis_the_handle_does_not_drive() {
        // A 2 x 100 hairline under the default 4 x 4 floor. Its floor scale on
        // x is 2.0, and `Bottom` has no x role at all — so clamping both axes
        // unconditionally doubled the width on the first sample of a purely
        // vertical drag, silently, before the pointer had moved 20 units.
        let cfg = TransformConfig::new().min_size(4.0, 4.0);
        let start = frame(0.0, 0.0, 2.0, 100.0);
        let mut s = session(TransformHandle::Bottom, start, Point::new(1.0, 100.0));
        s.current_screen = Some(Point::new(1.0, 120.0));
        let r = s.resolve(&cfg, 1.0, |p| p, None);
        assert!(
            (r.delta.scale.x - 1.0).abs() < 1e-6,
            "a vertical handle must not scale x: {:?}",
            r.delta
        );
        assert!(
            (r.frame.rect.width - 2.0).abs() < 1e-3,
            "the width is the gesture's business only if the handle drives it: {:?}",
            r.frame
        );
        assert!((r.frame.rect.height - 120.0).abs() < 1e-3, "{:?}", r.frame);
    }

    #[test]
    fn the_min_size_floor_still_bites_on_the_axis_the_handle_drives() {
        // The other half of the same rule: the axis with a role is clamped,
        // including on an item that starts under the floor.
        let cfg = TransformConfig::new().min_size(4.0, 4.0);
        let start = frame(0.0, 0.0, 100.0, 2.0);
        let mut s = session(TransformHandle::Bottom, start, Point::new(50.0, 2.0));
        s.current_screen = Some(Point::new(50.0, -50.0));
        let r = s.resolve(&cfg, 1.0, |p| p, None);
        assert!(
            (r.frame.rect.height - 4.0).abs() < 1e-3,
            "a driven axis stops at the floor rather than mirroring: {:?}",
            r.frame
        );
        assert!((r.frame.rect.width - 100.0).abs() < 1e-3, "{:?}", r.frame);
    }

    #[test]
    fn a_ratio_locked_resize_takes_the_tighter_of_the_two_floors() {
        // Under a ratio lock one scale drives both axes, so both floors bear on
        // it and the tighter wins — clamping them apart is the lock coming
        // undone exactly at the minimum size.
        let cfg = TransformConfig::new().min_size(10.0, 10.0).keep_ratio(true);
        let start = frame(0.0, 0.0, 100.0, 50.0);
        let mut s = session(
            TransformHandle::BottomTrailing,
            start,
            Point::new(100.0, 50.0),
        );
        s.current_screen = Some(Point::new(-400.0, -400.0));
        let r = s.resolve(&cfg, 1.0, |p| p, None);
        assert!(
            (r.delta.scale.x - r.delta.scale.y).abs() < 1e-6,
            "the ratio survives the floor: {:?}",
            r.delta
        );
        assert!(
            r.frame.rect.width >= 10.0 - 1e-3 && r.frame.rect.height >= 10.0 - 1e-3,
            "{:?}",
            r.frame
        );
    }

    #[test]
    fn centered_scaling_holds_the_centre() {
        let cfg = TransformConfig::new().centered_scaling(true);
        let start = frame(0.0, 0.0, 100.0, 100.0);
        let mut s = session(TransformHandle::Trailing, start, Point::new(100.0, 50.0));
        s.current_screen = Some(Point::new(125.0, 50.0));
        let r = s.resolve(&cfg, 1.0, |p| p, None);
        let cx = r.frame.rect.x + r.frame.rect.width * 0.5;
        assert!((cx - 50.0).abs() < 1e-3, "{:?}", r.frame);
        assert!((r.frame.rect.width - 150.0).abs() < 1e-3, "{:?}", r.frame);
    }

    #[test]
    fn keep_ratio_drives_both_axes_from_one_edge() {
        let cfg = TransformConfig::new().keep_ratio(true);
        let start = frame(0.0, 0.0, 100.0, 50.0);
        let mut s = session(TransformHandle::Trailing, start, Point::new(100.0, 25.0));
        s.current_screen = Some(Point::new(150.0, 25.0));
        let r = s.resolve(&cfg, 1.0, |p| p, None);
        assert!((r.delta.scale.x - 1.5).abs() < 1e-4);
        assert!((r.delta.scale.y - 1.5).abs() < 1e-4);
    }

    #[test]
    fn rotation_snaps_only_within_tolerance() {
        let quarter = std::f32::consts::FRAC_PI_2;
        let cfg = TransformConfig::new()
            .rotation_snaps([0.0, quarter])
            .rotation_snap_tolerance(0.15);
        let start = frame(-50.0, -50.0, 100.0, 100.0);
        // Grab at 0 rad from the centre, release ~0.102 rad shy of 90°.
        let mut s = session(TransformHandle::Rotate, start, Point::new(50.0, 0.0));
        s.current_screen = Some(Point::new(5.0, 49.0));
        let r = s.resolve(&cfg, 1.0, |p| p, None);
        assert!(
            (r.delta.rotation - quarter).abs() < 1e-3,
            "should snap: {:?}",
            r.delta
        );

        let cfg_far = TransformConfig::new()
            .rotation_snaps([0.0, quarter])
            .rotation_snap_tolerance(0.001);
        let r2 = s.resolve(&cfg_far, 1.0, |p| p, None);
        assert!(
            (r2.delta.rotation - quarter).abs() > 1e-3,
            "should not snap: {:?}",
            r2.delta
        );
    }

    #[test]
    fn the_geometry_constraint_rewrites_the_delta_not_just_the_drawing() {
        let cfg = TransformConfig::new();
        let mut scene = crate::scene::Scene::new();
        scene.set_geometry_constraint(|c| {
            let mut f = c.proposed;
            f.rect.width = f.rect.width.min(120.0);
            crate::constrain::ChangeVerdict::Adjust(f)
        });
        let call = crate::constrain::ConstraintCall::new(&scene).expect("installed");
        let start = frame(0.0, 0.0, 100.0, 100.0);
        let mut s = session(
            TransformHandle::BottomTrailing,
            start,
            Point::new(100.0, 100.0),
        );
        s.current_screen = Some(Point::new(300.0, 100.0));
        let r = s.resolve(&cfg, 1.0, |p| p, Some(&call));
        assert!((r.frame.rect.width - 120.0).abs() < 1e-3, "{:?}", r.frame);
        // …and the delta agrees, which is what stops the chrome and the commit
        // from telling two different stories.
        let applied = r.delta.frame_after(&start);
        assert!((applied.rect.width - 120.0).abs() < 1e-3, "{applied:?}");
    }

    #[test]
    fn the_constraint_is_told_the_op_the_handle_performs() {
        use std::cell::RefCell;
        let seen: Rc<RefCell<Vec<TransformOp>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        let cfg = TransformConfig::new();
        let mut scene = crate::scene::Scene::new();
        scene.set_geometry_constraint(move |c| {
            sink.borrow_mut().push(c.op);
            crate::constrain::ChangeVerdict::Accept
        });
        let call = crate::constrain::ConstraintCall::new(&scene).expect("installed");
        let start = frame(0.0, 0.0, 100.0, 100.0);
        for (handle, expect) in [
            (TransformHandle::Move, TransformOp::Move),
            (TransformHandle::BottomTrailing, TransformOp::Resize),
            (TransformHandle::Rotate, TransformOp::Rotate),
        ] {
            let mut s = session(handle, start, Point::new(100.0, 100.0));
            s.current_screen = Some(Point::new(140.0, 160.0));
            let _ = s.resolve(&cfg, 1.0, |p| p, Some(&call));
            assert_eq!(seen.borrow().last().copied(), Some(expect), "{handle:?}");
        }
    }

    #[test]
    fn resolving_twice_gives_the_same_answer_so_preview_and_commit_agree() {
        let cfg = TransformConfig::new();
        let mut scene = crate::scene::Scene::new();
        // A constraint that would be tempted to behave differently over time.
        let calls = std::cell::Cell::new(0u32);
        scene.set_geometry_constraint(move |c| {
            calls.set(calls.get() + 1);
            let mut f = c.proposed;
            f.rect.x = (f.rect.x / 25.0).round() * 25.0;
            f.rect.y = (f.rect.y / 25.0).round() * 25.0;
            crate::constrain::ChangeVerdict::Adjust(f)
        });
        let call = crate::constrain::ConstraintCall::new(&scene).expect("installed");
        let start = frame(0.0, 0.0, 100.0, 100.0);
        let mut s = session(TransformHandle::Move, start, Point::ZERO);
        s.current_screen = Some(Point::new(37.0, 61.0));
        let preview = s.resolve(&cfg, 1.0, |p| p, Some(&call));
        let commit = s.resolve(&cfg, 1.0, |p| p, Some(&call));
        assert_eq!(preview.frame, commit.frame);
        assert_eq!(preview.delta, commit.delta);
        assert!(
            (commit.frame.rect.x - 25.0).abs() < 1e-3,
            "{:?}",
            commit.frame
        );
        assert!(
            (commit.frame.rect.y - 50.0).abs() < 1e-3,
            "{:?}",
            commit.frame
        );
    }

    #[test]
    fn a_rejected_sample_resolves_to_the_identity() {
        let cfg = TransformConfig::new();
        let mut scene = crate::scene::Scene::new();
        scene.set_geometry_constraint(|_| crate::constrain::ChangeVerdict::Reject);
        let call = crate::constrain::ConstraintCall::new(&scene).expect("installed");
        let start = frame(0.0, 0.0, 100.0, 100.0);
        let mut s = session(TransformHandle::Move, start, Point::ZERO);
        s.current_screen = Some(Point::new(400.0, 400.0));
        let r = s.resolve(&cfg, 1.0, |p| p, Some(&call));
        assert_eq!(r.frame, start);
        assert!(r.delta.is_identity(), "{:?}", r.delta);
    }

    #[test]
    fn a_handle_set_round_trips_its_bits() {
        let s = TransformHandleSet::CORNERS.with(TransformHandle::Rotate);
        assert!(s.contains(TransformHandle::TopLeading));
        assert!(s.contains(TransformHandle::Rotate));
        assert!(!s.contains(TransformHandle::Top));
        assert_eq!(TransformHandleSet::from_bits(s.bits()), s);
        assert!(
            !s.without(TransformHandle::Rotate)
                .contains(TransformHandle::Rotate)
        );
    }

    #[test]
    fn every_handle_has_a_distinct_index_and_a_name() {
        let mut seen = std::collections::HashSet::new();
        for h in HANDLE_ORDER {
            assert!(seen.insert(h.index()), "duplicate index for {h:?}");
            assert!(!h.default_label().is_empty());
        }
        assert_eq!(seen.len(), 10);
    }

    #[test]
    fn a_keyboard_move_steps_along_the_frames_axes() {
        let cfg = TransformConfig::new();
        let start = TransformFrame {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            rotation: std::f32::consts::FRAC_PI_2,
            count: 1,
        };
        let mut s = session(TransformHandle::Move, start, Point::ZERO);
        s.source = TransformSource::Keyboard;
        s.current_screen = None;
        s.keyboard_step = Vec2::new(5.0, 0.0);
        let r = s.resolve(&cfg, 1.0, |p| p, None);
        // The frame's x axis points along scene +y at 90°.
        assert!(r.delta.translation.x.abs() < 1e-3, "{:?}", r.delta);
        assert!((r.delta.translation.y - 5.0).abs() < 1e-3, "{:?}", r.delta);
    }
}
