//! `Shake` — wraps a child and plays a damped horizontal oscillation
//! whenever an external trigger `Signal<u32>` is bumped. The classic
//! invalid-input feedback: wrong password, failed form validation,
//! "no more results" wall.
//!
//! ```ignore
//! let shake_trigger = ctx.signal(0_u32);
//! ctx.add(
//!     Shake::new(shake_trigger.clone())
//!         .child(text_input_field),
//! );
//! // ...elsewhere, on validation failure:
//! shake_trigger.set(shake_trigger.get() + 1);
//! ```
//!
//! ## Layout semantics
//!
//! Layout-stable: the wrapper reports the child's full natural size
//! and clips the oscillating-out-of-bounds excursions on each side.
//! Siblings don't reflow. The shake is a pure visual offset.
//!
//! ## Reduced motion
//!
//! Honours `prefers-reduced-motion`: the trigger no-ops. The widget
//! is still focusable / interactive — the visual feedback just
//! doesn't play. Pair with another a11y-friendly cue (red border,
//! error text) when error state must be communicated.

use std::cell::Cell;
use std::time::Duration;

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::Easing;

const DEFAULT_AMPLITUDE: f32 = 8.0;
const DEFAULT_CYCLES: f32 = 4.0;

/// Wraps a child and plays a damped horizontal-oscillation shake
/// each time the trigger signal value changes.
pub struct Shake {
    trigger: Signal<u32>,
    amplitude: f32,
    /// `None` → fall back to `MotionTokens::duration_slow` at build
    /// time so a re-themed motion stack flows through.
    duration: Option<Duration>,
    cycles: f32,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    /// Linear 0..1 progress driving the shake. `Cell` is fine because
    /// it's paired with the framework's signal; the natural_size cell
    /// follows the same pattern as Slide / Collapse.
    progress: Option<Signal<f32>>,
    natural_size: Cell<Size>,
}

impl Shake {
    /// Build a shake wrapper. Bumping `trigger` (any new value) plays
    /// one shake cycle.
    pub fn new(trigger: Signal<u32>) -> Self {
        Self {
            trigger,
            amplitude: DEFAULT_AMPLITUDE,
            duration: None,
            cycles: DEFAULT_CYCLES,
            pending_child: None,
            child_id: None,
            progress: None,
            natural_size: Cell::new(Size::ZERO),
        }
    }

    /// Peak horizontal offset in logical pixels. Default 8 px.
    pub fn amplitude(mut self, px: f32) -> Self {
        self.amplitude = px.max(0.0);
        self
    }

    /// Override the total shake duration. Default:
    /// `MotionTokens::duration_slow` (~300 ms) — the same one-shot
    /// "this should feel deliberate" budget dialogs use.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Number of full back-and-forth oscillations within `duration`.
    /// Default 4 cycles. Higher = jitterier; lower = wobblier.
    pub fn cycles(mut self, cycles: f32) -> Self {
        self.cycles = cycles.max(0.5);
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

impl std::fmt::Debug for Shake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shake")
            .field("amplitude", &self.amplitude)
            .field("duration", &self.duration)
            .field("cycles", &self.cycles)
            .finish()
    }
}

impl Widget for Shake {
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

        // 1.0 = at-rest (no shake offset). The shake formula maps
        // (1-t) * sin(...) so progress=1 → zero offset, progress=0 →
        // peak amplitude (start of the oscillation).
        let progress = ctx.animated_signal(1.0);
        self.progress = Some(progress.clone());

        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        progress.bind_to(id, registry, BindingLevel::Relayout);

        // Reduced motion: never start a shake. The trigger still
        // increments freely on the caller side, just no visual play.
        if ctx.prefers_reduced_motion() {
            return vec![child_id];
        }

        let duration = self.duration.unwrap_or(ctx.theme().motion.duration_slow);
        let progress_for_effect = progress;
        ctx.effect(&self.trigger, move |_| {
            // Restart from 0 each time, even if the previous shake
            // hadn't completed. Uses Linear easing so the per-tick
            // value is a true elapsed-fraction — the damped sine in
            // place_children does the visual shape.
            progress_for_effect.set(0.0);
            progress_for_effect.animate_to(1.0, duration, Easing::Linear);
        });

        vec![child_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        let Some(child_id) = self.child_id else {
            return (proposal.resolve(0.0, 0.0)).into();
        };
        let natural = ctx.child_size(child_id, proposal).unwrap_or(Size::ZERO);
        self.natural_size.set(natural);
        natural.into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let t = self
            .progress
            .as_ref()
            .map(|s| s.get().clamp(0.0, 1.0))
            .unwrap_or(1.0);
        // Damped sine: amplitude tapers linearly to 0 over [0, 1].
        let dx = if t >= 1.0 {
            0.0
        } else {
            let envelope = 1.0 - t;
            let phase = t * self.cycles * std::f32::consts::TAU;
            self.amplitude * envelope * phase.sin()
        };
        let natural = self.natural_size.get();
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x + dx, bounds.y);
            child.size = natural;
        }
    }

    fn clips_children(&self) -> bool {
        // Required: the oscillation pushes the child past the
        // wrapper's bounds during the shake; clip so it doesn't
        // overlap siblings.
        true
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Visual-only feedback wrapper. The child owns its own a11y.
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

    #[test]
    fn shake_starts_at_rest() {
        let trigger = Signal::new(0_u32);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(Shake::new(trigger).child(TextWidget::new_literal("oops")));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        assert!(
            !tree.has_active_animations(),
            "no animation until the trigger is bumped"
        );
    }

    #[test]
    fn bumping_trigger_starts_shake() {
        let trigger = Signal::new(0_u32);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(Shake::new(trigger.clone()).child(TextWidget::new_literal("oops")));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });

        trigger.set(1);
        tree.tick_animations(Duration::from_millis(50));
        assert!(
            tree.has_active_animations(),
            "shake should be in flight after trigger bump"
        );
    }

    #[test]
    fn shake_completes() {
        let trigger = Signal::new(0_u32);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(Shake::new(trigger.clone()).child(TextWidget::new_literal("oops")));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        trigger.set(1);
        // Tick well past the default 400ms duration — animation must
        // have completed and the scheduler dropped it.
        tree.tick_animations(Duration::from_millis(600));
        assert!(
            !tree.has_active_animations(),
            "shake should have completed after its duration"
        );
    }

    #[test]
    fn reduced_motion_swallows_trigger() {
        let trigger = Signal::new(0_u32);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.set_accessibility_preferences(false, true, 1.0);
        tree.add(Shake::new(trigger.clone()).child(TextWidget::new_literal("oops")));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });

        trigger.set(1);
        assert!(
            !tree.has_active_animations(),
            "reduced-motion path must not register animations"
        );
    }
}
