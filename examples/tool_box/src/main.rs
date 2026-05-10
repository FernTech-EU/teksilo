//! ToolBox example — a vertical palette of exclusive-disclosure sections
//! styled per Int UI.
//!
//! Run with: `cargo run -p tool-box`

use fern_ui::core::widget::WidgetPlacement;
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Badge, Button, ButtonVariant, Card, Expand, HStack, IconWidget, Panel, Spacer, TextWidget,
    ToolBox, ToolBoxItem, Toolbar, VStack,
};

fn dark_mode_toolbar() -> impl Widget {
    let is_dark = Signal::new(false);
    Toolbar::new().child(HStack::new().child(Spacer::new()).child(
        Button::new_literal("Toggle Dark Mode").on_activate_fn(move |ctx| {
            let next = !is_dark.get();
            is_dark.set(next);
            ctx.set_theme(if next {
                fern_ui::presets::intui::dark()
            } else {
                fern_ui::presets::intui::light()
            });
        }),
    ))
}

#[derive(Debug)]
struct Root {
    root_child_id: Option<WidgetId>,
    is_dark: std::rc::Rc<std::cell::Cell<bool>>,
}

impl Root {
    fn new() -> Self {
        Self {
            root_child_id: None,
            is_dark: std::rc::Rc::new(std::cell::Cell::new(false)),
        }
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let selected = ctx.signal(0_usize);

        let outline_panel = Panel::new().padding(12.0).child(
            VStack::new()
                .spacing(6.0)
                .child(TextWidget::new_literal("Chapter 1 — Opening"))
                .child(TextWidget::new_literal("Chapter 2 — Rising Action"))
                .child(TextWidget::new_literal("Chapter 3 — Turning Point"))
                .child(TextWidget::new_literal("Chapter 4 — Climax"))
                .child(TextWidget::new_literal("Chapter 5 — Resolution")),
        );

        let properties_panel = Panel::new().padding(12.0).child(
            VStack::new()
                .spacing(8.0)
                .child(TextWidget::new_literal("Title: Untitled manuscript"))
                .child(TextWidget::new_literal("Word count: 42 318"))
                .child(TextWidget::new_literal("Language: en-US"))
                .child(TextWidget::new_literal("Last modified: today")),
        );

        let references_panel = Panel::new().padding(12.0).child(
            VStack::new()
                .spacing(6.0)
                .child(TextWidget::new_literal("Note: unify the two protagonists"))
                .child(TextWidget::new_literal("Research: Napoleonic uniforms"))
                .child(TextWidget::new_literal("Research: 1810 Paris street plan"))
                .child(TextWidget::new_literal("Link: editor style guide")),
        );

        let build_panel = Panel::new().padding(12.0).child(TextWidget::new_literal(
            "Build tasks appear here during export. Nothing is running — the \
                 section is disabled so it keeps its slot in the palette without \
                 accepting focus.",
        ));

        let toolbox = ToolBox::new(selected.clone())
            .add(
                ToolBoxItem::new_literal("Outline", outline_panel)
                    .leading(IconWidget::chevron_down(14.0))
                    .trailing(Badge::new_literal("5")),
            )
            .item_literal("Properties", properties_panel)
            .add(
                ToolBoxItem::new_literal("References", references_panel)
                    .trailing(Badge::new_literal("12")),
            )
            .add(ToolBoxItem::new_literal("Build tasks", build_panel).enabled(false))
            .show_dividers(false);

        let is_dark = self.is_dark.clone();
        let theme_button = Button::new_literal("Toggle theme")
            .style(ButtonVariant::Flat)
            .on_activate_fn(move |ctx: &mut EventContext| {
                let next_dark = !is_dark.get();
                is_dark.set(next_dark);
                ctx.set_theme(if next_dark {
                    fern_ui::presets::intui::dark()
                } else {
                    fern_ui::presets::intui::light()
                });
            });

        let selected_hint = TextWidget::new_literal("Section index:")
            .bind_text(selected.map(|i| format!("Section index: {}", i)));

        let header_row = HStack::new()
            .spacing(16.0)
            .child(
                TextWidget::new_literal("ToolBox demo")
                    .style(TextStyleRole::BodyBold)
                    .color(TextRole::Primary),
            )
            .child(selected_hint)
            .child(theme_button);

        let instructions = TextWidget::new_literal(
            "Click a section header to expand it, or Tab into the palette and use \
             Up / Down / Home / End to navigate. The Build tasks row is disabled — \
             keyboard focus skips it.",
        )
        .color(TextRole::Secondary);

        let sidebar = Card::new()
            .header(TextWidget::new_literal("Palette").style(TextStyleRole::BodyBold))
            .content(toolbox);

        let content_row =
            HStack::new()
                .spacing(16.0)
                .child(sidebar)
                .child(Panel::new().padding(20.0).child(TextWidget::new_literal(
                    "The ToolBox on the left is a self-contained widget — it plays \
                     the same role as Qt's QToolBox or an IntelliJ settings-group \
                     accordion: exactly one section open at any time.",
                )));

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
    FernAppBuilder::new()
        .install_inspector_in_debug()
        .theme(fern_ui::presets::intui::light())
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
