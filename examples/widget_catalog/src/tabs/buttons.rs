//! Buttons tab — Button (×3 variants × states), IconButton, CommandLinkButton,
//! PopoverButton, PopoverIconButton, SplitButton.

use bastyde::prelude::*;
use bastyde::widgets::{
    Button, ButtonVariant, CommandLinkButton, Divider, HStack, IconButton, IconLocation,
    IconWidget, MenuItem, Panel, PopoverButton, PopoverIconButton, SplitButton, TextWidget, VStack,
};

fn popover_surface(content: impl Widget + 'static) -> impl Widget + 'static {
    Panel::new()
        .background(SurfaceRole::Main)
        .border_color(BorderRole::Default)
        .corner_radius(8.0_f32)
        .padding(12.0_f32)
        .child(content)
}

use crate::shared::{Signals, section, tab_header};

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
        "Button — variants",
        HStack::new()
            .spacing(8.0)
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
        "Button — disabled state",
        HStack::new()
            .spacing(8.0)
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
        "Button — with icon",
        HStack::new()
            .spacing(8.0)
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
        "IconButton",
        HStack::new()
            .spacing(8.0)
            .child(IconButton::add().tooltip(tr!(demo_new())))
            .child(IconButton::copy().tooltip(tr!(demo_copy())))
            .child(IconButton::clear().tooltip(tr!(demo_find())))
            .child(IconButton::search().tooltip(tr!(demo_find())))
            .child(IconButton::expand().tooltip(tr!(demo_open()))),
    );
    let cmd_link = section(
        ctx,
        "CommandLinkButton",
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
    );
    let popover_btn = section(
        ctx,
        "PopoverButton",
        PopoverButton::new(Button::new(tr!(btn_popover_trigger()))).content(popover_surface(
            VStack::new()
                .spacing(4.0)
                .child(TextWidget::new(tr!(btn_popover_title())).style(TextStyleRole::BodyBold))
                .child(TextWidget::new(tr!(btn_popover_body())).style(TextStyleRole::Small)),
        )),
    );
    let popover_icon = section(
        ctx,
        "PopoverIconButton",
        PopoverIconButton::new(IconButton::add()).content(popover_surface(
            TextWidget::new(tr!(btn_popover_icon_body())).style(TextStyleRole::Small),
        )),
    );
    let split = section(
        ctx,
        "SplitButton",
        SplitButton::new()
            .item(MenuItem::new(tr!(demo_save())))
            .item(MenuItem::new(tr!(buttons_save_as())))
            .separator()
            .item(MenuItem::new_literal("Export"))
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
        PopoverButton::new(Button::new(tr!(btn_popover_trigger()))).content(popover_surface(
            VStack::new()
                .spacing(4.0)
                .child(TextWidget::new(tr!(btn_popover_title())).style(TextStyleRole::BodyBold))
                .child(TextWidget::new(tr!(btn_popover_body())).style(TextStyleRole::Small)),
        )),
    );
    let popover_icon_widget = ctx.add(PopoverIconButton::new(IconButton::add()).content(
        popover_surface(TextWidget::new(tr!(btn_popover_icon_body())).style(TextStyleRole::Small)),
    ));

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
                TextWidget::new_literal("Button — variants") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 8.0
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
                TextWidget::new_literal("Button — disabled state") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 8.0
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
                TextWidget::new_literal("Button — with icon") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 8.0
                    #{ icon_btn_confirm }
                    #{ icon_btn_next }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("IconButton") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 8.0
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
                TextWidget::new_literal("CommandLinkButton") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
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

            VStack {
                spacing: 6.0
                TextWidget::new_literal("PopoverButton") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ popover_btn_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("PopoverIconButton") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ popover_icon_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("SplitButton") {
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
