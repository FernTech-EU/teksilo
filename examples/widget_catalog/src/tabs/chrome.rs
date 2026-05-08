//! Chrome tab — Toolbar, StatusBar, Banner, Breadcrumb, Wizard.

use fern_ui::prelude::*;
use fern_ui::widgets::{
    Banner, Breadcrumb, BreadcrumbItem, Button, ButtonVariant, Divider, HStack, StatusBar,
    TextWidget, Toolbar, VStack, Wizard, WizardStep,
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
        .step(WizardStep::new(tr!(chr_wizard_step1())).content(|| {
            TextWidget::new(tr!(chr_wizard_step1_body())).style(TextStyleRole::Body)
        }))
        .step(WizardStep::new(tr!(chr_wizard_step2())).content(|| {
            TextWidget::new(tr!(chr_wizard_step2_body())).style(TextStyleRole::Body)
        }))
        .step(WizardStep::new(tr!(chr_wizard_step3())).content(|| {
            TextWidget::new(tr!(chr_wizard_step3_body())).style(TextStyleRole::Body)
        }))
        .trigger(Button::new(tr!(chr_wizard_trigger())).style(ButtonVariant::Default))
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let toolbar = section(
        ctx,
        "Toolbar",
        Toolbar::new().child(
            HStack::new()
                .spacing(6.0)
                .child(Button::new(tr!(demo_new())).style(ButtonVariant::Flat))
                .child(Button::new(tr!(demo_open())).style(ButtonVariant::Flat))
                .child(Button::new(tr!(demo_save())).style(ButtonVariant::Flat)),
        ),
    );
    let status_bar = section(
        ctx,
        "StatusBar",
        StatusBar::new().child(
            TextWidget::new(tr!(chr_status()))
                .style(TextStyleRole::Tiny)
                .color(TextRole::Secondary),
        ),
    );
    let banners = section(
        ctx,
        "Banner",
        VStack::new()
            .spacing(8.0)
            .child(Banner::info(tr!(chr_banner_info_title())).description(tr!(chr_banner_info_body())))
            .child(Banner::success(tr!(chr_banner_success_title())).description(tr!(chr_banner_success_body())))
            .child(Banner::warning(tr!(chr_banner_warning_title())).description(tr!(chr_banner_warning_body())))
            .child(Banner::error(tr!(chr_banner_error_title())).description(tr!(chr_banner_error_body()))),
    );
    let breadcrumb = section(
        ctx,
        "Breadcrumb",
        Breadcrumb::new()
            .item(BreadcrumbItem::new(tr!(chr_breadcrumb_home())))
            .item(BreadcrumbItem::new(tr!(chr_breadcrumb_docs())))
            .item(BreadcrumbItem::new(tr!(chr_breadcrumb_fernui())))
            .item(BreadcrumbItem::current(tr!(chr_breadcrumb_current()))),
    );
    let wizard = section(ctx, "Wizard", make_wizard());

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(toolbar)
            .add_child(status_bar)
            .add_child(banners)
            .add_child(breadcrumb)
            .add_child(wizard),
    )
}

pub fn fern(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // Toolbar/StatusBar wrap a single child via .child(impl Widget).
    // Banner family takes title in ctor, description chained — fits
    // fern! Type::ctor(args) { method: value }. Wizard takes nested
    // steps with closures — pre-register.
    let wizard_widget = ctx.add(make_wizard());

    fern!(ctx =>
        VStack {
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
            Divider {}

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Toolbar") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Toolbar {
                    HStack {
                        spacing: 6.0
                        Button::new(tr!(demo_new())) { style: ButtonVariant::Flat }
                        Button::new(tr!(demo_open())) { style: ButtonVariant::Flat }
                        Button::new(tr!(demo_save())) { style: ButtonVariant::Flat }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("StatusBar") {
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
                TextWidget::new_literal("Banner") {
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
                TextWidget::new_literal("Breadcrumb") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Breadcrumb {
                    item: BreadcrumbItem::new(tr!(chr_breadcrumb_home()))
                    item: BreadcrumbItem::new(tr!(chr_breadcrumb_docs()))
                    item: BreadcrumbItem::new(tr!(chr_breadcrumb_fernui()))
                    item: BreadcrumbItem::current(tr!(chr_breadcrumb_current()))
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Wizard") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ wizard_widget }
            }
        }
    )
}
