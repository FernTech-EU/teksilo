//! TabWidget example.
//!
//! Run with: `cargo run -p tab-widget`

use fern_ui::core::widget::WidgetPlacement;
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Badge, Breadcrumb, BreadcrumbItem, Button, ButtonVariant, Card, HStack, Panel, TabItem,
    TabWidget, TextWidget, VStack,
};

#[derive(Debug, Clone, PartialEq)]
enum Cmd {
    ToggleTheme,
    OpenLibrary,
    OpenComponents,
}

impl AppCommand for Cmd {}

#[derive(Debug)]
struct Root {
    root_child_id: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        Self {
            root_child_id: None,
        }
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let selected = ctx.signal(0_usize);
        let selected_label = selected.map(|index| match *index {
            0 => "Overview".to_string(),
            1 => "Inspector".to_string(),
            _ => "Activity".to_string(),
        });

        let trailing = HStack::new()
            .spacing(12.0)
            .child(
                TextWidget::new_literal("")
                    .bind_text(selected_label)
                    .style(theme.typography.small.clone()),
            )
            .child(
                Button::new_literal("Toggle Theme")
                    .style(ButtonVariant::Flat)
                    .on_activate(Cmd::ToggleTheme),
            );

        let tabs = ctx.add(
            TabWidget::new(selected)
                .tab_literal(
                    "Overview",
                    Card::new()
                        .header(
                            TextWidget::new_literal("Overview")
                                .style(theme.typography.body_bold.clone())
                                .color(theme.colors.text_primary),
                        )
                        .content(
                            VStack::new()
                                .spacing(12.0)
                                .child(
                                    TextWidget::new_literal(
                                        "This first Milestone 6 slice ships a real TabWidget with dormant panes, keyboard navigation, and a trailing action slot.",
                                    )
                                    .style(theme.typography.body.clone())
                                    .color(theme.colors.text_primary),
                                )
                                .child(
                                    HStack::new()
                                        .spacing(8.0)
                                        .child(Badge::new_literal("Dormant Panes"))
                                        .child(Badge::new_literal("Arrow Navigation"))
                                        .child(Badge::new_literal("Trailing Slot")),
                                ),
                        ),
                )
                .tab_literal(
                    "Inspector",
                    Panel::new().padding(20.0).child(
                        VStack::new()
                            .spacing(10.0)
                            .child(
                                TextWidget::new_literal("Inspector")
                                    .style(theme.typography.body_bold.clone())
                                    .color(theme.colors.text_primary),
                            )
                            .child(
                                TextWidget::new_literal(
                                    "Use Tab to move focus into the tab strip, then Arrow Left and Arrow Right to switch tabs from the keyboard.",
                                )
                                .style(theme.typography.body.clone())
                                .color(theme.colors.text_primary),
                            ),
                    ),
                )
                .tab_literal(
                    "Activity",
                    Panel::new().padding(20.0).child(
                        VStack::new()
                            .spacing(10.0)
                            .child(
                                TextWidget::new_literal("Activity")
                                    .style(theme.typography.body_bold.clone())
                                    .color(theme.colors.text_primary),
                            )
                            .child(
                                TextWidget::new_literal(
                                    "This example will grow into the broader Milestone 6 showcase as SplitView, Dialog, Popover, and Snackbar land.",
                                )
                                .style(theme.typography.body.clone())
                                .color(theme.colors.text_primary),
                            ),
                    ),
                )
                .tab_item(
                    TabItem::new_literal(
                        "Disabled",
                        Panel::new().padding(20.0).child(
                            TextWidget::new_literal("Disabled tabs are visible but cannot be activated.")
                                .style(theme.typography.body.clone())
                                .color(theme.colors.text_primary),
                        ),
                    )
                    .enabled(false),
                )
                .trailing_slot(trailing),
        );

        let breadcrumb = ctx.add(
            Breadcrumb::new()
                .item(BreadcrumbItem::new_literal("Library").on_activate(Cmd::OpenLibrary))
                .item(BreadcrumbItem::new_literal("Components").on_activate(Cmd::OpenComponents))
                .item(BreadcrumbItem::current_literal("TabWidget")),
        );

        let root_id = ctx.add(
            Panel::new().padding(24.0).child(
                VStack::new()
                    .spacing(16.0)
                    .add_child(breadcrumb)
                    .child(
                        TextWidget::new_literal("TabWidget")
                            .style(theme.typography.body_bold.clone())
                            .color(theme.colors.text_primary),
                    )
                    .child(
                        TextWidget::new_literal(
                            "A focused Milestone 6 example for the first implementation slice.",
                        )
                        .style(theme.typography.body.clone())
                        .color(theme.colors.text_primary),
                    )
                    .add_child(tabs),
            ),
        );

        self.root_child_id = Some(root_id);
        vec![root_id]
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

fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("TabWidget")
        .window_size(960, 640)
        .on_command(|cmd: &Cmd, ctx| match cmd {
            Cmd::ToggleTheme => {
                let next = if ctx.theme().colors.surface_main == Theme::light_default().colors.surface_main {
                    Theme::dark_default()
                } else {
                    Theme::light_default()
                };
                ctx.set_theme(next);
            }
            Cmd::OpenLibrary | Cmd::OpenComponents => {}
        })
        .root(|tree| tree.add(Root::new()))
        .run();
}
