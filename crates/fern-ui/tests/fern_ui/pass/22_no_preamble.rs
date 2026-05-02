//! Spec §2: without `ctx =>`, `fern!(...)` returns a widget value
//! suitable for passing into `.child(...)` or storing in a `let`.

use fern_ui::prelude::*;

#[derive(Debug)]
struct Leaf {
    kind: &'static str,
}

impl Leaf {
    fn new() -> Self {
        Self { kind: "leaf" }
    }
}

impl Widget for Leaf {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let leaf: Leaf = fern!(Leaf);
    assert_eq!(leaf.kind, "leaf");
}
