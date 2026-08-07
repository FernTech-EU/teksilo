// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Fallback window menu for platforms with no OS-provided one.
//!
//! Right-clicking a title bar conventionally opens the *system* window menu
//! (Minimize / Maximize / Restore / Close). On Wayland the compositor provides
//! it via `xdg_toplevel.show_window_menu`, and on Windows the OS answers
//! `WM_SYSCOMMAND`/`SC_KEYMENU` — but **X11 has no such request**: winit's
//! `show_window_menu` is an empty stub there, and `_GTK_SHOW_WINDOW_MENU` (the
//! only cross-desktop attempt at one) is not implemented by KWin
//! (KDE bug 454756). Without a fallback, right-clicking a custom title bar on
//! X11 would simply do nothing.
//!
//! So the widget layer builds its own. It drives the same
//! [`WindowState::placement`](teksilo_core::window::WindowState::placement) and
//! `close` signals the title bar's own buttons use, so it needs no platform
//! surface at all and behaves identically everywhere it is used.
//!
//! [`TitleBar`](crate::TitleBar) installs this automatically whenever
//! [`PlatformTitleBarHost::has_window_menu`] reports `false`; nothing in an
//! application has to opt in.
//!
//! [`PlatformTitleBarHost::has_window_menu`]: teksilo_core::PlatformTitleBarHost::has_window_menu

use std::rc::Rc;

use teksilo_core::WindowPlacement;
use teksilo_core::widget::{EventContext, Widget};
use teksilo_i18n::tr;

use crate::{MenuItem, MenuList};

/// Build the fallback window menu.
///
/// Returns `None` when the widget has no hosting window (standalone widget
/// trees and tests), because every entry would be inert — and the framework's
/// context-menu contract reads `None` as "decline, let an ancestor answer",
/// which is the right behaviour rather than showing a dead menu.
///
/// `close_action` mirrors [`TitleBar::close_action`](crate::TitleBar::close_action):
/// when the application overrode the close button, the menu's Close entry must
/// do the same thing, or the two would disagree about what closing means (a
/// confirmation prompt, for instance).
pub(crate) fn build_window_menu(
    ctx: &mut EventContext,
    close_action: Option<Rc<dyn Fn(&mut EventContext)>>,
) -> Option<Box<dyn Widget>> {
    let window = ctx.window()?;
    let placement = window.placement();
    let is_maximized = placement.map(|p| p.is_maximized());

    let restore_placement = placement.clone();
    let maximize_placement = placement.clone();
    let minimize_placement = placement.clone();

    // Restore and Maximize are the same affordance in two states, so exactly
    // one is visible at a time — `item_when` collapses the hidden row to zero
    // height and skips it in keyboard navigation, so the menu reads correctly
    // to a screen reader too.
    let menu = MenuList::new()
        .item_when(
            MenuItem::new(tr!(window_menu_restore())).on_activate_fn(move |_ctx| {
                restore_placement.set(WindowPlacement::Floating);
            }),
            is_maximized.clone(),
        )
        .item_when(
            MenuItem::new(tr!(window_menu_maximize())).on_activate_fn(move |_ctx| {
                maximize_placement.set(WindowPlacement::Maximized);
            }),
            is_maximized.not(),
        )
        .item(
            MenuItem::new(tr!(window_menu_minimize())).on_activate_fn(move |_ctx| {
                minimize_placement.set(WindowPlacement::Minimized);
            }),
        )
        .separator()
        .item(MenuItem::new(tr!(window_menu_close())).on_activate_fn(
            move |ctx| match &close_action {
                Some(action) => action(ctx),
                None => ctx.close_window(),
            },
        ));

    Some(Box::new(menu))
}
