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
fn the_value_is_the_two_days_in_full_joined_by_words() {
    // The value on the field's own node is written for someone listening:
    // the two days the way the locale writes them, joined by the locale's
    // words, not the ISO "2026-05-01/2026-05-10" it used to be. The locale
    // is read in `build()`, so the field is built in English and must speak
    // French after a switch.
    use crate::common::locale_switch_test::speaking;
    let (mgr, mut tree) = speaking("en-US");
    let range = Signal::new(Some(DateRange::new(
        Date::constant(2026, 5, 1),
        Date::constant(2026, 5, 10),
    )));
    let id = tree.add(DateRangeEdit::new(range));
    let spoken_value = |tree: &mut WidgetTree| {
        tree.layout(SizeProposal {
            width: Some(600.0),
            height: None,
        });
        let update = tree.sync_accessibility();
        let target = teksilo_core::accessibility::widget_id_to_node_id(id);
        update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == target)
            .and_then(|(_, node)| node.value().map(str::to_string))
            .expect("date range edit node with a value")
    };
    assert_eq!(
        spoken_value(&mut tree),
        "Friday, May 1, 2026 to Sunday, May 10, 2026"
    );

    mgr.set_locale("fr-FR".parse().unwrap());
    tree.set_locale("fr-FR".to_string());
    assert_eq!(
        spoken_value(&mut tree),
        "du vendredi premier mai 2026 au dimanche 10 mai 2026"
    );
    teksilo_i18n::thread_local::clear();
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

#[test]
fn the_popover_calendar_speaks_the_users_language() {
    // The popover is the shared `Calendar` in range mode. Opened on a French
    // tree it used to be "Calendar, mai 2026", valued
    // "2026-05-01 (selected: 2026-05-01 to 2026-05-05)".
    use crate::common::locale_switch_test::{speaking, spoken_grid};
    let (_mgr, mut tree) = speaking("fr-FR");
    let range = Signal::new(Some(DateRange::new(
        Date::constant(2026, 5, 1),
        Date::constant(2026, 5, 5),
    )));
    tree.add(DateRangeEdit::new(range));
    let lay_out = |tree: &mut WidgetTree| {
        tree.layout(SizeProposal {
            width: Some(600.0),
            height: None,
        })
    };
    lay_out(&mut tree);
    let trigger = tree
        .find_by_label("Ouvrir le calendrier de plage")
        .expect("the calendar trigger, named in French");
    tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::Click,
        target: Some(trigger),
        target_node: teksilo_core::accessibility::root_node_id(),
        data: None,
    });
    lay_out(&mut tree);
    assert_eq!(
        spoken_grid(&mut tree),
        (
            "Calendrier, mai 2026".to_string(),
            "vendredi premier mai 2026 (sélection : du vendredi premier mai 2026 au mardi 5 mai 2026)"
                .to_string(),
        )
    );
    teksilo_i18n::thread_local::clear();
}
