// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Body items can be comma-separated on a single line.
//!
//! `Panel { padding: 8.0, color: RED }` parses as two properties, the
//! comma acting as an optional separator equivalent to a newline.
//! Multi-arg properties are still disambiguated: `border: col, width`
//! continues consuming because `width` doesn't look like a new body
//! item.

use teksilo::prelude::*;

#[derive(Debug, Default)]
struct Shape {
    padding: Option<f32>,
    color: Option<u32>,
    border_color: Option<u32>,
    border_width: Option<f32>,
}

impl Shape {
    fn new() -> Self {
        Self::default()
    }

    fn padding(mut self, p: f32) -> Self {
        self.padding = Some(p);
        self
    }

    fn color(mut self, c: u32) -> Self {
        self.color = Some(c);
        self
    }

    fn border(mut self, color: u32, width: f32) -> Self {
        self.border_color = Some(color);
        self.border_width = Some(width);
        self
    }
}

impl Widget for Shape {
    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        p.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let s: Shape = teksu!(
        Shape {
            padding: 8.0
            color: 0xFF0000
            border: 0x00FF00, 2.0
        }
    );
    assert_eq!(s.padding, Some(8.0));
    assert_eq!(s.color, Some(0xFF0000));
    assert_eq!(s.border_color, Some(0x00FF00));
    assert_eq!(s.border_width, Some(2.0));
}
