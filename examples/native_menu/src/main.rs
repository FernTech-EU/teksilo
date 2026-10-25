// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The application menu
//!
//! Demonstrates the application menu: one declarative [`MenuModel`], declared
//! by the application tree (`TeksiloAppBuilder::app_root`) with
//! `ctx.set_application_menu(model)`, shown in a window with
//! [`MenuBar::application`]. On macOS it is the global menu bar at the top of
//! the screen (`NSMenu`), also while the key window has no menu of its own.
//! On other platforms it renders in the window that asks for it.
//!
//! Run with: `cargo run -p native-menu`
//!
//! Choosing an item updates the status line. File ▸ Save is greyed out until
//! "Toggle Save enabled" turns it on; Edit ▸ Paste is greyed out because
//! nothing takes `app.paste`. Window ▸ Inspector opens a second window with
//! a menu of its own ([`MenuBar::from_model`]), which replaces the
//! application menu while that window is key.

use teksilo::core::Action;
use teksilo::core::MenuItemId;
use teksilo::core::shortcut::{KeyStroke, Shortcut};
use teksilo::prelude::*;
use teksilo::widgets::{
    Button, Divider, Expand, HStack, MenuBar, MenuEntry, MenuModel, NativeMenuMode, Padding, Panel,
    StandardMenu, StatusBar, TextWidget, VStack,
};

/// The state the application tree and the windows share: signals are
/// cheap handles, so each side keeps a clone.
#[derive(Clone)]
struct Shared {
    /// The status line the commands write to, so activations are visible.
    status: Signal<String>,
    /// Reactive check state of View ▸ Show Grid.
    grid_visible: Signal<bool>,
    /// Reactive enabled state of File ▸ Save.
    can_save: Signal<bool>,
    /// The id of File ▸ Open Recent, so a window can push rows into it.
    recent_menu: MenuItemId,
    /// The application menu, kept so a window can change it at runtime.
    model: MenuModel,
}

impl Shared {
    fn new() -> Self {
        let status = Signal::new("Choose a menu item…".to_string());
        let grid_visible = Signal::new(true);
        let can_save = Signal::new(false);
        let recent_menu = MenuItemId::next();
        let model = MenuModel::new()
            // The macOS App menu. Its labels go through i18n (pass `tr!` for
            // a localized app). Quit asks the application to quit through
            // every window's close guard; `quit_intent` sends `app.quit`
            // first, and Quit still asks to quit if nothing takes it.
            // `settings_intent` puts a Settings… row on ⌘,, a placement and
            // chord an ordinary MenuEntry cannot reach.
            .standard_menu(
                StandardMenu::app()
                    .title(lit!("Native Menu"))
                    .quit_intent("app.quit")
                    .quit_shortcut("app.quit")
                    .settings(lit!("Settings…"))
                    .settings_intent("app.settings"),
            )
            .menu(lit!("&File"), |m| {
                m.item(
                    MenuEntry::new(lit!("&New"))
                        .intent("app.new")
                        .shortcut("app.new"),
                )
                .item(
                    MenuEntry::new(lit!("&Open"))
                        .intent("app.open")
                        .shortcut("app.open"),
                )
                // Greyed out until "Toggle Save enabled" turns it on.
                .item(
                    MenuEntry::new(lit!("&Save"))
                        .intent("app.save")
                        .enabled(can_save.clone()),
                )
                // Empty submenu populated at runtime (see the window's buttons).
                .submenu_with_id(recent_menu, lit!("Open &Recent"), |s| s)
                .separator()
                .item(
                    MenuEntry::new(lit!("&Quit"))
                        .intent("app.quit")
                        .shortcut("app.quit"),
                )
            })
            // Commands of the window: its root takes Cut and Copy. Nothing
            // takes Paste, so its row is greyed out.
            .menu(lit!("&Edit"), |m| {
                m.item(MenuEntry::new(lit!("Cu&t")).intent("app.cut"))
                    .item(MenuEntry::new(lit!("&Copy")).intent("app.copy"))
                    .item(MenuEntry::new(lit!("&Paste")).intent("app.paste"))
            })
            .menu(lit!("&View"), |m| {
                // Two-way: choosing the item flips `grid_visible` itself, from
                // the native bar and the in-window bar alike.
                m.item(MenuEntry::new(lit!("Show &Grid")).checkable(grid_visible.clone()))
            })
            .menu(lit!("&Window"), |m| {
                m.item(MenuEntry::new(lit!("&Inspector")).intent("app.inspector"))
            })
            // Minimize / Zoom and the window list, on macOS.
            .standard_menu(StandardMenu::window());
        Self {
            status,
            grid_visible,
            can_save,
            recent_menu,
            model,
        }
    }
}

/// The application tree's root: the application's commands, their
/// shortcuts, and the application menu.
struct AppRoot(Shared);

impl std::fmt::Debug for AppRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppRoot")
    }
}

impl Widget for AppRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let shared = &self.0;

        // Registered here, the shortcuts work in every window: each window
        // gets the ones the application menu's rows name. On macOS they are
        // the native menu's ⌘N / ⌘O / ⌘Q key equivalents too.
        for (id, label, key) in [
            ("app.new", "New", Key::N),
            ("app.open", "Open", Key::O),
            ("app.quit", "Quit", Key::Q),
        ] {
            ctx.register_shortcut_global(
                Shortcut::new(id)
                    .name(label)
                    .primary(KeyStroke::ctrl(key))
                    .build(),
            );
        }

        // The application's commands. A row's intent goes from the focused
        // window first; these take what the window leaves.
        for (id, label) in [
            ("app.new", "New"),
            ("app.open", "Open"),
            ("app.save", "Save"),
            ("app.settings", "Settings"),
        ] {
            let status = shared.status.clone();
            ctx.register_action_global(
                Action::new(id).on_invoke(move |_, _| status.set(format!("{label} chosen"))),
            );
        }
        ctx.register_action_global(Action::new("app.quit").on_invoke(|_, ctx| ctx.request_quit()));
        ctx.register_action_global(Action::new("app.inspector").on_invoke(|_, ctx| {
            match ctx.find_window("inspector") {
                Some(id) => ctx.focus_window(id),
                None => {
                    ctx.open_window(
                        WindowConfig::new()
                            .id("inspector")
                            .title("Teksilo: Inspector")
                            .size(360, 240)
                            .root(|tree, _state| tree.add(Inspector::default())),
                    );
                }
            }
        }));

        ctx.set_application_menu(shared.model.clone());
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

/// The main window: the application menu, a body, a status line.
#[derive(Debug)]
struct Root {
    shared: Shared,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Shared")
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let shared = self.shared.clone();

        // Commands of this window, taken before the application's.
        for (id, label) in [("app.cut", "Cut"), ("app.copy", "Copy")] {
            let status = shared.status.clone();
            ctx.register_action(
                Action::new(id).on_invoke(move |_, _| status.set(format!("{label} chosen"))),
            );
        }

        // The application menu. On macOS, where it is the global menu bar,
        // this renders nothing in the window.
        let menu_bar = ctx.add(MenuBar::application());

        // Runtime controls: toggle Save's enabled state, and add items to
        // File ▸ Open Recent, reflected in every window and the native menu.
        let recent_count = ctx.signal(0_usize);
        let controls = ctx.add(
            HStack::new()
                .spacing(8.0)
                .child(Button::new(lit!("Toggle Save enabled")).on_activate_fn({
                    let can_save = shared.can_save.clone();
                    move |_| can_save.set(!can_save.get())
                }))
                .child(Button::new(lit!("Add recent file")).on_activate_fn({
                    let shared = shared.clone();
                    let recent_count = recent_count.clone();
                    move |_| {
                        let n = recent_count.get() + 1;
                        recent_count.set(n);
                        let name = format!("document-{n}.txt");
                        let status = shared.status.clone();
                        // The closure runs in the application tree.
                        shared.model.push_item(
                            shared.recent_menu,
                            MenuEntry::new(LocalizedString::literal(name.clone()))
                                .on_activate(move |_| status.set(format!("Opened {name}"))),
                        );
                    }
                })),
        );

        let grid_label = shared.grid_visible.map(|v| {
            if *v {
                "Grid: visible (toggle via View ▸ Show Grid)".to_string()
            } else {
                "Grid: hidden (toggle via View ▸ Show Grid)".to_string()
            }
        });

        let body = ctx.add(
            Panel::new().background(SurfaceRole::Main).corner_radius(8.0).child(
                Padding::uniform(20.0).child(
                    VStack::new()
                        .spacing(12.0)
                        .child(
                            TextWidget::new(lit!("The application menu"))
                                .style(TextStyleRole::BodyBold)
                                .color(TextRole::Primary),
                        )
                        .child(
                            TextWidget::new(lit!(
                                "The application tree declares one menu for the whole \
                                 application. On macOS it is the system menu bar at the top of \
                                 the screen, also with no window open; elsewhere it renders \
                                 above. Edit ▸ Paste is greyed out: nothing takes its command. \
                                 Window ▸ Inspector opens a window with a menu of its own."
                            ))
                            .style(TextStyleRole::Body)
                            .color(TextRole::Primary),
                        )
                        .child(Divider::new().thickness(1.0))
                        .child(
                            TextWidget::new(lit!(""))
                                .text(grid_label)
                                .style(TextStyleRole::Body)
                                .color(TextRole::Secondary),
                        )
                        .child(
                            TextWidget::new(lit!(
                                "Runtime control: these change the menu live (enable Save, or add \
                                 items to File ▸ Open Recent):"
                            ))
                            .style(TextStyleRole::Small)
                            .color(TextRole::Secondary),
                        )
                        .child(controls),
                ),
            ),
        );

        let root = ctx.add(
            VStack::new()
                .child(menu_bar)
                .child(Expand::new().child(Padding::uniform(24.0).child(body)))
                .child(
                    StatusBar::new().child(
                        TextWidget::new(lit!(""))
                            .text(shared.status.clone())
                            .style(TextStyleRole::Tiny)
                            .color(TextRole::Primary),
                    ),
                ),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
        .into()
    }
}

/// A window with a menu of its own: while it is key, its menu replaces the
/// application menu in the macOS menu bar.
#[derive(Debug, Default)]
struct Inspector {
    root_child_id: Option<WidgetId>,
}

impl Widget for Inspector {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let refreshed = ctx.signal(0_u32);
        {
            let refreshed = refreshed.clone();
            ctx.register_action(
                Action::new("inspector.refresh")
                    .on_invoke(move |_, _| refreshed.set(refreshed.get() + 1)),
            );
        }
        ctx.register_action(Action::new("inspector.close").on_invoke(|_, ctx| ctx.close_window()));
        let model = MenuModel::new().menu(lit!("&Inspector"), |m| {
            m.item(MenuEntry::new(lit!("&Refresh")).intent("inspector.refresh"))
                .separator()
                .item(MenuEntry::new(lit!("&Close")).intent("inspector.close"))
        });
        let label = refreshed.map(|n| format!("Refreshed {n} times"));
        let root = ctx.add(
            VStack::new()
                .child(MenuBar::from_model(model).native_on_macos(NativeMenuMode::Suppress))
                .child(
                    Padding::uniform(20.0).child(
                        TextWidget::new(lit!(""))
                            .text(label)
                            .style(TextStyleRole::Body)
                            .color(TextRole::Primary),
                    ),
                ),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
        .into()
    }
}

fn main() {
    let shared = Shared::new();
    let for_window = shared.clone();
    TeksiloAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_native_menu()
        .install_inspector_in_debug()
        .theme(teksilo::presets::intui::light())
        .app_root(move |tree| tree.add(AppRoot(shared)))
        .initial_window(
            WindowConfig::new()
                .title("Teksilo: Native Menu")
                .size(720, 480)
                .root(move |tree, _state| {
                    tree.add(Root {
                        shared: for_window.clone(),
                        root_child_id: None,
                    })
                }),
        )
        .run();
}
