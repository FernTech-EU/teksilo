// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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

use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{StandardItemStyle, StandardItemStyleConfig};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{BorderRole, CornerRadius, SurfaceRole};

use crate::primitives::{Expand, Padding, RectWidget, ZStack};

// IntUI design tokens for StandardListItem / StandardTreeItem.
// The recipe owns its own dimensions.
pub const STANDARD_ITEM_ICON_SIZE: f32 = 16.0;
pub const STANDARD_ITEM_SUBTITLE_ICON_SIZE: f32 = 12.0;
pub const STANDARD_ITEM_SLOT_GAP: f32 = 8.0;
pub const STANDARD_ITEM_SUBTITLE_SLOT_GAP: f32 = 6.0;
pub const STANDARD_ITEM_LABEL_SUBTITLE_GAP: f32 = 2.0;
pub const STANDARD_ITEM_PADDING_HORIZONTAL: f32 = 8.0;
pub const STANDARD_ITEM_PADDING_VERTICAL: f32 = 4.0;
pub const STANDARD_ITEM_MIN_HEIGHT_SINGLE_LINE: f32 = 28.0;
pub const STANDARD_ITEM_MIN_HEIGHT_TWO_LINE: f32 = 44.0;
pub const STANDARD_ITEM_CHEVRON_COLUMN_WIDTH: f32 = 16.0;
pub const STANDARD_ITEM_TREE_INDENT_STEP: f32 = 16.0;
pub const STANDARD_ITEM_ITEM_CORNER_RADIUS: f32 = 8.0;
pub const STANDARD_ITEM_BG_HORIZONTAL_INSET: f32 = 4.0;
/// Keyboard-focus ring thickness for the current item while its view is focused.
pub const STANDARD_ITEM_FOCUS_RING_WIDTH: f32 = 1.5;

/// Default `StandardItemStyle` shipped with Bastyde.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeStandardItemStyle;

impl StandardItemStyle for RecipeStandardItemStyle {
    fn make_body(&self, cfg: &StandardItemStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Background — `Selected` (when selected, regardless of hover)
        // > `Pressed` > `AccentSubtle` (hover) > Transparent. Disabled
        // is always Transparent (it shouldn't appear "pickable").
        let bg_role = bg_signal(
            &cfg.is_selected,
            &cfg.is_pressed,
            &cfg.is_hovered,
            &cfg.is_disabled,
            &cfg.is_focused,
        );

        // Keyboard-focus ring: drawn on the *current* item (selected, in a
        // single-selection view) while that view holds keyboard focus AND the
        // last input was keyboard (`:focus-visible`) — so it appears during Tab
        // / arrow navigation but NOT on a mouse click, and clears on any pointer
        // input. Width collapses to 0 otherwise. `BorderRole::Focused` is the
        // theme's focus-ring color.
        let ring_width = cfg
            .is_selected
            .zip3(&cfg.is_focused, &cfg.is_focus_visible)
            .map(|(sel, foc, vis)| {
                if *sel && *foc && *vis {
                    STANDARD_ITEM_FOCUS_RING_WIDTH
                } else {
                    0.0
                }
            });

        // Selection rect, inset horizontally so the rounded corners
        // visually float inside the row's full-width hit area.
        let bg_rect = ctx.add(
            RectWidget::new()
                .bind_background(bg_role)
                .corner_radius(CornerRadius::uniform(STANDARD_ITEM_ITEM_CORNER_RADIUS))
                .border_color(BorderRole::Focused)
                .border_width(ring_width),
        );
        let bg_padded = ctx.add(
            Padding::new(
                0.0,
                STANDARD_ITEM_BG_HORIZONTAL_INSET,
                0.0,
                STANDARD_ITEM_BG_HORIZONTAL_INSET,
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
            Padding::symmetric(
                STANDARD_ITEM_PADDING_VERTICAL,
                STANDARD_ITEM_PADDING_HORIZONTAL,
            )
            .child_id(content_expanded),
        );

        ctx.add(ZStack::new().add_child(bg_padded).add_child(content_padded))
    }
}

fn bg_signal(
    is_selected: &Signal<bool>,
    is_pressed: &Signal<bool>,
    is_hovered: &Signal<bool>,
    is_disabled: &Signal<bool>,
    is_focused: &Signal<bool>,
) -> Signal<SurfaceRole> {
    let combined = is_selected.zip3(is_pressed, is_hovered);
    combined.zip3(is_disabled, is_focused).map(
        |((selected, pressed, hovered), disabled, focused)| {
            if *disabled {
                SurfaceRole::Transparent
            } else if *selected {
                // Focus-aware: active selection while the view holds keyboard
                // focus, muted "inactive" selection when focus is elsewhere.
                if *focused {
                    SurfaceRole::Selected
                } else {
                    SurfaceRole::SelectedInactive
                }
            } else if *pressed {
                SurfaceRole::Pressed
            } else if *hovered {
                SurfaceRole::AccentSubtle
            } else {
                SurfaceRole::Transparent
            }
        },
    )
}
