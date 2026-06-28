// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Material 3 switch.
//!
//! The IntUI toggle (28×16 dp track, 12 dp thumb) reads wrong in an M3
//! context, so this is a full `impl ToggleStyle`. M3's switch has a
//! 52×32 dp track and a thumb that grows from 16 dp (off) to 24 dp (on);
//! the off track shows an outline. Exact M3 colors are read from the
//! [`Material3Palette`](crate::Material3Palette) extension (always present
//! under this theme), with role-color fallbacks for safety.

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::focus::FocusOrigin;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{ToggleStyle, ToggleStyleConfig};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{Color, CornerRadius, lerp};

use crate::Material3Palette;

const TRACK_W: f32 = 52.0;
const TRACK_H: f32 = 32.0;
const THUMB_OFF: f32 = 16.0;
const THUMB_ON: f32 = 24.0;
const TRACK_OUTLINE_W: f32 = 2.0;

/// Material 3 switch `ToggleStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct M3ToggleStyle;

impl ToggleStyle for M3ToggleStyle {
    fn make_body(&self, cfg: &ToggleStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let initial = if cfg.is_on.get() { 1.0 } else { 0.0 };
        let knob_position = ctx.animated_signal(initial);
        let knob_anim = ctx.animate().fast().standard();
        {
            let knob_position = knob_position.clone();
            let knob_anim = knob_anim.clone();
            ctx.effect(&cfg.is_on, move |on| {
                knob_anim.to_or_snap(&knob_position, if *on { 1.0 } else { 0.0 });
            });
        }

        let focus_origin = cfg
            .is_focused
            .zip(&cfg.is_focus_visible)
            .map(|(focused, visible)| {
                if *focused {
                    Some(if *visible {
                        FocusOrigin::Keyboard
                    } else {
                        FocusOrigin::Pointer
                    })
                } else {
                    None
                }
            });

        ctx.add(M3SwitchBody {
            knob_position,
            is_disabled: cfg.is_disabled.clone(),
            focus_origin,
        })
    }
}

struct M3SwitchBody {
    knob_position: Signal<f32>,
    is_disabled: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
}

impl std::fmt::Debug for M3SwitchBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("M3SwitchBody").finish()
    }
}

/// Colors the switch needs, resolved once per paint (from the M3 palette
/// when present, else from theme roles).
struct SwitchColors {
    track_off: Color,
    track_on: Color,
    outline: Color,
    thumb_off: Color,
    thumb_on: Color,
}

impl SwitchColors {
    fn resolve(ctx: &PaintContext) -> Self {
        if let Some(p) = ctx.theme.extension::<Material3Palette>() {
            Self {
                track_off: p.surface_container_highest,
                track_on: p.primary,
                outline: p.outline,
                thumb_off: p.outline,
                thumb_on: p.on_primary,
            }
        } else {
            let c = &ctx.theme.colors;
            Self {
                track_off: c.surface_sunken,
                track_on: c.accent,
                outline: c.border_strong,
                thumb_off: c.border_strong,
                thumb_on: Color::WHITE,
            }
        }
    }
}

impl Widget for M3SwitchBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.knob_position
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_disabled
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.focus_origin
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(TRACK_W, TRACK_H).into()
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
        let enabled = !self.is_disabled.get();
        let t = self.knob_position.get().clamp(0.0, 1.0);
        let colors = SwitchColors::resolve(ctx);

        let track_x = bounds.x;
        let track_y = bounds.y + (bounds.height - TRACK_H) / 2.0;
        let track_rect = Rect::new(track_x, track_y, TRACK_W, TRACK_H);
        let track_corner = CornerRadius::uniform(TRACK_H / 2.0);

        // Track fill: off-container → primary as the switch fills.
        let mut track_color = colors.track_off.mix(colors.track_on, t);
        if !enabled {
            // M3 disabled track container: 12 % opacity.
            track_color = track_color.with_alpha(0.12);
        }
        canvas.fill_rounded_rect(track_rect, track_corner, track_color);

        // Off-state outline (fades out as the switch turns on).
        let outline_alpha = (1.0 - t) * if enabled { 1.0 } else { 0.38 };
        if outline_alpha > 0.01 {
            canvas.stroke_rounded_rect(
                track_rect,
                track_corner,
                colors
                    .outline
                    .with_alpha(colors.outline.a() * outline_alpha),
                TRACK_OUTLINE_W,
            );
        }

        // Keyboard focus ring, drawn outside the track.
        if self.focus_origin.get() == Some(FocusOrigin::Keyboard) {
            let offset = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width / 2.0;
            let ring_rect = Rect::new(
                track_rect.x - offset,
                track_rect.y - offset,
                track_rect.width + offset * 2.0,
                track_rect.height + offset * 2.0,
            );
            canvas.stroke_rounded_rect(
                ring_rect,
                CornerRadius::uniform(TRACK_H / 2.0 + offset),
                ctx.theme.colors.focus_ring,
                ctx.theme.shape.focus_ring_width,
            );
        }

        // Thumb: grows 16 → 24 dp and slides as it turns on.
        let d = lerp(THUMB_OFF, THUMB_ON, t);
        let center_off = track_x + TRACK_H / 2.0;
        let center_on = track_x + TRACK_W - TRACK_H / 2.0;
        let center_x = lerp(center_off, center_on, t);
        let thumb_rect = Rect::new(center_x - d / 2.0, track_y + (TRACK_H - d) / 2.0, d, d);
        let mut thumb_color = colors.thumb_off.mix(colors.thumb_on, t);
        if !enabled {
            // M3 disabled handle: 38 % opacity.
            thumb_color = thumb_color.with_alpha(0.38);
        }
        canvas.fill_rounded_rect(thumb_rect, CornerRadius::uniform(d / 2.0), thumb_color);
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // The parent Toggle owns accessibility; this body is presentational.
    }
}
