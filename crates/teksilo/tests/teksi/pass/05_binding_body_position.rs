// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §3.3: a body-position binding `name = Element` hoists to the
//! enclosing teksu! block as `let name = ctx.add(...)` and attaches via
//! `.child(name)` on the parent. The binding's id is in scope for
//! sibling items in the same body and for nested property values.

use teksilo::prelude::*;

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
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
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

    fn child(self, c: impl teksilo_core::IntoTeksiChild) -> Self {
        match teksilo_core::IntoTeksiChild::into_pending(c) {
            teksilo_core::PendingChild::Id(id) => {
        self.child_ids.borrow_mut().push(id);
            }
            // This stub only records ids; an inline widget is ignored,
            // exactly as the generic `child` it replaces did.
            teksilo_core::PendingChild::Deferred(_) => {}
        }
        self
    }


    fn linked_to(self, id: WidgetId) -> Self {
        *self.linked_to.borrow_mut() = Some(id);
        self
    }
}

impl Widget for Stack {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn build(ctx: &mut BuildContext) -> WidgetId {
    teksu!(ctx => Stack {
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
