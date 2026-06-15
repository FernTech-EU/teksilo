// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! ProgressBar — a bar showing progress from 0.0 to 1.0.
//!
//! Determinate / indeterminate, horizontal / vertical. The stationary
//! chrome (track + determinate fill) is owned by `ProgressBarStyle`;
//! the indeterminate sweep stays widget-owned (principle 6 — motion
//! infrastructure is not chrome). Three paint paths:
//!
//! - **Horizontal indeterminate** uses the shader-driven animated-quad
//!   pipeline. `ProgressBar::build` registers an `AnimatedQuadHandle`
//!   and mounts a single `IndeterminateSweepLeaf` whose `paint()`
//!   issues one `draw_animated_quad` per frame; the shader composes
//!   the track + moving fill in a procedural draw. The recipe frame
//!   is NOT mounted in this case (the shader self-paints both).
//! - **Vertical indeterminate** keeps the signal-based path. The
//!   recipe frame paints the track; an `IndeterminateSweepLeaf` in
//!   signal mode paints a moving fill rect on top driven by a
//!   `Signal<f32>::animate_looping`.
//! - **Determinate** mounts the recipe frame only; the frame paints
//!   the track plus a proportional fill rect.

use std::rc::Rc;
use std::time::Duration;

use bastyde_canvas::{AnimatedQuadClass, Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::animated_quad::{AnimatedQuadHandle, AnimatedQuadKind};
use bastyde_core::binding::BindingLevel;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::styles::{ProgressBarStyleConfig, ProgressKind, SharedProgressBarStyle};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
#[cfg(test)]
use bastyde_tokens::Color;
use bastyde_tokens::{CornerRadius, Orientation, SurfaceRole};

use crate::primitives::ZStack;
use crate::styles::recipe_progress_bar_style::PROGRESS_BAR_CORNER_RADIUS;
use bastyde_i18n::LocalizedString;

const DEFAULT_THICKNESS: f32 = 4.0;
/// ~15 Hz cadence — see module-level note in the original. The eye
/// doesn't resolve >15 fps for the wide slow sweep, and every doubled
/// frame is a full wgpu submit.
const INDETERMINATE_FRAME_INTERVAL: Duration = Duration::from_millis(66);
const INDETERMINATE_SWEEP_RATIO: f32 = 0.42;

/// A progress bar — determinate or indeterminate, horizontal or vertical.
pub struct ProgressBar {
    value: Prop<f32>,
    indeterminate: bool,
    orientation: Orientation,
    thickness: f32,
    track_color: Option<ColorProp>,
    fill_color: Option<ColorProp>,
    label: Option<LocalizedString>,
    /// Per-call override for the stationary chrome (track + determinate fill).
    style_override: Option<SharedProgressBarStyle>,
    root_child_id: Option<WidgetId>,
}

impl ProgressBar {
    /// Create a determinate progress bar with a static value (0.0–1.0).
    pub fn new(value: f32) -> Self {
        Self {
            value: Prop::Static(value.clamp(0.0, 1.0)),
            indeterminate: false,
            orientation: Orientation::Horizontal,
            thickness: DEFAULT_THICKNESS,
            track_color: None,
            fill_color: None,
            label: None,
            style_override: None,
            root_child_id: None,
        }
    }

    /// Create an indeterminate progress bar (animated sweep).
    pub fn indeterminate() -> Self {
        Self {
            value: Prop::Static(0.0),
            indeterminate: true,
            orientation: Orientation::Horizontal,
            thickness: DEFAULT_THICKNESS,
            track_color: None,
            fill_color: None,
            label: None,
            style_override: None,
            root_child_id: None,
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

    /// Override the fill / sweep color. Default (unset) is `SurfaceRole::Accent`.
    /// Accepts `Color`, roles, or `Signal<Color>`.
    pub fn fill_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.fill_color = Some(color.into());
        self
    }

    /// Per-call style override for the stationary chrome (track +
    /// determinate fill). The indeterminate sweep is widget-owned and
    /// always uses the shader-quad / signal-driven path described in
    /// the module doc; the style supplies the sweep's *colour*
    /// recipe via `fill_color_override` / `track_color_override`.
    pub fn style(mut self, style: impl bastyde_core::styles::ProgressBarStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Accessible name for the progress bar.
    pub fn label(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.label = Some(ls);
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
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        // Reduced-motion gate: an indeterminate sweep is decorative;
        // when reduced-motion is on, fall through to a static
        // signal-driven path that never animates (pos stays at 0).
        let reduced_motion = ctx.prefers_reduced_motion();
        let animate = self.indeterminate && !reduced_motion;
        let use_shader_path = animate && matches!(self.orientation, Orientation::Horizontal);
        let sweep_period = ctx.theme().motion.duration_indeterminate_sweep;

        let style: SharedProgressBarStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.progress_bar.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeProgressBarStyle));
        let cfg = ProgressBarStyleConfig {
            orientation: self.orientation,
            progress: if self.indeterminate {
                ProgressKind::Indeterminate
            } else {
                ProgressKind::Determinate(self.value.clone())
            },
            track_color_override: self.track_color.clone(),
            fill_color_override: self.fill_color.clone(),
        };

        // Three branches matching the module-doc paint paths:
        //
        // 1. Horizontal indeterminate (shader): the shader self-paints
        //    track + sweep in one procedural quad; mount ONLY the
        //    sweep leaf, skip the recipe frame to avoid double-painting
        //    the track.
        // 2. Vertical indeterminate (or reduced-motion fallback): the
        //    recipe frame paints the track; the sweep leaf paints the
        //    moving fill on top inside a `ZStack`.
        // 3. Determinate (or reduced-motion non-indeterminate): the
        //    recipe frame paints track + proportional fill; no leaf.
        let root = if use_shader_path {
            let track = self
                .track_color
                .clone()
                .unwrap_or_else(|| SurfaceRole::Sunken.into());
            let fill = self
                .fill_color
                .clone()
                .unwrap_or_else(|| SurfaceRole::Accent.into());
            let handle = ctx.animated_quad(AnimatedQuadKind::IndeterminateSweep {
                period: sweep_period,
                sweep_ratio: INDETERMINATE_SWEEP_RATIO,
                track_color: track,
                fill_color: fill,
            });
            ctx.add(IndeterminateSweepLeaf::shader(handle))
        } else if self.indeterminate {
            let frame_id = style.make_body(&cfg, ctx);
            let pos = ctx.animated_signal(0.0);
            // Sub-perceptual epsilon + 15 Hz frame-interval cadence,
            // per the module-doc rationale. Skipped under
            // reduced-motion so the signal stays at 0.0.
            if !reduced_motion {
                ctx.animate()
                    .sweep()
                    .linear()
                    .frame_interval(INDETERMINATE_FRAME_INTERVAL)
                    .to(&pos, 1.0);
            }
            let fill = self
                .fill_color
                .clone()
                .unwrap_or_else(|| SurfaceRole::Accent.into());
            let leaf_id = ctx.add(IndeterminateSweepLeaf::signal(self.orientation, pos, fill));
            ctx.add(ZStack::new().add_child(frame_id).add_child(leaf_id))
        } else {
            style.make_body(&cfg, ctx)
        };
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::ProgressIndicator);
        if let Some(ref label) = self.label {
            builder.set_name(label.clone());
        }
        if self.indeterminate {
            builder.set_live(bastyde_core::accesskit::Live::Polite);
        } else {
            let value = self.value.get();
            builder.set_numeric_value(value as f64);
            builder.set_min_numeric_value(0.0);
            builder.set_max_numeric_value(1.0);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// Internal leaf that paints the indeterminate sweep. Owns the only
/// remaining `paint()` in the `ProgressBar` widget family (the
/// motion-infrastructure call to `draw_animated_quad` or the
/// signal-driven moving fill); the parent `ProgressBar` itself stays
/// pure composition.
enum IndeterminateSweepLeaf {
    /// Horizontal shader path — one procedural quad per frame.
    Shader(AnimatedQuadHandle),
    /// Vertical / reduced-motion signal path — a rect placed at
    /// `pos ∈ [0, 1]` along the long axis.
    Signal {
        orientation: Orientation,
        pos: Signal<f32>,
        fill: ColorProp,
    },
}

impl IndeterminateSweepLeaf {
    fn shader(handle: AnimatedQuadHandle) -> Self {
        Self::Shader(handle)
    }
    fn signal(orientation: Orientation, pos: Signal<f32>, fill: ColorProp) -> Self {
        Self::Signal {
            orientation,
            pos,
            fill,
        }
    }
}

impl std::fmt::Debug for IndeterminateSweepLeaf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shader(_) => f.debug_struct("IndeterminateSweepLeaf::Shader").finish(),
            Self::Signal { .. } => f.debug_struct("IndeterminateSweepLeaf::Signal").finish(),
        }
    }
}

impl Widget for IndeterminateSweepLeaf {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Self::Signal { pos, .. } = self {
            let id = ctx.self_id();
            pos.bind_to(id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        }
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse
    where
        Self: Sized,
    {
        // Fills whatever bounds the parent ZStack / ProgressBar
        // assigns.
        Size::new(
            proposal.width.unwrap_or(0.0),
            proposal.height.unwrap_or(0.0),
        )
        .into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        match self {
            Self::Shader(handle) => {
                // One quad per frame; the fragment shader self-paints
                // track + sweep. Sweep extends slightly past the
                // rounded corners on large radii — acceptable trade
                // for one-draw-call animation.
                canvas.draw_animated_quad(bounds, handle.slot(), AnimatedQuadClass::Procedural);
            }
            Self::Signal {
                orientation,
                pos,
                fill,
            } => {
                let radius = CornerRadius::uniform(PROGRESS_BAR_CORNER_RADIUS);
                let value = pos.get().clamp(0.0, 1.0);
                let fill_color = fill.resolve(ctx.theme, ctx.effective_enabled);
                let fill_rect = match orientation {
                    Orientation::Horizontal => {
                        let sweep_w = bounds.width * INDETERMINATE_SWEEP_RATIO;
                        let x = bounds.x - sweep_w + (bounds.width + sweep_w) * value;
                        Rect::new(x, bounds.y, sweep_w, bounds.height)
                    }
                    Orientation::Vertical => {
                        let sweep_h = bounds.height * INDETERMINATE_SWEEP_RATIO;
                        let y = bounds.y - sweep_h + (bounds.height + sweep_h) * value;
                        Rect::new(bounds.x, y, bounds.width, sweep_h)
                    }
                };
                canvas.fill_rounded_rect(fill_rect, radius, fill_color);
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational — the parent `ProgressBar` emits the
        // `Role::ProgressIndicator` node.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(ProgressBar::new(0.5));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        assert!(frame.shapes.len() >= 2, "should have track and fill shapes");
    }

    #[test]
    fn progress_bar_fill_width_proportional() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let _pb = tree.add(ProgressBar::new(0.5).fill_color(Color::RED));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        let fill_shapes: Vec<_> = frame
            .shapes
            .iter()
            .filter(|s| s.color == Color::RED.to_array())
            .collect();
        assert!(!fill_shapes.is_empty(), "should have a red fill shape");
        let fill = &fill_shapes[0];
        let fill_width = fill.screen[2];
        assert!(
            (fill_width - 100.0).abs() < 1.0,
            "fill width should be ~100, got {}",
            fill_width
        );
    }

    #[test]
    fn zero_value_no_fill() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(ProgressBar::new(0.0).fill_color(Color::RED));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
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
        assert_eq!(
            info.role(),
            bastyde_core::accesskit::Role::ProgressIndicator
        );
    }

    #[test]
    fn indeterminate_progress_bar_emits_animated_quad() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
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

        std::thread::sleep(Duration::from_millis(250));
        let frame2 = tree.render();
        let phase2 = frame2.anim_params[frame2.animated_quads[0].slot as usize].phase;
        assert_ne!(
            phase1, phase2,
            "animated-quad phase must advance between frames"
        );
    }
}
