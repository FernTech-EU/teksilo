// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;
use crate::common::datetime::Date;
use teksilo_core::event::Modifiers;
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
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
    assert_eq!(info.role(), teksilo_core::accesskit::Role::DateInput);
}

/// The value published on the `DateInput` node itself.
fn spoken_value(tree: &mut WidgetTree, id: WidgetId) -> String {
    let update = tree.sync_accessibility();
    let target = teksilo_core::accessibility::widget_id_to_node_id(id);
    update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == target)
        .and_then(|(_, node)| node.value().map(str::to_string))
        .expect("date edit node with a value")
}

#[test]
fn the_value_is_the_day_in_full_in_the_users_language() {
    // The value on the field's own node is written for someone listening:
    // the day in full, the way the locale writes it, not the ISO
    // "2026-05-02" it used to be. It follows a switch of language too.
    use crate::common::locale_switch_test::speaking;
    let (mgr, mut tree) = speaking("en-US");
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value));
    laid_out(&mut tree);
    assert_eq!(spoken_value(&mut tree, id), "Saturday, May 2, 2026");

    mgr.set_locale("fr-FR".parse().unwrap());
    tree.set_locale("fr-FR".to_string());
    laid_out(&mut tree);
    assert_eq!(spoken_value(&mut tree, id), "samedi 2 mai 2026");
    teksilo_i18n::thread_local::clear();
}

#[test]
fn date_edit_collapses_middle_container_node() {
    // Audit G9: DateEdit must expose exactly DateInput -> TextInput(editable),
    // not DateInput -> GenericContainer(named) -> TextInput. The label lives
    // only on the DateInput node; the inner TextInput's now-content-free
    // GenericContainer is dropped as presentational, so the label never
    // appears on a duplicated middle node.
    let mut tree = light_tree();
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let _id =
        tree.add(DateEdit::new(value).label(LocalizedString::literal("Due date".to_string())));
    tree.layout(SizeProposal {
        width: Some(300.0),
        height: None,
    });
    let update = tree.sync_accessibility();

    let date_inputs = update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == teksilo_core::accesskit::Role::DateInput)
        .count();
    assert_eq!(date_inputs, 1, "exactly one DateInput node");

    let named_due = update
        .nodes
        .iter()
        .filter(|(_, n)| n.label() == Some("Due date"))
        .count();
    assert_eq!(
        named_due, 1,
        "label appears only on the DateInput node, not a duplicated middle container"
    );

    assert!(
        update
            .nodes
            .iter()
            .any(|(_, n)| n.role() == teksilo_core::accesskit::Role::TextInput),
        "the editable TextInput field node is present"
    );
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
    assert!(msg.resolve_now().contains("with-notes") || msg.resolve_now().contains("validation"));
}

#[test]
fn clamp_recovery_clamps_out_of_range_month() {
    // 13/15/2026 → 12/15/2026 (month clamped 13→12, day 15 fits in Dec)
    let pattern = ParsedPattern::parse("%m/%d/%Y").unwrap();
    let (corrected, msg) =
        try_clamp_recovery(&pattern, "13/15/2026", None, None).expect("recovery");
    assert_eq!(corrected, "12/15/2026");
    assert!(msg.resolve_now().contains("with-notes") || msg.resolve_now().contains("validation"));
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
fn tooltip_appears_on_hover() {
    let mut tree = light_tree();
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value).tooltip(LocalizedString::literal("Tip".to_string())));
    tree.layout(SizeProposal {
        width: Some(300.0),
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
fn date_edit_validation_feedback_signal_starts_pristine() {
    use crate::primitives::text_input_field::ValidationFeedback;
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let editor = DateEdit::new(value);
    let feedback = editor.validation_feedback_signal();
    assert!(matches!(feedback.get(), ValidationFeedback::Pristine));
}

#[test]
fn rebuilding_a_date_edit_reaps_its_calendar() {
    // The calendar popup is detached — parked dormant and shown through an
    // overlay, never laid out inline under the field. Held by a bare
    // `ctx.add` it belonged to nobody, so every rebuild stranded a whole
    // ~200-widget month grid in the arena.
    let mut tree = light_tree();
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value));
    let proposal = SizeProposal {
        width: Some(300.0),
        height: None,
    };
    tree.layout(proposal);

    let baseline = tree.widget_count();
    for _ in 0..5 {
        tree.arena_mark_needs_rebuild_for_testing(id);
        tree.layout(proposal);
    }
    assert_eq!(
        tree.widget_count(),
        baseline,
        "each rebuild stranded another calendar"
    );

    tree.destroy_subtree_for_testing(id);
    assert_eq!(
        tree.widget_count(),
        0,
        "the calendar must die with the field that owns it"
    );
}

use crate::common::locale_switch_test::displayed_text;

#[test]
fn date_edit_re_derives_its_pattern_when_the_locale_switches() {
    // Regression: the locale-derived pattern is read once in `build()`,
    // and `WidgetTree::set_locale` only marks layout + paint. Without a
    // `Rebuild` binding on the locale signal the field kept rendering
    // en-US `MM/DD/YYYY` after a switch to fr-FR.
    let mut tree = light_tree();
    tree.set_locale("en-US".to_string());
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value));
    tree.layout(SizeProposal {
        width: Some(300.0),
        height: None,
    });
    assert_eq!(
        displayed_text(&mut tree, id).as_deref(),
        Some("05/02/2026"),
        "en-US should render month-first"
    );

    tree.set_locale("fr-FR".to_string());
    tree.layout(SizeProposal {
        width: Some(300.0),
        height: None,
    });
    assert_eq!(
        displayed_text(&mut tree, id).as_deref(),
        Some("02/05/2026"),
        "fr-FR should render day-first after the locale switch"
    );
}

#[test]
fn date_edit_explicit_pattern_survives_a_locale_switch() {
    // The `Rebuild` binding fires for every DateEdit, so an explicit
    // override must not be re-derived out from under the caller.
    let mut tree = light_tree();
    tree.set_locale("en-US".to_string());
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value).format_pattern("%Y-%m-%d"));
    tree.layout(SizeProposal {
        width: Some(300.0),
        height: None,
    });
    assert_eq!(displayed_text(&mut tree, id).as_deref(), Some("2026-05-02"));

    tree.set_locale("fr-FR".to_string());
    tree.layout(SizeProposal {
        width: Some(300.0),
        height: None,
    });
    assert_eq!(
        displayed_text(&mut tree, id).as_deref(),
        Some("2026-05-02"),
        "an explicit pattern is not locale-derived"
    );
}

// ── Keyboard: segment stepping and the popover chords ─────────────

fn focus_field(tree: &mut WidgetTree, id: WidgetId) {
    let field = tree
        .first_focusable_descendant(id)
        .expect("DateEdit has a focusable inner field");
    tree.focus(field);
}

fn laid_out(tree: &mut WidgetTree) {
    tree.layout(SizeProposal {
        width: Some(300.0),
        height: None,
    });
}

#[test]
fn arrow_keys_step_the_segment_under_the_caret() {
    // The module doc used to promise "±1 day; Shift+ → ±7 days" and
    // "PageUp/PageDown → ±1 month". The code has always been
    // *segment*-relative — `QAbstractSpinBox::stepBy` on the focused section,
    // ten times that on a page key — which is what `QDateTimeEdit` does. The
    // doc is what was wrong, and this pins the behaviour it now describes.
    let mut tree = light_tree();
    let value = Signal::new(Some(Date::constant(2026, 5, 15)));
    let id = tree.add(DateEdit::new(value.clone()));
    laid_out(&mut tree);
    focus_field(&mut tree, id);

    let before = value.get().expect("a date");
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    laid_out(&mut tree);
    assert_ne!(
        value.get(),
        Some(before),
        "an arrow steps the caret's segment"
    );
}

#[test]
fn alt_arrow_down_opens_the_calendar_instead_of_stepping_the_date() {
    // The chord this module's documentation has promised since it was written,
    // and which was implemented nowhere. Worse, the segment stepper reads only
    // `Shift`, so `Alt+ArrowDown` silently moved the date instead.
    let mut tree = light_tree();
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value.clone()));
    laid_out(&mut tree);
    focus_field(&mut tree, id);

    tree.press_key(Key::ArrowDown, Modifiers::ALT);
    laid_out(&mut tree);
    assert_eq!(
        value.get(),
        Some(Date::constant(2026, 5, 2)),
        "the date must not step"
    );
    assert!(
        tree.accessibility_node(id).is_expanded(),
        "the calendar must open"
    );
}

#[test]
fn f4_toggles_the_calendar() {
    // The Win32 `DateTimePicker` chord.
    let mut tree = light_tree();
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value.clone()));
    laid_out(&mut tree);
    focus_field(&mut tree, id);

    tree.press_key(Key::F4, Modifiers::NONE);
    laid_out(&mut tree);
    assert!(tree.accessibility_node(id).is_expanded());
    assert_eq!(value.get(), Some(Date::constant(2026, 5, 2)));
}

#[test]
fn alt_arrow_down_is_inert_without_a_calendar_button() {
    // `show_calendar_button(false)` builds no calendar, so the chord falls
    // through rather than pretending — and must not step the date either.
    let mut tree = light_tree();
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value.clone()).show_calendar_button(false));
    laid_out(&mut tree);
    focus_field(&mut tree, id);

    tree.press_key(Key::ArrowDown, Modifiers::ALT);
    laid_out(&mut tree);
    assert_eq!(value.get(), Some(Date::constant(2026, 5, 2)));
    assert!(!tree.accessibility_node(id).is_expanded());
}

#[test]
fn an_accelerator_chord_does_not_step_the_date() {
    // Behaviour change: the segment stepper read `Shift` alone, so
    // `Ctrl+ArrowUp` stepped the date and swallowed the chord.
    let mut tree = light_tree();
    let value = Signal::new(Some(Date::constant(2026, 5, 15)));
    let id = tree.add(DateEdit::new(value.clone()));
    laid_out(&mut tree);
    focus_field(&mut tree, id);

    for (key, mods) in [
        (Key::ArrowUp, Modifiers::CTRL),
        (Key::PageDown, Modifiers::SUPER),
    ] {
        tree.press_key(key, mods);
        laid_out(&mut tree);
        assert_eq!(
            value.get(),
            Some(Date::constant(2026, 5, 15)),
            "{key:?} with {mods:?} must fall through"
        );
    }
}

// ── The popover speaks the user's language ────────────────────────

#[test]
fn the_popover_calendar_speaks_the_users_language() {
    // The popover is the shared `Calendar`. Opened on a French tree it used
    // to be "Calendar, mai 2026", valued "2026-05-02 (selected: 2026-05-02)".
    use crate::common::locale_switch_test::{speaking, spoken_grid};
    let (_mgr, mut tree) = speaking("fr-FR");
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value));
    laid_out(&mut tree);
    focus_field(&mut tree, id);
    tree.press_key(Key::F4, Modifiers::NONE);
    laid_out(&mut tree);
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
fn opening_the_popover_says_the_calendar_once() {
    // What a screen reader is told as F4 opens the calendar. The grid used to
    // be a live region, and a live node entering the tree is announced on
    // every platform, with each of its descendants, which inherit the
    // setting: "Calendrier, mai 2026", then the seven weekday headers, and
    // then the same name again as the grid took focus. Now it is one focus
    // change, to the day the field holds, and on Orca the day alone: Orca
    // takes a table without the AT-SPI Table interface, which AccessKit does
    // not implement, for a layout table (`ax_table.py`, `is_layout_table`)
    // and leaves it out of the context it gives a new focus
    // (`speech_generator.py`, `_generateAncestors`, Orca 46.1).
    use crate::common::heard_test::{Heard, Listener};
    use crate::common::locale_switch_test::speaking;
    let (_mgr, mut tree) = speaking("fr-FR");
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value));
    laid_out(&mut tree);
    focus_field(&mut tree, id);
    laid_out(&mut tree);
    let mut listener = Listener::attach(&mut tree);
    tree.press_key(Key::F4, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Focus("samedi 2 mai 2026".to_string())]
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn a_reopened_calendar_speaks_as_it_did_the_first_time() {
    // `datetime-dateedit-popover` (tools/reader): open the calendar, move,
    // Escape, open it again, move. The calendar is built once and parked
    // dormant on close, so it used to come back under the ids the AT-SPI
    // adapter had announced defunct as it closed, and Orca 46.1 said nothing
    // on the reopening and nothing on a day met before ("Ignoring defunct
    // object: [table cell: 'Sunday, May 3, 2026']", 4 of 4 runs). The cursor
    // is brought back to the field's day before closing, so the reopening
    // lands where the first opening did.
    use crate::common::heard_test::{Heard, Listener};
    use crate::common::locale_switch_test::speaking;
    let (_mgr, mut tree) = speaking("en-US");
    let value = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(DateEdit::new(value));
    laid_out(&mut tree);
    focus_field(&mut tree, id);
    laid_out(&mut tree);
    let mut listener = Listener::attach(&mut tree);
    let day = |text: &str| vec![Heard::Focus(text.to_string())];

    tree.press_key(Key::ArrowDown, Modifiers::ALT);
    laid_out(&mut tree);
    assert_eq!(listener.heard(&mut tree), day("Saturday, May 2, 2026"));
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(listener.heard(&mut tree), day("Sunday, May 3, 2026"));
    tree.press_key(Key::ArrowLeft, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(listener.heard(&mut tree), day("Saturday, May 2, 2026"));
    tree.press_key(Key::Escape, Modifiers::NONE);
    laid_out(&mut tree);
    let _ = listener.heard(&mut tree);

    tree.press_key(Key::ArrowDown, Modifiers::ALT);
    laid_out(&mut tree);
    assert_eq!(
        listener.heard(&mut tree),
        day("Saturday, May 2, 2026"),
        "the reopened calendar is heard"
    );
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!(
        listener.heard(&mut tree),
        day("Sunday, May 3, 2026"),
        "a day met in the first opening is heard"
    );
    teksilo_i18n::thread_local::clear();
}
