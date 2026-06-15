// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `ProgressBarStyle` impl driven by paint-recipe data.
//!
//! `RecipeProgressBarStyle` ships the IntUI progress-bar stationary
//! chrome: a `surface_sunken` track and (for determinate bars) an
//! `accent`-colored proportional fill. The indeterminate sweep is
//! deliberately *not* part of this recipe — it stays widget-owned in
//! `ProgressBar::build`, which mounts an `IndeterminateSweepLeaf` on
//! top for both the horizontal-shader path and the vertical /
//! reduced-motion signal path (principle 6: motion infrastructure is
//! not chrome).
//!
//! Apps that want a different progress look (segmented chunks, gradient
//! fill, branded colour) write their own `impl ProgressBarStyle` block
//! and install it per-call (`ProgressBar::style(...)`) or theme-wide
//! (`theme.style_slots.progress_bar`).

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Prop;
use bastyde_core::styles::{ProgressBarStyle, ProgressBarStyleConfig, ProgressKind};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{CornerRadius, Orientation, SurfaceRole};

// IntUI design tokens for ProgressBar. The recipe owns its own dimensions.
pub const PROGRESS_BAR_CORNER_RADIUS: f32 = 2.0;

/// Default `ProgressBarStyle` shipped with Bastyde. Track is
/// `SurfaceRole::Sunken`, fill is `SurfaceRole::Accent`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeProgressBarStyle;

impl ProgressBarStyle for RecipeProgressBarStyle {
    fn make_body(&self, cfg: &ProgressBarStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let track = cfg
            .track_color_override
            .clone()
            .unwrap_or_else(|| SurfaceRole::Sunken.into());
        let fill = cfg
            .fill_color_override
            .clone()
            .unwrap_or_else(|| SurfaceRole::Accent.into());
        let determinate_value = match &cfg.progress {
            ProgressKind::Determinate(p) => Some(p.clone()),
            ProgressKind::Indeterminate => None,
        };
        ctx.add(ProgressBarFrame {
            orientation: cfg.orientation,
            track,
            fill,
            determinate_value,
        })
    }
}

/// Internal recipe widget that paints the progress bar's stationary
/// chrome. For determinate bars it paints the track + a fill rect
/// proportional to the bound value; for indeterminate bars it paints
/// only the track (the moving sweep is composed on top by the
/// `ProgressBar` widget's `build()` — see
/// `IndeterminateSweepLeaf` in `progress_bar.rs`). The shader-quad
/// horizontal path replaces the entire track-plus-sweep visual in one
/// procedural draw; the `ProgressBar` widget skips mounting this
/// frame in that case to avoid double-painting.
struct ProgressBarFrame {
    orientation: Orientation,
    track: ColorProp,
    fill: ColorProp,
    /// `Some` for determinate bars; `None` for indeterminate (the
    /// widget mounts the sweep leaf separately).
    determinate_value: Option<Prop<f32>>,
}

impl std::fmt::Debug for ProgressBarFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgressBarFrame")
            .field("orientation", &self.orientation)
            .finish()
    }
}

impl Widget for ProgressBarFrame {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        if let Some(p) = &self.determinate_value {
            p.register_if_bound(id, registry, BindingLevel::RepaintOnly);
        }
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // The frame fills whatever bounds its parent assigns — the
        // `ProgressBar` widget owns the intrinsic-size policy.
        Size::new(
            proposal.width.unwrap_or(0.0),
            proposal.height.unwrap_or(0.0),
        )
        .into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius = CornerRadius::uniform(PROGRESS_BAR_CORNER_RADIUS);
        let track_color = self.track.resolve(ctx.theme, ctx.effective_enabled);
        canvas.fill_rounded_rect(bounds, radius, track_color);

        if let Some(value_prop) = &self.determinate_value {
            let value = value_prop.get().clamp(0.0, 1.0);
            if value > 0.0 {
                let fill_color = self.fill.resolve(ctx.theme, ctx.effective_enabled);
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
        // Presentational — the parent `ProgressBar` emits the
        // `Role::ProgressIndicator` node with the numeric value.
        builder.set_hidden();
    }
}
