// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §5.5: `..expr` inlines an iterator of WidgetIds as children,
//! using statement-sequence lowering.

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
struct Stack {
    ids: Vec<WidgetId>,
}

impl Stack {
    fn new() -> Self {
        Self::default()
    }

    fn child(mut self, c: impl teksilo_core::IntoTeksiChild) -> Self {
        match teksilo_core::IntoTeksiChild::into_pending(c) {
            teksilo_core::PendingChild::Id(id) => {
        self.ids.push(id);
            }
            // This stub only records ids; an inline widget is ignored,
            // exactly as the generic `child` it replaces did.
            teksilo_core::PendingChild::Deferred(_) => {}
        }
        self
    }

}

impl Widget for Stack {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn build(ctx: &mut BuildContext) -> WidgetId {
    let plugin_widgets: Vec<WidgetId> = (0..3).map(|_| ctx.add(Leaf)).collect();
    teksu!(ctx => Stack {
            Leaf
            ..plugin_widgets
            Leaf
        }
    )
}

fn main() {
    let _build: fn(&mut BuildContext) -> WidgetId = build;
}
