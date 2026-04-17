//! Spec §5.5: `..expr` inlines an iterator of WidgetIds as children,
//! using statement-sequence lowering.

use fern_ui::prelude::*;

#[derive(Debug)]
struct Leaf;

impl Leaf {
    fn new() -> Self {
        Self
    }
}

impl Widget for Leaf {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        proposal.resolve(0.0, 0.0)
    }
}

#[derive(Debug, Default)]
struct Stack {
    ids: Vec<WidgetId>,
}

impl Stack {
    fn new() -> Self {
        Self::default()
    }

    fn add_child(mut self, id: WidgetId) -> Self {
        self.ids.push(id);
        self
    }

    fn child<W: Widget + 'static>(self, _w: W) -> Self {
        self
    }
}

impl Widget for Stack {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        proposal.resolve(0.0, 0.0)
    }
}

fn build(ctx: &mut BuildContext) -> WidgetId {
    let plugin_widgets: Vec<WidgetId> = (0..3).map(|_| ctx.add(Leaf)).collect();
    fern!(ctx =>
        Stack {
            Leaf
            ..plugin_widgets
            Leaf
        }
    )
}

fn main() {
    let _build: fn(&mut BuildContext) -> WidgetId = build;
}
