// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Declarative menu model: a single source of truth for both the in-window
//! [`MenuBar`](crate::menu_bar::MenuBar) and the platform's native menu bar
//! (`NSMenu` on macOS).
//!
//! A [`MenuModel`] describes the whole menu tree as data (titles, intents,
//! shortcut ids, enabled state, check / radio bindings, submenus, separators)
//! with no built widgets. Reactive `Signal`s drive every surface it is shown
//! on: a toggled check mark flips in the in-window menu and the native menu
//! alike.
//!
//! An application has **one application menu**, declared by the application
//! tree with [`set_application_menu`](ApplicationMenuExt::set_application_menu)
//! (see [`application`]). A window shows it with
//! [`MenuBar::application`](crate::menu_bar::MenuBar::application), or shows a
//! menu of its own instead with
//! [`MenuBar::from_model`](crate::menu_bar::MenuBar::from_model). On macOS the
//! application menu is the global menu bar whenever the key window has no menu
//! of its own, and while no window is open; on Windows and Linux it appears
//! only in the windows whose bar asks for it.
//!
//! A row's intent is sent from the focused widget of the window the user
//! works in, then reaches the application tree when that window leaves it; a
//! row with an intent, no closure and no state of its own to write (a
//! two-way check or radio row writes one) greys out while nothing on that
//! route takes it.
//!
//! ```ignore
//! use teksilo::prelude::*;
//! use teksilo::widgets::menu::{MenuModel, MenuEntry, StandardMenu};
//!
//! // In the application tree's root:
//! ctx.set_application_menu(
//!     MenuModel::new()
//!         .standard_menu(StandardMenu::app().title(tr!(app_name())).quit_intent("app.quit"))
//!         .menu(tr!(file()), |m| m
//!             .item(MenuEntry::new(tr!(new())).intent("app.new").shortcut("app.new"))
//!             .separator()
//!             .item(MenuEntry::new(tr!(quit())).intent("app.quit")))
//!         .menu(tr!(view()), |m| m
//!             .item(MenuEntry::new(tr!(show_grid())).tri_checkable(grid_state))),
//! );
//!
//! // In a window:
//! ctx.add(MenuBar::application());
//! ```

pub mod application;
pub mod model;
pub mod native;

pub use application::ApplicationMenuExt;
#[doc(hidden)]
pub use application::{ApplicationMenu, ApplicationMenuActivation};
pub use model::{MenuEntry, MenuItemState, MenuItems, MenuModel, MenuNode, StandardMenu};
pub use native::NativeMenuMode;

// Re-export the platform standard-menu roles so apps name them through the
// widget surface without a direct `teksilo-platform` dependency.
pub use teksilo_platform::native_menu::StandardMenuRole;
