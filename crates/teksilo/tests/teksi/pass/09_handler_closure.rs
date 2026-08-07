// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §3.5: handler attachment is just a property whose value is a
//! closure. The macro does not modify closure syntax — `move`, capture
//! semantics, and arity stay as the user wrote them.

use teksilo::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

struct Triggerable {
    handler: Option<Box<dyn Fn() -> u32>>,
}

impl std::fmt::Debug for Triggerable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Triggerable")
            .field("has_handler", &self.handler.is_some())
            .finish()
    }
}

impl Triggerable {
    fn new() -> Self {
        Self { handler: None }
    }

    fn on_tap(mut self, f: impl Fn() -> u32 + 'static) -> Self {
        self.handler = Some(Box::new(f));
        self
    }
}

impl Widget for Triggerable {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let counter = Rc::new(Cell::new(0_u32));
    let c = counter.clone();
    let t: Triggerable = teksu!(
        Triggerable {
            on_tap: move || {
                c.set(c.get() + 1);
                c.get()
            }
        }
    );
    let handler = t.handler.expect("on_tap was attached");
    assert_eq!(handler(), 1);
    assert_eq!(handler(), 2);
    assert_eq!(counter.get(), 2);
}
