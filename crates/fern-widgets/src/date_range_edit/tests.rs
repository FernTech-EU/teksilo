use super::*;
use crate::calendar::DateRange;
use crate::common::datetime::Date;
use fern_canvas::SizeProposal;
use fern_core::signal::Signal;
use fern_core::widget_tree::WidgetTree;

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(fern_core::presets::intui::light())
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
    assert_eq!(info.role(), fern_core::accesskit::Role::DateInput);
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
    let target = fern_core::accessibility::widget_id_to_node_id(id);
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
