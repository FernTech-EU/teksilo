// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TextInput::min_height` reaches this preset's field: its style replaces
//! its own control-height floor with the one the app asked for.

use teksilo_canvas::SizeProposal;
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_widgets::TextInput;
use teksilo_widgets::primitives::VStack;

/// The framed field's height: the first child of the composite's root.
fn frame_height(input: TextInput) -> f32 {
    let mut tree = WidgetTree::new().with_theme(teksilo_theme_fluent::light());
    // In a `VStack`, so the composite is laid out at the height it asks for.
    let id = tree.add(VStack::new().child(input));
    tree.layout(SizeProposal::exact(300.0, 200.0));
    let input_id = tree.children(id)[0];
    let root = tree.children(input_id)[0];
    let frame = tree.children(root)[0];
    tree.bounds(frame).height
}

fn input() -> TextInput {
    TextInput::new(Signal::new(String::new()))
}

fn small_style() -> teksilo_tokens::TextStyle {
    teksilo_tokens::TextStyle {
        family: "Inter".to_string(),
        size: 11.0,
        weight: teksilo_tokens::FontWeight::REGULAR,
        line_height: 1.3,
        letter_spacing: 0.0,
    }
}

/// Below the preset's own control height: only the style can stand in the
/// way there, since the widget's floor already follows the app's value.
#[test]
fn an_apps_min_height_replaces_the_presets_floor() {
    let preset = frame_height(input());
    let asked = 23.0;
    assert!(asked < preset, "the preset floor is {preset}");
    let short = frame_height(input().text_style(small_style()).min_height(asked));
    assert!(
        (short - asked).abs() < 0.01,
        "asked for {asked}, got {short}"
    );
}
