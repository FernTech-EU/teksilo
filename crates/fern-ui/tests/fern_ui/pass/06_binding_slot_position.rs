//! Spec §3.3 + §A.3: a binding at a Category B slot position hoists
//! the binding and routes the slot to the `_id` twin.
//! `header: title = Widget { ... }` desugars to:
//!   let title = ctx.add(Widget::new()...);
//!   parent.header_id(title);

use fern_ui::prelude::*;

#[derive(Debug)]
struct TextLike {
    text: &'static str,
}

impl TextLike {
    fn new(text: &'static str) -> Self {
        Self { text }
    }
}

impl Widget for TextLike {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        proposal.resolve(0.0, 0.0)
    }
}

#[derive(Debug, Default)]
struct CardWithIds {
    header_id: Option<WidgetId>,
    content_text: Option<String>,
}

impl CardWithIds {
    fn new() -> Self {
        Self::default()
    }

    fn header_id(mut self, id: WidgetId) -> Self {
        self.header_id = Some(id);
        self
    }

    fn content(mut self, c: TextLike) -> Self {
        self.content_text = Some(c.text.to_string());
        self
    }
}

impl Widget for CardWithIds {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        proposal.resolve(0.0, 0.0)
    }
}

fn build(ctx: &mut BuildContext) -> WidgetId {
    fern!(ctx =>
        CardWithIds {
            header: title = TextLike("Manuscript")
            content: TextLike("body")
        }
    )
}

fn main() {
    // trybuild only needs successful compilation; building the widget
    // would require a running BuildContext with a real arena.
    let _build: fn(&mut BuildContext) -> WidgetId = build;
}
