// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Minimal widgets for the arbitration integration tests.
//!
//! `teksilo_core::test_widgets` is `pub(crate)`, so an external test crate
//! cannot reach `FillWidget`/`StackWidget`. Rather than widen that module's
//! visibility — a seam reachable only by widening visibility is the wrong
//! seam — the two shapes the arbitration fixtures need are restated here in
//! forty lines against the public `Widget` trait. They are deliberately the
//! *same* shapes: a leaf that fills its proposal and a container that stacks
//! its children at the same origin, so the fixture geometry is a rectangle
//! every competitor shares and a press lands on all of them at once.

#![allow(dead_code)]

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;

/// A leaf that fills whatever it is proposed. The innermost node of every
/// fixture: what the pointer actually lands on.
#[derive(Debug, Default)]
pub struct Leaf;

impl Leaf {
    pub fn new() -> Self {
        Self
    }
}

impl Widget for Leaf {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

/// A container that places every child over its own whole bounds.
///
/// Co-located children are the point: an arbitration fixture wants a press to
/// hit the leaf *and* be on the ancestor's path, with no geometry standing
/// between them to explain a result away.
#[derive(Debug, Default)]
pub struct Stack {
    children: Vec<WidgetId>,
}

impl Stack {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    /// The container form every catalog widget now has: it takes an id or a
    /// widget value through `IntoTeksiChild`.
    pub fn child(mut self, c: impl teksilo_core::IntoTeksiChild) -> Self {
        match teksilo_core::IntoTeksiChild::into_pending(c) {
            teksilo_core::PendingChild::Id(id) => self.children.push(id),
            teksilo_core::PendingChild::Deferred(_) => unreachable!("Stack takes ids"),
        }
        self
    }
}

impl Widget for Stack {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.children.clone()
    }
}
