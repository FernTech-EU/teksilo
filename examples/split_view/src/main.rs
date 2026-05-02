use fern_ui::core::{Signal, WidgetPlacement};
use fern_ui::prelude::*;
use fern_ui::tokens::Orientation;
use fern_ui::widgets::{Badge, FixedSize, Panel, ScrollArea, SplitView, TextWidget, VStack};

#[derive(Debug)]
struct SplitViewDemo {
    horizontal_split: Signal<f32>,
    vertical_split: Signal<f32>,
    root_child_id: Option<WidgetId>,
}

impl SplitViewDemo {
    fn new() -> Self {
        Self {
            horizontal_split: Signal::new(0.32_f32),
            vertical_split: Signal::new(0.55_f32),
            root_child_id: None,
        }
    }
}

impl Widget for SplitViewDemo {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme_signal().get();

        let root = ctx.add(
            ScrollArea::new().child(
                VStack::new()
                    .spacing(24.0)
                    .child(
                        TextWidget::new_literal("SplitView")
                            .style(TextStyleRole::BodyBold)
                            .color(TextRole::Primary),
                    )
                    .child(
                        TextWidget::new_literal(
                            "Drag the divider or focus it and use arrow keys to resize the panes.",
                        )
                        .style(TextStyleRole::Body)
                        .color(TextRole::Secondary),
                    )
                    .child(Panel::new().padding(16.0).child(
                        SplitView::new(self.horizontal_split.clone())
                            .min_first_size(180.0)
                            .min_second_size(220.0)
                            .first(build_editor_pane(
                                "Project",
                                &["src", "crates", "examples", "README.md"],
                                &theme,
                            ))
                            .second(build_preview_pane(
                                "Preview",
                                "Wide horizontal split for project navigation and live preview.",
                                &theme,
                            )),
                    ))
                    .child(
                        Panel::new().padding(16.0).child(
                            FixedSize::new().bind_height(360.0_f32).child(
                                SplitView::new(self.vertical_split.clone())
                                    .orientation(Orientation::Vertical)
                                    .min_first_size(120.0)
                                    .min_second_size(140.0)
                                    .first(build_preview_pane(
                                        "Console",
                                        "A vertical split is useful for logs above an inspector or terminal.",
                                        &theme,
                                    ))
                                    .second(build_editor_pane(
                                        "Inspector",
                                        &["Selection", "Layout", "Accessibility", "Animations"],
                                        &theme,
                                    )),
                            ),
                        ),
                    ),
            )
            .widget_resizable(true),
        );

        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0)).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        debug_assert_eq!(children.len(), 1);
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

fn build_editor_pane(title: &str, items: &[&str], theme: &Theme) -> impl Widget {
    let mut stack = VStack::new().spacing(10.0).child(
        TextWidget::new_literal(title)
            .style(theme.typography.body_bold.clone())
            .color(theme.colors.text_primary),
    );
    for item in items {
        stack = stack.child(Badge::new_literal(*item));
    }
    Panel::new().padding(16.0).child(stack)
}

fn build_preview_pane(title: &str, text: &str, theme: &Theme) -> impl Widget {
    Panel::new().padding(20.0).child(
        VStack::new()
            .spacing(12.0)
            .child(
                TextWidget::new_literal(title)
                    .style(theme.typography.body_bold.clone())
                    .color(theme.colors.text_primary),
            )
            .child(
                TextWidget::new_literal(text)
                    .style(theme.typography.body.clone())
                    .color(theme.colors.text_secondary),
            ),
    )
}

fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .initial_window(
            WindowConfig::new()
            .title("SplitView")
            .size(980, 760)
            .root(|tree, _state| tree.add(SplitViewDemo::new()))
        )
        .run();
}
