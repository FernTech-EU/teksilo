// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TextInput::min_height` reaches this preset's field: its style replaces
//! the Breeze line edit's height with the one the app asked for.

use teksilo_canvas::SizeProposal;
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_widgets::TextInput;
use teksilo_widgets::primitives::VStack;

/// The framed field's height: the first child of the composite's root.
fn frame_height(input: TextInput) -> f32 {
    let mut tree = WidgetTree::new().with_theme(teksilo_theme_kde::light());
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

/// Without one, the field is Breeze's 32 px line edit.
#[test]
fn the_field_is_the_breeze_line_edit_by_default() {
    assert_eq!(frame_height(input()), 32.0);
}

/// Below the line edit's height: only the style can stand in the way there,
/// since the widget's floor already follows the app's value.
#[test]
fn an_apps_min_height_replaces_the_presets_height() {
    let preset = frame_height(input());
    let asked = 23.0;
    assert!(asked < preset, "the preset's height is {preset}");
    let short = frame_height(input().text_style(small_style()).min_height(asked));
    assert!(
        (short - asked).abs() < 0.01,
        "asked for {asked}, got {short}"
    );
}

/// Above it too.
#[test]
fn a_taller_min_height_raises_the_field() {
    let tall = frame_height(input().min_height(40.0));
    assert!((tall - 40.0).abs() < 0.01, "asked for 40, got {tall}");
}
