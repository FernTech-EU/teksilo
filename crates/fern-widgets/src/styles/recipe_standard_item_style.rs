//! Default `StandardItemStyle` impl driven by paint-recipe data.
//!
//! `RecipeStandardItemStyle` reproduces the IntUI selection chrome
//! used by `StandardListItem` / `StandardTreeItem`: a rounded
//! selection background inset horizontally so the rounded corners
//! show, with content padded inside. Selection / hover / pressed all
//! cycle through the same `SurfaceRole::{Selected | AccentSubtle |
//! Pressed}` cascade.
//!
//! The host widget composes the row contents (`[checkbox?]
//! [leading?] [center?] label_column [Spacer] [trailing?]`) and
//! passes the result as `cfg.content`; this style only owns the
//! chrome.

use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::styles::{StandardItemStyle, StandardItemStyleConfig};
use fern_core::widget_id::WidgetId;
use fern_tokens::{CornerRadius, SurfaceRole};

use crate::primitives::{Expand, Padding, RectWidget, ZStack};

/// Default `StandardItemStyle` shipped with FernUI. Reads dimensions
/// from `theme.components.standard_item` (`item_corner_radius`,
/// `bg_horizontal_inset`, `padding_vertical`, `padding_horizontal`).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeStandardItemStyle;

impl StandardItemStyle for RecipeStandardItemStyle {
    fn make_body(&self, cfg: &StandardItemStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let style = ctx.theme().components.standard_item;

        // Background — `Selected` (when selected, regardless of hover)
        // > `Pressed` > `AccentSubtle` (hover) > Transparent. Disabled
        // is always Transparent (it shouldn't appear "pickable").
        let bg_role = bg_signal(
            &cfg.is_selected,
            &cfg.is_pressed,
            &cfg.is_hovered,
            &cfg.is_disabled,
        );

        // Selection rect, inset horizontally so the rounded corners
        // visually float inside the row's full-width hit area.
        let bg_rect = ctx.add(
            RectWidget::new()
                .bind_background(bg_role)
                .corner_radius(CornerRadius::uniform(style.item_corner_radius)),
        );
        let bg_padded = ctx.add(
            Padding::new(
                0.0,
                style.bg_horizontal_inset,
                0.0,
                style.bg_horizontal_inset,
            )
            .child_id(bg_rect),
        );

        // Content padding so slot widgets don't touch the bg edges.
        // `Expand::horizontal` makes the row claim the full row width
        // even when its slot widgets have small intrinsic sizes —
        // without this, ZStack would center the natural-width content
        // and leave (e.g.) tree chevrons shifted off the leading edge.
        let content_expanded = ctx.add(Expand::horizontal().child_id(cfg.content));
        let content_padded = ctx.add(
            Padding::symmetric(style.padding_vertical, style.padding_horizontal)
                .child_id(content_expanded),
        );

        ctx.add(
            ZStack::new()
                .add_child(bg_padded)
                .add_child(content_padded),
        )
    }
}

fn bg_signal(
    is_selected: &Signal<bool>,
    is_pressed: &Signal<bool>,
    is_hovered: &Signal<bool>,
    is_disabled: &Signal<bool>,
) -> Signal<SurfaceRole> {
    let combined = is_selected.zip3(is_pressed, is_hovered);
    combined
        .zip(is_disabled)
        .map(|((selected, pressed, hovered), disabled)| {
            if *disabled {
                SurfaceRole::Transparent
            } else if *selected {
                SurfaceRole::Selected
            } else if *pressed {
                SurfaceRole::Pressed
            } else if *hovered {
                SurfaceRole::AccentSubtle
            } else {
                SurfaceRole::Transparent
            }
        })
}
