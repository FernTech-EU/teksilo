use std::time::Duration;

use bastyde::core::WidgetPlacement;
use bastyde::prelude::*;
use bastyde::widgets::{
    Badge, Button, ButtonVariant, Dialog, DialogContent, EventContextMessageBoxExt, Expand, HStack,
    MessageBox, MessageBoxButton, MessageBoxButtons, Panel, Popover, ScrollArea, Snackbar, Spacer,
    StandardButton, TextWidget, Toolbar, VStack,
};

fn dark_mode_toolbar() -> impl Widget {
    let is_dark = Signal::new(false);
    Toolbar::new().child(HStack::new().child(Spacer::new()).child(
        Button::new(lit!("Toggle Dark Mode")).on_activate_fn(move |ctx| {
            let next = !is_dark.get();
            is_dark.set(next);
            ctx.set_theme(if next {
                bastyde::presets::intui::dark()
            } else {
                bastyde::presets::intui::light()
            });
        }),
    ))
}

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
        let save_btn = Button::new(lit!("Save changes?"))
            .variant(ButtonVariant::Plain)
            .on_activate_fn(move |ctx| {
                let record = record_save.clone();
                ctx.present_message_box(
                    MessageBox::question(lit!("Save changes?"))
                        .text(lit!("You have unsaved changes in report.skrib."))
                        .informative_text(lit!("Your changes will be lost if you don't save them."))
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
        let delete_btn = Button::new(lit!("Delete file?"))
            .variant(ButtonVariant::Plain)
            .on_activate_fn(move |ctx| {
                let record = record_delete.clone();
                ctx.present_message_box(
                    MessageBox::critical(lit!("Delete file?"))
                        .text(lit!(
                            "Permanently delete report.skrib? This action cannot be undone."
                        ))
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
        let open_btn = Button::new(lit!("Could not open file"))
            .variant(ButtonVariant::Plain)
            .on_activate_fn(move |ctx| {
                let record = record_open.clone();
                ctx.present_message_box(
                    MessageBox::critical(lit!("Could not open file"))
                        .text(lit!("report.skrib could not be opened."))
                        .informative_text(lit!("You may not have permission."))
                        .detailed_text(lit!(
                            "Underlying OS error: EACCES (permission denied)\n\
                             open(\"report.skrib\", O_RDWR) → errno 13"
                        ))
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
        let welcome_btn = Button::new(lit!("Welcome"))
            .variant(ButtonVariant::Plain)
            .on_activate_fn(move |ctx| {
                let record = record_welcome.clone();
                ctx.present_message_box(
                    MessageBox::information(lit!("Welcome to Bastyde"))
                        .text(lit!(
                            "This demo showcases the MessageBox pipeline across severities."
                        ))
                        .show_again_checkbox(lit!("Don't show this again"))
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
        let help_btn = Button::new(lit!("Custom buttons"))
            .variant(ButtonVariant::Plain)
            .on_activate_fn(move |ctx| {
                let record = record_help.clone();
                ctx.present_message_box(
                    MessageBox::question(lit!("How do I…?"))
                        .text(lit!("Open help or dismiss with OK."))
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

        let result_readout = TextWidget::new(lit!("Last result:"))
            .style(TextStyleRole::Small)
            .color(TextRole::Secondary);

        let result_signal = self.last_message_box_result.clone();
        let result_value = TextWidget::new(lit!(result_signal.get()))
            .bind_text(result_signal)
            .style(TextStyleRole::BodyBold)
            .color(TextRole::Primary);

        Panel::new().padding(20.0).child(
            VStack::new()
                .spacing(14.0)
                .child(
                    TextWidget::new(lit!("MessageBox"))
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new(lit!("Higher-level alert surface built on ModalContainer. Each trigger below exercises a different severity and button set; the result is reported under the row."),
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
        let _theme = ctx.theme_signal().get();

        let popover_content = VStack::new()
            .spacing(12.0)
            .child(
                TextWidget::new(lit!("Popover"))
                    .style(TextStyleRole::Small)
                    .color(TextRole::Primary),
            )
            .child(
                TextWidget::new(lit!("Use popovers for compact contextual actions without leaving the current surface."),
                )
                .style(TextStyleRole::Body)
                .color(TextRole::Secondary),
            )
            .child(
                HStack::new()
                    .spacing(8.0)
                    .child(Badge::new(lit!("Quick actions")))
                    .child(Badge::new(lit!("Inline help")))
                    .child(Badge::new(lit!("Inspector"))),
            );

        let snackbar_content = HStack::new()
            .spacing(14.0)
            .child(
                TextWidget::new(lit!("Autosave complete"))
                    .style(TextStyleRole::Body)
                    .color(TextRole::TooltipText),
            )
            .child(
                Button::new(lit!("Dismiss"))
                    .variant(ButtonVariant::Plain)
                    .on_activate_fn(|ctx| ctx.dismiss_top_overlay()),
            );

        let popover_trigger = Panel::new().padding(12.0).child(
            HStack::new()
                .spacing(10.0)
                .child(Badge::new(lit!("Context")))
                .child(
                    TextWidget::new(lit!("Popover actions"))
                        .style(TextStyleRole::Small)
                        .color(TextRole::Primary),
                ),
        );

        let dialog_trigger = Panel::new().padding(12.0).child(
            HStack::new()
                .spacing(10.0)
                .child(Badge::new(lit!("Modal")))
                .child(
                    TextWidget::new(lit!("Review changes"))
                        .style(TextStyleRole::Small)
                        .color(TextRole::Primary),
                ),
        );
        let modal_trigger_id = ctx.add(
            Dialog::new(lit!("Adaptive modal window"))
                .content(move || {
                    DialogContent::new()
                        .title(lit!("Adaptive modal dialog"))
                        .supporting_text(lit!("The framework chooses the best modal presentation for the current backend: a native modal child window when reliable, otherwise a centered in-tree dialog."),
                        )
                        .body(
                            TextWidget::new(lit!("The app code does not branch on Wayland or window-system support here; it issues one modal request and lets Bastyde resolve it."),
                            )
                            .style(TextStyleRole::Body)
                            .color(TextRole::Secondary),
                        )
                        .footer(
                            Button::new(lit!("Close"))
                                .variant(ButtonVariant::Filled)
                                .on_activate_fn(|ctx| ctx.dismiss_modal()),
                        )
                })
                .variant(ButtonVariant::Plain),
        );

        let root = ctx.add(
            ScrollArea::new().child(
                VStack::new()
                    .spacing(24.0)
                    .child(
                        TextWidget::new(lit!("Dialogs and Popovers"))
                            .style(TextStyleRole::BodyBold)
                            .color(TextRole::Primary),
                    )
                    .child(
                        TextWidget::new(lit!("Bastyde now resolves dialogs through a shared modal presentation pipeline, alongside anchored popovers and timed snackbars."),
                        )
                        .style(TextStyleRole::Body)
                        .color(TextRole::Secondary),
                    )
                    .child(
                        Panel::new().padding(20.0).child(
                            HStack::new()
                                .spacing(16.0)
                                .child(
                                    Popover::new(lit!("Show popover"))
                                        .content(popover_content)
                                        .caret_size(12.0)
                                        .trigger(popover_trigger),
                                )
                                .child(
                                    Dialog::new(lit!("Open dialog"))
                                        .content(move || {
                                            DialogContent::new()
                                                .title(lit!("Review Changes"))
                                                .supporting_text(lit!("Dialogs open centered, dismiss on Escape or outside click, and can host structured body and action sections."),
                                                )
                                                .body(
                                                    TextWidget::new(lit!("This helper gives dialogs a consistent header, content spacing, and footer separation without forcing a single action-row layout."),
                                                    )
                                                    .style(TextStyleRole::Body)
                                                    .color(TextRole::Secondary),
                                                )
                                                .footer(
                                                    HStack::new()
                                                        .spacing(12.0)
                                                        .child(
                                                            Button::new(lit!("Cancel"))
                                                                .variant(ButtonVariant::Plain)
                                                                .on_activate_fn(|ctx| ctx.dismiss_modal()),
                                                        )
                                                        .child(
                                                            Button::new(lit!("Apply"))
                                                                .variant(ButtonVariant::Filled)
                                                                .on_activate_fn(|ctx| ctx.dismiss_modal()),
                                                        ),
                                                )
                                        })
                                        .trigger(dialog_trigger),
                                )
                                .child(
                                    Snackbar::new(lit!("Show snackbar"))
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
                                    TextWidget::new(lit!("Notes"))
                                        .style(TextStyleRole::BodyBold)
                                        .color(TextRole::Primary),
                                )
                                .child(
                                    TextWidget::new(lit!("Dialogs now share one modal request API. Footer actions can dismiss the current modal without knowing whether Bastyde resolved it to an in-tree overlay or a native modal child window."),
                                    )
                                    .style(TextStyleRole::Body)
                                    .color(TextRole::Secondary),
                                )
                                .child(
                                    TextWidget::new(lit!("MessageBox is the higher-level alert surface: severity ↔ icon ↔ color, standard Qt-style button sets (Ok/Cancel/Yes/No/Save/Discard/…), default-on-Enter + escape-on-Escape keyboard handling, and an `on_result` closure that receives which button fired plus the state of the optional don't-show-again checkbox. Critical severity disables click-outside dismissal and uses EscapeKey-only close behavior — matching Qt's critical dialog convention. Initial focus lands on the default button via `ModalRequest::focus_target` + `Widget::initial_focus_hint`, so platform-native button orderings (Cancel-left, Default-right) work even when the default isn't the first focusable descendant."),
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
                .title("Dialogs and Popovers")
                .size(980, 720)
                .root(|tree, _state| {
                    tree.add(
                        VStack::new()
                            .child(dark_mode_toolbar())
                            .child(Expand::new().child(OverlayDemo::new())),
                    )
                }),
        )
        .run();
}
