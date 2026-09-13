// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Touch-action and pan-claim vocabulary: what a subtree permits a direct
//! pointer to do to it, and who along the ancestor chain wants to pan.
//!
//! Modelled on the CSS `touch-action` property: a node's own declaration is
//! intersected with every ancestor's on the way down, so an ancestor can only
//! ever *narrow* what a descendant permits, never widen it. [`TouchAction`] is
//! the declaration; [`PanClaim`] is a *separate* declaration — "I am a pan
//! surface" — that a scrollable makes independently of what `TouchAction` its
//! subtree allows. The two folds a consumer needs
//! (`WidgetTree::effective_touch_action` / `WidgetTree::pan_candidates`) live
//! in `widget_tree/pointer_state.rs`, next to the rest of the pointer/hover/
//! capture bookkeeping.
//!
//! Both are read at dispatch time. [`TouchAction`] gates the pan claimants a
//! press enrols (`WidgetTree::begin_sequence`) and whether a subtree admits a
//! two-contact pinch at all; [`PanClaim`] is what
//! `WidgetTree::pan_candidates` collects into the chain a synthesised pan is
//! delivered along. See `widget_tree::pan_arbiter`.
//!
//! # Mouse is unaffected
//!
//! A mouse never consults [`TouchAction`] at all — it has no contact patch to
//! restrict and it already scrolls with the wheel, not by dragging content.
//! [`PanClaim::devices`] defaults to [`PointerKindMask::DIRECT`], which
//! excludes [`teksilo_tokens::PointerKind::Mouse`] — this is the field that
//! keeps a mouse from ever being treated as a panning pointer, however a
//! widget declares its claim. See `docs/events-and-gestures.md` "Touch action
//! and pan claims".

use teksilo_tokens::PointerKindMask;

/// Which axis a pan or a touch-action permission is about.
///
/// `teksilo-core` has no existing public `Axis`/`Orientation` type that fits
/// here (`teksilo_tokens::Orientation` names a *widget* layout axis, and
/// `teksilo-widgets`' private `Axis` isn't reachable from this crate), so this
/// module declares its own.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Axis {
    /// The horizontal axis.
    X,
    /// The vertical axis.
    Y,
}

/// What a direct pointer (touch, pen) is permitted to do to a subtree,
/// declared per node and intersected down the tree — the CSS `touch-action`
/// model.
///
/// A bitset over three permissions (pan-x, pan-y, pinch-zoom), plus the two
/// absorbing/identity extremes [`AUTO`](Self::AUTO) (everything permitted —
/// the default) and [`NONE`](Self::NONE) (nothing permitted). `AUTO` occupies
/// every bit of the backing `u8`, not just the three named ones, so it stays
/// the identity element for [`intersect`](Self::intersect) even if a later
/// package adds a fourth permission bit: intersecting anything with a value
/// that has every bit set can never clear a bit the other side already had.
///
/// A mouse never reads this type at all — see the module docs.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct TouchAction(u8);

impl TouchAction {
    const PAN_X_BIT: u8 = 1 << 0;
    const PAN_Y_BIT: u8 = 1 << 1;
    const PINCH_ZOOM_BIT: u8 = 1 << 2;

    /// Every default touch behaviour is permitted. The identity element for
    /// [`intersect`](Self::intersect) and [`Default`].
    pub const AUTO: Self = Self(u8::MAX);
    /// No default touch behaviour is permitted — the subtree wants every
    /// contact reserved for its own gesture handling. The absorbing element
    /// for [`intersect`](Self::intersect): once any ancestor declares `NONE`,
    /// nothing below it can un-forbid anything.
    pub const NONE: Self = Self(0);
    /// Horizontal panning only.
    pub const PAN_X: Self = Self(Self::PAN_X_BIT);
    /// Vertical panning only.
    pub const PAN_Y: Self = Self(Self::PAN_Y_BIT);
    /// Panning on either axis (`PAN_X | PAN_Y`).
    pub const PAN: Self = Self(Self::PAN_X_BIT | Self::PAN_Y_BIT);
    /// Pinch-to-zoom only.
    pub const PINCH_ZOOM: Self = Self(Self::PINCH_ZOOM_BIT);
    /// Panning and pinch-zoom, but no other browser-style default gesture
    /// (`PAN | PINCH_ZOOM`).
    pub const MANIPULATION: Self = Self(Self::PAN_X_BIT | Self::PAN_Y_BIT | Self::PINCH_ZOOM_BIT);

    /// The permissions both sides agree on — bitwise AND. Associative,
    /// commutative, `AUTO` is the identity, `NONE` is absorbing (all tested
    /// by `intersect_forms_a_commutative_monoid_with_auto_and_none_at_its_poles`
    /// below).
    pub const fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// The permissions either side allows — bitwise OR.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether panning is permitted on `axis`.
    pub const fn allows_pan(self, axis: Axis) -> bool {
        let bit = match axis {
            Axis::X => Self::PAN_X_BIT,
            Axis::Y => Self::PAN_Y_BIT,
        };
        self.0 & bit != 0
    }

    /// Whether horizontal panning is permitted. Sugar for
    /// `allows_pan(Axis::X)`.
    pub const fn allows_pan_x(self) -> bool {
        self.allows_pan(Axis::X)
    }

    /// Whether vertical panning is permitted. Sugar for
    /// `allows_pan(Axis::Y)`.
    pub const fn allows_pan_y(self) -> bool {
        self.allows_pan(Axis::Y)
    }

    /// Whether pinch-to-zoom is permitted.
    pub const fn allows_pinch(self) -> bool {
        self.0 & Self::PINCH_ZOOM_BIT != 0
    }

    /// Whether any delayed gesture recognition (long-press, a slop-gated
    /// drag) may still run on this subtree — true unless the whole action is
    /// [`NONE`](Self::NONE). A subtree that reserves every touch behaviour for
    /// itself gets its response immediately, with no arbitration delay.
    pub const fn allows_delayed_gestures(self) -> bool {
        !self.is_none()
    }

    /// Whether this is [`NONE`](Self::NONE) — nothing permitted.
    pub const fn is_none(self) -> bool {
        self.0 == Self::NONE.0
    }
}

impl std::ops::BitOr for TouchAction {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitAnd for TouchAction {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        self.intersect(rhs)
    }
}

impl Default for TouchAction {
    /// [`Self::AUTO`] — a node that declares nothing permits everything,
    /// exactly like the CSS property it mirrors.
    fn default() -> Self {
        Self::AUTO
    }
}

/// Which axes a [`PanClaim`] wants to pan on.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct PanAxes(u8);

impl PanAxes {
    const X_BIT: u8 = 1 << 0;
    const Y_BIT: u8 = 1 << 1;

    /// Neither axis. The default.
    pub const NONE: Self = Self(0);
    /// Horizontal only.
    pub const X: Self = Self(Self::X_BIT);
    /// Vertical only.
    pub const Y: Self = Self(Self::Y_BIT);
    /// Both axes.
    pub const BOTH: Self = Self(Self::X_BIT | Self::Y_BIT);

    /// Whether `axis` is one of the claimed axes.
    pub const fn contains(self, axis: Axis) -> bool {
        let bit = match axis {
            Axis::X => Self::X_BIT,
            Axis::Y => Self::Y_BIT,
        };
        self.0 & bit != 0
    }
}

/// A node's declaration that it is a **pan surface**: it wants to consume a
/// direct pointer's drag as content panning rather than let it arm a drag/
/// swipe recognizer or fall through.
///
/// Declared independently of [`TouchAction`] — a scrollable states "I pan"
/// via `PanClaim` regardless of what its own `touch_action` permits;
/// `TouchAction` is what the *arbitration* consults to decide whether a claim
/// further down the chain is still reachable. See
/// `WidgetTree::pan_candidates`.
///
/// Declaring a pan claim is orthogonal to having an `on_scroll` handler: a
/// `SpinBox` increments on wheel, a `TabBar` remaps wheel to horizontal tab
/// scroll, and `SceneView` zooms on Ctrl-wheel — none of those is a pan, and
/// none of them declares a `PanClaim`. A scroll container declares itself
/// explicitly via [`scroll_container`](crate::widget_builder::HandlerSet::scroll_container).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct PanClaim {
    /// Which axes this surface wants to pan on.
    pub axes: PanAxes,
    /// Which pointer kinds this claim applies to. Defaults to
    /// [`PointerKindMask::DIRECT`] (touch + pen, never mouse) — see the
    /// module docs on why a mouse must never be treated as a panning
    /// pointer.
    pub devices: PointerKindMask,
    /// Whether a release should hand off to a fling/settle simulation. `false`
    /// by default; [`scroll_container`](crate::widget_builder::HandlerSet::scroll_container)'s
    /// sugar turns it on.
    pub kinetic: bool,
}

impl PanClaim {
    /// A vertical-only claim, direct pointers only, no kinetic hand-off.
    pub fn vertical() -> Self {
        Self {
            axes: PanAxes::Y,
            ..Self::default()
        }
    }

    /// A horizontal-only claim, direct pointers only, no kinetic hand-off.
    pub fn horizontal() -> Self {
        Self {
            axes: PanAxes::X,
            ..Self::default()
        }
    }

    /// A both-axes claim, direct pointers only, no kinetic hand-off.
    pub fn both() -> Self {
        Self {
            axes: PanAxes::BOTH,
            ..Self::default()
        }
    }
}

impl Default for PanClaim {
    /// No axes claimed, [`PointerKindMask::DIRECT`] devices, not kinetic.
    /// [`vertical`](Self::vertical) / [`horizontal`](Self::horizontal) /
    /// [`both`](Self::both) build on this rather than repeating the device
    /// mask.
    fn default() -> Self {
        Self {
            axes: PanAxes::NONE,
            devices: PointerKindMask::DIRECT,
            kinetic: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- TouchAction algebra ----------------------------------------------

    #[test]
    fn intersect_forms_a_commutative_monoid_with_auto_and_none_at_its_poles() {
        let values = [
            TouchAction::AUTO,
            TouchAction::NONE,
            TouchAction::PAN_X,
            TouchAction::PAN_Y,
            TouchAction::PAN,
            TouchAction::PINCH_ZOOM,
            TouchAction::MANIPULATION,
        ];

        for &a in &values {
            // AUTO is the identity.
            assert_eq!(
                a.intersect(TouchAction::AUTO),
                a,
                "AUTO must be an identity"
            );
            assert_eq!(
                TouchAction::AUTO.intersect(a),
                a,
                "AUTO must be an identity"
            );
            // NONE is absorbing.
            assert_eq!(
                a.intersect(TouchAction::NONE),
                TouchAction::NONE,
                "NONE must absorb"
            );
            assert_eq!(
                TouchAction::NONE.intersect(a),
                TouchAction::NONE,
                "NONE must absorb"
            );

            for &b in &values {
                // Commutative.
                assert_eq!(
                    a.intersect(b),
                    b.intersect(a),
                    "intersect must be commutative"
                );

                for &c in &values {
                    // Associative.
                    assert_eq!(
                        a.intersect(b).intersect(c),
                        a.intersect(b.intersect(c)),
                        "intersect must be associative"
                    );
                }
            }
        }
    }

    #[test]
    fn union_is_the_dual_operator() {
        assert_eq!(
            TouchAction::PAN_X.union(TouchAction::PAN_Y),
            TouchAction::PAN
        );
        assert_eq!(
            TouchAction::PAN.union(TouchAction::PINCH_ZOOM),
            TouchAction::MANIPULATION
        );
        assert_eq!(TouchAction::PAN_X | TouchAction::PAN_Y, TouchAction::PAN);
        assert_eq!(TouchAction::PAN & TouchAction::PAN_X, TouchAction::PAN_X);
    }

    #[test]
    fn default_is_auto() {
        assert_eq!(TouchAction::default(), TouchAction::AUTO);
    }

    /// `allows_pan` per axis, for every named constant.
    #[test]
    fn allows_pan_matches_the_named_constants() {
        assert!(TouchAction::AUTO.allows_pan_x() && TouchAction::AUTO.allows_pan_y());
        assert!(TouchAction::AUTO.allows_pinch());
        assert!(!TouchAction::NONE.allows_pan_x() && !TouchAction::NONE.allows_pan_y());
        assert!(!TouchAction::NONE.allows_pinch());

        assert!(TouchAction::PAN_X.allows_pan_x());
        assert!(!TouchAction::PAN_X.allows_pan_y());
        assert!(!TouchAction::PAN_X.allows_pinch());

        assert!(TouchAction::PAN_Y.allows_pan_y());
        assert!(!TouchAction::PAN_Y.allows_pan_x());
        assert!(!TouchAction::PAN_Y.allows_pinch());

        assert!(TouchAction::PAN.allows_pan_x() && TouchAction::PAN.allows_pan_y());
        assert!(!TouchAction::PAN.allows_pinch());

        assert!(TouchAction::PINCH_ZOOM.allows_pinch());
        assert!(!TouchAction::PINCH_ZOOM.allows_pan_x() && !TouchAction::PINCH_ZOOM.allows_pan_y());

        assert!(
            TouchAction::MANIPULATION.allows_pan_x() && TouchAction::MANIPULATION.allows_pan_y()
        );
        assert!(TouchAction::MANIPULATION.allows_pinch());
    }

    #[test]
    fn allows_delayed_gestures_is_false_only_for_none() {
        assert!(TouchAction::AUTO.allows_delayed_gestures());
        assert!(TouchAction::PAN_X.allows_delayed_gestures());
        assert!(!TouchAction::NONE.allows_delayed_gestures());
        assert!(TouchAction::NONE.is_none());
        assert!(!TouchAction::AUTO.is_none());
    }

    // --- PanAxes ------------------------------------------------------------

    #[test]
    fn pan_axes_contains_per_axis() {
        assert!(PanAxes::BOTH.contains(Axis::X) && PanAxes::BOTH.contains(Axis::Y));
        assert!(PanAxes::X.contains(Axis::X) && !PanAxes::X.contains(Axis::Y));
        assert!(PanAxes::Y.contains(Axis::Y) && !PanAxes::Y.contains(Axis::X));
        assert!(!PanAxes::NONE.contains(Axis::X) && !PanAxes::NONE.contains(Axis::Y));
        assert_eq!(PanAxes::default(), PanAxes::NONE);
    }

    // --- PanClaim ------------------------------------------------------------

    /// This is the field that keeps a mouse from ever being read as a
    /// panning pointer — load-bearing for "mouse behaves as today".
    #[test]
    fn pan_claim_default_devices_excludes_mouse() {
        let claim = PanClaim::default();
        assert!(!claim.devices.contains(teksilo_tokens::PointerKind::Mouse));
        assert!(claim.devices.contains(teksilo_tokens::PointerKind::Touch));
        assert!(claim.devices.contains(teksilo_tokens::PointerKind::Pen(
            teksilo_tokens::PenKind::Pen
        )));
        assert!(!claim.kinetic);
        assert_eq!(claim.axes, PanAxes::NONE);
    }

    #[test]
    fn vertical_horizontal_both_helpers_set_only_their_axes() {
        assert_eq!(PanClaim::vertical().axes, PanAxes::Y);
        assert_eq!(PanClaim::horizontal().axes, PanAxes::X);
        assert_eq!(PanClaim::both().axes, PanAxes::BOTH);
        // The device mask and kinetic flag still come from Default.
        assert_eq!(PanClaim::vertical().devices, PointerKindMask::DIRECT);
        assert!(!PanClaim::vertical().kinetic);
    }
}
