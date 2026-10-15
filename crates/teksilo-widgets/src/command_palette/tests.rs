// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tests for the palette: the matcher's ranking and highlighting, and the
//! widget's own behaviour against a real `ShortcutRegistry`.

use std::cell::RefCell;

use super::matcher::{fuzzy_match, fuzzy_score, highlighted_markup};
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
        category_text: Some("File".into()),
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

/// A localized category is matched as the reader reads it, not by its key: a French
/// interface shows "Fichier", and "fichier" is what gets typed.
#[test]
fn the_category_is_matched_by_its_label_not_its_key() {
    let cmd = PaletteCommand {
        id: "work.new",
        name: "Nouveau projet".into(),
        category: Some("file"),
        category_text: Some("Fichier".into()),
        description: None,
        keystroke: None,
        enabled: true,
        intent: "work.new",
    };
    assert_eq!(cmd.haystack(), "Fichier Nouveau projet");
    assert!(fuzzy_score("fichier", &cmd.haystack()).is_some());
}

#[test]
fn the_best_alignment_is_highlighted_not_the_first() {
    // Taking the first `e` lands in "File" and scatters the rest; the
    // chosen alignment is the word the reader meant.
    let found = fuzzy_match("exp", "File Export").expect("matches");
    assert_eq!(found.positions, vec![5, 6, 7], "the E-x-p of Export");
    let found = fuzzy_match("nw", "New Window").expect("matches");
    assert_eq!(found.positions, vec![0, 4], "two word starts");
}

#[test]
fn the_alignment_beats_a_greedy_one_on_score_too() {
    // The same letters, found greedily (the `e` of "File", then the `x`
    // and `p` of "Export"), score less than the run the matcher picks.
    let best = fuzzy_score("exp", "File Export").unwrap();
    assert!(
        best > fuzzy_score("exp", "Fe Xylophone Piano").unwrap(),
        "a literal run must outrank the same letters on three word starts"
    );
}

#[test]
fn a_run_outranks_two_word_starts_a_few_letters_apart() {
    let found = fuzzy_match("zo", "View Zoom Out").expect("matches");
    assert_eq!(
        found.positions,
        vec![5, 6],
        "the Zo of Zoom, not the Z and the O of Out"
    );
}

#[test]
fn highlighted_markup_bolds_matched_runs_and_escapes_the_rest() {
    assert_eq!(
        highlighted_markup("New Window", &[0, 4]),
        "**N**ew **W**indow"
    );
    assert_eq!(highlighted_markup("Export", &[0, 1, 2]), "**Exp**ort");
    assert_eq!(
        highlighted_markup("Open [recent] (*)", &[]),
        "Open \\[recent\\] \\(\\*\\)"
    );
    assert_eq!(
        highlighted_markup("a*b", &[0, 1, 2]),
        "**a**\\***b**",
        "a matched `*` stays outside the bold run, escaped"
    );
}

// ── The widget ──────────────────────────────────────────────────────────────

/// A tree with three commands registered: two bound, one deliberately chord-less to
/// prove an unbound command is still reachable.
fn tree_with_commands() -> WidgetTree {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

/// A tree with `n` uncategorised commands, `Command 0` to `Command n-1`.
fn tree_with_many(n: usize) -> WidgetTree {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    for i in 0..n {
        let id: &'static str = Box::leak(format!("many.{i:02}").into_boxed_str());
        tree.shortcut_registry_mut()
            .register(Shortcut::new(id).name(format!("Command {i}")).build());
    }
    tree
}

const AREA: SizeProposal = SizeProposal {
    width: Some(560.0),
    height: Some(420.0),
};

/// One frame, as the app runs one.
fn frame(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.layout(AREA);
}

/// The listed command ids, in display order.
fn listed(state: &PaletteState) -> Vec<&'static str> {
    state.rows.borrow().iter().map(|e| e.command.id).collect()
}

/// A palette in a tree, laid out, with its search field focused the way a
/// presented palette's is.
fn mounted(mut tree: WidgetTree, palette: CommandPalette) -> (WidgetTree, PaletteState, WidgetId) {
    let state = palette.state.clone();
    let root = tree.add(palette);
    frame(&mut tree);
    let field = tree
        .first_focusable_descendant(root)
        .expect("the palette's search field must be focusable");
    tree.focus_with_origin(field, teksilo_core::FocusOrigin::Keyboard);
    frame(&mut tree);
    (tree, state, root)
}

fn type_keys(tree: &mut WidgetTree, root: WidgetId, text: &str) {
    for ch in text.chars() {
        tree.type_text(root, &ch.to_string());
        frame(tree);
    }
}

fn press(tree: &mut WidgetTree, key: Key) {
    tree.press_key(key, Default::default());
    frame(tree);
}

/// The realized rows, top to bottom.
fn options(tree: &WidgetTree, root: WidgetId) -> Vec<WidgetId> {
    fn walk(tree: &WidgetTree, id: WidgetId, out: &mut Vec<WidgetId>) {
        if tree.accessibility_node(id).role() == Role::ListBoxOption {
            out.push(id);
        }
        for child in tree.children(id) {
            walk(tree, child, out);
        }
    }
    let mut out = Vec::new();
    walk(tree, root, &mut out);
    out.sort_by(|a, b| tree.bounds(*a).y.total_cmp(&tree.bounds(*b).y));
    out
}

#[test]
fn a_command_with_no_keystroke_is_still_listed() {
    // The whole point of sourcing the palette from the registry: registering a name
    // with no chord is how an app publishes a command to the palette.
    let (_tree, state, _) = mounted(tree_with_commands(), CommandPalette::new());
    let listed = listed(&state);
    assert!(
        listed.contains(&"file.export"),
        "an unbound command must appear; got {listed:?}"
    );
    assert_eq!(listed.len(), 3, "every registered command should be listed");
}

#[test]
fn typing_filters_and_ranks_the_rows() {
    let (mut tree, state, root) = mounted(tree_with_commands(), CommandPalette::new());
    type_keys(&mut tree, root, "exp");
    assert_eq!(
        listed(&state),
        vec!["file.export"],
        "only the matching command should survive the query"
    );
}

#[test]
fn typing_rebuilds_the_list_and_never_the_search_field() {
    // The palette used to rebuild whole on every keystroke, field included,
    // so each key destroyed the field the user was typing in.
    let (mut tree, _state, root) = mounted(tree_with_commands(), CommandPalette::new());
    let field = tree.focused().expect("the field holds focus");
    type_keys(&mut tree, root, "ex");
    press(&mut tree, Key::ArrowDown);
    press(&mut tree, Key::ArrowUp);
    assert_eq!(
        tree.focused(),
        Some(field),
        "the same field, not a rebuilt one, must still hold focus"
    );
}

#[test]
fn typing_in_the_middle_of_the_query_keeps_the_caret_there() {
    // With the field rebuilt per key, its caret was put back at the end each
    // time, so a correction typed mid-query landed at the end.
    let (mut tree, state, root) = mounted(tree_with_commands(), CommandPalette::new());
    type_keys(&mut tree, root, "zom");
    press(&mut tree, Key::ArrowLeft);
    press(&mut tree, Key::ArrowLeft);
    type_keys(&mut tree, root, "ox");
    assert_eq!(state.query.get(), "zoxom", "inserted where the caret was");
}

#[test]
fn a_query_matching_nothing_settles() {
    // `selected >= rows.len()` held for 0 >= 0, so every build of an empty
    // result set wrote the highlight and scheduled the next build: the
    // palette rebuilt every frame for as long as nothing matched.
    let (mut tree, state, root) = mounted(tree_with_commands(), CommandPalette::new());
    type_keys(&mut tree, root, "zzz");
    assert!(
        state.rows.borrow().is_empty(),
        "precondition: nothing matches"
    );
    let count = tree.widget_count();
    let field = tree.focused();
    for _ in 0..5 {
        frame(&mut tree);
    }
    assert_eq!(tree.widget_count(), count, "idle frames must build nothing");
    assert_eq!(tree.focused(), field, "and leave the field alone");
}

#[test]
fn a_changed_query_sends_the_highlight_back_to_the_best_match() {
    // Without this, typing a letter that shortens the list leaves the highlight on
    // whatever row inherited the old index, so Enter runs a command nobody chose.
    let (mut tree, state, root) = mounted(tree_with_commands(), CommandPalette::new());
    press(&mut tree, Key::ArrowDown);
    press(&mut tree, Key::ArrowDown);
    assert_eq!(state.highlight.get(), 2, "precondition: highlight moved");
    type_keys(&mut tree, root, "z");
    assert_eq!(
        state.highlight.get(),
        0,
        "a new query must reset the highlight to the top match"
    );
}

#[test]
fn a_registry_change_keeps_the_highlight_in_range() {
    let (mut tree, state, _) = mounted(tree_with_commands(), CommandPalette::new());
    press(&mut tree, Key::ArrowDown);
    press(&mut tree, Key::ArrowDown);
    assert_eq!(state.highlight.get(), 2);
    tree.shortcut_registry_mut().unregister("view.zoom");
    frame(&mut tree);
    assert_eq!(state.rows.borrow().len(), 2);
    assert_eq!(
        state.highlight.get(),
        1,
        "the highlight must not point past the end of the list"
    );
}

#[test]
fn arrow_keys_wrap_and_page_keys_stop_at_the_ends() {
    let (mut tree, state, _) = mounted(tree_with_commands(), CommandPalette::new());
    press(&mut tree, Key::ArrowUp);
    assert_eq!(
        state.highlight.get(),
        2,
        "up from the first row wraps to the last"
    );
    press(&mut tree, Key::ArrowDown);
    assert_eq!(
        state.highlight.get(),
        0,
        "down from the last row wraps to the first"
    );
    press(&mut tree, Key::PageDown);
    assert_eq!(
        state.highlight.get(),
        2,
        "a page down stops at the last row"
    );
    press(&mut tree, Key::PageUp);
    assert_eq!(state.highlight.get(), 0, "a page up stops at the first");
}

#[test]
fn home_and_end_move_the_caret_not_the_highlight() {
    let (mut tree, state, root) = mounted(tree_with_commands(), CommandPalette::new());
    type_keys(&mut tree, root, "e");
    press(&mut tree, Key::Home);
    type_keys(&mut tree, root, "z");
    assert_eq!(
        state.query.get(),
        "ze",
        "Home put the caret before the query"
    );
    assert_eq!(state.highlight.get(), 0);
}

#[test]
fn stepping_an_empty_list_is_a_no_op() {
    let (mut tree, state, root) = mounted(tree_with_commands(), CommandPalette::new());
    type_keys(&mut tree, root, "qqq");
    assert!(
        state.rows.borrow().is_empty(),
        "precondition: nothing matches"
    );
    press(&mut tree, Key::ArrowDown);
    press(&mut tree, Key::PageDown);
    assert_eq!(
        state.highlight.get(),
        0,
        "an empty list must not move or panic"
    );
}

#[test]
fn the_highlight_is_scrolled_into_view() {
    // The old palette guessed the number of visible rows (7) and kept its own
    // scroll offset, which the density, the text scale and the mouse wheel
    // all proved wrong.
    let (mut tree, state, root) = mounted(tree_with_many(40), CommandPalette::new());
    for _ in 0..25 {
        press(&mut tree, Key::ArrowDown);
    }
    assert_eq!(state.highlight.get(), 25);
    let row = state
        .active_row
        .get()
        .expect("the highlighted row must be realized");
    let list = state.listbox_id.get().expect("the list is up");
    let (row, list) = (tree.bounds(row), tree.bounds(list));
    assert!(
        row.y >= list.y - 0.5 && row.bottom() <= list.bottom() + 0.5,
        "row {row:?} must be inside the list {list:?}"
    );
    let _ = root;
}

#[test]
fn the_palette_hugs_a_short_list() {
    // It used to be pinned at 560 x 420 whatever matched: three rows left two
    // thirds of the panel empty.
    let mut tree = tree_with_commands();
    let palette = tree.add(CommandPalette::new());
    let column = tree.add(crate::primitives::VStack::new().child(palette));
    let _ = column;
    tree.layout(AREA);
    let three = tree.bounds(palette).height;
    assert!(
        three < 220.0,
        "three rows want a short palette, got {three}"
    );

    let mut tree = tree_with_many(40);
    let palette = tree.add(CommandPalette::new());
    tree.add(crate::primitives::VStack::new().child(palette));
    tree.layout(AREA);
    let many = tree.bounds(palette).height;
    assert!(
        many > three && many < 420.0,
        "forty rows stop at the visible maximum and scroll: {many}"
    );
}

#[test]
fn rows_grow_with_the_text_scale() {
    let (mut tree, _, root) = mounted(tree_with_commands(), CommandPalette::new());
    let before = tree.bounds(options(&tree, root)[0]).height;
    tree.set_user_text_scale(2.0);
    frame(&mut tree);
    frame(&mut tree);
    let after = tree.bounds(options(&tree, root)[0]).height;
    assert!(
        after > before * 1.5,
        "a row must grow with its text: {before} -> {after}"
    );
}

#[test]
fn the_include_predicate_removes_a_command_entirely() {
    // The palette's own opening command must be able to hide itself, which is the
    // predicate's first real use.
    let (_tree, state, _) = mounted(
        tree_with_commands(),
        CommandPalette::new().include(|cmd| cmd.id != "view.zoom"),
    );
    let listed = listed(&state);
    assert!(
        !listed.contains(&"view.zoom"),
        "an excluded command must not be listed; got {listed:?}"
    );
    assert_eq!(listed.len(), 2);
}

#[test]
fn a_disabled_command_is_hidden_unless_asked_for() {
    let with_disabled_save = || {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.shortcut_registry_mut().register(
            Shortcut::new("file.save")
                .name("Save")
                .category("File")
                .enabled_when(Signal::new(false))
                .build(),
        );
        tree
    };
    let (_tree, state, _) = mounted(with_disabled_save(), CommandPalette::new());
    assert!(
        state.rows.borrow().is_empty(),
        "a command that cannot run now is not an answer to \"what can I do\""
    );

    let (_tree, state, _) = mounted(
        with_disabled_save(),
        CommandPalette::new().show_disabled(true),
    );
    assert_eq!(
        state.rows.borrow().len(),
        1,
        "show_disabled must bring it back"
    );
    assert!(
        !state.rows.borrow()[0].command.enabled,
        "and mark it disabled"
    );
}

#[test]
fn the_intent_falls_back_to_the_id_when_none_was_declared() {
    // Activation sends `intent`, so this is what decides which action runs.
    let (_tree, state, _) = mounted(tree_with_commands(), CommandPalette::new());
    let rows = state.rows.borrow();
    let export = rows.iter().find(|e| e.command.id == "file.export").unwrap();
    assert_eq!(
        export.command.intent, "file.export",
        "a command declaring no explicit intent must send its own id, exactly as the \
         keystroke dispatcher does"
    );
}

#[test]
fn the_category_and_the_name_share_one_line() {
    let (tree, state, root) = mounted(tree_with_commands(), CommandPalette::new());
    let rows = state.rows.borrow();
    let new = rows.iter().find(|e| e.command.id == "file.new").unwrap();
    assert_eq!(new.accessible_name, "File: New Work");
    assert!(new.chord.is_some(), "a bound command shows its chord");
    let row = tree.bounds(options(&tree, root)[0]);
    assert!(row.height < 40.0, "one line, not two: {row:?}");
}

#[test]
fn matched_letters_are_split_between_category_and_name() {
    let (mut tree, state, root) = mounted(tree_with_commands(), CommandPalette::new());
    type_keys(&mut tree, root, "fnw");
    let rows = state.rows.borrow();
    let new = rows.iter().find(|e| e.command.id == "file.new").unwrap();
    assert_eq!(new.category_positions, vec![0], "the F of File");
    assert_eq!(
        new.name_positions,
        vec![0, 4],
        "the N of New and the W of Work"
    );
}

/// Wraps a palette and answers every command it can run, recording which.
#[derive(Debug)]
struct Host {
    palette: Option<CommandPalette>,
    ran: Rc<RefCell<Vec<&'static str>>>,
    child: Option<WidgetId>,
}

impl Widget for Host {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        for id in ["file.new", "file.export", "view.zoom"] {
            let ran = self.ran.clone();
            ctx.register_action(teksilo_core::Action::new(id).on_invoke(move |intent, _| {
                ran.borrow_mut().push(intent.name);
            }));
        }
        let palette = self.palette.take().expect("built once");
        self.child = Some(ctx.add(palette));
        self.child.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.child
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_default()
            .into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }
}

/// A hosted palette whose `on_dismiss` counts.
fn hosted() -> (
    WidgetTree,
    PaletteState,
    WidgetId,
    Rc<RefCell<Vec<&'static str>>>,
    Rc<Cell<u32>>,
) {
    let dismissed = Rc::new(Cell::new(0));
    let counter = dismissed.clone();
    let palette = CommandPalette::new().on_dismiss(move |_| counter.set(counter.get() + 1));
    let state = palette.state.clone();
    let ran = Rc::new(RefCell::new(Vec::new()));
    let host = Host {
        palette: Some(palette),
        ran: ran.clone(),
        child: None,
    };
    let mut tree = tree_with_commands();
    let root = tree.add(host);
    frame(&mut tree);
    let field = tree.first_focusable_descendant(root).unwrap();
    tree.focus_with_origin(field, teksilo_core::FocusOrigin::Keyboard);
    frame(&mut tree);
    (tree, state, root, ran, dismissed)
}

#[test]
fn enter_runs_the_highlighted_command() {
    let (mut tree, _state, _root, ran, dismissed) = hosted();
    press(&mut tree, Key::ArrowDown);
    press(&mut tree, Key::Enter);
    assert_eq!(
        *ran.borrow(),
        vec!["file.new"],
        "the second row in (category, id) order"
    );
    assert_eq!(dismissed.get(), 1);
}

#[test]
fn clicking_a_row_runs_its_command() {
    // A click used to select the row for assistive technology only: the tint
    // stayed where it was, focus left the field, and nothing ran.
    let (mut tree, _state, root, ran, dismissed) = hosted();
    let rows = options(&tree, root);
    tree.click(rows[2]);
    frame(&mut tree);
    assert_eq!(*ran.borrow(), vec!["view.zoom"]);
    assert_eq!(dismissed.get(), 1);
}

#[test]
fn hovering_a_row_does_not_move_the_highlight() {
    let (mut tree, state, root, _, _) = hosted();
    let rows = options(&tree, root);
    tree.pointer_move(tree.bounds(rows[1]).center());
    frame(&mut tree);
    assert_eq!(
        state.highlight.get(),
        0,
        "the keyboard's place is the keyboard's"
    );
}

#[test]
fn an_embedded_palette_leaves_out_the_commands_of_the_focused_editor() {
    // Embedded, the palette keeps the focus, so an edit command would act on
    // its own search field.
    let (_tree, state, _) = mounted(tree_with_commands(), CommandPalette::new());
    assert!(!listed(&state).iter().any(|id| id.starts_with("edit.")));
}

#[test]
fn a_presented_palette_lists_the_commands_of_the_focused_editor() {
    // Presented, it hands focus back before the command runs, so cut, copy
    // and the rest reach the editor it was opened over.
    let palette = CommandPalette::new();
    palette.state.presented.set(true);
    let (_tree, state, _) = mounted(tree_with_commands(), palette);
    assert!(listed(&state).contains(&"edit.copy"));
}

#[test]
fn typing_a_query_keeps_every_character() {
    // A rebuild restoring focus used to look like a fresh keyboard focus, so
    // the field selected all of its text and the next key replaced it.
    let (mut tree, state, root) = mounted(tree_with_commands(), CommandPalette::new());
    type_keys(&mut tree, root, "exp");
    assert_eq!(state.query.get(), "exp");
    let focused = tree.focused().expect("focus must stay in the field");
    assert!(
        tree.is_descendant_of(focused, root),
        "focus must stay in the palette"
    );
}

// ── Accessibility ───────────────────────────────────────────────────────────
//
/// The published AccessKit nodes, keyed by id. `AccessibilityInfo` is a coarse
/// test view carrying neither `description` nor `active_descendant`, so these
/// assertions go to the real tree update.
fn a11y_nodes(
    t: &mut WidgetTree,
) -> std::collections::HashMap<teksilo_core::accesskit::NodeId, teksilo_core::accesskit::Node> {
    t.sync_accessibility().nodes.into_iter().collect()
}

fn node_of(
    nodes: &std::collections::HashMap<
        teksilo_core::accesskit::NodeId,
        teksilo_core::accesskit::Node,
    >,
    id: WidgetId,
) -> teksilo_core::accesskit::Node {
    nodes
        .get(&teksilo_core::accessibility::widget_id_to_node_id(id))
        .cloned()
        .unwrap_or_else(|| panic!("{id:?} published no AccessKit node"))
}

#[test]
fn the_dialog_is_named_and_reports_its_match_count() {
    // No `I18nManager` is installed by default in widget tests, so `tr_widget!`
    // resolves to the bare message key. Install one carrying the crate's real
    // framework locales, so the assertions read the sentence a user is actually
    // announced, including the plural selector, which is the point.
    use teksilo_i18n::{
        I18nConfig, I18nManager,
        thread_local::{clear, install},
    };
    clear();
    install(I18nManager::from_config(
        &I18nConfig::new().framework_locales(crate::framework_locales()),
    ));

    let (mut tree, _state, root) = mounted(tree_with_commands(), CommandPalette::new());
    let info = tree.accessibility_node(root);
    assert_eq!(info.role(), Role::Dialog);
    assert!(
        info.name().is_some_and(|n| !n.is_empty()),
        "an unnamed dialog announces that *something* opened, not what"
    );

    let nodes = a11y_nodes(&mut tree);
    assert_eq!(
        node_of(&nodes, root).description(),
        Some("3 commands"),
        "the match count must be announced, not left to be arrowed through"
    );

    type_keys(&mut tree, root, "exp");
    let nodes = a11y_nodes(&mut tree);
    assert_eq!(
        node_of(&nodes, root).description(),
        Some("1 command"),
        "the count must track the query"
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn the_highlighted_row_is_reported_as_selected() {
    let (mut tree, _state, root) = mounted(tree_with_commands(), CommandPalette::new());
    let rows = options(&tree, root);
    let nodes = a11y_nodes(&mut tree);
    assert_eq!(node_of(&nodes, rows[0]).is_selected(), Some(true));
    assert_eq!(node_of(&nodes, rows[1]).is_selected(), Some(false));

    press(&mut tree, Key::ArrowDown);
    let nodes = a11y_nodes(&mut tree);
    assert_eq!(
        node_of(&nodes, rows[1]).is_selected(),
        Some(true),
        "an arrow key must move the AT-visible selection, not only the tint"
    );
    assert_eq!(
        node_of(&nodes, rows[0]).is_selected(),
        Some(false),
        "the previous row must stop reporting itself as selected"
    );
}

#[test]
fn a_row_is_named_by_its_line_and_announces_its_shortcut() {
    let (mut tree, _state, root) = mounted(tree_with_commands(), CommandPalette::new());
    let rows = options(&tree, root);
    let nodes = a11y_nodes(&mut tree);
    let new = rows
        .iter()
        .map(|id| node_of(&nodes, *id))
        .find(|n| n.label() == Some("File: New Work"))
        .expect("the row is named by its whole line");
    assert!(new.keyboard_shortcut().is_some(), "and carries its chord");
}

#[test]
fn the_search_field_publishes_the_highlighted_row_as_its_active_descendant() {
    // The ARIA combobox pattern: focus stays in the search field, so the
    // highlight has to reach AT through *that* node's active descendant.
    let (mut tree, state, _root) = mounted(tree_with_commands(), CommandPalette::new());
    let first = state
        .active_row
        .get()
        .expect("the highlighted row must resolve to a live node");
    assert!(
        state.listbox_id.get().is_some(),
        "the result list must be published for the field's `controls` relation"
    );

    press(&mut tree, Key::ArrowDown);
    let second = state
        .active_row
        .get()
        .expect("the highlight must still resolve after moving");
    assert_ne!(
        first, second,
        "moving the highlight must move the announced active descendant"
    );

    // The relation must land on the node that actually holds keyboard focus:
    // an ancestor's active descendant is not what a screen reader follows.
    let want = teksilo_core::accessibility::widget_id_to_node_id(second);
    let nodes = a11y_nodes(&mut tree);
    let publisher = nodes
        .values()
        .find(|n| n.active_descendant() == Some(want))
        .expect("the highlighted row must be published as an active descendant");
    assert_eq!(
        publisher.role(),
        Role::TextInput,
        "the relation must sit on the focusable text field, not on a composite \
         ancestor: AT follows the focused node's active descendant"
    );
}

#[test]
fn an_empty_result_set_publishes_no_stale_active_descendant() {
    let (mut tree, state, root) = mounted(tree_with_commands(), CommandPalette::new());
    assert!(state.active_row.get().is_some(), "precondition");
    type_keys(&mut tree, root, "zzzzz");
    assert_eq!(
        state.active_row.get(),
        None,
        "with no rows there is no node to point at: a stale id would name a \
         destroyed widget"
    );
    assert_eq!(state.listbox_id.get(), None, "and no listbox either");
}
