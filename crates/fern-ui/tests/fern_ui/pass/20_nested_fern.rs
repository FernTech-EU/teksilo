//! Spec §7.6: Dialog's `content` factory closure contains an inner
//! `fern!(...)` call. The inner invocation lowers to a widget value
//! that flows through the closure's return.

use fern_ui::prelude::*;

#[derive(Debug)]
struct Body {
    text: &'static str,
}

impl Body {
    fn new(text: &'static str) -> Self {
        Self { text }
    }
}

impl Widget for Body {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Default)]
struct DialogShell {
    title: Option<&'static str>,
    factory: Option<Box<dyn Fn() -> Body>>,
}

impl std::fmt::Debug for DialogShell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialogShell")
            .field("title", &self.title)
            .field("has_factory", &self.factory.is_some())
            .finish()
    }
}

impl DialogShell {
    fn new_literal(title: &'static str) -> Self {
        Self {
            title: Some(title),
            factory: None,
        }
    }

    fn content<F>(mut self, factory: F) -> Self
    where
        F: Fn() -> Body + 'static,
    {
        self.factory = Some(Box::new(factory));
        self
    }
}

impl Widget for DialogShell {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let d: DialogShell = fern!(
        DialogShell::new_literal("Modal") {
            content: || fern!(Body("the dialog's body"))
        }
    );
    assert_eq!(d.title, Some("Modal"));
    let produced = d.factory.as_ref().expect("factory attached")();
    assert_eq!(produced.text, "the dialog's body");
}
