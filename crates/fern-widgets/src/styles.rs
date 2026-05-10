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
pub mod recipe_toggle_style;

pub use recipe_button_style::RecipeButtonStyle;
pub use recipe_toggle_style::RecipeToggleStyle;
