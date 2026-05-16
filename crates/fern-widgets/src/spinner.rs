//! `Spinner` — a shader-driven circular-arc loading indicator.
//!
//! Uses the same per-slot uniform-buffer pipeline as
//! [`ProgressBar::indeterminate`](crate::ProgressBar) (an
//! [`AnimatedQuadKind`] variant), so per-frame cost is one
//! `queue.write_buffer(64 B) + draw_indexed` — the widget's `paint()`
//! does not re-run between frames and there's no signal-dirty-mark
//! cascade.
//!
//! ```ignore
//! ctx.add(
//!     Spinner::new(24.0)
//!         .color(TextRole::Secondary)
//!         .label("Loading"),
//! );
//! ```
//!
//! Defaults match the typical CSS spinner: a quarter-circle (90°)
//! arc rotating clockwise from the top, completing one full
//! rotation every 900 ms.
//!
//! Honours `prefers-reduced-motion`: registers no animated quad and
//! falls back to a static three-quarter arc — the indicator is still
//! visible (so the user can tell the surface is busy) but doesn't
//! rotate.

use std::time::Duration;

use fern_canvas::{AnimatedQuadClass, Canvas, Path, Rect, Size, SizeProposal, StrokeStyle};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::animated_quad::{AnimatedQuadHandle, AnimatedQuadKind};
use fern_core::color_prop::ColorProp;
use fern_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::TextRole;

const DEFAULT_SIZE: f32 = 20.0;
const DEFAULT_PERIOD: Duration = Duration::from_millis(900);
const DEFAULT_ARC_FRACTION: f32 = 0.25;
const DEFAULT_STROKE_FRACTION: f32 = 0.12;

/// A circular-arc loading indicator. Decorative; pair with `.label`
/// for screen readers.
pub struct Spinner {
    size: f32,
    period: Duration,
    arc_fraction: f32,
    stroke_fraction: f32,
    color: ColorProp,
    label: Option<String>,
    handle: Option<AnimatedQuadHandle>,
}

impl Spinner {
    /// Construct a spinner of the given square edge length (logical
    /// pixels). Use small sizes (16–24) for inline spinners and
    /// larger (32–64) for full-content placeholders.
    pub fn new(size: f32) -> Self {
        Self {
            size,
            period: DEFAULT_PERIOD,
            arc_fraction: DEFAULT_ARC_FRACTION,
            stroke_fraction: DEFAULT_STROKE_FRACTION,
            color: TextRole::Secondary.into(),
            label: None,
            handle: None,
        }
    }

    /// Override the rotation period. Default: 900 ms (one full
    /// rotation per period).
    pub fn period(mut self, period: Duration) -> Self {
        self.period = period;
        self
    }

    /// Override the arc length as a fraction of the full circle.
    /// Default: 0.25 (a quarter-circle "comet tail" arc).
    pub fn arc_fraction(mut self, arc_fraction: f32) -> Self {
        self.arc_fraction = arc_fraction.clamp(0.0, 1.0);
        self
    }

    /// Override the stroke thickness as a fraction of the spinner's
    /// edge length. Default: 0.12 (so a 24-px spinner has a ~3-px
    /// stroke).
    pub fn stroke_fraction(mut self, stroke_fraction: f32) -> Self {
        self.stroke_fraction = stroke_fraction.clamp(0.0, 0.5);
        self
    }

    /// Override the arc colour. Default: `TextRole::Secondary` so the
    /// spinner picks up theme-aware text-tier styling.
    pub fn color(mut self, color: impl Into<ColorProp>) -> Self {
        self.color = color.into();
        self
    }

    /// Accessible name (e.g. "Loading", "Uploading file"). Without
    /// this, screen readers announce a bare "progress indicator"
    /// with no context.
    pub fn label(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `label(...)` accepting
    /// a raw string.
    #[doc(hidden)]
    pub fn label_literal(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }
}

impl std::fmt::Debug for Spinner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spinner")
            .field("size", &self.size)
            .field("period", &self.period)
            .finish()
    }
}

impl Widget for Spinner {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        // Skip the shader registration entirely under reduced motion
        // — the static fallback in `paint()` doesn't need a slot, and
        // the registry not having an entry stops the per-frame phase
        // tick.
        if ctx.prefers_reduced_motion() {
            self.handle = None;
        } else {
            self.handle = Some(ctx.animated_quad(AnimatedQuadKind::SpinnerArc {
                period: self.period,
                arc_fraction: self.arc_fraction,
                stroke_fraction: self.stroke_fraction,
                color: self.color.clone(),
            }));
        }
        vec![]
    }

    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        Size::new(self.size, self.size).into()
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        if let Some(handle) = self.handle {
            // Animated path: emit a single AnimatedQuad. The shader
            // computes the arc each frame from the per-slot phase.
            canvas.draw_animated_quad(bounds, handle.slot(), AnimatedQuadClass::Procedural);
        } else {
            // Reduced-motion fallback: draw a static three-quarter
            // arc, leading edge at the top. Communicates "busy"
            // without rotating.
            let color = self.color.resolve(ctx.theme);
            let extent = bounds.width.min(bounds.height);
            let stroke_w = extent * self.stroke_fraction;
            // Inscribe inside the bounds, leaving room for the stroke
            // so it doesn't get clipped.
            let inset = stroke_w * 0.5;
            let inscribed = Rect::new(
                bounds.x + inset,
                bounds.y + inset,
                bounds.width - inset * 2.0,
                bounds.height - inset * 2.0,
            );
            let mut path = Path::new();
            // Path::arc_to angles: 0° at 3 o'clock; offset -90° to
            // start at the top. Sweep the arc clockwise.
            path.arc_to(inscribed, -90.0, self.arc_fraction * 360.0);
            canvas.stroke_path(&path, color, StrokeStyle::solid(stroke_w));
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::ProgressIndicator);
        // Indeterminate (no numeric value); polite live region so
        // screen readers don't interrupt the user.
        builder.set_live(fern_core::accesskit::Live::Polite);
        if let Some(ref label) = self.label {
            builder.set_name(label.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::DrawCommand;
    use fern_core::widget_tree::WidgetTree;

    #[test]
    fn spinner_size() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let id = tree.add(Spinner::new(32.0));
        // Pass `None` for both axes so the layout pass uses the
        // spinner's natural (square) `size_that_fits` instead of
        // forcing the proposal dimensions.
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let b = tree.bounds(id);
        assert!((b.width - 32.0).abs() < 0.01);
        assert!((b.height - 32.0).abs() < 0.01);
    }

    #[test]
    fn spinner_emits_one_animated_quad_when_motion_allowed() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(Spinner::new(24.0));
        tree.layout(SizeProposal::exact(64.0, 64.0));
        let frame = tree.render();
        assert_eq!(
            frame.animated_quads.len(),
            1,
            "spinner with motion enabled should emit exactly one AnimatedQuad"
        );
        // No path commands — the shader does the rendering.
        let path_count = frame
            .draw_order
            .iter()
            .filter(|c| matches!(c, DrawCommand::Path(_)))
            .count();
        assert_eq!(path_count, 0);
    }

    #[test]
    fn spinner_emits_static_path_under_reduced_motion() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        // high_contrast = false, reduced_motion = true, text_scale = 1.0
        tree.set_accessibility_preferences(false, true, 1.0);
        tree.add(Spinner::new(24.0));
        tree.layout(SizeProposal::exact(64.0, 64.0));
        let frame = tree.render();
        assert_eq!(
            frame.animated_quads.len(),
            0,
            "no animated quad should register when reduced-motion is on"
        );
        let path_count = frame
            .draw_order
            .iter()
            .filter(|c| matches!(c, DrawCommand::Path(_)))
            .count();
        assert!(
            path_count >= 1,
            "reduced-motion fallback should emit at least one Path draw command"
        );
    }

    #[test]
    fn spinner_phase_advances_between_frames() {
        // Same shape as ProgressBar's animation test: the per-slot
        // phase must change between frames so the shader actually
        // animates without paint() re-running.
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(Spinner::new(24.0));
        tree.layout(SizeProposal::exact(64.0, 64.0));

        let frame1 = tree.render();
        assert_eq!(frame1.animated_quads.len(), 1);
        let phase1 = frame1.anim_params[frame1.animated_quads[0].slot as usize].phase;

        std::thread::sleep(Duration::from_millis(100));
        let frame2 = tree.render();
        let phase2 = frame2.anim_params[frame2.animated_quads[0].slot as usize].phase;
        assert_ne!(phase1, phase2, "spinner phase must advance between frames");
    }

    #[test]
    fn accessibility_role_and_live_region() {
        let mut tree = WidgetTree::new();
        let id = tree.add(Spinner::new(24.0).label_literal("Loading"));
        tree.layout(SizeProposal::exact(64.0, 64.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), fern_core::accesskit::Role::ProgressIndicator);
        assert_eq!(info.name(), Some("Loading"));
    }
}
