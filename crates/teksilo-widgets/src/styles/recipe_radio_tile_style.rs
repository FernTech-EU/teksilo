// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `RadioTileStyle` impl driven by paint-recipe data.
//!
//! `RecipeRadioTileStyle` ships the IntUI "selectable card" chrome: a
//! rounded surface with a reactive fill + border cascade driven by the
//! tile's selection / hover / press / focus state, an optional elevation
//! shadow for the `Elevated` variant, and content inset by padding.
//!
//! Like [`RecipeCardStyle`](crate::styles::RecipeCardStyle), the body is a
//! single `RadioTileFrame` container widget that paints the chrome **and**
//! positions the content with a padding inset — a bespoke frame rather than
//! a `ZStack`, because a `ZStack` measures children at `unspecified()` and
//! would break the height-for-width measurement of the tile's wrapping
//! description (its documented limitation).
//!
//! The fill / border cascade mirrors
//! [`RecipeStandardItemStyle`](crate::styles::RecipeStandardItemStyle): a
//! selected tile shows the vivid `AccentSubtle` surface only while focused
//! and window-active, else the muted `SelectedInactive`; the keyboard focus
//! ring (`BorderRole::Focused`) appears only under
//! `selected && focused && focus-visible`.

use teksilo_canvas::{Canvas, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{RadioTileStyle, RadioTileStyleConfig, RadioTileVariant};
use teksilo_core::widget::{
    LayoutContext, LayoutResponse, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{BorderRole, CornerRadius, SurfaceRole};

// IntUI design tokens for RadioTile. The recipe owns its own dimensions.
// Corner radius matches Button (4 dp) / SegmentedControl (3 dp) — a control,
// not a rounded card.
pub const RADIO_TILE_CORNER_RADIUS: f32 = 4.0;
pub const RADIO_TILE_PADDING: f32 = 14.0;
pub const RADIO_TILE_BORDER_WIDTH: f32 = 1.0;
pub const RADIO_TILE_SELECTED_BORDER_WIDTH: f32 = 1.5;
pub const RADIO_TILE_FOCUS_RING_WIDTH: f32 = 2.0;
/// 0..=1 multiplier on the elevation shadow alpha at paint time (Elevated variant).
pub const RADIO_TILE_SHADOW_DENSITY: f32 = 0.5;
/// Fixed row height (logical px) for a `RadioTileGroup` in
/// `TileLayout::Vertical` — the compact settings-list arrangement.
pub const RADIO_TILE_VERTICAL_ROW_HEIGHT: f32 = 44.0;

/// Dimension recipe for [`RecipeRadioTileStyle`]. Mirrors the `pub const`
/// defaults and allows per-instance overrides without a custom
/// `RadioTileStyle` impl.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadioTileRecipe {
    pub corner_radius: f32,
    pub padding: f32,
    pub border_width: f32,
    pub selected_border_width: f32,
    pub focus_ring_width: f32,
    pub shadow_density: f32,
    /// Fixed row height for `TileLayout::Vertical` compact rows.
    pub vertical_row_height: f32,
}

impl Default for RadioTileRecipe {
    fn default() -> Self {
        Self {
            corner_radius: RADIO_TILE_CORNER_RADIUS,
            padding: RADIO_TILE_PADDING,
            border_width: RADIO_TILE_BORDER_WIDTH,
            selected_border_width: RADIO_TILE_SELECTED_BORDER_WIDTH,
            focus_ring_width: RADIO_TILE_FOCUS_RING_WIDTH,
            shadow_density: RADIO_TILE_SHADOW_DENSITY,
            vertical_row_height: RADIO_TILE_VERTICAL_ROW_HEIGHT,
        }
    }
}

/// Default `RadioTileStyle` shipped with Teksilo.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeRadioTileStyle {
    pub recipe: RadioTileRecipe,
}

impl RecipeRadioTileStyle {
    pub fn new(recipe: RadioTileRecipe) -> Self {
        Self { recipe }
    }
}

impl RadioTileStyle for RecipeRadioTileStyle {
    fn vertical_row_height(&self) -> f32 {
        self.recipe.vertical_row_height
    }

    fn make_body(&self, cfg: &RadioTileStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let variant = cfg.variant;
        // Unchosen tiles are transparent (matching SegmentedControl's
        // unselected segments); selection tints the fill + border. The
        // keyboard focus ring is drawn once around the whole `RadioTileGroup`,
        // not per tile — so this style paints no per-tile ring.
        let bg_role = tile_bg_signal(
            &cfg.is_selected,
            &cfg.is_pressed,
            &cfg.is_hovered,
            &cfg.is_window_active,
            variant,
        );

        let border_width_base = self.recipe.border_width;

        // Selection is shown by the tinted fill + the filled radio dot, not a
        // bright accent border. The border stays neutral (hover strengthens it).
        let border_state = cfg.is_hovered.map(move |hov| {
            if *hov {
                (BorderRole::Strong, border_width_base)
            } else {
                // Filled variant is fill-only (transparent border); Outlined /
                // Elevated show a neutral 1 dp border.
                match variant {
                    RadioTileVariant::Filled => (BorderRole::Transparent, border_width_base),
                    _ => (BorderRole::Default, border_width_base),
                }
            }
        });
        let border_role = border_state.map(|(r, _)| *r);
        let border_width = border_state.map(|(_, w)| *w);

        let frame = RadioTileFrame {
            content_id: None,
            pending_content: Some(PendingChild::Id(cfg.content)),
            bg: ColorProp::DynamicSurfaceRole(bg_role),
            border: ColorProp::DynamicBorderRole(border_role),
            border_width: Prop::Bound(border_width),
            corner_radius: self.recipe.corner_radius,
            padding: self.recipe.padding,
            variant,
            shadow_density: self.recipe.shadow_density,
            is_compact: cfg.is_compact,
        };
        ctx.add(frame)
    }
}

/// Resting → interactive → selected surface cascade for a tile. The resting
/// (unchosen) surface is transparent — matching SegmentedControl's unselected
/// segments — except the `Filled` variant, which rests on a `Container`
/// surface. Selection tints the fill (`AccentSubtle`), desaturating to the
/// muted `SelectedInactive` when the window is inactive (the window-active
/// convention, shared with `StandardListItem`).
fn tile_bg_signal(
    is_selected: &Signal<bool>,
    is_pressed: &Signal<bool>,
    is_hovered: &Signal<bool>,
    is_window_active: &Signal<bool>,
    variant: RadioTileVariant,
) -> Signal<SurfaceRole> {
    let combined = is_selected.zip3(is_pressed, is_hovered);
    combined
        .zip(is_window_active)
        .map(move |((selected, pressed, hovered), window_active)| {
            let resting = match variant {
                RadioTileVariant::Filled => SurfaceRole::Container,
                _ => SurfaceRole::Transparent,
            };
            if *selected {
                if *window_active {
                    SurfaceRole::AccentSubtle
                } else {
                    SurfaceRole::SelectedInactive
                }
            } else if *pressed {
                SurfaceRole::Pressed
            } else if *hovered {
                SurfaceRole::Hover
            } else {
                resting
            }
        })
}

/// Single frame widget: paints the tile chrome (shadow + fill + border) and
/// positions the content child inset by `padding`. Modeled on `CardFrame`
/// so proposal propagation / height-for-width matches a padded container.
struct RadioTileFrame {
    content_id: Option<WidgetId>,
    pending_content: Option<PendingChild>,
    bg: ColorProp,
    border: ColorProp,
    border_width: Prop<f32>,
    corner_radius: f32,
    padding: f32,
    variant: RadioTileVariant,
    shadow_density: f32,
    /// Compact single-line row: horizontal padding only, content centered
    /// vertically in the fixed height (no over-constraint when short).
    is_compact: bool,
}

impl std::fmt::Debug for RadioTileFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadioTileFrame")
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for RadioTileFrame {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_content.take() {
            self.content_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.bg
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        self.border
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        self.border_width
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        self.content_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let inset = self.padding * 2.0;
        if let Some(child_id) = self.content_id {
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
        ctx: &LayoutContext,
    ) {
        let pad = self.padding;
        let inner_w = (bounds.width - pad * 2.0).max(0.0);
        for child in children.iter_mut() {
            if self.is_compact {
                // Compact single-line row: horizontal padding, and the content
                // row is given its natural height centered in the fixed row
                // height — so a short `TileLayout::Vertical` height never
                // over-constrains the content (no clipping / overflow stripes).
                let content_h = ctx
                    .child_size(
                        child.id,
                        SizeProposal {
                            width: Some(inner_w),
                            height: None,
                        },
                    )
                    .map(|s| s.height)
                    .unwrap_or(bounds.height)
                    .min(bounds.height);
                let y = bounds.y + ((bounds.height - content_h) * 0.5).max(0.0);
                child.origin = teksilo_canvas::Point::new(bounds.x + pad, y);
                child.size = Size::new(inner_w, content_h);
            } else {
                child.origin = teksilo_canvas::Point::new(bounds.x + pad, bounds.y + pad);
                child.size = Size::new(inner_w, (bounds.height - pad * 2.0).max(0.0));
            }
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let cr = CornerRadius::uniform(self.corner_radius);

        // Elevation shadow (Elevated variant only), painted before the fill.
        if matches!(self.variant, RadioTileVariant::Elevated) {
            crate::shadow::paint_layered_shadow(
                canvas,
                bounds,
                cr,
                &ctx.theme.shape.shadow_md,
                &ctx.theme.shape.shadow_inner_md,
                self.shadow_density,
                None,
            );
        }

        // Fill.
        let bg = self.bg.resolve(ctx.theme, ctx.effective_enabled);
        canvas.fill_rounded_rect(bounds, cr, bg);

        // Border — skipped when the role resolves transparent (Filled resting)
        // or the width is zero.
        let bc = self.border.resolve(ctx.theme, ctx.effective_enabled);
        let bw = self.border_width.get();
        if bw > 0.0 && bc.a() > 0.0 {
            canvas.stroke_rounded_rect(bounds, cr, bc, bw);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational — the parent RadioTile emits Role::RadioButton.
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.content_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_surface_tracks_window_active_resting_is_transparent() {
        let selected = Signal::new(true);
        let pressed = Signal::new(false);
        let hovered = Signal::new(false);
        let window_active = Signal::new(true);

        let role = tile_bg_signal(
            &selected,
            &pressed,
            &hovered,
            &window_active,
            RadioTileVariant::Outlined,
        );
        // Selected + window-active → vivid; inactive → muted.
        assert_eq!(role.get(), SurfaceRole::AccentSubtle);
        window_active.set(false);
        assert_eq!(role.get(), SurfaceRole::SelectedInactive);

        // Unchosen resting surface is transparent (SegmentedControl match).
        selected.set(false);
        window_active.set(true);
        assert_eq!(role.get(), SurfaceRole::Transparent);
    }

    #[test]
    fn filled_variant_rests_on_container_surface() {
        let selected = Signal::new(false);
        let pressed = Signal::new(false);
        let hovered = Signal::new(false);
        let window_active = Signal::new(true);
        let role = tile_bg_signal(
            &selected,
            &pressed,
            &hovered,
            &window_active,
            RadioTileVariant::Filled,
        );
        assert_eq!(role.get(), SurfaceRole::Container);
    }
}
