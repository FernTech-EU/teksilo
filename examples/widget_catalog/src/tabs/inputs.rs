// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Inputs tab — Checkbox, RadioButton, Toggle, Slider, SegmentedControl, ComboBox.

use teksilo::prelude::*;
use teksilo::tokens::Orientation;
use teksilo::widgets::{
    Checkbox, ComboBox, Divider, FixedSize, IconWidget, RadioButton, RadioTile, RadioTileGroup,
    SegmentedControl, Slider, TextWidget, TileLayout, Toggle, VStack,
};

use crate::shared::{Signals, section, tab_header};

// Minimal tintable glyphs for the RadioTile showcase (geometry only).
const FILE_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path d='M6 2h8l4 4v16H6z'/></svg>";
const FOLDER_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path d='M3 6h6l2 2h10v11H3z'/></svg>";
const BAN_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path d='M12 3a9 9 0 100 18 9 9 0 000-18zm0 2a7 7 0 015.7 11L7 6.3A7 7 0 0112 5z'/></svg>";
const LAYERS_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path d='M12 3l9 5-9 5-9-5z'/></svg>";
const NOTE_SVG: &str =
    "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path d='M5 3h14v18H5z'/></svg>";

/// A leading tile icon whose tint follows selection.
fn tile_icon(svg: &'static str, index: usize, selected: &Signal<usize>) -> IconWidget {
    let color = selected.map(move |s| {
        if *s == index {
            TextRole::Accent
        } else {
            TextRole::Secondary
        }
    });
    IconWidget::from_svg(svg).icon_size(20.0).color(color)
}

pub fn title() -> LocalizedString {
    tr!(tab_inputs_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_inputs_refs())
}

pub fn classic(ctx: &mut BuildContext, sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let checkbox = section(
        ctx,
        lit!("Checkbox"),
        VStack::new()
            .spacing(6.0)
            .child(
                Checkbox::new(sigs.checkbox_checked.clone()).label(tr!(inp_checkbox_two_state())),
            )
            .child(Checkbox::tristate(sigs.tristate.clone()).label(tr!(inp_checkbox_tristate())))
            .child(
                Checkbox::new(sigs.cb_disabled_state.clone())
                    .label(tr!(inp_checkbox_disabled()))
                    .enabled(false),
            ),
    );
    let radio = section(
        ctx,
        tr!(inp_heading_radio_group()),
        VStack::new()
            .spacing(4.0)
            .child(RadioButton::new(0, sigs.radio_selected.clone()).label(tr!(inp_radio_a())))
            .child(RadioButton::new(1, sigs.radio_selected.clone()).label(tr!(inp_radio_b())))
            .child(RadioButton::new(2, sigs.radio_selected.clone()).label(tr!(inp_radio_c()))),
    );
    let toggle = section(
        ctx,
        lit!("Toggle"),
        VStack::new()
            .spacing(6.0)
            .child(Toggle::new(sigs.toggle_on.clone()).label(tr!(inp_toggle_feature())))
            .child(Toggle::new(sigs.toggle_label_on.clone()).label(tr!(inp_toggle_with_label())))
            .child(
                Toggle::new(sigs.toggle_disabled_state.clone())
                    .label(tr!(inp_toggle_disabled()))
                    .enabled(false),
            ),
    );
    let slider_h = section(
        ctx,
        tr!(inp_heading_slider_h()),
        FixedSize::new().width(300.0_f32).child(
            Slider::new(sigs.slider_value.clone(), 0.0, 100.0).label(tr!(inp_slider_volume())),
        ),
    );
    let slider_stepped = section(
        ctx,
        tr!(inp_heading_slider_stepped()),
        FixedSize::new().width(300.0_f32).child(
            Slider::new(sigs.slider_stepped.clone(), 0.0, 100.0)
                .step(25.0)
                .label(tr!(inp_slider_stepped())),
        ),
    );
    let slider_v = section(
        ctx,
        tr!(inp_heading_slider_v()),
        FixedSize::new().height(150.0_f32).child(
            Slider::new(sigs.slider_v_value.clone(), 0.0, 1.0)
                .orientation(Orientation::Vertical)
                .label(tr!(inp_slider_vertical())),
        ),
    );
    let segmented = section(
        ctx,
        lit!("SegmentedControl"),
        SegmentedControl::new(sigs.segment_selected.clone()).segments([
            tr!(inp_segment_first()),
            tr!(inp_segment_second()),
            tr!(inp_segment_third()),
        ]),
    );
    let radio_tile = section(
        ctx,
        lit!("RadioTile"),
        VStack::new()
            .spacing(12.0)
            .child(
                RadioTileGroup::new(sigs.radio_tile_selected.clone())
                    .layout(TileLayout::Row)
                    .tile(
                        RadioTile::new()
                            .icon(tile_icon(FILE_SVG, 0, &sigs.radio_tile_selected))
                            .title(lit!("Single file"))
                            .description(lit!("One .skrib archive (zip).")),
                    )
                    .tile(
                        RadioTile::new()
                            .icon(tile_icon(FOLDER_SVG, 1, &sigs.radio_tile_selected))
                            .title(lit!("Bundle"))
                            .description(lit!("A folder of every text & asset.")),
                    ),
            )
            .child(
                RadioTileGroup::new(sigs.radio_tile_vertical_selected.clone())
                    .layout(TileLayout::Vertical)
                    .tile(
                        RadioTile::new()
                            .icon(tile_icon(BAN_SVG, 0, &sigs.radio_tile_vertical_selected))
                            .title(lit!("None"))
                            .trailing(lit!("empty binder")),
                    )
                    .tile(
                        RadioTile::new()
                            .icon(tile_icon(LAYERS_SVG, 1, &sigs.radio_tile_vertical_selected))
                            .title(lit!("Light Novel"))
                            .trailing(lit!("15 chapters")),
                    )
                    .tile(
                        RadioTile::new()
                            .icon(tile_icon(LAYERS_SVG, 2, &sigs.radio_tile_vertical_selected))
                            .title(lit!("Novel"))
                            .trailing(lit!("20 chapters")),
                    )
                    .tile(
                        RadioTile::new()
                            .icon(tile_icon(NOTE_SVG, 3, &sigs.radio_tile_vertical_selected))
                            .title(lit!("Notebook"))
                            .trailing(lit!("free-form notes")),
                    ),
            ),
    );
    let combo = section(
        ctx,
        lit!("ComboBox"),
        FixedSize::new().width(220.0_f32).child(
            ComboBox::from_items(
                vec![
                    tr!(inp_combo_apple()).resolve_now(),
                    tr!(inp_combo_banana()).resolve_now(),
                    tr!(inp_combo_cherry()).resolve_now(),
                ],
                sigs.combo_selected.clone(),
                |s: &String| lit!(s.clone()),
            )
            .placeholder(tr!(inp_combo_placeholder())),
        ),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(checkbox)
            .add_child(radio)
            .add_child(toggle)
            .add_child(slider_h)
            .add_child(slider_stepped)
            .add_child(slider_v)
            .add_child(segmented)
            .add_child(radio_tile)
            .add_child(combo),
    )
}

pub fn teksu(ctx: &mut BuildContext, sigs: &Signals) -> WidgetId {
    // SegmentedControl + ComboBox have multi-arg constructors that
    // teksu! ctor syntax can't express on its own — pre-register them.
    let segmented_widget = ctx.add(
        SegmentedControl::new(sigs.segment_selected.clone()).segments([
            tr!(inp_segment_first()),
            tr!(inp_segment_second()),
            tr!(inp_segment_third()),
        ]),
    );
    // RadioTileGroup's `.tile(...)` chain can't be expressed in teksu! ctor
    // syntax — pre-register it and reference by id.
    let radio_tile_widget = ctx.add(
        VStack::new()
            .spacing(12.0)
            .child(
                RadioTileGroup::new(sigs.radio_tile_selected.clone())
                    .layout(TileLayout::Row)
                    .tile(
                        RadioTile::new()
                            .icon(tile_icon(FILE_SVG, 0, &sigs.radio_tile_selected))
                            .title(lit!("Single file"))
                            .description(lit!("One .skrib archive (zip).")),
                    )
                    .tile(
                        RadioTile::new()
                            .icon(tile_icon(FOLDER_SVG, 1, &sigs.radio_tile_selected))
                            .title(lit!("Bundle"))
                            .description(lit!("A folder of every text & asset.")),
                    ),
            )
            .child(
                RadioTileGroup::new(sigs.radio_tile_vertical_selected.clone())
                    .layout(TileLayout::Vertical)
                    .tile(
                        RadioTile::new()
                            .icon(tile_icon(BAN_SVG, 0, &sigs.radio_tile_vertical_selected))
                            .title(lit!("None"))
                            .trailing(lit!("empty binder")),
                    )
                    .tile(
                        RadioTile::new()
                            .icon(tile_icon(LAYERS_SVG, 1, &sigs.radio_tile_vertical_selected))
                            .title(lit!("Novel"))
                            .trailing(lit!("20 chapters")),
                    )
                    .tile(
                        RadioTile::new()
                            .icon(tile_icon(NOTE_SVG, 2, &sigs.radio_tile_vertical_selected))
                            .title(lit!("Notebook"))
                            .trailing(lit!("free-form notes")),
                    ),
            ),
    );
    let combo_widget = ctx.add(
        ComboBox::from_items(
            vec![
                tr!(inp_combo_apple()).resolve_now(),
                tr!(inp_combo_banana()).resolve_now(),
                tr!(inp_combo_cherry()).resolve_now(),
            ],
            sigs.combo_selected.clone(),
            |s: &String| lit!(s.clone()),
        )
        .placeholder(tr!(inp_combo_placeholder())),
    );

    let cb_checked = sigs.checkbox_checked.clone();
    let cb_tri = sigs.tristate.clone();
    let cb_disabled = sigs.cb_disabled_state.clone();
    let radio_sel = sigs.radio_selected.clone();
    let toggle_on = sigs.toggle_on.clone();
    let toggle_label = sigs.toggle_label_on.clone();
    let toggle_disabled = sigs.toggle_disabled_state.clone();
    let slider_h_val = sigs.slider_value.clone();
    let slider_step = sigs.slider_stepped.clone();
    let slider_v_val = sigs.slider_v_value.clone();

    teksu!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_inputs_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_inputs_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Checkbox")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    Checkbox::new(cb_checked) {
                        label: tr!(inp_checkbox_two_state())
                    }
                    Checkbox::tristate(cb_tri) {
                        label: tr!(inp_checkbox_tristate())
                    }
                    Checkbox::new(cb_disabled) {
                        label: tr!(inp_checkbox_disabled())
                        enabled: false
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(inp_heading_radio_group())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 4.0
                    RadioButton::new(0, radio_sel.clone()) {
                        label: tr!(inp_radio_a())
                    }
                    RadioButton::new(1, radio_sel.clone()) {
                        label: tr!(inp_radio_b())
                    }
                    RadioButton::new(2, radio_sel) {
                        label: tr!(inp_radio_c())
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Toggle")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    Toggle::new(toggle_on) {
                        label: tr!(inp_toggle_feature())
                    }
                    Toggle::new(toggle_label) {
                        label: tr!(inp_toggle_with_label())
                    }
                    Toggle::new(toggle_disabled) {
                        label: tr!(inp_toggle_disabled())
                        enabled: false
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(inp_heading_slider_h())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    width: 300.0_f32
                    Slider::new(slider_h_val, 0.0, 100.0) {
                        label: tr!(inp_slider_volume())
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(inp_heading_slider_stepped())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    width: 300.0_f32
                    Slider::new(slider_step, 0.0, 100.0) {
                        step: 25.0
                        label: tr!(inp_slider_stepped())
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(inp_heading_slider_v())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    height: 150.0_f32
                    Slider::new(slider_v_val, 0.0, 1.0) {
                        orientation: Orientation::Vertical
                        label: tr!(inp_slider_vertical())
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("SegmentedControl")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ segmented_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("RadioTile")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ radio_tile_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("ComboBox")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    width: 220.0_f32
                    child_id: combo_widget
                }
            }
        }
    )
}
