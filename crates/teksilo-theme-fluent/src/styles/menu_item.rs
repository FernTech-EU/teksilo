// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Fluent menu row.
//!
//! Structurally this is the same `[leading] [label] [spacer] [trailing]`
//! row the IntUI recipe builds, so the layout is delegated to
//! [`RecipeMenuItemStyle`] with Fluent's metrics —
//! `MenuFlyoutItemThemePadding` `11,8,11,9`, a 28 dp icon/check column
//! (`MenuFlyoutItemPlaceholderThemeThickness`), and `ControlCornerRadius`
//! on the highlight.
//!
//! What cannot be delegated is the **hover colour**. IntUI tints a hovered
//! row with `SurfaceRole::AccentSubtle`; Fluent's menu hover is
//! `SubtleFillColorSecondary`, a *neutral* wash — an accent-tinted menu row
//! is one of the fastest ways to make a Fluent app look not-Fluent. So the
//! row background is painted here and the delegate is handed a row with no
//! background of its own.
//!
//! Rather than reimplement the row layout to swap one colour, the
//! delegated body is stacked *over* a Fluent background rect and the
//! delegate's own tint is suppressed by giving it interaction signals that
//! never fire. That keeps the slot/gap/padding arithmetic in one place.
//!
//! The style stores the recipe it lays out, built from the theme's input
//! tokens ([`FluentMenuItemStyle::for_tokens`]), so
//! [`MenuItemStyle::metrics`], which `MenuItem` and `MenuSeparator` read for
//! the parts they build themselves and `MenuList` reads to cap its height,
//! follows the density the theme was projected to, as the painted row does.

use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::density::{dp, spacing};
use teksilo_core::styles::{MenuItemMetrics, MenuItemStyle, MenuItemStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, InputTokens, SurfaceRole, TargetRole};
use teksilo_widgets::primitives::{RectWidget, ZStack};
use teksilo_widgets::styles::{MenuItemRecipe, RecipeMenuItemStyle};

use crate::shape::FLUENT_CONTROL_CORNER_RADIUS;

/// `MenuFlyoutItemThemePadding` top + bottom around a 20 dp Body line box.
const ITEM_HEIGHT: f32 = 8.0 + 20.0 + 9.0;
/// `MenuFlyoutItemThemePadding` leading / trailing (dp).
const PADDING_H: f32 = 11.0;
/// `MenuFlyoutItemPlaceholderThemeThickness` — the icon / check column.
const ICON_COLUMN: f32 = 16.0;
/// Gap from the icon column to the label, so icon + gap lands on the 28 dp
/// placeholder inset WinUI reserves.
const ICON_LABEL_GAP: f32 = 12.0;
/// Minimum gutter between a label and its shortcut chip (dp).
const SHORTCUT_GAP: f32 = 24.0;
/// `MenuFlyoutSeparatorThemePadding` is `-4,1,-4,1` around a 1 dp line.
const SEPARATOR_HEIGHT: f32 = 3.0;

/// The Fluent [`MenuItemRecipe`] — public so an app can start from it and
/// tune one dimension without rebuilding the whole style.
pub fn fluent_menu_item_recipe() -> MenuItemRecipe {
    fluent_menu_item_recipe_for(&InputTokens::default())
}

/// [`fluent_menu_item_recipe`] resolved against a density's [`InputTokens`].
///
/// `[WinUI]` values throughout, raised where the density asks for a bigger
/// target: `MenuFlyoutThemeMinHeight` already clears the 24 dp AA floor at
/// Compact, so this is the identity there.
pub fn fluent_menu_item_recipe_for(tokens: &InputTokens) -> MenuItemRecipe {
    MenuItemRecipe {
        item_height: dp(ITEM_HEIGHT, TargetRole::Target, tokens),
        padding_horizontal: spacing(PADDING_H, tokens),
        padding_leading: spacing(PADDING_H, tokens),
        icon_column_width: ICON_COLUMN,
        icon_label_gap: spacing(ICON_LABEL_GAP, tokens),
        shortcut_left_gap: spacing(SHORTCUT_GAP, tokens),
        separator_height: spacing(SEPARATOR_HEIGHT, tokens),
        item_corner_radius: FLUENT_CONTROL_CORNER_RADIUS,
    }
}

/// Fluent `MenuItemStyle`.
///
/// [`Default`] is the Compact density; the preset installs
/// [`FluentMenuItemStyle::for_tokens`] of the theme it builds, and builds it
/// again when the theme is projected to another density.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FluentMenuItemStyle {
    /// The row dimensions this style lays out and reports.
    pub recipe: MenuItemRecipe,
}

impl FluentMenuItemStyle {
    /// The style with `recipe`'s dimensions.
    pub fn new(recipe: MenuItemRecipe) -> Self {
        Self { recipe }
    }

    /// The style at the density of `tokens`: [`fluent_menu_item_recipe_for`].
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self::new(fluent_menu_item_recipe_for(tokens))
    }
}

impl Default for FluentMenuItemStyle {
    fn default() -> Self {
        Self::new(fluent_menu_item_recipe())
    }
}

impl MenuItemStyle for FluentMenuItemStyle {
    fn make_body(&self, cfg: &MenuItemStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Fluent's neutral row wash: `SubtleFillColorSecondary` on hover or
        // keyboard highlight, `SubtleFillColorTertiary` on press.
        let bg = row_surface(
            &cfg.is_pressed,
            &cfg.is_hovered,
            &cfg.is_highlighted,
            &cfg.is_disabled,
        );
        let backdrop = ctx.add(
            RectWidget::new()
                .background(bg)
                .corner_radius(CornerRadius::uniform(FLUENT_CONTROL_CORNER_RADIUS)),
        );

        // Delegate the row arithmetic, with the interaction signals held
        // low so the IntUI accent tint never paints under ours.
        let quiet = Signal::new(false);
        let inner_cfg = MenuItemStyleConfig {
            label: cfg.label,
            leading: cfg.leading,
            trailing: cfg.trailing,
            is_hovered: quiet.clone(),
            is_pressed: quiet.clone(),
            is_focused: cfg.is_focused.clone(),
            is_disabled: cfg.is_disabled.clone(),
            is_highlighted: quiet,
        };
        let row = RecipeMenuItemStyle::new(self.recipe).make_body(&inner_cfg, ctx);

        ctx.add(ZStack::new().child(backdrop).child(row))
    }

    /// Delegate the metrics alongside the row, from the same recipe.
    /// `MenuItem` builds the icon and chevron columns before any style runs
    /// and `MenuSeparator` is a different widget entirely, so without this
    /// the slots would keep the IntUI sizes while the row around them used
    /// ours, and a metric read at Compact would sit in a row laid out at
    /// another density.
    fn metrics(&self) -> MenuItemMetrics {
        RecipeMenuItemStyle::new(self.recipe).metrics()
    }
}

/// `Pressed > Hover | Highlighted > nothing`, with disabled always inert.
fn row_surface(
    is_pressed: &Signal<bool>,
    is_hovered: &Signal<bool>,
    is_highlighted: &Signal<bool>,
    is_disabled: &Signal<bool>,
) -> Signal<SurfaceRole> {
    is_pressed
        .zip3(is_hovered, is_highlighted)
        .zip(is_disabled)
        .map(|((pressed, hovered, highlighted), disabled)| {
            if *disabled {
                SurfaceRole::Transparent
            } else if *pressed {
                // `SubtleFillColorTertiary`, via the Fluent colour mapping.
                SurfaceRole::Pressed
            } else if *hovered || *highlighted {
                // `SubtleFillColorSecondary`.
                SurfaceRole::Hover
            } else {
                SurfaceRole::Transparent
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_carries_the_winui_menu_metrics() {
        let r = fluent_menu_item_recipe();
        // 8 + 20 + 9 — `MenuFlyoutItemThemePadding` around a Body line box.
        assert_eq!(r.item_height, 37.0);
        assert_eq!(r.padding_horizontal, 11.0);
        assert_eq!(r.padding_leading, 11.0);
        assert_eq!(r.item_corner_radius, FLUENT_CONTROL_CORNER_RADIUS);
    }

    #[test]
    fn icon_column_plus_gap_reaches_the_placeholder_inset() {
        // `MenuFlyoutItemPlaceholderThemeThickness` is 28,0,0,0 — the
        // label of an icon-less row lines up with one that has an icon.
        let r = fluent_menu_item_recipe();
        assert_eq!(r.icon_column_width + r.icon_label_gap, 28.0);
    }

    /// The metrics `MenuItem`, `MenuSeparator` and `MenuList` read are the
    /// stored recipe's, at the density it was built for: the row rises to
    /// the Touch target, the gutters and the separator follow the spacing
    /// factor, and Compact is the recipe this module documents.
    #[test]
    fn the_metrics_are_the_recipes_at_every_density() {
        use teksilo_tokens::TargetDensity;

        assert_eq!(
            FluentMenuItemStyle::default().recipe,
            fluent_menu_item_recipe()
        );
        for (density, item, separator, trailing) in [
            (TargetDensity::Compact, 37.0, 3.0, 11.0),
            (TargetDensity::Comfortable, 37.0, 3.45, 12.65),
            (TargetDensity::Touch, 44.0, 3.9, 14.3),
        ] {
            let tokens = InputTokens::for_density(density);
            let style = FluentMenuItemStyle::for_tokens(&tokens);
            assert_eq!(style.recipe, fluent_menu_item_recipe_for(&tokens));
            let m = style.metrics();
            assert_eq!(m.item_height, item, "{density:?}");
            assert!((m.separator_height - separator).abs() < 1e-4, "{density:?}");
            assert!(
                (m.trailing_column_width - trailing).abs() < 1e-4,
                "{density:?}"
            );
            assert_eq!(m.icon_column_width, ICON_COLUMN, "{density:?}");
        }
    }

    #[test]
    fn rows_are_taller_than_the_intui_default() {
        // Fluent menus are deliberately roomy; a 24 dp row would read as a
        // different design language.
        assert!(fluent_menu_item_recipe().item_height > MenuItemRecipe::default().item_height);
    }
}
