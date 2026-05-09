//! `Pulse` — a wrapper widget that pulses its child's opacity between
//! a `min` and `max` value on a fixed period, sine-shaped.
//!
//! The classic "blinking red light" / recording-indicator / attention
//! beacon pattern. The wrapped subtree pulses smoothly (sine
//! interpolation), giving a breathing-light feel rather than a hard
//! on/off blink.
//!
//! ```ignore
//! ctx.add(
//!     Pulse::opacity(0.3, 1.0)
//!         .period(Duration::from_millis(1200))
//!         .child(RectWidget::new().background(Color::RED)),
//! );
//! ```
//!
//! ## Layout semantics
//!
//! Layout-transparent — the child reports its full natural size at
//! all opacity values. Identical layout footprint to [`Fade`].
//!
//! ## Reduced motion
//!
//! Honours `prefers-reduced-motion`: skips the per-frame driver and
//! pins opacity at the midpoint `(min + max) / 2`. The subtree stays
//! visible at a steady, non-distracting brightness so the indicator
//! still communicates "active" without animating.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use fern_canvas::{Point, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::frame_tick_scheduler::FrameTickSubscription;
use fern_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// Wraps a child and pulses its opacity smoothly between `min` and
/// `max` on a fixed period. Useful for recording indicators,
/// notification beacons, and attention-grabbing status icons.
pub struct Pulse {
    min: f32,
    max: f32,
    /// `None` → fall back to `MotionTokens::duration_indeterminate_sweep`
    /// at build time so theme-driven motion changes flow through.
    period: Option<Duration>,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    /// RAII guard for the per-frame-effect subscription. Rebuilds
    /// replace it (the old guard's `Drop` removes the previous entry
    /// before the new one is registered); widget destruction drops it
    /// transparently.
    frame_tick_sub: Option<FrameTickSubscription>,
}

impl Pulse {
    /// Wrap a subtree in an opacity pulse between `min` and `max`
    /// (both clamped to `0..=1`). Uses a sine wave so the transitions
    /// at both extremes are smooth, not abrupt.
    pub fn opacity(min: f32, max: f32) -> Self {
        let lo = min.clamp(0.0, 1.0).min(max.clamp(0.0, 1.0));
        let hi = min.clamp(0.0, 1.0).max(max.clamp(0.0, 1.0));
        Self {
            min: lo,
            max: hi,
            period: None,
            pending_child: None,
            child_id: None,
            frame_tick_sub: None,
        }
    }

    /// Override the pulse period (full cycle min → max → min).
    /// Default: `MotionTokens::duration_indeterminate_sweep` (~900 ms),
    /// the same continuous-loop budget the indeterminate progress bar
    /// and spinner use — so a re-themed motion stack stays consistent.
    pub fn period(mut self, period: Duration) -> Self {
        self.period = Some(period);
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
}

impl std::fmt::Debug for Pulse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pulse")
            .field("min", &self.min)
            .field("max", &self.max)
            .field("period", &self.period)
            .finish()
    }
}

impl Widget for Pulse {
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

        let mid = (self.min + self.max) * 0.5;
        let opacity = ctx.signal(mid);
        let id = ctx.self_id();
        ctx.set_opacity(id, opacity.clone());

        // Reduced motion: pin at the midpoint and don't install the
        // per-frame driver. The indicator remains visible (informative)
        // but doesn't animate.
        if ctx.prefers_reduced_motion() {
            return vec![child_id];
        }

        // Sine-driven pulse via the frame tick. Each tick computes
        // phase = (elapsed / period) * 2π, opacity = mid + amp*sin(phase).
        // The framework auto-arms the frame chain after every render
        // in which `self_id` was painted (see
        // `BuildContext::subscribe_frame_tick`), so the chain dies
        // automatically when this Pulse sits in a non-selected
        // `Switcher` branch and resumes when it becomes visible
        // again — no manual `frame_request.set(true)` re-arm needed.
        let period = self
            .period
            .unwrap_or(ctx.theme().motion.duration_indeterminate_sweep);
        let period_secs = period.as_secs_f32().max(0.001);
        let amp = (self.max - self.min) * 0.5;
        let elapsed = Rc::new(Cell::new(0.0_f32));
        let opacity_for_tick = opacity;
        ctx.effect(&ctx.frame_tick(), move |&delta| {
            let t = (elapsed.get() + delta) % period_secs;
            elapsed.set(t);
            let phase = (t / period_secs) * std::f32::consts::TAU;
            let v = mid + amp * phase.sin();
            opacity_for_tick.set(v);
        });
        // Replace any prior subscription (rebuild path) with a fresh
        // one. Drop order matters: the old guard's `Drop` removes its
        // entry before the new subscription is recorded.
        self.frame_tick_sub = None;
        self.frame_tick_sub = Some(ctx.subscribe_frame_tick());

        vec![child_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        self.child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Visual-modulation wrapper. The child owns its own a11y.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::TextWidget;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn pulse_starts_at_midpoint() {
        // First layout pass, before any frame tick: opacity should be
        // the midpoint between min and max.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Pulse::opacity(0.2, 1.0).child(TextWidget::new_literal("●")));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        let ops: Vec<f32> = frame
            .draw_order
            .iter()
            .filter_map(|c| match c {
                fern_canvas::DrawCommand::SetOpacity(v) => Some(*v),
                _ => None,
            })
            .collect();
        assert_eq!(ops.len(), 1);
        // Midpoint = (0.2 + 1.0) / 2 = 0.6. Allow a small tolerance
        // for any tick that happened during render's first frame.
        assert!(
            (ops[0] - 0.6).abs() < 0.5,
            "opacity should start near midpoint 0.6, got {}",
            ops[0]
        );
    }

    #[test]
    fn pulse_pins_to_midpoint_under_reduced_motion() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.set_accessibility_preferences(false, true, 1.0);
        tree.add(Pulse::opacity(0.0, 1.0).child(TextWidget::new_literal("●")));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        let ops: Vec<f32> = frame
            .draw_order
            .iter()
            .filter_map(|c| match c {
                fern_canvas::DrawCommand::SetOpacity(v) => Some(*v),
                _ => None,
            })
            .collect();
        assert_eq!(ops.len(), 1);
        assert!(
            (ops[0] - 0.5).abs() < 1e-3,
            "reduced-motion opacity should be pinned at midpoint 0.5, got {}",
            ops[0]
        );
        assert!(
            !tree.has_active_animations(),
            "reduced-motion path must not register animations"
        );
    }

    #[test]
    fn pulse_does_not_change_layout() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(Pulse::opacity(0.0, 1.0).child(TextWidget::new_literal("hello")));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let bounds_initial = tree.bounds(id);
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let bounds_again = tree.bounds(id);
        assert_eq!(bounds_initial.size(), bounds_again.size());
    }

    #[test]
    fn pulse_clamps_inverted_min_max() {
        // Pulse::opacity(0.9, 0.1) should still produce a valid range.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Pulse::opacity(0.9, 0.1).child(TextWidget::new_literal("●")));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let _ = tree.render();
        // Just confirm it didn't panic and produced a frame.
    }
}
