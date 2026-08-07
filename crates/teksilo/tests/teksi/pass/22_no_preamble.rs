// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §2: without `ctx =>`, `teksu!(...)` returns a widget value
//! suitable for passing into `.child(...)` or storing in a `let`.

use teksilo::prelude::*;

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
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let leaf: Leaf = teksu!(Leaf);
    assert_eq!(leaf.kind, "leaf");
}
