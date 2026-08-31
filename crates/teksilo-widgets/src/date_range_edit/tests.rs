// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;
use crate::calendar::DateRange;
use crate::common::datetime::Date;
use teksilo_canvas::SizeProposal;
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

#[test]
fn date_range_edit_builds_with_value() {
    let mut tree = light_tree();
    let range = Signal::new(Some(DateRange::new(
        Date::constant(2026, 5, 1),
        Date::constant(2026, 5, 10),
    )));
    let id = tree.add(DateRangeEdit::new(range));
    tree.layout(SizeProposal {
        width: Some(600.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
    assert!(bounds.height > 0.0);
}

#[test]
fn date_range_edit_builds_with_none_value() {
    let mut tree = light_tree();
    let range: Signal<Option<DateRange>> = Signal::new(None);
    let id = tree.add(DateRangeEdit::new(range));
    tree.layout(SizeProposal {
        width: Some(600.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
}

#[test]
fn date_range_edit_role_is_date_input() {
    let mut tree = light_tree();
    let range = Signal::new(Some(DateRange::new(
        Date::constant(2026, 5, 1),
        Date::constant(2026, 5, 10),
    )));
    let id = tree.add(DateRangeEdit::new(range));
    tree.layout(SizeProposal {
        width: Some(600.0),
        height: None,
    });
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), teksilo_core::accesskit::Role::DateInput);
}

#[test]
fn external_value_change_pushes_to_halves() {
    let range = Signal::new(Some(DateRange::new(
        Date::constant(2026, 5, 1),
        Date::constant(2026, 5, 10),
    )));
    let widget = DateRangeEdit::new(range.clone());
    let start_part = widget.start_part.clone();
    let end_part = widget.end_part.clone();

    let mut tree = light_tree();
    let _id = tree.add(widget);
    tree.layout(SizeProposal {
        width: Some(600.0),
        height: None,
    });

    // External write to outer range — start/end halves track it.
    range.set(Some(DateRange::new(
        Date::constant(2026, 6, 1),
        Date::constant(2026, 6, 30),
    )));
    tree.layout(SizeProposal {
        width: Some(600.0),
        height: None,
    });
    assert_eq!(start_part.get(), Some(Date::constant(2026, 6, 1)));
    assert_eq!(end_part.get(), Some(Date::constant(2026, 6, 30)));

    // Clear external value — halves clear too.
    range.set(None);
    tree.layout(SizeProposal {
        width: Some(600.0),
        height: None,
    });
    assert_eq!(start_part.get(), None);
    assert_eq!(end_part.get(), None);
}

#[test]
fn date_range_swaps_when_end_before_start() {
    // DateRange::new always produces start <= end. The composite
    // relies on this — when the user types an end date that's earlier
    // than the start, the bound range swaps automatically rather than
    // producing an invalid pair.
    let r = DateRange::new(Date::constant(2026, 5, 10), Date::constant(2026, 5, 1));
    assert!(r.start <= r.end);
    assert_eq!(r.start, Date::constant(2026, 5, 1));
    assert_eq!(r.end, Date::constant(2026, 5, 10));
}

#[test]
fn date_range_edit_value_iso_in_at_tree() {
    let mut tree = light_tree();
    let range = Signal::new(Some(DateRange::new(
        Date::constant(2026, 5, 1),
        Date::constant(2026, 5, 10),
    )));
    let id = tree.add(DateRangeEdit::new(range));
    tree.layout(SizeProposal {
        width: Some(600.0),
        height: None,
    });
    let update = tree.sync_accessibility();
    let target = teksilo_core::accessibility::widget_id_to_node_id(id);
    let (_, node) = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == target)
        .expect("date range edit node");
    let v = node.value().unwrap_or_default();
    assert!(v.contains("2026-05-01"));
    assert!(v.contains("2026-05-10"));
    assert!(v.contains("/"));
}

#[test]
fn date_range_edit_re_derives_its_pattern_when_the_locale_switches() {
    // Both ends of the range render through the same locale-derived
    // pattern, so both must follow a `set_locale`.
    use crate::common::locale_switch_test::displayed_texts;

    let mut tree = light_tree();
    tree.set_locale("en-US".to_string());
    let value = Signal::new(Some(DateRange::new(
        Date::constant(2026, 5, 2),
        Date::constant(2026, 6, 3),
    )));
    let id = tree.add(DateRangeEdit::new(value));
    tree.layout(SizeProposal {
        width: Some(480.0),
        height: None,
    });
    let en = displayed_texts(&mut tree, id);
    assert!(
        en.iter().any(|t| t.starts_with("05/02/2026")),
        "en-US should render month-first; got {en:?}"
    );

    tree.set_locale("fr-FR".to_string());
    tree.layout(SizeProposal {
        width: Some(480.0),
        height: None,
    });
    let fr = displayed_texts(&mut tree, id);
    assert!(
        fr.iter().any(|t| t.starts_with("02/05/2026")),
        "fr-FR should render day-first after the switch; got {fr:?}"
    );
    assert!(
        fr.iter().any(|t| t.starts_with("03/06/2026")),
        "the range end must follow too; got {fr:?}"
    );
}
