// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Search highlights follow the text an edit moves, in the frame the edit is
//! laid out: typing batched for the frame, and an input method's commit.
//!
//! A host refreshes its [`FindSession`] when it gets round to it (a find banner
//! on its own frame tick, which may run before or after the editor's). The
//! boxes must already be on the matched words when the editor lays the edit
//! out, before the host has looked.

use teksilo_canvas::SizeProposal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{
    Color, FindOptions, FlowElementSnapshot, HighlightFormat, HighlightMask, TextDocument,
};

use super::FindSession;
use super::RichTextEditor;
use super::state::SharedState;
use crate::common::ime_ops_test::ImeOps;

const MATCH: Color = Color {
    red: 255,
    green: 214,
    blue: 0,
    alpha: 255,
};

fn boxed() -> HighlightFormat {
    HighlightFormat {
        background_color: Some(MATCH),
        ..Default::default()
    }
}

struct Searched {
    tree: WidgetTree,
    state: SharedState,
    doc: TextDocument,
    ops: ImeOps,
    find: FindSession,
}

fn viewport() -> SizeProposal {
    SizeProposal::exact(480.0, 400.0)
}

impl Searched {
    /// A focused editor over `text`, every `query` in it boxed, the caret at
    /// `caret`.
    fn new(text: &str, query: &str, caret: usize) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        let mut fs = FindSession::new(&doc, boxed(), boxed());
        fs.set_query(query, &FindOptions::default());
        let editor = RichTextEditor::editor(doc.clone());
        let handle = editor.handle();
        let state = editor.state_handle();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let editor = tree.add(editor);
        tree.layout(viewport());
        let _ = tree.render();
        let focus = tree
            .first_focusable_descendant(editor)
            .expect("the editor is focusable");
        tree.focus(focus);
        let mut s = Self {
            tree,
            state,
            doc,
            ops: ImeOps::default(),
            find: fs,
        };
        s.frame();
        handle.select_range(caret, caret);
        s.frame();
        s
    }

    /// One frame: the tick, the layout, the paint.
    fn frame(&mut self) {
        self.tree.request_frame();
        self.tree
            .tick_animations(std::time::Duration::from_millis(16));
        self.tree.layout(viewport());
        let _ = self.tree.render();
    }

    /// The boxes the layout carries, as `(line top, left, right)`, sorted.
    fn painted(&self) -> Vec<(i64, i64, i64)> {
        self.state.borrow_mut().engine.with_render_frame(|frame| {
            let mut v: Vec<(i64, i64, i64)> = frame
                .decorations
                .iter()
                .filter(|d| d.kind == teksilo_text::TypesetterDecorationKind::TextBackground)
                .map(|d| {
                    (
                        d.rect[1].round() as i64,
                        d.rect[0].round() as i64,
                        (d.rect[0] + d.rect[2]).round() as i64,
                    )
                })
                .collect();
            v.sort();
            v
        })
    }

    /// Each block's boxes in the document, as block-relative `(start, length)`.
    fn spans(&self) -> Vec<Vec<(usize, usize)>> {
        self.doc
            .snapshot_flow_masked(&HighlightMask::all())
            .elements
            .iter()
            .map(|e| match e {
                FlowElementSnapshot::Block(b) => b
                    .paint_highlights
                    .iter()
                    .filter(|s| s.background_color == Some(MATCH))
                    .map(|s| (s.start, s.length))
                    .collect(),
                _ => Vec::new(),
            })
            .collect()
    }
}

impl Searched {
    /// The boxes in the document and in the layout, before and after the host
    /// refreshes the session: each heading boxed from its first character,
    /// `first` the start of the one in the edited paragraph.
    fn assert_boxes_follow(&mut self, edited: &str, first: usize) {
        let expected = vec![vec![(first, 7)], vec![(0, 7)], vec![(0, 7)], vec![(0, 7)]];
        let fresh = painted_fresh(edited, "heading");
        assert_eq!(self.doc.to_plain_text().unwrap(), edited);
        assert_eq!(
            self.spans(),
            expected,
            "each heading is boxed from its first character, before the host refreshes"
        );
        assert_eq!(
            self.painted(),
            fresh,
            "the layout boxes each heading where it is, in the frame of the edit"
        );

        assert!(self.find.refresh_if_stale(), "the edit staled the matches");
        self.frame();
        assert_eq!(
            self.spans(),
            expected,
            "the refresh finds them where they were"
        );
        assert_eq!(self.painted(), fresh);
    }
}

/// The boxes of a fresh editor over the same text, where nothing has moved:
/// what the layout must carry after the edit.
fn painted_fresh(text: &str, query: &str) -> Vec<(i64, i64, i64)> {
    let mut s = Searched::new(text, query, 0);
    s.frame();
    s.painted()
}

/// A heading in the paragraph that is edited, and three below it.
const TEXT: &str = "Intro heading\nheading one\nheading two\nheading three";

/// Characters typed in one frame go in at the start of the document, as one
/// batch the frame inserts. Every box after them stays on its word.
#[test]
fn a_typed_burst_moves_the_boxes_with_the_text() {
    let mut s = Searched::new(TEXT, "heading", 0);
    let focused = s.tree.focused().expect("focused");
    s.tree.type_text(focused, "xyz");
    s.frame();
    s.assert_boxes_follow(&TEXT.replacen("Intro", "xyzIntro", 1), 9);
}

/// An input method's commit of one syllable near the start of the document,
/// after its composition showed it.
#[test]
fn an_ime_commit_moves_the_boxes_with_the_text() {
    use teksilo_core::event::WidgetEvent;
    let mut s = Searched::new(TEXT, "heading", 2);
    let compose = |s: &mut Searched, text: &str| {
        s.tree.dispatch_event_with_ops(
            WidgetEvent::ImeComposition {
                text: text.into(),
                cursor: None,
                replacement: None,
            },
            &mut s.ops,
        );
    };
    compose(&mut s, "\u{314E}");
    s.frame();
    compose(&mut s, "\u{D558}");
    s.frame();
    // What winit sends for a commit: the empty update, then the text.
    compose(&mut s, "");
    s.tree
        .dispatch_event_with_ops(WidgetEvent::ime_commit("\u{D558}"), &mut s.ops);
    s.frame();
    // The syllable goes in at the caret, after "In".
    let mut edited = TEXT.to_owned();
    edited.insert(2, '\u{D558}');
    s.assert_boxes_follow(&edited, 7);
}
