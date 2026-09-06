// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Hit slop — the *miss-only* re-attribution of a press to a small target it
//! nearly landed on, and the context every hit test carries.
//!
//! # Three mechanisms, three domains
//!
//! Teksilo widens hit targets in three places, and they do not overlap. Pick by
//! what the widget's problem actually is:
//!
//! | mechanism | where it runs | for |
//! | --- | --- | --- |
//! | [`Widget::target_regions`] | reporting only | a control painted as sub-regions of ONE leaf node — the scroll-bar thumb, the slider thumb |
//! | [`Widget::hit_outset`] | **inside** the exact pass | a thin grip that must win over what it overlaps — a splitter gutter, a column-resize strip |
//! | [`Widget::hit_distance`] + this module | **only after** the exact pass missed | an isolated small target — a radio dot, a chart mark |
//!
//! A grip needs `hit_outset` because it must *beat* its neighbours, and the
//! slop pass never beats anything the exact pass found. A radio dot needs the
//! slop pass because widening its rectangle would steal presses from the row
//! it sits in. Nothing needs both.
//!
//! # The size formula
//!
//! A node earns an outset of
//!
//! ```text
//! ((up_to − min(width, height)) / 2).clamp(0, radius)
//! ```
//!
//! so a target that is already at least `up_to` on its smaller axis earns
//! **nothing**: the mechanism is for small controls, and a full-viewport scrim
//! or a list row is excluded by arithmetic rather than by a rule. `radius`
//! comes from the pointer's gesture profile (0 dp mouse / 8 dp touch / 2 dp
//! pen) and is capped for a coarse pointer by `InputTokens::slop_budget`
//! (12 / 12 / 16 dp). `up_to` is the density's `target_size`.
//!
//! Because the mouse profile's radius is `0.0`, **every mouse hit test is
//! exactly the hit test Teksilo has always run** — the whole pass short-circuits
//! before it walks anything.
//!
//! Reference: `docs/density-and-targets.md`.
//!
//! [`Widget::target_regions`]: crate::widget::Widget::target_regions
//! [`Widget::hit_outset`]: crate::widget::Widget::hit_outset
//! [`Widget::hit_distance`]: crate::widget::Widget::hit_distance

use teksilo_canvas::{Point, Rect, Size, Transform2D};
use teksilo_tokens::{InputTokens, PointerKind, TargetDensity};

use crate::environment::LayoutDirection;
use crate::widget_id::WidgetId;

/// The token set the plain, pointer-less hit-test doors read.
///
/// [`WidgetArena::hit_test_at`](crate::arena::WidgetArena::hit_test_at) has no
/// theme to consult, so it reads the Compact ladder — which is the identity for
/// every hit-targeting mechanism (mouse radius `0.0`, grip outset `0.0`).
/// Callers that *do* hold a theme (`WidgetTree`) pass the live tokens instead.
static COMPACT_TOKENS: InputTokens = InputTokens::for_density(TargetDensity::Compact);

/// How far a *miss* may be re-attributed to a target, and up to what target
/// size the offer stands.
///
/// `radius` is a hard ceiling in dp; `up_to` is the size a target is topped up
/// *towards*. See the module docs for the formula and why a large node earns
/// nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitSlop {
    /// The largest outset any node may earn, in dp. `0.0` disables the
    /// mechanism outright.
    pub radius: f32,
    /// The target size a node is topped up towards, in dp. A node already at
    /// least this big on its smaller axis earns no outset at all.
    pub up_to: f32,
}

impl HitSlop {
    /// No slop at all — the mouse's value, and what
    /// [`no_hit_slop`](crate::widget_builder::HandlerSet::no_hit_slop) resolves
    /// to.
    pub const NONE: HitSlop = HitSlop {
        radius: 0.0,
        up_to: 0.0,
    };

    /// The density default for a pointer kind: the kind's profile `hit_slop`,
    /// capped for a **coarse** pointer by `InputTokens::slop_budget`, topped up
    /// towards `InputTokens::target_size`.
    ///
    /// A precise pointer (mouse, pen) never draws on the budget — its radius is
    /// already the tool's, and the budget describes a contact patch it does not
    /// have. The mouse's profile radius is `0.0`, so this returns
    /// [`NONE`](Self::NONE) for a mouse at every density.
    pub fn for_pointer(kind: PointerKind, tokens: &InputTokens) -> Self {
        let profile_radius = tokens.profile(kind).hit_slop;
        let radius = if kind.is_coarse() {
            profile_radius.min(tokens.slop_budget)
        } else {
            profile_radius
        };
        Self {
            radius: sanitize(radius),
            up_to: sanitize(tokens.target_size),
        }
    }

    /// Whether this slop can ever produce an outset.
    pub fn is_none(&self) -> bool {
        self.radius <= 0.0 || self.up_to <= 0.0
    }

    /// The outset a node of `size` earns:
    /// `((up_to − min(w, h)) / 2).clamp(0, radius)`.
    ///
    /// Zero for any node already at least `up_to` on its smaller axis, and zero
    /// for a degenerate (non-finite, negative) size.
    pub fn outset_for(&self, size: Size) -> f32 {
        let radius = sanitize(self.radius);
        if radius <= 0.0 {
            return 0.0;
        }
        let smaller = sanitize(size.width.min(size.height));
        ((sanitize(self.up_to) - smaller) / 2.0).clamp(0.0, radius)
    }
}

/// Replace a non-finite or negative value with `0.0`.
///
/// Every arithmetic path below feeds `f32::clamp`, which **panics** when its
/// bounds are `NaN` or out of order, so the guard is load-bearing rather than
/// defensive: a widget that measures to `NaN` under a degenerate proposal must
/// not take the process down through the hit test.
fn sanitize(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 { v } else { 0.0 }
}

/// Euclidean distance from `point` to the nearest edge of `rect`, `0.0` when it
/// is inside. The default [`Widget::hit_distance`] shape.
///
/// [`Widget::hit_distance`]: crate::widget::Widget::hit_distance
pub fn rect_distance(rect: Rect, point: Point) -> f32 {
    let dx = (rect.x - point.x).max(point.x - rect.right()).max(0.0);
    let dy = (rect.y - point.y).max(point.y - rect.bottom()).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

/// Euclidean distance from `point` to a disc, `0.0` when it is inside.
///
/// The shape a round control — a radio dot, a slider knob, a colour-strip thumb
/// — reports from [`Widget::hit_distance`] so its slop follows the silhouette
/// the user sees rather than the square it is laid out in.
///
/// [`Widget::hit_distance`]: crate::widget::Widget::hit_distance
pub fn circle_distance(center: Point, radius: f32, point: Point) -> f32 {
    let dx = point.x - center.x;
    let dy = point.y - center.y;
    ((dx * dx + dy * dy).sqrt() - radius.max(0.0)).max(0.0)
}

/// The **minimum singular value** of a transform's linear part — the factor by
/// which it shrinks the axis it shrinks most.
///
/// Closed form for a 2×2: with `E = (a+d)/2`, `F = (a−d)/2`, `G = (b+c)/2`,
/// `H = (b−c)/2`, the singular values are `hypot(E,H) ± hypot(F,G)`.
///
/// The slop pass measures distance in a node's own local space and compares it
/// against a radius quoted in screen dp, so it needs one number for "how much
/// bigger is a local unit on screen". σ<sub>min</sub> is the conservative choice
/// *in the user's favour*: under an anisotropic transform it is the axis along
/// which local distance buys the fewest screen pixels, so the reach never comes
/// out shorter than the token promised on any axis. A pure rotation returns
/// `1.0`; a singular transform returns `0.0` (its subtree is invisible, so it
/// takes no candidates).
pub fn min_singular_value(t: &Transform2D) -> f32 {
    let [a, b, c, d, _, _] = t.m;
    let e = (a + d) / 2.0;
    let f = (a - d) / 2.0;
    let g = (b + c) / 2.0;
    let h = (b - c) / 2.0;
    let q = e.hypot(h);
    let r = f.hypot(g);
    let sigma_min = (q - r).abs();
    if sigma_min.is_finite() {
        sigma_min
    } else {
        0.0
    }
}

/// One node the miss-only slop pass considered, and how far away it was.
///
/// Produced by
/// [`WidgetArena::hit_candidates`](crate::arena::WidgetArena::hit_candidates),
/// which is public so a test — and the target-conformance audit — can inspect
/// the pass's reasoning rather than only its verdict.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitCandidate {
    /// The node that offered itself.
    pub id: WidgetId,
    /// Distance from the pointer to the node's **uninflated** shape, in screen
    /// dp (a local distance under a transform is converted by
    /// [`min_singular_value`]).
    pub distance: f32,
    /// The outset this node earned from its resolved [`HitSlop`], in screen dp.
    /// A candidate is only ever reported when `distance <= outset`.
    pub outset: f32,
}

/// Everything a hit test needs to know that the arena cannot work out alone:
/// which pointer is asking, which token ladder is in force, which way the UI
/// reads, and which nodes refuse edits.
///
/// Built once per hit test and passed by reference through the recursion.
/// [`HitContext::mouse`] is the pointer-less door — an exact mouse hit test on
/// the Compact ladder, which is what every call site that predates the touch
/// programme has always meant.
pub struct HitContext<'a> {
    kind: PointerKind,
    tokens: &'a InputTokens,
    direction: LayoutDirection,
    slop: HitSlop,
    read_only: Option<&'a dyn Fn(WidgetId) -> bool>,
}

impl std::fmt::Debug for HitContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HitContext")
            .field("kind", &self.kind)
            .field("density", &self.tokens.density)
            .field("direction", &self.direction)
            .field("slop", &self.slop)
            .field("has_read_only_probe", &self.read_only.is_some())
            .finish()
    }
}

impl<'a> HitContext<'a> {
    /// A hit test on behalf of `kind` against `tokens`, left-to-right, with no
    /// read-only probe.
    pub fn new(kind: PointerKind, tokens: &'a InputTokens) -> Self {
        Self {
            kind,
            tokens,
            direction: LayoutDirection::LeftToRight,
            slop: HitSlop::for_pointer(kind, tokens),
            read_only: None,
        }
    }

    /// The pointer-less door: **mouse, exact**, Compact ladder.
    ///
    /// This is what [`WidgetArena::hit_test_at`](crate::arena::WidgetArena::hit_test_at)
    /// and the inspector's picker use. The mouse's slop radius is `0.0` at every
    /// density, so the ladder choice is unobservable — it matters only to a
    /// widget that opts a *precise* pointer into a hit outset.
    pub fn mouse() -> HitContext<'static> {
        HitContext::new(PointerKind::Mouse, &COMPACT_TOKENS)
    }

    /// Set the reading direction, so a widget's `leading` / `trailing` hit
    /// outsets land on the right screen edges.
    pub fn direction(mut self, direction: LayoutDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Install a probe the slop pass calls to ask whether a node refuses edits.
    ///
    /// A read-only surface is never a slop candidate, and "read-only" is a
    /// question only [`TextSurface`](crate::text_surface::TextSurface) can
    /// answer — the registry lives on the `WidgetTree`, not the arena, so the
    /// tree hands the arena a probe rather than the arena reaching for state it
    /// does not own.
    pub fn read_only_probe(mut self, probe: &'a dyn Fn(WidgetId) -> bool) -> Self {
        self.read_only = Some(probe);
        self
    }

    /// The pointer kind this test serves.
    pub fn kind(&self) -> PointerKind {
        self.kind
    }

    /// The token ladder in force.
    pub fn tokens(&self) -> &InputTokens {
        self.tokens
    }

    /// The reading direction.
    pub fn layout_direction(&self) -> LayoutDirection {
        self.direction
    }

    /// The density default slop for this pointer — the last link of the
    /// precedence chain.
    pub fn default_slop(&self) -> HitSlop {
        self.slop
    }

    /// Whether the miss-only pass can produce anything at all. `false` for a
    /// mouse, which is what keeps the historical path free of new work.
    pub fn slop_enabled(&self) -> bool {
        !self.slop.is_none()
    }

    /// Ask the installed probe whether `id` refuses edits. `false` when no
    /// probe was installed.
    pub fn is_read_only(&self, id: WidgetId) -> bool {
        self.read_only.map(|p| p(id)).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests;
