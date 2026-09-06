// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Input and density tokens — pure data describing *how big a target is* and
//! *how far a pointer may wander* before a gesture is recognised.
//!
//! This module is the single source of truth for the numbers the gesture
//! recognizers, the hit-test slop pass and the density sweep read. It sits in
//! `teksilo-tokens` (not `teksilo-core`) so that the token crate stays a leaf:
//! nothing here depends on the widget tree, the event types or a recognizer.
//!
//! # The three densities
//!
//! [`TargetDensity`] selects one of three target ladders:
//!
//! | density | `target_size` | `grab_size` | `spacing_factor` |
//! | --- | --- | --- | --- |
//! | [`Compact`](TargetDensity::Compact) | 24 dp | 6 dp | 1.00 |
//! | [`Comfortable`](TargetDensity::Comfortable) | 32 dp | 10 dp | 1.15 |
//! | [`Touch`](TargetDensity::Touch) | 44 dp | 16 dp | 1.30 |
//!
//! [`InputTokens::min_target_conformance`] is **24 dp at every density and is
//! never scaled**: it is the WCAG 2.2 SC 2.5.8 *Target Size (Minimum)* floor
//! (level AA). 44 dp is Apple's Human Interface Guidelines minimum and WCAG 2.2
//! SC 2.5.5 *Target Size (Enhanced)* (level **AAA**) — it must never be
//! described as AA. 48 dp is Material 3's touch-target minimum, which the
//! `teksilo-theme-material3` preset applies on top of the Touch ladder.
//!
//! `Compact` is the default and reproduces today's behaviour byte for byte —
//! see the per-constant provenance on [`GestureProfile::MOUSE`].
//!
//! Reference: `docs/density-and-targets.md`.

use std::time::Duration;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Pointer kinds
// ---------------------------------------------------------------------------

/// What kind of physical device produced a pointer event.
///
/// The distinction that matters to layout and gesture tuning is *direct vs
/// indirect* (does the user touch the pixel they mean?) and *coarse vs precise*
/// (how big is the contact patch?). A finger is direct and coarse; a stylus is
/// direct and precise; a mouse is indirect and precise.
///
/// `#[non_exhaustive]`: future input hardware (gaze, air pointers) may be added
/// without a breaking change, so `match` on it needs a `_` arm.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PointerKind {
    /// An indirect precise pointer with a hover state — mouse, trackpad,
    /// trackball. The default, and the only kind Teksilo saw before the
    /// touch-gesture programme.
    #[default]
    Mouse,
    /// A direct coarse pointer with no hover state — a finger on a touchscreen.
    Touch,
    /// A direct precise pointer, possibly hovering — a stylus or digitizer pen.
    Pen(PenKind),
    /// The backend could not classify the device. Treated as [`Mouse`] for
    /// gesture tuning (the conservative choice: tight slop, no hit outset).
    ///
    /// [`Mouse`]: PointerKind::Mouse
    Unknown,
}

impl PointerKind {
    /// Whether the user points at the pixel directly (finger, stylus) rather
    /// than through an on-screen cursor. Direct pointers occlude their own
    /// target, which is why they earn a hit outset and larger slop.
    pub const fn is_direct(self) -> bool {
        matches!(self, PointerKind::Touch | PointerKind::Pen(_))
    }

    /// Whether the contact patch is large enough that the reported point is an
    /// estimate rather than a position — true only for [`Touch`].
    ///
    /// [`Touch`]: PointerKind::Touch
    pub const fn is_coarse(self) -> bool {
        matches!(self, PointerKind::Touch)
    }

    /// Whether the reported position is accurate to roughly a pixel — a mouse
    /// or a pen, not a finger.
    pub const fn is_precise(self) -> bool {
        matches!(self, PointerKind::Mouse | PointerKind::Pen(_))
    }

    /// Whether this kind can report a position without a button held — i.e.
    /// whether hover-driven affordances (tooltips, hover reveal) can ever fire
    /// for it. False for [`Touch`], which is why [`RevealPolicy`] exists.
    ///
    /// [`Touch`]: PointerKind::Touch
    pub const fn hovers(self) -> bool {
        matches!(self, PointerKind::Mouse | PointerKind::Pen(_))
    }
}

/// The tool a [`PointerKind::Pen`] is acting as, as reported by the digitizer.
///
/// Teksilo does not interpret these beyond `Eraser` (which apps commonly bind
/// to a different action); they are carried through so a drawing surface can.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PenKind {
    /// A generic stylus tip. The default.
    #[default]
    Pen,
    /// The stylus's eraser end.
    Eraser,
    /// A brush tool (Windows Ink / Wacom tool id).
    Brush,
    /// A pencil tool.
    Pencil,
    /// An airbrush tool.
    Airbrush,
    /// A puck / lens cursor on a digitizer tablet.
    Lens,
    /// The digitizer reported a tool the backend does not recognise.
    Unknown,
}

/// A set of [`PointerKind`]s, for declaring which devices a behaviour accepts.
///
/// `PointerKind::Pen(_)` collapses to a single [`PEN`](Self::PEN) bit — masks
/// select devices, not tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct PointerKindMask(u8);

impl PointerKindMask {
    /// The empty set — matches nothing.
    pub const NONE: Self = Self(0);
    /// Just [`PointerKind::Mouse`].
    pub const MOUSE: Self = Self(1 << 0);
    /// Just [`PointerKind::Touch`].
    pub const TOUCH: Self = Self(1 << 1);
    /// Any [`PointerKind::Pen`], whatever the [`PenKind`].
    pub const PEN: Self = Self(1 << 2);
    /// Just [`PointerKind::Unknown`].
    pub const UNKNOWN: Self = Self(1 << 3);
    /// Every direct pointer — touch and pen. The set that earns a hit outset.
    pub const DIRECT: Self = Self(Self::TOUCH.0 | Self::PEN.0);
    /// Every pointer kind.
    pub const ALL: Self = Self(Self::MOUSE.0 | Self::TOUCH.0 | Self::PEN.0 | Self::UNKNOWN.0);

    /// Whether `kind` is a member of this set.
    pub const fn contains(self, kind: PointerKind) -> bool {
        let bit = match kind {
            PointerKind::Mouse => Self::MOUSE.0,
            PointerKind::Touch => Self::TOUCH.0,
            PointerKind::Pen(_) => Self::PEN.0,
            PointerKind::Unknown => Self::UNKNOWN.0,
        };
        self.0 & bit != 0
    }

    /// Whether the set is empty.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The raw bits, for callers that need to store the mask compactly.
    pub const fn bits(self) -> u8 {
        self.0
    }
}

impl std::ops::BitOr for PointerKindMask {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for PointerKindMask {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::BitOrAssign for PointerKindMask {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAndAssign for PointerKindMask {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

// ---------------------------------------------------------------------------
// Density
// ---------------------------------------------------------------------------

/// How large interactive targets are, as a whole-UI setting.
///
/// Named `TargetDensity` rather than `Density` on purpose: `teksilo-widgets`
/// already exports the public shadow constants `DENSITY_TOOLTIP` /
/// `DENSITY_SURFACE` / `DENSITY_DIALOG`, and two "density" vocabularies in one
/// crate would be a trap for readers.
///
/// [`Compact`](Self::Compact) is the default and is exactly today's behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TargetDensity {
    /// Desktop mouse-and-keyboard density: 24 dp targets, 6 dp grab handles.
    /// The default, and byte-for-byte today's constants.
    #[default]
    Compact,
    /// An intermediate ladder for hybrid devices and large-cursor users:
    /// 32 dp targets, 10 dp grab handles, spacing × 1.15.
    Comfortable,
    /// Finger-first density: 44 dp targets, 16 dp grab handles, spacing × 1.30.
    Touch,
}

/// What a dimension is *for*, so [`dp`](crate::input) callers can say whether a
/// value must grow with density.
///
/// The core helper lives in `teksilo_core::styles::density`; this enum is here
/// so the tokens crate owns the vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetRole {
    /// A tappable target — must reach `target_size`.
    Target,
    /// A draggable handle or divider — must reach `grab_size`. Grab affordances
    /// are usually thin decorations whose *hit* area is widened instead, so the
    /// floor is lower than a target's.
    Grab,
    /// Not interactive — an icon, a rule, a badge. Never scaled by density.
    Decoration,
}

/// The same distinction as [`TargetRole`] with `Spacing` added, for callers
/// that classify a whole table of dimensions in one pass (the density sweep,
/// `target_audit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DimensionRole {
    /// A tappable target — raised to `target_size`.
    Target,
    /// A draggable handle — raised to `grab_size`.
    Grab,
    /// Gap or padding — multiplied by `spacing_factor`.
    Spacing,
    /// Purely visual — left alone.
    Decoration,
}

/// Which axes of a [`Size`](crate) a target-size floor applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TargetAxes(u8);

impl TargetAxes {
    /// Raise the width only — for a full-width row whose height is the target.
    pub const WIDTH: Self = Self(1 << 0);
    /// Raise the height only — for a full-width row whose height is the target.
    pub const HEIGHT: Self = Self(1 << 1);
    /// Raise both axes — the usual case for a square-ish control.
    pub const BOTH: Self = Self(Self::WIDTH.0 | Self::HEIGHT.0);

    /// Whether the width axis is selected.
    pub const fn has_width(self) -> bool {
        self.0 & Self::WIDTH.0 != 0
    }

    /// Whether the height axis is selected.
    pub const fn has_height(self) -> bool {
        self.0 & Self::HEIGHT.0 != 0
    }
}

impl std::ops::BitOr for TargetAxes {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

// ---------------------------------------------------------------------------
// Gesture policy enums
// ---------------------------------------------------------------------------

/// When a drag may begin relative to the press that starts it.
///
/// `#[non_exhaustive]`: a future `AfterDelay(Duration)` is anticipated.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum DragActivation {
    /// Arm the drag as soon as the pointer passes `drag_slop` — the desktop
    /// convention, and what every Teksilo drag does today.
    Immediate,
    /// Require a long press first — the touch convention for reordering a list
    /// whose vertical axis is already claimed by scrolling.
    AfterLongPress,
    /// Choose per pointer kind: `Immediate` for an indirect precise pointer,
    /// `AfterLongPress` for a direct one whose axis is contested. The default,
    /// so a widget that states no preference behaves correctly on both.
    #[default]
    Auto,
}

/// What a scrollable does when the user pushes past its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum OverscrollStyle {
    /// Stop dead at the boundary — the Windows/GTK desktop convention, and
    /// Teksilo's behaviour today.
    #[default]
    Clamp,
    /// Follow the pointer with decreasing gain and spring back — the iOS /
    /// Flutter `BouncingScrollPhysics` convention.
    RubberBand,
}

/// Which fling/settle simulation a scrollable uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ScrollPhysics {
    /// Android `OverScroller` friction curve, hard stop at the bounds.
    Clamping,
    /// iOS/Flutter `BouncingScrollSimulation` — exponential decay plus a spring
    /// past the bounds.
    Bouncing,
    /// Pick per host OS at runtime: Bouncing on macOS/iOS, Clamping elsewhere.
    /// The default, so an app that states no preference feels native.
    #[default]
    Platform,
}

/// When a normally-hidden affordance (a hover toolbar, a row's delete button,
/// a scrollbar thumb) becomes visible.
///
/// Exists because a touch pointer never hovers: an `OnHover` affordance is
/// simply unreachable with a finger, so the Touch ladder promotes it to
/// `Always`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum RevealPolicy {
    /// Show while the pointer hovers the owning row/region. The default and
    /// today's behaviour.
    #[default]
    OnHover,
    /// Always visible. What the Touch ladder selects.
    Always,
    /// Reveal on a long press — the mobile "reveal row actions" idiom, for
    /// affordances too numerous to show permanently.
    OnLongPress,
}

/// How the active [`TargetDensity`] is chosen at runtime.
///
/// `Fixed(Compact)` is the default: the density never changes unless the app
/// changes it. `FollowLastPointer` is the auto-switching policy — a first touch
/// promotes the tree to the coarse density and a subsequent mouse move demotes
/// it, both after `hysteresis` has elapsed so a stray event cannot thrash a
/// full tree rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DensityPolicy {
    /// Always this density.
    Fixed(TargetDensity),
    /// Track the most recent pointer kind, with a settling delay.
    FollowLastPointer {
        /// Density adopted after a coarse ([`PointerKind::is_coarse`]) pointer.
        coarse: TargetDensity,
        /// Density adopted after a precise ([`PointerKind::is_precise`]) pointer.
        fine: TargetDensity,
        /// How long the new kind must persist before the switch commits.
        hysteresis: Duration,
    },
}

impl Default for DensityPolicy {
    fn default() -> Self {
        DensityPolicy::Fixed(TargetDensity::Compact)
    }
}

// ---------------------------------------------------------------------------
// GestureProfile
// ---------------------------------------------------------------------------

/// The per-pointer-kind gesture tuning constants.
///
/// One profile per [`PointerKind`] family, selected by
/// [`InputTokens::profile`]. The distances are in logical pixels (dp); the
/// velocities are in dp/second.
///
/// Every field of [`Self::MOUSE`] is exactly the constant Teksilo shipped
/// before the touch programme — see that constant's docs for the file and line
/// each was read from. The `TOUCH` and `PEN` columns are new; their provenance
/// is cited on those constants.
///
/// # Invariant
///
/// For every profile:
/// `pan_slop.unwrap_or(∞) > drag_slop >= tap_slop >= slop_precise`.
/// A pan must be harder to start than a drag (a scroll should lose to a
/// reorder only after a clearly longer travel), a drag no easier than a tap's
/// tolerance, and nothing may go below the precise floor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GestureProfile {
    /// How far the pointer may travel between down and up and still be a tap.
    pub tap_slop: f32,
    /// How far the pointer must travel from the press point to arm a drag.
    pub drag_slop: f32,
    /// How far apart two taps may land and still be a double/triple tap.
    pub multi_tap_slop: f32,
    /// The floor below which no slop may be reduced — the sampling jitter of a
    /// precise device. Same on every profile.
    pub slop_precise: f32,
    /// How far the pointer may travel during a long press without cancelling it.
    pub long_press_slop: f32,
    /// How far outside its bounds a widget still accepts a press, for direct
    /// pointers whose contact patch occludes the target. `0.0` for a mouse:
    /// the cursor hot-spot is exact.
    pub hit_slop: f32,
    /// How far the pointer must travel to arm a *pan* (a scroll drag on a
    /// scrollable), or `None` if this pointer kind never pans — the mouse
    /// scrolls with the wheel, so it has no pan slop at all.
    pub pan_slop: Option<f32>,
    /// How long a press must be held to become a long press.
    pub long_press: Duration,
    /// The maximum gap between the taps of a double/triple tap.
    pub multi_tap_interval: Duration,
    /// How long to wait before painting press feedback, so a press that turns
    /// out to be the start of a scroll never flashes a highlight.
    pub press_feedback_delay: Duration,
    /// The longest a press may be held and still resolve as a tap rather than
    /// waiting for the long-press timer.
    pub max_hold: Duration,
    /// Below this release velocity a drag settles instead of flinging.
    pub min_fling_velocity: f32,
    /// Release velocities are clamped to this before the fling simulation runs.
    pub max_fling_velocity: f32,
    /// The velocity a directional swipe must reach to be recognised.
    pub swipe_min_velocity: f32,
    /// The distance a directional swipe must cover to be recognised.
    pub swipe_min_distance: f32,
    /// Whether a drag arms on slop alone or needs a long press first.
    pub drag_activation: DragActivation,
}

impl GestureProfile {
    /// Indirect precise pointer (mouse, trackpad).
    ///
    /// **Every value here is the constant Teksilo already used**, so selecting
    /// this profile is a no-op. Provenance, verified by reading the source:
    ///
    /// - `tap_slop = 5.0` — `TapRecognizer::new`'s `max_distance`,
    ///   `teksilo-core/src/gesture/tap.rs:28`.
    /// - `drag_slop = 5.0` — `DragRecognizer::new`'s `threshold`,
    ///   `teksilo-core/src/gesture/drag.rs:23`, and the explicit
    ///   `.threshold(5.0)` at
    ///   `teksilo-core/src/widget_tree/gesture_dispatch_impl.rs:96`.
    /// - `multi_tap_slop = 10.0` — `max_distance` on both
    ///   `DoubleTapRecognizer::new` (`gesture/multi_tap.rs:35`) and
    ///   `TripleTapRecognizer::new` (`gesture/multi_tap.rs:216`).
    /// - `long_press_slop = 5.0`, `long_press = 500 ms` — `max_distance` and
    ///   `min_duration` in `LongPressRecognizer::new`,
    ///   `teksilo-core/src/gesture/long_press.rs:39-40`.
    /// - `multi_tap_interval = 300 ms` — `max_interval` on both multi-tap
    ///   recognizers, `gesture/multi_tap.rs:36` and `:217`.
    /// - `swipe_min_velocity = 200.0`, `swipe_min_distance = 30.0` —
    ///   `SwipeRecognizer::new`, `teksilo-core/src/gesture/swipe.rs:23-24`.
    /// - `hit_slop = 0.0` — Teksilo has no slop pass today; a mouse hit is
    ///   exact and stays exact.
    /// - `pan_slop = None` — a mouse scrolls with the wheel, never by dragging
    ///   the content, so there is no pan to arm.
    /// - `slop_precise = 2.0` — the shared precise-device jitter floor (see
    ///   [`Self::PEN`]); it is below every mouse slop, so it never bites.
    /// - `min_fling_velocity = 50.0`, `max_fling_velocity = 8000.0` — Android
    ///   `ViewConfiguration` `MINIMUM_FLING_VELOCITY` / `MAXIMUM_FLING_VELOCITY`
    ///   (50 and 8000 dp/s). New: Teksilo has no fling today, so any value is
    ///   a no-op until P12 lands the kinetic core.
    /// - `press_feedback_delay = 100 ms` — Flutter `kPressTimeout` /
    ///   Android `ViewConfiguration.getTapTimeout()`, both 100 ms. New: no
    ///   deferred press feedback exists yet.
    /// - `max_hold = 250 ms` — a Teksilo choice between `kPressTimeout`
    ///   (100 ms) and `kLongPressTimeout` (500 ms); there is no upstream
    ///   constant for it. New and currently unread.
    /// - `drag_activation = Auto` — resolves to `Immediate` for an indirect
    ///   precise pointer, i.e. today's behaviour.
    pub const MOUSE: Self = Self {
        tap_slop: 5.0,
        drag_slop: 5.0,
        multi_tap_slop: 10.0,
        slop_precise: 2.0,
        long_press_slop: 5.0,
        hit_slop: 0.0,
        pan_slop: None,
        long_press: Duration::from_millis(500),
        multi_tap_interval: Duration::from_millis(300),
        press_feedback_delay: Duration::from_millis(100),
        max_hold: Duration::from_millis(250),
        min_fling_velocity: 50.0,
        max_fling_velocity: 8000.0,
        swipe_min_velocity: 200.0,
        swipe_min_distance: 30.0,
        drag_activation: DragActivation::Auto,
    };

    /// Direct coarse pointer (a finger).
    ///
    /// Provenance — all new, none of these values existed in Teksilo before:
    ///
    /// - `tap_slop = drag_slop = 18.0` — Flutter `kTouchSlop`
    ///   (`flutter/packages/flutter/lib/src/gestures/constants.dart`), the
    ///   cross-platform touch slop every Flutter drag recognizer uses.
    /// - `pan_slop = 36.0` — Flutter `kPanSlop`, defined there as
    ///   `kTouchSlop * 2.0`: a two-dimensional pan must travel twice as far as
    ///   a one-dimensional drag before it wins the arena.
    /// - `multi_tap_slop = 40.0` — Flutter `kDoubleTapSlop`.
    /// - `long_press_slop = 18.0` — `kTouchSlop` again; Flutter cancels a long
    ///   press at the same distance it arms a drag.
    /// - `hit_slop = 8.0` — Android `ViewConfiguration` `TOUCH_SLOP`-scale
    ///   "hover/hit" allowance of 8 dp, the amount a finger's reported centre
    ///   may miss the intended target by.
    /// - `long_press = 500 ms` — Flutter `kLongPressTimeout`.
    /// - `multi_tap_interval = 300 ms` — Flutter `kDoubleTapTimeout`.
    /// - `swipe_min_velocity = 300.0`, `swipe_min_distance = 40.0` — raised
    ///   over the mouse column so an imprecise finger drag is not mistaken for
    ///   a deliberate swipe.
    /// - fling velocities, `press_feedback_delay`, `max_hold`, `slop_precise`
    ///   — as [`Self::MOUSE`]; these are device-independent.
    pub const TOUCH: Self = Self {
        tap_slop: 18.0,
        drag_slop: 18.0,
        multi_tap_slop: 40.0,
        slop_precise: 2.0,
        long_press_slop: 18.0,
        hit_slop: 8.0,
        pan_slop: Some(36.0),
        long_press: Duration::from_millis(500),
        multi_tap_interval: Duration::from_millis(300),
        press_feedback_delay: Duration::from_millis(100),
        max_hold: Duration::from_millis(250),
        min_fling_velocity: 50.0,
        max_fling_velocity: 8000.0,
        swipe_min_velocity: 300.0,
        swipe_min_distance: 40.0,
        drag_activation: DragActivation::Auto,
    };

    /// Direct precise pointer (a stylus).
    ///
    /// A pen reports position to roughly a pixel, so its slops are *tighter*
    /// than a mouse's — the value of a stylus is precision, and inheriting the
    /// mouse's 5 dp would throw it away. Provenance — all new:
    ///
    /// - `tap_slop = drag_slop = 2.0` — the `slop_precise` jitter floor; a
    ///   digitizer's noise, and nothing more.
    /// - `long_press_slop = 10.0` — larger than the drag slop on purpose: a
    ///   hand resting on a tablet drifts over half a second, and cancelling
    ///   the long press at 2 dp would make it unusable.
    /// - `multi_tap_slop = 12.0` — a stylus is re-placed between taps, so the
    ///   landing spot scatters more than the within-tap jitter.
    /// - `hit_slop = 2.0` — the pen tip occludes a little, but far less than
    ///   a finger.
    /// - `pan_slop = 8.0` — a pen can pan, but a small travel should still let
    ///   a drag win.
    /// - timings and fling velocities — as [`Self::MOUSE`].
    pub const PEN: Self = Self {
        tap_slop: 2.0,
        drag_slop: 2.0,
        multi_tap_slop: 12.0,
        slop_precise: 2.0,
        long_press_slop: 10.0,
        hit_slop: 2.0,
        pan_slop: Some(8.0),
        long_press: Duration::from_millis(500),
        multi_tap_interval: Duration::from_millis(300),
        press_feedback_delay: Duration::from_millis(100),
        max_hold: Duration::from_millis(250),
        min_fling_velocity: 50.0,
        max_fling_velocity: 8000.0,
        swipe_min_velocity: 200.0,
        swipe_min_distance: 30.0,
        drag_activation: DragActivation::Auto,
    };
}

impl Default for GestureProfile {
    /// The mouse profile — today's behaviour.
    fn default() -> Self {
        Self::MOUSE
    }
}

/// One [`GestureProfile`] per pointer-kind family.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GestureTokens {
    /// Tuning for indirect precise pointers. Also used for
    /// [`PointerKind::Unknown`].
    pub mouse: GestureProfile,
    /// Tuning for direct coarse pointers.
    pub touch: GestureProfile,
    /// Tuning for direct precise pointers.
    pub pen: GestureProfile,
}

impl GestureTokens {
    /// The shipped tuning: one profile per pointer-kind family. Const so
    /// [`InputTokens::for_density`] can stay a `const fn`.
    pub const DEFAULT: Self = Self {
        mouse: GestureProfile::MOUSE,
        touch: GestureProfile::TOUCH,
        pen: GestureProfile::PEN,
    };
}

impl Default for GestureTokens {
    fn default() -> Self {
        Self::DEFAULT
    }
}

// ---------------------------------------------------------------------------
// Scroll physics
// ---------------------------------------------------------------------------

/// Constants for the fling / settle / overscroll simulations.
///
/// Nothing reads these yet — P12 lands the kinetic core. They are declared here
/// so the whole input surface is one struct and a theme carries it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScrollPhysicsTokens {
    /// Which simulation family to run.
    pub physics: ScrollPhysics,
    /// Android `OverScroller`'s `DECELERATION_RATE`, `ln(0.78) / ln(0.9)`
    /// ≈ 2.3582. The exponent of the friction curve.
    pub clamping_deceleration_rate: f32,
    /// Android `OverScroller`'s `INFLEXION` — 0.35, where the spline switches
    /// from its viscous to its deceleration segment.
    pub clamping_inflexion: f32,
    /// Android `ViewConfiguration`'s scroll friction — 0.015.
    pub clamping_friction: f32,
    /// Flutter `BouncingScrollSimulation`'s per-second velocity retention —
    /// 0.135.
    pub bouncing_decay_per_second: f32,
    /// Mass of the settle spring, in the Flutter `SpringDescription` sense.
    pub spring_mass: f32,
    /// Stiffness of the settle spring.
    pub spring_stiffness: f32,
    /// Damping ratio of the settle spring. Slightly over 1.0 = just
    /// overdamped, so the content never overshoots on the way back.
    pub spring_damping_ratio: f32,
    /// iOS / Flutter `frictionFactor` for the rubber-band curve — 0.52. The
    /// gain applied to travel past the boundary.
    pub rubber_band_factor: f32,
}

impl ScrollPhysicsTokens {
    /// The shipped physics constants. Const so [`InputTokens::for_density`]
    /// can stay a `const fn`.
    pub const DEFAULT: Self = Self {
        physics: ScrollPhysics::Platform,
        // ln(0.78) / ln(0.9) — Android OverScroller::DECELERATION_RATE.
        clamping_deceleration_rate: 2.358_202,
        clamping_inflexion: 0.35,
        clamping_friction: 0.015,
        bouncing_decay_per_second: 0.135,
        spring_mass: 0.5,
        spring_stiffness: 100.0,
        spring_damping_ratio: 1.1,
        rubber_band_factor: 0.52,
    };
}

impl Default for ScrollPhysicsTokens {
    fn default() -> Self {
        Self::DEFAULT
    }
}

// ---------------------------------------------------------------------------
// InputTokens
// ---------------------------------------------------------------------------

/// The complete input/density token group carried by a `Theme`.
///
/// Read it through `ctx.theme().input`; project a theme to another density with
/// `Theme::with_density`. [`Default`] is [`Self::for_density`]`(Compact)`,
/// which reproduces today's behaviour exactly.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputTokens {
    /// Which ladder these values came from.
    pub density: TargetDensity,
    /// The absolute minimum size of an interactive target, in dp. **24.0 at
    /// every density and never scaled** — WCAG 2.2 SC 2.5.8 *Target Size
    /// (Minimum)*, level AA. A `TargetRole::Target` dimension may never come
    /// out below this.
    pub min_target_conformance: f32,
    /// The target size this density aims for: 24 / 32 / 44 dp. Always at least
    /// [`Self::min_target_conformance`].
    pub target_size: f32,
    /// The minimum extent of a draggable handle or divider: 6 / 10 / 16 dp.
    pub grab_size: f32,
    /// How far a *miss* may be from a target and still be re-attributed to it,
    /// in dp, for coarse pointers only: 12 / 12 / 16. Spent by the hit-test
    /// slop pass (P10); a precise pointer never draws on it.
    pub slop_budget: f32,
    /// The hit outset for a pen, in dp. Constant at 2.0 across densities: it
    /// describes the tool, not the UI.
    pub pen_hit_slop: f32,
    /// Multiplier applied to gaps and padding: 1.00 / 1.15 / 1.30.
    pub spacing_factor: f32,
    /// When hover-revealed affordances become visible. `Always` at Touch,
    /// since a finger never hovers.
    pub reveal: RevealPolicy,
    /// Per-pointer-kind gesture tuning.
    pub gestures: GestureTokens,
    /// Fling / settle / overscroll constants.
    pub scroll_physics: ScrollPhysicsTokens,
    /// Lines scrolled per wheel notch — 3.0, the Windows/GTK default.
    /// Mirrors the `LINES_PER_NOTCH` constant at
    /// `teksilo-platform/src/event_translation.rs:241`, which P15 replaces
    /// with a read of this field.
    pub lines_per_notch: f32,
    /// The runtime kill switch. When `false`, the platform translator drops
    /// touch input and the router installs no touch-only recognizers, so an
    /// app can fall back to mouse-only behaviour without a rebuild of the
    /// binary. `true` by default.
    pub touch_enabled: bool,
}

impl InputTokens {
    /// The token set for one density ladder.
    ///
    /// `for_density(Compact)` is [`Default`] and is byte-for-byte today's
    /// behaviour: every gesture profile is unchanged, `hit_slop` is zero for
    /// the mouse, and `spacing_factor` is 1.0.
    pub const fn for_density(density: TargetDensity) -> Self {
        let (target_size, grab_size, slop_budget, spacing_factor, reveal) = match density {
            TargetDensity::Compact => (24.0, 6.0, 12.0, 1.00, RevealPolicy::OnHover),
            TargetDensity::Comfortable => (32.0, 10.0, 12.0, 1.15, RevealPolicy::OnHover),
            TargetDensity::Touch => (44.0, 16.0, 16.0, 1.30, RevealPolicy::Always),
        };
        Self {
            density,
            // WCAG 2.2 SC 2.5.8 (AA). Never scaled — see the field docs.
            min_target_conformance: 24.0,
            target_size,
            grab_size,
            slop_budget,
            pen_hit_slop: 2.0,
            spacing_factor,
            reveal,
            gestures: GestureTokens::DEFAULT,
            scroll_physics: ScrollPhysicsTokens::DEFAULT,
            lines_per_notch: 3.0,
            touch_enabled: true,
        }
    }

    /// The gesture profile for a pointer kind.
    ///
    /// [`PointerKind::Unknown`] maps to the mouse profile — the conservative
    /// choice, since it has the tightest slop of the two indirect options and
    /// no hit outset.
    pub const fn profile(&self, kind: PointerKind) -> &GestureProfile {
        match kind {
            PointerKind::Mouse | PointerKind::Unknown => &self.gestures.mouse,
            PointerKind::Touch => &self.gestures.touch,
            PointerKind::Pen(_) => &self.gestures.pen,
        }
    }
}

impl Default for InputTokens {
    fn default() -> Self {
        Self::for_density(TargetDensity::Compact)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `pan_slop.unwrap_or(∞) > drag_slop >= tap_slop >= slop_precise`, on
    /// every profile. A pan must be harder to start than a drag; nothing may
    /// sink below the precise-device jitter floor.
    #[test]
    fn every_profile_orders_its_slops() {
        for (name, p) in [
            ("mouse", GestureProfile::MOUSE),
            ("touch", GestureProfile::TOUCH),
            ("pen", GestureProfile::PEN),
        ] {
            let pan = p.pan_slop.unwrap_or(f32::INFINITY);
            assert!(
                pan > p.drag_slop,
                "{name}: pan_slop {pan} <= drag {}",
                p.drag_slop
            );
            assert!(
                p.drag_slop >= p.tap_slop,
                "{name}: drag {} < tap {}",
                p.drag_slop,
                p.tap_slop
            );
            assert!(
                p.tap_slop >= p.slop_precise,
                "{name}: tap {} < precise {}",
                p.tap_slop,
                p.slop_precise
            );
        }
    }

    #[test]
    fn the_mouse_never_pans() {
        assert_eq!(GestureProfile::MOUSE.pan_slop, None);
        assert!(GestureProfile::TOUCH.pan_slop.is_some());
        assert!(GestureProfile::PEN.pan_slop.is_some());
    }

    /// The 24 dp WCAG 2.2 SC 2.5.8 (AA) floor is a conformance constant, not a
    /// density knob: it is identical on all three ladders.
    #[test]
    fn the_conformance_floor_is_24_at_every_density() {
        for d in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            assert_eq!(InputTokens::for_density(d).min_target_conformance, 24.0);
        }
    }

    /// A density's own `target_size` may never sit below the conformance floor.
    #[test]
    fn every_density_meets_its_own_floor() {
        for d in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let t = InputTokens::for_density(d);
            assert!(t.target_size >= t.min_target_conformance);
        }
    }

    /// Compact must reproduce the pre-programme constants exactly. Each row
    /// cites the source that was read to establish it.
    #[test]
    fn compact_equals_todays_constants() {
        let t = InputTokens::for_density(TargetDensity::Compact);
        let m = t.profile(PointerKind::Mouse);

        // gesture/tap.rs:28 — TapRecognizer::new { max_distance: 5.0 }
        assert_eq!(m.tap_slop, 5.0);
        // gesture/drag.rs:23 — DragRecognizer::new { threshold: 5.0 }, and
        // widget_tree/gesture_dispatch_impl.rs:96 — .threshold(5.0)
        assert_eq!(m.drag_slop, 5.0);
        // gesture/multi_tap.rs:35 and :216 — max_distance: 10.0
        assert_eq!(m.multi_tap_slop, 10.0);
        // gesture/long_press.rs:39 — max_distance: 5.0
        assert_eq!(m.long_press_slop, 5.0);
        // gesture/long_press.rs:40 — min_duration: from_millis(500)
        assert_eq!(m.long_press, Duration::from_millis(500));
        // gesture/multi_tap.rs:36 and :217 — max_interval: from_millis(300)
        assert_eq!(m.multi_tap_interval, Duration::from_millis(300));
        // gesture/swipe.rs:23 — min_velocity: 200.0
        assert_eq!(m.swipe_min_velocity, 200.0);
        // gesture/swipe.rs:24 — min_distance: 30.0
        assert_eq!(m.swipe_min_distance, 30.0);
        // No slop pass exists today: a mouse hit is exact.
        assert_eq!(m.hit_slop, 0.0);
        // A mouse scrolls with the wheel, so there is no pan to arm.
        assert_eq!(m.pan_slop, None);
        // platform/event_translation.rs:241 — const LINES_PER_NOTCH: f32 = 3.0
        assert_eq!(t.lines_per_notch, 3.0);
        // No density scaling before the programme.
        assert_eq!(t.spacing_factor, 1.00);
        // Hover-revealed affordances stay hover-revealed.
        assert_eq!(t.reveal, RevealPolicy::OnHover);
        // Touch input is on unless an app turns it off.
        assert!(t.touch_enabled);
    }

    #[test]
    fn default_is_compact() {
        assert_eq!(
            InputTokens::default(),
            InputTokens::for_density(TargetDensity::Compact)
        );
        assert_eq!(TargetDensity::default(), TargetDensity::Compact);
    }

    #[test]
    fn density_policy_defaults_to_fixed_compact() {
        assert_eq!(
            DensityPolicy::default(),
            DensityPolicy::Fixed(TargetDensity::Compact)
        );
    }

    #[test]
    fn unknown_pointers_use_the_mouse_profile() {
        let t = InputTokens::default();
        assert_eq!(
            t.profile(PointerKind::Unknown),
            t.profile(PointerKind::Mouse)
        );
        assert_eq!(
            t.profile(PointerKind::Pen(PenKind::Eraser)),
            &t.gestures.pen
        );
    }

    #[test]
    fn pointer_kind_predicates() {
        assert!(PointerKind::Touch.is_direct() && PointerKind::Touch.is_coarse());
        assert!(!PointerKind::Touch.hovers() && !PointerKind::Touch.is_precise());
        assert!(PointerKind::Pen(PenKind::Pen).is_direct());
        assert!(PointerKind::Pen(PenKind::Pen).is_precise());
        assert!(PointerKind::Pen(PenKind::Pen).hovers());
        assert!(!PointerKind::Mouse.is_direct());
        assert!(PointerKind::Mouse.is_precise() && PointerKind::Mouse.hovers());
        assert!(!PointerKind::Unknown.is_direct() && !PointerKind::Unknown.is_coarse());
    }

    #[test]
    fn masks_select_devices_not_tools() {
        assert!(PointerKindMask::DIRECT.contains(PointerKind::Touch));
        assert!(PointerKindMask::DIRECT.contains(PointerKind::Pen(PenKind::Eraser)));
        assert!(!PointerKindMask::DIRECT.contains(PointerKind::Mouse));
        assert!(PointerKindMask::NONE.is_empty());
        assert_eq!(
            PointerKindMask::TOUCH | PointerKindMask::PEN,
            PointerKindMask::DIRECT
        );
        assert_eq!(
            PointerKindMask::ALL & PointerKindMask::MOUSE,
            PointerKindMask::MOUSE
        );
        assert!(PointerKindMask::ALL.contains(PointerKind::Unknown));
    }

    #[test]
    fn target_axes_select_axes() {
        assert!(TargetAxes::BOTH.has_width() && TargetAxes::BOTH.has_height());
        assert!(TargetAxes::WIDTH.has_width() && !TargetAxes::WIDTH.has_height());
        assert_eq!(TargetAxes::WIDTH | TargetAxes::HEIGHT, TargetAxes::BOTH);
    }

    #[test]
    fn the_touch_ladder_always_reveals() {
        assert_eq!(
            InputTokens::for_density(TargetDensity::Touch).reveal,
            RevealPolicy::Always
        );
    }

    /// `InputTokens` rides on `Theme`, which is `Serialize + Deserialize`.
    #[test]
    fn input_tokens_round_trip_through_json() {
        for d in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let t = InputTokens::for_density(d);
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(serde_json::from_str::<InputTokens>(&json).unwrap(), t);
        }
    }

    /// `#[serde(default)]` on the struct means a partial token blob still
    /// deserializes — the same guarantee `Theme` needs for its `input` field.
    #[test]
    fn a_partial_blob_fills_from_default() {
        let t: InputTokens = serde_json::from_str(r#"{"target_size": 48.0}"#).unwrap();
        assert_eq!(t.target_size, 48.0);
        assert_eq!(t.min_target_conformance, 24.0);
        assert_eq!(t.lines_per_notch, 3.0);
    }

    /// `teksilo-tokens` is a leaf: it depends on `serde` and nothing else.
    /// Adding an edge here would push a dependency onto every crate in the
    /// workspace, so the manifest is a test fixture.
    #[test]
    fn teksilo_tokens_gains_no_dependency() {
        let manifest = include_str!("../Cargo.toml");
        let deps = manifest
            .split("[dependencies]")
            .nth(1)
            .expect("[dependencies] section")
            .split("\n[")
            .next()
            .expect("section body");
        let names: Vec<&str> = deps
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| l.split(['=', ' ']).next())
            .filter(|n| !n.is_empty())
            .collect();
        assert_eq!(
            names,
            vec!["serde"],
            "teksilo-tokens must stay a serde-only leaf; found {names:?}"
        );
    }
}
