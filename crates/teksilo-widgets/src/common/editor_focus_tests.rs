// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where focus lands in a multi-line text surface, as a platform adapter reads
//! it.
//!
//! `RichTextEditor`, `CodeEditor`, `PlainTextEditor` and `LogView` take focus
//! and keys on a wrapper widget and keep their text on a body inside it that
//! the keyboard never lands on. `tools/reader/` found every one of them
//! publishing the wrapper as the focus: an unnamed "section" for the rich text
//! editor (a `GenericContainer`, which `common_filter` keeps only while it is
//! focused) and an unnamed "unknown" for the code editor and the log. Orca 46.1
//! said "section." or nothing at all, and no arrow key was ever reported,
//! because `accesskit_atspi_common` sends a caret or selection event only for
//! the node that holds focus (`adapter.rs:215-218`).
//!
//! Each update here goes through `accesskit_consumer`, the crate all three
//! adapters are built on, and the reader keeps the two things the finding is
//! about: what it lands on when focus arrives (role, name, text), and the
//! caret moves AT-SPI would send.

use accesskit_consumer::{FilterResult, NodeRef, Tree, TreeChangeHandler, common_filter};
use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::Role;
use teksilo_core::event::{Key, Modifiers, WidgetEvent};
use teksilo_core::widget::Widget;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_text::text_document::TextDocument;

use crate::code_editor::{CodeEditor, LogView, PlainTextEditor};
use crate::rich_text::RichTextEditor;

const TEXT: &str = "first line\nsecond line\nthird line";

/// What focus arriving on a node tells a reader: the node's role, its name,
/// the text a reader starts reading from it, and the role of the node an
/// adapter lists it under.
#[derive(Debug, Clone, PartialEq)]
struct Arrival {
    role: Role,
    name: Option<String>,
    text: Option<String>,
    within: Option<Role>,
}

/// What one or more updates told the reader.
#[derive(Debug, Default)]
struct Told {
    arrivals: Vec<Arrival>,
    /// The role of each node AT-SPI would send `object:text-caret-moved` for.
    carets: Vec<Role>,
}

impl TreeChangeHandler for Told {
    fn node_added(&mut self, _: &NodeRef) {}

    /// `accesskit_atspi_common` `adapter.rs:287-318` and `197-233`: a node
    /// shown before and after, that supports text ranges, that held focus in
    /// the old tree, and whose caret moved.
    fn node_updated(&mut self, old: &NodeRef, new: &NodeRef) {
        let shown = |n: &NodeRef| common_filter(n) == FilterResult::Include;
        if !shown(old) || !shown(new) || !new.supports_text_ranges() || !old.is_focused() {
            return;
        }
        let caret = |n: &NodeRef| n.raw_text_selection().map(|s| s.focus);
        if caret(old) != caret(new) && new.text_selection_focus().is_some() {
            self.carets.push(new.role());
        }
    }

    fn focus_moved(&mut self, _old: Option<&NodeRef>, new: Option<&NodeRef>) {
        if let Some(node) = new {
            self.arrivals.push(Arrival {
                role: node.role(),
                name: node.label(),
                text: node
                    .supports_text_ranges()
                    .then(|| node.document_range().text()),
                within: node.filtered_parent(&common_filter).map(|p| p.role()),
            });
        }
    }

    fn node_removed(&mut self, _: &NodeRef) {}
}

/// A reader attached to a window holding one text surface.
struct Reader {
    tree: WidgetTree,
    platform: Tree,
    surface: WidgetId,
}

impl Reader {
    fn attach(surface: impl Widget + 'static) -> Self {
        let mut tree = WidgetTree::new();
        let surface = tree.add(surface);
        settle(&mut tree);
        let platform = Tree::new(tree.sync_accessibility(), true);
        Self {
            tree,
            platform,
            surface,
        }
    }

    /// Press `key`, let the surface settle, and return what the reader was told.
    fn press(&mut self, key: Key) -> Told {
        self.tree.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers: Modifiers::NONE,
            text: None,
        });
        settle(&mut self.tree);
        let mut told = Told::default();
        for _ in 0..2 {
            let update = self.tree.sync_accessibility();
            self.platform.update_and_process_changes(update, &mut told);
        }
        told
    }

    /// Tab into the surface, the keyboard's way in, and what the reader heard.
    fn tab_in(&mut self) -> Arrival {
        let told = self.press(Key::Tab);
        let focused = self.tree.focused();
        assert!(
            focused
                .is_some_and(|f| f == self.surface || self.tree.is_descendant_of(f, self.surface)),
            "Tab gives the keyboard to the surface (focus is on {focused:?})"
        );
        told.arrivals
            .last()
            .cloned()
            .expect("focus arriving is an event every reader hears")
    }
}

fn settle(tree: &mut WidgetTree) {
    for _ in 0..3 {
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        tree.layout(SizeProposal::exact(500.0, 300.0));
        let _ = tree.render();
    }
}

fn document() -> TextDocument {
    let doc = TextDocument::new();
    doc.set_plain_text(TEXT).unwrap();
    doc
}

/// Tab in, then Down: the reader lands on the named text, and hears the caret.
fn lands_on_text_and_hears_the_caret(surface: impl Widget + 'static, role: Role) {
    let mut reader = Reader::attach(surface);
    let arrival = reader.tab_in();
    let carets = reader.press(Key::ArrowDown).carets;
    let carries_text = arrival
        .text
        .as_deref()
        .is_some_and(|text| text.contains("second line"));
    assert_eq!(
        (arrival.role, arrival.name.as_deref(), carries_text, carets),
        (role, Some("Notes"), true, vec![role]),
        "focus must arrive on the named node that holds the text, not on the \
         wrapper that takes the keys, and Down must move a caret the adapter \
         reports on it (arrival {arrival:?})"
    );
    assert_eq!(
        arrival.within,
        Some(Role::Window),
        "the wrapper must not stand between the window and the text as an \
         unnamed node of its own"
    );
}

#[test]
fn a_rich_text_editor_takes_focus_on_its_text() {
    lands_on_text_and_hears_the_caret(
        RichTextEditor::editor(document()).access_label(lit!("Notes")),
        Role::MultilineTextInput,
    );
}

#[test]
fn a_rich_text_viewer_takes_focus_on_its_document() {
    lands_on_text_and_hears_the_caret(
        RichTextEditor::read_only(document()).access_label(lit!("Notes")),
        Role::Document,
    );
}

#[test]
fn a_code_editor_takes_focus_on_its_text() {
    lands_on_text_and_hears_the_caret(
        CodeEditor::new(document()).access_label(lit!("Notes")),
        Role::MultilineTextInput,
    );
}

#[test]
fn a_plain_text_editor_takes_focus_on_its_text() {
    lands_on_text_and_hears_the_caret(
        PlainTextEditor::new(document()).access_label(lit!("Notes")),
        Role::MultilineTextInput,
    );
}

#[test]
fn a_log_view_takes_focus_on_its_document() {
    let log = LogView::new();
    let handle = log.handle();
    handle.append_lines(TEXT.lines());
    let mut reader = Reader::attach(log.access_label(lit!("Notes")));
    let arrival = reader.tab_in();
    assert_eq!(
        (arrival.role, arrival.name.as_deref()),
        (Role::Document, Some("Notes")),
        "focus must arrive on the named document, not on the wrapper that takes \
         the keys: {arrival:?}"
    );
    assert!(
        arrival
            .text
            .as_deref()
            .is_some_and(|text| text.contains("second line")),
        "the document focus arrives on must carry the log's lines: {arrival:?}"
    );
    assert_eq!(
        arrival.within,
        Some(Role::Window),
        "the wrapper must not stand between the window and the log as an \
         unnamed node of its own"
    );
}

/// Each surface's own `.label(..)` names the node focus arrives on, and only
/// that node: a second node with the name would be read twice on the way in.
#[test]
fn label_names_the_node_focus_arrives_on() {
    let log = LogView::new().label(lit!("Notes"));
    log.handle().append_lines(TEXT.lines());
    let surfaces: Vec<(&str, Box<dyn Widget>, Role)> = vec![
        (
            "RichTextEditor::editor",
            Box::new(RichTextEditor::editor(document()).label(lit!("Notes"))),
            Role::MultilineTextInput,
        ),
        (
            "RichTextEditor::read_only",
            Box::new(RichTextEditor::read_only(document()).label(lit!("Notes"))),
            Role::Document,
        ),
        (
            "CodeEditor",
            Box::new(CodeEditor::new(document()).label(lit!("Notes"))),
            Role::MultilineTextInput,
        ),
        (
            "PlainTextEditor",
            Box::new(PlainTextEditor::new(document()).label(lit!("Notes"))),
            Role::MultilineTextInput,
        ),
        ("LogView", Box::new(log), Role::Document),
    ];
    let mut heard = Vec::new();
    let mut want = Vec::new();
    for (what, surface, role) in surfaces {
        let mut reader = Reader::attach(surface);
        let arrival = reader.tab_in();
        let update = reader.tree.sync_accessibility();
        let named = update
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some("Notes"))
            .count();
        heard.push((what, arrival.role, arrival.name, named));
        want.push((what, role, Some("Notes".to_string()), 1));
    }
    assert_eq!(
        heard, want,
        "each surface's label names the node that holds the text, and only that node"
    );
}
