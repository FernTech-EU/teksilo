// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `WidgetTree::accessibility_node` reports a node's value, numeric value,
//! numeric range and custom actions, so a test reads them from the one query
//! instead of walking `sync_accessibility().nodes`.
//!
//! The info is built from the overridden builder, so an application's
//! `.access_value(..)` / `.access_custom_action(..)` shows up too.

use teksilo_canvas::SizeProposal;
use teksilo_core::WidgetBuilder;
use teksilo_core::accesskit::Role;
use teksilo_core::presets::intui;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_widgets::slider::Slider;
use teksilo_widgets::text_input::TextInput;
use teksilo_widgets::*;

fn tree() -> WidgetTree {
    WidgetTree::new().with_theme(intui::light())
}

#[test]
fn a_slider_reports_its_numeric_value_and_range() {
    let value = Signal::new(25.0_f32);
    let mut tree = tree();
    let s = tree.add(Slider::new(value.clone(), 0.0, 100.0));
    tree.layout(SizeProposal::exact(200.0, 60.0));

    let info = tree.accessibility_node(s);
    assert_eq!(info.numeric_value(), Some(25.0));
    assert_eq!(info.min_numeric_value(), Some(0.0));
    assert_eq!(info.max_numeric_value(), Some(100.0));

    value.set(60.0);
    tree.layout(SizeProposal::exact(200.0, 60.0));
    assert_eq!(tree.accessibility_node(s).numeric_value(), Some(60.0));
}

#[test]
fn a_node_without_a_numeric_value_reports_none() {
    let mut tree = tree();
    let b = tree.add(Button::new(lit!("Go")));
    tree.layout(SizeProposal::exact(200.0, 60.0));
    let info = tree.accessibility_node(b);
    assert_eq!(info.numeric_value(), None);
    assert_eq!(info.min_numeric_value(), None);
    assert_eq!(info.max_numeric_value(), None);
    assert_eq!(info.value(), None);
    assert!(info.custom_actions().is_empty());
}

#[test]
fn a_text_input_reports_its_value() {
    let text = Signal::new("hello".to_string());
    let mut tree = tree();
    tree.add(TextInput::new(text));
    tree.layout(SizeProposal::exact(200.0, 40.0));
    let id = tree
        .find_by_role(Role::TextInput)
        .expect("a text input node");
    assert_eq!(tree.accessibility_node(id).value(), Some("hello"));
}

#[test]
fn an_access_value_override_is_reflected() {
    let mut tree = tree();
    let b = tree.add(
        Button::new(lit!("Volume"))
            .access_value(Prop::Static("loud".to_string()))
            .access_numeric_value(11.0),
    );
    tree.layout(SizeProposal::exact(200.0, 60.0));
    let info = tree.accessibility_node(b);
    assert_eq!(info.value(), Some("loud"));
    assert_eq!(info.numeric_value(), Some(11.0));
}

#[test]
fn custom_actions_are_listed_in_declaration_order() {
    let mut tree = tree();
    let b = tree.add(
        Button::new(lit!("Post"))
            .access_custom_action(Prop::Static("Publish now".to_string()), |_| {})
            .access_custom_action(Prop::Static("Schedule".to_string()), |_| {}),
    );
    tree.layout(SizeProposal::exact(200.0, 60.0));
    let info = tree.accessibility_node(b);
    assert_eq!(info.custom_actions(), ["Publish now", "Schedule"]);
}
