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

use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{MenuItemStyle, MenuItemStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, SurfaceRole};
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
    MenuItemRecipe {
        item_height: ITEM_HEIGHT,
        padding_horizontal: PADDING_H,
        padding_leading: PADDING_H,
        icon_column_width: ICON_COLUMN,
        icon_label_gap: ICON_LABEL_GAP,
        shortcut_left_gap: SHORTCUT_GAP,
        separator_height: SEPARATOR_HEIGHT,
        item_corner_radius: FLUENT_CONTROL_CORNER_RADIUS,
    }
}

/// Fluent `MenuItemStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct FluentMenuItemStyle;

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
        let row = RecipeMenuItemStyle::new(fluent_menu_item_recipe()).make_body(&inner_cfg, ctx);

        ctx.add(ZStack::new().add_child(backdrop).add_child(row))
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

    #[test]
    fn rows_are_taller_than_the_intui_default() {
        // Fluent menus are deliberately roomy; a 24 dp row would read as a
        // different design language.
        assert!(fluent_menu_item_recipe().item_height > MenuItemRecipe::default().item_height);
    }
}
