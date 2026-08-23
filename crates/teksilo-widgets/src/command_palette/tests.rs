// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tests for the palette: the matcher's ranking, and the widget's own behaviour
//! against a real `ShortcutRegistry`.

use super::*;
use teksilo_core::shortcut::Shortcut;
use teksilo_core::widget_tree::WidgetTree;

// ── The matcher ─────────────────────────────────────────────────────────────

/// Rank `needle` against every candidate, best first, dropping non-matches.
fn ranked<'a>(needle: &str, candidates: &[&'a str]) -> Vec<&'a str> {
    let mut scored: Vec<(i32, &str)> = candidates
        .iter()
        .filter_map(|c| Some((fuzzy_score(needle, c)?, *c)))
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, c)| c).collect()
}

#[test]
fn an_empty_needle_matches_everything_without_reordering() {
    let all = ["File New", "Edit Copy", "View Zoom"];
    assert_eq!(
        ranked("", &all),
        all,
        "an empty query must leave the registry's own order alone"
    );
}

#[test]
fn a_subsequence_matches_where_a_substring_would_not() {
    assert!(
        fuzzy_score("nwd", "New Window").is_some(),
        "letters scattered in order through the name must match"
    );
    assert!(
        fuzzy_score("zqx", "New Window").is_none(),
        "letters that are not present must not match"
    );
}

#[test]
fn letters_must_appear_in_order() {
    assert!(
        fuzzy_score("wn", "Window New").is_some(),
        "`w` then `n` appears in that order"
    );
    assert!(
        fuzzy_score("wn", "New").is_none(),
        "there is no `w` in `New` at all"
    );
}

#[test]
fn a_consecutive_run_outranks_the_same_letters_scattered() {
    assert_eq!(
        ranked("exp", &["Export", "Edit XML Properties"]).first(),
        Some(&"Export"),
        "a literal run must beat scattered initials"
    );
}

#[test]
fn word_starts_outrank_mid_word_matches() {
    assert_eq!(
        ranked("nw", &["New Window", "Unwrap lines"]).first(),
        Some(&"New Window"),
        "two word-initials must beat a mid-word run of the same letters"
    );
}

#[test]
fn an_early_match_outranks_a_late_one() {
    assert_eq!(
        ranked("save", &["Save", "Autosave"]).first(),
        Some(&"Save"),
        "the same run earlier in the string must rank higher"
    );
}

#[test]
fn matching_folds_case_in_both_directions() {
    assert!(fuzzy_score("new", "NEW WINDOW").is_some());
    assert!(fuzzy_score("nw", "new window").is_some());
}

#[test]
fn a_needle_longer_than_the_haystack_cannot_match() {
    assert!(fuzzy_score("abcdefghij", "abc").is_none());
}

#[test]
fn non_ascii_names_match_without_panicking() {
    // French command names are ordinary content here. The matcher must fold their case
    // and must never index past the end of a haystack whose lowercase form has a
    // different character count from the original.
    assert!(fuzzy_score("exp", "Exporter").is_some());
    assert!(fuzzy_score("écr", "Écrire un chapitre").is_some());
    // German ß uppercases to two characters, which is exactly the length-change case.
    let _ = fuzzy_score("s", "GROSSE STRAßE");
}

#[test]
fn the_category_takes_part_in_matching() {
    let cmd = PaletteCommand {
        id: "work.new",
        name: "New Work".into(),
        category: Some("File"),
        description: None,
        keystroke: None,
        enabled: true,
        intent: "work.new",
    };
    assert_eq!(cmd.haystack(), "File New Work");
    assert!(
        fuzzy_score("filenew", &cmd.haystack()).is_some(),
        "a query naming the category then the command must match the composed haystack"
    );
}

// ── The widget ──────────────────────────────────────────────────────────────

/// A tree with three commands registered: two bound, one deliberately chord-less to
/// prove an unbound command is still reachable.
fn tree_with_commands() -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.shortcut_registry_mut().register(
        Shortcut::new("file.new")
            .name("New Work")
            .category("File")
            .primary(teksilo_core::shortcut::KeyStroke::ctrl(
                teksilo_core::event::Key::N,
            ))
            .build(),
    );
    tree.shortcut_registry_mut().register(
        Shortcut::new("file.export")
            .name("Export")
            .category("File")
            .build(),
    );
    tree.shortcut_registry_mut().register(
        Shortcut::new("view.zoom")
            .name("Zoom In")
            .category("View")
            .build(),
    );
    tree
}

#[test]
fn a_command_with_no_keystroke_is_still_listed() {
    // The whole point of sourcing the palette from the registry: registering a name
    // with no chord is how an app publishes a command to the palette.
    let mut tree = tree_with_commands();
    let palette = CommandPalette::new();
    let state = palette.state.clone();
    let _ = tree.add(palette);
    tree.layout(SizeProposal::exact(560.0, 420.0));

    let listed: Vec<&str> = state.rows.borrow().iter().map(|c| c.id).collect();
    assert!(
        listed.contains(&"file.export"),
        "an unbound command must appear; got {listed:?}"
    );
    assert_eq!(listed.len(), 3, "every registered command should be listed");
}

#[test]
fn typing_filters_and_ranks_the_rows() {
    let mut tree = tree_with_commands();
    let palette = CommandPalette::new();
    let state = palette.state.clone();
    let _ = tree.add(palette);
    tree.layout(SizeProposal::exact(560.0, 420.0));

    state.query.set("exp".to_string());
    tree.layout(SizeProposal::exact(560.0, 420.0));

    let listed: Vec<&str> = state.rows.borrow().iter().map(|c| c.id).collect();
    assert_eq!(
        listed,
        vec!["file.export"],
        "only the matching command should survive the query"
    );
}

#[test]
fn a_changed_query_sends_the_highlight_back_to_the_best_match() {
    // Without this, typing a letter that shortens the list leaves the highlight on
    // whatever row inherited the old index — so Enter runs a command nobody chose.
    let mut tree = tree_with_commands();
    let palette = CommandPalette::new();
    let state = palette.state.clone();
    let _ = tree.add(palette);
    tree.layout(SizeProposal::exact(560.0, 420.0));

    state.step_selection(2);
    assert_eq!(state.selected.get(), 2, "precondition: highlight moved");

    state.query.set("zoom".to_string());
    tree.layout(SizeProposal::exact(560.0, 420.0));
    assert_eq!(
        state.selected.get(),
        0,
        "a new query must reset the highlight to the top match"
    );
}

#[test]
fn the_highlight_never_points_past_the_end_of_the_list() {
    let mut tree = tree_with_commands();
    let palette = CommandPalette::new();
    let state = palette.state.clone();
    let _ = tree.add(palette);
    tree.layout(SizeProposal::exact(560.0, 420.0));

    state.step_selection(99);
    assert_eq!(
        state.selected.get(),
        2,
        "stepping past the end must clamp to the last row"
    );
    state.step_selection(-99);
    assert_eq!(state.selected.get(), 0, "stepping before the start clamps");
}

#[test]
fn stepping_an_empty_list_is_a_no_op() {
    let mut tree = tree_with_commands();
    let palette = CommandPalette::new();
    let state = palette.state.clone();
    let _ = tree.add(palette);
    tree.layout(SizeProposal::exact(560.0, 420.0));

    state.query.set("nothingmatchesthis".to_string());
    tree.layout(SizeProposal::exact(560.0, 420.0));
    assert!(
        state.rows.borrow().is_empty(),
        "precondition: nothing matches"
    );

    state.step_selection(1);
    assert_eq!(
        state.selected.get(),
        0,
        "an empty list must not move or panic"
    );
}

#[test]
fn the_include_predicate_removes_a_command_entirely() {
    // The palette's own opening command must be able to hide itself, which is the
    // predicate's first real use.
    let mut tree = tree_with_commands();
    let palette = CommandPalette::new().include(|cmd| cmd.id != "view.zoom");
    let state = palette.state.clone();
    let _ = tree.add(palette);
    tree.layout(SizeProposal::exact(560.0, 420.0));

    let listed: Vec<&str> = state.rows.borrow().iter().map(|c| c.id).collect();
    assert!(
        !listed.contains(&"view.zoom"),
        "an excluded command must not be listed; got {listed:?}"
    );
    assert_eq!(listed.len(), 2);
}

#[test]
fn a_disabled_command_is_hidden_unless_asked_for() {
    let mut tree = WidgetTree::new();
    tree.shortcut_registry_mut().register(
        Shortcut::new("file.save")
            .name("Save")
            .category("File")
            .enabled_when(Signal::new(false))
            .build(),
    );

    let palette = CommandPalette::new();
    let state = palette.state.clone();
    let _ = tree.add(palette);
    tree.layout(SizeProposal::exact(560.0, 420.0));
    assert!(
        state.rows.borrow().is_empty(),
        "a command that cannot run now is not an answer to \"what can I do\""
    );

    let mut tree = WidgetTree::new();
    tree.shortcut_registry_mut().register(
        Shortcut::new("file.save")
            .name("Save")
            .category("File")
            .enabled_when(Signal::new(false))
            .build(),
    );
    let palette = CommandPalette::new().show_disabled(true);
    let state = palette.state.clone();
    let _ = tree.add(palette);
    tree.layout(SizeProposal::exact(560.0, 420.0));
    assert_eq!(
        state.rows.borrow().len(),
        1,
        "show_disabled must bring it back"
    );
    assert!(!state.rows.borrow()[0].enabled, "and mark it disabled");
}

#[test]
fn the_intent_falls_back_to_the_id_when_none_was_declared() {
    // Activation sends `intent`, so this is what decides which action runs.
    let mut tree = tree_with_commands();
    let palette = CommandPalette::new();
    let state = palette.state.clone();
    let _ = tree.add(palette);
    tree.layout(SizeProposal::exact(560.0, 420.0));

    let rows = state.rows.borrow();
    let export = rows.iter().find(|c| c.id == "file.export").unwrap();
    assert_eq!(
        export.intent, "file.export",
        "a command declaring no explicit intent must send its own id, exactly as the \
         keystroke dispatcher does"
    );
}

#[test]
fn revealing_scrolls_only_far_enough_to_show_the_row() {
    let state = PaletteState::new();
    *state.rows.borrow_mut() = (0..40)
        .map(|i| PaletteCommand {
            id: "x",
            name: format!("Command {i}"),
            category: None,
            description: None,
            keystroke: None,
            enabled: true,
            intent: "x",
        })
        .collect();

    // Walking down past the fold scrolls by exactly one row at a time.
    for _ in 0..VISIBLE_ROWS {
        state.step_selection(1);
    }
    assert_eq!(
        state.top_index.get(),
        1,
        "reaching one row past the fold must scroll one row, not jump"
    );

    // Walking back up to the top scrolls back to the top.
    for _ in 0..VISIBLE_ROWS {
        state.step_selection(-1);
    }
    assert_eq!(state.top_index.get(), 0, "returning to row 0 shows row 0");
}
