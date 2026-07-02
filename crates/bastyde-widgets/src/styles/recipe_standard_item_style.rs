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

/// Configurable dimensions for [`RecipeStandardItemStyle`].
///
/// The [`Default`] implementation reads the module-level `pub const` tokens,
/// so a `RecipeStandardItemStyle::default()` is identical to the old unit struct.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StandardItemRecipe {
    pub icon_size: f32,
    pub subtitle_icon_size: f32,
    pub slot_gap: f32,
    pub subtitle_slot_gap: f32,
    pub label_subtitle_gap: f32,
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub min_height_single_line: f32,
    pub min_height_two_line: f32,
    pub chevron_column_width: f32,
    pub tree_indent_step: f32,
    pub item_corner_radius: f32,
    pub bg_horizontal_inset: f32,
    pub focus_ring_width: f32,
}

impl Default for StandardItemRecipe {
    fn default() -> Self {
        Self {
            icon_size: STANDARD_ITEM_ICON_SIZE,
            subtitle_icon_size: STANDARD_ITEM_SUBTITLE_ICON_SIZE,
            slot_gap: STANDARD_ITEM_SLOT_GAP,
            subtitle_slot_gap: STANDARD_ITEM_SUBTITLE_SLOT_GAP,
            label_subtitle_gap: STANDARD_ITEM_LABEL_SUBTITLE_GAP,
            padding_horizontal: STANDARD_ITEM_PADDING_HORIZONTAL,
            padding_vertical: STANDARD_ITEM_PADDING_VERTICAL,
            min_height_single_line: STANDARD_ITEM_MIN_HEIGHT_SINGLE_LINE,
            min_height_two_line: STANDARD_ITEM_MIN_HEIGHT_TWO_LINE,
            chevron_column_width: STANDARD_ITEM_CHEVRON_COLUMN_WIDTH,
            tree_indent_step: STANDARD_ITEM_TREE_INDENT_STEP,
            item_corner_radius: STANDARD_ITEM_ITEM_CORNER_RADIUS,
            bg_horizontal_inset: STANDARD_ITEM_BG_HORIZONTAL_INSET,
            focus_ring_width: STANDARD_ITEM_FOCUS_RING_WIDTH,
        }
    }
}

/// Default `StandardItemStyle` shipped with Bastyde.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeStandardItemStyle {
    pub recipe: StandardItemRecipe,
}

impl RecipeStandardItemStyle {
    pub fn new(recipe: StandardItemRecipe) -> Self {
        Self { recipe }
    }
}

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
            &cfg.is_window_active,
        );

        // Keyboard-focus ring: drawn on the *current* item (selected, in a
        // single-selection view) while that view holds keyboard focus AND the
        // last input was keyboard (`:focus-visible`) — so it appears during Tab
        // / arrow navigation but NOT on a mouse click, and clears on any pointer
        // input. Width collapses to 0 otherwise. `BorderRole::Focused` is the
        // theme's focus-ring color.
        let focus_ring_width = self.recipe.focus_ring_width;
        let ring_width = cfg
            .is_selected
            .zip3(&cfg.is_focused, &cfg.is_focus_visible)
            .map(move |(sel, foc, vis)| {
                if *sel && *foc && *vis {
                    focus_ring_width
                } else {
                    0.0
                }
            });

        // Selection rect, inset horizontally so the rounded corners
        // visually float inside the row's full-width hit area.
        let bg_rect = ctx.add(
            RectWidget::new()
                .background(bg_role)
                .corner_radius(CornerRadius::uniform(self.recipe.item_corner_radius))
                .border_color(BorderRole::Focused)
                .border_width(ring_width),
        );
        let bg_padded = ctx.add(
            Padding::new(
                0.0,
                self.recipe.bg_horizontal_inset,
                0.0,
                self.recipe.bg_horizontal_inset,
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
            Padding::symmetric(self.recipe.padding_vertical, self.recipe.padding_horizontal)
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
    is_window_active: &Signal<bool>,
) -> Signal<SurfaceRole> {
    // Effective focus = the view holds keyboard focus AND the host window is
    // active. A selected row in an inactive window desaturates exactly as it
    // does when focus moves elsewhere in the same window.
    let effective_focus = is_focused.and(is_window_active);
    let combined = is_selected.zip3(is_pressed, is_hovered);
    combined.zip3(is_disabled, &effective_focus).map(
        |((selected, pressed, hovered), disabled, focused)| {
            if *disabled {
                SurfaceRole::Transparent
            } else if *selected {
                // Vivid selection while focused AND window-active; muted
                // "inactive" selection otherwise.
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

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_tokens::SurfaceRole;

    #[test]
    fn selection_role_requires_focus_and_window_active() {
        let selected = Signal::new(true);
        let pressed = Signal::new(false);
        let hovered = Signal::new(false);
        let disabled = Signal::new(false);
        let focused = Signal::new(true);
        let window_active = Signal::new(true);

        let role = bg_signal(
            &selected,
            &pressed,
            &hovered,
            &disabled,
            &focused,
            &window_active,
        );

        // Selected, view-focused, window-active → vivid.
        assert_eq!(role.get(), SurfaceRole::Selected);

        // Window goes inactive (view focus retained) → muted.
        window_active.set(false);
        assert_eq!(role.get(), SurfaceRole::SelectedInactive);

        // Window active again but view focus lost → still muted.
        window_active.set(true);
        focused.set(false);
        assert_eq!(role.get(), SurfaceRole::SelectedInactive);

        // Both satisfied again → vivid.
        focused.set(true);
        assert_eq!(role.get(), SurfaceRole::Selected);
    }
}
