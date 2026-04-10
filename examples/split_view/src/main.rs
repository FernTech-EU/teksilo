use fern_ui::prelude::*;
use fern_ui::core::WidgetPlacement;
use fern_ui::tokens::Orientation;
use fern_ui::widgets::{Badge, Panel, ScrollArea, SplitView, TextWidget, VStack};

#[derive(Debug)]
struct SplitViewDemo {
    root_child_id: Option<WidgetId>,
}

impl SplitViewDemo {
    fn new() -> Self {
        Self { root_child_id: None }
    }
}

impl Widget for SplitViewDemo {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let t = &theme.typography;
        let c = &theme.colors;

        let horizontal_split = ctx.signal(0.32_f32);
        let vertical_split = ctx.signal(0.55_f32);

        let root = ctx.add(
            ScrollArea::new(
                VStack::new()
                    .spacing(24.0)
                    .child(
                        TextWidget::new("SplitView")
                            .style(t.heading_1.clone())
                            .color(c.on_surface),
                    )
                    .child(
                        TextWidget::new(
                            "Drag the divider or focus it and use arrow keys to resize the panes.",
                        )
                        .style(t.body.clone())
                        .color(c.on_surface_secondary),
                    )
                    .child(
                        Panel::new()
                            .padding(16.0)
                            .child(
                                SplitView::new(horizontal_split)
                                    .min_first_size(180.0)
                                    .min_second_size(220.0)
                                    .first(build_editor_pane("Project", &["src", "crates", "examples", "README.md"]))
                                    .second(build_preview_pane("Preview", "Wide horizontal split for project navigation and live preview.")),
                            ),
                    )
                    .child(
                        Panel::new()
                            .padding(16.0)
                            .child(
                                SplitView::new(vertical_split)
                                    .orientation(Orientation::Vertical)
                                    .min_first_size(120.0)
                                    .min_second_size(140.0)
                                    .first(build_preview_pane("Console", "A vertical split is useful for logs above an inspector or terminal."))
                                    .second(build_editor_pane("Inspector", &["Selection", "Layout", "Accessibility", "Animations"])),
                            ),
                    ),
            )
            .widget_resizable(true),
        );

        self.root_child_id = Some(root);
        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

fn build_editor_pane(title: &str, items: &[&str]) -> impl Widget {
    let mut stack = VStack::new().spacing(10.0).child(TextWidget::new(title));
    for item in items {
        stack = stack.child(Badge::new(*item));
    }
    Panel::new().padding(16.0).child(stack)
}

fn build_preview_pane(title: &str, text: &str) -> impl Widget {
    Panel::new().padding(20.0).child(
        VStack::new()
            .spacing(12.0)
            .child(TextWidget::new(title))
            .child(TextWidget::new(text)),
    )
}

fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("SplitView")
        .window_size(980, 760)
        .root(|tree| tree.add(SplitViewDemo::new()))
        .run();
}