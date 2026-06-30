// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `ScrollBarStyle` impl driven by paint-recipe data.
//!
//! `RecipeScrollBarStyle` ships three visual variants (parallel to the
//! widget's historical `ScrollBarVisual`):
//!
//! - `Permanent` — full track + thumb, always visible. Single
//!   `FullBarPainter` child sized to the parent's slot.
//! - `Overlay` — thin idle indicator that cross-fades to the full bar
//!   on hover/drag. Two `Fade`-wrapped painters share the slot; the
//!   `active` signal flips when either `is_hovered` or `is_dragging`
//!   goes true.
//! - `Thin` — thin idle indicator only. One `ThinIndicatorPainter`
//!   child.
//!
//! The parent `ScrollBar` widget owns input handling (drag, track click,
//! keyboard, hover) and bounds caching; this style only paints.

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{
    ScrollBarOrientation, ScrollBarStyle, ScrollBarStyleConfig, ScrollBarVariant,
};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{Color, CornerRadius};

use crate::primitives::ZStack;

// IntUI design tokens for ScrollBar. The recipe owns its own dimensions.
pub const SCROLLBAR_THICKNESS_IDLE: f32 = 4.0;
pub const SCROLLBAR_THICKNESS_HOVER: f32 = 8.0;
pub const SCROLLBAR_MIN_THUMB_LENGTH: f32 = 24.0;
pub const SCROLLBAR_CORNER_RADIUS: f32 = 2.0;

// Opacity ramp applied to a `ScrollBarStyleConfig::thumb_color` override
// (idle / hover / pressed), giving state feedback while keeping the caller's
// tint. Mirrors the implicit alpha ramp of the default `scrollbar_thumb*`
// tokens, but scaled up so a single opaque tint (e.g. `TextRole::TooltipText`)
// reads clearly on whatever surface it sits over.
const OVERRIDE_THUMB_ALPHA_IDLE: f32 = 0.50;
const OVERRIDE_THUMB_ALPHA_HOVER: f32 = 0.72;
const OVERRIDE_THUMB_ALPHA_PRESSED: f32 = 0.92;
const OVERRIDE_TRACK_ALPHA: f32 = 0.12;

/// Resolve the thumb colour for the given interaction state: the optional
/// `thumb_color` override (tinted by the state alpha ramp) wins; otherwise the
/// theme's `scrollbar_thumb*` tokens. Shared by both painters so the override
/// path is identical for the thin indicator and the full bar.
fn resolve_thumb_color(
    override_color: Option<&ColorProp>,
    ctx: &PaintContext,
    hovered: bool,
    pressed: bool,
) -> Color {
    match override_color {
        Some(prop) => {
            let alpha = if pressed {
                OVERRIDE_THUMB_ALPHA_PRESSED
            } else if hovered {
                OVERRIDE_THUMB_ALPHA_HOVER
            } else {
                OVERRIDE_THUMB_ALPHA_IDLE
            };
            prop.resolve(ctx.theme, true).with_alpha(alpha)
        }
        None => {
            let c = &ctx.theme.colors;
            if pressed {
                c.scrollbar_thumb_pressed
            } else if hovered {
                c.scrollbar_thumb_hover
            } else {
                c.scrollbar_thumb
            }
        }
    }
}

/// Configurable dimensions for [`RecipeScrollBarStyle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollBarRecipe {
    pub thickness_idle: f32,
    pub thickness_hover: f32,
    pub min_thumb_length: f32,
    pub corner_radius: f32,
}

impl Default for ScrollBarRecipe {
    fn default() -> Self {
        Self {
            thickness_idle: SCROLLBAR_THICKNESS_IDLE,
            thickness_hover: SCROLLBAR_THICKNESS_HOVER,
            min_thumb_length: SCROLLBAR_MIN_THUMB_LENGTH,
            corner_radius: SCROLLBAR_CORNER_RADIUS,
        }
    }
}

/// Default `ScrollBarStyle` shipped with Bastyde. Colors come from
/// `theme.colors.scrollbar_*`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeScrollBarStyle {
    pub recipe: ScrollBarRecipe,
}

impl RecipeScrollBarStyle {
    pub fn new(recipe: ScrollBarRecipe) -> Self {
        Self { recipe }
    }
}

impl ScrollBarStyle for RecipeScrollBarStyle {
    fn make_body(&self, cfg: &ScrollBarStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Thickness values come from the recipe's own design tokens.
        // The widget's per-instance override (`ScrollBar::thickness`)
        // is already reflected in the bounds we receive, so these
        // values only affect the painters' fallback `layout_response`.
        let thickness = self.recipe.thickness_hover;
        let resting_thickness = self.recipe.thickness_idle;

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
                thumb_color: cfg.thumb_color.clone(),
            }),
            ScrollBarVariant::Thin => ctx.add(ThinIndicatorPainter {
                orientation: cfg.orientation,
                thickness,
                resting_thickness,
                min_thumb_length: cfg.min_thumb_length,
                scroll_ratio: cfg.scroll_ratio.clone(),
                viewport_ratio: cfg.viewport_ratio.clone(),
                is_idle: cfg.is_idle.clone(),
                thumb_color: cfg.thumb_color.clone(),
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
                    thumb_color: cfg.thumb_color.clone(),
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
                    thumb_color: cfg.thumb_color.clone(),
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
    thumb_color: Option<ColorProp>,
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
        // Thin indicator has no hover/drag state — it's the resting strip.
        let thumb_color = resolve_thumb_color(self.thumb_color.as_ref(), ctx, false, false);
        canvas.fill_rounded_rect(thumb_rect, radius, thumb_color);
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
    thumb_color: Option<ColorProp>,
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

        if self.show_track {
            let track = match &self.thumb_color {
                Some(prop) => prop
                    .resolve(ctx.theme, true)
                    .with_alpha(OVERRIDE_TRACK_ALPHA),
                None => ctx.theme.colors.scrollbar_track_hover,
            };
            canvas.fill_rounded_rect(bounds, radius, track);
        }

        let thumb = match self.orientation {
            ScrollBarOrientation::Vertical => {
                Rect::new(bounds.x, bounds.y + offset, bounds.width, thumb_len)
            }
            ScrollBarOrientation::Horizontal => {
                Rect::new(bounds.x + offset, bounds.y, thumb_len, bounds.height)
            }
        };
        let thumb_color = resolve_thumb_color(
            self.thumb_color.as_ref(),
            ctx,
            self.is_hovered.get(),
            self.is_dragging.get(),
        );
        canvas.fill_rounded_rect(thumb, radius, thumb_color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}
