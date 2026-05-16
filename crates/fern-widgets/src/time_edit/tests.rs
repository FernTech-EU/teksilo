use super::*;
use crate::common::datetime::Time;
use fern_core::signal::Signal;
use fern_core::widget_tree::WidgetTree;

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(fern_core::presets::intui::light())
}

#[test]
fn time_edit_builds_with_value() {
    let mut tree = light_tree();
    let value = Signal::new(Some(Time::new(14, 35, 0, 0).unwrap()));
    let id = tree.add(TimeEdit::new(value));
    tree.layout(SizeProposal {
        width: Some(200.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
}

#[test]
fn time_edit_role_is_time_input() {
    let mut tree = light_tree();
    let value = Signal::new(Some(Time::new(14, 35, 0, 0).unwrap()));
    let id = tree.add(TimeEdit::new(value));
    tree.layout(SizeProposal {
        width: Some(200.0),
        height: None,
    });
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), fern_core::accesskit::Role::TimeInput);
}

#[test]
fn time_edit_value_emits_iso_in_at_tree() {
    let mut tree = light_tree();
    let value = Signal::new(Some(Time::new(14, 35, 7, 0).unwrap()));
    let id = tree.add(TimeEdit::new(value));
    tree.layout(SizeProposal {
        width: Some(200.0),
        height: None,
    });
    let update = tree.sync_accessibility();
    let target = fern_core::accessibility::widget_id_to_node_id(id);
    let (_, node) = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == target)
        .expect("time edit node");
    assert_eq!(node.value().unwrap_or_default(), "14:35:07");
}

#[test]
fn time_edit_resolved_pattern_24h_no_seconds() {
    let value = Signal::new(Some(Time::midnight()));
    let editor = TimeEdit::new(value)
        .format(TimeFormat::Hour24)
        .seconds(SecondsMode::Hidden);
    assert_eq!(editor.resolved_pattern(TimeFormat::Hour24), "%H:%M");
}

#[test]
fn time_edit_resolved_pattern_12h_with_seconds() {
    let value = Signal::new(Some(Time::midnight()));
    let editor = TimeEdit::new(value)
        .format(TimeFormat::Hour12)
        .seconds(SecondsMode::Editable);
    assert_eq!(editor.resolved_pattern(TimeFormat::Hour12), "%I:%M:%S %p");
}

#[test]
fn time_edit_clamp_below_min() {
    let t = Time::new(8, 0, 0, 0).unwrap();
    let min = Time::new(9, 0, 0, 0).unwrap();
    let clamped = clamp_time(t, Some(min), None);
    assert_eq!(clamped, min);
}

#[test]
fn time_edit_clamp_above_max() {
    let t = Time::new(20, 0, 0, 0).unwrap();
    let max = Time::new(18, 0, 0, 0).unwrap();
    let clamped = clamp_time(t, None, Some(max));
    assert_eq!(clamped, max);
}

// ── Validation pipeline ─────────────────────────────────────

use crate::common::datetime::pattern::ParsedPattern;

#[test]
fn time_clamp_recovery_clamps_hour_24() {
    // 25:00 → 23:00 in 24h format
    let pattern = ParsedPattern::parse("%H:%M").unwrap();
    let (corrected, _msg) =
        try_clamp_time_recovery(&pattern, "25:00", None, None).expect("recovery");
    assert_eq!(corrected, "23:00");
}

#[test]
fn time_clamp_recovery_clamps_minute() {
    // 12:75 → 12:59
    let pattern = ParsedPattern::parse("%H:%M").unwrap();
    let (corrected, _msg) =
        try_clamp_time_recovery(&pattern, "12:75", None, None).expect("recovery");
    assert_eq!(corrected, "12:59");
}

#[test]
fn time_clamp_recovery_returns_none_for_garbage() {
    let pattern = ParsedPattern::parse("%H:%M").unwrap();
    assert!(try_clamp_time_recovery(&pattern, "abc", None, None).is_none());
    assert!(try_clamp_time_recovery(&pattern, "", None, None).is_none());
}

#[test]
fn time_edit_validation_behavior_builder_accepts_both_variants() {
    use crate::date_edit::ValidationBehavior;
    let v = Signal::new(Some(Time::midnight()));
    let _ = TimeEdit::new(v.clone()).validation_behavior(ValidationBehavior::AutoCorrect);
    let _ = TimeEdit::new(v).validation_behavior(ValidationBehavior::Reject);
}

#[test]
fn time_edit_validation_feedback_signal_starts_pristine() {
    use crate::primitives::text_input_field::ValidationFeedback;
    let v = Signal::new(Some(Time::midnight()));
    let editor = TimeEdit::new(v);
    let feedback = editor.validation_feedback_signal();
    assert!(matches!(feedback.get(), ValidationFeedback::Pristine));
}
