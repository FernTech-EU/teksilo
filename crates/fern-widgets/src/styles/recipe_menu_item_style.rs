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

use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::styles::{MenuItemStyle, MenuItemStyleConfig};
use fern_core::widget_id::WidgetId;
use fern_tokens::{CornerRadius, SurfaceRole};

use crate::primitives::{FixedSize, HStack, Padding, RectWidget, Spacer, ZStack};

/// Default `MenuItemStyle` shipped with FernUI. Reads dimensions from
/// `theme.components.menu` (`item_height`, `item_padding_horizontal`,
/// `icon_label_gap`, `item_corner_radius`) and chrome roles from the
/// active theme.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeMenuItemStyle;

impl MenuItemStyle for RecipeMenuItemStyle {
    fn make_body(&self, cfg: &MenuItemStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let menu_style = ctx.theme().components.menu;

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
                    .bind_width(menu_style.icon_label_gap)
                    .bind_height(1.0_f32)
                    .child_id(gap_spacer),
            );
            row = row.add_child(gap);
        }

        row = row.add_child(cfg.label);
        row = row.child(Spacer::new());

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
        let pad_v = ((menu_style.item_height - body_line) * 0.5).max(0.0);
        let padding = ctx.add(
            Padding::new(
                pad_v,
                0.0,
                pad_v,
                menu_style.item_padding_horizontal,
            )
            .child_id(row_id),
        );

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
                .corner_radius(CornerRadius::uniform(menu_style.item_corner_radius)),
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
