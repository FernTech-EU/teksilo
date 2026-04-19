use std::time::Duration;

use fern_ui::core::WidgetPlacement;
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Badge, Button, ButtonVariant, Dialog, DialogContent, EventContextMessageBoxExt, HStack,
    MessageBox, MessageBoxButton, MessageBoxButtons, Panel, Popover, ScrollArea, Snackbar,
    StandardButton, TextWidget, VStack,
};

#[derive(Debug)]
struct OverlayDemo {
    root_child_id: Option<WidgetId>,
    /// Last MessageBox result, rendered below the MessageBox row so the
    /// user can see which button fired (plus checkbox / escape flags).
    last_message_box_result: Signal<String>,
}

impl OverlayDemo {
    fn new() -> Self {
        Self {
            root_child_id: None,
            last_message_box_result: Signal::new("—".to_string()),
        }
    }
}

impl OverlayDemo {
    /// Panel demonstrating every MessageBox severity and preset. Each
    /// Button launches a MessageBox via `ctx.present_message_box(...)`,
    /// and the resulting `StandardButton` (plus the checkbox and
    /// escape-dismissal flags) is rendered below via the shared
    /// `last_message_box_result` signal.
    fn message_box_panel(&self) -> impl Widget + 'static {
        let last = self.last_message_box_result.clone();

        let record = move |summary: String| {
            last.set(summary);
        };

        // --- unsaved-changes triad (question severity) ---
        let record_save = record.clone();
        let save_btn = Button::new_literal("Save changes?")
            .style(ButtonVariant::Regular)
            .on_activate_fn(move |ctx| {
                let record = record_save.clone();
                ctx.present_message_box(
                    MessageBox::question_literal("Save changes?")
                        .text_literal("You have unsaved changes in report.skrib.")
                        .informative_text_literal(
                            "Your changes will be lost if you don't save them.",
                        )
                        .buttons(MessageBoxButtons::SaveDiscardCancel)
                        .default_button(StandardButton::Save)
                        .escape_button(StandardButton::Cancel)
                        .on_result(move |r, _| {
                            record(format!(
                                "Save changes? → {:?} (escape={})",
                                r.button, r.dismissed_by_escape
                            ));
                        }),
                );
            });

        // --- destructive confirmation (critical severity, safe default) ---
        let record_delete = record.clone();
        let delete_btn = Button::new_literal("Delete file?")
            .style(ButtonVariant::Regular)
            .on_activate_fn(move |ctx| {
                let record = record_delete.clone();
                ctx.present_message_box(
                    MessageBox::critical_literal("Delete file?")
                        .text_literal(
                            "Permanently delete report.skrib? This action cannot be undone.",
                        )
                        .buttons(MessageBoxButtons::YesNo)
                        .default_button(StandardButton::No)
                        .escape_button(StandardButton::No)
                        .on_result(move |r, _| {
                            record(format!(
                                "Delete file? → {:?} (escape={})",
                                r.button, r.dismissed_by_escape
                            ));
                        }),
                );
            });

        // --- error-with-retry triad (critical + detailed_text) ---
        let record_open = record.clone();
        let open_btn = Button::new_literal("Could not open file")
            .style(ButtonVariant::Regular)
            .on_activate_fn(move |ctx| {
                let record = record_open.clone();
                ctx.present_message_box(
                    MessageBox::critical_literal("Could not open file")
                        .text_literal("report.skrib could not be opened.")
                        .informative_text_literal("You may not have permission.")
                        .detailed_text_literal(
                            "Underlying OS error: EACCES (permission denied)\n\
                             open(\"report.skrib\", O_RDWR) → errno 13",
                        )
                        .buttons(MessageBoxButtons::RetryIgnoreAbort)
                        .default_button(StandardButton::Retry)
                        .escape_button(StandardButton::Abort)
                        .on_result(move |r, _| {
                            record(format!(
                                "Could not open → {:?} (escape={})",
                                r.button, r.dismissed_by_escape
                            ));
                        }),
                );
            });

        // --- informational + don't-show-again ---
        let record_welcome = record.clone();
        let welcome_btn = Button::new_literal("Welcome")
            .style(ButtonVariant::Regular)
            .on_activate_fn(move |ctx| {
                let record = record_welcome.clone();
                ctx.present_message_box(
                    MessageBox::information_literal("Welcome to FernUI")
                        .text_literal(
                            "This demo showcases the MessageBox pipeline across severities.",
                        )
                        .show_again_checkbox_literal("Don't show this again")
                        .buttons(MessageBoxButtons::Ok)
                        .on_result(move |r, _| {
                            record(format!(
                                "Welcome → {:?} (checkbox={}, escape={})",
                                r.button, r.checkbox_checked, r.dismissed_by_escape
                            ));
                        }),
                );
            });

        // --- custom button row (question + Help + Ok) ---
        let record_help = record.clone();
        let help_btn = Button::new_literal("Custom buttons")
            .style(ButtonVariant::Regular)
            .on_activate_fn(move |ctx| {
                let record = record_help.clone();
                ctx.present_message_box(
                    MessageBox::question_literal("How do I…?")
                        .text_literal("Open help or dismiss with OK.")
                        .buttons(MessageBoxButtons::Custom(vec![
                            MessageBoxButton::standard(StandardButton::Help),
                            MessageBoxButton::standard(StandardButton::Ok),
                        ]))
                        .default_button(StandardButton::Ok)
                        .escape_button(StandardButton::Ok)
                        .on_result(move |r, _| {
                            record(format!(
                                "Custom buttons → {:?} (escape={})",
                                r.button, r.dismissed_by_escape
                            ));
                        }),
                );
            });

        let result_readout = TextWidget::new_literal("Last result:")
            .style(TextStyleRole::Small)
            .color(TextRole::Secondary);

        let result_signal = self.last_message_box_result.clone();
        let result_value = TextWidget::new_literal(result_signal.get())
            .bind_text(result_signal)
            .style(TextStyleRole::BodyBold)
            .color(TextRole::Primary);

        Panel::new().padding(20.0).child(
            VStack::new()
                .spacing(14.0)
                .child(
                    TextWidget::new_literal("MessageBox")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal(
                        "Higher-level alert surface built on ModalContainer. Each trigger below exercises a different severity and button set; the result is reported under the row.",
                    )
                    .style(TextStyleRole::Body)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(12.0)
                        .child(save_btn)
                        .child(delete_btn)
                        .child(open_btn)
                        .child(welcome_btn)
                        .child(help_btn),
                )
                .child(HStack::new().spacing(8.0).child(result_readout).child(result_value)),
        )
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
                    .child(self.message_box_panel())
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
                                )
                                .child(
                                    TextWidget::new_literal(
                                        "MessageBox is the higher-level alert surface: severity ↔ icon ↔ color, standard Qt-style button sets (Ok/Cancel/Yes/No/Save/Discard/…), default-on-Enter + escape-on-Escape keyboard handling, and an `on_result` closure that receives which button fired plus the state of the optional don't-show-again checkbox. Critical severity disables click-outside dismissal and uses EscapeKey-only close behavior — matching Qt's critical dialog convention. Initial focus lands on the default button via `ModalRequest::focus_target` + `Widget::initial_focus_hint`, so platform-native button orderings (Cancel-left, Default-right) work even when the default isn't the first focusable descendant.",
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
