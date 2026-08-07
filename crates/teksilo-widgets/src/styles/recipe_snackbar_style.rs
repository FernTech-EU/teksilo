// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `SnackbarStyle` impl driven by paint-recipe data.
//!
//! `RecipeSnackbarStyle` ships the IntUI snackbar chrome: the
//! high-contrast (dark) `tooltip_bg` surface with a `tooltip_border`
//! stroke and rounded corners, content inset by the snackbar padding.
//!
//! Apps that want a different notification look (light surface,
//! status-tinted background, branded chrome) write their own
//! `impl SnackbarStyle` block and install it per-call
//! (`Snackbar::style(...)`) or theme-wide (`theme.style_slots.snackbar`).

use teksilo_canvas::{Canvas, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::{SnackbarStyle, SnackbarStyleConfig};
use teksilo_core::widget::{
    LayoutContext, LayoutResponse, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::CornerRadius;

// IntUI design tokens for Snackbar. The recipe owns its own dimensions.
pub const SNACKBAR_PADDING_HORIZONTAL: f32 = 12.0;
pub const SNACKBAR_PADDING_VERTICAL: f32 = 10.0;
pub const SNACKBAR_CORNER_RADIUS: f32 = 8.0;

/// Configurable dimensions for [`RecipeSnackbarStyle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnackbarRecipe {
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub corner_radius: f32,
}

impl Default for SnackbarRecipe {
    fn default() -> Self {
        Self {
            padding_horizontal: SNACKBAR_PADDING_HORIZONTAL,
            padding_vertical: SNACKBAR_PADDING_VERTICAL,
            corner_radius: SNACKBAR_CORNER_RADIUS,
        }
    }
}

/// Default `SnackbarStyle` shipped with Teksilo. Chrome from
/// `theme.colors.tooltip_bg` + `tooltip_border`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeSnackbarStyle {
    pub recipe: SnackbarRecipe,
}

impl RecipeSnackbarStyle {
    pub fn new(recipe: SnackbarRecipe) -> Self {
        Self { recipe }
    }
}

impl SnackbarStyle for RecipeSnackbarStyle {
    fn make_body(&self, cfg: &SnackbarStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(SnackbarFrame {
            child_id: None,
            pending_child: Some(PendingChild::Id(cfg.content)),
            recipe: self.recipe,
        })
    }
}

/// Internal container that paints the snackbar chrome (dark
/// `tooltip_bg` surface + `tooltip_border` stroke + corner radius) and
/// positions the content with the snackbar padding inset. Mirrors the
/// pre-migration `SnackbarSurface` layout exactly.
struct SnackbarFrame {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    recipe: SnackbarRecipe,
}

impl std::fmt::Debug for SnackbarFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnackbarFrame").finish()
    }
}

impl Widget for SnackbarFrame {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let inset_x = self.recipe.padding_horizontal * 2.0;
        let inset_y = self.recipe.padding_vertical * 2.0;
        let content = self
            .child_id
            .and_then(|id| {
                ctx.child_size(
                    id,
                    SizeProposal {
                        width: proposal.width.map(|width| (width - inset_x).max(0.0)),
                        height: proposal.height.map(|height| (height - inset_y).max(0.0)),
                    },
                )
            })
            .unwrap_or_else(|| proposal.resolve(220.0, 44.0));

        Size::new(content.width + inset_x, content.height + inset_y).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = teksilo_canvas::Point::new(
                bounds.x + self.recipe.padding_horizontal,
                bounds.y + self.recipe.padding_vertical,
            );
            child.size = Size::new(
                (bounds.width - self.recipe.padding_horizontal * 2.0).max(0.0),
                (bounds.height - self.recipe.padding_vertical * 2.0).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius = CornerRadius::uniform(self.recipe.corner_radius);
        // Notifications use the (dark) tooltip surface for high-contrast popups.
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.tooltip_bg);
        canvas.stroke_rounded_rect(
            bounds,
            radius,
            ctx.theme.colors.tooltip_border,
            ctx.theme.shape.border_width,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational — the parent `SnackbarSurface` emits the
        // `Role::Alert` + `Live::Polite` node with the announcement.
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
