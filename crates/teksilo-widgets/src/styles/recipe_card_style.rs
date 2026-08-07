// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `CardStyle` impl driven by paint-recipe data.
//!
//! `RecipeCardStyle` ships the IntUI card chrome — variant-driven
//! background / shadow / border defaults, with caller overrides
//! winning when set. Custom styles compose freely (glassmorphism card,
//! brutalist box, neumorphic raised surface, etc.) by writing their
//! own `impl CardStyle` block.
//!
//! Like `RecipePanelStyle`, the body is a single `CardFrame` container
//! widget that paints the chrome AND positions the content with
//! padding inset (one widget so the proposal-resolve / intrinsic-size
//! logic mirrors the pre-refactor `Card` exactly — splitting into a
//! ZStack would break proposal propagation).

use teksilo_canvas::{Canvas, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::signal::Prop;
use teksilo_core::styles::{CardStyle, CardStyleConfig, CardVariant};
use teksilo_core::widget::{
    LayoutContext, LayoutResponse, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, Shadow};

// IntUI design tokens for Card. The recipe owns its own dimensions.
pub const CARD_PADDING: f32 = 16.0;
pub const CARD_CORNER_RADIUS: f32 = 8.0;
pub const CARD_BORDER_WIDTH: f32 = 1.0;
/// 0..=1 multiplier on `shape.shadow_inner_md.color.a` at paint time.
pub const CARD_SHADOW_DENSITY: f32 = 0.5;

/// Dimension recipe for `RecipeCardStyle`. Mirrors the `pub const` defaults
/// and allows per-instance overrides without a custom `CardStyle` impl.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardRecipe {
    pub padding: f32,
    pub corner_radius: f32,
    pub border_width: f32,
    pub shadow_density: f32,
}

impl Default for CardRecipe {
    fn default() -> Self {
        Self {
            padding: CARD_PADDING,
            corner_radius: CARD_CORNER_RADIUS,
            border_width: CARD_BORDER_WIDTH,
            shadow_density: CARD_SHADOW_DENSITY,
        }
    }
}

/// Default `CardStyle` shipped with Teksilo. Honours all four
/// `CardVariant` values via background / shadow / border defaults.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeCardStyle {
    pub recipe: CardRecipe,
}

impl RecipeCardStyle {
    pub fn new(recipe: CardRecipe) -> Self {
        Self { recipe }
    }
}

impl CardStyle for RecipeCardStyle {
    fn make_body(&self, cfg: &CardStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let frame = CardFrame {
            child_id: None,
            pending_child: Some(PendingChild::Id(cfg.content)),
            variant: cfg.variant,
            background: cfg.background_override.clone(),
            corner_radius: cfg
                .corner_radius_override
                .clone()
                .unwrap_or(Prop::Static(self.recipe.corner_radius)),
            padding: cfg
                .padding_override
                .clone()
                .unwrap_or(Prop::Static(self.recipe.padding)),
            shadow_override: cfg.shadow_override,
            recipe: self.recipe,
        };
        ctx.add(frame)
    }
}

struct CardFrame {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    variant: CardVariant,
    background: Option<ColorProp>,
    corner_radius: Prop<f32>,
    padding: Prop<f32>,
    shadow_override: Option<Shadow>,
    recipe: CardRecipe,
}

impl std::fmt::Debug for CardFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CardFrame")
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for CardFrame {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        if let Some(p) = &self.background {
            p.register_if_bound(id, registry, BindingLevel::RepaintOnly);
        }
        self.corner_radius
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        self.padding
            .register_if_bound(id, registry, BindingLevel::Relayout);
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let pad = self.padding.get();
        let inset = pad * 2.0;
        if let Some(child_id) = self.child_id {
            let inner_proposal = SizeProposal {
                width: proposal.width.map(|w| (w - inset).max(0.0)),
                height: proposal.height.map(|h| (h - inset).max(0.0)),
            };
            if let Some(child_size) = ctx.child_size(child_id, inner_proposal) {
                return (Size::new(child_size.width + inset, child_size.height + inset)).into();
            }
        }
        proposal.resolve(inset, inset).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let pad = self.padding.get();
        for child in children.iter_mut() {
            child.origin = teksilo_canvas::Point::new(bounds.x + pad, bounds.y + pad);
            child.size = Size::new(
                (bounds.width - pad * 2.0).max(0.0),
                (bounds.height - pad * 2.0).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius = self.corner_radius.get();
        let cr = CornerRadius::uniform(radius);

        // Shadow (and inner counter-shadow) — variant decides density and
        // outer base; caller override (`shadow_override`) wins for the
        // outer shadow only. Plain / Outlined have no shadow; Elevated
        // and Filled use the theme's shadow_md pair; the variant-default
        // gets multiplied by `card.shadow_density` before painting.
        let outer = self.shadow_override.or(match self.variant {
            CardVariant::Plain | CardVariant::Outlined => None,
            CardVariant::Elevated | CardVariant::Filled => Some(ctx.theme.shape.shadow_md),
        });
        if let Some(outer) = outer {
            crate::shadow::paint_layered_shadow(
                canvas,
                bounds,
                cr,
                &outer,
                &ctx.theme.shape.shadow_inner_md,
                self.recipe.shadow_density,
                None,
            );
        }

        // Background — variant default with optional caller override.
        let bg = if let Some(p) = &self.background {
            p.resolve(ctx.theme, ctx.effective_enabled)
        } else {
            match self.variant {
                CardVariant::Plain | CardVariant::Outlined | CardVariant::Elevated => {
                    ctx.theme.colors.surface_main
                }
                CardVariant::Filled => ctx.theme.colors.surface_raised,
            }
        };
        canvas.fill_rounded_rect(bounds, cr, bg);

        // Outlined variant draws a 1 dp accent-neutral border.
        if matches!(self.variant, CardVariant::Outlined) && self.recipe.border_width > 0.0 {
            canvas.stroke_rounded_rect(
                bounds,
                cr,
                ctx.theme.colors.border,
                self.recipe.border_width,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational — the parent Card emits Role::Group.
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
