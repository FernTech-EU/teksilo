// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §5.6 expression-producing form: `rust { ... expr }` (no trailing
//! `;`) produces a widget value used as a child.

use bastyde::prelude::*;

#[derive(Debug)]
struct Marker {
    tag: &'static str,
}

impl Marker {
    fn new(tag: &'static str) -> Self {
        Self { tag }
    }
}

impl Widget for Marker {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug, Default)]
struct Holder {
    tags: std::cell::RefCell<Vec<&'static str>>,
}

impl Holder {
    fn new() -> Self {
        Self::default()
    }

    fn child(self, m: Marker) -> Self {
        self.tags.borrow_mut().push(m.tag);
        self
    }
}

impl Widget for Holder {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let h: Holder = bati!(
        Holder {
            Marker("head")
            rust {
                let tag = if true { "middle" } else { "other" };
                Marker::new(tag)
            }
            Marker("tail")
        }
    );
    let tags = h.tags.borrow();
    assert_eq!(*tags, vec!["head", "middle", "tail"]);
}
