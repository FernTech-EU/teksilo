// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Multi-Window Demo — exercises the new per-window `WindowState` +
//! `EventContext::open_window` API end-to-end.
//!
//! Run with: `cargo run -p multi-window`
//!
//! What this demo covers:
//!
//! - A main window opened with an explicit `WindowConfig`.
//! - `F11` shortcut → `WindowPlacement::Fullscreen` / `Floating`
//!   flipped through the state's placement signal. The drain loop
//!   translates the write into the appropriate winit call.
//! - `F1` shortcut → open a secondary Help window. Idempotent via
//!   `ctx.find_window("help")`: a second press focuses the existing
//!   window.
//! - A main-window button that fires the same shortcut's intent.

use bastyde::IntentKind;
use bastyde::core::Action;
use bastyde::core::shortcut::{KeyStroke, Shortcut};
use bastyde::prelude::*;
use bastyde::widgets::{Button, ButtonVariant, Expand, HStack, Spacer, Toolbar};

#[derive(Debug, IntentKind)]
enum AppIntent {
    #[name = "app.help"]
    ShowHelp,
    #[name = "app.toggle_fullscreen"]
    #[allow(dead_code)]
    ToggleFullscreen,
}

fn dark_mode_toolbar() -> impl Widget {
    Toolbar::new().child(
        HStack::new()
            .child(Spacer::new())
            .child(bastyde::widgets::ThemeSwitcher::new()),
    )
}

fn main() {
    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Multi-Window Demo")
                .id("main")
                .size(520, 320)
                .min_size(320, 200)
                .initial_placement(WindowPlacement::Floating)
                .root(|tree, _state| {
                    tree.add(
                        bastyde::widgets::VStack::new()
                            .child(dark_mode_toolbar())
                            .child(Expand::new().child(MainRoot::default())),
                    )
                }),
        )
        .run();
}

#[derive(Debug, Default)]
struct MainRoot {
    child: Option<WidgetId>,
}

impl Widget for MainRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.register_shortcut_global(
            Shortcut::new("app.help")
                .name("Open help")
                .primary(KeyStroke::new(Key::F1, Modifiers::empty()))
                .build(),
        );
        ctx.register_shortcut_global(
            Shortcut::new("app.toggle_fullscreen")
                .name("Toggle fullscreen")
                .primary(KeyStroke::new(Key::F11, Modifiers::empty()))
                .build(),
        );

        ctx.register_action(Action::new("app.help").on_invoke(|_i, ctx| {
            if let Some(id) = ctx.find_window("help") {
                ctx.focus_window(id);
                return;
            }
            ctx.open_window(
                WindowConfig::new()
                    .title("Bastyde — Help")
                    .id("help")
                    .size(360, 220)
                    .root(|tree, _state| {
                        tree.add(
                            Button::new(lit!("Close help"))
                                .variant(ButtonVariant::Filled)
                                .on_activate_fn(|ctx| ctx.close_window()),
                        )
                    }),
            );
        }));

        ctx.register_action(Action::new("app.toggle_fullscreen").on_invoke(|_i, ctx| {
            let Some(w) = ctx.window() else { return };
            let next = if w.placement().get().is_fullscreen() {
                WindowPlacement::Floating
            } else {
                WindowPlacement::Fullscreen
            };
            w.placement().set(next);
        }));

        let btn = ctx.add(
            Button::new(lit!("Open help (F1) / Toggle fullscreen (F11)"))
                .variant(ButtonVariant::Filled)
                .on_activate_fn(|ctx| ctx.send_intent(AppIntent::ShowHelp)),
        );
        self.child = Some(btn);
        vec![btn]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.child
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }
}
