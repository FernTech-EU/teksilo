//! Chrome tab — Toolbar, StatusBar, Banner, Breadcrumb, Wizard.

use bastyde::prelude::*;
use bastyde::widgets::{
    Banner, Breadcrumb, BreadcrumbItem, Button, ButtonVariant, Divider, FixedSize, HStack,
    StatusBar, Step, Stepper, TextWidget, Toolbar, VStack, Wizard,
};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_chrome_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_chrome_refs())
}

fn make_wizard() -> Wizard {
    Wizard::new(tr!(chr_wizard_title()))
        .step(
            Step::new(tr!(chr_wizard_step1())).content(|| {
                TextWidget::new(tr!(chr_wizard_step1_body())).style(TextStyleRole::Body)
            }),
        )
        .step(
            Step::new(tr!(chr_wizard_step2())).content(|| {
                TextWidget::new(tr!(chr_wizard_step2_body())).style(TextStyleRole::Body)
            }),
        )
        .step(
            Step::new(tr!(chr_wizard_step3())).content(|| {
                TextWidget::new(tr!(chr_wizard_step3_body())).style(TextStyleRole::Body)
            }),
        )
        .trigger(Button::new(tr!(chr_wizard_trigger())).variant(ButtonVariant::Filled))
}

/// Embedded (inline) modern Stepper: a visible indicator strip, non-linear
/// navigation, a per-step validation gate, and an optional step with Skip.
/// Demonstrates the data-flow pattern — step content writes a `Signal`,
/// `complete_when` derives the Next gate from it.
fn make_stepper() -> Stepper {
    let gate = Signal::new(false);
    let gate_for_step = gate.clone();
    Stepper::new()
        .non_linear(true)
        .step(
            Step::new(lit!("Account"))
                .supporting_text(lit!("Complete this step to continue"))
                .content(move || {
                    let gate = gate_for_step.clone();
                    Button::new(lit!("Mark step complete"))
                        .variant(ButtonVariant::Tinted)
                        .on_activate_fn(move |_ctx| gate.set(true))
                })
                .complete_when(gate),
        )
        .step(Step::new(lit!("Preferences")).optional(true).content(|| {
            TextWidget::new(lit!("This step is optional — Skip is offered."))
                .style(TextStyleRole::Body)
        }))
        .step(Step::new(lit!("Review")).content(|| {
            TextWidget::new(lit!("All set — press Finish.")).style(TextStyleRole::Body)
        }))
        .help(lit!("Help"), |_ctx, _ctrl| {})
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let toolbar = section(
        ctx,
        lit!("Toolbar"),
        Toolbar::new().child(
            HStack::new()
                .spacing(6.0)
                .child(Button::new(tr!(demo_new())).variant(ButtonVariant::Ghost))
                .child(Button::new(tr!(demo_open())).variant(ButtonVariant::Ghost))
                .child(Button::new(tr!(demo_save())).variant(ButtonVariant::Ghost)),
        ),
    );
    let status_bar = section(
        ctx,
        lit!("StatusBar"),
        StatusBar::new().child(
            TextWidget::new(tr!(chr_status()))
                .style(TextStyleRole::Tiny)
                .color(TextRole::Secondary),
        ),
    );
    let banners = section(
        ctx,
        lit!("Banner"),
        VStack::new()
            .spacing(8.0)
            .child(
                Banner::info(tr!(chr_banner_info_title())).description(tr!(chr_banner_info_body())),
            )
            .child(
                Banner::success(tr!(chr_banner_success_title()))
                    .description(tr!(chr_banner_success_body())),
            )
            .child(
                Banner::warning(tr!(chr_banner_warning_title()))
                    .description(tr!(chr_banner_warning_body())),
            )
            .child(
                Banner::error(tr!(chr_banner_error_title()))
                    .description(tr!(chr_banner_error_body())),
            ),
    );
    let breadcrumb = section(
        ctx,
        lit!("Breadcrumb"),
        Breadcrumb::new()
            .item(BreadcrumbItem::new(tr!(chr_breadcrumb_home())))
            .item(BreadcrumbItem::new(tr!(chr_breadcrumb_docs())))
            .item(BreadcrumbItem::new(tr!(chr_breadcrumb_bastyde())))
            .item(BreadcrumbItem::current(tr!(chr_breadcrumb_current()))),
    );
    let wizard = section(ctx, lit!("Wizard"), make_wizard());
    let stepper = section(
        ctx,
        lit!("Stepper (embedded)"),
        FixedSize::new().bind_height(220.0).child(make_stepper()),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(toolbar)
            .add_child(status_bar)
            .add_child(banners)
            .add_child(breadcrumb)
            .add_child(wizard)
            .add_child(stepper),
    )
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // Toolbar/StatusBar wrap a single child via .child(impl Widget).
    // Banner family takes title in ctor, description chained — fits
    // bati! Type::ctor(args) { method: value }. Wizard takes nested
    // steps with closures — pre-register.
    let wizard_widget = ctx.add(make_wizard());
    let stepper_widget = ctx.add(FixedSize::new().bind_height(220.0).child(make_stepper()));

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_chrome_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_chrome_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Toolbar")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Toolbar {
                    HStack {
                        spacing: 6.0
                        Button::new(tr!(demo_new())) {
                            variant: ButtonVariant::Ghost
                        }
                        Button::new(tr!(demo_open())) {
                            variant: ButtonVariant::Ghost
                        }
                        Button::new(tr!(demo_save())) {
                            variant: ButtonVariant::Ghost
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("StatusBar")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                StatusBar {
                    TextWidget::new(tr!(chr_status())) {
                        style: TextStyleRole::Tiny
                        color: TextRole::Secondary
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Banner")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 8.0
                    Banner::info(tr!(chr_banner_info_title())) {
                        description: tr!(chr_banner_info_body())
                    }
                    Banner::success(tr!(chr_banner_success_title())) {
                        description: tr!(chr_banner_success_body())
                    }
                    Banner::warning(tr!(chr_banner_warning_title())) {
                        description: tr!(chr_banner_warning_body())
                    }
                    Banner::error(tr!(chr_banner_error_title())) {
                        description: tr!(chr_banner_error_body())
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Breadcrumb")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Breadcrumb {
                    item: BreadcrumbItem::new(tr!(chr_breadcrumb_home()))
                    item: BreadcrumbItem::new(tr!(chr_breadcrumb_docs()))
                    item: BreadcrumbItem::new(tr!(chr_breadcrumb_bastyde()))
                    item: BreadcrumbItem::current(tr!(chr_breadcrumb_current()))
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Wizard")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ wizard_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Stepper (embedded)")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ stepper_widget }
            }
        }
    )
}
