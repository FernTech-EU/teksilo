//! Settings tab — ShortcutSettings (and PrivacySettings note).

use fern_ui::prelude::*;
use fern_ui::widgets::{Divider, FixedSize, ShortcutSettings, TextWidget, VStack};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_settings_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_settings_refs())
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let shortcuts = section(
        ctx,
        "ShortcutSettings",
        FixedSize::new()
            .bind_height(280.0_f32)
            .child(ShortcutSettings::new()),
    );
    let privacy = section(
        ctx,
        "PrivacySettings",
        TextWidget::new(tr!(set_privacy_note()))
            .style(TextStyleRole::Small)
            .color(TextRole::Secondary),
    );
    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(shortcuts)
            .add_child(privacy),
    )
}

pub fn fern(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    fern!(ctx =>
        VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_settings_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_settings_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider {}

            VStack {
                spacing: 6.0
                TextWidget::new_literal("ShortcutSettings") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_height: 280.0_f32
                    ShortcutSettings {}
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("PrivacySettings") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                TextWidget::new(tr!(set_privacy_note())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
        }
    )
}
