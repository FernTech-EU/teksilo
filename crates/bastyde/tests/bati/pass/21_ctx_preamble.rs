//! Spec §2: the `bati!(ctx => ...)` preamble wraps the root in
//! `ctx.add(...)` and returns a `WidgetId`.

use bastyde::prelude::*;

#[derive(Debug)]
struct Leaf;

impl Leaf {
    fn new() -> Self {
        Leaf
    }
}

impl Widget for Leaf {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let mut tree = bastyde::core::WidgetTree::new();
    let id: WidgetId = bati!(tree => Leaf);
    // The id is a real arena entry the tree knows about.
    let _children = tree.children(id);
}
