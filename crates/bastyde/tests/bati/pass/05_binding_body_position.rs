//! Spec §3.3: a body-position binding `name = Element` hoists to the
//! enclosing bati! block as `let name = ctx.add(...)` and attaches via
//! `.add_child(name)` on the parent. The binding's id is in scope for
//! sibling items in the same body and for nested property values.

use bastyde::prelude::*;

#[derive(Debug)]
struct Button {
    label: &'static str,
}

impl Button {
    fn new(label: &'static str) -> Self {
        Self { label }
    }
}

impl Widget for Button {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug, Default)]
struct Stack {
    child_ids: std::cell::RefCell<Vec<WidgetId>>,
    linked_to: std::cell::RefCell<Option<WidgetId>>,
}

impl Stack {
    fn new() -> Self {
        Self::default()
    }

    fn add_child(self, id: WidgetId) -> Self {
        self.child_ids.borrow_mut().push(id);
        self
    }

    fn child<W: Widget + 'static>(self, _w: W) -> Self {
        self
    }

    fn linked_to(self, id: WidgetId) -> Self {
        *self.linked_to.borrow_mut() = Some(id);
        self
    }
}

impl Widget for Stack {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn build(ctx: &mut BuildContext) -> WidgetId {
    bati!(ctx => Stack {
            open_btn = Button("Open")
            Button("Close")
            linked_to: open_btn
        }
    )
}

fn main() {
    // Compile-time check only — runtime requires a live BuildContext.
    let _build: fn(&mut BuildContext) -> WidgetId = build;
}
