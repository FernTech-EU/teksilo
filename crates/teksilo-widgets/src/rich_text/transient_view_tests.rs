// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Two views of one document: an editor an input method composes in, and a
//! read-only preview beside it.
//!
//! The composition is shown as transient edits, which the document announces
//! as it does any edit. It belongs to the view the writer is typing in: the
//! preview lays out, draws and reads out the recorded document, as a revert
//! would leave it, and shows its own selection on that text, until the
//! composition is committed or cancelled.

use accesskit_consumer::{NodeRef, Tree};
use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::accessibility::widget_id_to_node_id;
use teksilo_core::accesskit::{Action, ActionData, Role, TextSelection, TreeUpdate};
use teksilo_core::event::WidgetEvent;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::CursorAffinity;
use teksilo_text::text_document::TextDocument;

use super::state::SharedState;
use super::{EditorHandle, RichTextEditor};
use crate::common::ime_ops_test::ImeOps;

const TEXT: &str = "hello world";

fn viewport() -> SizeProposal {
    SizeProposal::exact(480.0, 400.0)
}

struct TwoViews {
    tree: WidgetTree,
    doc: TextDocument,
    editor: SharedState,
    preview: SharedState,
    preview_handle: EditorHandle,
    preview_id: WidgetId,
    ops: ImeOps,
}

impl TwoViews {
    /// An editor and a read-only preview of one document holding [`TEXT`],
    /// laid out one above the other, the editor focused with its caret at
    /// `caret`.
    fn new(caret: usize) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(TEXT).unwrap();
        doc.clear_undo_redo();
        let editor = RichTextEditor::editor(doc.clone());
        let editor_state = editor.state.clone();
        let editor_handle = editor.handle();
        let preview = RichTextEditor::read_only(doc.clone());
        let preview_state = preview.state.clone();
        let preview_handle = preview.handle();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let top = tree.add(
            crate::primitives::FixedSize::new()
                .height(150.0)
                .child(editor),
        );
        let preview_id = tree.add(preview);
        let bottom = tree.add(
            crate::primitives::FixedSize::new()
                .height(150.0)
                .child(preview_id),
        );
        tree.add(crate::primitives::VStack::new().child(top).child(bottom));
        tree.layout(viewport());
        let _ = tree.render();
        let focused = tree
            .first_focusable_descendant(top)
            .expect("the editor is focusable");
        tree.focus(focused);
        let mut views = Self {
            tree,
            doc,
            editor: editor_state,
            preview: preview_state,
            preview_handle,
            preview_id,
            ops: ImeOps::default(),
        };
        views.settle();
        editor_handle.select_range(caret, caret);
        views.settle();
        views
    }

    /// Run the frames the views lay out and render in.
    fn settle(&mut self) {
        for _ in 0..3 {
            self.tree.request_frame();
            self.tree
                .tick_animations(std::time::Duration::from_millis(16));
            self.tree.layout(viewport());
        }
        let _ = self.tree.render();
    }

    fn compose(&mut self, text: &str) {
        self.tree.dispatch_event_with_ops(
            WidgetEvent::ImeComposition {
                text: text.to_string(),
                cursor: None,
                replacement: None,
            },
            &mut self.ops,
        );
        self.settle();
    }

    /// What winit sends for a commit: an empty update, then the text.
    fn commit(&mut self, text: &str) {
        self.tree.dispatch_event_with_ops(
            WidgetEvent::ImeComposition {
                text: String::new(),
                cursor: None,
                replacement: None,
            },
            &mut self.ops,
        );
        self.tree
            .dispatch_event_with_ops(WidgetEvent::ime_commit(text), &mut self.ops);
        self.settle();
    }

    /// What an input method's cancel is: an empty update, and nothing after it
    /// before the next frame.
    fn cancel(&mut self) {
        self.compose("");
        self.settle();
    }

    fn update(&mut self) -> TreeUpdate {
        self.settle();
        self.tree.sync_accessibility()
    }

    /// Each view's text as a screen reader reads it: the editor's, then the
    /// preview's.
    fn read_out(&mut self) -> (String, String) {
        let update = self.update();
        (
            with_view(&update, Role::MultilineTextInput, |node| {
                node.document_range().text()
            }),
            with_view(&update, Role::Document, |node| node.document_range().text()),
        )
    }

    /// The preview's selection as a screen reader is told it, in its offsets.
    fn preview_selection_read_out(&mut self) -> (usize, usize) {
        let update = self.update();
        with_view(&update, Role::Document, |node| {
            let anchor = node
                .text_selection_anchor()
                .expect("the preview reports a selection");
            let focus = node
                .text_selection_focus()
                .expect("the preview reports a selection");
            (anchor.to_global_usv_index(), focus.to_global_usv_index())
        })
    }

    /// Where the line of `state`'s layout ends: the position its last
    /// character's trailing edge stands at, read off the layout itself.
    fn laid_out_line_end(state: &SharedState) -> usize {
        let st = state.borrow();
        let caret = st.engine.caret_rect(0, CursorAffinity::Downstream);
        let hit = st
            .engine
            .hit_test(st.engine.layout_width() - 1.0, caret[1] + caret[3] / 2.0)
            .expect("the line is laid out");
        hit.position
    }

    /// Where a click in the preview at the caret boundary before `shown`, one
    /// of the preview's own offsets, lands in the document.
    fn preview_hit_at(&self, shown: usize) -> usize {
        let st = self.preview.borrow();
        let caret = st.engine.caret_rect(shown, CursorAffinity::Downstream);
        st.hit_test(Point::new(caret[0] + 0.5, caret[1] + caret[3] / 2.0))
            .expect("the preview is laid out")
            .position
    }
}

fn with_view<R>(update: &TreeUpdate, role: Role, read: impl FnOnce(&NodeRef<'_>) -> R) -> R {
    let tree = Tree::new(update.clone(), true);
    let state = tree.state();
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if node.role() == role {
            return read(&node);
        }
        stack.extend(node.children());
    }
    panic!("no {role:?} node in the tree");
}

#[test]
fn a_composition_shows_in_the_editor_and_not_in_the_preview() {
    let mut v = TwoViews::new(5);
    for state in ["n", "ni", "nihao"] {
        v.compose(state);
    }
    assert_eq!(v.doc.to_plain_text().unwrap(), "hellonihao world");
    assert_eq!(
        v.read_out(),
        (
            "hellonihao world\n".to_string(),
            "hello world\n".to_string()
        )
    );
    assert_eq!(TwoViews::laid_out_line_end(&v.editor), 16);
    assert_eq!(TwoViews::laid_out_line_end(&v.preview), 11);
}

#[test]
fn both_views_agree_once_the_composition_is_committed() {
    let mut v = TwoViews::new(5);
    v.compose("ni");
    v.commit("你");
    assert_eq!(v.doc.to_plain_text().unwrap(), "hello你 world");
    let (editor, preview) = v.read_out();
    assert_eq!(editor, "hello你 world\n");
    assert_eq!(preview, editor);
    assert_eq!(TwoViews::laid_out_line_end(&v.editor), 12);
    assert_eq!(TwoViews::laid_out_line_end(&v.preview), 12);
}

#[test]
fn both_views_agree_once_the_composition_is_cancelled() {
    let mut v = TwoViews::new(5);
    v.compose("ni");
    v.cancel();
    assert!(!v.doc.has_transient_edits());
    assert_eq!(v.doc.to_plain_text().unwrap(), TEXT);
    let (editor, preview) = v.read_out();
    assert_eq!(editor, "hello world\n");
    assert_eq!(preview, editor);
    assert_eq!(TwoViews::laid_out_line_end(&v.editor), 11);
    assert_eq!(TwoViews::laid_out_line_end(&v.preview), 11);
}

#[test]
fn the_preview_shows_its_selection_on_the_text_it_shows() {
    let mut v = TwoViews::new(2);
    // "world", selected in the preview.
    v.preview_handle.select_range(6, 11);
    v.settle();
    v.compose("XYZ");
    {
        let preview = v.preview.borrow();
        assert_eq!(
            (preview.cursor.anchor(), preview.cursor.position()),
            (9, 14),
            "in the document, the composition moved the selection"
        );
    }
    let shown = v.preview.borrow_mut().cursor_display(false);
    assert_eq!((shown.anchor, shown.position), (6, 11));
    assert_eq!(v.preview_selection_read_out(), (6, 11));

    v.commit("ab");
    let shown = v.preview.borrow_mut().cursor_display(false);
    assert_eq!((shown.anchor, shown.position), (8, 13));
    assert_eq!(v.preview_selection_read_out(), (8, 13));
}

#[test]
fn what_the_preview_reads_off_its_layout_lands_on_that_text() {
    let mut v = TwoViews::new(2);
    v.compose("XYZ");
    // Before the composition a position is itself; after it, past it.
    assert_eq!(v.preview_hit_at(1), 1);
    assert_eq!(v.preview_hit_at(8), 11);

    // A screen reader selecting "or" in the preview selects it in the document.
    let update = v.update();
    let (anchor, focus) = with_view(&update, Role::Document, |node| {
        (
            node.text_position_from_global_usv_index(7)
                .expect("an offset inside the text")
                .to_raw(),
            node.text_position_from_global_usv_index(9)
                .expect("an offset inside the text")
                .to_raw(),
        )
    });
    let mut ops = teksilo_core::window::NoopWindowOps;
    assert!(v.tree.dispatch_access_action(
        widget_id_to_node_id(v.preview_id),
        Action::SetTextSelection,
        Some(ActionData::SetTextSelection(TextSelection {
            anchor,
            focus
        })),
        &mut ops,
    ));
    let preview = v.preview.borrow();
    assert_eq!(preview.cursor.selected_text().unwrap(), "or");
    assert_eq!(
        (
            preview.cursor.recorded_anchor(),
            preview.cursor.recorded_position()
        ),
        (7, 9)
    );
}

#[test]
fn the_preview_places_its_cursor_ends_as_a_revert_puts_them() {
    let mut v = TwoViews::new(0);
    v.preview_handle.select_range(2, 2);
    v.settle();
    // The input method's first step of a conversion: the syllable taken out, its
    // candidate not shown yet.
    let ime = v.doc.cursor();
    ime.set_position(2, teksilo_text::text_document::MoveMode::MoveAnchor);
    ime.set_position(3, teksilo_text::text_document::MoveMode::KeepAnchor);
    ime.remove_selected_text_transient().unwrap();
    v.settle();
    let preview = v.preview.borrow();
    // The preview's caret goes back where it stood, before the text put back;
    // a caret made at the same offset would go after it.
    assert_eq!(v.doc.recorded_position(2), 3);
    assert_eq!(preview.shown_position(preview.cursor.position()), 2);
}

// ── Two editors of one document ─────────────────────────────────────────────

struct TwoEditors {
    tree: WidgetTree,
    doc: TextDocument,
    states: [SharedState; 2],
    handles: [EditorHandle; 2],
    focusable: [WidgetId; 2],
    ops: ImeOps,
}

impl TwoEditors {
    /// Two editors of one document holding [`TEXT`], one above the other.
    fn new() -> Self {
        Self::with(TEXT, |_, editor| editor)
    }

    /// Two editors of one document holding `text`, each made by `make` from the
    /// plain editor.
    fn with(text: &str, make: impl Fn(usize, RichTextEditor) -> RichTextEditor) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        doc.clear_undo_redo();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let mut states = Vec::new();
        let mut handles = Vec::new();
        let mut boxes = Vec::new();
        for i in 0..2 {
            let editor = make(i, RichTextEditor::editor(doc.clone()));
            states.push(editor.state.clone());
            handles.push(editor.handle());
            let id = tree.add(editor);
            boxes.push(tree.add(crate::primitives::FixedSize::new().height(150.0).child(id)));
        }
        tree.add(
            crate::primitives::VStack::new()
                .child(boxes[0])
                .child(boxes[1]),
        );
        tree.layout(viewport());
        let _ = tree.render();
        let focusable = [0, 1].map(|i| {
            tree.first_focusable_descendant(boxes[i])
                .expect("the editor is focusable")
        });
        let mut editors = Self {
            tree,
            doc,
            states: [states[0].clone(), states[1].clone()],
            handles: [handles[0].clone(), handles[1].clone()],
            focusable,
            ops: ImeOps::default(),
        };
        editors.settle();
        editors
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.tree.request_frame();
            self.tree
                .tick_animations(std::time::Duration::from_millis(16));
            self.tree.layout(viewport());
        }
        let _ = self.tree.render();
    }

    /// Focus editor `which` with its caret at `caret`.
    fn focus(&mut self, which: usize, caret: usize) {
        self.tree.focus(self.focusable[which]);
        self.handles[which].select_range(caret, caret);
        self.settle();
    }

    fn ime(&mut self, event: WidgetEvent) {
        self.tree.dispatch_event_with_ops(event, &mut self.ops);
        self.settle();
    }

    fn compose(&mut self, text: &str) {
        self.ime(WidgetEvent::ImeComposition {
            text: text.to_string(),
            cursor: None,
            replacement: None,
        });
    }

    fn commit(&mut self, text: &str) {
        self.compose("");
        self.ime(WidgetEvent::ime_commit(text));
    }

    fn cancel(&mut self) {
        self.compose("");
        self.settle();
    }

    /// Each editor's text as a screen reader reads it.
    fn read_out(&mut self) -> [String; 2] {
        self.settle();
        let update = self.tree.sync_accessibility();
        let tree = Tree::new(update, true);
        // The two editors in the order they are laid out, top first.
        let mut editors = Vec::new();
        let mut stack = vec![tree.state().root()];
        while let Some(node) = stack.pop() {
            if node.role() == Role::MultilineTextInput {
                editors.push(node.document_range().text());
                continue;
            }
            let children: Vec<_> = node.children().collect();
            stack.extend(children.into_iter().rev());
        }
        assert_eq!(editors.len(), 2, "both editors are in the tree");
        [editors[0].clone(), editors[1].clone()]
    }

    fn line_ends(&self) -> [usize; 2] {
        [0, 1].map(|i| TwoViews::laid_out_line_end(&self.states[i]))
    }

    fn full_layouts(&self) -> [usize; 2] {
        [0, 1].map(|i| self.states[i].borrow().full_layouts.get())
    }
}

#[test]
fn a_composition_in_either_editor_shows_only_there() {
    for (composing, other) in [(0, 1), (1, 0)] {
        let mut e = TwoEditors::new();
        e.focus(composing, 5);
        e.compose("ni");
        let read = e.read_out();
        assert_eq!(read[composing], "helloni world\n", "editor {composing}");
        assert_eq!(read[other], "hello world\n", "editor {other}");
        let ends = e.line_ends();
        assert_eq!((ends[composing], ends[other]), (13, 11));
        // A full layout of the other editor, a resize say, lays out the same.
        e.states[other].borrow_mut().needs_full_layout = true;
        e.settle();
        assert_eq!(e.line_ends()[other], 11, "editor {other}, laid out again");
    }
}

#[test]
fn two_editors_agree_once_a_composition_is_committed_or_cancelled() {
    for commit in [true, false] {
        let mut e = TwoEditors::new();
        e.focus(1, 5);
        e.compose("ni");
        if commit {
            e.commit("你");
        } else {
            e.cancel();
        }
        let expected = if commit {
            "hello你 world\n"
        } else {
            "hello world\n"
        };
        let read = e.read_out();
        assert_eq!(read, [expected.to_string(), expected.to_string()]);
        let ends = e.line_ends();
        assert_eq!(ends[0], ends[1]);
        assert!(!e.doc.has_transient_edits());
    }
}

#[test]
fn the_editor_not_composing_shows_its_selection_on_the_recorded_text() {
    let mut e = TwoEditors::new();
    e.handles[0].select_range(6, 11);
    e.focus(1, 2);
    e.compose("XYZ");
    let mut other = e.states[0].borrow_mut();
    assert_eq!(
        (other.cursor.anchor(), other.cursor.position()),
        (9, 14),
        "in the document, the composition moved the selection"
    );
    let shown = other.cursor_display(false);
    assert_eq!((shown.anchor, shown.position), (6, 11));
    drop(other);
    let composing = e.states[1].borrow_mut().cursor_display(false);
    assert_eq!(composing.position, 5, "the composing editor shows its own");
}

#[test]
fn a_composition_lays_nothing_out_in_full_as_it_starts_updates_or_ends() {
    let mut e = TwoEditors::new();
    e.focus(0, 5);
    let before = e.full_layouts();
    for state in ["n", "ni", "nih", "niha", "nihao"] {
        e.compose(state);
    }
    e.commit("你好");
    assert_eq!(e.full_layouts(), before);
    assert_eq!(
        e.read_out(),
        [
            "hello你好 world\n".to_string(),
            "hello你好 world\n".to_string()
        ]
    );
}

#[test]
fn a_layer_made_after_a_write_took_the_composition_away_is_not_the_editors() {
    let mut e = TwoEditors::new();
    e.focus(0, 5);
    e.compose("ni");
    // Before the next frame, the application writes, which reverts the
    // composition, then shows a suggestion of its own as a transient edit.
    e.doc.cursor_at(0).insert_text("A").unwrap();
    e.doc.cursor_at(1).insert_text_transient("sug").unwrap();
    e.settle();
    assert!(!e.states[0].borrow().composition.is_held(&e.doc));
    let read = e.read_out();
    assert_eq!(
        read,
        ["Ahello world\n".to_string(), "Ahello world\n".to_string()]
    );
    assert_eq!(e.line_ends(), [12, 12]);
}

#[test]
fn the_host_override_shows_every_transient_edit() {
    let mut e = TwoEditors::with(TEXT, |i, editor| editor.show_transient_edits(i == 1));
    // Another editor's composition.
    e.focus(0, 5);
    e.compose("ni");
    assert_eq!(e.line_ends(), [13, 13]);
    e.commit("你");
    // A suggestion the host shows itself.
    e.doc.cursor_at(0).insert_text_transient("sug ").unwrap();
    e.settle();
    let read = e.read_out();
    assert_eq!(read[0], "hello你 world\n");
    assert_eq!(read[1], "sug hello你 world\n");
}

#[test]
fn a_composition_across_paragraphs_shows_only_where_it_is_made() {
    let mut e = TwoEditors::with("First\nSecond", |_, editor| editor);
    e.focus(0, 3);
    // An input method marking "st\nSe" and showing "X" in its place.
    e.ime(WidgetEvent::ImeComposition {
        text: "X".to_string(),
        cursor: None,
        replacement: Some(3..8),
    });
    assert_eq!(e.doc.to_plain_text().unwrap(), "FirXcond");
    let read = e.read_out();
    assert_eq!(read[0], "FirXcond\n");
    assert_eq!(read[1], "First\nSecond\n");
    e.cancel();
    assert_eq!(
        e.read_out(),
        ["First\nSecond\n".to_string(), "First\nSecond\n".to_string()]
    );
}

#[test]
fn the_keyboard_moves_a_caret_over_transient_edits_it_does_not_show() {
    let mut e = TwoEditors::with("hello world\nsecond line", |_, editor| editor);
    e.focus(1, 0);
    // A suggestion the host shows itself, which no editor here shows.
    e.doc.cursor_at(6).insert_text_transient("XYZ").unwrap();
    e.settle();
    e.tree.dispatch_event(WidgetEvent::KeyDown {
        key: teksilo_core::event::Key::ArrowDown,
        modifiers: teksilo_core::event::Modifiers::NONE,
        text: None,
    });
    e.settle();
    let st = e.states[1].borrow();
    assert_eq!(
        st.cursor.recorded_position(),
        12,
        "the start of the second line shown"
    );
    assert_eq!(st.cursor.position(), 15, "which is there in the document");
    drop(st);
    e.tree.dispatch_event(WidgetEvent::KeyDown {
        key: teksilo_core::event::Key::End,
        modifiers: teksilo_core::event::Modifiers::NONE,
        text: None,
    });
    e.settle();
    assert_eq!(e.states[1].borrow().cursor.recorded_position(), 23);
}
