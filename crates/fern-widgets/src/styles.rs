//! Default `Recipe*Style` impls — the IntUI look ships here. Apps that
//! want a different design language write their own `impl FooStyle`
//! blocks (per-call via `Foo::style(...)` or theme-wide via the
//! `ComponentStyles` slot bag in step 8).
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
pub mod recipe_icon_button_style;
pub mod recipe_link_style;
pub mod recipe_menu_item_style;
pub mod recipe_panel_style;
pub mod recipe_popover_style;
pub mod recipe_progress_bar_style;
pub mod recipe_radio_style;
pub mod recipe_rich_text_editor_style;
pub mod recipe_scroll_bar_style;
pub mod recipe_search_field_style;
pub mod recipe_segmented_control_style;
pub mod recipe_slider_style;
pub mod recipe_snackbar_style;
pub mod recipe_spin_box_style;
pub mod recipe_standard_item_style;
pub mod recipe_tab_style;
pub mod recipe_table_style;
pub mod recipe_text_input_style;
pub mod recipe_toggle_style;
pub mod recipe_tooltip_style;

pub use recipe_avatar_style::RecipeAvatarStyle;
pub use recipe_badge_style::RecipeBadgeStyle;
pub use recipe_banner_style::RecipeBannerStyle;
pub use recipe_button_style::RecipeButtonStyle;
pub use recipe_calendar_style::RecipeCalendarStyle;
pub use recipe_card_style::RecipeCardStyle;
pub use recipe_checkbox_style::RecipeCheckboxStyle;
pub use recipe_color_picker_style::RecipeColorPickerStyle;
pub use recipe_combo_box_style::RecipeComboBoxStyle;
pub use recipe_date_edit_style::RecipeDateEditStyle;
pub use recipe_dialog_style::RecipeDialogStyle;
pub use recipe_icon_button_style::RecipeIconButtonStyle;
pub use recipe_link_style::RecipeLinkStyle;
pub use recipe_menu_item_style::RecipeMenuItemStyle;
pub use recipe_panel_style::RecipePanelStyle;
pub use recipe_popover_style::RecipePopoverStyle;
pub use recipe_progress_bar_style::RecipeProgressBarStyle;
pub use recipe_radio_style::RecipeRadioStyle;
pub use recipe_rich_text_editor_style::RecipeRichTextEditorStyle;
pub use recipe_scroll_bar_style::RecipeScrollBarStyle;
pub use recipe_search_field_style::RecipeSearchFieldStyle;
pub use recipe_segmented_control_style::RecipeSegmentedControlStyle;
pub use recipe_slider_style::RecipeSliderStyle;
pub use recipe_snackbar_style::RecipeSnackbarStyle;
pub use recipe_spin_box_style::RecipeSpinBoxStyle;
pub use recipe_standard_item_style::RecipeStandardItemStyle;
pub use recipe_tab_style::RecipeTabStyle;
pub use recipe_table_style::RecipeTableStyle;
pub use recipe_text_input_style::RecipeTextInputStyle;
pub use recipe_toggle_style::RecipeToggleStyle;
pub use recipe_tooltip_style::RecipeTooltipStyle;
