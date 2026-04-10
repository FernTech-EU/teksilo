use std::time::Duration;

use fern_ui::app::WindowConfig;
use fern_ui::core::WidgetPlacement;
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Badge, Button, ButtonStyle, Dialog, DialogContent, HStack, Panel, Popover, ScrollArea,
    Snackbar, TextWidget, VStack,
};

#[derive(Debug, Clone, PartialEq)]
enum Cmd {
    OpenNativeModal,
    CloseNativeModal,
}

impl AppCommand for Cmd {}

#[derive(Debug)]
struct OverlayDemo {
    root_child_id: Option<WidgetId>,
}

impl OverlayDemo {
    fn new() -> Self {
        Self { root_child_id: None }
    }
}

impl Widget for OverlayDemo {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let t = &theme.typography;
        let c = &theme.colors;

        let dialog_content = DialogContent::new()
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
                            .on_tap(|ctx| ctx.dismiss_top_overlay()),
                    )
                    .child(
                        Button::new("Apply")
                            .style(ButtonStyle::Filled)
                            .on_tap(|ctx| ctx.dismiss_top_overlay()),
                    ),
            );

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
                            "FernUI now supports centered dialogs and anchored popovers in-tree, plus native modal windows through the app command context.",
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
                                    Dialog::new("Open dialog", dialog_content)
                                        .trigger(dialog_trigger),
                                )
                                .child(
                                    Snackbar::new("Show snackbar", snackbar_content)
                                        .auto_dismiss_after(Duration::from_millis(2500)),
                                )
                                .child(
                                    Button::new("Native modal window")
                                        .style(ButtonStyle::Tonal)
                                        .on_click(Cmd::OpenNativeModal),
                                ),
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
                                        "Dialogs, popovers, and snackbars all use the shared in-tree overlay system. Dialogs now have a reusable structured content helper, popovers can render a caret, and snackbars can auto-dismiss after a configured duration. The native modal example still uses a separate OS window and blocks its parent until closed.",
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

#[derive(Debug)]
struct NativeModalRoot {
    root_child_id: Option<WidgetId>,
}

impl NativeModalRoot {
    fn new() -> Self {
        Self { root_child_id: None }
    }
}

impl Widget for NativeModalRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let t = &theme.typography;
        let c = &theme.colors;
        let root = ctx.add(
            Panel::new().padding(24.0).child(
                VStack::new()
                    .spacing(16.0)
                    .child(
                        TextWidget::new("Native modal window")
                            .style(t.heading_2.clone())
                            .color(c.on_surface),
                    )
                    .child(
                        TextWidget::new(
                            "This dialog is a separate OS window created with WindowConfig::modal(true). Closing it unblocks the parent window.",
                        )
                        .style(t.body.clone())
                        .color(c.on_surface_secondary),
                    )
                    .child(Button::new("Close window").on_click(Cmd::CloseNativeModal)),
            ),
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
        .on_command(|cmd: &Cmd, ctx| match cmd {
            Cmd::OpenNativeModal => {
                ctx.create_window(
                    WindowConfig::new()
                        .title("Native Modal")
                        .size(460, 260)
                        .modal(true)
                        .parent(ctx.source_window())
                        .root(|tree| tree.add(NativeModalRoot::new())),
                );
            }
            Cmd::CloseNativeModal => {
                ctx.close_window(ctx.source_window());
            }
        })
        .root(|tree| tree.add(OverlayDemo::new()))
        .run();
}