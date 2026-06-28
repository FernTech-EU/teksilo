// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! ToolBox example — a vertical palette of exclusive-disclosure sections
//! styled per Int UI.
//!
//! Run with: `cargo run -p tool-box`

use bastyde::core::widget::WidgetPlacement;
use bastyde::prelude::*;
use bastyde::widgets::{
    Badge, Card, Expand, HStack, IconWidget, Panel, Spacer, TextWidget, ToolBox, ToolBoxItem,
    Toolbar, VStack,
};

fn dark_mode_toolbar() -> impl Widget {
    Toolbar::new().child(
        HStack::new()
            .child(Spacer::new())
            .child(bastyde::widgets::ThemeSwitcher::new()),
    )
}

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
        let selected = ctx.signal(0_usize);

        let outline_panel = Panel::new().padding(12.0).child(
            VStack::new()
                .spacing(6.0)
                .child(TextWidget::new(lit!("Chapter 1 — Opening")))
                .child(TextWidget::new(lit!("Chapter 2 — Rising Action")))
                .child(TextWidget::new(lit!("Chapter 3 — Turning Point")))
                .child(TextWidget::new(lit!("Chapter 4 — Climax")))
                .child(TextWidget::new(lit!("Chapter 5 — Resolution"))),
        );

        let properties_panel = Panel::new().padding(12.0).child(
            VStack::new()
                .spacing(8.0)
                .child(TextWidget::new(lit!("Title: Untitled manuscript")))
                .child(TextWidget::new(lit!("Word count: 42 318")))
                .child(TextWidget::new(lit!("Language: en-US")))
                .child(TextWidget::new(lit!("Last modified: today"))),
        );

        let references_panel = Panel::new().padding(12.0).child(
            VStack::new()
                .spacing(6.0)
                .child(TextWidget::new(lit!("Note: unify the two protagonists")))
                .child(TextWidget::new(lit!("Research: Napoleonic uniforms")))
                .child(TextWidget::new(lit!("Research: 1810 Paris street plan")))
                .child(TextWidget::new(lit!("Link: editor style guide"))),
        );

        let build_panel = Panel::new().padding(12.0).child(TextWidget::new(lit!(
            "Build tasks appear here during export. Nothing is running — the \
                 section is disabled so it keeps its slot in the palette without \
                 accepting focus."
        )));

        let toolbox = ToolBox::new(selected.clone())
            .add(
                ToolBoxItem::new(lit!("Outline"), outline_panel)
                    .leading(IconWidget::chevron_down(14.0))
                    .trailing(Badge::new(lit!("5"))),
            )
            .item(lit!("Properties"), properties_panel)
            .add(
                ToolBoxItem::new(lit!("References"), references_panel)
                    .trailing(Badge::new(lit!("12"))),
            )
            .add(ToolBoxItem::new(lit!("Build tasks"), build_panel).enabled(false))
            .show_dividers(false);

        let selected_hint = TextWidget::new(lit!("Section index:"))
            .bind_text(selected.map(|i| format!("Section index: {}", i)));

        let header_row = HStack::new()
            .spacing(16.0)
            .child(
                TextWidget::new(lit!("ToolBox demo"))
                    .style(TextStyleRole::BodyBold)
                    .color(TextRole::Primary),
            )
            .child(selected_hint);

        let instructions = TextWidget::new(lit!(
            "Click a section header to expand it, or Tab into the palette and use \
             Up / Down / Home / End to navigate. The Build tasks row is disabled — \
             keyboard focus skips it."
        ))
        .color(TextRole::Secondary);

        let sidebar = Card::new()
            .header(TextWidget::new(lit!("Palette")).style(TextStyleRole::BodyBold))
            .content(toolbox);

        let content_row =
            HStack::new()
                .spacing(16.0)
                .child(sidebar)
                .child(Panel::new().padding(20.0).child(TextWidget::new(lit!(
                    "The ToolBox on the left is a self-contained widget — it plays \
                     the same role as Qt's QToolBox or an IntelliJ settings-group \
                     accordion: exactly one section open at any time."
                ))));

        let root_id = ctx.add(
            Panel::new().padding(24.0).child(
                VStack::new()
                    .spacing(16.0)
                    .child(header_row)
                    .child(instructions)
                    .child(content_row),
            ),
        );

        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
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
    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("ToolBox")
                .size(840, 560)
                .root(|tree, _state| {
                    tree.add(
                        VStack::new()
                            .child(dark_mode_toolbar())
                            .child(Expand::new().child(Root::new())),
                    )
                }),
        )
        .run();
}
