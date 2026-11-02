// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Links from the keyboard and to a screen reader: Enter follows the link
//! under the caret in a viewer, Tab and Shift+Tab move between a viewer's
//! links, the context menu opens one, and every link is a `Role::Link` node
//! in reading order that a Click follows.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::{Action, Role};
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;

use super::policy::{CommandFilter, TabKey};
use super::{EditorHandle, RichTextEditor};
use crate::common::heard_test::{Heard, Listener};

const SEE_BACKUPS: &str = "see [backups](help:backups) for more";

fn doc(markdown: &str) -> TextDocument {
    let doc = TextDocument::new();
    doc.set_markdown(markdown).unwrap().wait().unwrap();
    doc
}

/// What the editor's `on_link_activated` was handed.
type Followed = Rc<RefCell<Vec<String>>>;

fn following(editor: RichTextEditor) -> (RichTextEditor, Followed) {
    let followed: Followed = Rc::default();
    let sink = followed.clone();
    let editor =
        editor.on_link_activated(move |href, _ctx| sink.borrow_mut().push(href.to_string()));
    (editor, followed)
}

/// A viewer that follows links (to nowhere): its links are Tab stops.
fn viewer(document: TextDocument) -> RichTextEditor {
    RichTextEditor::read_only(document).on_link_activated(|_, _| {})
}

struct Mounted {
    tree: WidgetTree,
    handle: EditorHandle,
    reader: Listener,
}

impl Mounted {
    /// `editor` between a "Before" and an "After" button, focus Tabbed into
    /// it, a reader attached.
    fn new(editor: RichTextEditor) -> Self {
        use crate::button::Button;
        use crate::primitives::VStack;
        use teksilo_i18n::lit;
        let handle = editor.handle();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(
            VStack::new()
                .child(Button::new(lit!("Before")))
                .child(editor)
                .child(Button::new(lit!("After"))),
        );
        tree.layout(SizeProposal::exact(600.0, 400.0));
        let _ = tree.render();
        let mut reader = Listener::attach(&mut tree);
        tree.press_key(Key::Tab, Modifiers::NONE);
        tree.press_key(Key::Tab, Modifiers::NONE);
        settle(&mut tree);
        reader.heard(&mut tree);
        Self {
            tree,
            handle,
            reader,
        }
    }

    fn settle(&mut self) {
        settle(&mut self.tree);
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) -> Vec<Heard> {
        self.tree.press_key(key, modifiers);
        self.settle();
        self.reader.heard(&mut self.tree)
    }

    fn selection(&self) -> (usize, usize) {
        let (anchor, pos) = self.handle.selection();
        (anchor.min(pos), anchor.max(pos))
    }

    fn caret_at(&mut self, at: usize) {
        self.handle.select_range(at, at);
        self.settle();
    }
}

/// One turn of the loop: the frame tick, the layout and the paint.
fn settle(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.layout(SizeProposal::exact(600.0, 400.0));
    let _ = tree.render();
}

fn left(heard: &[Heard], to: &str) -> bool {
    heard.contains(&Heard::Focus(to.into()))
}

// --- Enter ------------------------------------------------------------------

/// In a viewer, Enter with the caret on a link follows it; off a link, and
/// with Shift, it does nothing.
#[test]
fn enter_follows_the_link_under_a_viewers_caret() {
    let (editor, followed) = following(RichTextEditor::read_only(doc(SEE_BACKUPS)));
    let mut m = Mounted::new(editor);
    m.caret_at(6);
    m.press(Key::Enter, Modifiers::SHIFT);
    assert!(followed.borrow().is_empty(), "Shift+Enter follows nothing");
    m.press(Key::Enter, Modifiers::NONE);
    assert_eq!(followed.borrow().as_slice(), ["help:backups"]);
    m.caret_at(1);
    m.press(Key::Enter, Modifiers::NONE);
    assert_eq!(
        followed.borrow().len(),
        1,
        "off a link Enter follows nothing"
    );
}

/// A viewer with no callback installed: Enter on a link is a no-op.
#[test]
fn enter_on_a_link_with_no_callback_does_nothing() {
    let mut m = Mounted::new(RichTextEditor::read_only(doc(SEE_BACKUPS)));
    m.caret_at(6);
    m.press(Key::Enter, Modifiers::NONE);
    assert_eq!(m.handle.to_plain_text(), "see backups for more");
}

/// In an editable editor Enter still splits the paragraph, on a link too.
#[test]
fn enter_on_a_link_in_an_editor_splits_the_paragraph() {
    let (editor, followed) = following(RichTextEditor::editor(doc(SEE_BACKUPS)));
    let mut m = Mounted::new(editor);
    m.caret_at(6);
    m.press(Key::Enter, Modifiers::NONE);
    assert!(followed.borrow().is_empty());
    assert_eq!(m.handle.to_plain_text(), "see ba\nckups for more");
}

/// The gate is the command filter, as every other gate: an editor narrowed to
/// `ReadOnly` follows the link with Enter.
#[test]
fn enter_follows_a_link_in_an_editor_narrowed_to_read_only() {
    let (editor, followed) = following(RichTextEditor::editor(doc(SEE_BACKUPS)));
    let mut m = Mounted::new(editor);
    m.handle.set_command_filter(CommandFilter::ReadOnly);
    m.caret_at(6);
    m.press(Key::Enter, Modifiers::NONE);
    assert_eq!(followed.borrow().as_slice(), ["help:backups"]);
}

// --- Tab --------------------------------------------------------------------

/// Tab selects the next link, then leaves after the last one; Shift+Tab
/// selects the previous one, then leaves before the first; Enter on a
/// selected link follows it.
#[test]
fn tab_and_shift_tab_move_between_a_viewers_links() {
    let (editor, followed) = following(RichTextEditor::read_only(doc(SEE_BACKUPS)));
    let mut m = Mounted::new(editor);
    m.caret_at(0);
    let heard = m.press(Key::Tab, Modifiers::NONE);
    assert!(!left(&heard, "After"), "{heard:?}");
    assert_eq!(m.selection(), (4, 11));
    m.press(Key::Enter, Modifiers::NONE);
    assert_eq!(followed.borrow().as_slice(), ["help:backups"]);

    let heard = m.press(Key::Tab, Modifiers::NONE);
    assert!(
        left(&heard, "After"),
        "past the last link Tab leaves: {heard:?}"
    );

    let mut m = Mounted::new(viewer(doc(SEE_BACKUPS)));
    m.caret_at(15);
    let heard = m.press(Key::Tab, Modifiers::SHIFT);
    assert!(!left(&heard, "Before"), "{heard:?}");
    assert_eq!(m.selection(), (4, 11));
    let heard = m.press(Key::Tab, Modifiers::SHIFT);
    assert!(
        left(&heard, "Before"),
        "before the first link Shift+Tab leaves: {heard:?}"
    );
}

/// The role and name of the node a screen reader holds as focus, after
/// `active_descendant` is followed, and whether the node itself says it has
/// focus (UIA's `HasKeyboardFocus`, which `accesskit_windows` reads from
/// `NodeRef::is_focused`, active descendant included).
///
/// Not `is_focusable`: that reads an advertised `Action::Focus`, which the
/// link does not carry, as the table cells a `TableView` names as its active
/// descendant do not. The dispatcher services `Action::Focus` on the
/// editor that owns the node, so advertising it on the link would promise
/// a focus that lands on the document, not on the link.
fn reader_focus(m: &Mounted) -> (Role, Option<String>, bool) {
    let state = m.reader.platform().state();
    let focus = state.focus().expect("the window has focus");
    (focus.role(), focus.label(), focus.is_focused())
}

/// A link Tab selects is where a screen reader's focus is: the editor names
/// the link's node as its active descendant, so the reader moves onto the
/// link and says "link". It heard selected text in the document before. Any
/// other key, a click or leaving gives focus back to the document.
#[test]
fn a_link_tab_selects_holds_the_readers_focus() {
    let mut m = Mounted::new(viewer(doc(
        "see [backups](help:backups) or [notes](help:notes)",
    )));
    m.caret_at(0);
    assert_eq!(reader_focus(&m).0, Role::Document);

    let heard = m.press(Key::Tab, Modifiers::NONE);
    assert_eq!(
        reader_focus(&m),
        (Role::Link, Some("backups".into()), true),
        "{heard:?}"
    );
    assert!(
        left(&heard, "backups"),
        "focus moves onto the link: {heard:?}"
    );
    // The document keeps its text selection, the link's text: text review
    // and braille still read the document around it.
    {
        let state = m.reader.platform().state();
        let document = state.focus_in_tree();
        assert_eq!(document.role(), Role::Document);
        assert!(document.has_text_selection());
    }

    let heard = m.press(Key::Tab, Modifiers::NONE);
    assert_eq!(reader_focus(&m), (Role::Link, Some("notes".into()), true));
    assert!(left(&heard, "notes"), "{heard:?}");
    m.press(Key::Tab, Modifiers::SHIFT);
    assert_eq!(reader_focus(&m).1.as_deref(), Some("backups"));

    // Moving the caret off the link gives focus back to the document.
    m.press(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(reader_focus(&m).0, Role::Document);
    // A selection that covers a link but was not made by Tab is text.
    m.handle.select_range(4, 11);
    m.settle();
    m.reader.heard(&mut m.tree);
    assert_eq!(
        reader_focus(&m).0,
        Role::Document,
        "a selection set by hand is text, not a link stop"
    );

    // Past the last link focus leaves the editor for the next control, and
    // coming back finds the document, the link's text still selected.
    let mut m = Mounted::new(viewer(doc(SEE_BACKUPS)));
    m.caret_at(0);
    m.press(Key::Tab, Modifiers::NONE);
    assert_eq!(reader_focus(&m).0, Role::Link);
    let heard = m.press(Key::Tab, Modifiers::NONE);
    assert!(left(&heard, "After"), "{heard:?}");
    assert_eq!(reader_focus(&m).1.as_deref(), Some("After"));
    m.press(Key::Tab, Modifiers::SHIFT);
    assert_eq!(m.selection(), (4, 11));
    assert_eq!(reader_focus(&m).0, Role::Document);

    // Focus taken away with no key the editor sees (a click elsewhere, an
    // application moving it) ends the stop too.
    m.caret_at(0);
    m.press(Key::Tab, Modifiers::NONE);
    assert_eq!(reader_focus(&m).1.as_deref(), Some("backups"));
    let after = m
        .reader
        .widget(Role::Button, "After")
        .expect("the After button");
    m.tree.focus(after);
    m.settle();
    m.reader.heard(&mut m.tree);
    m.press(Key::Tab, Modifiers::SHIFT);
    assert_eq!(m.selection(), (4, 11));
    assert_eq!(reader_focus(&m).0, Role::Document);
}

/// A key that changes nothing the walk reads (Escape, a letter in a
/// viewer) still gives focus back to the document: clearing a stop asks for
/// a new accessibility walk, or the link kept the focus until some unrelated
/// change re-walked the tree.
#[test]
fn a_key_that_changes_nothing_else_ends_the_link_focus() {
    for key in [Key::Escape, Key::Q] {
        let mut m = Mounted::new(viewer(doc(SEE_BACKUPS)));
        m.caret_at(0);
        m.press(Key::Tab, Modifiers::NONE);
        assert_eq!(reader_focus(&m).0, Role::Link);
        let heard = m.press(key, Modifiers::NONE);
        assert_eq!(m.selection(), (4, 11), "{key:?} leaves the selection");
        assert_eq!(reader_focus(&m).0, Role::Document, "{key:?}: {heard:?}");
    }
}

/// An editable editor keeps Tab, so it has no link stops: a selection
/// covering a link leaves the reader on the editor and its text.
#[test]
fn an_editors_selected_link_is_text_to_a_reader() {
    let mut m = Mounted::new(RichTextEditor::editor(doc(SEE_BACKUPS)).on_link_activated(|_, _| {}));
    m.handle.select_range(4, 11);
    m.settle();
    m.reader.heard(&mut m.tree);
    assert_eq!(reader_focus(&m).0, Role::MultilineTextInput);
}

/// With no link in it, a viewer leaves on the first Tab, as before.
#[test]
fn tab_leaves_a_viewer_with_no_link() {
    let mut m = Mounted::new(viewer(doc("plain prose")));
    let heard = m.press(Key::Tab, Modifiers::NONE);
    assert!(left(&heard, "After"), "{heard:?}");
}

/// `TabKey::MoveFocus` turns the link stops off.
#[test]
fn move_focus_turns_the_link_stops_off() {
    let mut m = Mounted::new(viewer(doc(SEE_BACKUPS)).tab_key(TabKey::MoveFocus));
    m.caret_at(0);
    let heard = m.press(Key::Tab, Modifiers::NONE);
    assert!(left(&heard, "After"), "{heard:?}");
}

/// A mark inside a link does not split it: one stop for the whole link.
#[test]
fn a_bold_word_inside_a_link_is_one_stop() {
    let mut m = Mounted::new(viewer(doc("go [to **the** help](help:index) now")));
    m.caret_at(0);
    m.press(Key::Tab, Modifiers::NONE);
    assert_eq!(m.selection(), (3, 14));
    let heard = m.press(Key::Tab, Modifiers::NONE);
    assert!(left(&heard, "After"), "one stop only: {heard:?}");
}

/// Links in several blocks, a list item and a table cell are visited in
/// document order, and back in reverse.
#[test]
fn tab_visits_links_in_document_order() {
    let markdown = "[one](a:1) here\n\nplain\n\n- item [two](a:2)\n\n| [three](a:3) | x |\n|---|---|\n| y | z |\n\nlast [four](a:4)";
    let document = doc(markdown);
    let mut m = Mounted::new(viewer(document.clone()));
    m.caret_at(0);
    let mut seen = Vec::new();
    for _ in 0..4 {
        m.press(Key::Tab, Modifiers::NONE);
        seen.push(m.handle.selected_text());
    }
    assert_eq!(seen, ["one", "two", "three", "four"]);
    let mut back = Vec::new();
    for _ in 0..3 {
        m.press(Key::Tab, Modifiers::SHIFT);
        back.push(m.handle.selected_text());
    }
    assert_eq!(back, ["three", "two", "one"]);
}

/// Two links that touch are two stops, in both directions.
#[test]
fn touching_links_are_two_stops() {
    let mut m = Mounted::new(viewer(doc("[one](a:1)[two](a:2)")));
    m.caret_at(0);
    m.press(Key::Tab, Modifiers::NONE);
    assert_eq!(m.selection(), (0, 3));
    m.press(Key::Tab, Modifiers::NONE);
    assert_eq!(m.selection(), (3, 6));
    m.press(Key::Tab, Modifiers::SHIFT);
    assert_eq!(m.selection(), (0, 3));
}

// --- The context menu -------------------------------------------------------

/// Right-click the link and read the menu's rows, with the widget behind the
/// "Open Link" row if there is one.
fn menu_rows(m: &mut Mounted) -> (Vec<String>, Option<teksilo_core::widget_id::WidgetId>) {
    use teksilo_core::event::{PointerButton, WidgetEvent};
    let at = {
        let rect = m.handle.offset_rect(6).expect("laid out");
        teksilo_canvas::Point::new(rect.x + 1.0, rect.y + rect.height / 2.0)
    };
    m.tree.dispatch_event(WidgetEvent::pointer_down(
        at,
        PointerButton::Secondary,
        Modifiers::NONE,
    ));
    m.settle();
    let update = m.tree.sync_accessibility();
    let rows: Vec<_> = update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == Role::MenuItem)
        .collect();
    let open = rows
        .iter()
        .find(|(_, n)| n.label() == Some("Open Link"))
        .and_then(|(id, _)| teksilo_core::accessibility::node_id_to_widget_id_maybe(*id));
    let labels = rows
        .iter()
        .filter_map(|(_, n)| n.label().map(str::to_string))
        .collect();
    (labels, open)
}

/// A right-click on a link offers "Open Link" first, which follows it; in an
/// editor as in a viewer.
#[test]
fn the_context_menu_opens_a_link() {
    for editor in [
        RichTextEditor::editor(doc(SEE_BACKUPS)),
        RichTextEditor::read_only(doc(SEE_BACKUPS)),
    ] {
        let (editor, followed) = following(editor);
        let mut m = Mounted::new(editor);
        let (rows, open) = menu_rows(&mut m);
        assert_eq!(
            rows.first().map(String::as_str),
            Some("Open Link"),
            "{rows:?}"
        );
        m.tree.click(open.expect("the row maps to its widget"));
        m.settle();
        assert_eq!(followed.borrow().as_slice(), ["help:backups"]);
    }
}

/// No callback, no row.
#[test]
fn no_open_link_row_without_a_callback() {
    let mut m = Mounted::new(RichTextEditor::editor(doc(SEE_BACKUPS)));
    let (rows, _) = menu_rows(&mut m);
    assert!(!rows.iter().any(|r| r == "Open Link"), "{rows:?}");
}

// --- A screen reader --------------------------------------------------------

/// The link is a `Role::Link` a reader finds by its text, and its Click
/// follows it.
#[test]
fn a_link_is_a_link_node_whose_click_follows_it() {
    let (editor, followed) = following(RichTextEditor::read_only(doc(SEE_BACKUPS)));
    let mut m = Mounted::new(editor);
    assert!(m.reader.finds(Role::Link, "backups"));
    assert_eq!(link_clicks(&m, "backups"), Some(true));
    let link = find_link(&m, "backups");
    let mut ops = teksilo_core::window::NoopWindowOps;
    m.tree
        .dispatch_access_action(link, Action::Click, None, &mut ops);
    assert_eq!(followed.borrow().as_slice(), ["help:backups"]);
}

/// Whether the link named `name` advertises Click; `None` with no such link.
fn link_clicks(m: &Mounted, name: &str) -> Option<bool> {
    use accesskit_consumer::common_filter;
    fn walk(node: accesskit_consumer::NodeRef<'_>, name: &str) -> Option<bool> {
        if node.role() == Role::Link && node.label().as_deref() == Some(name) {
            return Some(node.data().supports_action(Action::Click));
        }
        node.filtered_children(&common_filter)
            .find_map(|c| walk(c, name))
    }
    walk(m.reader.platform().state().root(), name)
}

fn find_link(m: &Mounted, name: &str) -> teksilo_core::accesskit::NodeId {
    use accesskit_consumer::common_filter;
    fn walk(
        node: accesskit_consumer::NodeRef<'_>,
        name: &str,
    ) -> Option<teksilo_core::accesskit::NodeId> {
        if node.role() == Role::Link && node.label().as_deref() == Some(name) {
            return Some(node.locate().0);
        }
        node.filtered_children(&common_filter)
            .find_map(|c| walk(c, name))
    }
    walk(m.reader.platform().state().root(), name).expect("the link is in the tree")
}

/// The link's runs read through a text container that supports text
/// ranges, and the editor's text keeps one character space: a reader's
/// caret placed in the run after the link lands where it should.
#[test]
fn a_readers_caret_after_a_link_lands_in_place() {
    use accesskit_consumer::common_filter;
    use teksilo_core::accesskit::{ActionData, TextPosition, TextSelection};
    let mut m = Mounted::new(RichTextEditor::read_only(doc(SEE_BACKUPS)));
    let state = m.reader.platform().state();
    let editor = {
        fn find(node: accesskit_consumer::NodeRef<'_>) -> Option<accesskit_consumer::NodeRef<'_>> {
            if node.role() == Role::Document {
                return Some(node);
            }
            node.filtered_children(&common_filter).find_map(find)
        }
        find(state.root()).expect("the viewer")
    };
    assert!(
        editor
            .document_range()
            .text()
            .starts_with("see backups for more"),
        "one character space across the link"
    );
    let link = editor
        .filtered_children(&common_filter)
        .find(|c| c.role() == Role::Link)
        .expect("the link, among the editor's children in reading order");
    let container = link
        .filtered_children(&common_filter)
        .next()
        .expect("the link's text container");
    assert_eq!(container.role(), Role::Label);
    assert!(container.supports_text_ranges());
    // Runs are left out of object navigation, so the unfiltered children.
    let after = editor
        .children()
        .rfind(|c| c.role() == Role::TextRun)
        .expect("the run after the link")
        .locate()
        .0;
    let editor_id = editor.locate().0;
    let position = TextPosition {
        node: after,
        character_index: 1,
    };
    let mut ops = teksilo_core::window::NoopWindowOps;
    m.tree.dispatch_access_action(
        editor_id,
        Action::SetTextSelection,
        Some(ActionData::SetTextSelection(TextSelection {
            anchor: position,
            focus: position,
        })),
        &mut ops,
    );
    m.settle();
    assert_eq!(
        m.handle.cursor_position(),
        12,
        "one character into \" for more\""
    );
}

/// The extents the editor finds are text-document's own: at every position
/// of a document with links, `block_links` agrees with `link_at_caret`.
#[test]
fn the_links_agree_with_link_at_caret_everywhere() {
    let document = doc("a [one](x:1) b [t **w** o](x:2)[three](x:3) ![i](img.png) [f](x:4)");
    let flow = document.snapshot_flow();
    let links = super::links::document_links(&flow.elements);
    let len = document.character_count();
    let probe = document.cursor();
    for pos in 0..=len {
        probe.set_position(pos, teksilo_text::text_document::MoveMode::MoveAnchor);
        let ours = links
            .iter()
            .filter(|l| l.start <= pos && pos <= l.end)
            .map(|l| (l.start, l.end, l.href.clone()))
            .collect::<Vec<_>>();
        let theirs = probe.link_at_caret().map(|l| (l.start, l.end, l.href));
        match theirs {
            Some(t) => assert!(ours.contains(&t), "at {pos}: {t:?} not in {ours:?}"),
            None => assert!(ours.is_empty(), "at {pos}: none there, ours {ours:?}"),
        }
    }
}

/// A help viewer navigates by replacing the document it shows: the callback
/// runs with no borrow of the editor held, so it may.
#[test]
fn enter_may_follow_a_link_into_a_new_page_of_the_same_document() {
    let document = doc(SEE_BACKUPS);
    let page = document.clone();
    let editor = RichTextEditor::read_only(document.clone()).on_link_activated(move |_, _| {
        page.set_plain_text("the backups page").unwrap();
    });
    let mut m = Mounted::new(editor);
    m.caret_at(6);
    m.press(Key::Enter, Modifiers::NONE);
    m.settle();
    assert_eq!(m.handle.to_plain_text(), "the backups page");
}

// --- An editor that follows no link ------------------------------------------

/// With no `on_link_activated`, a viewer's links are no Tab stops: nothing
/// would follow them.
#[test]
fn a_viewer_that_follows_no_link_has_no_link_stops() {
    let mut m = Mounted::new(RichTextEditor::read_only(doc(SEE_BACKUPS)));
    m.caret_at(0);
    let heard = m.press(Key::Tab, Modifiers::NONE);
    assert!(left(&heard, "After"), "Tab leaves at once: {heard:?}");
}

/// …nor does Enter on one do anything…
#[test]
fn enter_on_a_link_of_a_viewer_that_follows_no_link_changes_nothing() {
    let mut m = Mounted::new(RichTextEditor::read_only(doc(SEE_BACKUPS)));
    m.caret_at(6);
    let heard = m.press(Key::Enter, Modifiers::NONE);
    assert!(heard.is_empty(), "{heard:?}");
    assert_eq!(m.handle.selection(), (6, 6));
}

/// …and its link nodes, still links a reader finds, advertise no Click.
#[test]
fn a_viewer_that_follows_no_link_advertises_no_click() {
    let m = Mounted::new(RichTextEditor::read_only(doc(SEE_BACKUPS)));
    assert!(m.reader.finds(Role::Link, "backups"));
    assert_eq!(link_clicks(&m, "backups"), Some(false));
}

/// A viewer of a document another view composes in shows no composition, so
/// its links are read off text that is shorter than the document by the
/// composition: the stop Tab selects is mapped back to the document.
#[test]
fn a_link_stop_is_a_document_range_while_another_view_composes() {
    use teksilo_core::event::WidgetEvent;
    let document = doc(SEE_BACKUPS);
    let editor = RichTextEditor::editor(document.clone());
    let editor_handle = editor.handle();
    let preview = viewer(document.clone());
    let preview_state = preview.state.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let top = tree.add(
        crate::primitives::FixedSize::new()
            .height(150.0)
            .child(editor),
    );
    tree.add(crate::primitives::VStack::new().child(top).child(preview));
    settle(&mut tree);
    let focused = tree.first_focusable_descendant(top).expect("the editor");
    tree.focus(focused);
    settle(&mut tree);
    editor_handle.select_range(0, 0);
    settle(&mut tree);
    tree.dispatch_event(WidgetEvent::ime_composition("\u{307b}\u{3052}", None));
    settle(&mut tree);
    assert!(
        document
            .to_plain_text()
            .unwrap_or_default()
            .starts_with("\u{307b}\u{3052}"),
        "setup: the composition is in the document"
    );
    let stop =
        super::keyboard::link_tab_stop(&preview_state.borrow(), false).expect("the preview's link");
    assert_eq!(
        stop,
        (6, 13),
        "the link's document range, past the composition"
    );
}

// --- Geometry, typing, cost --------------------------------------------------

/// A link in a table cell (a sibling of the cell's runs) has the box of its
/// text, as every node a reader can land on must.
#[test]
fn a_link_in_a_table_cell_has_a_box() {
    use accesskit_consumer::common_filter;
    let m = Mounted::new(viewer(doc("| [three](a:3) | x |\n|---|---|\n| y | z |")));
    fn walk(node: accesskit_consumer::NodeRef<'_>) -> Option<teksilo_core::accesskit::Rect> {
        if node.role() == Role::Link {
            return node.bounding_box();
        }
        node.filtered_children(&common_filter).find_map(walk)
    }
    let rect = walk(m.reader.platform().state().root()).expect("the cell's link has a box");
    assert!(
        rect.x1 - rect.x0 > 1.0,
        "a box as wide as its text: {rect:?}"
    );
}

/// Typing inside a link in an editable editor: the changed run's text
/// change is raised on the link's text container, which supports text
/// ranges, so the platform still carries it.
#[test]
fn typing_inside_a_link_still_raises_a_text_change() {
    use crate::common::heard_test::{Uia, UiaRecorder};
    let mut m = Mounted::new(RichTextEditor::editor(doc(SEE_BACKUPS)));
    m.caret_at(6);
    let mut adapter = UiaRecorder::attach(&mut m.tree);
    let focused = m.tree.focused().expect("the editor is focused");
    m.tree.type_text(focused, "x");
    m.settle();
    let events = adapter.frame(&mut m.tree);
    assert!(
        events.contains(&Uia::TextChanged(Role::Label)),
        "the link's container carries the change: {events:?}"
    );
    assert_eq!(m.handle.to_plain_text(), "see baxckups for more");
}

/// What the walk and a Tab press cost on a document with many links. A
/// measurement to read, not a gate: `cargo test -p teksilo-widgets --lib
/// links_cost -- --ignored --nocapture`.
#[test]
#[ignore = "measurement, prints the cost of the walk and of Tab with 2000 links"]
fn links_cost() {
    let mut markdown = String::new();
    for paragraph in 0..200 {
        for link in 0..10 {
            markdown.push_str(&format!(
                "word [link {paragraph} {link}](a:{paragraph}-{link}) "
            ));
        }
        markdown.push_str("\n\n");
    }
    let mut m = Mounted::new(viewer(doc(&markdown)));
    let start = std::time::Instant::now();
    m.tree.request_accessibility_update();
    let _ = m.tree.sync_accessibility();
    eprintln!("walk with 2000 links: {:?}", start.elapsed());
    m.caret_at(0);
    let start = std::time::Instant::now();
    for _ in 0..100 {
        m.tree.press_key(Key::Tab, Modifiers::NONE);
    }
    eprintln!("100 Tab presses: {:?}", start.elapsed());
}

/// The document's text selection as the Windows adapter holds it, as the
/// characters it covers.
fn document_selection(adapter: &crate::common::heard_test::UiaRecorder) -> Option<(usize, usize)> {
    fn find(node: accesskit_consumer::NodeRef<'_>) -> Option<accesskit_consumer::NodeRef<'_>> {
        if node.role() == Role::Document {
            return Some(node);
        }
        node.filtered_children(&accesskit_consumer::common_filter)
            .find_map(find)
    }
    let document = find(adapter.platform().state().root())?;
    let range = document.text_selection()?;
    Some((
        range.start().to_global_usv_index(),
        range.end().to_global_usv_index(),
    ))
}

/// The first Tab from the document onto a link moves the reader's focus
/// onto the link in an update of its own, and the document's selection
/// follows in the next. With both in one update the Windows adapter raised
/// `TextSelectionChanged` on the document before the focus event, and NVDA,
/// its focus still on the document, said "backups selected" before
/// "backups, link" (Windows re-check 2 at 758e0f8c). From a link to the next,
/// and back to the document, the events are as they were.
#[test]
fn a_tab_from_the_document_moves_the_readers_focus_before_the_selection() {
    use crate::common::heard_test::{Uia, UiaRecorder};
    let mut m = Mounted::new(viewer(doc(
        "see [backups](help:backups) or [notes](help:notes)",
    )));
    m.caret_at(0);
    let mut adapter = UiaRecorder::attach(&mut m.tree);
    // One frame: the loop's turn, then the update handed to the adapter.
    fn frame(m: &mut Mounted, adapter: &mut UiaRecorder) -> Vec<Uia> {
        m.settle();
        adapter.frame(&mut m.tree)
    }

    // A frame nobody else asked for: the hold must ask for the one that
    // publishes the selection, or an idle application keeps it held.
    fn unasked_frame(m: &mut Mounted, adapter: &mut UiaRecorder) -> Vec<Uia> {
        m.tree.tick_animations(std::time::Duration::from_millis(16));
        m.tree.layout(SizeProposal::exact(600.0, 400.0));
        let _ = m.tree.render();
        adapter.frame(&mut m.tree)
    }

    m.tree.press_key(Key::Tab, Modifiers::NONE);
    assert_eq!(frame(&mut m, &mut adapter), [Uia::Focus(Role::Link)]);
    assert!(m.tree.frame_requested(), "the hold asks for a frame");
    assert_eq!(
        unasked_frame(&mut m, &mut adapter),
        [Uia::TextSelectionChanged(Role::Document)],
        "the selection follows in the next update"
    );
    assert_eq!(frame(&mut m, &mut adapter), []);
    assert_eq!(document_selection(&adapter), Some((4, 11)));
    assert_eq!(
        m.selection(),
        (4, 11),
        "the editor selected the link at once"
    );

    // Link to link: the reader's focus is already on a link, which NVDA
    // holds as its focus, so the document's selection moves with it.
    m.tree.press_key(Key::Tab, Modifiers::NONE);
    assert_eq!(
        frame(&mut m, &mut adapter),
        [
            Uia::TextSelectionChanged(Role::Document),
            Uia::Focus(Role::Link)
        ]
    );
    assert_eq!(frame(&mut m, &mut adapter), []);
    assert_eq!(document_selection(&adapter), Some((15, 20)));

    // Escape gives focus back to the document, the link still selected.
    m.tree.press_key(Key::Escape, Modifiers::NONE);
    assert_eq!(frame(&mut m, &mut adapter), [Uia::Focus(Role::Document)]);
    assert_eq!(document_selection(&adapter), Some((15, 20)));

    // Shift+Tab from there is again a Tab from the document.
    m.tree.press_key(Key::Tab, Modifiers::SHIFT);
    assert_eq!(frame(&mut m, &mut adapter), [Uia::Focus(Role::Link)]);
    assert_eq!(
        frame(&mut m, &mut adapter),
        [Uia::TextSelectionChanged(Role::Document)]
    );
    assert_eq!(document_selection(&adapter), Some((4, 11)));

    // Enter follows it and gives focus back to the document.
    m.tree.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(frame(&mut m, &mut adapter), [Uia::Focus(Role::Document)]);
}

/// A link Tab or Shift+Tab selects is shown whole when it fits: a viewer
/// scrolling itself up to a link that wraps showed only its last line, where
/// the caret is, and the first was cut at the top (Windows re-check 2 at
/// 758e0f8c, about 7 px of "bold link"). Down to one, its last line shows.
#[test]
fn a_link_stop_that_wraps_is_scrolled_into_view_whole() {
    use crate::button::Button;
    use crate::primitives::{FixedSize, VStack};
    use teksilo_i18n::lit;
    use teksilo_text::CursorAffinity;

    let mut markdown = String::new();
    for i in 0..20 {
        markdown.push_str(&format!("filler paragraph {i}\n\n"));
    }
    markdown.push_str("[a link long enough to wrap over several lines here](a:b)\n\n");
    for i in 0..20 {
        markdown.push_str(&format!("more filler {i}\n\n"));
    }
    let editor = viewer(doc(&markdown)).v_scroll_policy(super::ScrollPolicy::Auto);
    let handle = editor.handle();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(
        VStack::new()
            .child(Button::new(lit!("Before")))
            .child(FixedSize::new().width(160.0).height(120.0).child(editor))
            .child(Button::new(lit!("After"))),
    );
    tree.layout(SizeProposal::exact(600.0, 400.0));
    let _ = tree.render();
    tree.press_key(Key::Tab, Modifiers::NONE);
    tree.press_key(Key::Tab, Modifiers::NONE);
    settle(&mut tree);

    // The link's two ends, as the engine places them in the viewport.
    let ends = |handle: &EditorHandle| {
        let st = handle.state.borrow();
        let (start, end) = st.link_stop.expect("a link stop");
        let top = st.engine.caret_rect(start, CursorAffinity::Downstream);
        let bottom = st.engine.caret_rect(end, CursorAffinity::Downstream);
        assert!(bottom[1] > top[1], "the link wraps: {top:?} {bottom:?}");
        (top[1], bottom[1] + bottom[3], st.viewport_height)
    };

    // Ctrl+End scrolls to the end, Ctrl+Home back to the top.
    tree.press_key(Key::End, Modifiers::COMMAND);
    settle(&mut tree);
    tree.press_key(Key::Tab, Modifiers::SHIFT);
    settle(&mut tree);
    let (top, bottom, height) = ends(&handle);
    assert!(
        top >= 0.0 && bottom <= height,
        "up to the link, all of it shows: {top}..{bottom} in 0..{height}"
    );

    tree.press_key(Key::Home, Modifiers::COMMAND);
    settle(&mut tree);
    tree.press_key(Key::Tab, Modifiers::NONE);
    settle(&mut tree);
    let (top, bottom, height) = ends(&handle);
    assert!(
        top >= 0.0 && bottom <= height,
        "down to the link, all of it shows: {top}..{bottom} in 0..{height}"
    );
}

/// The same in a page: a viewer sized to its text inside a `ScrollArea` is
/// scrolled by the page, and a link Tab or Shift+Tab selects is revealed
/// there whole, not only the caret's line at its end.
#[test]
fn a_link_stop_that_wraps_is_scrolled_into_the_page_whole() {
    use crate::ScrollArea;
    use crate::primitives::{FixedSize, VStack};

    let mut markdown = String::new();
    for i in 0..20 {
        markdown.push_str(&format!("filler paragraph {i}\n\n"));
    }
    markdown.push_str("[a link long enough to wrap over several lines here](a:b)\n\n");
    for i in 0..20 {
        markdown.push_str(&format!("more filler {i}\n\n"));
    }
    let editor = viewer(doc(&markdown))
        .v_scroll_policy(super::ScrollPolicy::AlwaysOff)
        .min_lines(1);
    let handle = editor.handle();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let page = tree.add(VStack::new().child(FixedSize::new().width(160.0).child(editor)));
    let outer = ScrollArea::from_id(page).smooth_scrolling(false);
    let outer_y = outer.scroll_y_signal().clone();
    let outer_id = tree.add(outer);
    let size = SizeProposal::exact(160.0, 120.0);
    let frame = |tree: &mut WidgetTree| {
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        tree.layout(size);
        let _ = tree.render();
    };
    for _ in 0..3 {
        frame(&mut tree);
    }
    tree.press_key(Key::Tab, Modifiers::NONE);
    frame(&mut tree);

    // The link's first and last lines in the window, against the page's
    // viewport.
    let ends = |tree: &WidgetTree, handle: &EditorHandle| {
        let st = handle.state.borrow();
        let (start, end) = st.link_stop.expect("a link stop");
        let top = super::keyboard::range_window_rect(&st, start, start).unwrap();
        let bottom = super::keyboard::range_window_rect(&st, end, end).unwrap();
        assert!(bottom.y > top.y, "the link wraps: {top:?} {bottom:?}");
        let page = tree.bounds(outer_id);
        (
            top.y,
            bottom.y + bottom.height,
            page.y,
            page.y + page.height,
        )
    };

    tree.press_key(Key::End, Modifiers::COMMAND);
    frame(&mut tree);
    assert!(outer_y.get() > 0.0, "Ctrl+End scrolled the page");
    tree.press_key(Key::Tab, Modifiers::SHIFT);
    frame(&mut tree);
    let (top, bottom, page_top, page_bottom) = ends(&tree, &handle);
    assert!(
        top >= page_top && bottom <= page_bottom,
        "up to the link, all of it shows: {top}..{bottom} in {page_top}..{page_bottom}"
    );

    tree.press_key(Key::Home, Modifiers::COMMAND);
    frame(&mut tree);
    tree.press_key(Key::Tab, Modifiers::NONE);
    frame(&mut tree);
    let (top, bottom, page_top, page_bottom) = ends(&tree, &handle);
    assert!(
        top >= page_top && bottom <= page_bottom,
        "down to the link, all of it shows: {top}..{bottom} in {page_top}..{page_bottom}"
    );
}

/// A page holding `editor` under `above` dp of filler, 160 wide and `height`
/// tall, its first frames run and focus Tabbed into the editor.
struct Page {
    tree: WidgetTree,
    handle: EditorHandle,
    outer_y: teksilo_core::Signal<f32>,
    outer_id: teksilo_core::widget_id::WidgetId,
    editor_id: teksilo_core::widget_id::WidgetId,
    height: f32,
}

impl Page {
    fn new(editor: RichTextEditor, editor_height: Option<f32>, above: f32, height: f32) -> Self {
        use crate::ScrollArea;
        use crate::primitives::{FixedSize, TextWidget, VStack};
        use teksilo_i18n::lit;
        let handle = editor.handle();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let mut sized = FixedSize::new().width(160.0);
        if let Some(h) = editor_height {
            sized = sized.height(h);
        }
        let editor_id = tree.add(sized.child(editor));
        let filler = || {
            FixedSize::new()
                .width(160.0)
                .height(above)
                .child(TextWidget::new(lit!("")))
        };
        let page = tree.add(
            VStack::new()
                .child(filler())
                .child(editor_id)
                .child(filler()),
        );
        let outer = ScrollArea::from_id(page).smooth_scrolling(false);
        let outer_y = outer.scroll_y_signal().clone();
        let outer_id = tree.add(outer);
        let mut page = Self {
            tree,
            handle,
            outer_y,
            outer_id,
            editor_id,
            height,
        };
        for _ in 0..3 {
            page.frame();
        }
        page.press(Key::Tab, Modifiers::NONE);
        page
    }

    fn frame(&mut self) {
        self.tree.request_frame();
        self.tree
            .tick_animations(std::time::Duration::from_millis(16));
        self.tree.layout(SizeProposal::exact(160.0, self.height));
        let _ = self.tree.render();
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.press_key(key, modifiers);
        self.frame();
    }

    /// The top of the link stop's first line and the bottom of its last, in
    /// the window.
    fn link_lines(&self) -> (f32, f32) {
        let st = self.handle.state.borrow();
        let (start, end) = st.link_stop.expect("a link stop");
        let top = super::keyboard::range_window_rect(&st, start, start).unwrap();
        let bottom = super::keyboard::range_window_rect(&st, end, end).unwrap();
        (top.y, bottom.y + bottom.height)
    }

    /// The page's viewport in the window, top and bottom.
    fn page(&self) -> (f32, f32) {
        let page = self.tree.bounds(self.outer_id);
        (page.y, page.y + page.height)
    }

    /// The caret's line in the window, against the page's viewport.
    fn caret_shows(&self) -> bool {
        let st = self.handle.state.borrow();
        let caret = super::keyboard::caret_window_rect(&st).expect("a caret");
        let page = self.tree.bounds(self.outer_id);
        caret.y >= page.y - 0.5 && caret.y + caret.height <= page.y + page.height + 0.5
    }
}

fn long_link_markdown(words: usize) -> String {
    let mut markdown = String::new();
    for i in 0..20 {
        markdown.push_str(&format!("filler paragraph {i}\n\n"));
    }
    let text = vec!["word"; words].join(" ");
    markdown.push_str(&format!("[{text}](a:b)\n\n"));
    for i in 0..20 {
        markdown.push_str(&format!("more filler {i}\n\n"));
    }
    markdown
}

/// A link taller than the page that shows the viewer keeps the caret, at
/// the link's end, in view whichever way the Tab went: the page aligns the
/// top of a target that comes from above, so the whole link would have left
/// the caret below it.
#[test]
fn a_link_taller_than_the_page_keeps_the_caret_in_view() {
    let editor = viewer(doc(&long_link_markdown(40)))
        .v_scroll_policy(super::ScrollPolicy::AlwaysOff)
        .min_lines(1);
    let mut p = Page::new(editor, None, 0.0, 120.0);

    p.press(Key::End, Modifiers::COMMAND);
    assert!(p.outer_y.get() > 0.0, "Ctrl+End scrolled the page");
    p.press(Key::Tab, Modifiers::SHIFT);
    {
        let st = p.handle.state.borrow();
        let (start, end) = st.link_stop.expect("a link stop");
        let top = super::keyboard::range_window_rect(&st, start, start).unwrap();
        let bottom = super::keyboard::range_window_rect(&st, end, end).unwrap();
        assert!(
            bottom.y + bottom.height - top.y > 120.0,
            "the link is taller than the page: {top:?} {bottom:?}"
        );
    }
    assert!(p.caret_shows(), "up to the link, the caret shows");

    p.press(Key::Home, Modifiers::COMMAND);
    p.press(Key::Tab, Modifiers::NONE);
    assert!(p.caret_shows(), "down to the link, the caret shows");
}

/// A viewer with a viewport of its own inside a page: a link taller than
/// that viewport is revealed inside it, and the page reveal takes only what
/// the viewer shows. Revealing the link's lines the viewer had scrolled out
/// of its own view scrolled the page up to the filler above the viewer.
#[test]
fn a_tall_link_in_an_embedded_viewer_does_not_scroll_the_page_above_it() {
    let editor = viewer(doc(&long_link_markdown(24))).v_scroll_policy(super::ScrollPolicy::Auto);
    let mut p = Page::new(editor, Some(60.0), 200.0, 200.0);
    // The page shows the viewer at its top.
    p.outer_y.set(200.0);
    p.frame();
    let editor_top = p.tree.bounds(p.editor_id).y;
    let page_top = p.tree.bounds(p.outer_id).y;
    assert!(
        (editor_top - page_top).abs() < 0.5,
        "{editor_top} {page_top}"
    );

    p.press(Key::Tab, Modifiers::NONE);
    let (top, bottom) = p.link_lines();
    assert!(
        bottom - top > 60.0,
        "the link is taller than the viewer: {top}..{bottom}"
    );
    assert!(
        (p.outer_y.get() - 200.0).abs() < 0.5,
        "the page did not move: {}",
        p.outer_y.get()
    );
    assert!(p.caret_shows(), "the caret shows");
}

/// A viewer whose end shows at the top of a page, a footer below it: Shift+Tab
/// up to a link above the page reveals all of it, since it fits the page.
/// The fit was measured against the part of the viewer on screen, so the
/// page showed only the caret's line and the link's first lines stayed above.
#[test]
fn a_link_that_fits_the_page_shows_whole_from_a_partly_visible_viewer() {
    let mut markdown = String::new();
    for i in 0..10 {
        markdown.push_str(&format!("filler paragraph {i}\n\n"));
    }
    markdown.push_str("[a link long enough to wrap over several lines here](a:b)\n\n");
    for i in 0..4 {
        markdown.push_str(&format!("more {i}\n\n"));
    }
    let editor = viewer(doc(&markdown))
        .v_scroll_policy(super::ScrollPolicy::AlwaysOff)
        .min_lines(1);
    let mut p = Page::new(editor, None, 400.0, 200.0);
    // The viewer's last 40 dp show at the page's top, the footer below.
    let editor = p.tree.bounds(p.editor_id);
    let (page_top, _) = p.page();
    let editor_bottom = editor.y + editor.height;
    p.outer_y
        .set(p.outer_y.get() + editor_bottom - page_top - 40.0);
    p.frame();
    let editor = p.tree.bounds(p.editor_id);
    let (page_top, page_bottom) = p.page();
    let editor_bottom = editor.y + editor.height;
    assert!(
        editor_bottom > page_top && editor_bottom < page_top + 60.0 && editor.y < page_top,
        "only the viewer's end shows: {editor:?} in {page_top}..{page_bottom}"
    );
    let length = p.handle.to_plain_text().chars().count();
    p.handle.select_range(length, length);
    p.frame();

    p.press(Key::Tab, Modifiers::SHIFT);
    let (top, bottom) = p.link_lines();
    let (page_top, page_bottom) = p.page();
    assert!(
        top >= page_top - 0.5 && bottom <= page_bottom + 0.5,
        "the link shows whole: {top}..{bottom} in {page_top}..{page_bottom}"
    );
    assert!(p.caret_shows(), "the caret shows");
}
