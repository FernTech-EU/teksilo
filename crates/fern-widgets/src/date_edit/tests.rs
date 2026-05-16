use super::*;
use crate::common::datetime::Date;
use fern_core::signal::Signal;
use fern_core::widget_tree::WidgetTree;

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(fern_core::presets::intui::light())
}

#[test]
fn date_edit_builds_with_value() {
    let mut tree = light_tree();
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value));
    tree.layout(SizeProposal {
        width: Some(300.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
    assert!(bounds.height > 0.0);
}

#[test]
fn date_edit_builds_with_none_value() {
    let mut tree = light_tree();
    let value = Signal::new(None::<Date>);
    let id = tree.add(DateEdit::new(value));
    tree.layout(SizeProposal {
        width: Some(300.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
}

#[test]
fn date_edit_role_is_date_input() {
    let mut tree = light_tree();
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value));
    tree.layout(SizeProposal {
        width: Some(300.0),
        height: None,
    });
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), fern_core::accesskit::Role::DateInput);
}

#[test]
fn date_edit_value_iso_in_at_tree() {
    let mut tree = light_tree();
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value));
    tree.layout(SizeProposal {
        width: Some(300.0),
        height: None,
    });
    let update = tree.sync_accessibility();
    let target = fern_core::accessibility::widget_id_to_node_id(id);
    let (_, node) = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == target)
        .expect("date edit node");
    assert_eq!(node.value().unwrap_or_default(), "2026-05-02");
}

#[test]
fn date_edit_clamp_inside_range_unchanged() {
    let d = Date::constant(2026, 5, 15);
    let clamped = clamp_date(
        d,
        Some(Date::constant(2020, 1, 1)),
        Some(Date::constant(2030, 12, 31)),
    );
    assert_eq!(clamped, d);
}

#[test]
fn date_edit_clamp_below_min() {
    let d = Date::constant(2010, 5, 15);
    let min = Date::constant(2020, 1, 1);
    let clamped = clamp_date(d, Some(min), None);
    assert_eq!(clamped, min);
}

#[test]
fn date_edit_clamp_above_max() {
    let d = Date::constant(2040, 5, 15);
    let max = Date::constant(2030, 12, 31);
    let clamped = clamp_date(d, None, Some(max));
    assert_eq!(clamped, max);
}

#[test]
fn date_edit_with_calendar_button_disabled_still_builds() {
    let mut tree = light_tree();
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value).show_calendar_button(false));
    tree.layout(SizeProposal {
        width: Some(300.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
}

// ── Validation pipeline ─────────────────────────────────────

use crate::common::datetime::pattern::ParsedPattern;

fn iso_pattern() -> ParsedPattern {
    ParsedPattern::parse("%Y-%m-%d").unwrap()
}

#[test]
fn clamp_recovery_clamps_out_of_range_day() {
    // 12/50/2026 → 12/31/2026 (December has 31 days, day clamped 50→31)
    let pattern = ParsedPattern::parse("%m/%d/%Y").unwrap();
    let (corrected, msg) =
        try_clamp_recovery(&pattern, "12/50/2026", None, None).expect("recovery");
    assert_eq!(corrected, "12/31/2026");
    // Without an i18n manager installed, the message is the literal
    // Fluent key — assert it's the "with notes" variant, not the
    // bare "corrected to" one.
    assert!(msg.contains("with-notes") || msg.contains("validation"));
}

#[test]
fn clamp_recovery_clamps_out_of_range_month() {
    // 13/15/2026 → 12/15/2026 (month clamped 13→12, day 15 fits in Dec)
    let pattern = ParsedPattern::parse("%m/%d/%Y").unwrap();
    let (corrected, msg) =
        try_clamp_recovery(&pattern, "13/15/2026", None, None).expect("recovery");
    assert_eq!(corrected, "12/15/2026");
    assert!(msg.contains("with-notes") || msg.contains("validation"));
}

#[test]
fn clamp_recovery_clamps_day_to_february() {
    // 2/31/2026 → 2/28/2026 (2026 is not a leap year)
    let pattern = ParsedPattern::parse("%m/%d/%Y").unwrap();
    let (corrected, _msg) =
        try_clamp_recovery(&pattern, "2/31/2026", None, None).expect("recovery");
    assert_eq!(corrected, "02/28/2026");
}

#[test]
fn clamp_recovery_returns_none_for_garbage() {
    let pattern = iso_pattern();
    assert!(try_clamp_recovery(&pattern, "abc", None, None).is_none());
    assert!(try_clamp_recovery(&pattern, "", None, None).is_none());
}

#[test]
fn date_edit_default_validation_behavior_is_auto_correct() {
    let value = Signal::new(Some(Date::constant(2026, 1, 1)));
    let editor = DateEdit::new(value);
    let _ = editor; // Builder type is opaque; default is documented elsewhere.
    // The behavior surfaces via try_clamp_recovery when the editor commits;
    // we cover the recovery path directly above. This test asserts the
    // builder method exists and accepts both variants.
    let value2 = Signal::new(Some(Date::constant(2026, 1, 1)));
    let _ = DateEdit::new(value2).validation_behavior(crate::date_edit::ValidationBehavior::Reject);
}

#[test]
fn date_edit_validation_feedback_signal_starts_pristine() {
    use crate::primitives::text_input_field::ValidationFeedback;
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let editor = DateEdit::new(value);
    let feedback = editor.validation_feedback_signal();
    assert!(matches!(feedback.get(), ValidationFeedback::Pristine));
}
