// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §2: the `teksu!(ctx => ...)` preamble wraps the root in
//! `ctx.add(...)` and returns a `WidgetId`.

use teksilo::prelude::*;

#[derive(Debug)]
struct Leaf;

impl Leaf {
    fn new() -> Self {
        Leaf
    }
}

impl Widget for Leaf {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let mut tree = teksilo::core::WidgetTree::new();
    let id: WidgetId = teksu!(tree => Leaf);
    // The id is a real arena entry the tree knows about.
    let _children = tree.children(id);
}
