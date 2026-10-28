// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A drag that starts at the left edge of a wrapped line's first word selects
//! from that word, in an editor and in a read-only view: never from the space
//! where the line above wraps.
//!
//! A press resolves to the start of the line it is on (the soft wrap's
//! downstream side). It also hit-tests the text as it is now: a press in a
//! preview ends the composition an input method holds in the editor beside it
//! before the preview sees the press, and the committed text moves everything
//! after it while the preview's layout still showed it without.

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::CursorAffinity;
use teksilo_text::text_document::TextDocument;

use super::RichTextEditor;
use super::state::SharedState;
use crate::common::ime_ops_test::ImeOps;

/// A short paragraph, where the input method composes, then one that wraps.
const TEXT: &str = "Showcase\n\
     The quick brown fox jumps over the lazy dog. \
     A long paragraph that absolutely positively must wrap across \
     multiple visual lines so a press at the start of a line has \
     somewhere to land.";

const WIDTH: f32 = 280.0;

fn settle(tree: &mut WidgetTree) {
    for _ in 0..3 {
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        tree.layout(SizeProposal::exact(WIDTH, 400.0));
    }
    let _ = tree.render();
}

/// The first soft wrap of `state`'s layout, in its own offsets, and the window
/// point on the left edge of the line it starts, `dx` to the right of it.
fn first_wrap(state: &SharedState, dx: f32) -> (usize, Point) {
    let st = state.borrow();
    let wrap = (1..TEXT.chars().count())
        .find(|&pos| {
            let end = st.engine.caret_rect(pos, CursorAffinity::Downstream);
            let start = st.engine.caret_rect(pos, CursorAffinity::Upstream);
            (start[1] - end[1]).abs() > 1.0
        })
        .expect("the paragraph wraps");
    let r = st.engine.caret_rect(wrap, CursorAffinity::Upstream);
    let at = Point::new(
        st.viewport_origin.x + r[0] + dx,
        st.viewport_origin.y + r[1] + r[3] / 2.0,
    );
    (wrap, at)
}

/// Press at `from`, drag 60 px to the right along the line, release.
fn drag(tree: &mut WidgetTree, ops: &mut ImeOps, from: Point) {
    tree.dispatch_event_with_ops(
        WidgetEvent::pointer_down(from, PointerButton::Primary, Modifiers::NONE),
        &mut *ops,
    );
    for i in 1..=6 {
        let at = Point::new(from.x + 10.0 * i as f32, from.y);
        tree.dispatch_event_with_ops(WidgetEvent::pointer_move(at), &mut *ops);
    }
    tree.dispatch_event_with_ops(
        WidgetEvent::pointer_up(
            Point::new(from.x + 60.0, from.y),
            PointerButton::Primary,
            Modifiers::NONE,
        ),
        &mut *ops,
    );
    settle(tree);
}

fn selected(state: &SharedState) -> String {
    state.borrow().cursor.selected_text().unwrap_or_default()
}

/// The word a wrapped line at `wrap` starts with, in [`TEXT`].
fn word_at(wrap: usize) -> String {
    TEXT.chars()
        .skip(wrap)
        .take_while(|c| !c.is_whitespace())
        .collect()
}

/// One view, nothing else going on.
#[test]
fn a_drag_from_a_wrapped_lines_left_edge_selects_from_its_first_word() {
    for read_only in [true, false] {
        for dx in [0.0, 0.5, 1.0, 2.0] {
            let doc = TextDocument::new();
            doc.set_plain_text(TEXT).unwrap();
            let view = if read_only {
                RichTextEditor::read_only(doc)
            } else {
                RichTextEditor::editor(doc)
            };
            let state = view.state_handle();
            let mut tree = WidgetTree::new();
            tree.add(view);
            settle(&mut tree);
            settle(&mut tree);

            let (wrap, from) = first_wrap(&state, dx);
            drag(&mut tree, &mut ImeOps::default(), from);
            let text = selected(&state);
            assert!(
                text.starts_with(&word_at(wrap)),
                "read_only {read_only}, dx {dx}: the selection starts at the line's first \
                 word {:?}, it is {text:?}",
                word_at(wrap)
            );
        }
    }
}

/// The preview of an editor an input method composes in (Windows, round 11).
/// The press ends the composition, keeping its text, before the preview sees
/// it; the preview's layout still shows the paragraph after it where it was.
#[test]
fn a_drag_in_a_preview_while_the_editor_composes_selects_from_the_first_word() {
    // `None` keeps what the composition showed (macOS, Linux); Windows' input
    // method commits its own text.
    for committed in [None, Some("ni"), Some("\u{4F60}")] {
        for dx in [0.0, 1.0] {
            let doc = TextDocument::new();
            doc.set_plain_text(TEXT).unwrap();
            let editor = RichTextEditor::editor(doc.clone());
            let editor_handle = editor.handle();
            let preview = RichTextEditor::read_only(doc.clone());
            let preview_state = preview.state_handle();
            let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
            let top = tree.add(
                crate::primitives::FixedSize::new()
                    .height(150.0)
                    .child(editor),
            );
            let bottom = tree.add(
                crate::primitives::FixedSize::new()
                    .height(200.0)
                    .child(preview),
            );
            tree.add(crate::primitives::VStack::new().child(top).child(bottom));
            settle(&mut tree);
            let focused = tree.first_focusable_descendant(top).expect("focusable");
            tree.focus(focused);
            settle(&mut tree);
            // The end of the first paragraph.
            editor_handle.select_range(8, 8);
            settle(&mut tree);
            let mut ops = ImeOps {
                committed: committed.map(str::to_string),
                ..ImeOps::default()
            };
            tree.dispatch_event_with_ops(
                WidgetEvent::ImeComposition {
                    text: "ni".into(),
                    cursor: None,
                    replacement: None,
                },
                &mut ops,
            );
            settle(&mut tree);
            assert!(doc.has_transient_edits(), "the editor composes");

            let (wrap, from) = first_wrap(&preview_state, dx);
            drag(&mut tree, &mut ops, from);
            assert!(
                !doc.has_transient_edits(),
                "the press ended the composition"
            );
            let text = selected(&preview_state);
            assert!(
                text.starts_with(&word_at(wrap)),
                "committed {committed:?}, dx {dx}: the selection starts at the line's first \
                 word {:?}, it is {text:?}",
                word_at(wrap)
            );
        }
    }
}
