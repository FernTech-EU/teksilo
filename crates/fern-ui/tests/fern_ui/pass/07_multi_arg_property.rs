//! Spec §3.4 multi-argument property form: `name: arg1, arg2` desugars
//! to `.name(arg1, arg2)` — the TitleBar `.border(color, width)` pattern.
//! Commas continue the arg list while on the same line as the previous
//! expression; a newline terminates.

use fern_ui::prelude::*;

#[derive(Debug, Default)]
struct Target {
    border_color: Option<u32>,
    border_width: Option<f32>,
    spacing: Option<f32>,
    tag: Option<&'static str>,
}

impl Target {
    fn new() -> Self {
        Self::default()
    }

    fn border(mut self, color: u32, width: f32) -> Self {
        self.border_color = Some(color);
        self.border_width = Some(width);
        self
    }

    fn spacing(mut self, value: f32) -> Self {
        self.spacing = Some(value);
        self
    }

    fn tag(mut self, tag: &'static str) -> Self {
        self.tag = Some(tag);
        self
    }
}

impl Widget for Target {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    // Multi-arg property on the first line; single-arg properties on
    // following lines — the newline between `2.0` and `spacing` should
    // terminate the border arg list.
    let t: Target = fern!(
        Target {
            border: 0xFF0000, 2.0
            spacing: 12.0
            tag: "demo"
        }
    );
    assert_eq!(t.border_color, Some(0xFF0000));
    assert_eq!(t.border_width, Some(2.0));
    assert_eq!(t.spacing, Some(12.0));
    assert_eq!(t.tag, Some("demo"));
}
