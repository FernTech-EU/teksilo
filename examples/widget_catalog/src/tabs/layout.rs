//! Layout primitives tab — every container/spacing primitive in
//! `fern_widgets::primitives`.

use fern_ui::prelude::*;
use fern_ui::widgets::{
    AspectRatio, Badge, Button, Center, Divider, Expand, FixedSize, FormLayout, Grid, HStack,
    MasonryLayout, MaxSize, MinSize, Padding, Panel, RectWidget, Spacer, Switcher, TextWidget,
    TrackSize, VStack, Wrap, ZStack,
};

use crate::shared::{Signals, color_cell, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_layout_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_layout_refs())
}

fn masonry_tile(height: f32) -> impl Widget + 'static {
    FixedSize::new().bind_height(height).child(
        Panel::new()
            .background(SurfaceRole::AccentSubtle)
            .child(Center::new().child(TextWidget::new_literal("·").style(TextStyleRole::Small))),
    )
}

pub fn classic(ctx: &mut BuildContext, sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let hstack = section(
        ctx,
        "HStack",
        HStack::new()
            .spacing(8.0)
            .child(color_cell(SurfaceRole::AccentSubtle, "A"))
            .child(color_cell(SurfaceRole::Raised, "B"))
            .child(color_cell(SurfaceRole::Sunken, "C")),
    );
    let vstack = section(
        ctx,
        "VStack",
        VStack::new()
            .spacing(6.0)
            .child(color_cell(SurfaceRole::AltRow, "Top"))
            .child(color_cell(SurfaceRole::AccentSubtle, "Mid"))
            .child(color_cell(SurfaceRole::Raised, "Bot")),
    );
    let zstack = section(
        ctx,
        "ZStack",
        FixedSize::new()
            .bind_width(120.0_f32)
            .bind_height(60.0_f32)
            .child(
                ZStack::new()
                    .child(RectWidget::new().background(SurfaceRole::Sunken))
                    .child(
                        TextWidget::new(tr!(lay_overlay()))
                            .style(TextStyleRole::SmallBold)
                            .color(TextRole::OnAccent),
                    ),
            ),
    );
    let grid = section(
        ctx,
        "Grid",
        Grid::new()
            .columns(vec![
                TrackSize::Fixed(80.0),
                TrackSize::Fractional(1.0),
                TrackSize::Fractional(2.0),
            ])
            .rows(vec![TrackSize::Auto, TrackSize::Auto])
            .column_gap(8.0)
            .row_gap(8.0)
            .child(color_cell(SurfaceRole::AltRow, "A1"))
            .child(color_cell(SurfaceRole::AccentSubtle, "B1"))
            .child(color_cell(SurfaceRole::Raised, "C1"))
            .child(color_cell(SurfaceRole::Sunken, "A2"))
            .child(color_cell(SurfaceRole::AltRow, "B2"))
            .child(color_cell(SurfaceRole::AccentSubtle, "C2")),
    );
    let wrap = section(
        ctx,
        "Wrap",
        Wrap::new()
            .spacing(8.0)
            .line_spacing(8.0)
            .child(Badge::new_literal("Rust"))
            .child(Badge::new_literal("GUI"))
            .child(Badge::new_literal("Reactive"))
            .child(Badge::new_literal("Accessible"))
            .child(Badge::new_literal("Fast"))
            .child(Badge::new(tr!(layout_cross_platform())))
            .child(Badge::new_literal("wgpu")),
    );
    let masonry = section(
        ctx,
        "MasonryLayout",
        MasonryLayout::new(3)
            .column_spacing(8.0)
            .item_spacing(8.0)
            .child(masonry_tile(60.0))
            .child(masonry_tile(100.0))
            .child(masonry_tile(40.0))
            .child(masonry_tile(80.0))
            .child(masonry_tile(50.0))
            .child(masonry_tile(70.0)),
    );
    let form = section(
        ctx,
        "FormLayout",
        FormLayout::new()
            .row_spacing(6.0)
            .line(
                TextWidget::new(tr!(lay_form_label_a())).style(TextStyleRole::Small),
                TextWidget::new(tr!(lay_form_value_a())).style(TextStyleRole::Body),
            )
            .line(
                TextWidget::new(tr!(lay_form_label_b())).style(TextStyleRole::Small),
                TextWidget::new(tr!(lay_form_value_b())).style(TextStyleRole::Body),
            ),
    );
    let center = section(
        ctx,
        "Center",
        FixedSize::new()
            .bind_width(180.0_f32)
            .bind_height(60.0_f32)
            .child(
                ZStack::new()
                    .child(RectWidget::new().background(SurfaceRole::Raised))
                    .child(Center::new().child(
                        TextWidget::new(tr!(lay_centered())).style(TextStyleRole::SmallBold),
                    )),
            ),
    );
    let expand = section(
        ctx,
        "Expand",
        FixedSize::new()
            .bind_width(200.0_f32)
            .bind_height(28.0_f32)
            .child(
                HStack::new()
                    .spacing(0.0)
                    .child(color_cell(SurfaceRole::Sunken, "fixed"))
                    .child(
                        Expand::new()
                            .flex(1.0)
                            .child(color_cell(SurfaceRole::AltRow, "1fr")),
                    )
                    .child(
                        Expand::new()
                            .flex(2.0)
                            .child(color_cell(SurfaceRole::AccentSubtle, "2fr")),
                    ),
            ),
    );
    let padding = section(
        ctx,
        "Padding",
        Panel::new().background(SurfaceRole::Raised).child(
            Padding::uniform(16.0)
                .child(TextWidget::new(tr!(lay_padding_body())).style(TextStyleRole::Small)),
        ),
    );
    let spacer = section(
        ctx,
        "Spacer",
        FixedSize::new()
            .bind_width(220.0_f32)
            .bind_height(28.0_f32)
            .child(
                HStack::new()
                    .child(color_cell(SurfaceRole::Sunken, "L"))
                    .child(Spacer::new())
                    .child(color_cell(SurfaceRole::AltRow, "R")),
            ),
    );
    let divider = section(
        ctx,
        "Divider",
        VStack::new()
            .spacing(4.0)
            .child(TextWidget::new_literal("above").style(TextStyleRole::Small))
            .child(Divider::new())
            .child(TextWidget::new_literal("below").style(TextStyleRole::Small)),
    );
    let fixed_size = section(
        ctx,
        "FixedSize",
        FixedSize::new()
            .bind_width(140.0_f32)
            .bind_height(40.0_f32)
            .child(
                Panel::new()
                    .background(SurfaceRole::AccentSubtle)
                    .child(TextWidget::new(tr!(lay_fixed_size())).style(TextStyleRole::SmallBold)),
            ),
    );
    let min_size = section(
        ctx,
        "MinSize",
        MinSize::new(160.0, 32.0).child(
            Panel::new()
                .background(SurfaceRole::Raised)
                .child(TextWidget::new(tr!(lay_min_size())).style(TextStyleRole::Small)),
        ),
    );
    let max_size = section(
        ctx,
        "MaxSize",
        MaxSize::new(240.0, 32.0).child(
            Panel::new()
                .background(SurfaceRole::Sunken)
                .child(TextWidget::new(tr!(lay_max_size())).style(TextStyleRole::Small)),
        ),
    );
    let aspect =
        section(
            ctx,
            "AspectRatio",
            FixedSize::new().bind_width(180.0_f32).child(
                AspectRatio::widescreen().child(
                    Panel::new()
                        .background(SurfaceRole::AltRow)
                        .child(
                            Center::new().child(
                                TextWidget::new(tr!(lay_aspect_label()))
                                    .style(TextStyleRole::SmallBold),
                            ),
                        ),
                ),
            ),
        );
    let switcher_idx = sigs.tool_box_selected.clone();
    let switcher_btn = Button::new(tr!(lay_switcher_next())).on_activate_fn({
        let s = switcher_idx.clone();
        move |_| s.set((s.get() + 1) % 3)
    });
    let switcher = section(
        ctx,
        "Switcher",
        VStack::new().spacing(6.0).child(switcher_btn).child(
            FixedSize::new()
                .bind_width(180.0_f32)
                .bind_height(40.0_f32)
                .child(
                    Switcher::new(switcher_idx)
                        .child(color_cell(SurfaceRole::AccentSubtle, "page 0"))
                        .child(color_cell(SurfaceRole::Raised, "page 1"))
                        .child(color_cell(SurfaceRole::Sunken, "page 2")),
                ),
        ),
    );
    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(hstack)
            .add_child(vstack)
            .add_child(zstack)
            .add_child(grid)
            .add_child(wrap)
            .add_child(masonry)
            .add_child(form)
            .add_child(center)
            .add_child(expand)
            .add_child(padding)
            .add_child(spacer)
            .add_child(divider)
            .add_child(fixed_size)
            .add_child(min_size)
            .add_child(max_size)
            .add_child(aspect)
            .add_child(switcher),
    )
}

pub fn fern(ctx: &mut BuildContext, sigs: &Signals) -> WidgetId {
    // FormLayout's `.line(label, field)` is a 2-arg method — no fern!
    // property form for it; pre-register. MasonryLayout, Grid, Wrap
    // express fine inline. Switcher's children are `.child(impl Widget)`
    // which fern! handles, but switcher takes a Signal in the
    // constructor — that fits fern! ctor syntax.
    let form_widget = ctx.add(
        FormLayout::new()
            .row_spacing(6.0)
            .line(
                TextWidget::new(tr!(lay_form_label_a())).style(TextStyleRole::Small),
                TextWidget::new(tr!(lay_form_value_a())).style(TextStyleRole::Body),
            )
            .line(
                TextWidget::new(tr!(lay_form_label_b())).style(TextStyleRole::Small),
                TextWidget::new(tr!(lay_form_value_b())).style(TextStyleRole::Body),
            ),
    );
    let switcher_idx = sigs.tool_box_selected.clone();
    let switcher_idx_for_btn = switcher_idx.clone();

    fern!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_layout_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_layout_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new_literal("HStack") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 8.0
                    child: color_cell(SurfaceRole::AltRow, "A")
                    child: color_cell(SurfaceRole::AccentSubtle, "B")
                    child: color_cell(SurfaceRole::Raised, "C")
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("VStack") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    child: color_cell(SurfaceRole::Sunken, "Top")
                    child: color_cell(SurfaceRole::AltRow, "Mid")
                    child: color_cell(SurfaceRole::AccentSubtle, "Bot")
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("ZStack") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 120.0_f32
                    bind_height: 60.0_f32
                    ZStack {
                        RectWidget {
                            background: SurfaceRole::Raised
                        }
                        TextWidget::new(tr!(lay_overlay())) {
                            style: TextStyleRole::SmallBold
                            color: TextRole::OnAccent
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Grid") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Grid {
                    columns: vec![
                        TrackSize::Fixed(80.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(2.0),
                    ]
                    rows: vec![TrackSize::Auto, TrackSize::Auto]
                    column_gap: 8.0
                    row_gap: 8.0
                    child: color_cell(SurfaceRole::Sunken, "A1")
                    child: color_cell(SurfaceRole::AltRow, "B1")
                    child: color_cell(SurfaceRole::AccentSubtle, "C1")
                    child: color_cell(SurfaceRole::Raised, "A2")
                    child: color_cell(SurfaceRole::Sunken, "B2")
                    child: color_cell(SurfaceRole::AltRow, "C2")
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Wrap") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Wrap {
                    spacing: 8.0
                    line_spacing: 8.0
                    Badge::new_literal("Rust")
                    Badge::new_literal("GUI")
                    Badge::new_literal("Reactive")
                    Badge::new_literal("Accessible")
                    Badge::new_literal("Fast")
                    Badge::new(tr!(layout_cross_platform()))
                    Badge::new_literal("wgpu")
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("MasonryLayout") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                MasonryLayout(3) {
                    column_spacing: 8.0
                    item_spacing: 8.0
                    child: masonry_tile(60.0)
                    child: masonry_tile(100.0)
                    child: masonry_tile(40.0)
                    child: masonry_tile(80.0)
                    child: masonry_tile(50.0)
                    child: masonry_tile(70.0)
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("FormLayout") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ form_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Center") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 180.0_f32
                    bind_height: 60.0_f32
                    ZStack {
                        RectWidget {
                            background: SurfaceRole::AccentSubtle
                        }
                        Center {
                            TextWidget::new(tr!(lay_centered())) {
                                style: TextStyleRole::SmallBold
                            }
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Expand") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 200.0_f32
                    bind_height: 28.0_f32
                    HStack {
                        spacing: 0.0
                        child: color_cell(SurfaceRole::Raised, "fixed")
                        Expand {
                            flex: 1.0
                            child: color_cell(SurfaceRole::Sunken, "1fr")
                        }
                        Expand {
                            flex: 2.0
                            child: color_cell(SurfaceRole::AltRow, "2fr")
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Padding") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Panel {
                    background: SurfaceRole::AccentSubtle
                    Padding::uniform(16.0) {
                        TextWidget::new(tr!(lay_padding_body())) {
                            style: TextStyleRole::Small
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Spacer") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 220.0_f32
                    bind_height: 28.0_f32
                    HStack {
                        child: color_cell(SurfaceRole::Raised, "L")
                        Spacer
                        child: color_cell(SurfaceRole::Sunken, "R")
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Divider") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 4.0
                    TextWidget::new_literal("above") {
                        style: TextStyleRole::Small
                    }
                    Divider
                    TextWidget::new_literal("below") {
                        style: TextStyleRole::Small
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("FixedSize") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 140.0_f32
                    bind_height: 40.0_f32
                    Panel {
                        background: SurfaceRole::AltRow
                        TextWidget::new(tr!(lay_fixed_size())) {
                            style: TextStyleRole::SmallBold
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("MinSize") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                MinSize(160.0, 32.0) {
                    Panel {
                        background: SurfaceRole::AccentSubtle
                        TextWidget::new(tr!(lay_min_size())) {
                            style: TextStyleRole::Small
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("MaxSize") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                MaxSize(240.0, 32.0) {
                    Panel {
                        background: SurfaceRole::Raised
                        TextWidget::new(tr!(lay_max_size())) {
                            style: TextStyleRole::Small
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("AspectRatio") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 180.0_f32
                    AspectRatio::widescreen() {
                        Panel {
                            background: SurfaceRole::Sunken
                            Center {
                                TextWidget::new(tr!(lay_aspect_label())) {
                                    style: TextStyleRole::SmallBold
                                }
                            }
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Switcher") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    Button::new(tr!(lay_switcher_next())) {
                        on_activate_fn: move |_| switcher_idx_for_btn.set((switcher_idx_for_btn.get() + 1) % 3)
                    }
                    FixedSize {
                        bind_width: 180.0_f32
                        bind_height: 40.0_f32
                        Switcher::new(switcher_idx) {
                            child: color_cell(SurfaceRole::AltRow, "page 0")
                            child: color_cell(SurfaceRole::AccentSubtle, "page 1")
                            child: color_cell(SurfaceRole::Raised, "page 2")
                        }
                    }
                }
            }
        }
    )
}
