//! Spec §2: the `fern!(ctx => ...)` preamble wraps the root in
//! `ctx.add(...)` and returns a `WidgetId`.

use fern_ui::prelude::*;

#[derive(Debug)]
struct Leaf;

impl Leaf {
    fn new() -> Self {
        Leaf
    }
}

impl Widget for Leaf {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        proposal.resolve(0.0, 0.0)
    }
}

fn main() {
    let mut tree = fern_ui::core::WidgetTree::new();
    let id: WidgetId = fern!(tree => Leaf);
    // The id is a real arena entry the tree knows about.
    let _children = tree.children(id);
}
