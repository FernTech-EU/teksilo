// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §6.1: `#{ expr }` escape. The expression is
//! expected to evaluate to a `WidgetId`. At body position it lowers to
//! `.add_child(expr)`; at slot-value position it forces the `_id`
//! suffix on the property.

use teksilo::prelude::*;

#[derive(Debug)]
struct Leaf;

impl Leaf {
    fn new() -> Self {
        Self
    }
}

impl Widget for Leaf {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
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

    /// The body-position escape lowers to `child`, which every container in
    /// the catalog takes as `impl IntoTeksiChild`, so one method carries a
    /// `WidgetId` and a widget value alike.
    fn child(mut self, c: impl teksilo_core::IntoTeksiChild) -> Self {
        match c.into_pending() {
            teksilo_core::PendingChild::Id(id) => self.child_ids.push(id),
            teksilo_core::PendingChild::Deferred(_) => unreachable!("fixture passes ids"),
        }
        self
    }

    fn header(mut self, c: impl teksilo_core::IntoTeksiChild) -> Self {
        match teksilo_core::IntoTeksiChild::into_pending(c) {
            teksilo_core::PendingChild::Id(id) => {
        self.header_id = Some(id);
            }
            teksilo_core::PendingChild::Deferred(_) => unreachable!("this fixture passes ids"),
        }
        self
    }
}

impl Widget for Holder {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn build(ctx: &mut BuildContext) -> WidgetId {
    let external = ctx.add(Leaf::new());
    // #{ external } at body position -> .child(external)
    // #{ external } at slot position -> .header_id(external)
    teksu!(ctx => Holder {
            header: #{ external }
            #{ external }
        }
    )
}

fn main() {
    let _build: fn(&mut BuildContext) -> WidgetId = build;
}
