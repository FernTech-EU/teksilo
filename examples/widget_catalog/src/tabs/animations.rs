// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Animations tab — every wrapper widget under
//! `teksilo::widgets::animations::*`.

use std::time::Duration;

use teksilo::prelude::*;
use teksilo::widgets::{
    Blur, Button, Collapse, Crossfade, Cycle, Divider, Fade, FixedSize, Panel, Pulse, Rotate,
    Scale, Shake, Slide, SlideEdge, SmoothSize, TextWidget, Toggle, VStack, Wrap,
};

use crate::shared::{Signals, color_cell, demo_row, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_animations_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_animations_refs())
}

fn drive_box(label: LocalizedString, signal: Signal<bool>) -> impl Widget + 'static {
    Toggle::new(signal).label(label)
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let fade_visible = ctx.signal(true);
    let fade = section(
        ctx,
        lit!("Fade"),
        VStack::new()
            .spacing(6.0)
            .child(drive_box(tr!(anim_visible()), fade_visible.clone()))
            .child(Fade::new(fade_visible).child(color_cell(SurfaceRole::AccentSubtle, "fading"))),
    );
    let pulse = section(
        ctx,
        lit!("Pulse"),
        Pulse::opacity(0.3, 1.0)
            .period(Duration::from_millis(900))
            .child(color_cell(SurfaceRole::Raised, "REC")),
    );
    let cycle = section(
        ctx,
        lit!("Cycle"),
        Cycle::new()
            .period(Duration::from_millis(1500))
            .child(TextWidget::new(tr!(anim_tip_1())).style(TextStyleRole::Body))
            .child(TextWidget::new(tr!(anim_tip_2())).style(TextStyleRole::Body))
            .child(TextWidget::new(tr!(anim_tip_3())).style(TextStyleRole::Body)),
    );
    let crossfade_key = ctx.signal(0_u32);
    let crossfade_btn = Button::new(tr!(anim_crossfade_next())).on_activate_fn({
        let k = crossfade_key.clone();
        move |_| k.set(k.get() + 1)
    });
    let crossfade = section(
        ctx,
        lit!("Crossfade"),
        VStack::new()
            .spacing(6.0)
            .child(crossfade_btn)
            .child(Crossfade::new(crossfade_key, |_k: &u32| {
                Box::new(color_cell(SurfaceRole::Sunken, "v")) as Box<dyn Widget>
            })),
    );
    let collapse_open = ctx.signal(true);
    let collapse = section(
        ctx,
        lit!("Collapse"),
        VStack::new()
            .spacing(6.0)
            .child(drive_box(tr!(anim_expanded()), collapse_open.clone()))
            .child(
                Collapse::new(collapse_open).child(
                    Panel::new()
                        .background(SurfaceRole::Raised)
                        .padding(12.0)
                        .child(
                            TextWidget::new(tr!(anim_collapse_body())).style(TextStyleRole::Body),
                        ),
                ),
            ),
    );
    let smooth_size = section(
        ctx,
        lit!("SmoothSize"),
        SmoothSize::new().child(
            Panel::new()
                .background(SurfaceRole::AccentSubtle)
                .padding(12.0)
                .child(TextWidget::new(tr!(anim_smooth_body())).style(TextStyleRole::Body)),
        ),
    );
    let slide_visible = ctx.signal(true);
    let slide = section(
        ctx,
        lit!("Slide"),
        VStack::new()
            .spacing(6.0)
            .child(drive_box(tr!(anim_visible()), slide_visible.clone()))
            .child(
                FixedSize::new().width(280.0_f32).height(40.0_f32).child(
                    Slide::new(slide_visible)
                        .from(SlideEdge::Trailing)
                        .child(color_cell(SurfaceRole::AltRow, "snackbar")),
                ),
            ),
    );
    let shake_trigger = ctx.signal(0_u32);
    let shake_btn = Button::new(tr!(anim_shake())).on_activate_fn({
        let t = shake_trigger.clone();
        move |_| t.set(t.get() + 1)
    });
    let shake = section(
        ctx,
        lit!("Shake"),
        demo_row(8.0)
            .child(shake_btn)
            .child(Shake::new(shake_trigger).child(color_cell(SurfaceRole::AccentSubtle, "input"))),
    );
    let scale_visible = ctx.signal(true);
    let scale = section(
        ctx,
        lit!("Scale"),
        VStack::new()
            .spacing(6.0)
            .child(drive_box(tr!(anim_visible()), scale_visible.clone()))
            .child(Scale::new(scale_visible).child(color_cell(SurfaceRole::Raised, "scaling"))),
    );
    let rotate_angle: Signal<f32> = ctx.signal(0.0_f32);
    let rotate_btn = Button::new(tr!(anim_rotate())).on_activate_fn({
        let a = rotate_angle.clone();
        move |_| a.set(a.get() + 45.0_f32.to_radians())
    });
    let rotate = section(
        ctx,
        lit!("Rotate"),
        demo_row(8.0).child(rotate_btn).child(
            FixedSize::new()
                .width(60.0_f32)
                .height(60.0_f32)
                .child(Rotate::new(rotate_angle).child(color_cell(SurfaceRole::Sunken, "↻"))),
        ),
    );
    let blur_radius: Signal<f32> = ctx.signal(0.0_f32);
    let blur_btn = Button::new(tr!(anim_blur_toggle())).on_activate_fn({
        let r = blur_radius.clone();
        move |_| r.set(if r.get() > 0.0 { 0.0 } else { 8.0 })
    });
    let blur = section(
        ctx,
        lit!("Blur"),
        VStack::new().spacing(6.0).child(blur_btn).child(
            Blur::new(blur_radius).child(
                Panel::new()
                    .background(SurfaceRole::AccentSubtle)
                    .padding(16.0)
                    .child(TextWidget::new(tr!(anim_blur_body())).style(TextStyleRole::Body)),
            ),
        ),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(fade)
            .add_child(pulse)
            .add_child(cycle)
            .add_child(crossfade)
            .add_child(collapse)
            .add_child(smooth_size)
            .add_child(slide)
            .add_child(shake)
            .add_child(scale)
            .add_child(rotate)
            .add_child(blur),
    )
}

pub fn teksu(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // Crossfade takes a builder closure as its second ctor arg —
    // pre-register. Pulse's `.opacity(min, max)` is the constructor
    // (so works in `Pulse::opacity(min, max) {}` form). Cycle's
    // `Cycle::new().period(...).child(...)` fits inline. Other
    // animations (Fade/Collapse/Slide/Scale/Rotate/Blur/SmoothSize)
    // wrap a child via `.child(impl Widget)` — fits teksu! children.
    let fade_visible = ctx.signal(true);
    let fade_for_drive = fade_visible.clone();
    let crossfade_key = ctx.signal(0_u32);
    let crossfade_key_for_btn = crossfade_key.clone();
    let crossfade_widget = ctx.add(Crossfade::new(crossfade_key, |_k: &u32| {
        Box::new(color_cell(SurfaceRole::AltRow, "v")) as Box<dyn Widget>
    }));
    let collapse_open = ctx.signal(true);
    let collapse_for_drive = collapse_open.clone();
    let slide_visible = ctx.signal(true);
    let slide_for_drive = slide_visible.clone();
    let shake_trigger = ctx.signal(0_u32);
    let shake_for_btn = shake_trigger.clone();
    let scale_visible = ctx.signal(true);
    let scale_for_drive = scale_visible.clone();
    let rotate_angle: Signal<f32> = ctx.signal(0.0_f32);
    let rotate_for_btn = rotate_angle.clone();
    let blur_radius: Signal<f32> = ctx.signal(0.0_f32);
    let blur_for_btn = blur_radius.clone();

    teksu!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_animations_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_animations_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Fade")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    child: drive_box(tr!(anim_visible()), fade_for_drive)
                    Fade::new(fade_visible) {
                        child: color_cell(SurfaceRole::AccentSubtle, "fading")
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Pulse")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Pulse::opacity(0.3, 1.0) {
                    period: Duration::from_millis(900)
                    child: color_cell(SurfaceRole::Raised, "REC")
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Cycle")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Cycle {
                    period: Duration::from_millis(1500)
                    TextWidget::new(tr!(anim_tip_1())) {
                        style: TextStyleRole::Body
                    }
                    TextWidget::new(tr!(anim_tip_2())) {
                        style: TextStyleRole::Body
                    }
                    TextWidget::new(tr!(anim_tip_3())) {
                        style: TextStyleRole::Body
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Crossfade")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    Button::new(tr!(anim_crossfade_next())) {
                        on_activate_fn: move |_| crossfade_key_for_btn.set(crossfade_key_for_btn.get() + 1)
                    }
                    #{ crossfade_widget }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Collapse")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    child: drive_box(tr!(anim_expanded()), collapse_for_drive)
                    Collapse::new(collapse_open) {
                        Panel {
                            background: SurfaceRole::Raised
                            padding: 12.0
                            TextWidget::new(tr!(anim_collapse_body())) {
                                style: TextStyleRole::Body
                            }
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("SmoothSize")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                SmoothSize {
                    Panel {
                        background: SurfaceRole::AccentSubtle
                        padding: 12.0
                        TextWidget::new(tr!(anim_smooth_body())) {
                            style: TextStyleRole::Body
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Slide")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    child: drive_box(tr!(anim_visible()), slide_for_drive)
                    FixedSize {
                        width: 280.0_f32
                        height: 40.0_f32
                        Slide::new(slide_visible) {
                            from: SlideEdge::Trailing
                            child: color_cell(SurfaceRole::Sunken, "snackbar")
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Shake")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Wrap {
                    spacing: 8.0
                    line_spacing: 8.0
                    Button::new(tr!(anim_shake())) {
                        on_activate_fn: move |_| shake_for_btn.set(shake_for_btn.get() + 1)
                    }
                    Shake::new(shake_trigger) {
                        child: color_cell(SurfaceRole::AltRow, "input")
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Scale")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    child: drive_box(tr!(anim_visible()), scale_for_drive)
                    Scale::new(scale_visible) {
                        child: color_cell(SurfaceRole::AccentSubtle, "scaling")
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Rotate")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Wrap {
                    spacing: 8.0
                    line_spacing: 8.0
                    Button::new(tr!(anim_rotate())) {
                        on_activate_fn: move |_| rotate_for_btn.set(rotate_for_btn.get() + 45.0_f32.to_radians())
                    }
                    FixedSize {
                        width: 60.0_f32
                        height: 60.0_f32
                        Rotate::new(rotate_angle) {
                            child: color_cell(SurfaceRole::Raised, "↻")
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Blur")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    Button::new(tr!(anim_blur_toggle())) {
                        on_activate_fn: move |_| blur_for_btn.set(if blur_for_btn.get() > 0.0 { 0.0 } else { 8.0 })
                    }
                    Blur::new(blur_radius) {
                        Panel {
                            background: SurfaceRole::AccentSubtle
                            padding: 16.0
                            TextWidget::new(tr!(anim_blur_body())) {
                                style: TextStyleRole::Body
                            }
                        }
                    }
                }
            }
        }
    )
}
