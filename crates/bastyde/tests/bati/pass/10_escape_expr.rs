// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §6.1: `#{ expr }` escape. The expression is
//! expected to evaluate to a `WidgetId`. At body position it lowers to
//! `.add_child(expr)`; at slot-value position it forces the `_id`
//! suffix on the property.

use bastyde::prelude::*;

#[derive(Debug)]
struct Leaf;

impl Leaf {
    fn new() -> Self {
        Self
    }
}

impl Widget for Leaf {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug, Default)]
struct Holder {
    child_ids: Vec<WidgetId>,
    header_id: Option<WidgetId>,
}

impl Holder {
    fn new() -> Self {
        Self::default()
    }

    fn add_child(mut self, id: WidgetId) -> Self {
        self.child_ids.push(id);
        self
    }

    fn header_id(mut self, id: WidgetId) -> Self {
        self.header_id = Some(id);
        self
    }
}

impl Widget for Holder {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn build(ctx: &mut BuildContext) -> WidgetId {
    let external = ctx.add(Leaf::new());
    // #{ external } at body position -> .add_child(external)
    // #{ external } at slot position -> .header_id(external)
    bati!(ctx => Holder {
            header: #{ external }
            #{ external }
        }
    )
}

fn main() {
    let _build: fn(&mut BuildContext) -> WidgetId = build;
}
