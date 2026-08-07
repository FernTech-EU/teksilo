// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `SmoothSize` — auto-sizes the slot to fit the child's intrinsic
//! size, but tweens the change instead of jumping. The "empty panel
//! that suddenly must grow gracefully to accept new content" pattern.
//!
//! ```ignore
//! ctx.add(
//!     SmoothSize::new()
//!         .axes(SmoothSizeAxes::Both)
//!         .child(Panel::new().child(content_signal)),
//! );
//! ```
//!
//! For *explicit* size animation (target is a numeric signal you
//! already drive, e.g. a sidebar width), use the existing
//! `FixedSize::new().width(animated_signal)` + `Signal::animate_to`
//! pattern instead — that path doesn't need to measure the child every
//! frame.
//!
//! ## Layout semantics
//!
//! - The wrapper measures the child's natural size at the proposal
//!   each layout pass.
//! - When the natural size differs from the current animation target
//!   (above 0.5 px), kicks off a new tween.
//! - `size_that_fits` returns the *current animated value* — what the
//!   wrapper actually occupies right now, not the target.
//! - The child is always laid out at its full natural size and clipped
//!   to the wrapper's smaller animated bounds. Same trick as
//!   [`Collapse`](super::Collapse) — the child's own internal layout
//!   doesn't reflow each frame, only the clip rect changes.
//!
//! ## Reduced motion
//!
//! Honours `prefers-reduced-motion`: snaps to the natural size each
//! layout pass instead of tweening.

use std::cell::Cell;
use std::time::Duration;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::Easing;

/// Which axes participate in the size tween. Use `Width` or `Height`
/// to leave the other axis tracking the child's natural size
/// instantly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmoothSizeAxes {
    /// Animate width changes only; height snaps to natural immediately.
    Width,
    /// Animate height changes only; width snaps to natural immediately.
    Height,
    /// Animate both width and height changes. Default.
    Both,
}

const SIZE_CHANGE_EPSILON: f32 = 0.5;

/// Wraps a child widget and animates the wrapper's reported size toward
/// the child's current natural size whenever that size changes.
pub struct SmoothSize {
    axes: SmoothSizeAxes,
    duration: Option<Duration>,
    easing: Option<Easing>,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    /// Animated current width. `BuildContext::animated_signal` —
    /// scheduler-registered, bound to self at Relayout level.
    width_anim: Option<Signal<f32>>,
    /// Animated current height. Same shape as `width_anim`.
    height_anim: Option<Signal<f32>>,
    /// Last natural size we kicked off a tween toward. Stored so a
    /// fresh `size_that_fits` call only initiates a new animation
    /// when the child's measure has actually changed.
    last_target: Cell<Size>,
    /// Latest measured natural size. `place_children` reads it so the
    /// child is laid out at full natural dimensions (the framework
    /// clips the overflow against the wrapper's animated bounds).
    natural_size: Cell<Size>,
    /// Reduced-motion snapshot taken at build(). Skips the tween path
    /// and snaps both signals straight to the new natural each frame.
    reduced_motion: bool,
    /// `true` until the first natural-size measurement. The first
    /// measurement *snaps* the size signals — without this guard the
    /// wrapper would visibly animate from 0×0 up to the child's
    /// natural size every time it first appears.
    needs_initial_snap: Cell<bool>,
}

impl SmoothSize {
    /// New wrapper. Both axes animate by default.
    pub fn new() -> Self {
        Self {
            axes: SmoothSizeAxes::Both,
            duration: None,
            easing: None,
            pending_child: None,
            child_id: None,
            width_anim: None,
            height_anim: None,
            last_target: Cell::new(Size::ZERO),
            natural_size: Cell::new(Size::ZERO),
            reduced_motion: false,
            needs_initial_snap: Cell::new(true),
        }
    }

    /// Restrict the tween to one axis (the other tracks the child's
    /// natural size instantly).
    pub fn axes(mut self, axes: SmoothSizeAxes) -> Self {
        self.axes = axes;
        self
    }

    /// Override the tween duration. Default: `MotionTokens::duration_normal`.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Override the easing curve. Default: `MotionTokens::easing_standard`.
    pub fn easing(mut self, easing: Easing) -> Self {
        self.easing = Some(easing);
        self
    }

    /// Inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Pre-registered child by `WidgetId`.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    fn animates_width(&self) -> bool {
        matches!(self.axes, SmoothSizeAxes::Width | SmoothSizeAxes::Both)
    }

    fn animates_height(&self) -> bool {
        matches!(self.axes, SmoothSizeAxes::Height | SmoothSizeAxes::Both)
    }
}

impl Default for SmoothSize {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SmoothSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmoothSize")
            .field("axes", &self.axes)
            .field("duration", &self.duration)
            .finish()
    }
}

impl Widget for SmoothSize {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        let Some(child_id) = self.child_id else {
            return vec![];
        };

        // Both signals exist whether or not the axis animates — keeps
        // size_that_fits branch-free. For pinned axes we just skip
        // the animate_to call.
        let w_sig = ctx.animated_signal(0.0);
        let h_sig = ctx.animated_signal(0.0);

        // Bind both to self at Relayout level so each animation tick
        // triggers a fresh size_that_fits / place_children pass.
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        w_sig.bind_to(id, registry, BindingLevel::Relayout);
        h_sig.bind_to(id, registry, BindingLevel::Relayout);

        self.width_anim = Some(w_sig);
        self.height_anim = Some(h_sig);
        self.reduced_motion = ctx.prefers_reduced_motion();

        vec![child_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        let Some(child_id) = self.child_id else {
            return (proposal.resolve(0.0, 0.0)).into();
        };
        let natural = ctx.child_size(child_id, proposal).unwrap_or(Size::ZERO);
        self.natural_size.set(natural);

        let (Some(w_sig), Some(h_sig)) = (self.width_anim.as_ref(), self.height_anim.as_ref())
        else {
            // build() hasn't run yet — fall back to natural size.
            return (natural).into();
        };

        let last = self.last_target.get();
        let width_target_changed = (natural.width - last.width).abs() > SIZE_CHANGE_EPSILON;
        let height_target_changed = (natural.height - last.height).abs() > SIZE_CHANGE_EPSILON;

        if width_target_changed || height_target_changed {
            self.last_target.set(natural);
            // First measurement snaps. Reduced motion always snaps.
            // Otherwise tween only the axes that actually changed and
            // are configured to animate.
            let snap = self.reduced_motion || self.needs_initial_snap.get();
            self.needs_initial_snap.set(false);
            if snap {
                w_sig.set(natural.width);
                h_sig.set(natural.height);
            } else {
                let duration = self.duration.unwrap_or(ctx.theme.motion.duration_normal);
                let easing = self.easing.unwrap_or(ctx.theme.motion.easing_standard);
                if self.animates_width() && width_target_changed {
                    w_sig.animate_to(natural.width, duration, easing);
                } else if !self.animates_width() {
                    w_sig.set(natural.width);
                }
                if self.animates_height() && height_target_changed {
                    h_sig.animate_to(natural.height, duration, easing);
                } else if !self.animates_height() {
                    h_sig.set(natural.height);
                }
            }
        }

        Size::new(w_sig.get().max(0.0), h_sig.get().max(0.0)).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Lay the child out at its FULL natural size — let the
        // framework's clip pass crop the overflow against the
        // wrapper's smaller animated bounds. Same trick as Collapse.
        let natural = self.natural_size.get();
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = natural;
        }
    }

    fn clips_children(&self) -> bool {
        // Required: child laid out at natural, wrapper bounds are
        // smaller during the tween.
        true
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Layout-animation wrapper. The child owns its own a11y.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{FixedSize, TextWidget};
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    #[test]
    fn first_measurement_snaps_to_natural_no_grow_in_animation() {
        // Regression: SmoothSize used to animate from 0 → natural on
        // its very first appearance, producing a visible "grow from
        // nothing" glitch. The first measurement must snap to the
        // child's natural size; only *changes* should tween.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            SmoothSize::new()
                .duration(Duration::from_millis(500))
                .child(FixedSize::new().width(180.0).height(70.0)),
        );
        // Single layout, NO animation tick: the wrapper must already
        // report (180, 70), not 0×0 or anything mid-tween.
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let b = tree.bounds(id);
        assert!(
            (b.width - 180.0).abs() < 0.5 && (b.height - 70.0).abs() < 0.5,
            "first-frame size must equal natural; got ({}, {})",
            b.width,
            b.height
        );
        assert!(
            !tree.has_active_animations(),
            "first-frame snap must not register an animation"
        );
    }

    #[test]
    fn subsequent_change_animates() {
        // After the initial snap, a change in the child's natural
        // size (here driven through a Signal-bound FixedSize) must
        // trigger an in-flight animation rather than another snap.
        let width_signal = Signal::new(100.0_f32);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            SmoothSize::new()
                .duration(Duration::from_millis(200))
                .child(FixedSize::new().width(width_signal.clone()).height(50.0)),
        );
        // Initial snap to 100×50.
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let initial = tree.bounds(id);
        assert!((initial.width - 100.0).abs() < 0.5);

        // Bump the child's intrinsic width.
        width_signal.set(250.0);
        // First layout: SmoothSize sees the new natural and queues
        // an animate_to on its width signal.
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        // Second layout: process_pending_animations drains the queued
        // request into the scheduler. Bounds remain at the *current*
        // animated value (still close to 100, not yet 250).
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let mid = tree.bounds(id);
        assert!(
            tree.has_active_animations(),
            "size change must kick off a tween (got bounds {:?})",
            mid
        );
        assert!(
            mid.width < 240.0,
            "mid-tween width should still be near the start, got {}",
            mid.width
        );

        // Tick to completion.
        tree.tick_animations(Duration::from_millis(250));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let final_b = tree.bounds(id);
        assert!(
            (final_b.width - 250.0).abs() < 1.0,
            "after tween, width should reach 250; got {}",
            final_b.width
        );
    }

    #[test]
    fn reduced_motion_snaps_to_natural() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.set_accessibility_preferences(false, true, 1.0);
        let id = tree.add(SmoothSize::new().child(FixedSize::new().width(150.0).height(60.0)));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        // First layout pass: width_anim/height_anim = 0 still get set
        // to natural via the snap path. A second layout reads the new
        // values.
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let b = tree.bounds(id);
        assert!(
            (b.width - 150.0).abs() < 0.5 && (b.height - 60.0).abs() < 0.5,
            "expected (150, 60), got ({}, {})",
            b.width,
            b.height
        );
        assert!(
            !tree.has_active_animations(),
            "reduced-motion path must not register animations"
        );
    }

    #[test]
    fn empty_smooth_size_is_safe() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(SmoothSize::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let _ = tree.render();
    }

    #[test]
    fn axes_width_only_pins_height() {
        // With axes=Width, height should snap to natural; width animates.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            SmoothSize::new()
                .axes(SmoothSizeAxes::Width)
                .duration(Duration::from_millis(100))
                .child(TextWidget::new(lit!("hi"))),
        );
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        tree.tick_animations(Duration::from_millis(150));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let b = tree.bounds(id);
        assert!(b.height > 0.0);
    }
}
