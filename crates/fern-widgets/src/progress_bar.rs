//! ProgressBar — a bar showing progress from 0.0 to 1.0.
//!
//! Supports determinate mode (bound to a `Prop<f32>`) and indeterminate mode.
//!
//! - **Horizontal indeterminate** uses the shader-driven animated-quad
//!   pipeline: the widget's `paint()` emits one `AnimatedQuad` draw
//!   command, and the fragment shader computes the sweep position
//!   from a uniform updated each frame by the widget tree. Paint()
//!   only re-runs when layout changes — per-frame animation cost is
//!   one uniform write + one `draw_indexed` call.
//! - **Vertical indeterminate** keeps the signal-based animate_looping
//!   path — the procedural shader currently only handles horizontal
//!   sweeps.
//! - **Determinate** uses a static rounded-rect fill (unchanged).

use fern_canvas::{AnimatedQuadClass, Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::animated_quad::{AnimatedQuadHandle, AnimatedQuadKind};
use fern_core::color_prop::ColorProp;
use fern_core::signal::{Prop, Signal};
use fern_core::binding::BindingLevel;
use fern_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
#[cfg(test)]
use fern_tokens::Color;
use fern_tokens::{CornerRadius, Orientation, SurfaceRole};
use std::time::Duration;

const DEFAULT_THICKNESS: f32 = 4.0;
/// ~15 Hz cadence. The indeterminate sweep is a continuous smooth
/// motion; the eye doesn't resolve >15 fps for a 42%-wide bar moving
/// across the viewport in ~0.9 s, and every doubled frame is a full
/// wgpu submit even when only the sweep position changed.
const INDETERMINATE_FRAME_INTERVAL: Duration = Duration::from_millis(66);
const INDETERMINATE_SWEEP_RATIO: f32 = 0.42;

/// A progress bar — determinate or indeterminate, horizontal or vertical.
pub struct ProgressBar {
    value: Prop<f32>,
    indeterminate: bool,
    /// Legacy signal-based sweep position (0.0–1.0). Only used by the
    /// vertical indeterminate path; the horizontal path uses the
    /// shader pipeline via `anim_handle` instead.
    indeterminate_pos: Signal<f32>,
    /// Shader-driven animated-quad handle for horizontal indeterminate
    /// bars. `Some` only when `indeterminate && orientation ==
    /// Horizontal && !prefers_reduced_motion` at the last `build()`.
    anim_handle: Option<AnimatedQuadHandle>,
    orientation: Orientation,
    thickness: f32,
    track_color: Option<ColorProp>,
    fill_color: Option<ColorProp>,
    label: Option<String>,
}

impl ProgressBar {
    /// Create a determinate progress bar with a static value (0.0–1.0).
    pub fn new(value: f32) -> Self {
        Self {
            value: Prop::Static(value.clamp(0.0, 1.0)),
            indeterminate: false,
            indeterminate_pos: Signal::new_animated(0.0),
            anim_handle: None,
            orientation: Orientation::Horizontal,
            thickness: DEFAULT_THICKNESS,
            track_color: None,
            fill_color: None,
            label: None,
        }
    }

    /// Create an indeterminate progress bar (animated sweep).
    ///
    /// The sweep uses a throttled frame cadence and only keeps looping while
    /// the widget continues to be painted. If the progress bar is fully
    /// outside an ancestor clip, the current sweep finishes and then stops.
    pub fn indeterminate() -> Self {
        Self {
            value: Prop::Static(0.0),
            indeterminate: true,
            indeterminate_pos: Signal::new_animated(0.0),
            anim_handle: None,
            orientation: Orientation::Horizontal,
            thickness: DEFAULT_THICKNESS,
            track_color: None,
            fill_color: None,
            label: None,
        }
    }

    /// Bind the progress value to a reactive state.
    pub fn bind_value(mut self, state: impl Into<Prop<f32>>) -> Self {
        self.value = state.into();
        self
    }

    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn thickness(mut self, thickness: f32) -> Self {
        self.thickness = thickness;
        self
    }

    /// Override the track background. Default (unset) is `SurfaceRole::Sunken`.
    /// Accepts `Color`, roles, or `Signal<Color>`.
    pub fn track_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.track_color = Some(color.into());
        self
    }

    /// Override the fill color. Default (unset) is `SurfaceRole::Accent`.
    /// Accepts `Color`, roles, or `Signal<Color>`.
    pub fn fill_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.fill_color = Some(color.into());
        self
    }

    /// Accessible name for the progress bar (e.g. "Uploading files",
    /// "Loading"). Without this, screen readers announce a bare
    /// "progress indicator" with no context.
    pub fn label(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `label(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn label_literal(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }
}

impl std::fmt::Debug for ProgressBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgressBar")
            .field("thickness", &self.thickness)
            .finish()
    }
}

impl Widget for ProgressBar {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        // Honor the OS-level reduced-motion preference: an
        // indeterminate sweep is decorative; when reduced-motion is on,
        // fall through to a static "half-filled" bar rather than
        // running the animation.
        let reduced_motion = ctx.prefers_reduced_motion();
        let animate = self.indeterminate && !reduced_motion;
        let use_shader_path = animate && matches!(self.orientation, Orientation::Horizontal);
        let sweep_period = ctx.theme().motion.duration_indeterminate_sweep;

        // Horizontal indeterminate: register a shader-driven animated
        // quad. paint() below emits ONE DrawCommand::AnimatedQuad; the
        // fragment shader computes the sweep from a uniform updated
        // each frame without re-running paint().
        if use_shader_path {
            let track_color = self
                .track_color
                .clone()
                .unwrap_or_else(|| SurfaceRole::Sunken.into());
            let fill_color = self
                .fill_color
                .clone()
                .unwrap_or_else(|| SurfaceRole::Accent.into());
            self.anim_handle = Some(ctx.animated_quad(AnimatedQuadKind::IndeterminateSweep {
                period: sweep_period,
                sweep_ratio: INDETERMINATE_SWEEP_RATIO,
                track_color,
                fill_color,
            }));
        } else {
            self.anim_handle = None;
        }

        // Vertical indeterminate still uses the signal path — the
        // procedural shader only draws horizontal sweeps today.
        if animate && !use_shader_path {
            self.indeterminate_pos = ctx.animated_signal(0.0);
        }

        // Register bindings
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.value
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        if animate && !use_shader_path {
            self.indeterminate_pos
                .bind_to(id, registry, BindingLevel::RepaintOnly);
            // `sweep()` reads `duration_indeterminate_sweep` from
            // theme motion tokens AND switches the spec to looping
            // mode with a sub-perceptual epsilon and the default
            // 30 Hz frame interval. Override to 15 Hz here: the
            // sweep is wide and slow enough that the eye can't
            // resolve the difference, and every doubled frame is a
            // wgpu submit.
            ctx.animate()
                .sweep()
                .linear()
                .frame_interval(INDETERMINATE_FRAME_INTERVAL)
                .to(&self.indeterminate_pos, 1.0);
        }
        vec![]
    }

    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        match self.orientation {
            Orientation::Horizontal => {
                let width = proposal.width.unwrap_or(100.0);
                Size::new(width, self.thickness)
            }
            Orientation::Vertical => {
                let height = proposal.height.unwrap_or(100.0);
                Size::new(self.thickness, height)
            }
        }
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
        let track_color = self
            .track_color
            .as_ref()
            .map(|c| c.resolve(ctx.theme))
            .unwrap_or(ctx.theme.colors.surface_sunken);
        let fill_color = self
            .fill_color
            .as_ref()
            .map(|c| c.resolve(ctx.theme))
            .unwrap_or(ctx.theme.colors.accent);
        let radius = CornerRadius::uniform(ctx.theme.components.progress_bar.corner_radius);

        // Track
        canvas.fill_rounded_rect(bounds, radius, track_color);

        if let Some(handle) = self.anim_handle {
            // Horizontal indeterminate via shader pipeline. The sweep
            // is drawn as a rectangular band inside the track's
            // rounded corners — the shader does not honor the corner
            // radius, so on rounded tracks with large radii the sweep
            // slightly overlaps the corner. In practice progress-bar
            // radii are 1–3 px and the artifact is imperceptible; the
            // trade is that paint() emits one draw command instead of
            // recomputing the sweep rect each frame.
            canvas.draw_animated_quad(bounds, handle.slot(), AnimatedQuadClass::Procedural);
        } else if self.indeterminate {
            // Vertical indeterminate (or reduced-motion fallback):
            // signal-driven path. The scheduler's own visibility gate
            // stops the sweep when the bar scrolls offscreen.
            let pos = self.indeterminate_pos.get().clamp(0.0, 1.0);
            let sweep_ratio = INDETERMINATE_SWEEP_RATIO;
            let fill_rect = match self.orientation {
                Orientation::Horizontal => {
                    let sweep_w = bounds.width * sweep_ratio;
                    let x = bounds.x - sweep_w + (bounds.width + sweep_w) * pos;
                    Rect::new(x, bounds.y, sweep_w, bounds.height)
                }
                Orientation::Vertical => {
                    let sweep_h = bounds.height * sweep_ratio;
                    let y = bounds.y - sweep_h + (bounds.height + sweep_h) * pos;
                    Rect::new(bounds.x, y, bounds.width, sweep_h)
                }
            };
            canvas.fill_rounded_rect(fill_rect, radius, fill_color);
        } else {
            // Determinate: fill proportional to value
            let value = self.value.get().clamp(0.0, 1.0);
            if value > 0.0 {
                let fill_rect = match self.orientation {
                    Orientation::Horizontal => {
                        Rect::new(bounds.x, bounds.y, bounds.width * value, bounds.height)
                    }
                    Orientation::Vertical => {
                        let fill_h = bounds.height * value;
                        Rect::new(
                            bounds.x,
                            bounds.y + bounds.height - fill_h,
                            bounds.width,
                            fill_h,
                        )
                    }
                };
                canvas.fill_rounded_rect(fill_rect, radius, fill_color);
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::ProgressIndicator);
        if let Some(ref label) = self.label {
            builder.set_name(label.clone());
        }
        if self.indeterminate {
            // Indeterminate bars have no meaningful numeric value —
            // don't announce a stale 0.0. Live::Polite lets screen
            // readers pick up "busy / please wait" transitions
            // without interrupting the user's current action.
            builder.set_live(fern_core::accesskit::Live::Polite);
        } else {
            let value = self.value.get();
            builder.set_numeric_value(value as f64);
            builder.set_min_numeric_value(0.0);
            builder.set_max_numeric_value(1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn progress_bar_size() {
        let mut tree = WidgetTree::new();
        let pb = tree.add(ProgressBar::new(0.5));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        let b = tree.bounds(pb);
        assert!((b.width - 200.0).abs() < 0.01);
        assert!((b.height - 4.0).abs() < 0.01);
    }

    #[test]
    fn progress_bar_paints_track_and_fill() {
        let mut tree = WidgetTree::new().with_theme(fern_tokens::Theme::light_default());
        tree.add(ProgressBar::new(0.5));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        // Should have at least 2 shapes: track + fill
        assert!(frame.shapes.len() >= 2, "should have track and fill shapes");
    }

    #[test]
    fn progress_bar_fill_width_proportional() {
        let mut tree = WidgetTree::new().with_theme(fern_tokens::Theme::light_default());
        let _pb = tree.add(ProgressBar::new(0.5).fill_color(Color::RED));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        // The fill rect should be about half width (100px out of 200px)
        let fill_shapes: Vec<_> = frame
            .shapes
            .iter()
            .filter(|s| s.color == Color::RED.to_array())
            .collect();
        assert!(!fill_shapes.is_empty(), "should have a red fill shape");
        // Shape width is approximately 100 (half of 200)
        let fill = &fill_shapes[0];
        let fill_width = fill.screen[2]; // [x, y, w, h]
        assert!(
            (fill_width - 100.0).abs() < 1.0,
            "fill width should be ~100, got {}",
            fill_width
        );
    }

    #[test]
    fn zero_value_no_fill() {
        let mut tree = WidgetTree::new().with_theme(fern_tokens::Theme::light_default());
        tree.add(ProgressBar::new(0.0).fill_color(Color::RED));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        // Only track, no fill
        let fill_shapes: Vec<_> = frame
            .shapes
            .iter()
            .filter(|s| s.color == Color::RED.to_array())
            .collect();
        assert!(fill_shapes.is_empty(), "zero progress should have no fill");
    }

    #[test]
    fn accessibility_values() {
        let mut tree = WidgetTree::new();
        let pb = tree.add(ProgressBar::new(0.75));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let info = tree.accessibility_node(pb);
        assert_eq!(info.role(), fern_core::accesskit::Role::ProgressIndicator);
    }

    #[test]
    fn indeterminate_progress_bar_emits_animated_quad() {
        // Horizontal indeterminate uses the shader pipeline: one
        // rounded-rect track + one AnimatedQuad whose per-frame phase
        // lives in `frame.anim_params`. The widget's own `paint()`
        // does not re-run between frames — the shader samples live
        // params from the uniform buffer.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(ProgressBar::indeterminate());

        tree.layout(SizeProposal::exact(200.0, 40.0));
        let frame1 = tree.render();
        assert_eq!(
            frame1.animated_quads.len(),
            1,
            "horizontal indeterminate should emit exactly one AnimatedQuad"
        );
        assert_eq!(frame1.anim_params.len(), 1);
        let phase1 = frame1.anim_params[frame1.animated_quads[0].slot as usize].phase;

        // Advance time and re-render. The arena is not dirtied (paint()
        // doesn't need to re-run for phase advancement), but the tree
        // ticks the animated-quad registry every render() and writes
        // fresh anim_params into the frame.
        std::thread::sleep(Duration::from_millis(250));
        let frame2 = tree.render();
        let phase2 = frame2.anim_params[frame2.animated_quads[0].slot as usize].phase;
        assert_ne!(
            phase1, phase2,
            "animated-quad phase must advance between frames"
        );
    }
}
