// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;
use crate::common::datetime::{Date, DateTime, Time};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

fn make_dt() -> DateTime {
    Date::constant(2026, 5, 2).to_datetime(Time::new(14, 35, 7, 0).unwrap())
}

#[test]
fn date_time_edit_builds_with_value() {
    let mut tree = light_tree();
    let value = Signal::new(Some(make_dt()));
    let id = tree.add(DateTimeEdit::new(value));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
}

#[test]
fn date_time_edit_role_is_date_time_input() {
    let mut tree = light_tree();
    let value = Signal::new(Some(make_dt()));
    let id = tree.add(DateTimeEdit::new(value));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), teksilo_core::accesskit::Role::DateTimeInput);
}

#[test]
fn date_time_edit_value_in_at_tree() {
    let mut tree = light_tree();
    let value = Signal::new(Some(make_dt()));
    let id = tree.add(DateTimeEdit::new(value));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let update = tree.sync_accessibility();
    let target = teksilo_core::accessibility::widget_id_to_node_id(id);
    let (_, node) = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == target)
        .expect("date time edit node");
    assert_eq!(node.value().unwrap_or_default(), "2026-05-02T14:35:07");
}

#[test]
fn date_time_edit_validation_feedback_signal_starts_pristine() {
    use crate::primitives::text_input_field::ValidationFeedback;
    let value = Signal::new(Some(make_dt()));
    let editor = DateTimeEdit::new(value);
    let feedback = editor.validation_feedback_signal();
    assert!(matches!(feedback.get(), ValidationFeedback::Pristine));
}

#[test]
fn compose_feedback_picks_more_severe() {
    use crate::primitives::text_input_field::ValidationFeedback;
    use std::time::Instant;
    let invalid = ValidationFeedback::Invalid {
        message: teksilo_i18n::lit!("x"),
    };
    let corrected = ValidationFeedback::Corrected {
        message: teksilo_i18n::lit!("c"),
        since: Instant::now(),
    };
    let valid = ValidationFeedback::Valid;
    let pristine = ValidationFeedback::Pristine;

    // Invalid > Corrected
    assert!(matches!(
        compose_feedback(&invalid, &corrected),
        ValidationFeedback::Invalid { .. }
    ));
    assert!(matches!(
        compose_feedback(&corrected, &invalid),
        ValidationFeedback::Invalid { .. }
    ));
    // Corrected > Valid
    assert!(matches!(
        compose_feedback(&corrected, &valid),
        ValidationFeedback::Corrected { .. }
    ));
    // Valid > Pristine
    assert!(matches!(
        compose_feedback(&valid, &pristine),
        ValidationFeedback::Valid
    ));
    // Pristine + Pristine = Pristine
    assert!(matches!(
        compose_feedback(&pristine, &pristine),
        ValidationFeedback::Pristine
    ));
}

#[test]
fn date_time_edit_halves_compose_back() {
    // Build the widget so the mirror effects are wired, then mutate
    // the bound signal externally and confirm the halves track.
    let mut tree = light_tree();
    let value: Signal<Option<DateTime>> = Signal::new(None);
    let editor = DateTimeEdit::new(value.clone());
    let date_part = editor.date_part.clone();
    let time_part = editor.time_part.clone();
    let _id = tree.add(editor);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    // Initial: all halves empty.
    assert_eq!(date_part.get(), None);
    assert_eq!(time_part.get(), None);
    // Mutate outer; halves follow.
    value.set(Some(make_dt()));
    assert_eq!(date_part.get(), Some(Date::constant(2026, 5, 2)));
    assert_eq!(time_part.get(), Some(Time::new(14, 35, 7, 0).unwrap()));
}

#[test]
fn tooltip_appears_on_hover() {
    let mut tree = light_tree();
    let value = Signal::new(Some(make_dt()));
    let id = tree.add(DateTimeEdit::new(value).tooltip(lit!("Tip")));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    tree.pointer_move(tree.bounds(id).center());
    tree.advance_time(std::time::Duration::from_secs(1));
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "tooltip should appear on hover"
    );
    assert!(tree.find_by_label("Tip").is_some());
}
