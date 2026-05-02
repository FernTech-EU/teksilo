//! Spec §7.7 stop-gate: the internationalization translation mixes
//! declarative elements, `let` bindings at body position, and a
//! side-effect `rust { ... }` block. Exercises Phase 3a's
//! statement-sequence lowering.

use fern_ui::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

#[derive(Debug)]
struct Text {
    text: String,
    color: u32,
}

impl Text {
    fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: 0,
        }
    }

    fn color(mut self, c: u32) -> Self {
        self.color = c;
        self
    }
}

impl Widget for Text {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug, Default)]
struct Panel {
    lines: std::cell::RefCell<Vec<(String, u32)>>,
    padding: Option<f32>,
}

impl Panel {
    fn new() -> Self {
        Self::default()
    }

    fn padding(mut self, p: f32) -> Self {
        self.padding = Some(p);
        self
    }

    fn child(self, t: Text) -> Self {
        self.lines.borrow_mut().push((t.text, t.color));
        self
    }
}

impl Widget for Panel {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let direction_changed = Rc::new(Cell::new(0_u32));
    let probe = direction_changed.clone();
    let accent: u32 = 0x336699;

    let p: Panel = fern!(
        Panel {
            padding: 24.0

            let heading_color = accent;
            let body_color = accent >> 1;

            rust {
                    // Simulates a ctx.effect registration from §7.7.
                    probe.set(probe.get() + 1);
                }

            Text("Heading") {
                color: heading_color
            }
            Text("Body paragraph") {
                color: body_color
            }
            Text("Trailing") {
                color: body_color
            }
        }
    );

    assert_eq!(direction_changed.get(), 1);
    assert_eq!(p.padding, Some(24.0));
    let lines = p.lines.borrow();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], ("Heading".to_string(), 0x336699));
    assert_eq!(lines[1], ("Body paragraph".to_string(), 0x336699 >> 1));
    assert_eq!(lines[2], ("Trailing".to_string(), 0x336699 >> 1));
}
