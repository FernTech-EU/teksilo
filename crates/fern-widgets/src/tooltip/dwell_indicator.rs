//! Visual dwell indicator for the rich-tooltip sticky-on-dwell UX.
//!
//! Sits in the top-right corner of a rich tooltip surface and reports
//! the dwell-promotion progress.
//!
//! Visual states:
//! - **step 0**: empty circle outline — pointer just entered the
//!   tooltip, no dwell yet.
//! - **step 1**: 25% pie wedge filled (12 o'clock → 3 o'clock).
//! - **step 2**: 50% wedge.
//! - **step 3**: 75% wedge.
//! - **step 4 / sticky**: a small filled pin icon (head + tail) — the
//!   tooltip has been promoted to sticky and won't auto-dismiss.
//!
//! Driven by two reactive signals supplied by `RichTooltipWidget`:
//! - `Signal<u32>` step (clamped 0..=4)
//! - `Signal<bool>` sticky — set when the tooltip has been promoted

use fern_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, PaintContext, Widget};
use fern_core::widget_id::WidgetId;
use fern_tokens::Color;

const DWELL_INDICATOR_SIZE: f32 = 14.0;

/// Top-right dwell indicator for a rich tooltip. Owned by
/// `RichTooltipWidget`; not exposed as a public API.
pub(crate) struct DwellIndicator {
    step: Signal<u32>,
    sticky: Signal<bool>,
    color: Color,
}

impl std::fmt::Debug for DwellIndicator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DwellIndicator")
            .field("step", &self.step.get())
            .field("sticky", &self.sticky.get())
            .finish()
    }
}

impl DwellIndicator {
    pub(crate) fn new(step: Signal<u32>, sticky: Signal<bool>, color: Color) -> Self {
        Self {
            step,
            sticky,
            color,
        }
    }
}

impl Widget for DwellIndicator {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Re-paint when either signal changes. Both are RepaintOnly —
        // the indicator's bounding box never changes shape, only its
        // contents.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.step
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        self.sticky
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        Vec::new()
    }

    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        Size::new(DWELL_INDICATOR_SIZE, DWELL_INDICATOR_SIZE).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        let center = Point::new(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        );
        let radius = (bounds.width.min(bounds.height) / 2.0) - 1.0;
        let color = self.color;
        let sticky = self.sticky.get();

        if sticky {
            // Pin icon: small filled circle (head) + downward triangle
            // (tail). Sized to fit inside the indicator slot.
            let head_r = radius * 0.55;
            let head_center = Point::new(center.x, center.y - radius * 0.15);
            canvas.fill_circle(head_center, head_r, color);

            let tail_top_y = head_center.y + head_r * 0.4;
            let tail_bottom = Point::new(center.x, center.y + radius * 0.95);
            let mut tail = Path::new();
            tail.move_to(Point::new(center.x - head_r * 0.55, tail_top_y));
            tail.line_to(Point::new(center.x + head_r * 0.55, tail_top_y));
            tail.line_to(tail_bottom);
            tail.close();
            canvas.fill_path(&tail, color);
            return;
        }

        // Non-sticky: empty circle outline + pie wedge for the
        // current dwell step. Step is clamped 0..=4.
        let step = self.step.get().min(4);

        canvas.stroke_circle(
            center,
            radius,
            color,
            fern_canvas::paint::StrokeStyle::solid(1.5),
        );

        if step == 0 {
            return;
        }

        // Build a pie wedge from 12 o'clock (top), sweeping clockwise
        // by `step * 90` degrees. `Path::arc_to` takes the inscribing
        // rect and start/sweep angles in degrees; canvas draws angle
        // 0° at the right (3 o'clock), so we offset by -90° to start
        // at the top.
        let inscribed = Rect::new(
            center.x - radius,
            center.y - radius,
            radius * 2.0,
            radius * 2.0,
        );
        let start_angle = -90.0;
        let sweep_angle = (step as f32) * 90.0;
        let mut wedge = Path::new();
        wedge.move_to(center);
        wedge.arc_to(inscribed, start_angle, sweep_angle);
        wedge.close();
        canvas.fill_path(&wedge, color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The indicator is decorative — give it a generic role so
        // screen readers don't announce it as content. The tooltip's
        // own a11y node carries the meaningful content.
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }
}
