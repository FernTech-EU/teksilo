use std::time::Duration;

use fern_ui::core::WidgetPlacement;
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Badge, Button, ButtonVariant, Dialog, DialogContent, HStack, Panel, Popover, ScrollArea,
    Snackbar, TextWidget, VStack,
};

#[derive(Debug)]
struct OverlayDemo {
    root_child_id: Option<WidgetId>,
}

impl OverlayDemo {
    fn new() -> Self {
        Self {
            root_child_id: None,
        }
    }
}

impl Widget for OverlayDemo {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme_signal().get();

        let popover_content = VStack::new()
            .spacing(12.0)
            .child(
                TextWidget::new_literal("Popover")
                    .style(TextStyleRole::Small)
                    .color(TextRole::Primary),
            )
            .child(
                TextWidget::new_literal(
                    "Use popovers for compact contextual actions without leaving the current surface.",
                )
                .style(TextStyleRole::Body)
                .color(TextRole::Secondary),
            )
            .child(
                HStack::new()
                    .spacing(8.0)
                    .child(Badge::new_literal("Quick actions"))
                    .child(Badge::new_literal("Inline help"))
                    .child(Badge::new_literal("Inspector")),
            );

        let snackbar_content = HStack::new()
            .spacing(14.0)
            .child(
                TextWidget::new_literal("Autosave complete")
                    .style(TextStyleRole::Body)
                    .color(TextRole::TooltipText),
            )
            .child(
                Button::new_literal("Dismiss")
                    .style(ButtonVariant::Regular)
                    .on_tap(|_, ctx| ctx.dismiss_top_overlay()),
            );

        let popover_trigger = Panel::new().padding(12.0).child(
            HStack::new()
                .spacing(10.0)
                .child(Badge::new_literal("Context"))
                .child(
                    TextWidget::new_literal("Popover actions")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Primary),
                ),
        );

        let dialog_trigger = Panel::new().padding(12.0).child(
            HStack::new()
                .spacing(10.0)
                .child(Badge::new_literal("Modal"))
                .child(
                    TextWidget::new_literal("Review changes")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Primary),
                ),
        );
        let modal_trigger_id = ctx.add(
            Dialog::new_literal("Adaptive modal window")
                .content(move || {
                    DialogContent::new()
                        .title_literal("Adaptive modal dialog")
                        .supporting_text_literal(
                            "The framework chooses the best modal presentation for the current backend: a native modal child window when reliable, otherwise a centered in-tree dialog.",
                        )
                        .body(
                            TextWidget::new_literal(
                                "The app code does not branch on Wayland or window-system support here; it issues one modal request and lets FernUI resolve it.",
                            )
                            .style(TextStyleRole::Body)
                            .color(TextRole::Secondary),
                        )
                        .footer(
                            Button::new_literal("Close")
                                .style(ButtonVariant::Default)
                                .on_tap(|_, ctx| ctx.dismiss_modal()),
                        )
                })
                .style(ButtonVariant::Regular),
        );

        let root = ctx.add(
            ScrollArea::new().child(
                VStack::new()
                    .spacing(24.0)
                    .child(
                        TextWidget::new_literal("Dialogs and Popovers")
                            .style(TextStyleRole::BodyBold)
                            .color(TextRole::Primary),
                    )
                    .child(
                        TextWidget::new_literal(
                            "FernUI now resolves dialogs through a shared modal presentation pipeline, alongside anchored popovers and timed snackbars.",
                        )
                        .style(TextStyleRole::Body)
                        .color(TextRole::Secondary),
                    )
                    .child(
                        Panel::new().padding(20.0).child(
                            HStack::new()
                                .spacing(16.0)
                                .child(
                                    Popover::new_literal("Show popover")
                                        .content(popover_content)
                                        .caret_size(12.0)
                                        .trigger(popover_trigger),
                                )
                                .child(
                                    Dialog::new_literal("Open dialog")
                                        .content(move || {
                                            DialogContent::new()
                                                .title_literal("Review Changes")
                                                .supporting_text_literal(
                                                    "Dialogs open centered, dismiss on Escape or outside click, and can host structured body and action sections.",
                                                )
                                                .body(
                                                    TextWidget::new_literal(
                                                        "This helper gives dialogs a consistent header, content spacing, and footer separation without forcing a single action-row layout.",
                                                    )
                                                    .style(TextStyleRole::Body)
                                                    .color(TextRole::Secondary),
                                                )
                                                .footer(
                                                    HStack::new()
                                                        .spacing(12.0)
                                                        .child(
                                                            Button::new_literal("Cancel")
                                                                .style(ButtonVariant::Regular)
                                                                .on_tap(|_, ctx| ctx.dismiss_modal()),
                                                        )
                                                        .child(
                                                            Button::new_literal("Apply")
                                                                .style(ButtonVariant::Default)
                                                                .on_tap(|_, ctx| ctx.dismiss_modal()),
                                                        ),
                                                )
                                        })
                                        .trigger(dialog_trigger),
                                )
                                .child(
                                    Snackbar::new_literal("Show snackbar")
                                        .content(snackbar_content)
                                        .auto_dismiss_after(Duration::from_millis(2500)),
                                )
                                .add_child(modal_trigger_id),
                        ),
                    )
                    .child(
                        Panel::new().padding(20.0).child(
                            VStack::new()
                                .spacing(10.0)
                                .child(
                                    TextWidget::new_literal("Notes")
                                        .style(TextStyleRole::BodyBold)
                                        .color(TextRole::Primary),
                                )
                                .child(
                                    TextWidget::new_literal(
                                        "Dialogs now share one modal request API. Footer actions can dismiss the current modal without knowing whether FernUI resolved it to an in-tree overlay or a native modal child window.",
                                    )
                                    .style(TextStyleRole::Body)
                                    .color(TextRole::Secondary),
                                ),
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

fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("Dialogs and Popovers")
        .window_size(980, 720)
        .root(|tree| tree.add(OverlayDemo::new()))
        .run();
}
