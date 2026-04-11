use std::time::Duration;

use fern_ui::core::WidgetPlacement;
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Badge, Button, ButtonStyle, Dialog, DialogContent, HStack, Panel, Popover, ScrollArea,
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
        let theme = ctx.theme().clone();
        let t = &theme.typography;
        let c = &theme.colors;

        let dialog_theme = theme.clone();

        let popover_content = VStack::new()
            .spacing(12.0)
            .child(
                TextWidget::new("Popover")
                    .style(t.label.clone())
                    .color(c.on_surface),
            )
            .child(
                TextWidget::new(
                    "Use popovers for compact contextual actions without leaving the current surface.",
                )
                .style(t.body.clone())
                .color(c.on_surface_secondary),
            )
            .child(
                HStack::new()
                    .spacing(8.0)
                    .child(Badge::new("Quick actions"))
                    .child(Badge::new("Inline help"))
                    .child(Badge::new("Inspector")),
            );

        let snackbar_content = HStack::new()
            .spacing(14.0)
            .child(
                TextWidget::new("Autosave complete")
                    .style(t.body.clone())
                    .color(c.tooltip_text),
            )
            .child(
                Button::new("Dismiss")
                    .style(ButtonStyle::Outlined)
                    .on_tap(|ctx| ctx.dismiss_top_overlay()),
            );

        let popover_trigger = Panel::new().padding(12.0).child(
            HStack::new()
                .spacing(10.0)
                .child(Badge::new("Context"))
                .child(
                    TextWidget::new("Popover actions")
                        .style(t.label.clone())
                        .color(c.on_surface),
                ),
        );

        let dialog_trigger = Panel::new().padding(12.0).child(
            HStack::new()
                .spacing(10.0)
                .child(Badge::new("Modal"))
                .child(
                    TextWidget::new("Review changes")
                        .style(t.label.clone())
                        .color(c.on_surface),
                ),
        );

        let auto_modal_theme = theme.clone();
        let modal_trigger_id = ctx.add(
            Dialog::new("Adaptive modal window", move || {
                let t = &auto_modal_theme.typography;
                let c = &auto_modal_theme.colors;
                DialogContent::new()
                    .title("Adaptive modal dialog")
                    .supporting_text(
                        "The framework chooses the best modal presentation for the current backend: a native modal child window when reliable, otherwise a centered in-tree dialog.",
                    )
                    .body(
                        TextWidget::new(
                            "The app code does not branch on Wayland or window-system support here; it issues one modal request and lets FernUI resolve it.",
                        )
                        .style(t.body.clone())
                        .color(c.on_surface_secondary),
                    )
                    .footer(
                        Button::new("Close")
                            .style(ButtonStyle::Filled)
                            .on_tap(|ctx| ctx.dismiss_modal()),
                    )
            })
            .style(ButtonStyle::Tonal),
        );

        let root = ctx.add(
            ScrollArea::new(
                VStack::new()
                    .spacing(24.0)
                    .child(
                        TextWidget::new("Dialogs and Popovers")
                            .style(t.heading_1.clone())
                            .color(c.on_surface),
                    )
                    .child(
                        TextWidget::new(
                            "FernUI now resolves dialogs through a shared modal presentation pipeline, alongside anchored popovers and timed snackbars.",
                        )
                        .style(t.body.clone())
                        .color(c.on_surface_secondary),
                    )
                    .child(
                        Panel::new().padding(20.0).child(
                            HStack::new()
                                .spacing(16.0)
                                .child(
                                    Popover::new("Show popover", popover_content)
                                        .caret_size(12.0)
                                        .trigger(popover_trigger),
                                )
                                .child(
                                    Dialog::new("Open dialog", move || {
                                        let t = &dialog_theme.typography;
                                        let c = &dialog_theme.colors;
                                        DialogContent::new()
                                            .title("Review Changes")
                                            .supporting_text(
                                                "Dialogs open centered, dismiss on Escape or outside click, and can host structured body and action sections.",
                                            )
                                            .body(
                                                TextWidget::new(
                                                    "This helper gives dialogs a consistent header, content spacing, and footer separation without forcing a single action-row layout.",
                                                )
                                                .style(t.body.clone())
                                                .color(c.on_surface_secondary),
                                            )
                                            .footer(
                                                HStack::new()
                                                    .spacing(12.0)
                                                    .child(
                                                        Button::new("Cancel")
                                                            .style(ButtonStyle::Outlined)
                                                            .on_tap(|ctx| ctx.dismiss_modal()),
                                                    )
                                                    .child(
                                                        Button::new("Apply")
                                                            .style(ButtonStyle::Filled)
                                                            .on_tap(|ctx| ctx.dismiss_modal()),
                                                    ),
                                            )
                                    })
                                        .trigger(dialog_trigger),
                                )
                                .child(
                                    Snackbar::new("Show snackbar", snackbar_content)
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
                                    TextWidget::new("Notes")
                                        .style(t.heading_3.clone())
                                        .color(c.on_surface),
                                )
                                .child(
                                    TextWidget::new(
                                        "Dialogs now share one modal request API. Footer actions can dismiss the current modal without knowing whether FernUI resolved it to an in-tree overlay or a native modal child window.",
                                    )
                                    .style(t.body.clone())
                                    .color(c.on_surface_secondary),
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
