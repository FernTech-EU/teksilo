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
pub mod recipe_checkbox_style;
pub mod recipe_icon_button_style;
pub mod recipe_panel_style;
pub mod recipe_radio_style;
pub mod recipe_toggle_style;

pub use recipe_button_style::RecipeButtonStyle;
pub use recipe_checkbox_style::RecipeCheckboxStyle;
pub use recipe_icon_button_style::RecipeIconButtonStyle;
pub use recipe_panel_style::RecipePanelStyle;
pub use recipe_radio_style::RecipeRadioStyle;
pub use recipe_toggle_style::RecipeToggleStyle;
