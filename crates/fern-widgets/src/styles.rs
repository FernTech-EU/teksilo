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

pub mod recipe_button_style;
pub mod recipe_card_style;
pub mod recipe_checkbox_style;
pub mod recipe_combo_box_style;
pub mod recipe_icon_button_style;
pub mod recipe_menu_item_style;
pub mod recipe_panel_style;
pub mod recipe_popover_style;
pub mod recipe_radio_style;
pub mod recipe_scroll_bar_style;
pub mod recipe_slider_style;
pub mod recipe_standard_item_style;
pub mod recipe_text_input_style;
pub mod recipe_toggle_style;
pub mod recipe_tooltip_style;

pub use recipe_button_style::RecipeButtonStyle;
pub use recipe_card_style::RecipeCardStyle;
pub use recipe_checkbox_style::RecipeCheckboxStyle;
pub use recipe_combo_box_style::RecipeComboBoxStyle;
pub use recipe_icon_button_style::RecipeIconButtonStyle;
pub use recipe_menu_item_style::RecipeMenuItemStyle;
pub use recipe_panel_style::RecipePanelStyle;
pub use recipe_popover_style::RecipePopoverStyle;
pub use recipe_radio_style::RecipeRadioStyle;
pub use recipe_scroll_bar_style::RecipeScrollBarStyle;
pub use recipe_slider_style::RecipeSliderStyle;
pub use recipe_standard_item_style::RecipeStandardItemStyle;
pub use recipe_text_input_style::RecipeTextInputStyle;
pub use recipe_toggle_style::RecipeToggleStyle;
pub use recipe_tooltip_style::RecipeTooltipStyle;
