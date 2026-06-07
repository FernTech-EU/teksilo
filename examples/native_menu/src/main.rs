//! Native (OS) menu bar
//!
//! Demonstrates [`MenuBar::from_model`] + [`MenuBar::native_on_macos`]: one
//! declarative [`MenuModel`] drives both the in-window menu bar and — on macOS —
//! the global menu bar at the top of the screen (`NSMenu`).
//!
//! Run with: `cargo run -p native-menu`
//!
//! On macOS the menus appear in the system menu bar (File / Edit / View, after
//! the standard application menu). Choosing an item updates the status line
//! below; toggling "Show Grid" flips the native check mark live. On other
//! platforms the same model renders as the usual in-window bar (the native
//! backend is a no-op there).

use bastyde::core::Action;
use bastyde::core::MenuItemId;
use bastyde::core::shortcut::{KeyStroke, Shortcut};
use bastyde::prelude::*;
use bastyde::widgets::{
    Button, Divider, Expand, HStack, MenuBar, MenuEntry, MenuModel, NativeMenuMode, Padding, Panel,
    StatusBar, TextWidget, VStack,
};

#[derive(Debug)]
struct Root {
    root_child_id: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        Self {
            root_child_id: None,
        }
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // A status line the actions write to, so menu activations are visible.
        let status = ctx.signal("Choose a menu item…".to_string());
        // Reactive check state shared by the menu item and the body label.
        let grid_visible = ctx.signal(true);
        // Reactive enabled state for File ▸ Save (greys out live).
        let can_save = ctx.signal(false);
        // Pre-allocated id for the "Open Recent" submenu, so handlers can push
        // items into it at runtime.
        let recent_menu = MenuItemId::next();

        // Shortcuts → become ⌘N / ⌘O / ⌘Q key equivalents on the native menu.
        ctx.register_shortcut_global(
            Shortcut::new("app.new").name("New").primary(KeyStroke::ctrl(Key::N)).build(),
        );
        ctx.register_shortcut_global(
            Shortcut::new("app.open").name("Open").primary(KeyStroke::ctrl(Key::O)).build(),
        );
        ctx.register_shortcut_global(
            Shortcut::new("app.quit").name("Quit").primary(KeyStroke::ctrl(Key::Q)).build(),
        );

        // Actions invoked whether the item is chosen from the native menu, the
        // in-window menu, or its keyboard shortcut.
        for (id, label) in [
            ("app.new", "New"),
            ("app.open", "Open"),
            ("app.save", "Save"),
            ("app.cut", "Cut"),
            ("app.copy", "Copy"),
            ("app.paste", "Paste"),
        ] {
            let status = status.clone();
            ctx.register_action(
                Action::new(id).on_invoke(move |_, _| status.set(format!("{label} chosen"))),
            );
        }
        ctx.register_action(Action::new("app.quit").on_invoke(|_, ctx| ctx.close_window()));

        // One declarative model, shared by the in-window bar and the OS menu.
        let model = MenuModel::new()
            .menu(lit!("&File"), |m| {
                m.item(MenuEntry::new(lit!("&New")).intent("app.new").shortcut("app.new"))
                    .item(MenuEntry::new(lit!("&Open")).intent("app.open").shortcut("app.open"))
                    // Reactively enabled — greys out until "Allow Save" is on.
                    .item(MenuEntry::new(lit!("&Save")).intent("app.save").enabled(can_save.clone()))
                    // Empty submenu populated at runtime (see the body buttons).
                    .submenu_with_id(recent_menu, lit!("Open &Recent"), |s| s)
                    .separator()
                    .item(MenuEntry::new(lit!("&Quit")).intent("app.quit").shortcut("app.quit"))
            })
            .menu(lit!("&Edit"), |m| {
                m.item(MenuEntry::new(lit!("Cu&t")).intent("app.cut"))
                    .item(MenuEntry::new(lit!("&Copy")).intent("app.copy"))
                    .item(MenuEntry::new(lit!("&Paste")).intent("app.paste"))
            })
            .menu(lit!("&View"), |m| {
                m.item(
                    MenuEntry::new(lit!("Show &Grid"))
                        .checkable(grid_visible.clone())
                        .on_activate({
                            let g = grid_visible.clone();
                            move |_| g.set(!g.get())
                        }),
                )
            });

        // Keep a handle so window buttons can mutate the menu at runtime.
        let model_handle = model.clone();

        // `Suppress` ⇒ on macOS the OS menu bar carries the menus and the
        // in-window strip is hidden; elsewhere the in-window bar renders.
        let menu_bar = ctx.add(
            MenuBar::from_model(model).native_on_macos(NativeMenuMode::Suppress),
        );

        // Runtime controls: toggle Save's enabled state, and add/clear items in
        // the "Open Recent" submenu — both reflected in the native menu live.
        let recent_count = ctx.signal(0_usize);
        let controls = ctx.add(
            HStack::new()
                .spacing(8.0)
                .child(Button::new(lit!("Toggle Save enabled")).on_activate_fn({
                    let can_save = can_save.clone();
                    move |_| can_save.set(!can_save.get())
                }))
                .child(Button::new(lit!("Add recent file")).on_activate_fn({
                    let model = model_handle.clone();
                    let recent_count = recent_count.clone();
                    let status = status.clone();
                    move |_| {
                        let n = recent_count.get() + 1;
                        recent_count.set(n);
                        let name = format!("document-{n}.txt");
                        let status = status.clone();
                        model.push_item(
                            recent_menu,
                            MenuEntry::new(LocalizedString::literal(name.clone()))
                                .on_activate(move |_| status.set(format!("Opened {name}"))),
                        );
                    }
                })),
        );

        let grid_label = grid_visible.map(|v| {
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
                            TextWidget::new(lit!("Native menu bar"))
                                .style(TextStyleRole::BodyBold)
                                .color(TextRole::Primary),
                        )
                        .child(
                            TextWidget::new(lit!(
                                "On macOS the menus live in the system menu bar at the top of \
                                 the screen. Choosing an item updates the status line; the View \
                                 ▸ Show Grid check mark reflects the bound signal. On other \
                                 platforms the same model renders inline above."
                            ))
                            .style(TextStyleRole::Body)
                            .color(TextRole::Primary),
                        )
                        .child(Divider::new().thickness(1.0))
                        .child(
                            TextWidget::new(lit!(""))
                                .bind_text(grid_label)
                                .style(TextStyleRole::Body)
                                .color(TextRole::Secondary),
                        )
                        .child(
                            TextWidget::new(lit!(
                                "Runtime control — these mutate the menu live (enable Save, or add \
                                 items to File ▸ Open Recent):"
                            ))
                            .style(TextStyleRole::Small)
                            .color(TextRole::Secondary),
                        )
                        .add_child(controls),
                ),
            ),
        );

        let root = ctx.add(
            VStack::new()
                .add_child(menu_bar)
                .child(Expand::new().child(Padding::uniform(24.0).child_id(body)))
                .child(
                    StatusBar::new().child(
                        TextWidget::new(lit!(""))
                            .bind_text(status)
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

fn main() {
    BastydeAppBuilder::new()
        .install_native_menu()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Native Menu")
                .size(720, 480)
                .root(|tree, _state| tree.add(Root::new())),
        )
        .run();
}
