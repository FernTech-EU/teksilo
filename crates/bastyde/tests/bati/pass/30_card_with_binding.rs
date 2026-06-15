// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §7.9 stop-gate: a Card with a bound slot widget. Exercises
//! body-wide features: Category B slots, slot binding with `_id`
//! routing, element-valued slots, nested element body, and binding
//! reference from inside a closure at a deeper level.

use bastyde::prelude::*;

// Widgets: a minimal Card-like and TextWidget-like with enough API for
// the fixture. Avoids a hard dependency on bastyde-widgets' real Card.

#[derive(Debug)]
struct TextLike {
    text: &'static str,
    style: Option<u32>,
}

impl TextLike {
    fn new(text: &'static str) -> Self {
        Self { text, style: None }
    }

    fn style(mut self, s: u32) -> Self {
        self.style = Some(s);
        self
    }
}

impl Widget for TextLike {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug, Default)]
struct VLike {
    spacing: Option<f32>,
}

impl VLike {
    fn new() -> Self {
        Self::default()
    }

    fn spacing(mut self, s: f32) -> Self {
        self.spacing = Some(s);
        self
    }

    fn child<W: Widget + 'static>(self, _w: W) -> Self {
        self
    }
}

impl Widget for VLike {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Default)]
struct Button {
    label: &'static str,
    on_tap: Option<Box<dyn Fn() -> WidgetId>>,
}

impl std::fmt::Debug for Button {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Button")
            .field("label", &self.label)
            .field("has_on_tap", &self.on_tap.is_some())
            .finish()
    }
}

impl Button {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            on_tap: None,
        }
    }

    fn on_tap<F: Fn() -> WidgetId + 'static>(mut self, f: F) -> Self {
        self.on_tap = Some(Box::new(f));
        self
    }
}

impl Widget for Button {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug, Default)]
struct CardShell {
    header_id: Option<WidgetId>,
    padding: Option<f32>,
}

impl CardShell {
    fn new() -> Self {
        Self::default()
    }

    fn header_id(mut self, id: WidgetId) -> Self {
        self.header_id = Some(id);
        self
    }

    fn content<W: Widget + 'static>(self, _w: W) -> Self {
        self
    }

    fn padding(mut self, p: f32) -> Self {
        self.padding = Some(p);
        self
    }
}

impl Widget for CardShell {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn build(ctx: &mut BuildContext) -> WidgetId {
    bati!(ctx => CardShell {
            header: title = TextLike("Manuscript Title") {
                style: 1
            }
            content: VLike {
                spacing: 12.0
                TextLike("Edit title:")
                Button("Focus title") {
                    on_tap: move || title
                }
            }
            padding: 16.0
        }
    )
}

fn main() {
    let _build: fn(&mut BuildContext) -> WidgetId = build;
}
