//! Declarative menu model — a single source of truth for both the in-window
//! [`MenuBar`](crate::menu_bar::MenuBar) and the platform's native menu bar
//! (`NSMenu` on macOS).
//!
//! A [`MenuModel`] describes the whole menu tree as data — titles, intents,
//! shortcut ids, enabled state, check / radio bindings, submenus, separators —
//! with no built widgets. [`MenuBar::from_model`](crate::menu_bar::MenuBar::from_model)
//! renders it as the usual in-window dropdown bar, and the same model is mirrored
//! into the OS menu bar via the `native_on_macos` flag. Reactive `Signal`s drive
//! both: a toggled check mark flips in the in-window menu and the native menu
//! alike.
//!
//! ```ignore
//! use bastyde::widgets::menu::{MenuModel, MenuEntry};
//! let model = MenuModel::new()
//!     .menu(tr!(file()), |m| m
//!         .item(MenuEntry::new(tr!(new())).intent("app.new").shortcut("app.new"))
//!         .separator()
//!         .item(MenuEntry::new(tr!(quit())).intent("app.quit")))
//!     .menu(tr!(view()), |m| m
//!         .item(MenuEntry::new(tr!(show_grid())).tri_checkable(grid_state)));
//! ```

pub mod model;
pub mod native;

pub use model::{MenuEntry, MenuItemState, MenuItems, MenuModel, MenuNode};
pub use native::NativeMenuMode;

// Re-export the platform standard-menu roles so apps name them through the
// widget surface without a direct `bastyde-platform` dependency.
pub use bastyde_platform::native_menu::StandardMenuRole;
