//! Visuals tab — RectWidget, TextWidget, IconWidget, ImageWidget,
//! ImageMaskShape, TwistArrow, Panel.

use fern_ui::prelude::*;
use fern_ui::tokens::CornerRadius;
use fern_ui::widgets::primitives::{ImageMaskShape, TwistArrow};
use fern_ui::widgets::{
    Divider, FixedSize, HStack, IconWidget, ImageFit, ImageWidget, Panel, RectWidget, TextWidget,
    VStack,
};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_visuals_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_visuals_refs())
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let rect = section(
        ctx,
        "RectWidget",
        FixedSize::new()
            .bind_width(220.0_f32)
            .bind_height(36.0_f32)
            .child(
                RectWidget::new()
                    .background(SurfaceRole::AccentSubtle)
                    .border_color(BorderRole::Strong)
                    .border_width(1.0)
                    .corner_radius(CornerRadius::uniform(6.0)),
            ),
    );
    let text = section(
        ctx,
        "TextWidget",
        VStack::new()
            .spacing(4.0)
            .child(TextWidget::new(tr!(vis_text_body())).style(TextStyleRole::Body))
            .child(
                TextWidget::new(tr!(vis_text_bold()))
                    .style(TextStyleRole::BodyBold)
                    .color(TextRole::Primary),
            )
            .child(
                TextWidget::new(tr!(vis_text_small()))
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
            )
            .child(
                TextWidget::new(tr!(vis_text_tiny()))
                    .style(TextStyleRole::Tiny)
                    .color(TextRole::Disabled),
            ),
    );
    let icon = section(
        ctx,
        "IconWidget",
        HStack::new()
            .spacing(12.0)
            .child(IconWidget::checkmark(20.0))
            .child(IconWidget::chevron_right(20.0))
            .child(IconWidget::chevron_down(20.0))
            .child(IconWidget::chevron_left(20.0))
            .child(IconWidget::chevron_up(20.0)),
    );
    let star_icon = fern_ui::res!("resources/icons/star.png");
    let image = section(
        ctx,
        "ImageWidget",
        HStack::new()
            .spacing(12.0)
            .child(
                ImageWidget::new(star_icon)
                    .size(48.0, 48.0)
                    .fit(ImageFit::Contain)
                    .alt(tr!(vis_image_alt_1()).resolve_now()),
            )
            .child(
                ImageWidget::new(star_icon)
                    .size(48.0, 48.0)
                    .mask(ImageMaskShape::Circle)
                    .alt(tr!(vis_image_alt_2()).resolve_now()),
            )
            .child(
                ImageWidget::new(star_icon)
                    .size(48.0, 48.0)
                    .mask(ImageMaskShape::RoundedSquare(0.25))
                    .alt(tr!(vis_image_alt_3()).resolve_now()),
            ),
    );
    let twist_expanded = ctx.signal(true);
    let twist_classic = twist_expanded.clone();
    let twist = section(
        ctx,
        "TwistArrow",
        HStack::new()
            .spacing(8.0)
            .child(TwistArrow::new(16.0, true, true).on_click(move || {
                twist_classic.set(!twist_classic.get());
            }))
            .child(TwistArrow::new(16.0, true, false).on_click(|| {}))
            .child(TwistArrow::new(16.0, false, false).on_click(|| {})),
    );
    let panel_demo = section(
        ctx,
        "Panel (visual primitive sample)",
        Panel::new()
            .background(SurfaceRole::Raised)
            .border_color(BorderRole::Default)
            .border_width(1.0)
            .corner_radius(8.0)
            .padding(12.0)
            .child(TextWidget::new(tr!(vis_panel_body())).style(TextStyleRole::Small)),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(rect)
            .add_child(text)
            .add_child(icon)
            .add_child(image)
            .add_child(twist)
            .add_child(panel_demo),
    )
}

pub fn fern(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let star_icon = fern_ui::res!("resources/icons/star.png");
    let twist_expanded = ctx.signal(true);
    let twist_for_click = twist_expanded.clone();

    fern!(ctx =>
        VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_visuals_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_visuals_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider {}

            VStack {
                spacing: 6.0
                TextWidget::new_literal("RectWidget") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 220.0_f32
                    bind_height: 36.0_f32
                    RectWidget {
                        background: SurfaceRole::AccentSubtle
                        border_color: BorderRole::Strong
                        border_width: 1.0
                        corner_radius: CornerRadius::uniform(6.0)
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("TextWidget") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 4.0
                    TextWidget::new(tr!(vis_text_body())) { style: TextStyleRole::Body }
                    TextWidget::new(tr!(vis_text_bold())) {
                        style: TextStyleRole::BodyBold
                        color: TextRole::Primary
                    }
                    TextWidget::new(tr!(vis_text_small())) {
                        style: TextStyleRole::Small
                        color: TextRole::Secondary
                    }
                    TextWidget::new(tr!(vis_text_tiny())) {
                        style: TextStyleRole::Tiny
                        color: TextRole::Disabled
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("IconWidget") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 12.0
                    IconWidget::checkmark(20.0)
                    IconWidget::chevron_right(20.0)
                    IconWidget::chevron_down(20.0)
                    IconWidget::chevron_left(20.0)
                    IconWidget::chevron_up(20.0)
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("ImageWidget") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 12.0
                    ImageWidget::new(star_icon) {
                        width: 48.0
                        height: 48.0
                        fit: ImageFit::Contain
                        alt: tr!(vis_image_alt_1()).resolve_now()
                    }
                    ImageWidget::new(star_icon) {
                        width: 48.0
                        height: 48.0
                        mask: ImageMaskShape::Circle
                        alt: tr!(vis_image_alt_2()).resolve_now()
                    }
                    ImageWidget::new(star_icon) {
                        width: 48.0
                        height: 48.0
                        mask: ImageMaskShape::RoundedSquare(0.25)
                        alt: tr!(vis_image_alt_3()).resolve_now()
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("TwistArrow") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 8.0
                    TwistArrow::new(16.0, true, true) {
                        on_click: move || { twist_for_click.set(!twist_for_click.get()); }
                    }
                    TwistArrow::new(16.0, true, false) { on_click: || {} }
                    TwistArrow::new(16.0, false, false) { on_click: || {} }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Panel (visual primitive sample)") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Panel {
                    background: SurfaceRole::Raised
                    border_color: BorderRole::Default
                    border_width: 1.0
                    corner_radius: 8.0
                    padding: 12.0
                    TextWidget::new(tr!(vis_panel_body())) { style: TextStyleRole::Small }
                }
            }
        }
    )
}
