//! Inputs tab — Checkbox, RadioButton, Toggle, Slider, SegmentedControl, ComboBox.

use fern_ui::prelude::*;
use fern_ui::tokens::Orientation;
use fern_ui::widgets::{
    Checkbox, ComboBox, Divider, FixedSize, RadioButton, SegmentedControl, Slider, TextWidget,
    Toggle, VStack,
};

use crate::shared::{Signals, section, tab_header};

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
        "Checkbox",
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
        "RadioButton (in a group)",
        VStack::new()
            .spacing(4.0)
            .child(RadioButton::new(0, sigs.radio_selected.clone()).label(tr!(inp_radio_a())))
            .child(RadioButton::new(1, sigs.radio_selected.clone()).label(tr!(inp_radio_b())))
            .child(RadioButton::new(2, sigs.radio_selected.clone()).label(tr!(inp_radio_c()))),
    );
    let toggle = section(
        ctx,
        "Toggle",
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
        "Slider — horizontal",
        FixedSize::new().bind_width(300.0_f32).child(
            Slider::new(sigs.slider_value.clone(), 0.0, 100.0).label(tr!(inp_slider_volume())),
        ),
    );
    let slider_stepped = section(
        ctx,
        "Slider — stepped",
        FixedSize::new().bind_width(300.0_f32).child(
            Slider::new(sigs.slider_stepped.clone(), 0.0, 100.0)
                .step(25.0)
                .label(tr!(inp_slider_stepped())),
        ),
    );
    let slider_v = section(
        ctx,
        "Slider — vertical",
        FixedSize::new().bind_height(150.0_f32).child(
            Slider::new(sigs.slider_v_value.clone(), 0.0, 1.0)
                .orientation(Orientation::Vertical)
                .label(tr!(inp_slider_vertical())),
        ),
    );
    let segmented = section(
        ctx,
        "SegmentedControl",
        SegmentedControl::new(
            vec![
                tr!(inp_segment_first()).resolve_now(),
                tr!(inp_segment_second()).resolve_now(),
                tr!(inp_segment_third()).resolve_now(),
            ],
            sigs.segment_selected.clone(),
        ),
    );
    let combo = section(
        ctx,
        "ComboBox",
        FixedSize::new().bind_width(220.0_f32).child(
            ComboBox::from_items(
                vec![
                    tr!(inp_combo_apple()).resolve_now(),
                    tr!(inp_combo_banana()).resolve_now(),
                    tr!(inp_combo_cherry()).resolve_now(),
                ],
                sigs.combo_selected.clone(),
                |s: &String| s.clone(),
            )
            .placeholder(tr!(inp_combo_placeholder()).resolve_now()),
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
            .add_child(combo),
    )
}

pub fn fern(ctx: &mut BuildContext, sigs: &Signals) -> WidgetId {
    // SegmentedControl + ComboBox have multi-arg constructors that
    // fern! ctor syntax can't express on its own — pre-register them.
    let segmented_widget = ctx.add(SegmentedControl::new(
        vec![
            tr!(inp_segment_first()).resolve_now(),
            tr!(inp_segment_second()).resolve_now(),
            tr!(inp_segment_third()).resolve_now(),
        ],
        sigs.segment_selected.clone(),
    ));
    let combo_widget = ctx.add(
        ComboBox::from_items(
            vec![
                tr!(inp_combo_apple()).resolve_now(),
                tr!(inp_combo_banana()).resolve_now(),
                tr!(inp_combo_cherry()).resolve_now(),
            ],
            sigs.combo_selected.clone(),
            |s: &String| s.clone(),
        )
        .placeholder(tr!(inp_combo_placeholder()).resolve_now()),
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

    fern!(ctx => VStack {
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
                TextWidget::new_literal("Checkbox") {
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
                TextWidget::new_literal("RadioButton (in a group)") {
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
                TextWidget::new_literal("Toggle") {
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
                TextWidget::new_literal("Slider — horizontal") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 300.0_f32
                    Slider::new(slider_h_val, 0.0, 100.0) {
                        label: tr!(inp_slider_volume())
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Slider — stepped") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 300.0_f32
                    Slider::new(slider_step, 0.0, 100.0) {
                        step: 25.0
                        label: tr!(inp_slider_stepped())
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Slider — vertical") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_height: 150.0_f32
                    Slider::new(slider_v_val, 0.0, 1.0) {
                        orientation: Orientation::Vertical
                        label: tr!(inp_slider_vertical())
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("SegmentedControl") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ segmented_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("ComboBox") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 220.0_f32
                    child_id: combo_widget
                }
            }
        }
    )
}
