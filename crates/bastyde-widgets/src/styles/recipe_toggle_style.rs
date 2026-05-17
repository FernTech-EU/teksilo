//! Default `ToggleStyle` impl driven by paint-recipe data.
//!
//! `RecipeToggleStyle` ships the IntUI look out of the box; apps that
//! want a different design language (Material 3 switch, neumorphic
//! toggle, Cupertino) write their own `impl ToggleStyle` block and
//! install it per-call (`Toggle::style(...)`) or theme-wide
//! (step 8's `ComponentStyles.toggle = Rc::new(MyToggle)`).
//!
//! The visual body is a tiny custom leaf widget ([`ToggleBody`]) that
//! paints track + knob on the canvas. We could compose
//! `RectWidget(track) | Position(knob_x, RectWidget(knob))` instead,
//! but no general-purpose absolute-positioning primitive exists today
//! and the compositional version isn't free either — it adds one
//! layout pass and two arena nodes per Toggle. The leaf body keeps
//! parity with the paint-cost of the pre-refactor Toggle. Custom
//! `ToggleStyle` impls are free to compose if they prefer.

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::focus::FocusOrigin;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{ToggleStyle, ToggleStyleConfig, ToggleVariant};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{Color, CornerRadius};

// IntUI design tokens for the Toggle chrome. Used to live in
// `theme.components.toggle`; moved here as part of Step 7 of the
// styling refactor — "The recipe IS the new dimension data; no
// parallel store." Custom design languages override these by
// providing their own `impl ToggleStyle`.
pub const TOGGLE_TRACK_WIDTH: f32 = 28.0;
pub const TOGGLE_TRACK_HEIGHT: f32 = 16.0;
pub const TOGGLE_THUMB_DIAMETER: f32 = 12.0;
pub const TOGGLE_THUMB_INSET: f32 = 2.0;

/// Default `ToggleStyle` shipped with Bastyde. Reads its dimensions
/// from `theme.components.toggle` and its colors from
/// `theme.colors.{accent, surface_sunken, ...}`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeToggleStyle;

impl ToggleStyle for RecipeToggleStyle {
    fn make_body(&self, cfg: &ToggleStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Animated knob position — separate signal, registered with
        // the animation scheduler. Tracks `is_on` via an effect.
        let initial = if cfg.is_on.get() { 1.0 } else { 0.0 };
        let knob_position = ctx.animated_signal(initial);
        let knob_anim = ctx.animate().fast().standard();

        // When `is_on` flips, tween the knob position to the new end.
        // `to_or_snap` honours `prefers-reduced-motion` (snaps under
        // that flag instead of tweening).
        {
            let knob_position = knob_position.clone();
            let knob_anim = knob_anim.clone();
            ctx.effect(&cfg.is_on, move |on| {
                let target = if *on { 1.0 } else { 0.0 };
                knob_anim.to_or_snap(&knob_position, target);
            });
        }

        // Focus-origin signal, derived from is_focused × is_hovered.
        // Pointer-induced focus skips the focus ring; keyboard focus
        // shows it.
        let focus_origin = cfg
            .is_focused
            .zip(&cfg.is_hovered)
            .map(|(focused, hovered)| {
                if *focused {
                    Some(if *hovered {
                        FocusOrigin::Pointer
                    } else {
                        FocusOrigin::Keyboard
                    })
                } else {
                    None
                }
            });

        ctx.add(ToggleBody {
            knob_position,
            is_disabled: cfg.is_disabled.clone(),
            focus_origin,
            variant: cfg.variant,
        })
    }
}

/// Internal leaf widget that paints the track + knob. Owned by
/// `RecipeToggleStyle::make_body`; not exposed publicly because custom
/// `ToggleStyle` impls compose their own body instead.
struct ToggleBody {
    knob_position: Signal<f32>,
    is_disabled: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    variant: ToggleVariant,
}

impl std::fmt::Debug for ToggleBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToggleBody")
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for ToggleBody {
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
        let row_h = TOGGLE_TRACK_HEIGHT.max(24.0);
        Size::new(TOGGLE_TRACK_WIDTH, row_h).into()
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
        let track_w = TOGGLE_TRACK_WIDTH;
        let track_h = TOGGLE_TRACK_HEIGHT;
        let knob_size = TOGGLE_THUMB_DIAMETER;
        let knob_inset = TOGGLE_THUMB_INSET;
        let enabled = !self.is_disabled.get();

        // Track is centered in the (possibly larger) hit-area row.
        let track_x = bounds.x;
        let track_y = bounds.y + (bounds.height - track_h) / 2.0;
        let track_rect = Rect::new(track_x, track_y, track_w, track_h);

        // Track background — interpolates surface→accent with the knob
        // position so the slide reads visually as the toggle "filling".
        let t = self.knob_position.get();
        let track_color = if !enabled {
            colors.accent_disabled
        } else {
            let off = colors.surface_sunken;
            let on = colors.accent;
            Color::new(
                bastyde_tokens::lerp(off.r(), on.r(), t),
                bastyde_tokens::lerp(off.g(), on.g(), t),
                bastyde_tokens::lerp(off.b(), on.b(), t),
                bastyde_tokens::lerp(off.a(), on.a(), t),
            )
        };

        // Variant-specific corner radius. Switch / Pill use full
        // pill ends; Square is sharp; Inset is slightly rounded.
        let track_corner = match self.variant {
            ToggleVariant::Switch | ToggleVariant::Pill => CornerRadius::uniform(track_h / 2.0),
            ToggleVariant::Square => CornerRadius::uniform(0.0),
            ToggleVariant::Inset => CornerRadius::uniform(4.0),
        };
        canvas.fill_rounded_rect(track_rect, track_corner, track_color);

        // Focus ring — keyboard-only, drawn outside the track in the
        // theme-defined gap.
        if self.focus_origin.get() == Some(FocusOrigin::Keyboard) {
            let offset = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width / 2.0;
            let ring_rect = Rect::new(
                track_rect.x - offset,
                track_rect.y - offset,
                track_rect.width + offset * 2.0,
                track_rect.height + offset * 2.0,
            );
            let ring_corner = match self.variant {
                ToggleVariant::Switch | ToggleVariant::Pill => {
                    CornerRadius::uniform(track_h / 2.0 + offset)
                }
                ToggleVariant::Square => CornerRadius::uniform(offset),
                ToggleVariant::Inset => CornerRadius::uniform(4.0 + offset),
            };
            canvas.stroke_rounded_rect(
                ring_rect,
                ring_corner,
                colors.focus_ring,
                ctx.theme.shape.focus_ring_width,
            );
        }

        // Knob — only painted for variants that have one.
        if matches!(self.variant, ToggleVariant::Switch | ToggleVariant::Square) {
            let min_x = track_x + knob_inset;
            let max_x = track_x + track_w - knob_size - knob_inset;
            let knob_x = bastyde_tokens::lerp(min_x, max_x, t.clamp(0.0, 1.0));
            let knob_y = track_y + (track_h - knob_size) / 2.0;
            let knob_rect = Rect::new(knob_x, knob_y, knob_size, knob_size);
            let knob_color = if !enabled {
                colors.text_disabled
            } else {
                Color::WHITE
            };
            let knob_corner = match self.variant {
                ToggleVariant::Switch => CornerRadius::uniform(knob_size / 2.0),
                ToggleVariant::Square => CornerRadius::uniform(0.0),
                _ => unreachable!(),
            };
            canvas.fill_rounded_rect(knob_rect, knob_corner, knob_color);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Accessibility lives on the parent Toggle widget, not the
        // body. The body is presentational — the AT walker reaches
        // the Toggle node directly.
    }
}
