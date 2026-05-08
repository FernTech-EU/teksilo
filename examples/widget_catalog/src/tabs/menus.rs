//! Menus tab — MenuBar, MenuList, MenuItem.

use fern_ui::prelude::*;
use fern_ui::widgets::{Divider, MenuBar, MenuItem, MenuList, TextWidget, VStack};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_menus_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_menus_refs())
}

fn make_menu_bar() -> MenuBar {
    MenuBar::new()
        .menu(tr!(mnu_file()), || {
            Box::new(
                MenuList::new()
                    .item(MenuItem::new(tr!(demo_new())).on_activate_fn(|_| println!("New")))
                    .item(MenuItem::new(tr!(demo_open())).on_activate_fn(|_| println!("Open")))
                    .separator()
                    .item(MenuItem::new(tr!(demo_quit())).on_activate_fn(|_| println!("Quit"))),
            )
        })
        .menu(tr!(mnu_menu_edit()), || {
            Box::new(
                MenuList::new()
                    .item(MenuItem::new(tr!(demo_undo())).on_activate_fn(|_| println!("Undo")))
                    .item(MenuItem::new(tr!(demo_redo())).on_activate_fn(|_| println!("Redo")))
                    .separator()
                    .item(MenuItem::new(tr!(demo_cut())).on_activate_fn(|_| println!("Cut")))
                    .item(MenuItem::new(tr!(demo_copy())).on_activate_fn(|_| println!("Copy")))
                    .item(MenuItem::new(tr!(demo_paste())).on_activate_fn(|_| println!("Paste"))),
            )
        })
}

fn make_menu_list() -> MenuList {
    MenuList::new()
        .item(MenuItem::new(tr!(demo_cut())).on_activate_fn(|_| println!("Cut")))
        .item(MenuItem::new(tr!(demo_copy())).on_activate_fn(|_| println!("Copy")))
        .item(MenuItem::new(tr!(demo_paste())).on_activate_fn(|_| println!("Paste")))
        .separator()
        .item(MenuItem::new(tr!(demo_find())).on_activate_fn(|_| println!("Find")))
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let menu_bar = section(ctx, "MenuBar", make_menu_bar());
    let menu_list = section(ctx, "MenuList (standalone)", make_menu_list());
    let menu_item = section(
        ctx,
        "MenuItem (standalone)",
        VStack::new()
            .spacing(2.0)
            .child(MenuItem::new(tr!(mnu_standalone_a())))
            .child(MenuItem::new(tr!(mnu_with_shortcut())).shortcut_label("Ctrl+S"))
            .child(MenuItem::new(tr!(mnu_disabled())).enabled(false)),
    );
    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(menu_bar)
            .add_child(menu_list)
            .add_child(menu_item),
    )
}

pub fn fern(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // MenuBar's `.menu(...)` method takes a closure — fern! property
    // syntax can't express that cleanly, so we pre-register.
    let menu_bar = ctx.add(make_menu_bar());
    let menu_list = ctx.add(make_menu_list());

    fern!(ctx =>
        VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_menus_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_menus_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider {}

            VStack {
                spacing: 6.0
                TextWidget::new_literal("MenuBar") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ menu_bar }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("MenuList (standalone)") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ menu_list }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("MenuItem (standalone)") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 2.0
                    MenuItem::new(tr!(mnu_standalone_a()))
                    MenuItem::new(tr!(mnu_with_shortcut())) { shortcut_label: "Ctrl+S" }
                    MenuItem::new(tr!(mnu_disabled())) { enabled: false }
                }
            }
        }
    )
}
