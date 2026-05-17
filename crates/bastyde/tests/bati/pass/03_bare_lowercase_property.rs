//! Spec §3.4 bare-lowercase-identifier rule: a body item consisting of a
//! single lowercase ident is an argument-free method call. Typical use:
//! `Expand { fills_stack }`.

use bastyde::prelude::*;

#[derive(Debug, Default)]
struct Marker {
    fills_stack: bool,
    ready: bool,
}

impl Marker {
    fn new() -> Self {
        Self::default()
    }

    fn fills_stack(mut self) -> Self {
        self.fills_stack = true;
        self
    }

    fn ready(mut self) -> Self {
        self.ready = true;
        self
    }
}

impl Widget for Marker {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let m: Marker = bati!(
        Marker {
            fills_stack
            ready
        }
    );
    assert!(m.fills_stack);
    assert!(m.ready);
}
