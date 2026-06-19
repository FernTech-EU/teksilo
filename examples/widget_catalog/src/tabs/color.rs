// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Color tab — HexColorInput, ColorEdit, ColorPicker.

use bastyde::prelude::*;
use bastyde::widgets::{
    ColorEdit, ColorPicker, Divider, HexColorInput, MaxSize, TextWidget, VStack,
};

use crate::shared::{FIELD_MAX_WIDTH, Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_color_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_color_refs())
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let hex_color = ctx.signal(Color::from_hex("#CC6633"));
    let edit_color = ctx.signal(Color::from_hex("#55AADD"));
    let pick_color = ctx.signal(Color::from_hex("#8844BB"));

    let hex = section(
        ctx,
        lit!("HexColorInput"),
        // Holds "#RRGGBB" (7 chars) — cap near that, not the free-text width.
        MaxSize::width(120.0).child(HexColorInput::new(hex_color).label(tr!(clr_brand_label()))),
    );
    let edit = section(
        ctx,
        lit!("ColorEdit"),
        MaxSize::width(FIELD_MAX_WIDTH).child(
            ColorEdit::new(edit_color)
                .alpha_enabled(true)
                .label(tr!(clr_accent_label())),
        ),
    );
    let picker = section(
        ctx,
        lit!("ColorPicker"),
        ColorPicker::new(pick_color)
            .alpha_enabled(true)
            .show_hsv_canvas(true)
            .show_hue_strip(true),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(hex)
            .add_child(edit)
            .add_child(picker),
    )
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let hex_color = ctx.signal(Color::from_hex("#CC6633"));
    let edit_color = ctx.signal(Color::from_hex("#55AADD"));
    let pick_color = ctx.signal(Color::from_hex("#8844BB"));

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_color_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_color_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("HexColorInput")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                MaxSize::width(120.0) {
                    HexColorInput::new(hex_color) {
                        label: tr!(clr_brand_label())
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("ColorEdit")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                MaxSize::width(FIELD_MAX_WIDTH) {
                    ColorEdit::new(edit_color) {
                        alpha_enabled: true
                        label: tr!(clr_accent_label())
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("ColorPicker")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                ColorPicker::new(pick_color) {
                    alpha_enabled: true
                    show_hsv_canvas: true
                    show_hue_strip: true
                }
            }
        }
    )
}
