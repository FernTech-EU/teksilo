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

use teksilo_canvas::{Canvas, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::focus::FocusOrigin;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{SliderOrientation, SliderStyle, SliderStyleConfig, SliderVariant};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, InputTokens};

/// Minimum cross-axis size of the slider row, in dp. Sized to
/// accommodate the thumb plus the focus-ring envelope.
const MIN_CROSS_SIZE: f32 = 24.0;

// IntUI design tokens for Slider. The recipe owns its own dimensions.
pub const SLIDER_TRACK_HEIGHT: f32 = 4.0;
pub const SLIDER_THUMB_DIAMETER: f32 = 14.0;
pub const SLIDER_TICK_SIZE: f32 = 2.0;

/// Dimension data for `RecipeSliderStyle`. All fields are in dp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderRecipe {
    pub track_height: f32,
    pub thumb_diameter: f32,
    pub tick_size: f32,
}

impl SliderRecipe {
    /// This recipe's dimensions resolved against a density's [`InputTokens`].
    ///
    /// [`Default`] is `for_tokens(&InputTokens::default())` — the Compact
    /// ladder — so the shipped values below are the Compact column by
    /// construction and cannot drift from it.
    ///
    /// Every dimension this recipe carries is a decoration (a corner radius, a
    /// hairline, a glyph metric), so the parameter is unused: the density
    /// ladder never moves any of them. It is taken all the same, so every
    /// recipe is constructed the same way at its widget's build site.
    pub fn for_tokens(_tokens: &InputTokens) -> Self {
        Self {
            track_height: SLIDER_TRACK_HEIGHT,
            thumb_diameter: SLIDER_THUMB_DIAMETER,
            tick_size: SLIDER_TICK_SIZE,
        }
    }
}

impl Default for SliderRecipe {
    fn default() -> Self {
        Self::for_tokens(&InputTokens::default())
    }
}

/// Default `SliderStyle` shipped with Teksilo. Colors come from
/// `theme.colors.{accent, surface_sunken, ...}`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeSliderStyle {
    pub recipe: SliderRecipe,
}

impl RecipeSliderStyle {
    pub fn new(recipe: SliderRecipe) -> Self {
        Self { recipe }
    }

    /// This style with every dimension resolved against a density's
    /// [`InputTokens`], as `RecipeSliderStyle::for_tokens(&ctx.theme().input)` at
    /// the widget's own build site.
    ///
    /// [`Default`] is the `TargetDensity::Compact` projection, so a Compact
    /// tree gets exactly the values this module documents.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            recipe: SliderRecipe::for_tokens(tokens),
        }
    }
}

impl SliderStyle for RecipeSliderStyle {
    fn thumb_diameter(&self, _cfg: &SliderStyleConfig) -> f32 {
        self.recipe.thumb_diameter
    }

    fn make_body(&self, cfg: &SliderStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(SliderBody {
            recipe: self.recipe,
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
    recipe: SliderRecipe,
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
        let cross = (self.recipe.thumb_diameter + envelope * 2.0).max(MIN_CROSS_SIZE);
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
        let track_height = self.recipe.track_height;
        let thumb_diameter = self.recipe.thumb_diameter;
        let thumb_radius = thumb_diameter * 0.5;
        let enabled = !self.is_disabled.get();
        let t = self.value_normalized.get().clamp(0.0, 1.0);

        let rtl = ctx.layout_direction == LayoutDirection::RightToLeft;

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
                // A horizontal slider is a reading-order axis: its minimum sits
                // at the leading edge, which is the RIGHT edge in an RTL UI.
                // Mirroring here and in `Slider`'s own position→value map is
                // one change made twice, and the two are pinned to each other
                // by `Slider::target_regions`, which reports the thumb the same
                // way. A vertical slider is unaffected — RTL is a horizontal
                // convention.
                let thumb_pos = if rtl {
                    track.x + usable * (1.0 - t)
                } else {
                    track.x + usable * t
                };
                let fill = if rtl {
                    let fill_w = (track.right() - thumb_pos).max(0.0);
                    Rect::new(thumb_pos, ty, fill_w, track_height)
                } else {
                    let fill_w = (thumb_pos - track.x).max(0.0);
                    Rect::new(track.x, ty, fill_w, track_height)
                };
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
                // A vertical slider's minimum is at the **bottom** and its
                // maximum at the top — Qt's `QSlider`, GTK4's `GtkScale`, the
                // Win32 trackbar and `<input type=range>` all agree, and it is
                // what makes `ArrowUp` (which `range_nav` reports as an
                // increase) move the thumb up. Growing the thumb position with
                // `t` sent it *down* on every increase, so the keys and the
                // pointer both drove the control backwards.
                let thumb_pos = track.bottom() - usable * t;
                let fill_h = (track.bottom() - thumb_pos).max(0.0);
                let fill = Rect::new(tx, thumb_pos, track_height, fill_h);
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
            let tick_size = self.recipe.tick_size.max(2.0);
            let tick_color = if enabled {
                colors.text_secondary
            } else {
                colors.text_disabled
            };
            for i in 0..n {
                let tt = i as f32 / (n - 1) as f32;
                match self.orientation {
                    SliderOrientation::Horizontal => {
                        let tt = if rtl { 1.0 - tt } else { tt };
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
