// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::shortcut::{KeyStroke, Shortcut};
use teksilo_core::signal::Signal;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;

use super::state::{CaptureTarget, FocusKey, Recorded, SlotKind, classify_recorded, is_reserved};
use super::table::ShortcutTable;
use super::{ShortcutConflict, ShortcutSettings};
use crate::keystroke_format::format_keystroke;

fn cmd(key: Key) -> KeyStroke {
    KeyStroke::new(key, Modifiers::COMMAND)
}

fn primary(id: &'static str) -> FocusKey {
    FocusKey::Slot(CaptureTarget::new(id, SlotKind::Primary))
}

fn secondary(id: &'static str) -> FocusKey {
    FocusKey::Slot(CaptureTarget::new(id, SlotKind::Secondary))
}

fn shortcut(id: &'static str, name: &str, chord: KeyStroke) -> Shortcut {
    Shortcut::new(id).name(name).primary(chord).build()
}

/// Save, Sync and Open, uncategorised.
fn three() -> Vec<Shortcut> {
    vec![
        shortcut("app.save", "Save", cmd(Key::S)),
        shortcut("app.sync", "Sync", cmd(Key::K)),
        shortcut("app.open", "Open", cmd(Key::O)),
    ]
}

struct Harness {
    tree: WidgetTree,
    table: WidgetId,
}

impl Harness {
    fn new(shortcuts: Vec<Shortcut>, settings: ShortcutSettings) -> Self {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        for s in shortcuts {
            tree.shortcut_registry_mut().register(s);
        }
        let root = tree.add(settings);
        let mut harness = Self { tree, table: root };
        harness.layout();
        harness.table = find_table(&harness.tree, root).expect("the list is built");
        harness
    }

    fn layout(&mut self) {
        self.tree.layout(SizeProposal::exact(900.0, 600.0));
    }

    fn list(&self) -> &ShortcutTable {
        self.tree
            .widget_as_any(self.table)
            .and_then(|a| a.downcast_ref::<ShortcutTable>())
            .expect("the list")
    }

    fn control(&self, key: FocusKey) -> WidgetId {
        self.list()
            .control(key)
            .unwrap_or_else(|| panic!("no control for {key:?}"))
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.press_key(key, modifiers);
        self.layout();
    }

    /// Focus the control behind `key` and press Enter on it.
    fn activate(&mut self, key: FocusKey) {
        let id = self.control(key);
        self.tree.focus(id);
        self.press(Key::Enter, Modifiers::NONE);
    }

    fn effective(&self, id: &str) -> (Option<KeyStroke>, Option<KeyStroke>) {
        let eff = self
            .tree
            .shortcut_registry()
            .effective(id)
            .expect("registered");
        (eff.primary, eff.secondary)
    }

    fn focused_key(&self) -> Option<FocusKey> {
        let focused = self.tree.focused()?;
        let list = self.list();
        [
            primary("app.save"),
            primary("app.sync"),
            primary("app.open"),
            secondary("app.save"),
            secondary("app.sync"),
            secondary("app.open"),
            FocusKey::Reassign("app.sync"),
            FocusKey::KeepBinding("app.sync"),
            FocusKey::Reset("app.open"),
        ]
        .into_iter()
        .find(|k| list.control(*k) == Some(focused))
    }
}

fn find_table(tree: &WidgetTree, root: WidgetId) -> Option<WidgetId> {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if tree
            .widget_as_any(id)
            .is_some_and(|a| a.downcast_ref::<ShortcutTable>().is_some())
        {
            return Some(id);
        }
        stack.extend(tree.children(id));
    }
    None
}

// ── What a captured chord means ─────────────────────────────────────────

#[test]
fn escape_and_tab_cancel_a_recording() {
    assert_eq!(
        classify_recorded(KeyStroke::new(Key::Escape, Modifiers::NONE)),
        Recorded::Cancel
    );
    assert_eq!(
        classify_recorded(KeyStroke::new(Key::Tab, Modifiers::NONE)),
        Recorded::Cancel
    );
    assert_eq!(
        classify_recorded(KeyStroke::new(Key::Tab, Modifiers::SHIFT)),
        Recorded::Cancel
    );
}

#[test]
fn bare_delete_and_backspace_clear_the_slot() {
    for key in [Key::Delete, Key::Backspace] {
        assert_eq!(
            classify_recorded(KeyStroke::new(key, Modifiers::NONE)),
            Recorded::Clear
        );
    }
}

#[test]
fn a_printable_key_needs_more_than_shift() {
    for ks in [
        KeyStroke::new(Key::A, Modifiers::NONE),
        KeyStroke::new(Key::A, Modifiers::SHIFT),
        KeyStroke::new(Key::Space, Modifiers::NONE),
        KeyStroke::new(Key::ArrowUp, Modifiers::NONE),
        KeyStroke::new(Key::Character('1'), Modifiers::SHIFT),
    ] {
        assert!(
            matches!(classify_recorded(ks), Recorded::Reject(_)),
            "{ks:?} must be refused"
        );
    }
}

#[test]
fn function_keys_and_modified_chords_are_accepted() {
    for ks in [
        KeyStroke::new(Key::F5, Modifiers::NONE),
        KeyStroke::new(Key::F5, Modifiers::SHIFT),
        cmd(Key::B),
        KeyStroke::new(Key::B, Modifiers::ALT),
        KeyStroke::new(Key::Delete, Modifiers::CTRL),
    ] {
        assert_eq!(classify_recorded(ks), Recorded::Chord(ks), "{ks:?}");
    }
}

#[test]
fn the_focus_escape_and_system_chords_are_reserved() {
    assert!(is_reserved(KeyStroke::new(Key::Tab, Modifiers::CTRL)));
    assert!(is_reserved(KeyStroke::new(
        Key::Tab,
        Modifiers::CTRL | Modifiers::SHIFT
    )));
    if cfg!(target_os = "macos") {
        assert!(is_reserved(KeyStroke::new(Key::Tab, Modifiers::SUPER)));
        assert!(is_reserved(KeyStroke::new(Key::Space, Modifiers::SUPER)));
    } else {
        assert!(is_reserved(KeyStroke::new(Key::F4, Modifiers::ALT)));
        assert!(is_reserved(KeyStroke::new(Key::Tab, Modifiers::ALT)));
    }
    assert!(!is_reserved(cmd(Key::S)));
    assert!(matches!(
        classify_recorded(KeyStroke::new(Key::Tab, Modifiers::CTRL)),
        Recorded::Reject(_)
    ));
}

#[test]
fn caps_lock_and_the_menu_key_are_unusable() {
    for key in [Key::CapsLock, Key::ContextMenu] {
        assert!(matches!(
            classify_recorded(KeyStroke::new(key, Modifiers::CTRL)),
            Recorded::Reject(_)
        ));
    }
}

// ── The list ────────────────────────────────────────────────────────────

#[test]
fn rows_are_grouped_and_sorted_by_what_they_say() {
    let h = Harness::new(
        vec![
            Shortcut::new("file.save")
                .name("Save")
                .category("File")
                .primary(cmd(Key::S))
                .build(),
            Shortcut::new("file.open")
                .name("Open")
                .category("File")
                .primary(cmd(Key::O))
                .build(),
            Shortcut::new("view.zoom")
                .name("Zoom")
                .category("View")
                .primary(cmd(Key::Z))
                .build(),
            shortcut("app.quit", "Quit", cmd(Key::Q)),
        ],
        ShortcutSettings::new(),
    );
    // Every tree also registers the standard edit shortcuts, in a group
    // titled "Edit", which reads before "File".
    let ours = ["app.quit", "file.open", "file.save", "view.zoom"];
    let order: Vec<&'static str> = h
        .list()
        .nav_rows()
        .iter()
        .filter_map(|row| match row[0] {
            FocusKey::Slot(t) => Some(t.id),
            _ => None,
        })
        .collect();
    let first_edit = order.iter().position(|id| id.starts_with("edit.")).unwrap();
    let file = order.iter().position(|id| *id == "file.open").unwrap();
    assert!(
        first_edit < file,
        "the Edit group comes before File: {order:?}"
    );
    let ours_in_order: Vec<&str> = order
        .iter()
        .copied()
        .filter(|id| ours.contains(id))
        .collect();
    assert_eq!(
        ours_in_order, ours,
        "uncategorised first, then groups by title, rows by name"
    );
}

#[test]
fn chord_columns_line_up_whatever_the_chord_length() {
    let h = Harness::new(
        vec![
            shortcut("app.short", "Short", cmd(Key::S)),
            shortcut(
                "app.long",
                "Long",
                KeyStroke::new(
                    Key::PageDown,
                    Modifiers::COMMAND | Modifiers::SHIFT | Modifiers::ALT,
                ),
            ),
        ],
        ShortcutSettings::new(),
    );
    let x = |key| h.tree.bounds(h.control(key)).x;
    assert!((x(primary("app.short")) - x(primary("app.long"))).abs() < 0.01);
    assert!((x(secondary("app.short")) - x(secondary("app.long"))).abs() < 0.01);
    assert!(x(secondary("app.short")) > x(primary("app.long")));
}

#[test]
fn recording_does_not_move_the_columns() {
    let mut h = Harness::new(three(), ShortcutSettings::new());
    let columns = |h: &Harness| {
        [
            primary("app.save"),
            secondary("app.save"),
            primary("app.open"),
        ]
        .map(|k| h.tree.bounds(h.control(k)).x)
    };
    let before = columns(&h);
    h.activate(primary("app.save"));
    assert!(
        h.list().shared().capturing.get().is_some(),
        "Enter on a chord starts recording"
    );
    let during = columns(&h);
    for (b, d) in before.iter().zip(during.iter()) {
        assert!((b - d).abs() < 0.01, "{before:?} became {during:?}");
    }
}

#[test]
fn a_recorded_chord_is_assigned_and_focus_comes_back() {
    let mut h = Harness::new(three(), ShortcutSettings::new());
    h.activate(primary("app.save"));
    assert_eq!(h.focused_key(), Some(primary("app.save")));
    h.press(Key::B, Modifiers::COMMAND);
    assert_eq!(h.effective("app.save").0, Some(cmd(Key::B)));
    assert_eq!(h.list().shared().capturing.get(), None);
    assert_eq!(
        h.focused_key(),
        Some(primary("app.save")),
        "the rebuild must give focus back to the chord just recorded"
    );
}

#[test]
fn a_refused_chord_keeps_recording_until_escape() {
    let mut h = Harness::new(three(), ShortcutSettings::new());
    h.activate(primary("app.save"));
    h.press(Key::A, Modifiers::SHIFT);
    assert_eq!(
        h.effective("app.save").0,
        Some(cmd(Key::S)),
        "nothing changed"
    );
    let shared = h.list().shared().clone();
    assert!(shared.capturing.get().is_some(), "still recording");
    assert!(shared.rejection.get().is_some(), "and saying why");

    h.press(Key::Escape, Modifiers::NONE);
    assert_eq!(shared.capturing.get(), None);
    assert!(shared.rejection.get().is_none());
    assert_eq!(h.effective("app.save").0, Some(cmd(Key::S)));
    assert_eq!(h.focused_key(), Some(primary("app.save")));
}

#[test]
fn delete_on_a_focused_chord_removes_it() {
    let mut h = Harness::new(three(), ShortcutSettings::new());
    let id = h.control(primary("app.open"));
    h.tree.focus(id);
    h.press(Key::Delete, Modifiers::NONE);
    assert_eq!(h.effective("app.open").0, None);
    assert_eq!(h.focused_key(), Some(primary("app.open")));
}

#[test]
fn delete_while_recording_removes_the_chord() {
    let mut h = Harness::new(three(), ShortcutSettings::new());
    h.activate(primary("app.open"));
    h.press(Key::Backspace, Modifiers::NONE);
    assert_eq!(h.effective("app.open").0, None);
}

#[test]
fn a_conflict_waits_for_reassign() {
    let mut h = Harness::new(three(), ShortcutSettings::new());
    h.activate(primary("app.sync"));
    h.press(Key::S, Modifiers::COMMAND);
    assert_eq!(h.effective("app.save").0, Some(cmd(Key::S)), "untouched");
    assert_eq!(h.effective("app.sync").0, Some(cmd(Key::K)), "untouched");
    let pending = h.list().shared().pending.get().expect("parked");
    assert_eq!(pending.conflict_id, "app.save");
    assert_eq!(pending.conflict_name, "Save");
    assert_eq!(
        h.focused_key(),
        Some(FocusKey::Reassign("app.sync")),
        "focus goes to the question"
    );

    h.press(Key::Enter, Modifiers::NONE);
    assert_eq!(h.effective("app.sync").0, Some(cmd(Key::S)));
    assert_eq!(h.effective("app.save").0, None, "save lost the chord");
    assert!(h.list().shared().pending.get().is_none());
    assert_eq!(h.focused_key(), Some(primary("app.sync")));
}

#[test]
fn escape_on_a_conflict_keeps_both_bindings() {
    let mut h = Harness::new(three(), ShortcutSettings::new());
    h.activate(primary("app.sync"));
    h.press(Key::S, Modifiers::COMMAND);
    h.press(Key::Escape, Modifiers::NONE);
    assert!(h.list().shared().pending.get().is_none());
    assert_eq!(h.effective("app.save").0, Some(cmd(Key::S)));
    assert_eq!(h.effective("app.sync").0, Some(cmd(Key::K)));
    assert_eq!(h.focused_key(), Some(primary("app.sync")));
}

#[test]
fn without_confirmation_the_chord_moves_at_once() {
    let seen: Rc<RefCell<Option<ShortcutConflict>>> = Rc::new(RefCell::new(None));
    let sink = seen.clone();
    let mut h = Harness::new(
        three(),
        ShortcutSettings::new()
            .confirm_conflicts(false)
            .on_conflict(move |c| *sink.borrow_mut() = Some(c.clone())),
    );
    h.activate(primary("app.sync"));
    h.press(Key::S, Modifiers::COMMAND);
    assert_eq!(h.effective("app.sync").0, Some(cmd(Key::S)));
    assert_eq!(h.effective("app.save").0, None);
    let c = seen.borrow().clone().expect("told");
    assert_eq!(c.displaced_id, "app.save");
    assert_eq!(c.keystroke, cmd(Key::S));
}

#[test]
fn a_conflict_on_an_alternative_unbinds_only_that_slot() {
    let mut h = Harness::new(
        vec![
            Shortcut::new("edit.undo")
                .name("Undo")
                .primary(cmd(Key::Z))
                .secondary(KeyStroke::new(Key::Backspace, Modifiers::ALT))
                .build(),
            Shortcut::new("edit.redo").name("Redo").build(),
        ],
        ShortcutSettings::new().confirm_conflicts(false),
    );
    h.activate(primary("edit.redo"));
    h.press(Key::Backspace, Modifiers::ALT);
    assert_eq!(
        h.effective("edit.redo").0,
        Some(KeyStroke::new(Key::Backspace, Modifiers::ALT))
    );
    assert_eq!(h.effective("edit.undo"), (Some(cmd(Key::Z)), None));
}

#[test]
fn the_list_is_one_tab_stop_and_arrows_move_inside_it() {
    let mut h = Harness::new(three(), ShortcutSettings::new());
    let stops = h.tree.tab_stops_within(h.table);
    assert_eq!(stops.len(), 1, "one Tab stop for the whole list");
    // Rows read Open, Save, Sync.
    assert_eq!(stops[0], h.control(primary("app.open")));

    h.tree.focus(stops[0]);
    h.press(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(h.focused_key(), Some(primary("app.save")));
    h.press(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(h.focused_key(), Some(secondary("app.save")));
    h.press(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(h.focused_key(), Some(secondary("app.sync")));
    h.press(Key::Home, Modifiers::NONE);
    assert_eq!(h.focused_key(), Some(secondary("app.open")));
    let stops = h.tree.tab_stops_within(h.table);
    assert_eq!(
        stops,
        vec![h.control(secondary("app.open"))],
        "the stop follows"
    );
}

#[test]
fn search_finds_a_command_by_its_chord_text() {
    let filter = Signal::new(String::new());
    let mut h = Harness::new(three(), ShortcutSettings::new().with_filter(filter.clone()));
    filter.set(format_keystroke(cmd(Key::K)).to_lowercase());
    h.layout();
    assert_eq!(
        h.list().nav_rows(),
        vec![vec![primary("app.sync"), secondary("app.sync")]]
    );
}

#[test]
fn search_does_not_match_internal_ids() {
    let filter = Signal::new(String::new());
    let mut h = Harness::new(three(), ShortcutSettings::new().with_filter(filter.clone()));
    filter.set("app.".to_string());
    h.layout();
    assert!(h.list().nav_rows().is_empty());
}

#[test]
fn find_by_keys_matches_the_exact_chord() {
    let mut h = Harness::new(
        vec![
            shortcut("app.save", "Save", cmd(Key::S)),
            shortcut(
                "app.save_as",
                "Save as",
                KeyStroke::new(Key::S, Modifiers::COMMAND | Modifiers::SHIFT),
            ),
        ],
        ShortcutSettings::new(),
    );
    let shared = h.list().shared().clone();
    shared.chord_filter.set(Some(cmd(Key::S)));
    shared.filter.set(format_keystroke(cmd(Key::S)));
    h.layout();
    assert_eq!(h.list().nav_rows().len(), 1);
    assert_eq!(h.list().nav_rows()[0][0], primary("app.save"));

    // Editing the text drops the exact match back to a text search.
    shared.filter.set("save".to_string());
    h.layout();
    assert_eq!(h.list().nav_rows().len(), 2);
}

#[test]
fn changed_rows_get_a_reset_that_restores_the_default() {
    let modified = Signal::new(false);
    let mut h = Harness::new(
        three(),
        ShortcutSettings::new().modified_only(modified.clone()),
    );
    assert!(h.list().control(FocusKey::Reset("app.open")).is_none());

    h.activate(primary("app.open"));
    h.press(Key::P, Modifiers::COMMAND);
    assert_eq!(h.effective("app.open").0, Some(cmd(Key::P)));
    assert_eq!(h.list().nav_rows()[0].len(), 3, "the reset joins the row");

    modified.set(true);
    h.layout();
    assert_eq!(h.list().nav_rows().len(), 1, "only the changed row");

    h.activate(FocusKey::Reset("app.open"));
    assert_eq!(h.effective("app.open").0, Some(cmd(Key::O)));
    assert!(
        h.list().nav_rows().is_empty(),
        "nothing is changed any more"
    );
}

#[test]
fn an_empty_list_says_so_without_a_tab_stop() {
    let filter = Signal::new("nothing like this".to_string());
    let h = Harness::new(three(), ShortcutSettings::new().with_filter(filter));
    assert!(h.list().nav_rows().is_empty());
    assert!(h.tree.tab_stops_within(h.table).is_empty());
}

#[test]
fn dropping_the_widget_cancels_a_recording() {
    let mut h = Harness::new(three(), ShortcutSettings::new());
    h.activate(primary("app.save"));
    let root = h.tree.roots()[0];
    h.tree.destroy_subtree_for_testing(root);
    h.tree.press_key(Key::B, Modifiers::COMMAND);
    assert_eq!(h.effective("app.save").0, Some(cmd(Key::S)));
}

#[test]
fn the_search_field_keeps_its_width_and_gives_way_when_narrow() {
    let mut h = Harness::new(three(), ShortcutSettings::new());
    let shared = h.list().shared().clone();
    let search = shared.search_field.get().expect("toolbar built");
    let reset_all = shared.reset_all_button.get().expect("toolbar built");

    // Wide: the field keeps its own width instead of filling the toolbar.
    let wide = h.tree.bounds(search).width;
    assert!((wide - super::SEARCH_WIDTH).abs() < 0.5, "{wide}");

    // Narrow: the field shrinks so the toolbar still fits.
    h.tree.layout(SizeProposal::exact(520.0, 600.0));
    let narrow = h.tree.bounds(search).width;
    assert!(
        (super::SEARCH_MIN_WIDTH..super::SEARCH_WIDTH).contains(&narrow),
        "{narrow}"
    );
    let edge = h.tree.bounds(reset_all);
    assert!(
        edge.x + edge.width <= 520.0 + 0.5,
        "Reset all overflows: {edge:?}"
    );
}
