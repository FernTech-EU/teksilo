//! Spec §5.1: `if cond { Elem }` without an `else` arm lowers to
//! `.child_opt(if cond { Some(Elem::new()) } else { None })`. The
//! parent must expose a `.child_opt(Option<W>)` method — stacks do.

use fern_ui::prelude::*;

#[derive(Debug)]
struct Banner {
    kind: &'static str,
}

impl Banner {
    fn new(kind: &'static str) -> Self {
        Self { kind }
    }
}

impl Widget for Banner {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug, Default)]
struct StackLike {
    visible: std::cell::RefCell<Vec<&'static str>>,
}

impl StackLike {
    fn new() -> Self {
        Self::default()
    }

    fn child_opt<W: Widget + 'static>(self, widget: Option<W>) -> Self {
        if widget.is_some() {
            // The real VStack inspects the widget; we just record
            // that a child was attached. Using `is_some()` avoids
            // needing to downcast.
            self.visible.borrow_mut().push("present");
        }
        self
    }

    fn child(self, _: Banner) -> Self {
        self
    }
}

impl Widget for StackLike {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let is_logged_in = true;
    let is_error = false;

    let s_present: StackLike = fern!(
        StackLike {
            if is_logged_in { Banner("profile") }
            if is_error { Banner("error") }
        }
    );
    assert_eq!(*s_present.visible.borrow(), vec!["present"]);
}
