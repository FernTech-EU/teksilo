//! Settings tab — ShortcutSettings (and PrivacySettings note).

use bastyde::prelude::*;
use bastyde::widgets::{Divider, Expand, Panel, ShortcutSettings, TextWidget, VStack};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_settings_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_settings_refs())
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    // Wrap in a Panel for visual delineation, and an
    // `Expand::horizontal()` so the widget claims the tab's full width
    // (without it, ShortcutSettings reports its natural row-content
    // width and hugs the leading edge). Height is unbounded — the
    // catalog's TabContent already scrolls vertically.
    let shortcuts = section(
        ctx,
        "ShortcutSettings",
        Panel::new()
            .background(SurfaceRole::Raised)
            .border_color(BorderRole::Default)
            .border_width(1.0)
            .corner_radius(8.0)
            .padding(16.0)
            .child(
                Expand::horizontal()
                    .respect_intrinsic()
                    .child(ShortcutSettings::new()),
            ),
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

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // bati! cannot express the parameterless `Expand::respect_intrinsic()`
    // builder method as a property — pre-build in Rust, drop the id in.
    let shortcut_body_id = ctx.add(
        Expand::horizontal()
            .respect_intrinsic()
            .child(ShortcutSettings::new()),
    );

    bati!(ctx => VStack {
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
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new_literal("ShortcutSettings") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Panel {
                    background: SurfaceRole::Raised
                    border_color: BorderRole::Default
                    border_width: 1.0
                    corner_radius: 8.0
                    padding: 16.0
                    child_id: shortcut_body_id
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
