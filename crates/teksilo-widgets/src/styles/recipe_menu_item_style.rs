// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `MenuItemStyle` impl driven by paint-recipe data.
//!
//! `RecipeMenuItemStyle` ships the IntUI menu-row chrome:
//! transparent at rest, accent-subtle tint on hover (or highlight via
//! keyboard navigation), pressed surface tint while clicked. The row
//! is composed as `[leading?] [icon-label gap] [label] [Spacer]
//! [trailing?]`, padded vertically to `item_height` and horizontally
//! to `item_padding_horizontal`. The trailing slot's right edge IS
//! the row's right edge — the slot widget should reserve its own
//! right-padding column (typical: shortcut + chevron column inside
//! a HStack).
//!
//! Apps that want a different look (Windows-11 row, macOS pull-down,
//! brutalist square row) write their own `impl MenuItemStyle` block.

use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{MenuItemStyle, MenuItemStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, SurfaceRole};

use crate::primitives::{FixedSize, HStack, MinSize, Padding, RectWidget, Spacer, ZStack};

// IntUI design tokens for MenuItem / MenuList rows. The recipe owns
// its own dimensions. The MenuList / MenuBar / ComboBox panel widgets
// import these constants when they need menu-related row dimensions
// (item height, separator height). The menu *panel* surface (corner
// radius, border, shadow density) is owned by `PopoverStyle` (the
// `Menu` variant).
pub const MENU_ITEM_HEIGHT: f32 = 24.0;
/// Right-side padding column (also used as chevron column width).
pub const MENU_ITEM_PADDING_HORIZONTAL: f32 = 12.0;
/// Leading-side padding before the icon/check column.
pub const MENU_ITEM_PADDING_LEADING: f32 = 6.0;
pub const MENU_ICON_COLUMN_WIDTH: f32 = 16.0;
pub const MENU_ICON_LABEL_GAP: f32 = 6.0;
pub const MENU_SHORTCUT_LEFT_GAP: f32 = 24.0;
pub const MENU_SEPARATOR_HEIGHT: f32 = 9.0;
/// Corner radius of the per-row hover / pressed highlight rect.
pub const MENU_ITEM_CORNER_RADIUS: f32 = 8.0;

/// Recipe dimensions for [`RecipeMenuItemStyle`].
///
/// All fields default to the corresponding module-level `pub const`.
/// Override individual fields to tune the menu row without writing a full
/// custom `MenuItemStyle` impl.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuItemRecipe {
    pub item_height: f32,
    pub padding_horizontal: f32,
    pub padding_leading: f32,
    pub icon_column_width: f32,
    pub icon_label_gap: f32,
    pub shortcut_left_gap: f32,
    pub separator_height: f32,
    pub item_corner_radius: f32,
}

impl Default for MenuItemRecipe {
    fn default() -> Self {
        Self {
            item_height: MENU_ITEM_HEIGHT,
            padding_horizontal: MENU_ITEM_PADDING_HORIZONTAL,
            padding_leading: MENU_ITEM_PADDING_LEADING,
            icon_column_width: MENU_ICON_COLUMN_WIDTH,
            icon_label_gap: MENU_ICON_LABEL_GAP,
            shortcut_left_gap: MENU_SHORTCUT_LEFT_GAP,
            separator_height: MENU_SEPARATOR_HEIGHT,
            item_corner_radius: MENU_ITEM_CORNER_RADIUS,
        }
    }
}

/// Default `MenuItemStyle` shipped with Teksilo. Chrome roles come from
/// the active theme.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeMenuItemStyle {
    pub recipe: MenuItemRecipe,
}

impl RecipeMenuItemStyle {
    pub fn new(recipe: MenuItemRecipe) -> Self {
        Self { recipe }
    }
}

impl MenuItemStyle for RecipeMenuItemStyle {
    fn make_body(&self, cfg: &MenuItemStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let recipe = self.recipe;

        // Row composition: leading | gap | label | Spacer | trailing.
        // HStack spacing is 0 — the only inter-child gap is between
        // leading and label (`icon_label_gap`); everything else is
        // tight (Spacer handles stretch).
        let mut row = HStack::new().spacing(0.0);

        if let Some(leading) = cfg.leading {
            row = row.add_child(leading);
            // Explicit icon-to-label gap. Fixed-width Spacer rather than
            // HStack::spacing so we don't inject gaps around every other
            // child (which would push the trailing slot inward).
            let gap_spacer = ctx.add(Spacer::new());
            let gap = ctx.add(
                FixedSize::new()
                    .width(recipe.icon_label_gap)
                    .height(1.0_f32)
                    .child_id(gap_spacer),
            );
            row = row.add_child(gap);
        }

        row = row.add_child(cfg.label);
        // MinSize ensures the shortcut never abuts the label when the menu
        // is narrower than label + shortcut combined.
        row = row.child(MinSize::width(recipe.shortcut_left_gap).child(Spacer::new()));

        if let Some(trailing) = cfg.trailing {
            row = row.add_child(trailing);
        }

        let row_id = ctx.add(row);

        // Padding: vertical derived so the row has the full
        // `item_height` after the body line height. Horizontal: only
        // left padding here — the trailing slot is responsible for
        // its own right-padding column (matches the pre-refactor
        // MenuItem semantic so submenu and regular items line up).
        let body = &ctx.theme().typography.body;
        let body_line = body.size * body.line_height;
        let pad_v = ((recipe.item_height - body_line) * 0.5).max(0.0);
        let padding =
            ctx.add(Padding::new(pad_v, 0.0, pad_v, recipe.padding_leading).child_id(row_id));

        // Background — Hover / Highlighted both use AccentSubtle (the
        // same row tint), Pressed uses Pressed, Disabled stays
        // Transparent.
        let bg_role = bg_signal(
            &cfg.is_pressed,
            &cfg.is_hovered,
            &cfg.is_highlighted,
            &cfg.is_disabled,
        );
        let bg = ctx.add(
            RectWidget::new()
                .background(bg_role)
                .corner_radius(CornerRadius::uniform(recipe.item_corner_radius)),
        );

        ctx.add(ZStack::new().add_child(bg).add_child(padding))
    }
}

fn bg_signal(
    is_pressed: &Signal<bool>,
    is_hovered: &Signal<bool>,
    is_highlighted: &Signal<bool>,
    is_disabled: &Signal<bool>,
) -> Signal<SurfaceRole> {
    let combined = is_pressed.zip3(is_hovered, is_highlighted);
    combined
        .zip(is_disabled)
        .map(|((pressed, hovered, highlighted), disabled)| {
            if *disabled {
                SurfaceRole::Transparent
            } else if *pressed {
                SurfaceRole::Pressed
            } else if *hovered || *highlighted {
                SurfaceRole::AccentSubtle
            } else {
                SurfaceRole::Transparent
            }
        })
}
