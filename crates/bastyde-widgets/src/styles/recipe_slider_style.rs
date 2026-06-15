// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `SliderStyle` impl driven by paint-recipe data.
//!
//! `RecipeSliderStyle` ships the IntUI look out of the box. The visual
//! body is a tiny private leaf widget (`SliderBody`) that paints
//! track + fill + thumb + focus ring directly on the canvas. Same
//! "leaf body" choice as `RecipeToggleStyle` and for the same reason —
//! there's no general-purpose absolute-positioning primitive for the
//! thumb on the track, and the compositional version would add several
//! arena nodes per slider for no visual win.
//!
//! Custom `SliderStyle` impls are free to compose `RectWidget` layers
//! and a positioned thumb if they prefer.

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::focus::FocusOrigin;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{SliderOrientation, SliderStyle, SliderStyleConfig, SliderVariant};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::CornerRadius;

/// Minimum cross-axis size of the slider row, in dp. Sized to
/// accommodate the thumb plus the focus-ring envelope.
const MIN_CROSS_SIZE: f32 = 24.0;

// IntUI design tokens for Slider. The recipe owns its own dimensions.
pub const SLIDER_TRACK_HEIGHT: f32 = 4.0;
pub const SLIDER_THUMB_DIAMETER: f32 = 14.0;
pub const SLIDER_TICK_SIZE: f32 = 2.0;

/// Default `SliderStyle` shipped with Bastyde. Colors come from
/// `theme.colors.{accent, surface_sunken, ...}`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeSliderStyle;

impl SliderStyle for RecipeSliderStyle {
    fn thumb_diameter(&self, _cfg: &SliderStyleConfig) -> f32 {
        SLIDER_THUMB_DIAMETER
    }

    fn make_body(&self, cfg: &SliderStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(SliderBody {
            value_normalized: cfg.value_normalized.clone(),
            is_hovered: cfg.is_hovered.clone(),
            is_dragging: cfg.is_dragging.clone(),
            is_disabled: cfg.is_disabled.clone(),
            focus_origin: cfg.focus_origin.clone(),
            orientation: cfg.orientation,
            tick_count: cfg.tick_count,
            variant: cfg.variant,
        })
    }
}

/// Internal leaf widget that paints the track + fill + thumb. Owned
/// by `RecipeSliderStyle::make_body`; not exposed publicly because
/// custom `SliderStyle` impls compose their own body instead.
struct SliderBody {
    value_normalized: Signal<f32>,
    is_hovered: Signal<bool>,
    is_dragging: Signal<bool>,
    is_disabled: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    orientation: SliderOrientation,
    tick_count: Option<u32>,
    variant: SliderVariant,
}

impl std::fmt::Debug for SliderBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliderBody")
            .field("orientation", &self.orientation)
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for SliderBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.value_normalized
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_hovered
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_dragging
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_disabled
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.focus_origin
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let envelope = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width;
        let cross = (SLIDER_THUMB_DIAMETER + envelope * 2.0).max(MIN_CROSS_SIZE);
        match self.orientation {
            SliderOrientation::Horizontal => {
                let width = proposal.width.unwrap_or(200.0);
                Size::new(width, cross)
            }
            SliderOrientation::Vertical => {
                let height = proposal.height.unwrap_or(200.0);
                Size::new(cross, height)
            }
        }
        .into()
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
        let colors = &ctx.theme.colors;
        let shape = &ctx.theme.shape;
        let track_height = SLIDER_TRACK_HEIGHT;
        let thumb_diameter = SLIDER_THUMB_DIAMETER;
        let thumb_radius = thumb_diameter * 0.5;
        let enabled = !self.is_disabled.get();
        let t = self.value_normalized.get().clamp(0.0, 1.0);

        let radius = CornerRadius::uniform(track_height * 0.5);
        let track_color = if enabled {
            colors.surface_sunken
        } else {
            colors.accent_disabled
        };
        let fill_color = if enabled {
            colors.accent
        } else {
            colors.text_disabled
        };

        let (track_rect, fill_rect, thumb_cx, thumb_cy) = match self.orientation {
            SliderOrientation::Horizontal => {
                let ty = bounds.y + (bounds.height - track_height) * 0.5;
                let track = Rect::new(
                    bounds.x + thumb_radius,
                    ty,
                    (bounds.width - thumb_radius * 2.0).max(0.0),
                    track_height,
                );
                let usable = track.width;
                let thumb_pos = track.x + usable * t;
                let fill_w = (thumb_pos - track.x).max(0.0);
                let fill = Rect::new(track.x, ty, fill_w, track_height);
                (track, fill, thumb_pos, bounds.y + bounds.height * 0.5)
            }
            SliderOrientation::Vertical => {
                let tx = bounds.x + (bounds.width - track_height) * 0.5;
                let track = Rect::new(
                    tx,
                    bounds.y + thumb_radius,
                    track_height,
                    (bounds.height - thumb_radius * 2.0).max(0.0),
                );
                let usable = track.height;
                let thumb_pos = track.y + usable * t;
                let fill_h = (thumb_pos - track.y).max(0.0);
                let fill = Rect::new(tx, track.y, track_height, fill_h);
                (track, fill, bounds.x + bounds.width * 0.5, thumb_pos)
            }
        };

        canvas.fill_rounded_rect(track_rect, radius, track_color);
        if fill_rect.width > 0.0 && fill_rect.height > 0.0 {
            canvas.fill_rounded_rect(fill_rect, radius, fill_color);
        }

        // Discrete-variant ticks. Painted above the track so they
        // read as fixed reference points; the fill overlays them
        // naturally because tick rendering happens before the thumb.
        // Continuous variant skips this loop entirely.
        if matches!(self.variant, SliderVariant::Discrete)
            && let Some(n) = self.tick_count
            && n >= 2
        {
            let tick_size = SLIDER_TICK_SIZE.max(2.0);
            let tick_color = if enabled {
                colors.text_secondary
            } else {
                colors.text_disabled
            };
            for i in 0..n {
                let tt = i as f32 / (n - 1) as f32;
                match self.orientation {
                    SliderOrientation::Horizontal => {
                        let tx = track_rect.x + track_rect.width * tt;
                        let ty = track_rect.y - tick_size - 2.0;
                        let r = Rect::new(tx - tick_size * 0.5, ty, tick_size, tick_size);
                        canvas.fill_rounded_rect(
                            r,
                            CornerRadius::uniform(tick_size * 0.5),
                            tick_color,
                        );
                    }
                    SliderOrientation::Vertical => {
                        let ty = track_rect.y + track_rect.height * tt;
                        let tx = track_rect.x - tick_size - 2.0;
                        let r = Rect::new(tx, ty - tick_size * 0.5, tick_size, tick_size);
                        canvas.fill_rounded_rect(
                            r,
                            CornerRadius::uniform(tick_size * 0.5),
                            tick_color,
                        );
                    }
                }
            }
        }

        // Thumb
        let thumb_color = if !enabled {
            colors.text_disabled
        } else if self.is_dragging.get() {
            colors.accent_pressed
        } else if self.is_hovered.get() {
            colors.accent_hover
        } else {
            colors.accent
        };
        let thumb_rect = Rect::new(
            thumb_cx - thumb_radius,
            thumb_cy - thumb_radius,
            thumb_diameter,
            thumb_diameter,
        );
        canvas.fill_rounded_rect(thumb_rect, CornerRadius::uniform(thumb_radius), thumb_color);

        // Focus ring — keyboard-only, drawn outside the thumb in the
        // theme-defined gap.
        if self.focus_origin.get() == Some(FocusOrigin::Keyboard) {
            let offset = shape.focus_ring_offset;
            let half_stroke = shape.focus_ring_width * 0.5;
            let ring_inset = offset + half_stroke;
            let ring_rect = Rect::new(
                thumb_rect.x - ring_inset,
                thumb_rect.y - ring_inset,
                thumb_rect.width + ring_inset * 2.0,
                thumb_rect.height + ring_inset * 2.0,
            );
            canvas.stroke_rounded_rect(
                ring_rect,
                CornerRadius::uniform(thumb_radius + ring_inset),
                colors.focus_ring,
                shape.focus_ring_width,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Accessibility lives on the parent Slider widget, not the
        // body. The body is presentational; mark it hidden so the
        // walker prunes it rather than emitting a nameless Role::Unknown.
        builder.set_hidden();
    }
}
