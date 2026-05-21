//! Spec §3.2, §3.4: element with explicit constructor, positional arg,
//! and a body of single-argument properties.
//!
//! Mirrors the §7.1 simple-button worked translation without depending
//! on the real Button widget — keeps the fixture self-contained.

use bastyde::prelude::*;

#[derive(Debug)]
struct Probe {
    label: String,
    style: Option<u32>,
    tag: Option<&'static str>,
}

impl Probe {
    fn new_literal(label: &str) -> Self {
        Self {
            label: label.to_string(),
            style: None,
            tag: None,
        }
    }

    fn style(mut self, value: u32) -> Self {
        self.style = Some(value);
        self
    }

    fn tag(mut self, tag: &'static str) -> Self {
        self.tag = Some(tag);
        self
    }
}

impl Widget for Probe {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let w: Probe = bati!(
        Probe::new(lit!("Click Me")) {
            style: 42
            tag: "demo"
        }
    );
    assert_eq!(w.label, "Click Me");
    assert_eq!(w.style, Some(42));
    assert_eq!(w.tag, Some("demo"));
}
