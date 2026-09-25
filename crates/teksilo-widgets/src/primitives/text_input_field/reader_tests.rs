// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a screen reader on Linux is told while a text field is edited.
//!
//! `tools/reader/` found every field built on `TextInputField` quiet in three
//! ways: an arrow, Home, End or a selection put nothing on the bus and left
//! the reported caret at the last edit; the first character typed into an
//! empty field was never reported; and a masked password field reported
//! nothing at all. Reading the node after a key press shows none of this: the
//! node was right whenever something else re-walked it.
//!
//! A headless tree has no adapter, so each accessibility update runs through
//! `accesskit_consumer` and [`Adapter`] raises from it what
//! `accesskit_atspi_common-0.20.0` raises: a text change only when the old and
//! the new node both support text ranges (`adapter.rs:120-176`), and a caret
//! move or a selection change only on a node that held focus
//! (`adapter.rs:197-247`). The Text interface itself is published only for a
//! node that supports text ranges (`node.rs:486-488`).

use super::*;
use std::collections::HashSet;

use accesskit_consumer::{
    FilterResult, FullNodeId, NodeRef, Tree, TreeChangeHandler, common_filter,
};
use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::Role;
use teksilo_core::event::Modifiers;
use teksilo_core::widget_tree::WidgetTree;

/// One event on the AT-SPI bus.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Bus {
    /// `object:text-changed:insert`, at a character offset.
    Inserted(usize, String),
    /// `object:text-changed:delete`, at a character offset.
    Removed(usize, String),
    /// `object:text-caret-moved`, to a character offset.
    CaretMoved(usize),
    /// `object:text-selection-changed`.
    SelectionChanged,
}

/// A reader attached to a tree.
struct Reader {
    platform: Tree,
}

impl Reader {
    fn attach(tree: &mut WidgetTree) -> Self {
        Self {
            platform: Tree::new(tree.sync_accessibility(), true),
        }
    }

    /// Everything put on the bus since the last call.
    fn bus(&mut self, tree: &mut WidgetTree) -> Vec<Bus> {
        let mut events = Vec::new();
        for _ in 0..2 {
            // One handler per update, as the adapter makes one: its
            // once-per-node text-change guard lasts one update.
            let mut adapter = Adapter {
                events: Vec::new(),
                checked: HashSet::new(),
            };
            self.platform
                .update_and_process_changes(tree.sync_accessibility(), &mut adapter);
            events.extend(adapter.events);
        }
        events
    }

    /// The Text interface on the focused node: its whole text, or `None` when
    /// AT-SPI publishes no Text interface for it.
    fn focus_text(&self) -> Option<String> {
        let state = self.platform.state();
        let focus = state.focus()?;
        focus
            .supports_text_ranges()
            .then(|| focus.document_range().text())
    }
}

/// `accesskit_atspi_common`'s change handler, cut down to its text events.
struct Adapter {
    events: Vec<Bus>,
    checked: HashSet<FullNodeId>,
}

impl Adapter {
    /// `emit_text_change_if_needed_parent`.
    fn text_change_parent(&mut self, old: &NodeRef, new: &NodeRef) {
        if !new.supports_text_ranges() || !old.supports_text_ranges() {
            return;
        }
        if !self.checked.insert(new.id()) {
            return;
        }
        let old_text = old.document_range().text();
        let new_text = new.document_range().text();
        if old_text == new_text {
            return;
        }
        let prefix: usize = old_text
            .chars()
            .zip(new_text.chars())
            .take_while(|(a, b)| a == b)
            .count();
        let old_rest: Vec<char> = old_text.chars().skip(prefix).collect();
        let new_rest: Vec<char> = new_text.chars().skip(prefix).collect();
        let suffix = old_rest
            .iter()
            .rev()
            .zip(new_rest.iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let removed: String = old_rest[..old_rest.len() - suffix].iter().collect();
        let inserted: String = new_rest[..new_rest.len() - suffix].iter().collect();
        if !removed.is_empty() {
            self.events.push(Bus::Removed(prefix, removed));
        }
        if !inserted.is_empty() {
            self.events.push(Bus::Inserted(prefix, inserted));
        }
    }

    /// `emit_text_selection_change`, for a node that was already there.
    fn selection_change(&mut self, old: &NodeRef, new: &NodeRef) {
        if !new.supports_text_ranges() {
            return;
        }
        if !old.is_focused() || new.raw_text_selection() == old.raw_text_selection() {
            return;
        }
        if let Some(selection) = new.text_selection()
            && (!selection.is_degenerate()
                || old
                    .text_selection()
                    .is_some_and(|selection| !selection.is_degenerate()))
        {
            self.events.push(Bus::SelectionChanged);
        }
        let old_caret = old.raw_text_selection().map(|s| s.focus);
        let new_caret = new.raw_text_selection().map(|s| s.focus);
        if old_caret != new_caret
            && let Some(focus) = new.text_selection_focus()
        {
            self.events
                .push(Bus::CaretMoved(focus.to_global_usv_index()));
        }
    }
}

impl TreeChangeHandler for Adapter {
    fn node_added(&mut self, _node: &NodeRef) {}

    fn node_updated(&mut self, old: &NodeRef, new: &NodeRef) {
        match new.role() {
            Role::TextRun | Role::GenericContainer => {
                if let (Some(old_parent), Some(new_parent)) = (
                    old.filtered_parent(&common_filter),
                    new.filtered_parent(&common_filter),
                ) {
                    self.text_change_parent(&old_parent, &new_parent);
                }
            }
            _ => self.text_change_parent(old, new),
        }
        if common_filter(old) == FilterResult::Include
            && common_filter(new) == FilterResult::Include
        {
            self.selection_change(old, new);
        }
    }

    fn focus_moved(&mut self, _old: Option<&NodeRef>, _new: Option<&NodeRef>) {}

    fn node_removed(&mut self, _node: &NodeRef) {}
}

fn tick(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.layout(SizeProposal::exact(200.0, 24.0));
}

/// A field as an application runs it: measured by text-typeset, the backend
/// every Teksilo app shapes with, and not by the mock. The difference is the
/// point for an empty field: text-typeset measures "" as no line at all.
fn app_tree() -> WidgetTree {
    use std::any::TypeId;
    use std::collections::HashMap;
    let shared = SharedTypesetter::new_with_default_font();
    let mut tree = WidgetTree::new()
        .with_theme(teksilo_core::presets::intui::light())
        .with_text_backend(shared.as_text_backend());
    let mut registry: HashMap<TypeId, Box<dyn std::any::Any>> = HashMap::new();
    registry.insert(TypeId::of::<SharedTypesetter>(), Box::new(shared));
    let ctx = teksilo_core::event_source::TreeAppContext::empty().with_app_state(registry);
    tree.set_app_context(Rc::new(ctx));
    tree
}

/// `field` in an application tree, focused from the keyboard, with a reader
/// attached once the focus has settled.
fn focused(field: TextInputField) -> (WidgetTree, Reader) {
    let mut tree = app_tree();
    let id = tree.add(field);
    tick(&mut tree);
    tree.focus(id);
    tick(&mut tree);
    let reader = Reader::attach(&mut tree);
    (tree, reader)
}

fn press(tree: &mut WidgetTree, key: Key, modifiers: Modifiers) {
    tree.press_key(key, modifiers);
    tick(tree);
}

fn type_str(tree: &mut WidgetTree, text: &str) {
    tree.type_text(WidgetId::default(), text);
    tick(tree);
}

#[test]
fn caret_moves_and_selections_reach_the_reader() {
    let (mut tree, mut reader) = focused(TextInputField::new(Signal::new("ab".to_string())));

    // Keyboard focus selected everything; End collapses it at the end.
    press(&mut tree, Key::End, Modifiers::NONE);
    assert_eq!(
        reader.bus(&mut tree),
        vec![Bus::SelectionChanged],
        "End collapses the selection a keyboard focus made"
    );

    press(&mut tree, Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(
        reader.bus(&mut tree),
        vec![Bus::CaretMoved(1)],
        "Left over 'ab' moves the reader's caret to 1"
    );

    press(&mut tree, Key::Home, Modifiers::NONE);
    assert_eq!(reader.bus(&mut tree), vec![Bus::CaretMoved(0)], "Home");

    press(&mut tree, Key::End, Modifiers::SHIFT);
    assert_eq!(
        reader.bus(&mut tree),
        vec![Bus::SelectionChanged, Bus::CaretMoved(2)],
        "Shift+End selects to the end"
    );

    press(&mut tree, Key::ArrowLeft, Modifiers::NONE);
    let _ = reader.bus(&mut tree);
    press(&mut tree, Key::A, Modifiers::COMMAND);
    assert!(
        reader.bus(&mut tree).contains(&Bus::SelectionChanged),
        "Ctrl+A selects the whole text"
    );
}

/// An edit moves the caret too, and not through a key handler: the typed
/// character is inserted by the frame tick. The caret that edit left must be
/// the one the next move is measured against, or a Left that lands where the
/// field's own record of the caret still (wrongly) is changes nothing.
#[test]
fn a_caret_move_after_typing_reaches_the_reader() {
    let (mut tree, mut reader) = focused(TextInputField::new(Signal::new("ab".to_string())));
    press(&mut tree, Key::End, Modifiers::NONE);
    type_str(&mut tree, "c");
    assert!(
        reader
            .bus(&mut tree)
            .contains(&Bus::Inserted(2, "c".into())),
        "the typed character is reported"
    );

    press(&mut tree, Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(
        reader.bus(&mut tree),
        vec![Bus::CaretMoved(2)],
        "Left after typing 'c' at the end of 'ab' moves the reader's caret to 2"
    );
}

#[test]
fn an_empty_field_reports_its_first_and_last_character() {
    let (mut tree, mut reader) = focused(TextInputField::new(Signal::new(String::new())));
    assert_eq!(
        reader.focus_text().as_deref(),
        Some(""),
        "an empty field offers the Text interface, with no characters"
    );

    type_str(&mut tree, "a");
    assert!(
        reader
            .bus(&mut tree)
            .contains(&Bus::Inserted(0, "a".into())),
        "the first character typed into an empty field is reported"
    );

    press(&mut tree, Key::Backspace, Modifiers::NONE);
    assert!(
        reader.bus(&mut tree).contains(&Bus::Removed(0, "a".into())),
        "deleting the last character is reported"
    );
    assert_eq!(reader.focus_text().as_deref(), Some(""));
}

#[test]
fn a_masked_field_echoes_each_keystroke_as_a_mask_character() {
    let (mut tree, mut reader) =
        focused(TextInputField::new(Signal::new(String::new())).secure(EchoMode::Masked));
    assert_eq!(
        reader.focus_text().as_deref(),
        Some(""),
        "a masked field offers the Text interface its echo is read through"
    );

    let mut heard = Vec::new();
    for ch in "q7".chars() {
        type_str(&mut tree, &ch.to_string());
        heard.extend(reader.bus(&mut tree));
    }
    assert!(
        heard.contains(&Bus::Inserted(0, "•".into()))
            && heard.contains(&Bus::Inserted(1, "•".into())),
        "each keystroke is reported as one mask character, got {heard:?}"
    );
    assert_eq!(reader.focus_text().as_deref(), Some("••"));

    press(&mut tree, Key::Backspace, Modifiers::NONE);
    let deleted = reader.bus(&mut tree);
    assert!(
        deleted.contains(&Bus::Removed(1, "•".into())),
        "a deletion is reported as one mask character, got {deleted:?}"
    );
    heard.extend(deleted);
    for event in &heard {
        if let Bus::Inserted(_, text) | Bus::Removed(_, text) = event {
            assert!(
                !text.contains('q') && !text.contains('7'),
                "the secret reached the bus: {event:?}"
            );
        }
    }
}

#[test]
fn a_no_echo_field_reports_nothing_of_what_is_typed() {
    let (mut tree, mut reader) =
        focused(TextInputField::new(Signal::new(String::new())).secure(EchoMode::NoEcho));
    type_str(&mut tree, "ab");
    let heard = reader.bus(&mut tree);
    assert!(
        !heard
            .iter()
            .any(|e| matches!(e, Bus::Inserted(..) | Bus::CaretMoved(_))),
        "NoEcho hides even the length, so typing reports nothing, got {heard:?}"
    );
    assert_eq!(
        reader.focus_text().as_deref(),
        Some(""),
        "the Text interface is there, and always empty"
    );
}

/// Revealed under `SwapRole`, a password is the field's text, for a reader who
/// asks for it. The reveal must not broadcast it: the adapter reports a change
/// of text as the new text inserted, and Orca 46.1 speaks an insertion into
/// the focused field when that field's selection is the inserted text
/// (`script_utilities.py`, `isSelectedTextInsertionEvent`), as it is once a
/// keyboard focus has selected the whole password. Before the mask had runs, a
/// reveal changed only the role.
#[test]
fn revealing_a_password_puts_no_plaintext_on_the_bus() {
    let revealed = Signal::new(false);
    let (mut tree, mut reader) = focused(
        TextInputField::new(Signal::new("hunter2".to_string()))
            .secure(EchoMode::Masked)
            .revealed(revealed.clone()),
    );
    assert_eq!(reader.focus_text().as_deref(), Some("•••••••"));

    revealed.set(true);
    let mut heard = Vec::new();
    for _ in 0..2 {
        tick(&mut tree);
        heard.extend(reader.bus(&mut tree));
    }
    for event in &heard {
        if let Bus::Inserted(_, text) | Bus::Removed(_, text) = event {
            assert!(
                !text.contains("hunter"),
                "revealing the password put it on the bus: {event:?}"
            );
        }
    }
    assert_eq!(
        reader.focus_text().as_deref(),
        Some("hunter2"),
        "once revealed, the field reads as its text to a reader who asks"
    );
}

/// A field revealed from the start has published nothing a reveal could
/// change, so its first walk, the tree a reader first receives, holds its text.
#[test]
fn a_field_revealed_from_the_start_reads_as_its_text_at_once() {
    let (_tree, reader) = focused(
        TextInputField::new(Signal::new("hunter2".to_string()))
            .secure(EchoMode::Masked)
            .revealed(Signal::new(true)),
    );
    assert_eq!(reader.focus_text().as_deref(), Some("hunter2"));
}

/// Revealed under `SwapRole`, a password is the field's text by the user's
/// own choice. Hidden again, it must not cross the bus once more: the adapter
/// reports a change of text as the old text deleted, and the old text is the
/// password.
#[test]
fn hiding_a_revealed_password_puts_no_plaintext_on_the_bus() {
    let revealed = Signal::new(false);
    let (mut tree, mut reader) = focused(
        TextInputField::new(Signal::new("hunter2".to_string()))
            .secure(EchoMode::Masked)
            .revealed(revealed.clone()),
    );
    assert_eq!(reader.focus_text().as_deref(), Some("•••••••"));

    revealed.set(true);
    for _ in 0..2 {
        tick(&mut tree);
        let _ = reader.bus(&mut tree);
    }
    assert_eq!(
        reader.focus_text().as_deref(),
        Some("hunter2"),
        "revealed under SwapRole, the field reads as its text"
    );

    revealed.set(false);
    tick(&mut tree);
    let mut heard = reader.bus(&mut tree);
    tick(&mut tree);
    heard.extend(reader.bus(&mut tree));
    for event in &heard {
        if let Bus::Inserted(_, text) | Bus::Removed(_, text) = event {
            assert!(
                !text.contains("hunter"),
                "hiding the password put it on the bus again: {event:?}"
            );
        }
    }
    assert_eq!(
        reader.focus_text().as_deref(),
        Some("•••••••"),
        "the mask is back once the field is hidden"
    );

    press(&mut tree, Key::End, Modifiers::NONE);
    let _ = reader.bus(&mut tree);
    type_str(&mut tree, "x");
    assert!(
        reader
            .bus(&mut tree)
            .contains(&Bus::Inserted(7, "•".into())),
        "and the next keystroke is echoed as a mask character"
    );
}
