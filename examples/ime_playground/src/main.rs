// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! IME playground — exercise the full input-method pipeline without an OS IME.
//!
//! Run with: `cargo run -p ime-playground`.
//!
//! Real input methods (Pinyin, Kotoeri, fcitx5, MS-IME, …) feed text
//! through winit `WindowEvent::Ime` events. This demo replays canned
//! composition scripts through the *same* dispatch path via the
//! [`SyntheticImeInject`] hook, so you can watch the whole preedit
//! pipeline — tentative insert, composition underline, caret tracking,
//! candidate-area reporting, accessibility selection, and final commit —
//! on real widgets with nothing installed.
//!
//! How to use:
//!
//! 1. Click into one of the three fields (single-line, secure, or the
//!    multi-line rich editor). The OS IME is enabled for it automatically
//!    (a password field reports `ImePurpose::Password`).
//! 2. Press a replay shortcut — focus stays in the field because a
//!    shortcut never moves focus:
//!    - **F1** — Pinyin: `ni` → `nihao` → commit `你好`.
//!    - **F2** — dead-key accent: `^` → commit `ê`.
//!    - **F3** — start composing `ni`, then cancel (no commit).
//! 3. Watch the underline appear under the composing text and disappear
//!    on commit / cancel. On the secure field the composition shows as
//!    masked bullets and is never exposed to assistive tech.

use bastyde::app::SyntheticImeInject;
use bastyde::core::event::WidgetEvent;
use bastyde::prelude::*;
use bastyde::text_document::TextDocument;
use bastyde::widgets::rich_text::RichTextEditor;
use bastyde::widgets::{Expand, PasswordField, TextInput, TextWidget, VStack};

/// Pinyin: build `nihao` candidate then commit `你好`. The empty preedit
/// is winit's synthetic clear emitted right before the commit.
fn pinyin_script() -> Vec<WidgetEvent> {
    vec![
        WidgetEvent::ImeComposition {
            text: "ni".to_string(),
            cursor: Some(2..2),
        },
        WidgetEvent::ImeComposition {
            text: "nihao".to_string(),
            cursor: Some(5..5),
        },
        WidgetEvent::ImeComposition {
            text: String::new(),
            cursor: None,
        },
        WidgetEvent::ImeCommit {
            text: "你好".to_string(),
        },
    ]
}

/// Dead-key accent: `^` preedit, then commit the composed `ê`.
fn dead_key_script() -> Vec<WidgetEvent> {
    vec![
        WidgetEvent::ImeComposition {
            text: "^".to_string(),
            cursor: Some(1..1),
        },
        WidgetEvent::ImeComposition {
            text: String::new(),
            cursor: None,
        },
        WidgetEvent::ImeCommit {
            text: "ê".to_string(),
        },
    ]
}

/// Begin composing `ni`, then cancel — an empty preedit with no commit.
fn cancel_script() -> Vec<WidgetEvent> {
    vec![
        WidgetEvent::ImeComposition {
            text: "ni".to_string(),
            cursor: Some(2..2),
        },
        WidgetEvent::ImeComposition {
            text: String::new(),
            cursor: None,
        },
    ]
}

#[derive(Debug)]
struct Root {
    single: Signal<String>,
    secret: Signal<String>,
    child: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        Self {
            single: Signal::new(String::new()),
            secret: Signal::new(String::new()),
            child: None,
        }
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Replay shortcuts. A shortcut resolves before the focused
        // widget's key handling and never moves focus, so the scripted
        // events reach whichever field is currently focused.
        for (id, key) in [
            ("ime.replay.pinyin", Key::F1),
            ("ime.replay.deadkey", Key::F2),
            ("ime.replay.cancel", Key::F3),
        ] {
            ctx.register_shortcut_global(
                Shortcut::new(id)
                    .primary(KeyStroke::new(key, Modifiers::NONE))
                    .build(),
            );
        }

        let inject = |events: Vec<WidgetEvent>| {
            move |_intent: &Intent, ctx: &mut EventContext| {
                if let Some(poster) = ctx.poster() {
                    poster.post_external(Box::new(SyntheticImeInject {
                        events: events.clone(),
                    }));
                }
            }
        };
        ctx.register_action(Action::new("ime.replay.pinyin").on_invoke(inject(pinyin_script())));
        ctx.register_action(Action::new("ime.replay.deadkey").on_invoke(inject(dead_key_script())));
        ctx.register_action(Action::new("ime.replay.cancel").on_invoke(inject(cancel_script())));

        let doc = TextDocument::new();

        let root = ctx.add(
            VStack::new()
                .spacing(16.0)
                .child(TextWidget::new(lit!(
                    "Click a field, then press F1 (Pinyin 你好), F2 (dead-key ê), or F3 (cancel)."
                )))
                .child(labeled(
                    "Single-line TextInput",
                    TextInput::new(self.single.clone()).placeholder(lit!("compose here")),
                ))
                .child(labeled(
                    "Secure PasswordField (masked preedit, ImePurpose::Password)",
                    PasswordField::new(self.secret.clone()).label(lit!("Password")),
                ))
                .child(labeled(
                    "Multi-line RichTextEditor",
                    Expand::new().child(RichTextEditor::editor(doc)),
                )),
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
}

/// A label above a control.
fn labeled(label: &str, control: impl Widget + 'static) -> impl Widget {
    VStack::new()
        .spacing(4.0)
        .child(TextWidget::new(lit!(label)).style(TextStyleRole::SmallBold))
        .child(control)
}

fn main() {
    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — IME Playground")
                .size(720, 520)
                .root(|tree, _state| tree.add(Root::new())),
        )
        .run();
}
