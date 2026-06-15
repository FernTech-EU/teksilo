// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §3.6, §4.1: bare child elements at body position lower to
//! `.child(...)` calls on the parent.

use bastyde::prelude::*;

#[derive(Debug)]
struct Container {
    children: std::cell::RefCell<Vec<String>>,
    spacing: f32,
}

impl Container {
    fn new() -> Self {
        Self {
            children: std::cell::RefCell::new(Vec::new()),
            spacing: 0.0,
        }
    }

    fn spacing(mut self, value: f32) -> Self {
        self.spacing = value;
        self
    }

    fn child(self, tag: Tag) -> Self {
        self.children.borrow_mut().push(tag.name.clone());
        self
    }
}

impl Widget for Container {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug)]
struct Tag {
    name: String,
}

impl Tag {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl Widget for Tag {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let c: Container = bati!(
        Container {
            spacing: 12.0
            Tag("first")
            Tag("second")
            Tag("third")
        }
    );
    assert_eq!(c.spacing, 12.0);
    assert_eq!(
        &*c.children.borrow(),
        &["first".to_string(), "second".to_string(), "third".to_string()]
    );
}
