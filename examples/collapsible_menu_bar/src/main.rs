// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Collapsible / Hamburger MenuBar
//!
//! Demonstrates `MenuBar::collapsible()` — the optional hamburger (☰)
//! representation of a menu bar.
//!
//! Two bars are shown:
//!
//! 1. **Top bar — `CollapsePolicy::Always`** (collapsed by default).
//!    It is *always* a hamburger regardless of width and owns the
//!    window's keyboard-dispatch slot, so every reveal path works:
//!    - Click the ☰ button → the bar floats in, *trailing* the button,
//!      with focus on the first menu (File).
//!    - `Alt`+`F` / `Alt`+`E` / `Alt`+`V` → reveal the bar AND open that
//!      menu. `F10` or a bare `Alt`-tap → reveal and focus the first menu.
//!    - Click anywhere outside, or press `Esc`, to hide it again.
//!
//! 2. **Responsive showcase — `CollapsePolicy::Responsive`** (the
//!    default for `.collapsible()`). Drag the slider to change the width
//!    the bar is allotted: when the menus no longer fit it folds into a
//!    hamburger; widen it again and the full inline bar returns. This
//!    second bar uses `.no_dispatcher_install()` (only one bar per window
//!    may own the keyboard slot), so it reveals via mouse.
//!
//! Run with: `cargo run -p collapsible-menu-bar`

use bastyde::IntentKind;
use bastyde::core::Action;
use bastyde::core::shortcut::{KeyStroke, Shortcut};
use bastyde::prelude::*;
use bastyde::widgets::{
    Button, ButtonVariant, CollapsePolicy, Divider, Expand, FixedSize, HStack, MenuBar, MenuItem,
    MenuList, Padding, Panel, ScrollArea, Slider, StatusBar, TextWidget, VStack,
};

#[derive(Debug, IntentKind)]
enum AppIntent {
    #[name = "demo.new"]
    New,
    #[name = "demo.open"]
    Open,
    #[name = "demo.quit"]
    Quit,
}

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

    /// A small reusable menu set so both bars carry the same File/Edit/View.
    fn file_menu() -> Box<dyn Widget> {
        Box::new(
            MenuList::new()
                .item(
                    MenuItem::new(lit!("&New"))
                        .on_activate_fn(|ctx| ctx.send_intent(AppIntent::New))
                        .shortcut_label("Ctrl+N"),
                )
                .item(
                    MenuItem::new(lit!("&Open"))
                        .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Open))
                        .shortcut_label("Ctrl+O"),
                )
                .separator()
                .item(
                    MenuItem::new(lit!("&Quit"))
                        .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Quit)),
                ),
        )
    }

    fn edit_menu() -> Box<dyn Widget> {
        Box::new(
            MenuList::new()
                .item(MenuItem::new(lit!("&Undo")).on_activate_fn(|_| println!("Undo")))
                .item(MenuItem::new(lit!("&Redo")).on_activate_fn(|_| println!("Redo")))
                .separator()
                .item(MenuItem::new(lit!("Cu&t")).on_activate_fn(|_| println!("Cut")))
                .item(MenuItem::new(lit!("&Copy")).on_activate_fn(|_| println!("Copy")))
                .item(MenuItem::new(lit!("&Paste")).on_activate_fn(|_| println!("Paste"))),
        )
    }

    fn view_menu() -> Box<dyn Widget> {
        Box::new(
            MenuList::new()
                .item(MenuItem::new(lit!("Zoom &In")).on_activate_fn(|_| println!("Zoom In")))
                .item(MenuItem::new(lit!("Zoom &Out")).on_activate_fn(|_| println!("Zoom Out")))
                .item(
                    MenuItem::new(lit!("&Reset Zoom")).on_activate_fn(|_| println!("Reset Zoom")),
                ),
        )
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Shortcuts so the menu items read as real commands. They are
        // app-wide and unrelated to the menubar's own Alt-mnemonics.
        ctx.register_shortcut_global(
            Shortcut::new("demo.new")
                .name("New")
                .primary(KeyStroke::ctrl(Key::N))
                .build(),
        );
        ctx.register_shortcut_global(
            Shortcut::new("demo.open")
                .name("Open")
                .primary(KeyStroke::ctrl(Key::O))
                .build(),
        );
        ctx.register_action(Action::new("demo.new").on_invoke(|_, _| println!("New")));
        ctx.register_action(Action::new("demo.open").on_invoke(|_, _| println!("Open")));
        ctx.register_action(Action::new("demo.quit").on_invoke(|_, ctx| ctx.close_window()));

        // ── Bar 1: always-collapsed top menu bar ────────────────────────
        // `CollapsePolicy::Always` ⇒ collapsed by default (a hamburger).
        // It owns the window keyboard-dispatch slot, so Alt+letter / F10 /
        // Alt-tap reveal it too.
        let top_bar = ctx.add(
            MenuBar::new()
                .collapse_policy(CollapsePolicy::Always)
                .menu(lit!("&File"), Root::file_menu)
                .menu(lit!("&Edit"), Root::edit_menu)
                .menu(lit!("&View"), Root::view_menu)
                .trailing_slot(|| {
                    Button::new(lit!("Settings"))
                        .variant(ButtonVariant::Ghost)
                        .on_activate_fn(|_| println!("Settings"))
                }),
        );

        // ── Bar 2: responsive showcase driven by a width slider ─────────
        let demo_width = ctx.signal(640.0_f32);
        let demo_collapsed = ctx.signal(false);
        let demo_bar = ctx.add(
            MenuBar::new()
                .collapsible_bound(demo_collapsed.clone())
                .no_dispatcher_install()
                .menu(lit!("&File"), Root::file_menu)
                .menu(lit!("&Edit"), Root::edit_menu)
                .menu(lit!("&View"), Root::view_menu)
                .trailing_slot(|| {
                    Button::new(lit!("Help"))
                        .variant(ButtonVariant::Ghost)
                        .on_activate_fn(|_| println!("Help"))
                }),
        );
        let demo_bar_slot = ctx.add(
            FixedSize::new()
                .bind_width(demo_width.clone())
                .child_id(demo_bar),
        );
        let demo_state = demo_collapsed.map(|c| {
            if *c {
                "State: collapsed → click the ☰ to reveal the bar trailing it".to_string()
            } else {
                "State: expanded → the full inline bar fits".to_string()
            }
        });

        let intro = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new(lit!("Collapsible / Hamburger MenuBar"))
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new(lit!(
                        "The menu bar at the very top is collapsed by default \
                         (CollapsePolicy::Always). Click the ☰ — or press Alt+F / Alt+E / \
                         Alt+V, F10, or tap Alt — to reveal it. It appears trailing the \
                         button with focus on the first menu, so you can navigate with the \
                         arrow keys and Enter. Click elsewhere or press Esc to hide it."
                    ))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Primary),
                ),
        );

        let responsive_panel = ctx.add(
            Panel::new()
                .background(SurfaceRole::Main)
                .corner_radius(8.0)
                .child(
                    Padding::uniform(16.0).child(
                        VStack::new()
                            .spacing(12.0)
                            .child(
                                TextWidget::new(lit!("Responsive auto-collapse"))
                                    .style(TextStyleRole::BodyBold)
                                    .color(TextRole::Primary),
                            )
                            .child(
                                TextWidget::new(lit!(
                                    "Drag the slider to change the width this bar is given. \
                                     When the menus stop fitting it folds into a hamburger; \
                                     widen it and the inline bar returns."
                                ))
                                .style(TextStyleRole::Body)
                                .color(TextRole::Primary),
                            )
                            .child(
                                HStack::new()
                                    .spacing(8.0)
                                    .child(
                                        TextWidget::new(lit!("Width"))
                                            .style(TextStyleRole::Small)
                                            .color(TextRole::Primary),
                                    )
                                    .child(
                                        Slider::new(demo_width.clone(), 60.0, 760.0)
                                            .label(lit!("Bar width")),
                                    ),
                            )
                            .add_child(demo_bar_slot)
                            .child(
                                TextWidget::new(lit!(""))
                                    .bind_text(demo_state)
                                    .style(TextStyleRole::Small)
                                    .color(TextRole::Secondary),
                            ),
                    ),
                ),
        );

        let content = ctx.add(
            VStack::new()
                .spacing(24.0)
                .add_child(intro)
                .child(Divider::new().thickness(1.0))
                .add_child(responsive_panel),
        );
        let padded = ctx.add(Padding::uniform(24.0).child_id(content));
        let scroll = ctx.add(ScrollArea::from_id(padded));

        let root = ctx.add(
            VStack::new()
                .add_child(top_bar)
                .child(Expand::new().child_id(scroll))
                .child(
                    StatusBar::new().child(
                        TextWidget::new(lit!("Collapsible MenuBar showcase"))
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
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Collapsible MenuBar")
                .size(900, 640)
                .root(|tree, _state| tree.add(Root::new())),
        )
        .run();
}
