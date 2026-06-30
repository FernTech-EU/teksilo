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
//! - **Window-active appearance**: each window shows a status label, a
//!   `TextInput`, and a `.dim_when_inactive(..)` panel. Click between the two
//!   windows to watch the *inactive* one hide its caret, mute its text
//!   selection, dim its panel, and flip its status label — the modern
//!   `appearsActive` / `:backdrop` behaviour, driven by the per-window
//!   `window_active` signal.

use bastyde::IntentKind;
use bastyde::core::Action;
use bastyde::core::shortcut::{KeyStroke, Shortcut};
use bastyde::prelude::*;
use bastyde::widgets::{
    Button, ButtonVariant, Expand, HStack, Spacer, TextInput, TextWidget, Toolbar, VStack,
};

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
        .install_automation_bridge_in_debug()
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
                        VStack::new()
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
                            VStack::new()
                                .spacing(12.0)
                                .child(TextWidget::new(lit!(
                                    "Focus the main window and watch this field's caret vanish."
                                )))
                                .child(TextInput::new(Signal::new(
                                    "editable — select me, then switch windows".to_string(),
                                )))
                                .child(
                                    Button::new(lit!("Close help"))
                                        .variant(ButtonVariant::Filled)
                                        .on_activate_fn(|ctx| ctx.close_window()),
                                ),
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

        // Status label reflecting *this* window's active state, bound to the
        // per-window `window_active` signal (the SwiftUI `appearsActive` /
        // GTK `:backdrop` equivalent). Re-renders on focus/occlusion change,
        // no rebuild.
        let status = ctx.window_active_signal().map(|active| {
            if *active {
                "● ACTIVE — caret blinks, selection vivid, panel at full opacity".to_string()
            } else {
                "○ INACTIVE — caret hidden, selection muted, panel dimmed".to_string()
            }
        });

        let field_text =
            ctx.signal("Select some of this text, then click the Help window →".to_string());

        let root = ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(TextWidget::new(lit!("")).bind_text(status))
                // Caret hides + selection desaturates automatically when the
                // window is inactive — no per-widget opt-in.
                .child(TextInput::new(field_text))
                // Custom content opts in to dimming via the builder modifier.
                .child(
                    Button::new(lit!("This panel dims when the window is inactive"))
                        .variant(ButtonVariant::Filled)
                        .dim_when_inactive(0.4),
                )
                .child(
                    Button::new(lit!("Open help (F1) / Toggle fullscreen (F11)"))
                        .variant(ButtonVariant::Filled)
                        .on_activate_fn(|ctx| ctx.send_intent(AppIntent::ShowHelp)),
                ),
        );
        self.child = Some(root);
        vec![root]
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
