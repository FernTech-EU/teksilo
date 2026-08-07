// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `Recipe*Style` impls — the IntUI look ships here. Apps that
//! want a different design language write their own `impl FooStyle`
//! blocks (per-call via `Foo::style(...)` or theme-wide via
//! `theme.style_slots`).
//!
//! Each impl widget composes a small subtree (or for chrome-dense
//! widgets like Toggle, a tiny private leaf widget that paints
//! directly) so the public `Foo` widget itself stays pure
//! composition over the `FooStyle` trait — no `paint()` method on
//! any themable widget.

pub mod recipe_avatar_style;
pub mod recipe_badge_style;
pub mod recipe_banner_style;
pub mod recipe_button_style;
pub mod recipe_calendar_style;
pub mod recipe_card_style;
pub mod recipe_checkbox_style;
pub mod recipe_color_picker_style;
pub mod recipe_combo_box_style;
pub mod recipe_date_edit_style;
pub mod recipe_dialog_style;
pub mod recipe_drop_target_style;
pub mod recipe_drop_zone_style;
pub mod recipe_grid_view_style;
pub mod recipe_icon_button_style;
pub mod recipe_link_style;
pub mod recipe_list_container_style;
pub mod recipe_menu_item_style;
pub mod recipe_panel_style;
pub mod recipe_popover_style;
pub mod recipe_progress_bar_style;
pub mod recipe_radio_style;
pub mod recipe_radio_tile_style;
pub mod recipe_rich_text_editor_style;
pub mod recipe_scroll_bar_style;
pub mod recipe_search_field_style;
pub mod recipe_segmented_control_style;
pub mod recipe_slider_style;
pub mod recipe_snackbar_style;
pub mod recipe_spin_box_style;
pub mod recipe_split_button_style;
pub mod recipe_splitter_style;
pub mod recipe_standard_item_style;
pub mod recipe_tab_style;
pub mod recipe_table_style;
pub mod recipe_text_input_style;
pub mod recipe_toast_style;
pub mod recipe_toggle_style;
pub mod recipe_tooltip_style;

pub use recipe_avatar_style::{AvatarRecipe, RecipeAvatarStyle};
pub use recipe_badge_style::{BadgeRecipe, RecipeBadgeStyle};
pub use recipe_banner_style::{BannerRecipe, RecipeBannerStyle};
pub use recipe_button_style::RecipeButtonStyle;
pub use recipe_calendar_style::{CalendarRecipe, RecipeCalendarStyle};
pub use recipe_card_style::{CardRecipe, RecipeCardStyle};
pub use recipe_checkbox_style::{CheckboxRecipe, RecipeCheckboxStyle};
pub use recipe_color_picker_style::{ColorPickerRecipe, RecipeColorPickerStyle};
pub use recipe_combo_box_style::{ComboBoxRecipe, RecipeComboBoxStyle};
pub use recipe_date_edit_style::{DateEditRecipe, RecipeDateEditStyle};
pub use recipe_dialog_style::{DialogRecipe, RecipeDialogStyle};
pub use recipe_drop_target_style::{DropTargetRecipe, RecipeDropTargetStyle};
pub use recipe_drop_zone_style::{DropZoneRecipe, RecipeDropZoneStyle};
pub use recipe_grid_view_style::RecipeGridViewStyle;
pub use recipe_icon_button_style::{IconButtonRecipe, RecipeIconButtonStyle};
pub use recipe_link_style::{LinkRecipe, RecipeLinkStyle};
pub use recipe_list_container_style::RecipeListContainerStyle;
pub use recipe_menu_item_style::{MenuItemRecipe, RecipeMenuItemStyle};
pub use recipe_panel_style::{PanelRecipe, RecipePanelStyle};
pub use recipe_popover_style::{PopoverRecipe, RecipePopoverStyle};
pub use recipe_progress_bar_style::{ProgressBarRecipe, RecipeProgressBarStyle};
pub use recipe_radio_style::{RadioRecipe, RecipeRadioStyle};
pub use recipe_radio_tile_style::{
    RADIO_TILE_CORNER_RADIUS, RADIO_TILE_VERTICAL_ROW_HEIGHT, RadioTileRecipe, RecipeRadioTileStyle,
};
pub use recipe_rich_text_editor_style::RecipeRichTextEditorStyle;
pub use recipe_scroll_bar_style::{RecipeScrollBarStyle, ScrollBarRecipe};
pub use recipe_search_field_style::{RecipeSearchFieldStyle, SearchFieldRecipe};
pub use recipe_segmented_control_style::{RecipeSegmentedControlStyle, SegmentedControlRecipe};
pub use recipe_slider_style::{RecipeSliderStyle, SliderRecipe};
pub use recipe_snackbar_style::{RecipeSnackbarStyle, SnackbarRecipe};
pub use recipe_spin_box_style::RecipeSpinBoxStyle;
pub use recipe_split_button_style::RecipeSplitButtonStyle;
pub use recipe_splitter_style::{RecipeSplitterStyle, SplitterRecipe};
pub use recipe_standard_item_style::{RecipeStandardItemStyle, StandardItemRecipe};
pub use recipe_tab_style::{RecipeTabStyle, TabRecipe};
pub use recipe_table_style::{RecipeTableStyle, TableRecipe};
pub use recipe_text_input_style::{RecipeTextInputStyle, TextInputRecipe};
pub use recipe_toast_style::{RecipeToastStyle, ToastRecipe};
pub use recipe_toggle_style::{RecipeToggleStyle, ToggleRecipe};
pub use recipe_tooltip_style::{RecipeTooltipStyle, TooltipRecipe};

use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_tokens::ColorTokens;

/// Reactive resolution palette for recipe colours: the live theme palette while
/// the host window is active, the accent-desaturated projection
/// ([`ColorTokens::for_inactive_window`]) while it is inactive.
///
/// A recipe style that **bakes** a colour into a `ColorProp::Bound(Signal)` /
/// `PaintProp` at build time (resolving a `RecipeColor` against the theme) must
/// resolve against this signal rather than a plain `ctx.theme_signal()`. A
/// `Bound` colour is a concrete value that `ColorProp::resolve` returns verbatim
/// — it ignores the paint walker's window-inactive theme projection — so accent
/// chrome baked from `theme_signal` (a Filled button fill, an accent focus
/// border) would stay vivid in a background window while paint-resolving
/// controls (Toggle, Checkbox, which read `colors.accent` in `paint()`) greyed
/// out. Resolving against this greys them out uniformly.
///
/// Role-preserving paint paths — passing a `SurfaceRole` / `BorderRole` /
/// `Signal<Role>` straight to a widget so it resolves against `ctx.theme` at
/// paint — already follow the projection and need no change.
pub(crate) fn window_resolution_colors(ctx: &BuildContext) -> Signal<ColorTokens> {
    let wa = ctx.window_active_signal();
    ctx.theme_signal().zip(&wa).map(|(theme, active)| {
        if *active {
            theme.colors.clone()
        } else {
            theme.colors.for_inactive_window()
        }
    })
}
