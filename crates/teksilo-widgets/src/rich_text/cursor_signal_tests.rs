// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The cursor signals are written after every caret event, which a drag
//! makes on each pointer move. A write that changes nothing must not notify
//! (a bound signal dirties the widget and the window lays out for it), and a
//! real change, a flipped wrap affinity included, must.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_text::CursorAffinity;
use teksilo_text::text_document::TextDocument;

use super::{RichTextEditor, sync_cursor_signals};

/// Counts the notifications of the three cursor signals.
struct Counts {
    position: Rc<Cell<u32>>,
    anchor: Rc<Cell<u32>>,
    selection: Rc<Cell<u32>>,
    // Dropping a handle detaches its observer.
    _handles: Vec<teksilo_core::signal::ObserverHandle>,
}

fn editor_with_counts() -> (RichTextEditor, Counts) {
    let doc = TextDocument::new();
    doc.set_plain_text("hello world").unwrap();
    let editor = RichTextEditor::editor(doc);
    let st = editor.state_handle();
    fn count<T>(cell: &Rc<Cell<u32>>) -> impl Fn(&T) + 'static {
        let cell = cell.clone();
        move |_: &T| cell.set(cell.get() + 1)
    }
    let counts = Counts {
        position: Rc::default(),
        anchor: Rc::default(),
        selection: Rc::default(),
        _handles: Vec::new(),
    };
    let handles = {
        let st = st.borrow();
        vec![
            st.cursor_position.observe(count(&counts.position)),
            st.cursor_anchor.observe(count(&counts.anchor)),
            st.has_selection.observe(count(&counts.selection)),
        ]
    };
    (
        editor,
        Counts {
            _handles: handles,
            ..counts
        },
    )
}

#[test]
fn an_unchanged_caret_does_not_notify() {
    let (editor, counts) = editor_with_counts();
    let state = editor.state_handle();
    sync_cursor_signals(&state);
    sync_cursor_signals(&state);
    assert_eq!(counts.position.get(), 0);
    assert_eq!(counts.anchor.get(), 0);
    assert_eq!(counts.selection.get(), 0);
}

#[test]
fn a_moved_caret_and_a_new_selection_notify_what_changed() {
    let (editor, counts) = editor_with_counts();
    let state = editor.state_handle();
    editor.handle().select_range(3, 3);
    assert_eq!(
        (
            counts.position.get(),
            counts.anchor.get(),
            counts.selection.get()
        ),
        (1, 1, 0),
        "a caret move changes the offset and the anchor, not the selection flag"
    );
    editor.handle().select_range(3, 7);
    assert_eq!(
        (
            counts.position.get(),
            counts.anchor.get(),
            counts.selection.get()
        ),
        (2, 1, 1),
        "extending from the same anchor changes the offset and the flag only"
    );
    sync_cursor_signals(&state);
    assert_eq!(counts.position.get(), 2, "a repeat changes nothing");
    assert_eq!(state.borrow().cursor_position.get(), 7);
    assert_eq!(state.borrow().cursor_anchor.get(), 3);
    assert!(state.borrow().has_selection.get());
}

#[test]
fn a_flipped_affinity_at_the_same_offset_still_notifies() {
    let (editor, counts) = editor_with_counts();
    let state = editor.state_handle();
    editor.handle().select_range(4, 4);
    let before = counts.position.get();
    state.borrow_mut().cursor_affinity = CursorAffinity::Upstream;
    sync_cursor_signals(&state);
    assert_eq!(
        counts.position.get(),
        before + 1,
        "the caret is drawn on the other side of the wrap, so it needs a repaint"
    );
    sync_cursor_signals(&state);
    assert_eq!(counts.position.get(), before + 1, "once per flip");
}
