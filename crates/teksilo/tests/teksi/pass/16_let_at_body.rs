// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §5.4: a `let` at body position introduces a computed value
//! used by subsequent body items. Switches the enclosing element to
//! statement-sequence lowering form.

use teksilo::prelude::*;

#[derive(Debug)]
struct Label {
    text: String,
    color: u32,
}

impl Label {
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

impl Widget for Label {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug, Default)]
struct Stack {
    labels: std::cell::RefCell<Vec<(String, u32)>>,
}

impl Stack {
    fn new() -> Self {
        Self::default()
    }

    fn child(self, l: Label) -> Self {
        self.labels.borrow_mut().push((l.text, l.color));
        self
    }
}

impl Widget for Stack {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let accent: u32 = 0xFF0000;
    let s: Stack = teksu!(
        Stack {
            let prefix = "Hello, ";
            let accent_color = accent;
            Label(format!("{}world", prefix)) {
                color: accent_color
            }
            Label(format!("{}friend", prefix)) {
                color: accent_color
            }
        }
    );
    let labels = s.labels.borrow();
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0], ("Hello, world".to_string(), 0xFF0000));
    assert_eq!(labels[1], ("Hello, friend".to_string(), 0xFF0000));
}
