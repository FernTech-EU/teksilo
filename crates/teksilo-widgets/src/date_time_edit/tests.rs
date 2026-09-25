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

/// The value published on the `DateTimeInput` node itself.
fn spoken_value(tree: &mut WidgetTree, id: WidgetId) -> String {
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let update = tree.sync_accessibility();
    let target = teksilo_core::accessibility::widget_id_to_node_id(id);
    update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == target)
        .and_then(|(_, node)| node.value().map(str::to_string))
        .expect("date time edit node with a value")
}

#[test]
fn the_value_is_the_day_and_time_in_the_users_language() {
    // The value on the field's own node is written for someone listening:
    // the day in full and the time, the way the locale writes them, not the
    // ISO "2026-05-02T14:35:07" it used to be. The seconds are written only
    // when the field shows them. The locale is read in `build()`, so the
    // fields are built in English and must speak French after a switch.
    use crate::common::locale_switch_test::speaking;
    use crate::time_edit::SecondsMode;
    let (mgr, mut tree) = speaking("en-US");
    let hidden = tree.add(DateTimeEdit::new(Signal::new(Some(make_dt()))));
    let shown =
        tree.add(DateTimeEdit::new(Signal::new(Some(make_dt()))).seconds(SecondsMode::Editable));
    assert_eq!(
        spoken_value(&mut tree, hidden),
        "Saturday, May 2, 2026 at 2:35\u{202f}PM"
    );

    mgr.set_locale("fr-FR".parse().unwrap());
    tree.set_locale("fr-FR".to_string());
    assert_eq!(spoken_value(&mut tree, hidden), "samedi 2 mai 2026 à 14:35");
    assert_eq!(
        spoken_value(&mut tree, shown),
        "samedi 2 mai 2026 à 14:35:07"
    );
    teksilo_i18n::thread_local::clear();
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

#[test]
fn date_time_edit_re_derives_pattern_and_clock_when_the_locale_switches() {
    // This widget snapshots *two* locale-derived conventions in `build()`
    // — the date pattern and the 12-vs-24-hour clock — so both must
    // follow a `set_locale`.
    use crate::common::locale_switch_test::displayed_texts;

    let mut tree = light_tree();
    tree.set_locale("en-US".to_string());
    let value = Signal::new(Some(Date::constant(2026, 5, 2).at(14, 30, 0, 0)));
    let id = tree.add(DateTimeEdit::new(value));
    tree.layout(SizeProposal {
        width: Some(420.0),
        height: None,
    });
    let en = displayed_texts(&mut tree, id);
    assert!(
        en.iter().any(|t| t.starts_with("05/02/2026")),
        "en-US should render month-first; got {en:?}"
    );
    assert!(
        en.iter().any(|t| t.contains("PM") || t.contains("pm")),
        "en-US should render a 12-hour clock; got {en:?}"
    );

    tree.set_locale("fr-FR".to_string());
    tree.layout(SizeProposal {
        width: Some(420.0),
        height: None,
    });
    let fr = displayed_texts(&mut tree, id);
    assert!(
        fr.iter().any(|t| t.starts_with("02/05/2026")),
        "fr-FR should render day-first after the switch; got {fr:?}"
    );
    assert!(
        fr.iter().any(|t| t.starts_with("14")) && !fr.iter().any(|t| t.contains("PM")),
        "fr-FR should render a 24-hour clock after the switch; got {fr:?}"
    );
}

#[test]
fn the_popover_calendar_speaks_the_users_language() {
    // The popover is the shared `Calendar`, seeded with the date half.
    // Opened on a French tree it used to be "Calendar, mai 2026", valued
    // "2026-05-02 (selected: 2026-05-02)".
    use crate::common::locale_switch_test::{speaking, spoken_grid};
    let (_mgr, mut tree) = speaking("fr-FR");
    tree.add(DateTimeEdit::new(Signal::new(Some(make_dt()))));
    let lay_out = |tree: &mut WidgetTree| {
        tree.layout(SizeProposal {
            width: Some(600.0),
            height: None,
        })
    };
    lay_out(&mut tree);
    let trigger = tree
        .find_by_label("Ouvrir le calendrier")
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
            "samedi 2 mai 2026 (sélectionné)".to_string(),
        )
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn the_calendar_opens_on_the_date_the_date_part_holds() {
    // Up on the date part, then the calendar: it opened on the day the field
    // held when it was built, and Enter wrote that day back, keeping the time.
    use crate::common::heard_test::{Heard, Listener};
    use crate::common::locale_switch_test::speaking;
    use teksilo_core::event::{Key, Modifiers};
    let (_mgr, mut tree) = speaking("en-US");
    let value = Signal::new(Some(make_dt()));
    let id = tree.add(DateTimeEdit::new(value.clone()));
    let lay_out = |tree: &mut WidgetTree| {
        tree.layout(SizeProposal {
            width: Some(600.0),
            height: None,
        })
    };
    lay_out(&mut tree);
    let date_part = tree.first_focusable_descendant(id).expect("the date part");
    tree.focus(date_part);
    lay_out(&mut tree);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    lay_out(&mut tree);
    let held = value.get().expect("a value");
    assert_ne!(
        held.date(),
        Date::constant(2026, 5, 2),
        "the step moved the date"
    );

    let trigger = tree.find_by_label("Open calendar").expect("the trigger");
    tree.focus(trigger);
    lay_out(&mut tree);
    let mut listener = Listener::attach(&mut tree);
    tree.press_key(Key::Space, Modifiers::NONE);
    lay_out(&mut tree);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Focus(crate::common::datetime::written::full_date(
            held.date(),
            &"en-US".parse().unwrap()
        ))]
    );
    tree.press_key(Key::Enter, Modifiers::NONE);
    lay_out(&mut tree);
    assert_eq!(value.get(), Some(held), "Enter keeps the date and the time");
    teksilo_i18n::thread_local::clear();
}

#[test]
fn each_part_is_an_entry_named_for_what_it_holds() {
    // The two parts were re-roled `DateInput` and `TimeInput`, which the
    // AT-SPI adapter hands a reader as a date editor
    // (`accesskit_atspi_common` `node.rs`, `Role::DateInput |
    // Role::DateTimeInput | Role::TimeInput => AtspiRole::DateEditor`), and
    // Orca 46.1 reads a date editor by its name and role alone, with no
    // text: "Date date editor." (`formatting.py`, the `default` format). A
    // part is an editable text field, which a reader reads with its text;
    // the date and time semantics stay on the `DateTimeInput` around them.
    use crate::common::heard_test::Listener;
    use crate::common::locale_switch_test::speaking;
    let (_mgr, mut tree) = speaking("en-US");
    let id = tree.add(DateTimeEdit::new(Signal::new(Some(make_dt()))));
    let lay_out = |tree: &mut WidgetTree| {
        tree.layout(SizeProposal {
            width: Some(600.0),
            height: None,
        })
    };
    lay_out(&mut tree);
    let _listener = Listener::attach(&mut tree);
    let stops = tree.tab_stops_within(id);
    let mut parts = Vec::new();
    for stop in stops.iter().take(2) {
        tree.focus(*stop);
        lay_out(&mut tree);
        let platform = accesskit_consumer::Tree::new(tree.sync_accessibility(), true);
        let focus = platform.state().focus().expect("a focus");
        parts.push((focus.role(), focus.label().unwrap_or_default()));
    }
    assert_eq!(
        parts,
        vec![
            (teksilo_core::accesskit::Role::TextInput, "Date".to_string()),
            (teksilo_core::accesskit::Role::TextInput, "Time".to_string()),
        ]
    );
    teksilo_i18n::thread_local::clear();
}
