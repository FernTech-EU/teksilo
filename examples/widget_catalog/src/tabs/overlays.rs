//! Overlays tab — Tooltip (cascading 3-tier showcase), Popover, Dialog,
//! MessageBox, Snackbar, Shadow.

use std::rc::Rc;
use std::time::Duration;

use bastyde::prelude::*;
use bastyde::widgets::{
    Button, ButtonVariant, Divider, EventContextMessageBoxExt, Expand, HStack, MessageBox,
    MessageBoxButtons, Panel, Popover, ProgressBar, Snackbar, Spacer, TabInfo, TabWidget,
    TextWidget, VStack,
};

/// A row of toast triggers (one per severity) plus the
/// `NotificationCenterButton` bell. `install_toast_default()` in
/// main.rs registers the host + archive that make these live.
fn toast_row(archive: Option<Rc<NotificationArchiveModel>>) -> impl Widget + 'static {
    let mut row = HStack::new()
        .spacing(8.0)
        .child(
            Button::new(tr!(ovr_toast_btn_info())).on_activate_fn(|ctx| {
                ctx.show_toast(Toast::info(tr!(ovr_toast_info_msg())));
            }),
        )
        .child(
            Button::new(tr!(ovr_toast_btn_success()))
                .variant(ButtonVariant::Filled)
                .on_activate_fn(|ctx| {
                    ctx.show_toast(Toast::success(tr!(ovr_toast_success_msg())));
                }),
        )
        .child(
            Button::new(tr!(ovr_toast_btn_warning())).on_activate_fn(|ctx| {
                ctx.show_toast(
                    Toast::warning(tr!(ovr_toast_warning_msg()))
                        .body(tr!(ovr_toast_warning_body())),
                );
            }),
        )
        .child(
            Button::new(tr!(ovr_toast_btn_error())).on_activate_fn(|ctx| {
                ctx.show_toast(
                    Toast::error(tr!(ovr_toast_error_msg()))
                        .body(tr!(ovr_toast_error_body()))
                        .action(ToastAction::primary(tr!(ovr_toast_error_action()), |_| {
                            println!("[widget-catalog] show errors clicked");
                        })),
                );
            }),
        )
        .child(
            Button::new(tr!(ovr_toast_btn_loading())).on_activate_fn(|ctx| {
                ctx.show_toast(Toast::loading(tr!(ovr_toast_loading_msg())));
            }),
        )
        .child(Spacer::new());
    if let Some(archive) = archive {
        row = row.child(NotificationCenterButton::new(archive));
    }
    row
}

use crate::shared::{
    KEY_STAT_FOOD, KEY_STAT_HAPPINESS, KEY_STAT_TRADE, KEY_TIP_A, KEY_TIP_B, KEY_TIP_C, Signals,
    section, tab_header,
};

// ── Cascading tooltip showcase ─────────────────────────────────────────
// Mirrors `examples/tooltips_showcase`: three columns laid out as
// `HStack { Expand.flex(1) × 3 }`. Each column showcases one tooltip
// tier with cascading depth, sharing the registry built in
// `shared::build_tooltip_registry`.

fn cascade_plain_column() -> impl Widget + 'static {
    Panel::new()
        .background(SurfaceRole::Raised)
        .border_color(BorderRole::Default)
        .border_width(1.0)
        .corner_radius(8.0)
        .padding(12.0)
        .child(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new(tr!(ovr_plain_tooltips_heading()))
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new(tr!(ovr_plain_tooltips_subtitle()))
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(Button::new(lit!("Save")).tooltip(tr!(ovr_tooltip_save_doc())))
                .child(Button::new(lit!("Open")).tooltip(tr!(ovr_tooltip_open_file())))
                .child(Button::new(lit!("Close")).tooltip(tr!(ovr_tooltip_close_tab())))
                .child(Spacer::new()),
        )
}

fn cascade_rich_column() -> impl Widget + 'static {
    Panel::new()
        .background(SurfaceRole::Raised)
        .border_color(BorderRole::Default)
        .border_width(1.0)
        .corner_radius(8.0)
        .padding(12.0)
        .child(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new(tr!(ovr_rich_tooltips_heading()))
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new(tr!(ovr_rich_tooltips_subtitle()))
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(Button::new(tr!(ovr_hover_level_1())).rich_tooltip(KEY_TIP_A))
                .child(Button::new(tr!(ovr_hover_level_2())).rich_tooltip(KEY_TIP_B))
                .child(Button::new(tr!(ovr_hover_level_3())).rich_tooltip(KEY_TIP_C))
                .child(
                    Button::new(tr!(ovr_plain_among_rich()))
                        .tooltip(tr!(ovr_plain_among_rich_tip())),
                )
                .child(
                    TextWidget::new(tr!(ovr_rich_dwell_tip()))
                        .style(TextStyleRole::Tiny)
                        .color(TextRole::Secondary),
                ),
        )
}

/// Composite-tooltip body: a "province info" card whose stat buttons
/// cascade *into* the rich-tooltip chain — multi-tier mixing.
///
/// All `TextWidget`s use `TextRole::TooltipText` because the composite
/// tooltip surface is `tooltip_bg` (dark in both themes) — the
/// regular `Primary` / `Secondary` roles would render as black-on-black
/// in the light theme.
fn province_composite_body() -> impl Widget + 'static {
    VStack::new()
        .spacing(8.0)
        .child(
            TextWidget::new(tr!(ovr_province_iberia()))
                .style(TextStyleRole::BodyBold)
                .color(TextRole::TooltipText),
        )
        .child(
            TextWidget::new(tr!(ovr_province_overview()))
                .style(TextStyleRole::Small)
                .color(TextRole::TooltipShortcut),
        )
        .child(ProgressBar::new(0.65))
        .child(
            HStack::new()
                .spacing(12.0)
                .child(Button::new(tr!(ovr_stat_food_label())).rich_tooltip(KEY_STAT_FOOD))
                .child(Button::new(tr!(ovr_stat_trade_label())).rich_tooltip(KEY_STAT_TRADE))
                .child(
                    Button::new(tr!(ovr_stat_happiness_label())).rich_tooltip(KEY_STAT_HAPPINESS),
                ),
        )
}

/// Composite-tooltip body with an embedded `TabWidget` — proves
/// arbitrary composition inside a tooltip surface.
///
/// `TabWidget` is configured for the dark tooltip background via
/// `selected_text_role` / `idle_text_role`; otherwise the tab labels
/// would render in their default `Primary` / `Secondary` roles
/// (dark-on-dark in the light theme). Tab content text uses
/// `TextRole::TooltipText` for the same reason.
fn tabbed_composite_body() -> impl Widget + 'static {
    let selected: Signal<Option<bastyde::widgets::tab_widget::TabId>> = Signal::new(None);
    let body = TabWidget::new(selected)
        .selected_text_role(TextRole::TooltipText)
        .idle_text_role(TextRole::TooltipShortcut)
        .static_tab(
            TabInfo::new().title(tr!(ovr_tab_stats())),
            VStack::new()
                .spacing(4.0)
                .child(TextWidget::new(tr!(ovr_stat_population())).color(TextRole::TooltipText))
                .child(TextWidget::new(tr!(ovr_stat_garrison())).color(TextRole::TooltipText)),
        )
        .static_tab(
            TabInfo::new().title(tr!(ovr_tab_history())),
            TextWidget::new(tr!(ovr_province_history())).color(TextRole::TooltipText),
        );
    VStack::new()
        .spacing(8.0)
        .child(
            TextWidget::new(tr!(ovr_tabbed_details()))
                .style(TextStyleRole::BodyBold)
                .color(TextRole::TooltipText),
        )
        .child(body)
}

/// Composite-tooltip body with an internal `Button` — demonstrates
/// keyboard-focus reachability after dwell-to-sticky promotion.
fn interactive_composite_body() -> impl Widget + 'static {
    VStack::new()
        .spacing(8.0)
        .child(
            TextWidget::new(tr!(ovr_treasury_report()))
                .style(TextStyleRole::BodyBold)
                .color(TextRole::TooltipText),
        )
        .child(
            TextWidget::new(tr!(ovr_treasury_subtitle()))
                .style(TextStyleRole::Small)
                .color(TextRole::TooltipShortcut),
        )
        .child(ProgressBar::new(0.42))
        .child(Button::new(tr!(ovr_open_ledger())).on_activate_fn(|_ctx| {
            println!("Open ledger pressed from inside a composite tooltip!");
        }))
}

fn cascade_composite_column() -> impl Widget + 'static {
    Panel::new()
        .background(SurfaceRole::Raised)
        .border_color(BorderRole::Default)
        .border_width(1.0)
        .corner_radius(8.0)
        .padding(12.0)
        .child(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new(tr!(ovr_composite_tooltips_heading()))
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new(tr!(ovr_composite_tooltips_subtitle()))
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    Button::new(tr!(ovr_province_info_btn()))
                        .composite_tooltip(province_composite_body()),
                )
                .child(
                    Button::new(tr!(ovr_tabbed_details()))
                        .composite_tooltip(tabbed_composite_body()),
                )
                .child(
                    Button::new(tr!(ovr_with_internal_button()))
                        .composite_tooltip(interactive_composite_body()),
                )
                .child(
                    TextWidget::new(tr!(ovr_composite_dwell_tip()))
                        .style(TextStyleRole::Tiny)
                        .color(TextRole::Secondary),
                ),
        )
}

/// Three-column cascading tooltip showcase: plain / rich / composite
/// tiers laid out side by side via `HStack { Expand.flex(1) × 3 }`.
///
/// `respect_intrinsic()` is the key: inside the catalog's intrinsic-
/// sized section/scroll wrapper the parent passes an unspecified
/// height proposal, so a default `Expand::new().flex(1)` (basis 0)
/// would collapse to zero and the entire showcase would render as
/// just the section heading. Falling back to the column's natural
/// size as the basis keeps each column visible at its intrinsic
/// height while still letting the columns share horizontal slack.
fn cascade_showcase() -> impl Widget + 'static {
    HStack::new()
        .spacing(12.0)
        .child(
            Expand::new()
                .flex(1.0)
                .respect_intrinsic()
                .child(cascade_plain_column()),
        )
        .child(
            Expand::new()
                .flex(1.0)
                .respect_intrinsic()
                .child(cascade_rich_column()),
        )
        .child(
            Expand::new()
                .flex(1.0)
                .respect_intrinsic()
                .child(cascade_composite_column()),
        )
}

pub fn title() -> LocalizedString {
    tr!(tab_overlays_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_overlays_refs())
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let tooltip = section(ctx, tr!(ovr_section_tooltip_cascade()), cascade_showcase());
    let popover = section(
        ctx,
        tr!(ovr_section_popover()),
        Popover::new(tr!(ovr_popover_anchor())).content(
            VStack::new()
                .spacing(4.0)
                .child(
                    TextWidget::new(tr!(overlays_popover_content())).style(TextStyleRole::BodyBold),
                )
                .child(
                    TextWidget::new(tr!(overlays_click_outside_to_dismiss()))
                        .style(TextStyleRole::Small),
                ),
        ),
    );
    let dialog_btn = Button::new(tr!(overlays_open_dialog()))
        .variant(ButtonVariant::Filled)
        .on_activate_fn(|ctx| {
            ctx.present_message_box(
                MessageBox::information(tr!(overlays_dialog_example()))
                    .detailed_text(tr!(overlays_this_is_a_dialog_presented_via()))
                    .buttons(MessageBoxButtons::Ok),
            );
        });
    let dialog = section(ctx, tr!(ovr_section_dialog()), dialog_btn);
    let messagebox = section(
        ctx,
        tr!(ovr_section_messagebox()),
        HStack::new()
            .spacing(8.0)
            .child(
                Button::new(tr!(ovr_mb_info()))
                    .variant(ButtonVariant::Ghost)
                    .on_activate_fn(|ctx| {
                        ctx.present_message_box(
                            MessageBox::information(tr!(ovr_mb_info()))
                                .detailed_text(tr!(overlays_informational_dialog()))
                                .buttons(MessageBoxButtons::Ok),
                        );
                    }),
            )
            .child(
                Button::new(tr!(ovr_mb_warning()))
                    .variant(ButtonVariant::Ghost)
                    .on_activate_fn(|ctx| {
                        ctx.present_message_box(
                            MessageBox::warning(tr!(ovr_mb_warning()))
                                .detailed_text(tr!(overlays_disk_is_almost_full()))
                                .buttons(MessageBoxButtons::Ok),
                        );
                    }),
            )
            .child(
                Button::new(tr!(ovr_mb_error()))
                    .variant(ButtonVariant::Ghost)
                    .on_activate_fn(|ctx| {
                        ctx.present_message_box(
                            MessageBox::critical(tr!(ovr_mb_error()))
                                .detailed_text(tr!(overlays_something_went_wrong()))
                                .buttons(MessageBoxButtons::Ok),
                        );
                    }),
            )
            .child(
                Button::new(tr!(demo_confirm()))
                    .variant(ButtonVariant::Ghost)
                    .on_activate_fn(|ctx| {
                        ctx.present_message_box(
                            MessageBox::question(tr!(overlays_are_you_sure()))
                                .detailed_text(tr!(overlays_this_action_cannot_be_undone()))
                                .buttons(MessageBoxButtons::OkCancel),
                        );
                    }),
            ),
    );
    let snackbar = section(
        ctx,
        lit!("Snackbar"),
        Snackbar::new(tr!(overlays_file_saved_successfully()))
            .content(
                // The snackbar surface is the dark `tooltip_bg`, so content
                // text must use `TooltipText` to stay legible in light theme.
                TextWidget::new(tr!(overlays_file_saved_successfully_2()))
                    .style(TextStyleRole::Body)
                    .color(TextRole::TooltipText),
            )
            .trigger(Button::new(tr!(overlays_show_snackbar())).variant(ButtonVariant::Filled))
            .auto_dismiss_after(std::time::Duration::from_secs(3)),
    );
    let shadow = section(
        ctx,
        tr!(ovr_section_shadow()),
        Panel::new()
            .background(SurfaceRole::Raised)
            .padding(16.0)
            .child(
                TextWidget::new(tr!(overlays_card_like_surface_with_the_def()))
                    .style(TextStyleRole::Small),
            ),
    );
    let archive = ctx.app_state::<Rc<NotificationArchiveModel>>().cloned();
    let toast = section(
        ctx,
        lit!("Toast + NotificationCenterButton"),
        toast_row(archive),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(tooltip)
            .add_child(popover)
            .add_child(dialog)
            .add_child(messagebox)
            .add_child(snackbar)
            .add_child(toast)
            .add_child(shadow),
    )
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // Popover.content(VStack with chained children), Snackbar
    // .content+.trigger+chained — the chained imperative form is the
    // straightforward reading; pre-register and splice via #{...}.
    let popover_widget = ctx.add(
        Popover::new(tr!(ovr_popover_anchor())).content(
            VStack::new()
                .spacing(4.0)
                .child(
                    TextWidget::new(tr!(overlays_popover_content())).style(TextStyleRole::BodyBold),
                )
                .child(
                    TextWidget::new(tr!(overlays_click_outside_to_dismiss()))
                        .style(TextStyleRole::Small),
                ),
        ),
    );
    let snackbar_widget = ctx.add(
        Snackbar::new(tr!(overlays_file_saved_successfully()))
            .content(
                // Dark `tooltip_bg` surface — content text needs `TooltipText`.
                TextWidget::new(tr!(overlays_file_saved_successfully_2()))
                    .style(TextStyleRole::Body)
                    .color(TextRole::TooltipText),
            )
            .trigger(Button::new(tr!(overlays_show_snackbar())).variant(ButtonVariant::Filled))
            .auto_dismiss_after(Duration::from_secs(3)),
    );
    let archive = ctx.app_state::<Rc<NotificationArchiveModel>>().cloned();
    let toast_widget = ctx.add(toast_row(archive));

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_overlays_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_overlays_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(ovr_section_tooltip_cascade())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 12.0
                    Expand {
                        flex: 1.0
                        respect_intrinsic
                        child: cascade_plain_column()
                    }
                    Expand {
                        flex: 1.0
                        respect_intrinsic
                        child: cascade_rich_column()
                    }
                    Expand {
                        flex: 1.0
                        respect_intrinsic
                        child: cascade_composite_column()
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(ovr_section_popover())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ popover_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(ovr_section_dialog())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Button::new(tr!(overlays_open_dialog())) {
                    variant: ButtonVariant::Filled
                    on_activate_fn: |ctx| {
                        ctx.present_message_box(
                            MessageBox::information(tr!(overlays_dialog_example()))
                                .detailed_text(tr!(overlays_this_is_a_dialog_presented_via()))
                                .buttons(MessageBoxButtons::Ok),
                        );
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(ovr_section_messagebox())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 8.0
                    Button::new(tr!(ovr_mb_info())) {
                        variant: ButtonVariant::Ghost
                        on_activate_fn: |ctx| {
                            ctx.present_message_box(
                                MessageBox::information(tr!(ovr_mb_info()))
                                    .detailed_text(tr!(overlays_informational_dialog()))
                                    .buttons(MessageBoxButtons::Ok),
                            );
                        }
                    }
                    Button::new(tr!(ovr_mb_warning())) {
                        variant: ButtonVariant::Ghost
                        on_activate_fn: |ctx| {
                            ctx.present_message_box(
                                MessageBox::warning(tr!(ovr_mb_warning()))
                                    .detailed_text(tr!(overlays_disk_is_almost_full()))
                                    .buttons(MessageBoxButtons::Ok),
                            );
                        }
                    }
                    Button::new(tr!(ovr_mb_error())) {
                        variant: ButtonVariant::Ghost
                        on_activate_fn: |ctx| {
                            ctx.present_message_box(
                                MessageBox::critical(tr!(ovr_mb_error()))
                                    .detailed_text(tr!(overlays_something_went_wrong()))
                                    .buttons(MessageBoxButtons::Ok),
                            );
                        }
                    }
                    Button::new(tr!(demo_confirm())) {
                        variant: ButtonVariant::Ghost
                        on_activate_fn: |ctx| {
                            ctx.present_message_box(
                                MessageBox::question(tr!(overlays_are_you_sure()))
                                    .detailed_text(tr!(overlays_this_action_cannot_be_undone()))
                                    .buttons(MessageBoxButtons::OkCancel),
                            );
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Snackbar")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ snackbar_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Toast + NotificationCenterButton")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ toast_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(ovr_section_shadow())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Panel {
                    background: SurfaceRole::Raised
                    padding: 16.0
                    TextWidget::new(tr!(overlays_card_like_surface_with_the_def())) {
                        style: TextStyleRole::Small
                    }
                }
            }
        }
    )
}
