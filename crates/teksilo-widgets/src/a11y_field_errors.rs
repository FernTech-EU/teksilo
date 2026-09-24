// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A field's validation message, as a screen reader meets it (WCAG 3.3.1).
//!
//! `TextInput`, `PasswordField`, `DateTimeEdit` and `DateRangeEdit` draw their
//! message in a `ValidationStrip` under the field, a live region, and point
//! the field that takes focus at it with `described_by`. That relation reaches
//! no reader by itself through AccessKit, so the tree writes the strip's text
//! into the field's description (`teksilo_core`'s
//! `accessibility_description_impl`), and two things are asked of every one of
//! them here:
//!
//! - the message is said **once** when it appears while the user is in the
//!   field, the strip's announcement, and not a second time as the field's
//!   description changing under the user, which Orca speaks;
//! - it is not begun again as the user **leaves** the field, where Orca, which
//!   still holds the field as its focus while it hears the field's changes,
//!   would start saying it and cut it for the next field's name;
//! - it is **read again** with the field when the user comes back to it, which
//!   is what the relation is for and what nobody heard before.
//!
//! [`Ears`] hears each delivered update the way the readers do (the module docs
//! of `accessibility_description_impl` give the sources): an announcement is
//! spoken; arriving at a node reads its name and its description; and a change
//! to the description of the node the reader holds as focus is spoken too,
//! which only Orca does, so "once" is asserted against the strictest reader.
//! What was said before a focus move in the same update is cut by the arrival
//! where this host's reader cuts it (Orca on AT-SPI, and VoiceOver, which the
//! tree treats the same way), and heard on Windows, where NVDA keeps it.

#![cfg(test)]

use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::{NodeId, TreeUpdate};
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::signal::Signal;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use crate::button::Button;
use crate::password_field::PasswordField;
use crate::primitives::text_input_field::{ValidationFeedback, ValidationOutcome};
use crate::text_input::{TextInput, ValidationState};
use crate::{DateRangeEdit, DateTimeEdit};

const MESSAGE: &str = "Must be eight characters or more";

/// Whether this host's reader cuts what it was saying to read a new focus,
/// the platform choice `teksilo_core` makes for the same reason.
const FOCUS_READ_CUTS: bool = !cfg!(target_os = "windows");

/// What the readers say, update by update.
#[derive(Default)]
struct Ears {
    heard: Vec<String>,
    /// Begun, then cut by the read of a new focus.
    cut: Vec<String>,
    seq: u64,
    focus: Option<NodeId>,
    focus_description: Option<String>,
}

impl Ears {
    fn hear(&mut self, tree: &mut WidgetTree) {
        settle(tree);
        let update = tree.sync_accessibility();
        // What the adapters send before the focus move (`accesskit_consumer`
        // `tree.rs:640-673`): the announcements, and a change to the
        // description of the node the reader still holds as focus.
        let mut before_focus: Vec<String> = Vec::new();
        for announcement in tree.announcements_since(self.seq) {
            self.seq = announcement.seq;
            before_focus.push(announcement.text);
        }
        if let Some(held) = self.focus {
            let description = description_of(&update, held);
            if description != self.focus_description {
                before_focus.extend(description);
            }
        }
        let description = description_of(&update, update.focus);
        if self.focus != Some(update.focus) {
            if FOCUS_READ_CUTS {
                self.cut.append(&mut before_focus);
            } else {
                self.heard.append(&mut before_focus);
            }
            let name = update
                .nodes
                .iter()
                .find(|(id, _)| *id == update.focus)
                .and_then(|(_, node)| node.label());
            self.heard.extend(name.map(str::to_owned));
            self.heard.extend(description.clone());
        } else {
            self.heard.append(&mut before_focus);
        }
        self.focus = Some(update.focus);
        self.focus_description = description;
    }

    fn times(&self, text: &str) -> usize {
        self.heard.iter().filter(|said| said.contains(text)).count()
    }

    fn cut_times(&self, text: &str) -> usize {
        self.cut.iter().filter(|said| said.contains(text)).count()
    }
}

fn settle(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.layout(SizeProposal::exact(600.0, 200.0));
}

/// The description an adapter hands the platform for `target`, read through
/// the consumer every adapter is built on.
fn description_of(update: &TreeUpdate, target: NodeId) -> Option<String> {
    let consumer = accesskit_consumer::Tree::new(update.clone(), true);
    let state = consumer.state();
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if node.locate().0 == target {
            return node.description();
        }
        stack.extend(node.children());
    }
    None
}

/// A form holding the widget under test and a button to leave it for.
fn form(widget: impl teksilo_core::widget::Widget + 'static) -> (WidgetTree, WidgetId, WidgetId) {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(widget);
    let button = tree.add(Button::new(lit!("Save")).on_activate_fn(|_| {}));
    settle(&mut tree);
    let field = tree
        .first_focusable_descendant(id)
        .expect("the composite has a field that takes focus");
    let away = tree
        .first_focusable_descendant(button)
        .expect("a button takes focus");
    (tree, field, away)
}

/// Both halves of the contract, for one widget: `refuse` puts [`MESSAGE`] in
/// the strip while focus is on `field`, the way a commit-time validator or an
/// application does.
fn said_once_then_read_on_return(
    mut tree: WidgetTree,
    field: WidgetId,
    away: WidgetId,
    refuse: impl FnOnce(&mut WidgetTree),
) {
    let mut ears = Ears::default();
    tree.focus(field);
    ears.hear(&mut tree);

    refuse(&mut tree);
    ears.hear(&mut tree);
    // Anything else that rebuilds the tree while the user stays: a caret
    // move, a keystroke, a clock. Each is a moment the description could
    // change under the user.
    for _ in 0..2 {
        tree.request_accessibility_update();
        ears.hear(&mut tree);
    }
    assert_eq!(
        ears.times(MESSAGE),
        1,
        "the message is said once where it appears; heard {:?}",
        ears.heard
    );

    tree.focus(away);
    ears.hear(&mut tree);
    assert_eq!(
        (ears.times(MESSAGE), ears.cut_times(MESSAGE)),
        (1, 0),
        "leaving the field does not begin the message again; heard {:?}, cut {:?}",
        ears.heard,
        ears.cut
    );
    tree.focus(field);
    ears.hear(&mut tree);
    assert_eq!(
        ears.times(MESSAGE),
        2,
        "coming back to the field reads the message with it; heard {:?}",
        ears.heard
    );
}

#[test]
fn a_text_inputs_message_is_said_once_and_read_again_on_return() {
    let validation = Signal::new(ValidationState::None);
    let (tree, field, away) = form(
        TextInput::new(Signal::new(String::new()))
            .label(lit!("Password hint"))
            .validation(validation.clone()),
    );
    said_once_then_read_on_return(tree, field, away, move |_| {
        validation.set(ValidationState::Error(lit!(MESSAGE)));
    });
}

/// Refused by its own validator, on Enter, which is the commit that happens
/// while the user is still in the field.
#[test]
fn a_password_fields_message_is_said_once_and_read_again_on_return() {
    let (tree, field, away) = form(
        PasswordField::new(Signal::new("short".to_owned()))
            .label(lit!("Password"))
            .validator(|text| {
                if text.chars().count() < 8 {
                    ValidationOutcome::Invalid {
                        message: lit!(MESSAGE),
                    }
                } else {
                    ValidationOutcome::Valid
                }
            }),
    );
    said_once_then_read_on_return(tree, field, away, |tree| {
        tree.press_key(Key::Enter, Modifiers::NONE);
    });
}

#[test]
fn a_date_time_edits_message_is_said_once_and_read_again_on_return() {
    let edit = DateTimeEdit::new(Signal::new(None));
    let feedback = edit.validation_feedback_signal();
    let (tree, field, away) = form(edit);
    said_once_then_read_on_return(tree, field, away, move |_| {
        feedback.set(ValidationFeedback::Invalid {
            message: lit!(MESSAGE),
        });
    });
}

#[test]
fn a_date_range_edits_message_is_said_once_and_read_again_on_return() {
    let edit = DateRangeEdit::new(Signal::new(None));
    let feedback = edit.validation_feedback_signal();
    let (tree, field, away) = form(edit);
    said_once_then_read_on_return(tree, field, away, move |_| {
        feedback.set(ValidationFeedback::Invalid {
            message: lit!(MESSAGE),
        });
    });
}
