// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Menus tab — MenuBar, MenuList, MenuItem.

use teksilo::prelude::*;
use teksilo::widgets::{
    CollapsePolicy, Divider, FixedSize, HStack, MenuBar, MenuItem, MenuList, Slider, TextWidget,
    VStack,
};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_menus_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_menus_refs())
}

fn make_menu_bar() -> MenuBar {
    // `no_dispatcher_install()` skips the per-window keyboard-dispatcher
    // registration — the catalog's host window already mounts a
    // primary MenuBar at the top of the chrome (File / Help / Quit /
    // Documentation / About), and the slot is single-occupancy. This
    // MenuBar is a visual showcase of the widget; mouse + arrow-key
    // navigation still work, only F10 / Alt+letter routing is left
    // to the primary one above.
    MenuBar::new()
        .no_dispatcher_install()
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
                    .item(MenuItem::new(tr!(demo_paste())).on_activate_fn(|_| println!("Paste")))
                    .separator()
                    .item(MenuItem::submenu(tr!(mnu_alignment()), || {
                        Box::new(
                            MenuList::new()
                                .item(
                                    MenuItem::new(tr!(mnu_align_left()))
                                        .on_activate_fn(|_| println!("AlignLeft")),
                                )
                                .item(
                                    MenuItem::new(tr!(mnu_align_center()))
                                        .on_activate_fn(|_| println!("AlignCenter")),
                                )
                                .item(
                                    MenuItem::new(tr!(mnu_align_right()))
                                        .on_activate_fn(|_| println!("AlignRight")),
                                ),
                        )
                    })),
            )
        })
}

/// A responsive collapsible (hamburger) menu bar. `no_dispatcher_install`
/// because the catalog's host window already owns the keyboard slot.
fn make_collapsible_bar() -> MenuBar {
    MenuBar::new()
        .collapse_policy(CollapsePolicy::Responsive)
        .no_dispatcher_install()
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
                    .item(MenuItem::new(tr!(demo_redo())).on_activate_fn(|_| println!("Redo"))),
            )
        })
}

/// Slider-driven width box around a responsive collapsible bar: narrow
/// the slider until the menus fold into a ☰, widen it to bring them back.
fn collapsible_section(width: Signal<f32>) -> impl Widget + 'static {
    VStack::new()
        .spacing(8.0)
        .child(
            HStack::new()
                .spacing(8.0)
                .child(TextWidget::new(lit!("Width")).style(TextStyleRole::Small))
                .child(Slider::new(width.clone(), 80.0, 520.0).label(lit!("Bar width"))),
        )
        .child(
            FixedSize::new()
                .width(width)
                .height(36.0_f32)
                .child(make_collapsible_bar()),
        )
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
    let menu_bar = section(ctx, lit!("MenuBar"), make_menu_bar());
    let collapsible_width = ctx.signal(160.0_f32);
    let collapsible = section(
        ctx,
        lit!("MenuBar (collapsible / hamburger)"),
        collapsible_section(collapsible_width),
    );
    let menu_list = section(ctx, tr!(mnu_menu_list_standalone()), make_menu_list());
    let menu_item = section(
        ctx,
        tr!(mnu_menu_item_standalone()),
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
            .add_child(collapsible)
            .add_child(menu_list)
            .add_child(menu_item),
    )
}

pub fn teksu(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // MenuBar's `.menu(...)` method takes a closure — teksu! property
    // syntax can't express that cleanly, so we pre-register.
    let menu_bar = ctx.add(make_menu_bar());
    let collapsible_width = ctx.signal(160.0_f32);
    let collapsible = ctx.add(collapsible_section(collapsible_width));
    let menu_list = ctx.add(make_menu_list());

    teksu!(ctx => VStack {
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
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("MenuBar")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ menu_bar }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("MenuBar (collapsible / hamburger)")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ collapsible }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(mnu_menu_list_standalone())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ menu_list }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(mnu_menu_item_standalone())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 2.0
                    MenuItem::new(tr!(mnu_standalone_a()))
                    MenuItem::new(tr!(mnu_with_shortcut())) {
                        shortcut_label: "Ctrl+S"
                    }
                    MenuItem::new(tr!(mnu_disabled())) {
                        enabled: false
                    }
                }
            }
        }
    )
}
