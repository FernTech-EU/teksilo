// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! InputDialog — a `QInputDialog`-style modal that prompts the user for
//! a single string. Built on the same `present_modal` infrastructure as
//! [`MessageBox`](crate::message_box::MessageBox), with a [`TextInput`]
//! body between the prompt and the Ok / Cancel buttons.
//!
//! Use [`MessageBox`](crate::message_box::MessageBox) when the dialog
//! conveys information without requiring data; use `InputDialog` when
//! the modal needs to capture exactly one short string. Forms longer
//! than a single field belong in a custom [`Dialog`](crate::dialog::Dialog).
//!
//! ```ignore
//! InputDialog::new(tr!(rename_title()))
//!     .prompt(tr!(rename_prompt()))
//!     .default_text(current_name)
//!     .placeholder("New name")
//!     .on_result(|result, _ctx| {
//!         if let Some(name) = result {
//!             rename(name);
//!         }
//!     })
//!     .present(ctx);
//! ```
//!
//! ## Live validation
//!
//! [`validate`](InputDialog::validate) runs on every keystroke and both **disables OK**
//! and shows its message under the field, so a value the caller cannot accept can never
//! be submitted:
//!
//! ```ignore
//! InputDialog::new(tr!(save_as_template_title()))
//!     .validate(move |name| {
//!         if name.trim().is_empty() {
//!             Err(None)                                  // block, say nothing
//!         } else if let Some(clash) = taken(name) {
//!             Err(Some(tr!(duplicate(name = clash))))    // block, and explain
//!         } else {
//!             Ok(())
//!         }
//!     })
//!     .on_result(|result, _| { /* only ever called with a valid value */ })
//!     .present(ctx);
//! ```
//!
//! `Err(None)` is the "not yet" case — it disables OK without printing anything, which
//! is what an *untouched* empty field wants: shouting at someone before they have typed
//! is noise, and the greyed button already says the dialog is not ready. A message is
//! withheld until the field has been edited for the same reason, so a caller can return
//! `Err(Some(..))` for the empty case without it flashing on open.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::modal::{ModalCloseBehavior, ModalPresentation, ModalRequest};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::LocalizedString;
use bastyde_tokens::TextStyleRole;

use crate::button::{Button, ButtonVariant};
use crate::dialog::ModalContainer;
use crate::primitives::{HStack, Spacer, TextWidget, VStack};
use crate::text_input::{TextInput, ValidationState};

/// Verdict from an [`InputDialog::validate`] callback.
///
/// `Ok(())` accepts. `Err(None)` blocks silently; `Err(Some(msg))` blocks and shows
/// `msg` beneath the field once it has been edited.
pub type ValidateResult = Result<(), Option<LocalizedString>>;

type ValidatorFn = Rc<dyn Fn(&str) -> ValidateResult>;

/// A single-field input modal.
pub struct InputDialog {
    title: LocalizedString,
    prompt: Option<LocalizedString>,
    placeholder: Option<LocalizedString>,
    default_text: String,
    ok_label: Option<LocalizedString>,
    cancel_label: Option<LocalizedString>,
    on_result: Option<Box<dyn Fn(Option<String>, &mut EventContext)>>,
    validate: Option<ValidatorFn>,
}

impl InputDialog {
    /// Construct a new input dialog with the given title.
    pub fn new(title: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = title.into();
        Self {
            title: ls,
            prompt: None,
            placeholder: None,
            default_text: String::new(),
            ok_label: None,
            cancel_label: None,
            on_result: None,
            validate: None,
        }
    }

    /// Prompt rendered above the input field. Optional but recommended.
    pub fn prompt(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.prompt = Some(ls);
        self
    }

    /// Placeholder shown when the field is empty.
    pub fn placeholder(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.placeholder = Some(ls);
        self
    }

    /// Initial value pre-filled into the field.
    pub fn default_text(mut self, text: impl Into<String>) -> Self {
        self.default_text = text.into();
        self
    }

    /// Override the OK button label (defaults to the framework's
    /// translated "OK" string).
    pub fn ok_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.ok_label = Some(label.into());
        self
    }

    /// Override the Cancel button label (defaults to the framework's
    /// translated "Cancel" string).
    pub fn cancel_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.cancel_label = Some(label.into());
        self
    }

    /// Result callback. Invoked exactly once when the user accepts
    /// (`Some(value)`) or cancels (`None`).
    pub fn on_result(mut self, f: impl Fn(Option<String>, &mut EventContext) + 'static) -> Self {
        self.on_result = Some(Box::new(f));
        self
    }

    /// Install a **live** validator, run on every keystroke.
    ///
    /// While it returns `Err`, the OK button is disabled and Enter does nothing, so
    /// [`on_result`](Self::on_result) is only ever called with a value the validator
    /// accepted (or with `None`, for Cancel). `Err(Some(msg))` shows `msg` under the
    /// field; `Err(None)` blocks without saying anything.
    ///
    /// The message is withheld until the field has been edited, so a validator that
    /// rejects the empty string does not greet the writer with an error on a dialog they
    /// have not yet typed into. The disabled OK is what communicates "not yet" there.
    ///
    /// Distinct from [`TextInput::validator`](crate::text_input::TextInput::validator),
    /// which fires on *commit* and cannot gate a dialog's accept path.
    pub fn validate(mut self, f: impl Fn(&str) -> ValidateResult + 'static) -> Self {
        self.validate = Some(Rc::new(f));
        self
    }

    /// Present the dialog as a modal on top of `ctx`'s tree. Consumes
    /// `self`.
    pub fn present(self, ctx: &mut EventContext) {
        let title = self.title.clone();
        let dialog_title = self.title.clone();
        let mut inner = Some(self);
        ctx.present_modal(
            ModalRequest::deferred(move |tree| {
                let dlg = inner
                    .take()
                    .expect("InputDialog present closure called twice");
                tree.add(ModalContainer::new(InputDialogBody::new(dlg)).title(dialog_title.clone()))
            })
            .presentation(ModalPresentation::Auto)
            .close_behavior(ModalCloseBehavior::EscapeOrClickOutside)
            .title(title)
            .size(420, 180),
        );
    }
}

impl std::fmt::Debug for InputDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputDialog")
            .field("title", &self.title)
            .field("prompt", &self.prompt)
            .field("default_text", &self.default_text)
            .finish()
    }
}

// ── InputDialogBody — the actual widget that renders inside the modal ─

struct InputDialogBody {
    title: LocalizedString,
    prompt: Option<LocalizedString>,
    placeholder: Option<LocalizedString>,
    text: Signal<String>,
    ok_label: LocalizedString,
    cancel_label: LocalizedString,
    on_result: Rc<RefCell<Option<Box<dyn Fn(Option<String>, &mut EventContext)>>>>,
    fired: Rc<std::cell::Cell<bool>>,
    validate: Option<ValidatorFn>,
    /// `true` while the current value is acceptable. Bound to OK's `enabled`, and read
    /// by the Enter path so the two cannot disagree about what is submittable.
    valid: Signal<bool>,
    /// What to show under the field. Held here rather than derived, because
    /// `TextInput::validation` wants a real `Signal` and because the message is
    /// suppressed until `touched` — a rule a `.map()` could not express.
    validation: Signal<ValidationState>,
    /// Whether the field has been edited since the dialog opened.
    touched: Rc<std::cell::Cell<bool>>,
    root_child_id: Option<WidgetId>,
}

impl InputDialogBody {
    fn new(dlg: InputDialog) -> Self {
        let ok_label = dlg
            .ok_label
            .unwrap_or_else(|| bastyde_i18n::tr_widget!(messagebox_btn_ok()));
        let cancel_label = dlg
            .cancel_label
            .unwrap_or_else(|| bastyde_i18n::tr_widget!(messagebox_btn_cancel()));
        // Seeded from the default text, so a dialog that opens pre-filled with an
        // acceptable value has OK live immediately, and one that opens empty under a
        // reject-empty validator opens with OK already greyed.
        let initial_valid = dlg
            .validate
            .as_ref()
            .map(|f| f(&dlg.default_text).is_ok())
            .unwrap_or(true);
        Self {
            title: dlg.title,
            prompt: dlg.prompt,
            placeholder: dlg.placeholder,
            text: Signal::new(dlg.default_text),
            ok_label,
            cancel_label,
            on_result: Rc::new(RefCell::new(dlg.on_result)),
            fired: Rc::new(std::cell::Cell::new(false)),
            validate: dlg.validate,
            valid: Signal::new(initial_valid),
            validation: Signal::new(ValidationState::None),
            touched: Rc::new(std::cell::Cell::new(false)),
            root_child_id: None,
        }
    }

    fn fire(
        on_result: &Rc<RefCell<Option<Box<dyn Fn(Option<String>, &mut EventContext)>>>>,
        fired: &Rc<std::cell::Cell<bool>>,
        value: Option<String>,
        ctx: &mut EventContext,
    ) {
        if fired.replace(true) {
            return;
        }
        if let Some(handler) = on_result.borrow().as_ref() {
            handler(value, ctx);
        }
        ctx.dismiss_modal();
    }
}

impl std::fmt::Debug for InputDialogBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputDialogBody")
            .field("title", &self.title)
            .finish()
    }
}

impl Widget for InputDialogBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let title = TextWidget::new(self.title.clone())
            .style(TextStyleRole::BodyBold)
            .single_line();

        let mut column = VStack::new().spacing(10.0).child(title);
        if let Some(p) = &self.prompt {
            column = column.child(TextWidget::new(p.clone()).style(TextStyleRole::Body));
        }

        // Live validation. Pushed from an effect on the text rather than bound: the
        // verdict feeds two places (the field's message and OK's enabled state) and one
        // of them — the message — is additionally gated on `touched`, which no derived
        // signal could express.
        if let Some(validate) = self.validate.clone() {
            let valid = self.valid.clone();
            let validation = self.validation.clone();
            let touched = self.touched.clone();
            let initial = self.text.get();
            ctx.effect(&self.text, move |typed| {
                // The first callback fires with the seeded value, before any keystroke;
                // only a real change counts as an edit.
                if *typed != initial {
                    touched.set(true);
                }
                match validate(typed) {
                    Ok(()) => {
                        valid.set(true);
                        validation.set(ValidationState::None);
                    }
                    Err(msg) => {
                        valid.set(false);
                        validation.set(match msg {
                            Some(m) if touched.get() => ValidationState::Error(m),
                            // Either the caller chose to stay silent, or the writer has
                            // not typed yet. The greyed OK carries the message instead.
                            _ => ValidationState::None,
                        });
                    }
                }
            });
        }

        // The bound text input. Submit-on-Enter accepts the dialog — but only when the
        // value is acceptable, or Enter would bypass the disabled OK button.
        let text_signal = self.text.clone();
        let on_result_for_submit = self.on_result.clone();
        let fired_for_submit = self.fired.clone();
        let valid_for_submit = self.valid.clone();
        let mut input = TextInput::new(text_signal.clone()).on_submit_fn(move |ctx| {
            if !valid_for_submit.get() {
                return;
            }
            let value = text_signal.get();
            Self::fire(&on_result_for_submit, &fired_for_submit, Some(value), ctx);
        });
        if let Some(ph) = &self.placeholder {
            input = input.placeholder(ph.clone());
        }
        if self.validate.is_some() {
            input = input.validation(self.validation.clone());
        }
        column = column.child(input);

        // Footer: Spacer + Cancel + OK (right-aligned).
        let on_result_cancel = self.on_result.clone();
        let fired_cancel = self.fired.clone();
        let cancel_label = self.cancel_label.clone();
        let cancel_btn = Button::new(cancel_label)
            .variant(ButtonVariant::Plain)
            .on_activate_fn(move |ctx| {
                Self::fire(&on_result_cancel, &fired_cancel, None, ctx);
            });

        let on_result_ok = self.on_result.clone();
        let fired_ok = self.fired.clone();
        let text_for_ok = self.text.clone();
        let ok_label = self.ok_label.clone();
        let ok_btn = Button::new(ok_label)
            .variant(ButtonVariant::Filled)
            .enabled(self.valid.clone())
            .on_activate_fn(move |ctx| {
                let value = text_for_ok.get();
                Self::fire(&on_result_ok, &fired_ok, Some(value), ctx);
            });

        let footer = HStack::new()
            .spacing(8.0)
            .child(Spacer::new())
            .child(cancel_btn)
            .child(ok_btn);
        column = column.child(footer);

        let root = ctx.add(column);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        self.root_child_id
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
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    /// Build a body and lay it out, returning it so its signals can be inspected.
    fn built(dlg: InputDialog) -> (WidgetTree, InputDialogBody) {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let body = InputDialogBody::new(dlg);
        let probe = InputDialogBody {
            title: body.title.clone(),
            prompt: body.prompt.clone(),
            placeholder: body.placeholder.clone(),
            text: body.text.clone(),
            ok_label: body.ok_label.clone(),
            cancel_label: body.cancel_label.clone(),
            on_result: body.on_result.clone(),
            fired: body.fired.clone(),
            validate: body.validate.clone(),
            valid: body.valid.clone(),
            validation: body.validation.clone(),
            touched: body.touched.clone(),
            root_child_id: None,
        };
        tree.add(body);
        tree.layout(SizeProposal {
            width: Some(420.0),
            height: None,
        });
        (tree, probe)
    }

    fn reject_empty() -> impl Fn(&str) -> ValidateResult {
        |v: &str| {
            if v.trim().is_empty() {
                Err(Some(lit!("Name it")))
            } else {
                Ok(())
            }
        }
    }

    /// Without a validator nothing changes: OK is live from the start, which is what
    /// every existing caller relies on.
    #[test]
    fn no_validator_leaves_ok_enabled() {
        let (_t, b) = built(InputDialog::new(lit!("T")));
        assert!(b.valid.get());
    }

    /// A dialog that opens empty under a reject-empty validator opens with OK greyed —
    /// the seed is validated, not assumed good.
    #[test]
    fn an_invalid_default_opens_with_ok_disabled() {
        let (_t, b) = built(InputDialog::new(lit!("T")).validate(reject_empty()));
        assert!(!b.valid.get());
    }

    #[test]
    fn a_valid_default_opens_with_ok_enabled() {
        let (_t, b) = built(
            InputDialog::new(lit!("T"))
                .default_text("Chapter One")
                .validate(reject_empty()),
        );
        assert!(b.valid.get());
    }

    /// The message is withheld until the field is edited: an error on a dialog nobody has
    /// typed into yet is noise, and the greyed OK already says it is not ready.
    #[test]
    fn the_message_is_withheld_until_the_field_is_edited() {
        let (_t, b) = built(InputDialog::new(lit!("T")).validate(reject_empty()));
        assert!(
            matches!(b.validation.get(), ValidationState::None),
            "silent while untouched"
        );
        assert!(!b.valid.get(), "but still not submittable");

        b.text.set("x".into());
        b.text.set("".into());
        assert!(
            matches!(b.validation.get(), ValidationState::Error(_)),
            "once edited, an empty value explains itself"
        );
    }

    /// Typing something acceptable clears both the block and the message.
    #[test]
    fn a_valid_value_clears_the_block_and_the_message() {
        let (_t, b) = built(InputDialog::new(lit!("T")).validate(reject_empty()));
        b.text.set("Character sheet".into());
        assert!(b.valid.get());
        assert!(matches!(b.validation.get(), ValidationState::None));
    }

    /// `Err(None)` blocks without printing anything — the "not yet" case.
    #[test]
    fn a_silent_rejection_blocks_without_a_message() {
        let (_t, b) = built(
            InputDialog::new(lit!("T"))
                .default_text("seed")
                .validate(|_: &str| Err(None)),
        );
        b.text.set("anything".into());
        assert!(!b.valid.get());
        assert!(matches!(b.validation.get(), ValidationState::None));
    }

    #[test]
    fn input_dialog_body_builds() {
        // Smoke test: the body widget renders without panic when added
        // standalone (without going through present_modal).
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let dlg = InputDialog::new(lit!("Rename"))
            .prompt(lit!("Choose a new name:"))
            .default_text("untitled");
        let body = InputDialogBody::new(dlg);
        let id = tree.add(body);
        tree.layout(SizeProposal {
            width: Some(420.0),
            height: None,
        });
        let b = tree.bounds(id);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }
}
