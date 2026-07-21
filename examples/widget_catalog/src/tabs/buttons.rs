// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Buttons tab — Button (×3 variants × states), IconButton, CommandLinkButton,
//! PopoverButton, PopoverIconButton, SplitButton.

use bastyde::prelude::*;
use bastyde::widgets::{
    Button, ButtonVariant, CommandLinkButton, Divider, IconButton, IconButtonSize, IconLocation,
    IconWidget, MaxSize, MenuItem, PopoverButton, PopoverIconButton, SplitButton, TextWidget,
    VStack, Wrap,
};

use crate::shared::{FIELD_MAX_WIDTH, Signals, demo_row, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_buttons_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_buttons_refs())
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let variants = section(
        ctx,
        tr!(btn_heading_variants()),
        demo_row(8.0)
            .child(
                Button::new(tr!(btn_default()))
                    .variant(ButtonVariant::Filled)
                    .on_activate_fn(|_| println!("Default")),
            )
            .child(
                Button::new(tr!(btn_regular()))
                    .variant(ButtonVariant::Plain)
                    .on_activate_fn(|_| println!("Regular")),
            )
            .child(
                Button::new(tr!(btn_flat()))
                    .variant(ButtonVariant::Ghost)
                    .on_activate_fn(|_| println!("Flat")),
            ),
    );
    let states = section(
        ctx,
        tr!(btn_heading_disabled()),
        demo_row(8.0)
            .child(
                Button::new(tr!(btn_default()))
                    .variant(ButtonVariant::Filled)
                    .enabled(false),
            )
            .child(
                Button::new(tr!(btn_regular()))
                    .variant(ButtonVariant::Plain)
                    .enabled(false),
            )
            .child(
                Button::new(tr!(btn_flat()))
                    .variant(ButtonVariant::Ghost)
                    .enabled(false),
            ),
    );
    let with_icon = section(
        ctx,
        tr!(btn_heading_with_icon()),
        demo_row(8.0)
            .child(
                Button::new(tr!(btn_confirm_label()))
                    .icon(IconWidget::checkmark(16.0), IconLocation::Leading)
                    .variant(ButtonVariant::Filled),
            )
            .child(
                Button::new(tr!(demo_next()))
                    .icon(IconWidget::chevron_right(16.0), IconLocation::Trailing)
                    .variant(ButtonVariant::Plain),
            ),
    );
    let icon_btns = section(
        ctx,
        lit!("IconButton"),
        demo_row(8.0)
            .child(IconButton::add().tooltip(tr!(demo_new())))
            .child(IconButton::copy().tooltip(tr!(demo_copy())))
            .child(IconButton::clear().tooltip(tr!(demo_find())))
            .child(IconButton::search().tooltip(tr!(demo_find())))
            .child(IconButton::expand().tooltip(tr!(demo_open()))),
    );
    // All five IconButtonSize steps, same glyph so the size delta reads
    // clearly: Compact 22 · Default 24 · Toolbar 30 · Large 40 · Hero 50.
    let icon_sizes = section(
        ctx,
        lit!("IconButton — sizes"),
        demo_row(12.0)
            .child(
                IconButton::search()
                    .size(IconButtonSize::Compact)
                    .tooltip(lit!("Compact · 22dp")),
            )
            .child(
                IconButton::search()
                    .size(IconButtonSize::Default)
                    .tooltip(lit!("Default · 24dp")),
            )
            .child(
                IconButton::search()
                    .size(IconButtonSize::Toolbar)
                    .tooltip(lit!("Toolbar · 30dp")),
            )
            .child(
                IconButton::search()
                    .size(IconButtonSize::Large)
                    .tooltip(lit!("Large · 40dp")),
            )
            .child(
                IconButton::search()
                    .size(IconButtonSize::Hero)
                    .tooltip(lit!("Hero · 50dp")),
            ),
    );
    let cmd_link = section(
        ctx,
        lit!("CommandLinkButton"),
        // CommandLinkButton is a rigid composite (see its `layout_response`)
        // and its multi-line description is measured unwrapped during the
        // enclosing HStack's intrinsic-width query, so it reports a
        // natural width wider than a narrow viewport regardless of the
        // proposal it's given. Cap + clip it like the other narrow-viewport
        // demos in this catalog (see `FIELD_MAX_WIDTH`'s doc comment).
        MaxSize::width(FIELD_MAX_WIDTH).child(
            VStack::new()
                .spacing(6.0)
                .child(
                    CommandLinkButton::new(tr!(btn_cmdlink_signin_title()))
                        .description(tr!(btn_cmdlink_signin_desc())),
                )
                .child(
                    CommandLinkButton::new(tr!(btn_cmdlink_signup_title()))
                        .description(tr!(btn_cmdlink_signup_desc())),
                ),
        ),
    );
    let popover_btn = section(
        ctx,
        lit!("PopoverButton"),
        PopoverButton::new(Button::new(tr!(btn_popover_trigger()))).content(
            VStack::new()
                .spacing(4.0)
                .child(TextWidget::new(tr!(btn_popover_title())).style(TextStyleRole::BodyBold))
                .child(TextWidget::new(tr!(btn_popover_body())).style(TextStyleRole::Small)),
        ),
    );
    let popover_icon = section(
        ctx,
        lit!("PopoverIconButton"),
        PopoverIconButton::new(IconButton::add())
            .content(TextWidget::new(tr!(btn_popover_icon_body())).style(TextStyleRole::Small)),
    );
    let split = section(
        ctx,
        lit!("SplitButton"),
        SplitButton::new()
            .item(MenuItem::new(tr!(demo_save())))
            .item(MenuItem::new(tr!(buttons_save_as())))
            .separator()
            .item(MenuItem::new(tr!(btn_export_sample())))
            .variant(ButtonVariant::Filled),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(variants)
            .add_child(states)
            .add_child(with_icon)
            .add_child(icon_btns)
            .add_child(icon_sizes)
            .add_child(cmd_link)
            .add_child(popover_btn)
            .add_child(popover_icon)
            .add_child(split),
    )
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // Pre-register multi-arg / chained construction that bati! property
    // syntax can't express directly. Each gets its own WidgetId so the
    // bati! body can splice via `#{ id }`.
    let icon_btn_confirm = ctx.add(
        Button::new(tr!(btn_confirm_label()))
            .icon(IconWidget::checkmark(16.0), IconLocation::Leading)
            .variant(ButtonVariant::Filled),
    );
    let icon_btn_next = ctx.add(
        Button::new(tr!(demo_next()))
            .icon(IconWidget::chevron_right(16.0), IconLocation::Trailing)
            .variant(ButtonVariant::Plain),
    );
    let popover_btn_widget = ctx.add(
        PopoverButton::new(Button::new(tr!(btn_popover_trigger()))).content(
            VStack::new()
                .spacing(4.0)
                .child(TextWidget::new(tr!(btn_popover_title())).style(TextStyleRole::BodyBold))
                .child(TextWidget::new(tr!(btn_popover_body())).style(TextStyleRole::Small)),
        ),
    );
    let popover_icon_widget = ctx.add(
        PopoverIconButton::new(IconButton::add())
            .content(TextWidget::new(tr!(btn_popover_icon_body())).style(TextStyleRole::Small)),
    );

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_buttons_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_buttons_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(btn_heading_variants())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Wrap {
                    spacing: 8.0
                    line_spacing: 8.0
                    Button::new(tr!(btn_default())) {
                        variant: ButtonVariant::Filled
                        on_activate_fn: |_| println!("Default")
                    }
                    Button::new(tr!(btn_regular())) {
                        variant: ButtonVariant::Plain
                        on_activate_fn: |_| println!("Regular")
                    }
                    Button::new(tr!(btn_flat())) {
                        variant: ButtonVariant::Ghost
                        on_activate_fn: |_| println!("Flat")
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(btn_heading_disabled())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Wrap {
                    spacing: 8.0
                    line_spacing: 8.0
                    Button::new(tr!(btn_default())) {
                        variant: ButtonVariant::Filled
                        enabled: false
                    }
                    Button::new(tr!(btn_regular())) {
                        variant: ButtonVariant::Plain
                        enabled: false
                    }
                    Button::new(tr!(btn_flat())) {
                        variant: ButtonVariant::Ghost
                        enabled: false
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(btn_heading_with_icon())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Wrap {
                    spacing: 8.0
                    line_spacing: 8.0
                    #{ icon_btn_confirm }
                    #{ icon_btn_next }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("IconButton")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Wrap {
                    spacing: 8.0
                    line_spacing: 8.0
                    IconButton::add() {
                        tooltip: tr!(demo_new())
                    }
                    IconButton::copy() {
                        tooltip: tr!(demo_copy())
                    }
                    IconButton::clear() {
                        tooltip: tr!(demo_find())
                    }
                    IconButton::search() {
                        tooltip: tr!(demo_find())
                    }
                    IconButton::expand() {
                        tooltip: tr!(demo_open())
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("IconButton — sizes")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Wrap {
                    spacing: 12.0
                    line_spacing: 12.0
                    IconButton::search() {
                        size: IconButtonSize::Compact
                        tooltip: lit!("Compact · 22dp")
                    }
                    IconButton::search() {
                        size: IconButtonSize::Default
                        tooltip: lit!("Default · 24dp")
                    }
                    IconButton::search() {
                        size: IconButtonSize::Toolbar
                        tooltip: lit!("Toolbar · 30dp")
                    }
                    IconButton::search() {
                        size: IconButtonSize::Large
                        tooltip: lit!("Large · 40dp")
                    }
                    IconButton::search() {
                        size: IconButtonSize::Hero
                        tooltip: lit!("Hero · 50dp")
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("CommandLinkButton")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                MaxSize::width(FIELD_MAX_WIDTH) {
                    VStack {
                        spacing: 6.0
                        CommandLinkButton::new(tr!(btn_cmdlink_signin_title())) {
                            description: tr!(btn_cmdlink_signin_desc())
                        }
                        CommandLinkButton::new(tr!(btn_cmdlink_signup_title())) {
                            description: tr!(btn_cmdlink_signup_desc())
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("PopoverButton")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ popover_btn_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("PopoverIconButton")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ popover_icon_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("SplitButton")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                SplitButton {
                    item: MenuItem::new(tr!(demo_save()))
                    item: MenuItem::new(tr!(buttons_save_as()))
                    variant: ButtonVariant::Filled
                }
            }
        }
    )
}
