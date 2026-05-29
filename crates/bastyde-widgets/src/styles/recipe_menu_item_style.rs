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

use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{MenuItemStyle, MenuItemStyleConfig};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{CornerRadius, SurfaceRole};

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

/// Default `MenuItemStyle` shipped with Bastyde. Chrome roles come from
/// the active theme.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeMenuItemStyle;

impl MenuItemStyle for RecipeMenuItemStyle {
    fn make_body(&self, cfg: &MenuItemStyleConfig, ctx: &mut BuildContext) -> WidgetId {
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
                    .bind_width(MENU_ICON_LABEL_GAP)
                    .bind_height(1.0_f32)
                    .child_id(gap_spacer),
            );
            row = row.add_child(gap);
        }

        row = row.add_child(cfg.label);
        // MinSize ensures the shortcut never abuts the label when the menu
        // is narrower than label + shortcut combined.
        row = row.child(MinSize::width(MENU_SHORTCUT_LEFT_GAP).child(Spacer::new()));

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
        let pad_v = ((MENU_ITEM_HEIGHT - body_line) * 0.5).max(0.0);
        let padding =
            ctx.add(Padding::new(pad_v, 0.0, pad_v, MENU_ITEM_PADDING_LEADING).child_id(row_id));

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
                .bind_background(bg_role)
                .corner_radius(CornerRadius::uniform(MENU_ITEM_CORNER_RADIUS)),
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
