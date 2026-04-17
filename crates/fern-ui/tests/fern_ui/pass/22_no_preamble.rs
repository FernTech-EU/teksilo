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
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        proposal.resolve(0.0, 0.0)
    }
}

fn main() {
    let leaf: Leaf = fern!(Leaf);
    assert_eq!(leaf.kind, "leaf");
}
