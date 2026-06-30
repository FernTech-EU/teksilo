// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! PasswordField demo — a sign-in form plus an echo-mode showcase.
//!
//! Run with: `cargo run -p password-field`.
//!
//! What's on screen:
//!
//! - A **sign-in card**: a username `TextInput`, a `PasswordField` with
//!   a click-to-reveal eye toggle and an 8-character minimum validator,
//!   and a "Confirm password" field that validates equality. The
//!   "Sign in" button enables only when both fields are valid and
//!   match.
//! - An **echo-mode showcase**: the same secret shown as `Masked`,
//!   `RevealWhileTyping` (clear while focused), `NoEcho` (length
//!   hidden), and with a **hold-to-reveal** button (`RevealMode::Hold`).
//!
//! Things to try:
//!
//! - Click the eye to reveal; copy is blocked while masked and allowed
//!   once revealed.
//! - Turn **Caps Lock** on and focus a field — a warning glyph appears
//!   and screen readers announce it.
//! - Tab between the field and its eye button: the focus ring stays lit
//!   across the whole control.

use bastyde::core::WidgetPlacement;
use bastyde::prelude::*;
use bastyde::widgets::{
    AtRevealPolicy, Button, ButtonVariant, EchoMode, Expand, GroupBox, HStack, Padding,
    PasswordField, RevealMode, Spacer, TextInput, TextWidget, ThemeSwitcher, Toolbar, VStack,
    ValidationOutcome,
};

fn dark_mode_toolbar() -> impl Widget {
    bati!(
        Toolbar {
            HStack {
                TextWidget::new(lit!("PasswordField demo")) {
                    style: TextStyleRole::BodyBold
                }
                Spacer
                ThemeSwitcher::new()
            }
        }
    )
}

/// Minimum acceptable password length.
const MIN_LEN: usize = 8;

#[derive(Debug)]
struct Root {
    password: Signal<String>,
    confirm: Signal<String>,
    username: Signal<String>,
    child: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        Self {
            password: Signal::new(String::new()),
            confirm: Signal::new(String::new()),
            username: Signal::new(String::new()),
            child: None,
        }
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // "Sign in" enables only when both fields are valid and equal.
        let pw = self.password.clone();
        let cf = self.confirm.clone();
        let can_submit = pw
            .zip(&cf)
            .map(|(p, c)| p.chars().count() >= MIN_LEN && p == c);

        let card = GroupBox::new(lit!("Sign in")).child(
            VStack::new()
                .spacing(12.0)
                .child(labeled(
                    "Username",
                    TextInput::new(self.username.clone()).placeholder(lit!("you@example.com")),
                ))
                .child(labeled(
                    "Password",
                    PasswordField::new(self.password.clone())
                        .label(lit!("Password"))
                        .placeholder(lit!("At least 8 characters"))
                        .validator(|s| {
                            if s.chars().count() >= MIN_LEN {
                                ValidationOutcome::Valid
                            } else {
                                ValidationOutcome::Invalid {
                                    message: lit!(format!("Use at least {MIN_LEN} characters")),
                                }
                            }
                        }),
                ))
                .child(labeled(
                    "Confirm password",
                    PasswordField::new(self.confirm.clone())
                        .label(lit!("Confirm password"))
                        .validator({
                            let pw = self.password.clone();
                            move |s| {
                                if s == pw.get().as_str() {
                                    ValidationOutcome::Valid
                                } else {
                                    ValidationOutcome::Invalid {
                                        message: lit!("Passwords don't match"),
                                    }
                                }
                            }
                        }),
                ))
                .child({
                    let btn = Button::new(lit!("Sign in")).variant(ButtonVariant::Filled);
                    let id = ctx.add(btn);
                    ctx.enabled_when(id, can_submit);
                    HStack::new().child(Spacer::new()).add_child(id)
                }),
        );

        let showcase = GroupBox::new(lit!("Echo modes")).child(
            VStack::new()
                .spacing(12.0)
                .child(labeled(
                    "Masked (default)",
                    PasswordField::new(Signal::new("hunter2".to_string())).label(lit!("Masked")),
                ))
                .child(labeled(
                    "Reveal while typing",
                    PasswordField::new(Signal::new("hunter2".to_string()))
                        .label(lit!("Reveal while typing"))
                        .echo_mode(EchoMode::RevealWhileTyping),
                ))
                .child(labeled(
                    "No echo (length hidden)",
                    PasswordField::new(Signal::new("hunter2".to_string()))
                        .label(lit!("No echo"))
                        .echo_mode(EchoMode::NoEcho),
                ))
                .child(labeled(
                    "Hold to reveal (press the eye)",
                    PasswordField::new(Signal::new("hunter2".to_string()))
                        .label(lit!("Hold to reveal"))
                        .reveal_mode(RevealMode::Hold),
                ))
                .child(labeled(
                    "Always protected for AT",
                    PasswordField::new(Signal::new("hunter2".to_string()))
                        .label(lit!("Always protected"))
                        .at_reveal_policy(AtRevealPolicy::AlwaysProtected),
                )),
        );

        let root = ctx.add(
            Padding::new(20.0, 20.0, 20.0, 20.0).child(
                HStack::new()
                    .spacing(24.0)
                    .child(Expand::new().child(card))
                    .child(Expand::new().child(showcase)),
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

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        if let Some(p) = children.first_mut() {
            p.origin = Point::new(bounds.x, bounds.y);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }
}

/// A small caption above a field.
fn labeled(caption: &str, field: impl Widget + 'static) -> impl Widget {
    VStack::new()
        .spacing(4.0)
        .child(
            TextWidget::new(lit!(caption))
                .style(TextStyleRole::Small)
                .color(TextRole::Secondary),
        )
        .child(field)
}

fn main() {
    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — PasswordField")
                .size(820, 520)
                .root(|tree, _state| {
                    bati!(tree => VStack {
                        child: dark_mode_toolbar()
                        Expand {
                            Root::new()
                        }
                    })
                }),
        )
        .run();
}
