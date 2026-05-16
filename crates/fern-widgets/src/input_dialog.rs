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
//! InputDialog::new(tr!("rename_title"))
//!     .prompt(tr!("rename_prompt"))
//!     .default_text(current_name)
//!     .placeholder("New name")
//!     .on_result(|result, _ctx| {
//!         if let Some(name) = result {
//!             rename(name);
//!         }
//!     })
//!     .present(ctx);
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::modal::{ModalCloseBehavior, ModalPresentation, ModalRequest};
use fern_core::signal::Signal;
use fern_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_i18n::LocalizedString;
use fern_tokens::TextStyleRole;

use crate::button::{Button, ButtonVariant};
use crate::dialog::ModalContainer;
use crate::primitives::{HStack, Spacer, TextWidget, VStack};
use crate::text_input::TextInput;

/// A single-field input modal.
pub struct InputDialog {
    title: String,
    prompt: Option<String>,
    placeholder: Option<String>,
    default_text: String,
    ok_label: Option<LocalizedString>,
    cancel_label: Option<LocalizedString>,
    on_result: Option<Box<dyn Fn(Option<String>, &mut EventContext)>>,
}

impl InputDialog {
    /// Construct a new input dialog with the given title.
    pub fn new(title: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = title.into();
        Self {
            title: ls.resolve_now(),
            prompt: None,
            placeholder: None,
            default_text: String::new(),
            ok_label: None,
            cancel_label: None,
            on_result: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw title in
    /// `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(title: impl Into<String>) -> Self {
        Self::new(LocalizedString::literal(title))
    }

    /// Prompt rendered above the input field. Optional but recommended.
    pub fn prompt(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.prompt = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `prompt(...)`.
    #[doc(hidden)]
    pub fn prompt_literal(self, text: impl Into<String>) -> Self {
        self.prompt(LocalizedString::literal(text))
    }

    /// Placeholder shown when the field is empty.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = Some(text.into());
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
                tree.add(
                    ModalContainer::new(InputDialogBody::new(dlg))
                        .title_literal(dialog_title.clone()),
                )
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
    title: String,
    prompt: Option<String>,
    placeholder: Option<String>,
    text: Signal<String>,
    ok_label: LocalizedString,
    cancel_label: LocalizedString,
    on_result: Rc<RefCell<Option<Box<dyn Fn(Option<String>, &mut EventContext)>>>>,
    fired: Rc<std::cell::Cell<bool>>,
    root_child_id: Option<WidgetId>,
}

impl InputDialogBody {
    fn new(dlg: InputDialog) -> Self {
        let ok_label = dlg
            .ok_label
            .unwrap_or_else(|| fern_i18n::tr_widget!(messagebox_btn_ok()));
        let cancel_label = dlg
            .cancel_label
            .unwrap_or_else(|| fern_i18n::tr_widget!(messagebox_btn_cancel()));
        Self {
            title: dlg.title,
            prompt: dlg.prompt,
            placeholder: dlg.placeholder,
            text: Signal::new(dlg.default_text),
            ok_label,
            cancel_label,
            on_result: Rc::new(RefCell::new(dlg.on_result)),
            fired: Rc::new(std::cell::Cell::new(false)),
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
        let title = TextWidget::new_literal(&self.title)
            .style(TextStyleRole::BodyBold)
            .single_line();

        let mut column = VStack::new().spacing(10.0).child(title);
        if let Some(p) = &self.prompt {
            column = column.child(TextWidget::new_literal(p).style(TextStyleRole::Body));
        }

        // The bound text input. Submit-on-Enter accepts the dialog.
        let text_signal = self.text.clone();
        let on_result_for_submit = self.on_result.clone();
        let fired_for_submit = self.fired.clone();
        let mut input = TextInput::new(text_signal.clone()).on_submit_fn(move |ctx| {
            let value = text_signal.get();
            Self::fire(&on_result_for_submit, &fired_for_submit, Some(value), ctx);
        });
        if let Some(ph) = &self.placeholder {
            input = input.placeholder(ph.clone());
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
    ) -> fern_core::widget::LayoutResponse {
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
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    #[test]
    fn input_dialog_body_builds() {
        // Smoke test: the body widget renders without panic when added
        // standalone (without going through present_modal).
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let dlg = InputDialog::new_literal("Rename")
            .prompt_literal("Choose a new name:")
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
