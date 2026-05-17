//! Default `ScrollBarStyle` impl driven by paint-recipe data.
//!
//! `RecipeScrollBarStyle` ships three visual variants (parallel to the
//! widget's historical `ScrollBarVisual`):
//!
//! - `Permanent` — full track + thumb, always visible. Single
//!   [`FullBarPainter`] child sized to the parent's slot.
//! - `Overlay` — thin idle indicator that cross-fades to the full bar
//!   on hover/drag. Two `Fade`-wrapped painters share the slot; the
//!   `active` signal flips when either `is_hovered` or `is_dragging`
//!   goes true.
//! - `Thin` — thin idle indicator only. One [`ThinIndicatorPainter`]
//!   child.
//!
//! The parent `ScrollBar` widget owns input handling (drag, track click,
//! keyboard, hover) and bounds caching; this style only paints.

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{
    ScrollBarOrientation, ScrollBarStyle, ScrollBarStyleConfig, ScrollBarVariant,
};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::CornerRadius;

use crate::primitives::ZStack;

// IntUI design tokens for ScrollBar. Moved here in Step 7 of the
// styling refactor — the recipe owns its own dimensions instead of
// reading from `theme.components.scrollbar`.
pub const SCROLLBAR_THICKNESS_IDLE: f32 = 4.0;
pub const SCROLLBAR_THICKNESS_HOVER: f32 = 8.0;
pub const SCROLLBAR_MIN_THUMB_LENGTH: f32 = 24.0;
pub const SCROLLBAR_CORNER_RADIUS: f32 = 2.0;

/// Default `ScrollBarStyle` shipped with Bastyde. Colors come from
/// `theme.colors.scrollbar_*`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeScrollBarStyle;

impl ScrollBarStyle for RecipeScrollBarStyle {
    fn make_body(&self, cfg: &ScrollBarStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Thickness values come from the recipe's own design tokens.
        // The widget's per-instance override (`ScrollBar::thickness`)
        // is already reflected in the bounds we receive, so these
        // values only affect the painters' fallback `layout_response`.
        let thickness = SCROLLBAR_THICKNESS_HOVER;
        let resting_thickness = SCROLLBAR_THICKNESS_IDLE;

        match cfg.variant {
            ScrollBarVariant::Permanent => ctx.add(FullBarPainter {
                orientation: cfg.orientation,
                thickness,
                min_thumb_length: cfg.min_thumb_length,
                scroll_ratio: cfg.scroll_ratio.clone(),
                viewport_ratio: cfg.viewport_ratio.clone(),
                is_hovered: cfg.is_hovered.clone(),
                is_dragging: cfg.is_dragging.clone(),
                is_idle: cfg.is_idle.clone(),
                show_track: true,
            }),
            ScrollBarVariant::Thin => ctx.add(ThinIndicatorPainter {
                orientation: cfg.orientation,
                thickness,
                resting_thickness,
                min_thumb_length: cfg.min_thumb_length,
                scroll_ratio: cfg.scroll_ratio.clone(),
                viewport_ratio: cfg.viewport_ratio.clone(),
                is_idle: cfg.is_idle.clone(),
            }),
            ScrollBarVariant::Overlay => {
                // The overlay variant cross-fades two painters that
                // share the slot. Avoid `Fade` here because its tween
                // effect can only observe *mutable* signals; the
                // hover/drag-derived "active" signal is multi-source
                // and lives on the derived side of the Signal split.
                //
                // Instead, run two scoped effects (one per source
                // signal — both are mutable on the widget side) and
                // drive a single animated `revealed` opacity. The
                // inverse opacity for the idle indicator is a derived
                // signal, fine to bind through `set_opacity`.
                let initial = if cfg.is_hovered.get() || cfg.is_dragging.get() {
                    1.0
                } else {
                    0.0
                };
                let revealed = ctx.animated_signal(initial);
                let anim = ctx.animate().fast().standard();
                {
                    let dragging = cfg.is_dragging.clone();
                    let revealed = revealed.clone();
                    let anim = anim.clone();
                    ctx.effect(&cfg.is_hovered, move |hovered| {
                        let target = if *hovered || dragging.get() { 1.0 } else { 0.0 };
                        anim.to_or_snap(&revealed, target);
                    });
                }
                {
                    let hovered = cfg.is_hovered.clone();
                    let revealed = revealed.clone();
                    let anim = anim.clone();
                    ctx.effect(&cfg.is_dragging, move |dragging| {
                        let target = if hovered.get() || *dragging { 1.0 } else { 0.0 };
                        anim.to_or_snap(&revealed, target);
                    });
                }
                let inactive_opacity = revealed.map(|r| 1.0 - *r);

                let thin = ThinIndicatorPainter {
                    orientation: cfg.orientation,
                    thickness,
                    resting_thickness,
                    min_thumb_length: cfg.min_thumb_length,
                    scroll_ratio: cfg.scroll_ratio.clone(),
                    viewport_ratio: cfg.viewport_ratio.clone(),
                    is_idle: cfg.is_idle.clone(),
                };
                let full = FullBarPainter {
                    orientation: cfg.orientation,
                    thickness,
                    min_thumb_length: cfg.min_thumb_length,
                    scroll_ratio: cfg.scroll_ratio.clone(),
                    viewport_ratio: cfg.viewport_ratio.clone(),
                    is_hovered: cfg.is_hovered.clone(),
                    is_dragging: cfg.is_dragging.clone(),
                    is_idle: cfg.is_idle.clone(),
                    show_track: false,
                };

                let thin_id = ctx.add(thin);
                let full_id = ctx.add(full);
                ctx.set_opacity(thin_id, inactive_opacity);
                ctx.set_opacity(full_id, revealed);
                ctx.add(ZStack::new().add_child(thin_id).add_child(full_id))
            }
        }
    }
}

/// Paints the thin resting indicator. Used as the only body in `Thin`
/// variant; layered under the full bar in `Overlay` variant.
struct ThinIndicatorPainter {
    orientation: ScrollBarOrientation,
    /// Slot thickness — needed so `layout_response` returns a sensible
    /// fallback size matching the parent slot.
    thickness: f32,
    resting_thickness: f32,
    min_thumb_length: f32,
    scroll_ratio: Signal<f32>,
    viewport_ratio: Signal<f32>,
    is_idle: Signal<bool>,
}

impl std::fmt::Debug for ThinIndicatorPainter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThinIndicatorPainter")
            .field("orientation", &self.orientation)
            .finish()
    }
}

impl Widget for ThinIndicatorPainter {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.scroll_ratio
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.viewport_ratio
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_idle
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        match self.orientation {
            ScrollBarOrientation::Vertical => {
                Size::new(self.thickness, proposal.height.unwrap_or(0.0))
            }
            ScrollBarOrientation::Horizontal => {
                Size::new(proposal.width.unwrap_or(0.0), self.thickness)
            }
        }
        .into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        if self.is_idle.get() {
            return;
        }
        let thin = self.resting_thickness;
        let thin_bounds = match self.orientation {
            ScrollBarOrientation::Vertical => {
                Rect::new(bounds.right() - thin, bounds.y, thin, bounds.height)
            }
            ScrollBarOrientation::Horizontal => {
                Rect::new(bounds.x, bounds.bottom() - thin, bounds.width, thin)
            }
        };
        let track_len = match self.orientation {
            ScrollBarOrientation::Vertical => bounds.height,
            ScrollBarOrientation::Horizontal => bounds.width,
        };
        let ratio = self.viewport_ratio.get().clamp(0.0, 1.0);
        let thumb_len = (track_len * ratio)
            .max(self.min_thumb_length)
            .min(track_len);
        let scroll_ratio = self.scroll_ratio.get().clamp(0.0, 1.0);
        let offset = scroll_ratio * (track_len - thumb_len);

        let radius = CornerRadius::uniform(thin / 2.0);
        let thumb_rect = match self.orientation {
            ScrollBarOrientation::Vertical => {
                Rect::new(thin_bounds.x, thin_bounds.y + offset, thin, thumb_len)
            }
            ScrollBarOrientation::Horizontal => {
                Rect::new(thin_bounds.x + offset, thin_bounds.y, thumb_len, thin)
            }
        };
        canvas.fill_rounded_rect(thumb_rect, radius, ctx.theme.colors.scrollbar_thumb);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

/// Paints the full track + thumb. Used as the only body in `Permanent`
/// variant; layered over the thin indicator in `Overlay` variant (where
/// `show_track` is false because the track only paints when the bar is
/// active).
struct FullBarPainter {
    orientation: ScrollBarOrientation,
    thickness: f32,
    min_thumb_length: f32,
    scroll_ratio: Signal<f32>,
    viewport_ratio: Signal<f32>,
    is_hovered: Signal<bool>,
    is_dragging: Signal<bool>,
    is_idle: Signal<bool>,
    /// `true` in `Permanent` variant — paints a track behind the thumb.
    /// `false` in `Overlay` variant — track only paints alongside the
    /// thumb when the overlay reveals itself.
    show_track: bool,
}

impl std::fmt::Debug for FullBarPainter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FullBarPainter")
            .field("orientation", &self.orientation)
            .field("show_track", &self.show_track)
            .finish()
    }
}

impl Widget for FullBarPainter {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.scroll_ratio
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.viewport_ratio
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_hovered
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_dragging
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_idle
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        match self.orientation {
            ScrollBarOrientation::Vertical => {
                Size::new(self.thickness, proposal.height.unwrap_or(0.0))
            }
            ScrollBarOrientation::Horizontal => {
                Size::new(proposal.width.unwrap_or(0.0), self.thickness)
            }
        }
        .into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        if self.is_idle.get() {
            return;
        }
        let ratio = self.viewport_ratio.get().clamp(0.0, 1.0);
        let track_len = match self.orientation {
            ScrollBarOrientation::Vertical => bounds.height,
            ScrollBarOrientation::Horizontal => bounds.width,
        };
        let thumb_len = (track_len * ratio)
            .max(self.min_thumb_length)
            .min(track_len);
        let scroll_ratio = self.scroll_ratio.get().clamp(0.0, 1.0);
        let offset = scroll_ratio * (track_len - thumb_len);

        let radius = CornerRadius::uniform(self.thickness / 2.0);
        let colors = &ctx.theme.colors;

        if self.show_track {
            canvas.fill_rounded_rect(bounds, radius, colors.scrollbar_track_hover);
        }

        let thumb = match self.orientation {
            ScrollBarOrientation::Vertical => {
                Rect::new(bounds.x, bounds.y + offset, bounds.width, thumb_len)
            }
            ScrollBarOrientation::Horizontal => {
                Rect::new(bounds.x + offset, bounds.y, thumb_len, bounds.height)
            }
        };
        let thumb_color = if self.is_dragging.get() {
            colors.scrollbar_thumb_pressed
        } else if self.is_hovered.get() {
            colors.scrollbar_thumb_hover
        } else {
            colors.scrollbar_thumb
        };
        canvas.fill_rounded_rect(thumb, radius, thumb_color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}
