// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;
use crate::common::datetime::Date;
use crate::common::locale_switch_test::{speaking, spoken_grid as grid};
use std::rc::Rc;
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

#[test]
fn single_calendar_builds_with_value() {
    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(Calendar::single(date));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
    assert!(bounds.height > 0.0);
}

#[test]
fn single_calendar_builds_with_none_value() {
    let mut tree = light_tree();
    let date = Signal::new(None::<Date>);
    let id = tree.add(Calendar::single(date));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
    assert!(bounds.height > 0.0);
}

#[test]
fn range_calendar_builds() {
    let mut tree = light_tree();
    let range = Signal::new(None::<DateRange>);
    let id = tree.add(Calendar::range(range));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
}

#[test]
fn calendar_role_is_grid() {
    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(Calendar::single(date));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), teksilo_core::accesskit::Role::Grid);
}

// ── What a screen reader is told, and how many times ─────────────

use crate::common::heard_test::{Heard, Listener};
use teksilo_core::event::{Key, Modifiers};

/// A French grid on 12 March 2027 holding keyboard focus, and a screen
/// reader listening from that moment.
fn focused_french_grid(
    calendar: Calendar,
) -> (Rc<teksilo_i18n::I18nManager>, WidgetTree, Listener) {
    let (mgr, mut tree) = speaking("fr-FR");
    let id = tree.add(calendar);
    lay_out(&mut tree);
    tree.focus(id);
    lay_out(&mut tree);
    let listener = Listener::attach(&mut tree);
    (mgr, tree, listener)
}

fn press(tree: &mut WidgetTree, key: Key) {
    tree.press_key(key, Modifiers::NONE);
    lay_out(tree);
}

fn twelfth_of_march() -> Calendar {
    Calendar::single(Signal::new(Some(Date::constant(2027, 3, 12))))
}

#[test]
fn an_arrow_press_is_a_focus_change_to_the_next_day() {
    // Orca heard nothing as the cursor moved: the grid's name changes only
    // with the month, and the value, where the cursor was, reaches no AT-SPI
    // interface. UIA and macOS heard the value change. Every platform
    // speaks a focus change, and the day under the cursor is now the focus.
    let (_mgr, mut tree, mut listener) = focused_french_grid(twelfth_of_march());
    press(&mut tree, Key::ArrowRight);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Focus("samedi 13 mars 2027".to_string())]
    );
    press(&mut tree, Key::ArrowDown);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Focus("samedi 20 mars 2027".to_string())]
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn a_change_of_month_from_the_keyboard_is_said_once() {
    // The grid used to be a live region, so PageDown announced its new name
    // and Orca spoke the rename of its focus as well: the month, twice. Now
    // it is one focus change, to a day of the new month whose name says the
    // month, while the rename of the grid, no longer the focus, is not
    // spoken (Orca's `onNameChanged` speaks only its locus of focus).
    let (_mgr, mut tree, mut listener) = focused_french_grid(twelfth_of_march());
    press(&mut tree, Key::PageDown);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Focus("lundi 12 avril 2027".to_string())]
    );
    teksilo_i18n::thread_local::clear();
}

/// Every node a platform adapter walks under the calendar's grid, as
/// `(role, name)` with each week's days after it, in order.
fn platform_tree_of_the_grid(
    tree: &mut WidgetTree,
) -> Vec<(teksilo_core::accesskit::Role, String)> {
    fn walk(
        node: accesskit_consumer::NodeRef<'_>,
        out: &mut Vec<(teksilo_core::accesskit::Role, String)>,
    ) {
        out.push((node.role(), node.label().unwrap_or_default()));
        for child in node.filtered_children(&accesskit_consumer::common_filter) {
            walk(child, out);
        }
    }
    let platform = accesskit_consumer::Tree::new(tree.sync_accessibility(), true);
    let state = platform.state();
    let mut out = Vec::new();
    for child in state
        .root()
        .filtered_children(&accesskit_consumer::common_filter)
        .filter(|node| node.role() == teksilo_core::accesskit::Role::Grid)
    {
        walk(child, &mut out);
    }
    out
}

#[test]
fn every_day_is_in_the_platform_tree_week_by_week() {
    // The day grid and the header used to be hidden nodes, meant as
    // "publish nothing of my own" and read by every adapter as "drop me and
    // everything under me". A screen reader could not read a single day, nor
    // find a header button that did not hold focus.
    use teksilo_core::accesskit::Role;
    let (_mgr, mut tree) = speaking("fr-FR");
    tree.add(twelfth_of_march());
    lay_out(&mut tree);
    let nodes = platform_tree_of_the_grid(&mut tree);
    let buttons: Vec<&str> = nodes
        .iter()
        .filter(|(role, _)| *role == Role::Button)
        .map(|(_, name)| name.as_str())
        .collect();
    assert_eq!(
        buttons,
        [
            "Année précédente",
            "Mois précédent",
            "mars 2027",
            "Mois suivant",
            "Année suivante"
        ]
    );
    // One row of weekday headers, then six weeks of seven days.
    let mut weeks: Vec<Vec<&str>> = Vec::new();
    for (role, name) in &nodes {
        match role {
            Role::Row => weeks.push(Vec::new()),
            Role::GridCell => weeks
                .last_mut()
                .expect("a day inside a row")
                .push(name.as_str()),
            _ => {}
        }
    }
    assert_eq!(weeks.len(), 7, "{nodes:?}");
    assert!(
        weeks[0].is_empty(),
        "the first row holds the weekday headers"
    );
    assert!(weeks[1..].iter().all(|week| week.len() == 7), "{weeks:?}");
    assert_eq!(weeks[1][0], "lundi premier mars 2027");
    assert_eq!(weeks[2][4], "vendredi 12 mars 2027");
    assert_eq!(weeks[6][6], "dimanche 11 avril 2027");
    teksilo_i18n::thread_local::clear();
}

#[test]
fn the_grid_names_a_day_only_while_it_holds_focus_in_the_day_view() {
    // A container without focus has no descendant to speak of, and in the
    // month or year view no day is on screen: the grid itself is the focus.
    let calendar = twelfth_of_march();
    let mode = calendar.mode_signal();
    let (_mgr, mut tree) = speaking("fr-FR");
    let id = tree.add(calendar);
    lay_out(&mut tree);
    let platform_focus = |tree: &mut WidgetTree| -> (teksilo_core::accesskit::Role, String) {
        let platform = accesskit_consumer::Tree::new(tree.sync_accessibility(), true);
        let focus = platform.state().focus().expect("the window has focus");
        (focus.role(), focus.label().unwrap_or_default())
    };
    let grid_names_a_day = |tree: &mut WidgetTree| -> bool {
        tree.sync_accessibility().nodes.iter().any(|(_, node)| {
            node.role() == teksilo_core::accesskit::Role::Grid && node.active_descendant().is_some()
        })
    };
    assert!(!grid_names_a_day(&mut tree), "no focus, no day named");

    tree.focus(id);
    lay_out(&mut tree);
    assert_eq!(
        platform_focus(&mut tree),
        (
            teksilo_core::accesskit::Role::GridCell,
            "vendredi 12 mars 2027".to_string()
        )
    );

    mode.set(CalendarMode::Months);
    lay_out(&mut tree);
    assert!(!grid_names_a_day(&mut tree), "no day on screen, none named");
    assert_eq!(
        platform_focus(&mut tree),
        (
            teksilo_core::accesskit::Role::Grid,
            "Calendrier, mars 2027".to_string()
        )
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn each_header_button_tab_reaches_is_said_once() {
    // A descendant of a live node is live too, and each header button used to
    // enter the platform's tree only as it took focus, the header above it
    // being hidden, so under the live grid each one was announced as it
    // arrived and then spoken again as the new focus. With the header in the
    // tree, Tab no longer adds a button a live grid could announce (the tests
    // above keep the grid from being live), so what this holds is the button
    // being in the tree before Tab reaches it, where object navigation finds
    // it too, and said once when Tab does.
    let (_mgr, mut tree, mut listener) = focused_french_grid(twelfth_of_march());
    for name in [
        "Année précédente",
        "Mois précédent",
        "mars 2027",
        "Mois suivant",
    ] {
        assert!(
            listener.finds(teksilo_core::accesskit::Role::Button, name),
            "{name} is in the tree before Tab reaches it"
        );
        press(&mut tree, Key::Tab);
        assert_eq!(
            listener.heard(&mut tree),
            vec![Heard::Focus(name.to_string())],
            "Tab to {name}"
        );
    }
    teksilo_i18n::thread_local::clear();
}

#[test]
fn a_header_arrow_announces_where_it_moved_to_once() {
    // Focus stays on the arrow, whose name does not change, so the month it
    // moves to has to be announced, and only by the arrow: the grid is no
    // longer a live region that would say it again. In the months view the
    // same arrow steps a year and says the year.
    let calendar = twelfth_of_march();
    let mode = calendar.mode_signal();
    let (_mgr, mut tree, mut listener) = focused_french_grid(calendar);
    let next = tree
        .find_by_label("Mois suivant")
        .expect("the next-month arrow");
    tree.focus(next);
    lay_out(&mut tree);
    let _ = listener.heard(&mut tree);

    press(&mut tree, Key::Enter);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Live("avril 2027".to_string())]
    );

    mode.set(CalendarMode::Months);
    lay_out(&mut tree);
    let _ = listener.heard(&mut tree);
    press(&mut tree, Key::Enter);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Live("2028".to_string())]
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn a_header_arrow_clicked_while_the_grid_has_focus_is_not_announced() {
    // An assistive technology can click the arrow without moving focus off
    // the grid. The grid's own change then says the new month, so the arrow
    // must not announce it a second time.
    let (_mgr, mut tree, mut listener) = focused_french_grid(twelfth_of_march());
    let next = tree
        .find_by_label("Mois suivant")
        .expect("the next-month arrow");
    tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::Click,
        target: Some(next),
        target_node: teksilo_core::accessibility::root_node_id(),
        data: None,
    });
    lay_out(&mut tree);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Focus("lundi 12 avril 2027".to_string())],
        "the change is heard through the grid, and only there"
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn the_today_button_announces_the_day_it_moved_to_once() {
    // The same reason as the arrows: focus stays on the button. The day is
    // said in full, which says its month too.
    let calendar = twelfth_of_march().show_today_button(true);
    let (_mgr, mut tree, mut listener) = focused_french_grid(calendar);
    let today = resolve_message_widget("calendar-button-today", &[]);
    let button = tree.find_by_label(&today).expect("the Today button");
    tree.focus(button);
    lay_out(&mut tree);
    let _ = listener.heard(&mut tree);

    press(&mut tree, Key::Enter);
    let lang: LanguageIdentifier = "fr-FR".parse().unwrap();
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Live(full_date(today_local(), &lang))]
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn the_today_button_clicked_while_the_grid_has_focus_is_not_announced() {
    // The arrows' case again, for the Today button: clicked by an assistive
    // technology while the grid keeps focus, the cursor's move to today is a
    // focus change to that day, and an announcement would say it twice.
    let (_mgr, mut tree, mut listener) =
        focused_french_grid(twelfth_of_march().show_today_button(true));
    let today = resolve_message_widget("calendar-button-today", &[]);
    let button = tree.find_by_label(&today).expect("the Today button");
    tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::Click,
        target: Some(button),
        target_node: teksilo_core::accessibility::root_node_id(),
        data: None,
    });
    lay_out(&mut tree);
    let lang: LanguageIdentifier = "fr-FR".parse().unwrap();
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Focus(full_date(today_local(), &lang))],
        "the move is heard through the grid, and only there"
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn an_assistive_focus_on_a_day_leaves_the_keyboard_on_the_grid() {
    // A day is the grid's active descendant and takes no keys of its own.
    // It used to offer `Action::Focus`, which the dispatcher services by
    // moving keyboard focus onto the node named, focusable or not. After a
    // UIA `SetFocus` on a day, or VoiceOver's keyboard focus following its
    // cursor there, the grid had lost focus and named no descendant, and the
    // next arrow press moved the cursor in silence, the platform's focus
    // left on the day that had been focused.
    let calendar = twelfth_of_march();
    let (_mgr, mut tree, mut listener) = focused_french_grid(calendar);
    let grid = tree.focused();
    let day = tree
        .find_by_label("mardi 16 mars 2027")
        .expect("a day of the month on show");
    assert!(
        !tree
            .accessibility_node(day)
            .actions()
            .contains(&teksilo_core::accesskit::Action::Focus),
        "a day offers no focus of its own"
    );
    tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::Focus,
        target: Some(day),
        target_node: teksilo_core::accessibility::root_node_id(),
        data: None,
    });
    lay_out(&mut tree);
    assert_eq!(tree.focused(), grid, "keyboard focus stays on the grid");
    assert_eq!(listener.heard(&mut tree), vec![]);
    press(&mut tree, Key::ArrowRight);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Focus("samedi 13 mars 2027".to_string())]
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn a_day_has_nothing_under_it_a_screen_reader_would_read_instead() {
    // Orca reads a focused table cell by the text of its children when it
    // has any, and by its own name only when it has none (Orca 46.1,
    // `orca/generator.py`, `_generateRealActiveDescendantDisplayedText`). The
    // digit drawn in each day is kept out of the tree for that reason: in
    // it, "12" would be what Orca said for every day the cursor reached, in
    // place of "vendredi 12 mars 2027".
    let (_mgr, mut tree) = speaking("fr-FR");
    tree.add(twelfth_of_march());
    lay_out(&mut tree);
    fn walk(node: accesskit_consumer::NodeRef<'_>, days: &mut Vec<(String, Vec<String>)>) {
        let children: Vec<accesskit_consumer::NodeRef<'_>> = node
            .filtered_children(&accesskit_consumer::common_filter)
            .collect();
        if node.role() == teksilo_core::accesskit::Role::GridCell {
            days.push((
                node.label().unwrap_or_default(),
                children
                    .iter()
                    .map(|child| format!("{:?} {:?}", child.role(), child.label()))
                    .collect(),
            ));
        }
        for child in children {
            walk(child, days);
        }
    }
    let platform = accesskit_consumer::Tree::new(tree.sync_accessibility(), true);
    let mut days = Vec::new();
    walk(platform.state().root(), &mut days);
    assert_eq!(days.len(), 42, "{days:?}");
    let with_children: Vec<_> = days
        .iter()
        .filter(|(_, children)| !children.is_empty())
        .collect();
    assert!(with_children.is_empty(), "{with_children:?}");
    teksilo_i18n::thread_local::clear();
}

// ── What the calendar says, in the user's language ──────────────

fn lay_out(tree: &mut WidgetTree) {
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
}

/// Name and value of every published node with `role`, in tree order.
fn spoken(tree: &mut WidgetTree, role: teksilo_core::accesskit::Role) -> Vec<(String, String)> {
    tree.sync_accessibility()
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == role)
        .map(|(_, node)| {
            (
                node.label().unwrap_or_default().to_string(),
                node.value().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

fn day_names(tree: &mut WidgetTree) -> Vec<String> {
    spoken(tree, teksilo_core::accesskit::Role::GridCell)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

fn button_named(tree: &mut WidgetTree, name: &str) -> bool {
    spoken(tree, teksilo_core::accesskit::Role::Button)
        .iter()
        .any(|(label, _)| label == name)
}

#[test]
fn the_calendar_speaks_french_to_a_french_user() {
    // What a French screen reader user meets when a date picker opens on
    // 12 March 2027: no English word, no ISO date, every day the way French
    // writes it. 1 March 2027 is a Monday, the first day of a French week,
    // so it opens the grid. The cursor opens on the selected day, which the
    // value says once, not as the cursor and then the selection.
    let (_mgr, mut tree) = speaking("fr-FR");
    let date = Signal::new(Some(Date::constant(2027, 3, 12)));
    tree.add(Calendar::single(date));
    lay_out(&mut tree);

    assert_eq!(
        grid(&mut tree),
        (
            "Calendrier, mars 2027".to_string(),
            "vendredi 12 mars 2027 (sélectionné)".to_string(),
        )
    );
    let days = day_names(&mut tree);
    assert_eq!(days.len(), 42, "six weeks of days: {days:?}");
    assert_eq!(days[0], "lundi premier mars 2027");
    assert!(
        days.contains(&"vendredi 12 mars 2027".to_string()),
        "{days:?}"
    );
    assert_eq!(days[41], "dimanche 11 avril 2027");
    assert!(
        button_named(&mut tree, "mars 2027"),
        "the title button is the month and its year"
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn the_calendar_speaks_english_to_an_english_user() {
    let (_mgr, mut tree) = speaking("en-US");
    let date = Signal::new(Some(Date::constant(2027, 3, 12)));
    tree.add(Calendar::single(date));
    lay_out(&mut tree);

    assert_eq!(
        grid(&mut tree),
        (
            "Calendar, March 2027".to_string(),
            "Friday, March 12, 2027 (selected)".to_string(),
        )
    );
    let days = day_names(&mut tree);
    // en-US starts the week on Sunday.
    assert_eq!(days[0], "Sunday, February 28, 2027");
    assert!(
        days.contains(&"Friday, March 12, 2027".to_string()),
        "{days:?}"
    );
    assert!(button_named(&mut tree, "March 2027"));
    teksilo_i18n::thread_local::clear();
}

#[test]
fn the_month_and_the_year_are_in_the_order_the_locale_writes_them() {
    // Japanese writes the year first. A title assembled as "<month> <year>"
    // from the translated month name gives "3月 2027"; the month and its
    // year come from ICU as one field set, in the locale's order.
    let (_mgr, mut tree) = speaking("ja-JP");
    tree.add(Calendar::single(Signal::new(Some(Date::constant(
        2027, 3, 12,
    )))));
    lay_out(&mut tree);
    assert_eq!(grid(&mut tree).0, "カレンダー、2027年3月");
    assert!(
        button_named(&mut tree, "2027年3月"),
        "buttons: {:?}",
        spoken(&mut tree, teksilo_core::accesskit::Role::Button)
    );
    assert!(day_names(&mut tree).contains(&"2027年3月12日金曜日".to_string()));
    teksilo_i18n::thread_local::clear();
}

#[test]
fn the_grid_is_named_on_the_calendar_it_is_laid_out_in() {
    // CLDR gives Persian the Persian calendar, and the grid lays March 2027
    // out on the Gregorian one. Written in the locale's calendar, its title
    // would be Esfand 1405, a month most of the days under it do not belong
    // to, and each cell would name a different day number from the one drawn
    // in it. The words stay English here: only the tree speaks Persian.
    let (_mgr, mut tree) = speaking("en-US");
    tree.set_locale("fa-IR".to_string());
    tree.add(Calendar::single(Signal::new(Some(Date::constant(
        2027, 3, 12,
    )))));
    lay_out(&mut tree);
    assert_eq!(grid(&mut tree).0, "Calendar, مارس ۲۰۲۷");
    assert!(
        button_named(&mut tree, "مارس ۲۰۲۷"),
        "buttons: {:?}",
        spoken(&mut tree, teksilo_core::accesskit::Role::Button)
    );
    let days = day_names(&mut tree);
    assert!(days.contains(&"جمعه ۱۲ مارس ۲۰۲۷".to_string()), "{days:?}");
    teksilo_i18n::thread_local::clear();
}

#[test]
fn the_first_of_the_month_is_an_ordinal_in_italian_and_romanian() {
    // Only the tree speaks Italian or Romanian here, so the framework's words
    // stay English while every date is the locale's. The grid of March 2027
    // opens on Monday the first in both.
    for (tag, first, twelfth) in [
        ("it-IT", "lunedì primo marzo 2027", "venerdì 12 marzo 2027"),
        ("ro-RO", "luni, întâi martie 2027", "vineri, 12 martie 2027"),
    ] {
        let (_mgr, mut tree) = speaking("en-US");
        tree.set_locale(tag.to_string());
        tree.add(twelfth_of_march());
        lay_out(&mut tree);
        let days = day_names(&mut tree);
        assert_eq!(days[0], first, "{tag}");
        assert!(days.contains(&twelfth.to_string()), "{tag}: {days:?}");
        teksilo_i18n::thread_local::clear();
    }
}

#[test]
fn the_cursor_is_spoken_in_full_and_apart_from_the_selection() {
    // The value leads with the day under the keyboard cursor, the part that
    // changes as the user arrows, and keeps the selection after it.
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};
    let (_mgr, mut tree) = speaking("fr-FR");
    let date = Signal::new(Some(Date::constant(2027, 3, 12)));
    let id = tree.add(Calendar::single(date));
    lay_out(&mut tree);
    tree.focus(id);
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::NONE,
        text: None,
    });
    lay_out(&mut tree);
    assert_eq!(
        grid(&mut tree).1,
        "samedi 13 mars 2027 (sélection : vendredi 12 mars 2027)"
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn an_empty_calendar_speaks_only_the_cursor() {
    let (_mgr, mut tree) = speaking("fr-FR");
    let calendar = Calendar::single(Signal::new(None));
    calendar
        .focused_date_signal()
        .set(Date::constant(2027, 3, 12));
    calendar
        .visible_month_signal()
        .set(crate::common::datetime::types::YearMonth::new(2027, 3));
    tree.add(calendar);
    lay_out(&mut tree);
    assert_eq!(grid(&mut tree).1, "vendredi 12 mars 2027");
    teksilo_i18n::thread_local::clear();
}

#[test]
fn a_range_is_spoken_in_words() {
    // The grid's value joins the two ends with words, where an en-dash
    // would be skipped by some screen readers.
    let (_mgr, mut tree) = speaking("fr-FR");
    let range = Signal::new(Some(DateRange::new(
        Date::constant(2027, 3, 1),
        Date::constant(2027, 3, 12),
    )));
    tree.add(Calendar::range(range));
    lay_out(&mut tree);

    assert_eq!(
        grid(&mut tree).1,
        "lundi premier mars 2027 (sélection : du lundi premier mars 2027 au vendredi 12 mars 2027)"
    );
    teksilo_i18n::thread_local::clear();
}

/// The text of every `TextWidget` at or below `root`, hidden from assistive
/// technology or not: what a sighted user reads. Read off the widgets
/// themselves, because a hidden label publishes no node to read it from.
fn drawn_texts(tree: &WidgetTree, root: WidgetId) -> Vec<String> {
    let mut found = Vec::new();
    let mut queue = vec![root];
    while let Some(id) = queue.pop() {
        if let Some(text) = tree
            .widget_as_any(id)
            .and_then(|widget| widget.downcast_ref::<TextWidget>())
        {
            found.push(text.resolved_text());
        }
        queue.extend(tree.children(id));
    }
    found
}

#[test]
fn the_range_status_line_follows_the_range_in_the_users_language() {
    // The line under a range calendar is on screen only, hidden from
    // assistive technology because the value says the same in words, so it
    // is read from the labels the footer draws. It follows the committed
    // range wherever the commit came from: here the application commits one
    // after the calendar is built, which no click on a cell ever saw.
    let (_mgr, mut tree) = speaking("fr-FR");
    let range = Signal::new(None);
    let root = tree.add(Calendar::range(range.clone()));
    lay_out(&mut tree);
    let before = drawn_texts(&tree, root);
    assert!(
        !before.iter().any(|text| text.starts_with("Sélection")),
        "nothing is selected yet: {before:?}"
    );

    range.set(Some(DateRange::new(
        Date::constant(2027, 3, 1),
        Date::constant(2027, 3, 12),
    )));
    lay_out(&mut tree);
    let after = drawn_texts(&tree, root);
    assert!(
        after.contains(&"Sélection : 1er mars 2027 – 12 mars 2027".to_string()),
        "{after:?}"
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn the_decade_title_is_spoken_in_words() {
    let (_mgr, mut tree) = speaking("fr-FR");
    let calendar = Calendar::single(Signal::new(Some(Date::constant(2027, 3, 12))));
    calendar.mode_signal().set(CalendarMode::Years);
    tree.add(calendar);
    lay_out(&mut tree);
    assert!(
        button_named(&mut tree, "de 2020 à 2029"),
        "buttons: {:?}",
        spoken(&mut tree, teksilo_core::accesskit::Role::Button)
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn a_language_switch_reaches_every_date_the_calendar_speaks() {
    // The dates are written when the calendar builds, and a switch rebuilds
    // it: the same widget, switched from English to French, must speak
    // French everywhere, not keep the English it was first built in.
    let (mgr, mut tree) = speaking("en-US");
    tree.add(Calendar::single(Signal::new(Some(Date::constant(
        2027, 3, 12,
    )))));
    lay_out(&mut tree);
    assert_eq!(grid(&mut tree).0, "Calendar, March 2027");

    mgr.set_locale("fr-FR".parse().unwrap());
    tree.set_locale("fr-FR".to_string());
    lay_out(&mut tree);
    assert_eq!(
        grid(&mut tree),
        (
            "Calendrier, mars 2027".to_string(),
            "vendredi 12 mars 2027 (sélectionné)".to_string(),
        )
    );
    // Monday first now, where en-US had Sunday 28 February.
    assert_eq!(day_names(&mut tree)[0], "lundi premier mars 2027");
    assert!(button_named(&mut tree, "mars 2027"));
    teksilo_i18n::thread_local::clear();
}

#[test]
fn calendar_rebuilds_on_month_navigation() {
    // Mutating `visible_month` after the initial layout must
    // re-`build()` the calendar body, regenerating cells with the
    // new month's dates. Before the fix, BindingLevel::Relayout
    // only triggered measure, leaving cells stuck on the original
    // month's dates.
    use crate::common::datetime::types::YearMonth;
    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let calendar = Calendar::single(date);
    let visible_month = calendar.visible_month_signal();
    let id = tree.add(calendar);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let initial_descendant_count = count_descendants(&tree, id);

    // Navigate forward a month — the day-grid widget should re-build
    // with the new month's cells. The descendant count should stay
    // the same (always 6 weeks × 7 days = 42 cells), but the cell
    // labels would change. Asserting on the count ensures the
    // rebuild path actually runs and doesn't accumulate stale nodes.
    visible_month.set(YearMonth::new(2026, 6));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let after_nav_count = count_descendants(&tree, id);
    // Counts may differ by exactly 1 because the today-cell ring is
    // an optional extra node and "today" lives in only one of the
    // two months. The key invariant is "no accumulation": rebuilds
    // shouldn't leak previous-month cells into the new tree, so the
    // total stays bounded near the expected ~290 nodes per month.
    assert!(
        (initial_descendant_count as i64 - after_nav_count as i64).abs() <= 1,
        "rebuild leaked nodes: {} → {}",
        initial_descendant_count,
        after_nav_count
    );
    // Also assert we're in the right ballpark — 6 rows × 7 cells +
    // header/footer/etc. for the day grid (~290), plus the dormant
    // Months and Years zoom grids (12 cells each, mounted but not
    // visible — Switcher mounts all children to avoid rebuild
    // churn on mode flips). Total lands around 520. The check is
    // a leak detector: as long as it's bounded, we're not piling up
    // stale per-month nodes across navigations.
    assert!(
        after_nav_count > 200 && after_nav_count < 800,
        "expected calendar descendant count in 200..800, got {after_nav_count}"
    );
}

fn count_descendants(tree: &WidgetTree, root: WidgetId) -> usize {
    let mut count = 0;
    let mut queue = vec![root];
    while let Some(id) = queue.pop() {
        count += 1;
        queue.extend(tree.children(id));
    }
    count
}

/// Find a day cell by the AT name it publishes — the same way a screen
/// reader addresses it. `DayCell` is `Role::GridCell` and names itself
/// with the full date, which with no locale installed is en-US's
/// "<weekday>, <month> <day>, <year>".
fn find_day_cell(tree: &WidgetTree, root: WidgetId, day: u8) -> WidgetId {
    let needle = format!(" {day}, ");
    let mut queue = vec![root];
    while let Some(id) = queue.pop() {
        let node = tree.accessibility_node(id);
        if node.role() == teksilo_core::accesskit::Role::GridCell
            && node.name().is_some_and(|n| n.contains(&needle))
        {
            return id;
        }
        queue.extend(tree.children(id));
    }
    panic!("no day cell for day {day}");
}

/// A day cell advertises `Action::Click`; invoking it (screen reader,
/// automation bridge) must commit the date. Without a handler the cell is
/// announced as clickable and then does nothing — the cell is not
/// arena-disabled, so its `interactable` guard has to be honoured on the
/// AT path too.
#[test]
fn access_click_on_day_cell_commits_the_date() {
    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = tree.add(Calendar::single(date.clone()));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });

    let cell = find_day_cell(&tree, cal, 17);
    tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::Click,
        target: Some(cell),
        target_node: teksilo_core::accessibility::root_node_id(),
        data: None,
    });

    assert_eq!(
        date.get(),
        Some(Date::constant(2026, 5, 17)),
        "AT click on a day cell must commit that date"
    );
}

#[test]
fn range_mode_first_commit_parks_anchor_second_commit_sets_value() {
    // The first commit in range mode parks the anchor without touching the
    // bound `value` signal; only the second commit publishes the range. We
    // drive the real widget through its keyboard path (the calendar root owns
    // `.focusable` + `.on_key`, and Enter commits the focused day) rather than
    // poking module-private internals — the second commit doubles as a
    // positive control proving the keystrokes actually land.
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};

    let mut tree = light_tree();
    let value: Signal<Option<DateRange>> = Signal::new(None);
    let id = tree.add(Calendar::range(value.clone()));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    assert_eq!(value.get(), None, "no value before any commit");

    tree.focus(id);

    let press = |tree: &mut WidgetTree, key: Key| {
        tree.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers: Modifiers::NONE,
            text: None,
        });
    };

    // First Enter: park the anchor on the focused day. `value` must stay None
    // (observers shouldn't see a transient one-day range).
    press(&mut tree, Key::Enter);
    assert_eq!(
        value.get(),
        None,
        "first commit must park the anchor without publishing a range"
    );

    // Move focus, then a second Enter commits the range.
    press(&mut tree, Key::ArrowRight);
    press(&mut tree, Key::Enter);
    let committed = value.get();
    assert!(
        committed.is_some(),
        "second commit must publish a range (also proves the keystrokes landed)"
    );
    let range = committed.unwrap();
    assert!(
        range.start < range.end,
        "ArrowRight then commit should span two adjacent days, got {range:?}"
    );
}

#[test]
fn date_range_invariant() {
    let r = DateRange::new(Date::constant(2026, 5, 5), Date::constant(2026, 5, 1));
    assert!(r.start <= r.end);
    assert_eq!(r.start, Date::constant(2026, 5, 1));
    assert_eq!(r.end, Date::constant(2026, 5, 5));
}

#[test]
fn date_range_contains_inclusive() {
    let r = DateRange::new(Date::constant(2026, 5, 1), Date::constant(2026, 5, 5));
    assert!(r.contains(Date::constant(2026, 5, 1)));
    assert!(r.contains(Date::constant(2026, 5, 3)));
    assert!(r.contains(Date::constant(2026, 5, 5)));
    assert!(!r.contains(Date::constant(2026, 4, 30)));
    assert!(!r.contains(Date::constant(2026, 5, 6)));
}

// ── Header-zoom mode behaviour ──────────────────────────────────

#[test]
fn calendar_mode_default_is_days() {
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = Calendar::single(date);
    assert_eq!(cal.mode_signal().get(), CalendarMode::Days);
}

#[test]
fn calendar_mode_demote_chain() {
    assert_eq!(CalendarMode::Days.demote(), CalendarMode::Months);
    assert_eq!(CalendarMode::Months.demote(), CalendarMode::Years);
    // Years is the coarsest; further demote is a no-op.
    assert_eq!(CalendarMode::Years.demote(), CalendarMode::Years);
}

#[test]
fn calendar_mode_signal_writable_for_programmatic_zoom() {
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = Calendar::single(date);
    let mode = cal.mode_signal();
    mode.set(CalendarMode::Years);
    assert_eq!(mode.get(), CalendarMode::Years);
    let mut tree = light_tree();
    let _id = tree.add(cal);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    // Mode persists across the build pass — read again to confirm.
    assert_eq!(mode.get(), CalendarMode::Years);
}

#[test]
fn years_grid_decade_calculation() {
    use crate::calendar::zoom_grid::YearsGrid;
    assert_eq!(YearsGrid::decade_of(2026), 2020);
    assert_eq!(YearsGrid::decade_of(2020), 2020);
    assert_eq!(YearsGrid::decade_of(2029), 2020);
    assert_eq!(YearsGrid::decade_of(2030), 2030);
    assert_eq!(YearsGrid::decade_of(1999), 1990);
    assert_eq!(YearsGrid::decade_of(0), 0);
}

#[test]
fn calendar_title_button_click_demotes_mode() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = Calendar::single(date);
    let mode = cal.mode_signal();
    let mut tree = light_tree();
    let id = tree.add(cal);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    assert_eq!(mode.get(), CalendarMode::Days);

    // Click in the horizontal centre of the calendar at a y near
    // the top — that's where the header label sits ("May 2026").
    let bounds = tree.bounds(id);
    let click_pos = Point::new(bounds.x + bounds.width / 2.0, bounds.y + 20.0);
    tree.dispatch_event(WidgetEvent::pointer_down(
        click_pos,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        click_pos,
        PointerButton::Primary,
        Modifiers::NONE,
    ));

    // After the click, mode should have demoted to Months.
    assert_eq!(
        mode.get(),
        CalendarMode::Months,
        "tap on header title should demote mode Days → Months; \
         got {:?}. The TitleButton's on_tap is not firing.",
        mode.get()
    );
}

#[test]
fn calendar_months_body_does_not_collapse_to_left_edge() {
    // Regression for the zoom-mode "body collapses to leading edge"
    // bug: in Months / Years mode, cells were rendered at zero
    // wanted-width, the row's natural width fell to ~spacing, and
    // the parent VStack assigned cross-axis width = wanted (small).
    // Visually the body shrank to a tiny column on the left.
    //
    // Fix: each zoom cell is wrapped in `FixedSize(cell_width,
    // cell_height)` so the row reports a real natural width.
    // Verify by checking that descendants of the calendar in zoom
    // mode have bounds spanning a meaningful fraction of the
    // calendar's width (not collapsed to ~zero).
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = Calendar::single(date);
    let mode = cal.mode_signal();
    mode.set(CalendarMode::Months);
    let mut tree = light_tree();
    let id = tree.add(cal);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    // Second pass settles visibility / activation flips.
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });

    // Walk the tree; the maximum-x cell across all descendants
    // should land beyond ~50% of the calendar's width if the months
    // grid distributes its cells. Pre-fix the rightmost descendant
    // sat near x=8 (just inside the outer padding).
    let cal_bounds = tree.bounds(id);
    let mut max_right: f32 = 0.0;
    let mut stack = vec![id];
    while let Some(node_id) = stack.pop() {
        let b = tree.bounds(node_id);
        if b.width > 0.0 {
            max_right = max_right.max(b.right());
        }
        for child in tree.children(node_id) {
            stack.push(child);
        }
    }
    let half_width = cal_bounds.x + cal_bounds.width * 0.5;
    assert!(
        max_right > half_width,
        "in Months mode the zoom body should distribute past the centre — \
         max-right child x = {max_right}, calendar mid x = {half_width}, \
         cal_bounds = {cal_bounds:?}"
    );
}

#[test]
fn calendar_title_button_clickable_across_centered_band() {
    // Verify the click target spans the full width between the
    // chevron buttons — the bug fix's whole point. Clicks at three
    // positions across the centred band (just-right-of-prev-chevrons,
    // dead-center, and just-left-of-next-chevrons) all flip mode.
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = Calendar::single(date);
    let mode = cal.mode_signal();
    let mut tree = light_tree();
    let id = tree.add(cal);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    let header_y = bounds.y + 20.0;

    for pct in [0.30_f32, 0.50, 0.70] {
        // Reset mode each iteration (tests interact independently).
        mode.set(CalendarMode::Days);
        let click_pos = Point::new(bounds.x + bounds.width * pct, header_y);
        tree.dispatch_event(WidgetEvent::pointer_down(
            click_pos,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.dispatch_event(WidgetEvent::pointer_up(
            click_pos,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert_eq!(
            mode.get(),
            CalendarMode::Months,
            "click at {pct:.0}% of header width should flip mode; \
             title button bounds don't span the centred band."
        );
    }
}

/// The label of the first weekday column header — the observable form of
/// the calendar's first-day-of-week.
fn first_weekday_header(tree: &mut WidgetTree, root: WidgetId) -> Option<String> {
    fn collect(tree: &WidgetTree, id: WidgetId, out: &mut Vec<WidgetId>) {
        out.push(id);
        for c in tree.children(id) {
            collect(tree, c, out);
        }
    }
    let mut ids = Vec::new();
    collect(tree, root, &mut ids);

    let update = tree.sync_accessibility();
    ids.iter().find_map(|id| {
        let target = teksilo_core::accessibility::widget_id_to_node_id(*id);
        update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == target)
            .filter(|(_, n)| n.role() == teksilo_core::accesskit::Role::ColumnHeader)
            .and_then(|(_, n)| n.label().map(|s| s.to_string()))
    })
}

#[test]
fn calendar_re_derives_its_first_day_of_week_when_the_locale_switches() {
    // Regression, same class as `DateEdit`: the first day of week is read
    // from the locale in `build()`, and `set_locale` only marks layout +
    // paint. en-US starts the week on Sunday, fr-FR on Monday.
    let mut tree = light_tree();
    tree.set_locale("en-US".to_string());
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(Calendar::single(date));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(400.0),
    });
    let en = first_weekday_header(&mut tree, id).expect("weekday header");
    assert!(
        en.contains("sunday"),
        "en-US should start the week on Sunday; got `{en}`"
    );

    tree.set_locale("fr-FR".to_string());
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(400.0),
    });
    let fr = first_weekday_header(&mut tree, id).expect("weekday header");
    assert!(
        fr.contains("monday"),
        "fr-FR should start the week on Monday after the switch; got `{fr}`"
    );
}

#[test]
fn zoom_cell_label_is_hidden_from_accessibility_tree() {
    // `ZoomCell` (Role::GridCell, used by both MonthsGrid and YearsGrid)
    // already carries the cell's text as its own name, so the embedded
    // `TextWidget` inside it must not reach the AT tree as a second,
    // duplicate-named stop.
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = Calendar::single(date);
    cal.mode_signal().set(CalendarMode::Months);
    let mut tree = light_tree();
    let _id = tree.add(cal);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    // Second pass settles visibility / activation flips (same as
    // `calendar_months_body_does_not_collapse_to_left_edge`).
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let update = tree.sync_accessibility();

    // Without a registered i18n manager, resolution falls back to the
    // literal Fluent key — computed the same way `MonthsGrid::build`
    // computes the cell's own label, so this matches regardless of
    // whether a manager is installed.
    let expected =
        teksilo_i18n::resolve_message_widget(crate::common::datetime::month_long_key(5), &[]);

    let cell_node = update
        .nodes
        .iter()
        .find(|(_, node)| {
            node.role() == teksilo_core::accesskit::Role::GridCell
                && node.label() == Some(expected.as_str())
        })
        .map(|(_, node)| node)
        .expect("May zoom cell present in AT update");
    assert_eq!(
        cell_node.label(),
        Some(expected.as_str()),
        "hiding the embedded label must not take the cell's own name away with it",
    );

    assert!(
        !update.nodes.iter().any(
            |(_, node)| node.role() == teksilo_core::accesskit::Role::Label
                && node.label() == Some(expected.as_str())
        ),
        "the zoom cell's embedded label must not survive as a duplicate-named node",
    );
}

// ---------------------------------------------------------------------------
// Touch
// ---------------------------------------------------------------------------

/// A day cell is a tap target, not a manipulator: its activation is `on_tap`,
/// so it already lands on the release, and its 32 dp box already clears the
/// 24 dp floor and follows the density ladder above it. A finger tap selects a
/// day, and that is the whole of what the controls sweep owes it.
#[test]
fn a_touch_tap_selects_a_day_on_release() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_core::pointer::PointerPhase;

    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(Calendar::single(date.clone()));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let cell = day_cell(&tree, id, 17).expect("the grid has day cells");
    assert!(
        cell.width >= 24.0 && cell.height >= 24.0,
        "a Compact day cell measured {cell:?}",
    );

    let before = date.get();
    let at = cell.center();
    let contact = finger();
    tree.dispatch_pointer(touch(contact, PointerPhase::Down, at, 0));
    assert_eq!(date.get(), before, "the press selects nothing");
    tree.dispatch_pointer(touch(contact, PointerPhase::Up, at, 30));
    assert_ne!(date.get(), before, "the release selects the day under it");
}

/// And the calendar deliberately does **not** declare `touch_action(NONE)`: a
/// finger that comes to rest on a day cell and then drags is scrolling the
/// surface the calendar sits in, because a day cell produces no value from the
/// press position and has no drag of its own to protect.
#[test]
fn a_finger_pan_over_the_calendar_scrolls_its_container() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_canvas::Point;
    use teksilo_core::event::EventResponse;
    use teksilo_core::pointer::PointerPhase;
    use teksilo_core::pointer::touch_action::{PanAxes, PanClaim};
    use teksilo_core::widget_builder::WidgetBuilder;

    let scrolled = std::rc::Rc::new(std::cell::Cell::new(0_u32));
    let count = scrolled.clone();
    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = tree.add(Calendar::single(date.clone()));
    let _page = tree.add(
        crate::primitives::VStack::new()
            .child(cal)
            .scroll_container(PanAxes::BOTH)
            .pan_claim(PanClaim::vertical())
            .on_scroll(move |_e, _c| {
                count.set(count.get() + 1);
                EventResponse::Handled
            }),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(600.0),
    });
    let cell = day_cell(&tree, cal, 17).expect("the grid has day cells");
    let before = date.get();

    let contact = finger();
    let start = cell.center();
    tree.dispatch_pointer(touch(contact, PointerPhase::Down, start, 0));
    for (i, dy) in [40.0_f32, 90.0, 150.0].into_iter().enumerate() {
        let at = Point::new(start.x, start.y - dy);
        tree.dispatch_pointer(touch(contact, PointerPhase::Move, at, 20 + i as u64 * 20));
    }
    tree.dispatch_pointer(touch(
        contact,
        PointerPhase::Up,
        Point::new(start.x, start.y - 150.0),
        100,
    ));
    assert!(scrolled.get() > 0, "the pan never reached the scroller");
    assert_eq!(date.get(), before, "and it selected nothing on the way");
}

/// The nth `DayCell` in the grid, in layout order.
fn day_cell(
    tree: &WidgetTree,
    calendar: teksilo_core::widget_id::WidgetId,
    n: usize,
) -> Option<teksilo_canvas::Rect> {
    let mut cells: Vec<teksilo_canvas::Rect> = Vec::new();
    let mut stack = vec![calendar];
    while let Some(id) = stack.pop() {
        if tree
            .widget_type_name(id)
            .is_some_and(|t| t.contains("DayCell"))
        {
            cells.push(tree.bounds(id));
        }
        stack.extend(tree.children(id).iter().copied());
    }
    cells.sort_by(|a, b| {
        (a.y, a.x)
            .partial_cmp(&(b.y, b.x))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    cells.into_iter().nth(n)
}
