// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Keyboard dispatch for the rich text editor.
//!
//! This module owns the full `KeyDown` / `ImeComposition` / `ImeCommit` dispatch: navigation
//! (arrows, Home/End, PageUp/Down), editing (Backspace, Delete, Enter,
//! `Ctrl+Backspace` / `Ctrl+Delete` for word‑level deletion). Undo / redo,
//! the clipboard and select-all (with its table‑aware escalation ladder) are
//! not keys: they come through the rebindable `edit.*` shortcuts and
//! [`run_standard_edit`]. Neither are bold, italic, underline and the forced
//! paragraph insert: the `format.*` shortcuts and [`run_format_shortcut`].
//!
//! All functions take `&SharedState` so they can be called from inside
//! `HandlerSet` closures without borrowing the widget struct.

use teksilo_core::event::{EventResponse, Key, ScrollMotion, WidgetEvent};
use teksilo_core::shortcut::StandardEdit;

use super::typing_format::FormatToggle;
use crate::editor_shortcuts::FormatShortcut;
use teksilo_core::widget::EventContext;
use teksilo_text::text_document::{
    BlockFormat, InsertionOrigin, ListFormat, MoveMode, MoveOperation, SelectionType, TableCellRef,
    TextCursor, TextFormat,
};
use teksilo_text::{CursorAffinity, TextDirection};

use crate::EditSource;
use crate::common::composition::position_after_commit;
use crate::common::text_nav::{self, CaretStep, LineStep};
use crate::common::wrapped_caret::{affinity_after_step, is_soft_wrap};

use super::clipboard;
use super::nesting;
use super::policy::EditCommandKind;
use super::reading_order::ReadingOrder;
use super::state::{EditorState, SharedState};
use super::sync_cursor_signals;

/// Kind of key action taken by `handle_key`, used to decide whether to
/// clear the sticky preferred-X afterwards and how to update the
/// cursor's [`CursorAffinity`] at soft-wrap boundaries.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) enum KeyAction {
    /// The key caused horizontal motion, a selection change, an edit,
    /// or anything else where the resulting caret position is not on
    /// a wrap boundary or, if it is, should render at the downstream
    /// (end-of-previous-line) placement. Clears the sticky column AND
    /// resets `cursor_affinity` to `Downstream`. Covers
    /// Ctrl+Home/Ctrl+End, Enter/typing, paste and the Shift+Arrow
    /// cell-selection branch, etc. (the plain Left/Right arrows, Backspace
    /// and Delete carry [`KeyAction::LineEdgeMotion`]).
    ClearPreferredX,
    /// Visual-line edge motion that went through
    /// [`move_cursor_to_line_edge`], or horizontal motion or a removal that
    /// had to choose a side of a soft wrap or a direction seam. The handler
    /// already set `cursor_affinity` itself, so the post-processing must NOT
    /// clobber it. Clears the sticky column. Covers non-Ctrl Home/End, the
    /// Left/Right arrows, Backspace and Delete.
    LineEdgeMotion,
    /// Vertical motion (Up/Down): the sticky column must be preserved
    /// so repeated vertical presses land on the same visual column.
    /// The helpers also set `cursor_affinity` from hit-test, so
    /// post-processing must NOT clobber it. Ctrl+A also lives here
    /// since it leaves the caret position unchanged and any current
    /// affinity is still valid. PageUp/PageDown want the same
    /// treatment but carry [`KeyAction::PageMotion`], which says so
    /// separately.
    KeepPreferredX,
    /// PageUp/PageDown. Behaves exactly like [`KeyAction::KeepPreferredX`] —
    /// the helper set affinity, the sticky column must survive — but is kept
    /// distinct so the typewriter pin can *glide* to the new position instead
    /// of snapping. A page jump is a deliberate, screen-sized move where the
    /// animation helps the reader keep their bearings, unlike the per-keystroke
    /// re-pin, which must be instant.
    PageMotion,
    /// The key was not handled.
    Unhandled,
}

/// Whether a line break (Shift+Enter) can go in at the caret.
///
/// Not in a table cell, which Djot writes as one line, nor in a code block,
/// whose every line is a block of its own. There Shift+Enter does what Enter
/// does, and [`EditorHandle::insert_line_break`](super::EditorHandle::insert_line_break)
/// does nothing.
///
/// Asked where the break would go in, not where the caret is: over a
/// selection that is its start, since the insertion replaces it, or its end
/// under a filter that keeps a selection (see
/// [`collapse_selection_before_insert`]). A forward-only selection reaching
/// into a code block would otherwise have put the break in the code block.
pub(super) fn line_break_fits_at_caret(state: &EditorState) -> bool {
    let cursor = &state.cursor;
    let (position, anchor) = (cursor.position(), cursor.anchor());
    let at = if position == anchor {
        position
    } else if state
        .policy
        .command_filter
        .collapses_selection_before_insert()
    {
        position.max(anchor)
    } else {
        position.min(anchor)
    };
    let fits = |cursor: &TextCursor| {
        cursor.current_table_cell().is_none()
            && !cursor
                .block_format()
                .is_ok_and(|format| format.is_code_block == Some(true))
    };
    if at == position {
        fits(cursor)
    } else {
        fits(&state.document.cursor_at(at))
    }
}

/// Insert a line break at the caret, as Shift+Enter does: over the selection,
/// or after it under a filter that forbids taking text away. `false`, with
/// nothing changed, where [`line_break_fits_at_caret`] says no.
pub(super) fn insert_line_break(state: &EditorState, origin: InsertionOrigin) -> bool {
    if !line_break_fits_at_caret(state) {
        return false;
    }
    collapse_selection_before_insert(state);
    state.cursor.insert_line_break_with_origin(origin).is_ok()
}

/// Under a filter that forbids taking text away, move the caret to the end of
/// the selection instead of letting the imminent insertion replace it.
///
/// Every insertion path in text-document removes the selected range first
/// (`TextCursor::insert_text` / `insert_fragment` do it internally, below any
/// command vocabulary), so a filter that accepts `InsertChar` while rejecting
/// `DeletePrev` would still let a writer select three paragraphs and type over
/// them. Collapsing first turns that into an append at the end of the
/// selection: nothing is lost, and the keystroke still lands — dropping it
/// instead would make the mode feel like a broken keyboard.
///
/// A no-op under every other filter, so insert sites can call it
/// unconditionally.
pub(super) fn collapse_selection_before_insert(state: &EditorState) {
    if !state
        .policy
        .command_filter
        .collapses_selection_before_insert()
    {
        return;
    }
    if !state.cursor.has_selection() {
        return;
    }
    let end = state.cursor.position().max(state.cursor.anchor());
    state.cursor.set_position(end, MoveMode::MoveAnchor);
}

pub(super) fn handle_key(
    state: &SharedState,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    // IME composition — ongoing preedit (squiggly-underline preview
    // while the user is still choosing a candidate). Render the
    // preedit as a tentative insert so the caret position and visible
    // text match what the input method is showing. Replaced by the
    // next composition event, or finalised by the matching `ImeCommit`.
    if let WidgetEvent::ImeComposition {
        text,
        cursor,
        replacement,
    } = event
    {
        return handle_ime_composition(state, ctx, text, cursor.as_ref(), replacement.as_ref());
    }

    // IME commit — the user picked a candidate or typed a printable
    // key that finalises the sequence, or the input system wrote text of its
    // own (dictation, Writing Tools, the emoji viewer). The commit of a
    // composition takes its place; text with no composition before it is
    // batched into `pending_chars` like typing, or replaces the range the
    // input system named.
    if let WidgetEvent::ImeCommit {
        text,
        origin,
        replacement,
    } = event
    {
        let source = EditSource::from_ime_origin(*origin);
        let composing = {
            let mut guard = state.borrow_mut();
            let st = &mut *guard;
            st.composition.reconcile(&st.document);
            st.composition.is_held(&st.document)
        };
        return match replacement {
            Some(range) => {
                // A range the commit names itself is read against the text
                // without the composition.
                if composing {
                    let mut guard = state.borrow_mut();
                    let st = &mut *guard;
                    st.composition.cancel(&st.document);
                }
                replace_from_input_system(state, ctx, text, range.clone(), source)
            }
            None if composing => commit_composition(state, ctx, text, source),
            None => push_pending_chars(state, ctx, text, source),
        };
    }

    let WidgetEvent::KeyDown {
        key,
        modifiers,
        text,
        ..
    } = event
    else {
        return EventResponse::Ignored;
    };

    // Every key ends a link stop; the Tab that moves to the next link sets
    // the next one. The walk is what moves the reader's focus back, and a
    // key that changes nothing else it reads (Escape, a letter in a viewer)
    // would not ask for one, so ask here.
    if state.borrow_mut().link_stop.take().is_some() {
        ctx.request_accessibility_update();
    }

    // A key after a composition's empty update, where its commit would have
    // been: the input method cancelled it.
    if settle_cancelled_composition(&mut state.borrow_mut()) {
        sync_cursor_signals(state);
        ctx.request_frame();
    }

    // A key the text service passed on while the composition it writes is
    // open ends that composition, its text kept, before the key acts, as a
    // native edit control's does (measured in Notepad: the Microsoft
    // Japanese IME passes Down after the first Space, and the composition
    // ends there).
    let written = {
        let st = state.borrow();
        st.composition.is_written(&st.document)
    };
    if written {
        finish_composition(state, ctx);
    }

    // Enter with the caret on a link follows it, in an editor that takes no
    // new paragraph (a viewer) and follows links: the keyboard's form of a
    // click on the link. Resolved before the state is borrowed for the key,
    // because the callback may replace the very document. The filter question
    // agrees with the one that makes the links Tab stops (`tab_consumed`,
    // which asks `NavigateTableCell`) for every shipped filter.
    if matches!(key, Key::Enter) && *modifiers == teksilo_core::event::Modifiers::NONE {
        let href = {
            let st = state.borrow();
            if st.on_link_activated.is_none()
                || st
                    .policy
                    .command_filter
                    .accepts(EditCommandKind::InsertBlock)
            {
                None
            } else {
                st.cursor.link_at_caret().map(|link| link.href)
            }
        };
        if let Some(href) = href {
            return super::follow_link(state, &href, ctx);
        }
    }

    let shift = modifiers.shift();
    // The platform accelerator, not physical Control: ⌘ on macOS, Ctrl
    // elsewhere. Testing both would make the Win key act as Ctrl on Linux and
    // leave ⌃C doing Copy on macOS, where Control belongs to the text system.
    let ctrl = modifiers.command();
    // Which motion the arrow keys mean here. Not derivable from `ctrl`: on
    // macOS word-jump is ⌥←/→ while ⌘←/→ reaches the line edge and ⌘↑/↓ the
    // document, a three-way split no single accelerator flag can express.
    // See `common::text_nav`.
    let caret_motion = text_nav::caret_step(*modifiers);
    let line_motion = text_nav::line_step(*modifiers);
    // Tab's escape hatch is the exception, and it needs physical Control too.
    // Ctrl+Tab hands focus to the next widget on every platform, macOS
    // included (⌘⇥ is the application switcher and never arrives), so the Tab
    // arms below must decline a ⌃-modified Tab even where ⌃ is not the
    // accelerator — otherwise the editor swallows the one chord that gets a
    // keyboard user out of a table.
    let tab_escape = ctrl || modifiers.ctrl();
    let mode = if shift {
        MoveMode::KeepAnchor
    } else {
        MoveMode::MoveAnchor
    };

    // `TextCursor::clone()` creates an **independent** cursor with
    // its own position/anchor data (see the Clone impl in
    // text-document/.../cursor.rs). Cloning and mutating the clone
    // leaves `state.cursor` untouched. We must therefore operate on
    // `state.cursor` directly through a short-lived borrow and drop
    // the state before calling `sync_cursor_signals` so the signal
    // observers see the post-move value.
    // Every key resets `select_all_level`, so the next select-all starts at
    // level 1 — matching the godot reference's own reset. Select-all itself
    // is not a key here (see `run_standard_edit`), so nothing is exempt.
    let action: KeyAction = {
        let mut st = state.borrow_mut();
        st.select_all_level = 0;
        // A key that types text joins the batch below. Any other acts at
        // once, so what was typed before it in this frame goes in first.
        let types_text = !ctrl
            && text
                .as_deref()
                .is_some_and(|t| t.chars().any(|c| !c.is_control()));
        if !types_text {
            super::frame_loop::flush_pending_chars(&mut st);
        }
        let filter = st.policy.command_filter;
        // One predicate for Tab, shared with the accessibility description.
        let tab_consumed = st.tab_consumed();
        match key {
            Key::ArrowLeft if filter.accepts(EditCommandKind::MoveLeft) => {
                // Shift+Arrow at a cell boundary activates / extends a
                // rectangular cell selection (godot parity). Only
                // claims the event when we're actually at a cell
                // boundary; otherwise fall through to text-range
                // selection.
                if shift && try_extend_cell_selection(&mut st, -1, 0) {
                    KeyAction::ClearPreferredX
                } else if caret_motion == CaretStep::LineEdge {
                    let edge = visual_line_edge(&st, VisualDirection::Left);
                    move_cursor_to_line_edge(&mut st, edge, mode);
                    KeyAction::LineEdgeMotion
                } else if !shift
                    && settle_without_stepping(&mut st, VisualDirection::Left, caret_motion)
                {
                    KeyAction::LineEdgeMotion
                } else {
                    let op = visual_move(&st, VisualDirection::Left, caret_motion);
                    st.cursor.move_position(op, mode, 1);
                    settle_affinity_after_step(&mut st, VisualDirection::Left);
                    KeyAction::LineEdgeMotion
                }
            }
            Key::ArrowRight if filter.accepts(EditCommandKind::MoveRight) => {
                if shift && try_extend_cell_selection(&mut st, 1, 0) {
                    KeyAction::ClearPreferredX
                } else if caret_motion == CaretStep::LineEdge {
                    let edge = visual_line_edge(&st, VisualDirection::Right);
                    move_cursor_to_line_edge(&mut st, edge, mode);
                    KeyAction::LineEdgeMotion
                } else if !shift
                    && settle_without_stepping(&mut st, VisualDirection::Right, caret_motion)
                {
                    KeyAction::LineEdgeMotion
                } else {
                    let op = visual_move(&st, VisualDirection::Right, caret_motion);
                    st.cursor.move_position(op, mode, 1);
                    settle_affinity_after_step(&mut st, VisualDirection::Right);
                    KeyAction::LineEdgeMotion
                }
            }
            Key::ArrowUp if filter.accepts(EditCommandKind::MoveUp) => {
                if shift && try_extend_cell_selection(&mut st, 0, -1) {
                    KeyAction::ClearPreferredX
                } else {
                    match vertical_op(line_motion, false) {
                        Some(op) => {
                            st.cursor.move_position(op, mode, 1);
                            KeyAction::ClearPreferredX
                        }
                        None => {
                            move_cursor_vertical(&mut st, -1, mode);
                            KeyAction::KeepPreferredX
                        }
                    }
                }
            }
            Key::ArrowDown if filter.accepts(EditCommandKind::MoveDown) => {
                if shift && try_extend_cell_selection(&mut st, 0, 1) {
                    KeyAction::ClearPreferredX
                } else {
                    match vertical_op(line_motion, true) {
                        Some(op) => {
                            st.cursor.move_position(op, mode, 1);
                            KeyAction::ClearPreferredX
                        }
                        None => {
                            move_cursor_vertical(&mut st, 1, mode);
                            KeyAction::KeepPreferredX
                        }
                    }
                }
            }
            Key::PageUp if filter.accepts(EditCommandKind::PageUp) => {
                move_cursor_page(&mut st, -1, mode);
                KeyAction::PageMotion
            }
            Key::PageDown if filter.accepts(EditCommandKind::PageDown) => {
                move_cursor_page(&mut st, 1, mode);
                KeyAction::PageMotion
            }
            Key::Home if filter.accepts(EditCommandKind::MoveHome) => {
                if ctrl {
                    st.cursor.move_position(MoveOperation::Start, mode, 1);
                    KeyAction::ClearPreferredX
                } else {
                    // `move_cursor_to_line_edge` sets `cursor_affinity`
                    // from the line's extent, so this path returns
                    // `LineEdgeMotion` (skips the post-processing
                    // affinity reset).
                    move_cursor_to_line_edge(&mut st, LineEdge::Start, mode);
                    KeyAction::LineEdgeMotion
                }
            }
            Key::End if filter.accepts(EditCommandKind::MoveEnd) => {
                if ctrl {
                    st.cursor.move_position(MoveOperation::End, mode, 1);
                    KeyAction::ClearPreferredX
                } else {
                    // Use the typesetter to find end-of-visual-line
                    // rather than text-document's EndOfBlock. Two
                    // wins: (a) a second End press from an already-
                    // at-end cursor is a no-op, avoiding the
                    // block-advance bug where `get_block_at_position`
                    // returns the *next* block when queried at a
                    // boundary; (b) wrapped blocks stop at the wrap
                    // point, which is the standard editor behaviour.
                    // The helper sets `cursor_affinity` from the
                    // line's extent → returns `LineEdgeMotion`.
                    move_cursor_to_line_edge(&mut st, LineEdge::End, mode);
                    KeyAction::LineEdgeMotion
                }
            }
            // Select-all, the clipboard and undo / redo are not keys here:
            // they arrive as the `edit.*` actions `run_standard_edit` serves,
            // so the user can rebind them.
            // --- Editor-preset mutating commands ---
            Key::Backspace if filter.accepts(EditCommandKind::DeletePrev) => {
                let word = text_nav::deletes_word(*modifiers);
                // Delete the word to the left: Ctrl+⌫, ⌥⌫ on macOS.
                // Select the word, then delete the selection, which
                // matches the godot reference's delete-word (there is
                // no dedicated delete-word API on TextCursor).
                if word && !st.cursor.has_selection() {
                    st.cursor
                        .move_position(MoveOperation::WordLeft, MoveMode::KeepAnchor, 1);
                }
                let landing = if st.cursor.has_selection() {
                    st.cursor.selection_start()
                } else {
                    st.cursor.position().saturating_sub(1)
                };
                let affinity = affinity_after_step(&st.engine, st.shown_position(landing));
                if word || st.cursor.has_selection() {
                    let _ = st.cursor.remove_selected_text();
                } else if st.cursor.at_block_start()
                    && is_cursor_in_list(&st)
                    && filter.accepts(EditCommandKind::ExitList)
                {
                    // Backspace at start of a list item:
                    //  * Indented → decrement the block's indent level
                    //    (visually pulls the item leftward; standard
                    //    word-processor behaviour for Backspace-at-
                    //    indent).
                    //  * Indent 0 → remove the block from the list
                    //    entirely (converts it back to a regular
                    //    paragraph). Matches the godot reference's
                    //    backspace-in-a-list rule.
                    if let Ok(fmt) = st.cursor.block_format() {
                        let level = fmt.indent.unwrap_or(0);
                        if level > 0 {
                            let new_fmt = BlockFormat {
                                indent: Some(level - 1),
                                ..Default::default()
                            };
                            let _ = st.cursor.set_block_format(&new_fmt);
                        } else {
                            let _ = st.cursor.remove_current_block_from_list();
                        }
                    }
                } else if st.cursor.at_block_start()
                    && st.cursor.is_in_blockquote()
                    && st.cursor.is_first_block_in_current_frame()
                    && filter.accepts(EditCommandKind::ExitFrame)
                {
                    // Backspace at the very start of the first block of
                    // a blockquote — unwrap one level so the block lifts
                    // out of the quote. Without this, fallthrough would
                    // delete the boundary `\n` and silently drag the
                    // quoted paragraph into the preceding block (the
                    // crash class A1/A2/A3 closed at the data layer;
                    // here we make the UX intent explicit).
                    let _ = st.cursor.unwrap_current_block_from_blockquote();
                } else {
                    let _ = st.cursor.delete_previous_char();
                }
                st.cursor_affinity = affinity;
                KeyAction::LineEdgeMotion
            }
            Key::Delete if filter.accepts(EditCommandKind::DeleteNext) => {
                // The caret stays where it is, on the line it is on, unless a
                // selection goes with the key.
                let affinity = if st.cursor.has_selection() {
                    affinity_after_step(&st.engine, st.shown_position(st.cursor.selection_start()))
                } else {
                    st.cursor_affinity
                };
                if text_nav::deletes_word(*modifiers) {
                    if !st.cursor.has_selection() {
                        st.cursor
                            .move_position(MoveOperation::WordRight, MoveMode::KeepAnchor, 1);
                    }
                    let _ = st.cursor.remove_selected_text();
                } else if st.cursor.has_selection() {
                    let _ = st.cursor.remove_selected_text();
                } else if st.cursor.at_block_end()
                    && st.cursor.is_in_blockquote()
                    && st.cursor.is_last_block_in_current_frame()
                    && filter.accepts(EditCommandKind::ExitFrame)
                {
                    // Delete at the very end of the last block of a
                    // blockquote — unwrap one level instead of merging
                    // across the frame boundary into the following
                    // block (symmetric with Backspace at-block-start).
                    let _ = st.cursor.unwrap_current_block_from_blockquote();
                } else {
                    let _ = st.cursor.delete_char();
                }
                st.cursor_affinity = affinity;
                KeyAction::LineEdgeMotion
            }
            // The forced paragraph insert (Ctrl+Enter) is not a key here:
            // it is the `format.insert_paragraph` action
            // (`run_format_shortcut`), so the user can rebind it.
            //
            // Shift+Enter: a line break inside the block, so the next line is
            // more of the same paragraph, list item or quotation. It stays a
            // key arm and is not a rebindable shortcut: a global shortcut
            // resolves before the focused widget, and would take Shift+Enter
            // away from every other text field (a find bar's "previous
            // match"). Where a line break cannot be kept it falls through to
            // Enter below.
            Key::Enter
                if shift
                    && !ctrl
                    && filter.accepts(EditCommandKind::InsertLineBreak)
                    && line_break_fits_at_caret(&st) =>
            {
                insert_line_break(&st, InsertionOrigin::Typed);
                KeyAction::ClearPreferredX
            }
            Key::Enter if !ctrl && filter.accepts(EditCommandKind::InsertBlock) => {
                // Enter (without Ctrl) inside a table cell: move to
                // the cell in the same column one row down, or (on
                // the last row) step out of the table to the block
                // that follows. Outside a table, fall through to
                // `insert_block`. Shift+Enter arrives here in a table
                // cell or a code block, and does the same.
                let cell_info = st
                    .cursor
                    .current_table_cell()
                    .map(|c| (c.table.id(), c.row, c.column, c.table.rows()));
                if let Some((table_id, row, col, rows)) = cell_info
                    && filter.accepts(EditCommandKind::NavigateTableCellDown)
                {
                    navigate_table_cell_down(&mut st, table_id, row, col, rows);
                } else if st.cursor.is_in_blockquote()
                    && st.cursor.current_block_is_empty()
                    && filter.accepts(EditCommandKind::ExitFrame)
                {
                    // Enter on an empty paragraph inside a blockquote
                    // unwraps one nesting level (user-confirmed
                    // semantics). Inserting another empty block here
                    // would trap the cursor inside the quote with no
                    // intuitive way out.
                    let _ = st.cursor.unwrap_current_block_from_blockquote();
                } else {
                    collapse_selection_before_insert(&st);
                    split_paragraph(&mut st);
                }
                KeyAction::ClearPreferredX
            }
            Key::Tab if !tab_escape && shift && tab_consumed => {
                // Shift+Tab (no Ctrl): previous table cell when
                // inside a table; dedent the current list item when
                // the caret sits anywhere inside it (matches standard
                // word-processor behaviour — the user doesn't need to
                // move to the exact block start first). Otherwise
                // swallow (prevents focus-navigation bleed).
                // Ctrl+Shift+Tab is left unhandled so the OS /
                // app-level focus navigation can claim it.
                let cell_info = st.cursor.current_table_cell().map(|c| {
                    (
                        c.table.id(),
                        c.row,
                        c.column,
                        c.table.rows(),
                        c.table.columns(),
                    )
                });
                if let Some((table_id, row, col, rows, cols)) = cell_info {
                    navigate_table_cell(&mut st, table_id, row, col, rows, cols, -1);
                } else if is_cursor_in_list(&st) {
                    dedent_current_block(&mut st);
                } else if st.cursor.is_in_blockquote()
                    && filter.accepts(EditCommandKind::DecreaseBlockquoteDepth)
                {
                    // Shift+Tab inside a blockquote (no list active):
                    // pop one nesting level. At depth 1 this unwraps to
                    // a plain paragraph.
                    let _ = st.cursor.decrease_blockquote_depth();
                }
                KeyAction::ClearPreferredX
            }
            // In an editor that leaves Tab to focus traversal because its
            // filter takes no edits (a viewer) and that follows links, the
            // links are the stops inside it: Tab selects the next one and
            // Shift+Tab the previous one, and past the last (or before the
            // first) the key leaves the editor. `TabKey::MoveFocus` turns the
            // stops off. The filter question agrees with Enter's (above) for
            // every shipped filter.
            Key::Tab if !tab_escape && st.links_are_stops() => match link_tab_stop(&st, shift) {
                Some((start, end)) => {
                    select_link_stop_locked(&mut st, start, end);
                    KeyAction::ClearPreferredX
                }
                None => KeyAction::Unhandled,
            },
            Key::Tab if !tab_escape && !shift && tab_consumed => {
                // Tab (no Ctrl, no Shift):
                //  * Inside a table cell → next cell (wraps to next row;
                //    at last cell, insert a new row below).
                //  * Inside a list item → increase indent. Works from
                //    any caret position within the block so the user
                //    doesn't have to Home first.
                //  * Inside a blockquote → nest one level deeper.
                //  * Otherwise → insert a literal `\t`.
                // The two nesting arms stop at the nesting ceilings and then
                // do nothing (see `nesting`).
                // Ctrl+Tab is left unhandled (OS focus navigation).
                let cell_info = st.cursor.current_table_cell().map(|c| {
                    (
                        c.table.id(),
                        c.row,
                        c.column,
                        c.table.rows(),
                        c.table.columns(),
                    )
                });
                if let Some((table_id, row, col, rows, cols)) = cell_info {
                    navigate_table_cell(&mut st, table_id, row, col, rows, cols, 1);
                } else if is_cursor_in_list(&st) {
                    // The list keeps the key even where the item cannot go
                    // deeper (the list ceiling): passing it on would nest the
                    // quote around the item instead, or type a tab over the
                    // selection.
                    if filter.accepts(EditCommandKind::IndentBlock) {
                        indent_current_block(&mut st);
                    }
                } else if st.cursor.is_in_blockquote()
                    && filter.accepts(EditCommandKind::IncreaseBlockquoteDepth)
                {
                    // Tab inside a blockquote (no list active): wrap the
                    // current block in a deeper nested quote. Order
                    // matters — the list check above takes priority for
                    // a list-inside-quote scenario.
                    //
                    // At the quote ceiling the key is spent here and nothing
                    // changes, as in a list at its deepest level. Falling
                    // through to the literal-tab arm would answer a request
                    // for depth by typing a tab character, over the selected
                    // paragraphs when there is a selection.
                    nesting::increase_blockquote_depth(&st);
                } else if filter.accepts(EditCommandKind::InsertTab) {
                    collapse_selection_before_insert(&st);
                    let _ = st.cursor.insert_text("\t");
                }
                KeyAction::ClearPreferredX
            }
            // Bold, italic and underline are the `format.*` actions
            // (`run_format_shortcut`), not keys.
            _ => {
                // Printable character fallback: winit populates
                // `KeyDown::text` with the character produced by the
                // key (post-layout mapping, so Shift / dead keys /
                // layout translations are already applied). Guard
                // against the platform accelerator (⌘ on macOS, Ctrl
                // elsewhere) being held — some
                // layouts populate `text` with a control character
                // even for Ctrl+letter, and we don't want
                // unhandled Ctrl combos (Ctrl+F, Ctrl+W, …) to
                // accidentally insert the letter. Alt / AltGr are
                // NOT rejected because they produce legitimate
                // diacritic input on European layouts
                // (e.g. macOS Option+E → ´).
                if ctrl {
                    KeyAction::Unhandled
                } else if let Some(t) = text.as_deref() {
                    if filter.accepts(EditCommandKind::InsertChar) {
                        let clean: String = t.chars().filter(|c| !c.is_control()).collect();
                        if !clean.is_empty() {
                            st.pending_arrivals
                                .note(EditSource::Keyboard, clean.chars().count());
                            st.pending_chars.push_str(&clean);
                            KeyAction::ClearPreferredX
                        } else {
                            KeyAction::Unhandled
                        }
                    } else {
                        KeyAction::Unhandled
                    }
                } else {
                    KeyAction::Unhandled
                }
            }
        }
    };

    finish_key_action(state, ctx, action)
}

/// Whether one of the standard edit commands has anything to act on in the
/// editor now, which is what greys its menu rows: a command the editor's
/// [`CommandFilter`](crate::common::editor_runtime::CommandFilter) accepts,
/// and for Copy and Cut a selection, for Select All some text, for Undo and
/// Redo some history that way. Never asked by a dispatch, which runs
/// [`run_standard_edit`] whatever this answers. `true` while the state is
/// borrowed, the answer from before there was a question.
pub(super) fn standard_edit_available(state: &SharedState, command: StandardEdit) -> bool {
    let Ok(st) = state.try_borrow() else {
        return true;
    };
    let filter = st.policy.command_filter;
    match command {
        StandardEdit::Copy => filter.accepts(EditCommandKind::Copy) && st.cursor.has_selection(),
        StandardEdit::Cut => filter.accepts(EditCommandKind::Cut) && st.cursor.has_selection(),
        StandardEdit::Paste => filter.accepts(EditCommandKind::Paste),
        StandardEdit::PastePlain => filter.accepts(EditCommandKind::PasteUnformatted),
        StandardEdit::SelectAll => {
            filter.accepts(EditCommandKind::SelectAll) && st.document.character_count() > 0
        }
        StandardEdit::Undo => filter.accepts(EditCommandKind::Undo) && st.document.can_undo(),
        StandardEdit::Redo => filter.accepts(EditCommandKind::Redo) && st.document.can_redo(),
        _ => false,
    }
}

/// Whether the editor declines a standard edit command now, which passes the
/// intent on to an ancestor's action and then to the plain key: a command
/// the editor's [`CommandFilter`](crate::common::editor_runtime::CommandFilter)
/// refuses (an application that must not route a refused Undo elsewhere asks
/// the focused text surface's `history_frozen` in its own action), and with
/// `empty_history_passes_on` (the tree's
/// [`TextSurfaces::set_empty_history_passes_on`](teksilo_core::text_surface::TextSurfaces::set_empty_history_passes_on))
/// Undo with nothing to undo and Redo with nothing to redo.
/// [`run_standard_edit`] declines exactly these, and a menu row asks this to
/// know whether the editor is where its command stops. `false` while the
/// state is borrowed.
pub(super) fn standard_edit_passes_on(
    state: &SharedState,
    command: StandardEdit,
    empty_history_passes_on: bool,
) -> bool {
    state
        .try_borrow()
        .is_ok_and(|st| declines(&st, command, empty_history_passes_on))
}

fn declines(st: &EditorState, command: StandardEdit, empty_history_passes_on: bool) -> bool {
    let filter = st.policy.command_filter;
    let kind = match command {
        StandardEdit::SelectAll => EditCommandKind::SelectAll,
        StandardEdit::Copy => EditCommandKind::Copy,
        StandardEdit::Cut => EditCommandKind::Cut,
        StandardEdit::Paste => EditCommandKind::Paste,
        StandardEdit::PastePlain => EditCommandKind::PasteUnformatted,
        StandardEdit::Undo => EditCommandKind::Undo,
        StandardEdit::Redo => EditCommandKind::Redo,
        _ => return true,
    };
    !filter.accepts(kind)
        || (empty_history_passes_on
            && match command {
                StandardEdit::Undo => !st.document.can_undo(),
                StandardEdit::Redo => !st.document.can_redo(),
                _ => false,
            })
}

/// Whether `command` changes the text: the ones that end a composition in
/// progress before they run. Select-all and copy leave it alive.
fn edits_text(command: StandardEdit) -> bool {
    matches!(
        command,
        StandardEdit::Cut
            | StandardEdit::Paste
            | StandardEdit::PastePlain
            | StandardEdit::Undo
            | StandardEdit::Redo
    )
}

/// Whether the editor takes `command` at all, from any route: what its keys
/// take, by the same rule ([`declines`], the command filter). `TextSurface::accepts`
/// answers with it, and the surface's own commands obey it, so a routed command
/// and the key agree.
pub(super) fn accepts_standard_edit(st: &EditorState, command: StandardEdit) -> bool {
    !declines(st, command, false)
}

/// Run one of the standard edit commands, as its `edit.*` action does.
///
/// Returns whether it ran: it declines what [`standard_edit_passes_on`] says
/// it does, which passes the intent on.
pub(super) fn run_standard_edit(
    state: &SharedState,
    command: StandardEdit,
    empty_history_passes_on: bool,
    ctx: &mut EventContext,
) -> bool {
    // A command that edits the text ends a composition in progress first, its
    // text kept, as a click elsewhere does (see `finish_composition`). Left
    // alive, the edit would take the composition out of the text while the
    // input method still held it, and its next update would bring it back at
    // the caret. The keys, the native menu and a menu row all come here.
    if edits_text(command) && !declines(&state.borrow(), command, false) {
        finish_composition(state, ctx);
    }
    let action = {
        let mut st = state.borrow_mut();
        // After what was typed before the command in this frame.
        super::frame_loop::flush_pending_chars(&mut st);
        // Select-all is the one command that keeps climbing its ladder;
        // anything else in between starts it again at level 1.
        if command != StandardEdit::SelectAll {
            st.select_all_level = 0;
        }
        if declines(&st, command, empty_history_passes_on) {
            crate::common::input_trace::trace(format_args!("{command:?} declined"));
            return false;
        }
        crate::common::input_trace::trace(format_args!(
            "{command:?} runs; typing waiting {:?}, composition held {}",
            st.pending_chars,
            st.composition.is_held(&st.document)
        ));
        match command {
            StandardEdit::SelectAll => {
                apply_select_all_ladder(&mut st);
                // Horizontal motion is invalidated regardless.
                st.preferred_x = None;
                // KeepPreferredX skips the preferred_x reset in
                // post-processing — we already nulled it inline, and
                // crucially we must not clobber the `select_all_level`
                // which was just incremented.
                KeyAction::KeepPreferredX
            }
            StandardEdit::Copy => {
                clipboard::copy(&mut st, ctx);
                KeyAction::ClearPreferredX
            }
            StandardEdit::Cut => {
                clipboard::cut(&mut st, ctx);
                KeyAction::ClearPreferredX
            }
            StandardEdit::Paste => {
                clipboard::paste(&mut st, ctx);
                KeyAction::ClearPreferredX
            }
            StandardEdit::PastePlain => {
                clipboard::paste_unformatted(&mut st, ctx);
                KeyAction::ClearPreferredX
            }
            // `history_step` sets the caret's side of a wrap, which
            // `LineEdgeMotion` keeps (it clears the sticky column too).
            StandardEdit::Undo => {
                history_step(&mut st, HistoryStep::Undo);
                KeyAction::LineEdgeMotion
            }
            StandardEdit::Redo => {
                history_step(&mut st, HistoryStep::Redo);
                KeyAction::LineEdgeMotion
            }
            _ => return false,
        }
    };
    finish_key_action(state, ctx, action);
    true
}

/// Which way [`history_step`] goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum HistoryStep {
    Undo,
    Redo,
}

/// Undo or redo one step, and show the caret on the line its text is on now.
///
/// The document puts the caret back at a position, and a position on a soft
/// wrap is on two lines. The caret is put at the start of the next line, as
/// after a character step or a removal ([`affinity_after_step`]): undoing a
/// Backspace or a letter typed at the start of a wrapped line otherwise showed
/// the caret at the end of the line above, and a reader asked for the caret's
/// line read that line. text-document keeps no affinity in its history, so an
/// edit made at the end of a wrapped line (after End) comes back at the start
/// of the next one: the same position, on the line after.
///
/// The layout is stale after the step, so it is brought up to date first.
pub(super) fn history_step(st: &mut EditorState, step: HistoryStep) {
    let document = st.document.clone();
    match step {
        HistoryStep::Undo => {
            crate::common::input_trace::traced_history_step(&document, "undo", || document.undo());
        }
        HistoryStep::Redo => {
            crate::common::input_trace::traced_history_step(&document, "redo", || document.redo());
        }
    }
    super::frame_loop::catch_up_layout(st);
    st.cursor_affinity = affinity_after_step(&st.engine, st.shown_caret());
}

/// Run one of the editor's own [`FormatShortcut`] commands, as its `format.*`
/// action does. Returns whether it ran: a command the filter refuses is
/// declined and passed on, as in [`run_standard_edit`].
pub(super) fn run_format_shortcut(
    state: &SharedState,
    command: FormatShortcut,
    ctx: &mut EventContext,
) -> bool {
    // A format ends a composition in progress first, its text kept, as an
    // edit command does (see `run_standard_edit`).
    finish_composition(state, ctx);
    let action = {
        let mut st = state.borrow_mut();
        // After what was typed before the command in this frame.
        super::frame_loop::flush_pending_chars(&mut st);
        // Not select-all, so the select-all ladder starts again at level 1.
        st.select_all_level = 0;
        let filter = st.policy.command_filter;
        match command {
            FormatShortcut::Bold if filter.accepts(EditCommandKind::ToggleBold) => {
                toggle_char_format(&mut st, FormatBit::Bold);
            }
            FormatShortcut::Italic if filter.accepts(EditCommandKind::ToggleItalic) => {
                toggle_char_format(&mut st, FormatBit::Italic);
            }
            FormatShortcut::Underline if filter.accepts(EditCommandKind::ToggleUnderline) => {
                toggle_char_format(&mut st, FormatBit::Underline);
            }
            FormatShortcut::InsertParagraph
                if filter.accepts(EditCommandKind::InsertBlockForced) =>
            {
                // Always a new block, bypassing table-cell navigation.
                // Matches the godot reference's forced-block-insert branch.
                collapse_selection_before_insert(&st);
                split_paragraph(&mut st);
            }
            _ => return false,
        }
        KeyAction::ClearPreferredX
    };
    finish_key_action(state, ctx, action);
    true
}

/// Make the link over `[start, end)` (document positions) the link stop, as
/// Tab does, and show its start in the editor's own viewport. The caret is at
/// the link's end, which [`finish_link_stop`] then scrolls into view, so a
/// link that wraps is seen whole when it fits: showing only the caret's line,
/// Shift+Tab up to one showed only its last line.
pub(super) fn select_link_stop_locked(st: &mut EditorState, start: usize, end: usize) {
    st.cursor.set_position(start, MoveMode::MoveAnchor);
    st.cursor.set_position(end, MoveMode::KeepAnchor);
    let (a, b) = st.shown_selection();
    st.link_stop = Some((a.min(b), a.max(b)));
    ensure_position_visible_locked(st, a.min(b), CursorAffinity::Downstream);
}

/// The tail Tab runs after [`select_link_stop_locked`]: the caret's end of the
/// link in the editor's own viewport and the whole link in any enclosing
/// scroll area, the cursor published, a frame requested. For an assistive
/// `Focus` on a link, which reaches the same link stop by another way.
pub(super) fn finish_link_stop(state: &SharedState, ctx: &mut EventContext) -> EventResponse {
    finish_key_action(state, ctx, KeyAction::ClearPreferredX)
}

/// The shared tail of a handled key or command: what `action` says about the
/// sticky column and the caret's affinity, then the caret-moved epilogue.
fn finish_key_action(
    state: &SharedState,
    ctx: &mut EventContext,
    action: KeyAction,
) -> EventResponse {
    match action {
        KeyAction::Unhandled => EventResponse::Ignored,
        KeyAction::ClearPreferredX => {
            {
                let mut st = state.borrow_mut();
                st.preferred_x = None;
                // Horizontal motion / edit / Ctrl+Home / Ctrl+End:
                // any sticky affinity from a previous click or
                // vertical move is no longer meaningful. The caret
                // moved logically; render at the default downstream
                // placement.
                st.cursor_affinity = CursorAffinity::Downstream;
            }
            caret_moved_epilogue(state, ctx)
        }
        KeyAction::LineEdgeMotion => {
            // Home/End went through `move_cursor_to_line_edge`, which
            // already set `cursor_affinity` from the typesetter's
            // hit-test. Just clear the sticky column.
            {
                let mut st = state.borrow_mut();
                st.preferred_x = None;
            }
            caret_moved_epilogue(state, ctx)
        }
        KeyAction::KeepPreferredX => {
            // Up/Down's helper already set affinity from hit-test; Ctrl+A
            // didn't move the caret. Preserve both `preferred_x` and
            // `cursor_affinity` unchanged.
            caret_moved_epilogue(state, ctx)
        }
        KeyAction::PageMotion => {
            // As `KeepPreferredX`, but the typewriter pin glides — see the
            // variant's docs.
            caret_moved_epilogue_with(state, ctx, ScrollMotion::Smooth)
        }
    }
}

/// Shared tail for the caret-moving key actions. Keeps the caret inside the
/// editor's own viewport ([`ensure_caret_visible`]), publishes the new cursor
/// position to observers, updates the OS-IME candidate area, chases the caret
/// into any enclosing scroll area ([`chase_caret_into_view`]), and requests a
/// frame. Always returns [`EventResponse::Handled`].
fn caret_moved_epilogue(state: &SharedState, ctx: &mut EventContext) -> EventResponse {
    caret_moved_epilogue_with(state, ctx, ScrollMotion::Instant)
}

/// [`caret_moved_epilogue`] with an explicit typewriter-pin motion — see
/// [`chase_caret_into_view_with`]. Used by the page-sized jumps, which glide.
fn caret_moved_epilogue_with(
    state: &SharedState,
    ctx: &mut EventContext,
    motion: ScrollMotion,
) -> EventResponse {
    // Any keyboard-driven caret move hands control back to the typewriter pin
    // after a click or drag stood it down.
    state.borrow_mut().mouse_anchored = false;
    ensure_caret_visible(state);
    sync_cursor_signals(state);
    report_ime_cursor_area(state, ctx);
    chase_caret_into_view_with(state, ctx, motion);
    ctx.request_frame();
    EventResponse::Handled
}

/// The caret's rectangle in **absolute window (tree) coordinates**, or `None`
/// when the editor is unfocused or the engine has not been laid out yet.
///
/// [`RichTextEngine::caret_rect`](teksilo_text::RichTextEngine::caret_rect)
/// answers in two spaces at once: `y` already has the engine's scroll offset
/// subtracted — it is where the caret paints — while `x` does not, because the
/// engine has no horizontal scroll to apply. So the window rect adds the body's
/// window origin to both and subtracts the horizontal scroll from `x` alone.
/// Taking the vertical offset off a second time, which is what this did, put
/// the rect a whole viewport out of place in any scrolled editor — and this
/// rect steers the OS-IME candidate window, the enclosing-scroll-area follow
/// and the typewriter pin.
///
/// See [`vertical_scroll_slip`] for the one thing `y` still needs.
pub(super) fn caret_window_rect(st: &EditorState) -> Option<teksilo_canvas::Rect> {
    if !st.has_focus || !st.engine.has_full_layout() {
        return None;
    }
    let caret = st.engine.caret_rect(st.shown_caret(), st.cursor_affinity);
    Some(teksilo_canvas::Rect::new(
        st.viewport_origin.x + caret[0] - st.scroll_x.get(),
        st.viewport_origin.y + caret[1] + vertical_scroll_slip(st),
        caret[2].max(1.0),
        caret[3],
    ))
}

/// An engine caret rectangle (`[x, y, width, height]`, as
/// `RichTextEngine::caret_rect` answers it) in window space, by the
/// composition [`caret_window_rect`] and [`range_window_rect`] use: the body's
/// window origin added to both axes, the horizontal scroll taken off `x`, the
/// vertical scroll slip added to `y`. At least 1 wide and tall.
pub(super) fn engine_caret_to_window(st: &EditorState, caret: [f32; 4]) -> teksilo_canvas::Rect {
    teksilo_canvas::Rect::new(
        st.viewport_origin.x + caret[0] - st.scroll_x.get(),
        st.viewport_origin.y + caret[1] + vertical_scroll_slip(st),
        caret[2].max(1.0),
        caret[3].max(1.0),
    )
}

/// How far the engine's idea of the scroll offset lags the widget's.
///
/// The engine subtracts the offset it was last *given*, which the widget hands
/// it at paint. A scroll that has happened since — a wheel event this frame, a
/// programmatic `scroll_y.set` before the next render — has not reached it, so
/// a screen-space `y` straight out of the engine is stale by exactly this much.
/// Zero on any settled frame, which is why it went unnoticed until a query ran
/// between a scroll and a repaint.
fn vertical_scroll_slip(st: &EditorState) -> f32 {
    (st.engine.scroll_offset() - st.scroll_y.get()) * st.engine.zoom()
}

/// The **content-space** rectangle enclosing `[start, end)` — y = 0 at the top of
/// the laid-out text, unaffected by scrolling and by where the editor sits in the
/// window.
///
/// The scroll-free half of [`range_window_rect`]. A decoration drawn *beside* the
/// text (a comment's leader, a bracket) wants window space, because it has to line
/// itself up with a rect the arena also stores. Anything that maps a document onto
/// a **proportion** of itself — an overview lane, a scrollbar annotation — wants
/// this instead: window space answers "where is this on screen right now", which
/// changes with every scroll tick and is meaningless for an offset the writer has
/// scrolled past.
pub(super) fn range_content_rect(
    st: &EditorState,
    start: usize,
    end: usize,
) -> Option<teksilo_canvas::Rect> {
    if !st.engine.has_full_layout() {
        return None;
    }
    // The same fixed Downstream affinity `range_window_rect` uses, and for the same
    // reason: these are arbitrary offsets unrelated to the caret, so reading the live
    // `cursor_affinity` would put the rect on the wrong side of a soft-wrap boundary
    // depending on where the caret happens to sit.
    //
    // `caret_rect_content`, not `caret_rect`: the plain form answers in screen
    // space, so building this rect from it made the "scroll-free" half of the
    // pair move with every scroll tick — and an overview lane dividing it by
    // `content_height` drew its mark somewhere else on every frame.
    let a = st
        .engine
        .caret_rect_content(start, CursorAffinity::Downstream);
    let b = st
        .engine
        .caret_rect_content(end, CursorAffinity::Downstream);
    let (ay, ah) = (a[1], a[3].max(1.0));
    let (by, bh) = (b[1], b[3].max(1.0));

    let x0 = a[0].min(b[0]);
    let x1 = a[0].max(b[0]);
    let y0 = ay.min(by);
    let y1 = (ay + ah).max(by + bh);
    Some(teksilo_canvas::Rect::new(
        x0,
        y0,
        (x1 - x0).max(1.0),
        (y1 - y0).max(ah.max(bh)),
    ))
}

/// The **window-space** rectangle enclosing an arbitrary character range
/// `[start, end)` — the union of the caret rects at its two ends, in the same
/// absolute space [`caret_window_rect`] answers in.
///
/// (This paragraph used to sit above [`range_content_rect`], which has its own:
/// two doc comments in a row, the first one describing the function below the
/// second.)
///
/// Unlike [`caret_window_rect`], this takes explicit offsets (not the live cursor) and does
/// **not** require focus — it exists so a search match can be scrolled into view whether or
/// not the editor is focused. `None` before the first full layout.
pub(super) fn range_window_rect(
    st: &EditorState,
    start: usize,
    end: usize,
) -> Option<teksilo_canvas::Rect> {
    if !st.engine.has_full_layout() {
        return None;
    }
    // As in `caret_window_rect`: the engine's `y` is already screen-space and
    // its `x` is not, so only `x` takes a scroll subtraction here.
    let slip = vertical_scroll_slip(st);
    let to_window = |c: [f32; 4]| {
        (
            st.viewport_origin.x + c[0] - st.scroll_x.get(),
            st.viewport_origin.y + c[1] + slip,
            c[3].max(1.0),
        )
    };
    // A **fixed** affinity, not the live cursor's: these are arbitrary offsets unrelated to
    // the caret, and reveal only needs a stable line — using `st.cursor_affinity` would place
    // the rect on the wrong side of a soft-wrap boundary depending on where the caret happens
    // to sit.
    let (ax, ay, ah) = to_window(st.engine.caret_rect(start, CursorAffinity::Downstream));
    let (bx, by, bh) = to_window(st.engine.caret_rect(end, CursorAffinity::Downstream));

    let x0 = ax.min(bx);
    let y0 = ay.min(by);
    let x1 = ax.max(bx);
    let y1 = (ay + ah).max(by + bh);
    Some(teksilo_canvas::Rect::new(
        x0,
        y0,
        (x1 - x0).max(1.0),
        (y1 - y0).max(ah.max(bh)),
    ))
}

/// Report the caret's window-space rectangle to the platform so the OS IME
/// candidate window tracks the insertion point. No-op when unfocused or the
/// engine has not been laid out yet. Called whenever the caret moves.
pub(super) fn report_ime_cursor_area(state: &SharedState, ctx: &mut EventContext) {
    let area = {
        let st = state.borrow();
        match caret_window_rect(&st) {
            Some(a) => a,
            None => return,
        }
    };
    // Dedup against the last reported area. The platform-side
    // `WindowOps::set_ime_cursor_area` does *not* dedup (it assumes text
    // widgets only report on caret moves); re-forwarding an unchanged area is
    // wasted work and, on some winit IME backends (ibus / fcitx), echoes back
    // a fresh empty `Ime::Preedit`, sustaining a feedback loop. Only forward a
    // genuinely new position.
    {
        let mut st = state.borrow_mut();
        if st.last_ime_area == Some(area) {
            return;
        }
        st.last_ime_area = Some(area);
    }
    ctx.set_ime_cursor_area(area);
}

/// After the caret moves, ask any enclosing scroll area (a form/page the
/// editor is embedded in) to reveal the caret. The editor already keeps the
/// caret inside its *own* viewport via [`ensure_caret_visible`]; this adds the
/// outer chaining so that when the editor **grows** inside a scrolling page
/// (its own scroll suppressed) the page follows the caret. Framework-side,
/// `ctx.ensure_visible` walks only the editor's ancestors (never the editor
/// itself), so this never fights the internal follow.
///
/// Fires only on a caret *move* — a plain wheel / scrollbar scroll never
/// triggers it, so the reader can scroll away from the caret and the view
/// stays put until the caret next moves. A zero margin means it scrolls only
/// when the caret is genuinely off-screen (no nudging when it is already
/// visible). Off entirely when [`follow_caret_in_page`](EditorState::follow_caret_in_page)
/// is disabled, unfocused, not yet laid out, or with no enclosing scroll area.
///
/// The revealed rect is the caret line **plus one line below it**. Beyond a
/// mild always-a-line-of-context-under-the-caret scroll-off, this proactively
/// reveals the line a wrapping character lands on: typed input is applied to
/// the document a frame later by `frame_loop::tick` (which has no
/// `EventContext` to re-chase), so this chase sees the caret's *pre-insert*
/// line — the look-ahead keeps the post-wrap line on screen without waiting for
/// the next keystroke. Structural edits (Enter/Backspace/Delete) mutate the
/// cursor synchronously and are exact.
pub(super) fn chase_caret_into_view(state: &SharedState, ctx: &mut EventContext) {
    chase_caret_into_view_with(state, ctx, ScrollMotion::Instant);
}

/// [`chase_caret_into_view`] with an explicit scroll motion for the typewriter
/// pin.
///
/// Only the pin reads it — a minimal reveal is always instant, because it fires
/// on ordinary caret movement where an animation would just add lag. The pin,
/// by contrast, re-asserts on **every** keystroke, so animating it is what
/// produces the "screen bouncing" complaint typewriter modes attract in other
/// editors: hence [`ScrollMotion::Instant`] for typing and character-level
/// navigation, and [`ScrollMotion::Smooth`] reserved for the deliberate jumps
/// (page up/down, a search hit) where a glide reads as orientation rather than
/// noise.
pub(super) fn chase_caret_into_view_with(
    state: &SharedState,
    ctx: &mut EventContext,
    motion: ScrollMotion,
) {
    let (area, pin, link, visible_height) = {
        let mut st = state.borrow_mut();
        if !st.follow_caret_in_page {
            return;
        }
        // The pin stands down while the pointer owns the caret: a click just
        // places the caret and *becomes* the new resting position, and a
        // drag-selection is never interrupted by a scroll (which would make
        // precise selection nearly impossible — a real, filed bug in editors
        // that re-centre on pointer input).
        let pin = match st.typewriter {
            Some(f)
                if !st.mouse_anchored
                    && !matches!(st.drag_state, super::state::DragState::Selecting { .. }) =>
            {
                Some(f)
            }
            _ => None,
        };

        let pos = st.cursor.position();
        let rect = match caret_window_rect(&st) {
            Some(a) => a,
            None => return,
        };
        let link = link_stop_span(&st);
        let visible_height = st.visible_height;

        // Reveal the caret only when it actually MOVES. A repeat call at the
        // same document position — IME preedit churn (Linux ibus/fcitx floods
        // empty `Ime::Preedit` events), a no-op nav key, a redundant click —
        // must not scroll: once the user has deliberately scrolled the caret
        // off-screen, a same-position chase would yank the page straight back,
        // fighting the scroll. `frame_loop::tick` applies typed input a frame
        // later, so within a fast burst several keystrokes share one pre-insert
        // position; the one-line look-ahead below covers that batch, and the
        // next real move re-chases.
        //
        // A pin additionally compares the caret's window `y`, because it must
        // survive a reflow that moves the caret's *rect* without moving its
        // offset — a soft-wrap change, a typography or zoom change, a resize.
        // Deduping a pin on position alone lets it drift off its line and stay
        // there, which is the shape of the zoom-dependent jitter bugs that
        // dog other implementations.
        let same_pos = st.last_chase_pos == Some(pos);
        if pin.is_some() {
            let same_y = st
                .last_chase_y
                .is_some_and(|y| (y - rect.y).abs() <= f32::EPSILON);
            if same_pos && same_y {
                return;
            }
            st.last_chase_y = Some(rect.y);
        } else {
            if same_pos {
                return;
            }
            st.last_chase_y = None;
        }
        st.last_chase_pos = Some(pos);
        (rect, pin, link, visible_height)
    };

    match pin {
        // The bare caret rect, with no look-ahead: alignment is computed from
        // the target's height, so padding it by a line would shift the pin by
        // half a line and put the text permanently off the mark.
        Some(fraction) => ctx.ensure_visible_aligned(area, fraction, motion),
        None => ctx.ensure_visible(page_follow_area(area, link, visible_height)),
    }
}

/// The window-space span, top to bottom, of the link Tab selected, when the
/// selection is still that link stop.
///
/// A link a Tab selected is revealed whole, from its first line to its last,
/// not only the caret's line at its end: Shift+Tab up to a link that wraps
/// left its first lines above the page's viewport. The editor's own viewport
/// gets the same from [`select_link_stop_locked`]. Only the part of the link
/// inside that viewport: lines the editor scrolled out of its own view are not
/// on the page either, and revealing them scrolled the page to whatever sits
/// above the editor.
fn link_stop_span(st: &EditorState) -> Option<(f32, f32)> {
    st.link_stop
        .filter(|&stop| {
            let (a, b) = st.shown_selection();
            (a.min(b), a.max(b)) == stop
        })
        .and_then(|(start, end)| range_window_rect(st, start, end))
        .map(|link| {
            let top = link.y.max(st.viewport_origin.y);
            let bottom = (link.y + link.height)
                .min(st.viewport_origin.y + st.viewport_height)
                .max(top);
            (top, bottom)
        })
}

/// What the page follow shows of a caret at `caret` (window space) with no
/// typewriter pin: its line and the next one, and the `link` span
/// ([`link_stop_span`]) when there is one.
fn page_follow_area(
    caret: teksilo_canvas::Rect,
    link: Option<(f32, f32)>,
    visible_height: Option<f32>,
) -> teksilo_canvas::Rect {
    // Extend downward by one line so the line a wrapping character would land
    // on is revealed together with the current caret line.
    let line = caret.height.max(1.0);
    let mut area = teksilo_canvas::Rect::new(caret.x, caret.y, caret.width, caret.height + line);
    // The link's lines with the caret's (horizontally, the caret's alone),
    // unless together they do not fit the page's view: the page aligns the top
    // of a target above its viewport (reached by Shift+Tab), so a link taller
    // than that view would leave the caret below it.
    if let Some((top, bottom)) = link {
        let top = top.min(area.y);
        let bottom = bottom.max(area.y + area.height);
        if visible_height.is_none_or(|height| bottom - top <= height) {
            area = teksilo_canvas::Rect::new(area.x, top, area.width, bottom - top);
        }
    }
    area
}

/// The rectangle the framework reveals in an enclosing scroll area when the
/// editor takes focus: the caret's line, or, when the selection is a link
/// stop an assistive `Focus` made before the editor had focus, what Tab's
/// page follow shows of that link ([`page_follow_area`]). `None` before the
/// first layout or while unfocused.
pub(super) fn focus_reveal_window_rect(st: &EditorState) -> Option<teksilo_canvas::Rect> {
    let caret = caret_window_rect(st)?;
    Some(match link_stop_span(st) {
        Some(link) => page_follow_area(caret, Some(link), st.visible_height),
        None => caret,
    })
}

/// Ctrl+A escalation ladder. Mirrors the godot reference's `select_all`.
/// When the caret sits inside a table cell, four consecutive presses
/// widen the selection: paragraph -> cell -> table -> document. The document
/// selection leaves the caret at the document's end, so the fifth press starts
/// the ladder again at the paragraph only when that end is inside a table; at
/// the end of a document that finishes outside one it selects the document
/// again. Outside a table Ctrl+A is single-shot `SelectionType::Document`
/// with `select_all_level = 0`.
///
/// The ladder reads the caret's cell again at every press. On
/// text-document 1.12.3 and later `current_table_cell()` answers with the
/// caret's cell at a block edge too, which is where level 1's
/// `select(BlockUnderCursor)` leaves the position, and levels 2 and 3 leave
/// position and anchor where level 1 put them. Nothing is remembered between
/// presses, so a table the application removed in between ends the ladder
/// at the document instead of aiming at a table that is gone.
pub(super) fn apply_select_all_ladder(st: &mut EditorState) {
    let next_level = st.select_all_level + 1;

    let Some(cell) = st.cursor.current_table_cell() else {
        st.cursor.select(SelectionType::Document);
        st.select_all_level = 0;
        return;
    };
    let (table_id, row, column) = (cell.table.id(), cell.row, cell.column);
    let (table_rows, table_columns) = (cell.table.rows(), cell.table.columns());

    match next_level {
        1 => st.cursor.select(SelectionType::BlockUnderCursor),
        2 => st.select_table_cell(table_id, row, column),
        3 => {
            st.select_cell_range(
                table_id,
                0,
                0,
                table_rows.saturating_sub(1),
                table_columns.saturating_sub(1),
            );
        }
        _ => {
            st.cursor.select(SelectionType::Document);
        }
    }
    st.select_all_level = if next_level >= 4 { 0 } else { next_level };
}

/// Enter: a paragraph break at the caret, as LibreOffice Writer makes it.
///
/// * At the end of a heading (an empty one included), the new paragraph is a
///   body paragraph: Writer gives it the heading style's next style, "Text
///   Body". Only the heading level goes. Writer keeps the paragraph's direct
///   attributes on the new paragraph (alignment, indentation) and resets list
///   attributes only where the style change takes a list style away, which a
///   list here, always a direct attribute of the paragraph, never comes from;
///   so alignment, indentation and list membership carry on as `insert_block`
///   leaves them (`SwTextNode::MakeNewTextNode` and `lcl_ResetParAttrs` in
///   Writer's `sw/source/core/txtnode/ndtxt.cxx`).
/// * In the middle of a heading, both halves stay headings, and at the start
///   of a non-empty one the empty paragraph above is a heading too, the text
///   staying one: Writer applies the next style only at a paragraph's end.
///
/// The break and the level change are one undo step. The caret's character
/// format goes on in the new paragraph (see `typing_format::insert_paragraph`).
fn split_paragraph(st: &mut EditorState) {
    let ends_heading = !st.cursor.has_selection()
        && st.cursor.at_block_end()
        && st
            .cursor
            .block_format()
            .is_ok_and(|format| format.heading_level.is_some_and(|level| level > 0));
    super::typing_format::insert_paragraph(st, |st| {
        st.cursor.begin_edit_block();
        if st.cursor.insert_block().is_ok() && ends_heading {
            let _ = st.cursor.set_block_format(&BlockFormat {
                heading_level: Some(0),
                ..Default::default()
            });
        }
        st.cursor.end_edit_block();
    });
}

/// Which `TextFormat` bit a Ctrl+B/I/U toggle flips.
#[derive(Copy, Clone)]
enum FormatBit {
    Bold,
    Italic,
    Underline,
}

/// Toggle a single character-format bit at the caret, mirroring the
/// godot reference's `toggle_bold` / `toggle_italic` / `toggle_underline`:
/// the decision to turn a format on or off is read from the current caret
/// format (`char_format()`), not from a selection-wide consensus.
///
/// **Read-position subtlety**: `TextCursor::char_format()` reads
/// the inline element at `position()`. After a select-all the
/// caret sits at the *end* of the selection, which may be past the
/// last character (an empty "virtual" element with default format).
/// To get a meaningful read for the toggle decision we use
/// `selection_start()` when a selection is active — that position
/// is always the actual first character of the selected range.
///
/// On a collapsed caret the toggle is kept for the text typed next there,
/// flipped from the format that text would have had (see
/// [`super::typing_format`]).
fn toggle_char_format(st: &mut EditorState, bit: FormatBit) {
    if !st.cursor.has_selection() {
        let toggle = match bit {
            FormatBit::Bold => FormatToggle::Bold,
            FormatBit::Italic => FormatToggle::Italic,
            FormatBit::Underline => FormatToggle::Underline,
        };
        super::typing_format::toggle(st, toggle);
        return;
    }
    let probe = st.document.cursor();
    probe.set_position(st.cursor.selection_start(), MoveMode::MoveAnchor);
    let current = probe.char_format().unwrap_or_default();
    let new_value = !match bit {
        FormatBit::Bold => current.font_bold.unwrap_or(false),
        FormatBit::Italic => current.font_italic.unwrap_or(false),
        FormatBit::Underline => current.font_underline.unwrap_or(false),
    };
    let fmt = match bit {
        FormatBit::Bold => TextFormat {
            font_bold: Some(new_value),
            ..Default::default()
        },
        FormatBit::Italic => TextFormat {
            font_italic: Some(new_value),
            ..Default::default()
        },
        FormatBit::Underline => TextFormat {
            font_underline: Some(new_value),
            ..Default::default()
        },
    };
    let _ = st.cursor.merge_char_format(&fmt);
    st.pending_format_changed = true;
}

/// Apply an `ImeComposition` event: show its text as the composition, a
/// transient edit no undo history records (see [`crate::common::composition`]).
/// Each update replaces the text of the one before, so the document always
/// reflects the input method's current state.
///
/// A first update that names a `replacement` (the input method marked text
/// already in the document, as Korean Hanja conversion does) shows the
/// composition in place of that text, which a cancel puts back with its
/// formats. The empty update leaves the composition's text where it is: winit
/// sends one right before every commit too, so the commit that follows decides
/// (see [`commit_composition`]), and anything else cancels it (see
/// [`settle_cancelled_composition`]).
fn handle_ime_composition(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
    cursor: Option<&std::ops::Range<usize>>,
    replacement: Option<&std::ops::Range<usize>>,
) -> EventResponse {
    let filter = state.borrow().policy.command_filter;
    if !filter.accepts(EditCommandKind::InsertChar) {
        return EventResponse::Ignored;
    }
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    if clean.is_empty() {
        // The empty update of a live composition: leave its text where it is
        // until the next event says whether it is committed. The frame asked
        // for here settles a cancel if nothing else does first.
        //
        // With no live composition it is a genuine no-op. Some Linux IME
        // backends (ibus / fcitx via winit) emit a continuous stream of empty
        // `Ime::Preedit("")` events while the field is focused; processing each
        // one (an IME-cursor-area report, a caret chase) is pure waste. Worse,
        // the report re-arms the very loop that produced the event (see
        // `report_ime_cursor_area`), and the chase fights the user's scroll.
        // Bail before touching the document, the platform, or the scroll. An
        // ended composition waits on: the stream must not cancel what the
        // commit after it would commit.
        let ended = {
            let mut guard = state.borrow_mut();
            let st = &mut *guard;
            st.composition.reconcile(&st.document);
            st.composition.end(&st.document)
        };
        if !ended {
            return EventResponse::Ignored;
        }
        ctx.request_frame();
        return EventResponse::Handled;
    }
    {
        let mut guard = state.borrow_mut();
        let st = &mut *guard;
        // An update after an earlier composition's empty one, with no commit in
        // between: that composition was cancelled.
        settle_cancelled_composition(st);
        // The preedit goes in after what was typed before it in this frame. A
        // recorded insertion, which would revert a live composition: there is
        // none, unless keys typed during it, and it is then started again.
        super::frame_loop::flush_pending_chars(st);
        st.composition.reconcile(&st.document);
        if !st.composition.is_live(&st.document) {
            st.composition.begin(&st.document);
            // The first update of a composition that takes the place of text
            // already in the document. Later updates name the same text, which
            // the composition now stands in for: they replace its text.
            if let Some(range) = replacement {
                // Paragraph breaks included, as in the range: `character_count`
                // leaves them out, and cut a range in a later paragraph short.
                let doc_end = super::input_document::document_end(st);
                let start = range.start.min(doc_end);
                let end = range.end.min(doc_end).max(start);
                if filter.collapses_selection_before_insert() {
                    // A forward-only surface removes nothing: the composition
                    // goes after the range, as typing over a selection does
                    // there.
                    st.cursor.set_position(end, MoveMode::MoveAnchor);
                } else {
                    st.cursor.set_position(start, MoveMode::MoveAnchor);
                    st.cursor.set_position(end, MoveMode::KeepAnchor);
                }
            } else {
                // A composition starting while a selection is live takes its
                // place, as typing does, unless the filter forbids taking text
                // away.
                collapse_selection_before_insert(st);
            }
        }
        // In the format toggled on the caret before the composition, if any:
        // the commit gives it to the text it records.
        let typing = super::typing_format::pending(st).cloned();
        if st
            .composition
            .show_in(&st.document, &st.cursor, &clean, typing.as_ref())
        {
            // Which part the input method is on: the segment it converts, or
            // the caret's place inside the composition.
            let kept: Vec<bool> = text.chars().map(|c| !c.is_control()).collect();
            st.composition.mark(text, cursor, &kept);
            st.composition.place_caret(&st.document, &st.cursor);
        }
        st.preferred_x = None;
        // Editing always lands the caret at a logical position; any
        // sticky upstream affinity from a previous click is invalid
        // now. Render at the default downstream placement.
        st.cursor_affinity = CursorAffinity::Downstream;
        st.pending_text_changed = true;
        // Lay the preedit out now, not at the next frame: the area reported
        // below is where the platform opens its candidate window, and it is
        // read off the layout. Without the preedit in it, the caret's offset
        // after the preedit lies that many characters further into the old
        // text, and at the end of a paragraph that is the next paragraph, at
        // the editor's left edge. An input method that lists candidates from
        // the first key (Microsoft Pinyin) opened them there; one that lists
        // them only on conversion, once a frame has laid the reading out,
        // happened to land close to the preedit.
        super::frame_loop::catch_up_layout(st);
    }
    sync_cursor_signals(state);
    report_ime_cursor_area(state, ctx);
    chase_caret_into_view(state, ctx);
    ctx.request_frame();
    EventResponse::Handled
}

/// Cancel a composition the input method ended with its empty update and did
/// not commit: the document back as it was before it, the text it took the
/// place of back with its formats, nothing recorded. Called with whatever
/// comes instead of the commit (a key, another composition, a press, a focus
/// change) and at the start of every frame. Returns whether it changed the
/// document.
pub(super) fn settle_cancelled_composition(st: &mut EditorState) -> bool {
    st.composition.reconcile(&st.document);
    if !st.composition.is_ended(&st.document) {
        return false;
    }
    let changed = st.composition.cancel(&st.document);
    st.preferred_x = None;
    st.cursor_affinity = CursorAffinity::Downstream;
    st.pending_text_changed = true;
    changed
}

/// The commit of a composition: the committed text takes the composition's
/// place, and the place of the text it stood in for, as one undo step of its
/// own (see [`Composition::commit`](crate::common::composition::Composition::commit)).
/// Reported under `source`. An empty commit commits nothing: the composition
/// ends as a cancel.
fn commit_composition(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
    source: EditSource,
) -> EventResponse {
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    {
        let mut guard = state.borrow_mut();
        let st = &mut *guard;
        if !st
            .policy
            .command_filter
            .accepts(EditCommandKind::InsertChar)
        {
            // The editor stopped taking text while the composition was shown.
            st.composition.cancel(&st.document);
            return EventResponse::Ignored;
        }
        // In the format toggled on the caret before the composition, if any.
        let mut arrived = 0;
        super::typing_format::insert_with(st, |st| {
            arrived = st
                .composition
                .commit(&st.document, &st.cursor, &clean, true);
            arrived
        });
        st.report_inserted_chars(source, arrived);
        st.preferred_x = None;
        st.cursor_affinity = CursorAffinity::Downstream;
        st.pending_text_changed = true;
        // A press may follow before the next frame (the Microsoft Korean IME
        // commits its syllable on the mouse-down itself), and it hit-tests the
        // layout: bring it up to date, as the area reported below needs too.
        super::frame_loop::catch_up_layout(st);
    }
    sync_cursor_signals(state);
    report_ime_cursor_area(state, ctx);
    chase_caret_into_view(state, ctx);
    ctx.request_frame();
    EventResponse::Handled
}

/// Before a pointer moves the caret or starts a selection (a press, a tap's
/// release, a hold, a multi-tap): the typing and the input method's commit
/// batched for this frame go in where they were typed and are laid out (see
/// `frame_loop::insert_typing_before_pointer`), then a composition in progress
/// ends with its text kept (see [`finish_composition`]).
pub(super) fn settle_before_press(state: &SharedState, ctx: &mut EventContext) {
    if super::frame_loop::insert_typing_before_pointer(&mut state.borrow_mut()) {
        sync_cursor_signals(state);
        ctx.request_frame();
    }
    finish_composition(state, ctx);
}

/// End the composition the input method still holds, keeping it as committed
/// text, for a press in the editor or a focus change: what a native text view
/// does, rather than leave the composition to be committed later at its old
/// place (or in the next field focused).
///
/// The platform is told first ([`EventContext::finish_ime_composition`]),
/// while the composition is still live, and answers with what the input
/// method committed for it, if anything: on Windows the input method
/// completes it, and its commit can differ from what it showed (Microsoft
/// Pinyin commits "nihao" for "ni'hao"). That text is kept, or the text the
/// composition showed when there is none. It goes through what a commit does:
/// one undo step that takes it back out, reported to `on_text_inserted` as
/// [`EditSource::Ime`]. What the platform still delivers for the composition
/// (an empty update, an empty commit) arrives after this handler returns and
/// finds no composition to change. A composition the input method had already
/// ended (its empty update, no commit) is settled as the cancel it was, and
/// the platform is not told. With no composition, does nothing.
pub(super) fn finish_composition(state: &SharedState, ctx: &mut EventContext) {
    let live = {
        let mut guard = state.borrow_mut();
        let st = &mut *guard;
        if settle_cancelled_composition(st) {
            false
        } else if st.composition.is_live(&st.document) || st.composition.is_written(&st.document) {
            // A written composition is told even before anything was written
            // in it: the text service opened it and holds it until told.
            true
        } else {
            return;
        }
    };
    let committed = if live {
        ctx.finish_ime_composition()
    } else {
        None
    };
    {
        let mut guard = state.borrow_mut();
        let st = &mut *guard;
        if live {
            keep_composition(st, committed.as_deref());
        }
        // The press that follows hit-tests the layout: bring it up to date.
        super::frame_loop::catch_up_layout(st);
    }
    sync_cursor_signals(state);
    ctx.request_frame();
}

/// Commit the live composition where it stands, as its own commit would,
/// reported as [`EditSource::Ime`]: as `committed`, the input method's commit
/// for it, cleaned as a commit is, or as the text it showed when there is none
/// or nothing of it is left. The caret ends after the text, unless it stood
/// outside the composition (a right-click put it there before the menu took
/// focus), where it stays.
fn keep_composition(st: &mut EditorState, committed: Option<&str>) {
    let caret = (st.cursor.anchor(), st.cursor.position());
    let range = st.composition.range(&st.document);
    let outside = range.as_ref().is_some_and(|range| {
        let inside = |at: usize| range.start <= at && at <= range.end;
        !inside(caret.0) || !inside(caret.1)
    });
    // What the composition shows: a written one's text is the document's
    // over the transient edits (the text service's, which it was asked to
    // end and did not).
    let shown = if st.composition.is_written(&st.document) {
        st.composition.written_text(&st.document)
    } else {
        st.composition.text(&st.document).map(str::to_owned)
    };
    let Some(shown) = shown else {
        return;
    };
    let text = committed
        .map(|text| text.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|text| !text.is_empty())
        .unwrap_or(shown);
    let mut arrived = 0;
    super::typing_format::insert_with(st, |st| {
        arrived = st.composition.commit(&st.document, &st.cursor, &text, true);
        arrived
    });
    st.report_inserted_chars(EditSource::Ime, arrived);
    // A caret after the composition moves with its end, by the difference
    // between the committed text and the text shown.
    if outside && let Some(range) = range {
        let length = text.chars().count();
        let carried = |at| position_after_commit(at, &range, length);
        st.cursor
            .set_position(carried(caret.0), MoveMode::MoveAnchor);
        st.cursor
            .set_position(carried(caret.1), MoveMode::KeepAnchor);
    }
    st.preferred_x = None;
    st.cursor_affinity = CursorAffinity::Downstream;
    st.pending_text_changed = true;
}

/// Helper for IME-commit character ingestion: push the text
/// into `pending_chars`, clear sticky `preferred_x`, request a frame.
/// Called only from the IME commit path, which names the [`EditSource`] the
/// text is counted under.
fn push_pending_chars(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
    source: EditSource,
) -> EventResponse {
    if text.is_empty() {
        return EventResponse::Ignored;
    }
    let filter = state.borrow().policy.command_filter;
    if !filter.accepts(EditCommandKind::InsertChar) {
        return EventResponse::Ignored;
    }
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    if clean.is_empty() {
        return EventResponse::Ignored;
    }
    {
        let mut st = state.borrow_mut();
        // The **only** caller is the `ImeCommit` arm above, so what lands here
        // came from the input system rather than a keystroke: the characters
        // that arrive are not the keys that were pressed, and counting them as
        // typed would be wrong about every writer of a language with an input
        // method, and about everyone who dictates.
        st.pending_arrivals.note(source, clean.chars().count());
        st.pending_chars.push_str(&clean);
        st.preferred_x = None;
        // Typing always lands the caret at a logical position; any
        // sticky upstream affinity from a previous click is invalid.
        st.cursor_affinity = CursorAffinity::Downstream;
    }
    report_ime_cursor_area(state, ctx);
    // Printable input is batched into `pending_chars` and applied by
    // `frame_loop::tick` next frame, so this chase sees the caret at its
    // PRE-insert position; `chase_caret_into_view`'s one-line look-ahead
    // reveals the line a wrap would land on so the page keeps up without
    // waiting for the next keystroke.
    chase_caret_into_view(state, ctx);
    ctx.request_frame();
    EventResponse::Handled
}

/// Replace `range` with `text`, as the input system asked: a dictation
/// correction, a Writing Tools rewrite. One undo step, reported once under
/// `source`.
///
/// `range` is in document character offsets, the units of
/// `TextCursor::position`, and is clamped to the document: the input system
/// read the text a moment ago and may name a range an edit since has
/// shortened. The template is AccessKit's `ReplaceSelectedText` arm: the same
/// filter check, the typing batched ahead of it inserted first so it lands
/// before rather than after, and the text inserted as plain text, so a newline
/// becomes a paragraph. Under a forward-only filter the range is not removed:
/// the text lands after it, as typing over a selection does there.
fn replace_from_input_system(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
    range: std::ops::Range<usize>,
    source: EditSource,
) -> EventResponse {
    let filter = state.borrow().policy.command_filter;
    if !filter.accepts(EditCommandKind::InsertChar) {
        return EventResponse::Ignored;
    }
    {
        let mut st = state.borrow_mut();
        super::frame_loop::flush_pending_chars(&mut st);
        // The document's last offset, paragraph breaks included: the range was
        // read from text that has them. `character_count` leaves them out.
        let doc_end = super::input_document::document_end(&st);
        let start = range.start.min(doc_end);
        let end = range.end.min(doc_end).max(start);
        st.cursor.set_position(start, MoveMode::MoveAnchor);
        st.cursor.set_position(end, MoveMode::KeepAnchor);
        collapse_selection_before_insert(&st);
        if text.is_empty() {
            // Nothing to write, so the replacement is a deletion. Under a
            // forward-only filter the collapse above left nothing selected.
            if st.cursor.has_selection() {
                let _ = st.cursor.remove_selected_text();
            }
        } else {
            let arrived = clipboard::insert_plain_text(&st, text);
            st.report_inserted_chars(source, arrived);
        }
        st.preferred_x = None;
        st.cursor_affinity = CursorAffinity::Downstream;
        st.pending_text_changed = true;
    }
    sync_cursor_signals(state);
    report_ime_cursor_area(state, ctx);
    chase_caret_into_view(state, ctx);
    ctx.request_frame();
    EventResponse::Handled
}

/// Ask the typesetter to compute a scroll offset that keeps the
/// current caret inside the viewport, and write that offset into
/// the widget's `scroll_y` signal. Called only from keyboard
/// handlers (after arrow/page nav), never from the frame loop —
/// otherwise wheel scrolls that move the viewport away from the
/// caret would be undone on the next tick.
fn ensure_caret_visible(state: &SharedState) {
    let mut st = state.borrow_mut();
    ensure_caret_visible_locked(&mut st);
}

/// [`ensure_caret_visible`] for a caller that already holds the borrow — the
/// body's paint, which reveals the caret again after a viewport **shrink** and
/// has the state open around the relayout that shrink forced.
pub(super) fn ensure_caret_visible_locked(st: &mut EditorState) {
    // Not redundant with the helper's own check: the horizontal reveal below
    // needs a full layout too.
    if !st.engine.has_full_layout() {
        return;
    }
    // Name the position rather than letting the engine read its own cached
    // cursor: that cache is refreshed at paint, so inside a key handler it still
    // holds the caret from *before* the keystroke. Correcting against it revealed
    // the old caret and left the new one outside the viewport until the next key
    // press — which is what made walking with the arrows lurch a line behind.
    // Where the layout shows it, which may be the recorded document's.
    let pos = st.shown_caret();
    let affinity = st.cursor_affinity;
    ensure_position_visible_locked(st, pos, affinity);
    // Horizontal caret visibility — matches godot's `ensure_caret_h_visible`.
    // Margin is 20 logical pixels on each side so the caret doesn't sit flush
    // against the viewport edge.
    ensure_caret_h_visible_locked(st);
}

/// Scroll the editor's own viewport the least that shows the line at
/// `position` (in the text the view lays out), as [`ensure_caret_visible`]
/// does for the caret.
fn ensure_position_visible_locked(st: &mut EditorState, position: usize, affinity: CursorAffinity) {
    if !st.engine.has_full_layout() {
        return;
    }
    // Forward the current wheel-driven scroll so the correction is computed
    // relative to where the viewport actually is, not where it was at last
    // paint.
    let current = st.scroll_y.get();
    st.engine.set_scroll_offset(current);
    if let Some(new_off) = st.engine.ensure_position_visible(position, affinity) {
        st.scroll_y.set(new_off);
    }
}

/// Horizontal caret-visibility. Factored out so code paths that
/// need it without the vertical adjustment (e.g. after `Home` on a
/// very long wrapped block) can call it directly.
///
/// Takes `&mut EditorState` (caller holds the borrow) rather than
/// `&SharedState` because the current call sites are inside the
/// `ensure_caret_visible` borrow.
fn ensure_caret_h_visible_locked(st: &mut EditorState) {
    if !st.engine.has_full_layout() {
        return;
    }
    // When wrap mode is Word, there is no horizontal overflow: the
    // engine wraps content to the viewport width and `max_scroll_x`
    // is pinned at 0. Nothing to adjust.
    let max_x = st.max_scroll_x.get();
    if max_x <= 0.0 {
        return;
    }
    let viewport_w = st.viewport_width;
    if viewport_w <= 0.0 {
        return;
    }

    let pos = st.shown_caret();
    let caret = st.engine.caret_rect(pos, st.cursor_affinity);
    // The engine's `x` is content-space — it applies no horizontal scroll of its
    // own — so the viewport-relative position is this minus the widget's own
    // `scroll_x`. (Its `y`, confusingly, *is* screen-space; see
    // `RichTextEngine::caret_rect`.)
    let caret_x = caret[0];
    let current_x = st.scroll_x.get();
    // Margin: 20 px on each side, matching godot's
    // `ensure_caret_h_visible`.
    let margin = 20.0_f32;
    let screen_x = caret_x - current_x;

    let new_x = if screen_x < margin {
        (caret_x - margin).max(0.0)
    } else if screen_x > viewport_w - margin {
        (caret_x - viewport_w + margin).clamp(0.0, max_x)
    } else {
        return;
    };
    st.scroll_x.set(new_x.clamp(0.0, max_x));
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum LineEdge {
    Start,
    End,
}

/// The way the user pressed, in screen terms.
#[derive(Clone, Copy, PartialEq, Eq)]
enum VisualDirection {
    Left,
    Right,
}

/// The logical move that carries the caret one step in the direction the
/// user actually pressed.
///
/// text-document's `MoveOperation::Left`/`Right` are *logical* aliases —
/// `Left` is "previous character", `Right` is "next character" — which
/// is correct for left-to-right text and backwards for right-to-left,
/// where the next character is drawn further left. Pressing → inside an
/// Arabic word therefore walked the caret leftward across the screen.
///
/// The direction is taken at the caret rather than from the paragraph,
/// so an English quotation inside Arabic prose steps the way it reads.
/// `resolve_move` stays pure logical stepping; choosing *which* logical
/// primitive to invoke is the widget layer's job, because only it knows
/// how the text was laid out.
fn visual_move(st: &EditorState, pressed: VisualDirection, step: CaretStep) -> MoveOperation {
    horizontal_op(moves_forward(st, pressed), step)
}

/// The stepping primitive for a horizontal motion already resolved to a
/// direction. Split from [`visual_move`] so the table is testable without an
/// `EditorState` — and, more to the point, on a host whose platform never
/// produces `CaretStep::LineEdge`.
fn horizontal_op(forward: bool, step: CaretStep) -> MoveOperation {
    match (forward, step) {
        (true, CaretStep::Word) => MoveOperation::WordRight,
        (false, CaretStep::Word) => MoveOperation::WordLeft,
        // `LineEdge` never reaches here — the caller routes it to
        // `move_cursor_to_line_edge`, which needs the line's extent rather
        // than a stepping primitive. Treat it as a character step so a future
        // caller that forgets still moves the caret somewhere sane.
        (true, _) => MoveOperation::Right,
        (false, _) => MoveOperation::Left,
    }
}

/// The whole-document / whole-block move a vertical arrow resolves to, or
/// `None` when it is the ordinary one-visual-line step (which needs the
/// typesetter, not a logical primitive).
fn vertical_op(step: LineStep, downward: bool) -> Option<MoveOperation> {
    match (step, downward) {
        (LineStep::Document, false) => Some(MoveOperation::Start),
        (LineStep::Document, true) => Some(MoveOperation::End),
        (LineStep::Paragraph, false) => Some(MoveOperation::StartOfBlock),
        (LineStep::Paragraph, true) => Some(MoveOperation::EndOfBlock),
        (LineStep::Line, _) => None,
    }
}

/// Which *logical* line edge the user pressed towards.
///
/// `move_cursor_to_line_edge` speaks in logical edges, deliberately, so Home
/// and End land on their own ends in right-to-left text. macOS's ⌘←/⌘→ are
/// visual (`moveToLeftEndOfLine:`), so the pressed direction has to be folded
/// through the caret's direction the same way [`visual_move`] folds character
/// and word steps — otherwise ⌘← and the ← next to it would disagree about
/// which way "left" is inside an Arabic paragraph.
fn visual_line_edge(st: &EditorState, pressed: VisualDirection) -> LineEdge {
    line_edge_op(moves_forward(st, pressed))
}

/// The logical line edge reached by a horizontal motion already resolved to a
/// direction. Split for the same reason as [`horizontal_op`].
fn line_edge_op(forward: bool) -> LineEdge {
    if forward {
        LineEdge::End
    } else {
        LineEdge::Start
    }
}

/// Whether a press in `pressed` walks *forward* through the text, given the
/// direction of the run the caret sits in.
///
/// Taken at the caret rather than from the paragraph, so an English quotation
/// inside Arabic prose steps the way it reads.
fn moves_forward(st: &EditorState, pressed: VisualDirection) -> bool {
    let rtl = st.engine.direction_at(st.shown_caret()) == TextDirection::RightToLeft;
    match pressed {
        VisualDirection::Right => !rtl,
        VisualDirection::Left => rtl,
    }
}

/// Left or Right where the press is answered without a step, which is when it
/// returns `true`.
///
/// With a selection, a character step goes to the selection's end in that
/// direction, and no further: the selected text was the point, and stepping from
/// the caret left the caret one character past it, or inside it.
///
/// Forward from the end of a wrapped line, where End puts the caret, is the same
/// position as the start of the next line. The press crosses to that start, as
/// it does from the character before, or the next line's first character could
/// not be reached by walking.
fn settle_without_stepping(
    st: &mut EditorState,
    pressed: VisualDirection,
    step: CaretStep,
) -> bool {
    if step != CaretStep::Character {
        return false;
    }
    let forward = moves_forward(st, pressed);
    if st.cursor.has_selection() {
        let target = if forward {
            st.cursor.selection_end()
        } else {
            st.cursor.selection_start()
        };
        st.cursor.set_position(target, MoveMode::MoveAnchor);
        st.cursor_affinity = affinity_after_step(&st.engine, st.shown_caret());
        return true;
    }
    if forward
        && st.cursor_affinity == CursorAffinity::Downstream
        && is_soft_wrap(&st.engine, st.shown_caret())
    {
        st.cursor_affinity = CursorAffinity::Upstream;
        return true;
    }
    false
}

/// Pick which side of a soft wrap or a direction seam the caret belongs to
/// after a horizontal move.
///
/// On a wrap it is the start of the next line, from either direction: that is
/// where the next character is, and a step from it leaves the line behind. At
/// the end of the line above it would have skipped the first character of this
/// one.
///
/// Where an LTR run meets an RTL one, a single character position has
/// two visual homes on the same line — often at opposite ends of it —
/// and only affinity says which. Landing on a seam without choosing
/// leaves the caret wherever the previous keystroke's affinity happened
/// to put it, which reads as the caret teleporting across the line.
///
/// The rule is that the caret stays on the side the user came *from*:
/// arriving from the left it attaches to the text before the offset
/// (downstream), arriving from the right to the text after it
/// (upstream).
fn settle_affinity_after_step(st: &mut EditorState, pressed: VisualDirection) {
    let pos = st.shown_caret();
    if is_soft_wrap(&st.engine, pos) {
        st.cursor_affinity = CursorAffinity::Upstream;
        return;
    }
    if !st.engine.is_direction_boundary_at(pos) {
        st.cursor_affinity = CursorAffinity::Downstream;
        return;
    }
    st.cursor_affinity = match pressed {
        VisualDirection::Right => CursorAffinity::Downstream,
        VisualDirection::Left => CursorAffinity::Upstream,
    };
}

/// Move the cursor to the start or end of the current visual line,
/// asking the typesetter for the line's logical extent. Solves two bugs
/// at once:
///  * A second End press after landing at line end is a no-op,
///    avoiding text-document's block-boundary ambiguity where
///    `get_block_at_position(block_end_pos)` returns the next block.
///  * Wrapped blocks stop at the wrap point (the standard editor
///    Home/End semantics).
///
/// Affinity handling: read the current `st.cursor_affinity` so the
/// range query picks the line the user is visually on (matters at
/// soft-wrap boundaries), then set it to match the edge just moved to —
/// Upstream when Home lands on a wrap continuation, Downstream
/// otherwise.
fn move_cursor_to_line_edge(st: &mut EditorState, edge: LineEdge, mode: MoveMode) {
    if !st.engine.has_full_layout() {
        return;
    }
    // The layout's positions, which are the recorded document's in a view that
    // does not show the transient edits (see `EditorState::shows_transient_edits`):
    // the line is found in them, and the edge mapped back into the document.
    let pos = st.shown_caret();

    // Ask the typesetter for the line's logical extent rather than
    // hit-testing a far-off-screen x. The probe used to be purely
    // visual — leftmost for Home, rightmost for End — which is only the
    // same thing in left-to-right text; in an RTL paragraph the logical
    // start is drawn on the right, so Home and End landed on each
    // other's ends. Asking for the range directly sidesteps both that
    // and the question of how a hit-test clamps coordinates far outside
    // the text.
    let Some((line_start, line_end)) = st.engine.visual_line_range_at(pos, st.cursor_affinity)
    else {
        return;
    };

    let target = match edge {
        LineEdge::Start => line_start,
        LineEdge::End => line_end,
    };
    let shown_target = target;
    let target = st.document_position(target);
    if target != st.cursor.position() && !move_moving_end(st, target, mode) {
        // The step gave back a table the selection held, and the caret is not
        // on this line at all.
        st.cursor_affinity = CursorAffinity::Downstream;
        return;
    }

    // Keep the caret drawn on the line it was sent to. Only a soft-wrap
    // boundary — a position that both ends one line and starts the next
    // — has two placements to choose between; anywhere else affinity is
    // a no-op and the convention is the `Downstream` default.
    //
    // End always wants Downstream (the end of the line it just moved
    // to). Home wants Upstream, but only where that means something: at
    // a block's first line, position 0 starts a line without ending one,
    // and claiming Upstream there would report a wrap continuation that
    // does not exist.
    let starts_a_wrap_continuation = |st: &EditorState, at: usize| {
        matches!(
            st.engine.visual_line_range_at(at, CursorAffinity::Downstream),
            Some((_, end)) if end == at
        )
    };
    st.cursor_affinity = match edge {
        LineEdge::Start if starts_a_wrap_continuation(st, shown_target) => CursorAffinity::Upstream,
        LineEdge::Start | LineEdge::End => CursorAffinity::Downstream,
    };
}

/// Move the cursor up or down by one visual line, using the
/// typesetter's layout and `caret_rect` for the source position and
/// `hit_test` at the target Y to find the position on the next line.
/// Uses a sticky `preferred_x` so repeated vertical presses stay on
/// the same visual column even across short lines.
///
/// Called from `handle_key` with `state.borrow_mut()` already held.
///
/// Affinity: source `caret_rect` is queried with the current affinity
/// so vertical motion starts from the visually-correct line; the new
/// position's affinity is read off the hit-test (which sets Upstream
/// when the matched line's start coincides with the previous line's
/// end).
/// The vertical midpoint of the visual line one step `direction` from the one holding `pos`, in
/// document space — the y a hit test should probe to land on the neighbouring line.
///
/// **Not the caret's own height.** `caret_rect`'s height is the caret: ascent plus descent of
/// the text it sits in. The distance to the next line is the line's *advance*, which is larger
/// whenever the paragraph sets any line spacing above 1.0 — as prose typically does. Stepping by
/// the caret height then lands inside the line you started on, and the hit test hands back the
/// position you already had.
///
/// That failed asymmetrically, which is what made it easy to miss: moving **up** from the lower
/// part of a tall line still crosses into the previous line, so Up kept working while Down, from
/// the top of a line, never reached the next one and did nothing at all.
///
/// Derived from the real line geometry instead: the neighbouring line is found through the
/// character just past (or before) the current visual line, and its caret rect gives that line's
/// band. Falls back to the caret-height step only when the engine cannot name the line — better
/// an approximate move than none.
pub(crate) fn neighbour_line_center_y(
    engine: &teksilo_text::RichTextEngine,
    affinity_at: CursorAffinity,
    pos: usize,
    direction: i32,
    caret: [f32; 4],
) -> Option<f32> {
    let fallback = || {
        let center_y = caret[1] + caret[3] * 0.5;
        Some(center_y + (direction as f32) * caret[3].max(16.0))
    };
    let Some((start, end)) = engine.visual_line_range_at(pos, affinity_at) else {
        return fallback();
    };

    // A position just outside the current visual line sits on the neighbouring one — but where
    // "just outside" is depends on how the line ended. `end` is where End stops: after a **soft
    // wrap** it is already the next line's first character, while before a **hard break** (a line
    // break inside the block, or the block's own end) it still maps back to the line being left,
    // and the next line starts one character on, past the break. So going down, try both and take
    // the first that is genuinely lower; going up, the character before the line's start is on the
    // previous line either way.
    let candidates: [usize; 2] = if direction > 0 {
        [end, end.saturating_add(1)]
    } else if start == 0 {
        // Nothing above the first line.
        return None;
    } else {
        [start - 1, start.saturating_sub(2)]
    };

    // Ask with the affinity that binds a wrap boundary to the line being moved *to*.
    let affinity = if direction > 0 {
        CursorAffinity::Upstream
    } else {
        CursorAffinity::Downstream
    };
    for probe in candidates {
        let neighbour = engine.caret_rect(probe, affinity);
        // A degenerate rect names no line; and a rect on the wrong side of the caret is the
        // same line answering again, which is exactly the failure this function exists to fix.
        if neighbour[3] <= 0.0 {
            continue;
        }
        let crossed = if direction > 0 {
            neighbour[1] > caret[1]
        } else {
            neighbour[1] < caret[1]
        };
        if crossed {
            return Some(neighbour[1] + neighbour[3] * 0.5);
        }
    }
    fallback()
}

fn move_cursor_vertical(st: &mut EditorState, direction: i32, mode: MoveMode) {
    if !st.engine.has_full_layout() {
        return;
    }
    // Measured in the layout's positions, the hit mapped back into the document
    // (see `move_cursor_to_line_edge`).
    let pos = st.shown_caret();
    let caret = st.engine.caret_rect(pos, st.cursor_affinity);

    let x = st.preferred_x.unwrap_or(caret[0]);
    if st.preferred_x.is_none() {
        st.preferred_x = Some(caret[0]);
    }

    let Some(target_y) =
        neighbour_line_center_y(&st.engine, st.cursor_affinity, pos, direction, caret)
    else {
        return;
    };
    // Both ends of this comparison have to be in the space `caret_rect` and
    // `hit_test` speak — which is screen space, scroll already subtracted.
    // `content_height()` is the document's own height, measured from the top of
    // the text with no scrolling, so testing against it directly held only
    // while the editor was scrolled to the top: below that, the line above the
    // caret has a negative screen y and the guard threw away an ordinary
    // ArrowUp, and a caret the reader had scrolled off the top could not be
    // brought back with ArrowDown at all.
    let (content_top, content_bottom) = st.engine.content_band_screen();
    if target_y < content_top || target_y > content_bottom {
        return;
    }

    let from = st.cursor.position();
    if let Some(hit) = st.hit_test(teksilo_canvas::Point::new(x, target_y)) {
        st.cursor_affinity = if hit.position == from || move_moving_end(st, hit.position, mode) {
            hit.affinity
        } else {
            CursorAffinity::Downstream
        };
    }
}

/// Move the caret to `target`, where a visual step (a line, a page, a line's
/// edge) landed, extending the selection under `KeepAnchor`. Returns whether
/// the caret is at `target`, so the caller knows whether the affinity the step
/// found still applies.
///
/// A step back into a table the selection holds whole gives the table back.
/// `set_position` moves any position in such a table to its edge on the far
/// side from the selection's fixed end, and after taking the table in, the
/// moving end already stands there: the step changed nothing, and Shift with
/// Up, Page Up or Home could only grow the selection. text-document's
/// `move_position` releases the table when the moving end steps back into it
/// from that edge, taking the end to the table's other side, so a step the
/// table's edge swallowed is handed to `move_position` as one character in the
/// step's direction: enough to land in the table, whatever the step's length.
fn move_moving_end(st: &mut EditorState, target: usize, mode: MoveMode) -> bool {
    let from = st.cursor.position();
    if mode == MoveMode::KeepAnchor && target != from {
        // A clone is a cursor of its own with the same selection, so the step
        // is tried there and the editor's cursor moves once.
        let trial = st.cursor.clone();
        trial.set_position(target, MoveMode::KeepAnchor);
        if trial.position() == from {
            let toward = if target < from {
                MoveOperation::PreviousCharacter
            } else {
                MoveOperation::NextCharacter
            };
            st.cursor.move_position(toward, MoveMode::KeepAnchor, 1);
            return st.cursor.position() == target;
        }
    }
    st.cursor.set_position(target, mode);
    true
}

/// Move the cursor up or down by roughly one viewport page, and
/// scroll so the caret stays visible. Like `move_cursor_vertical`,
/// uses a sticky preferred X.
fn move_cursor_page(st: &mut EditorState, direction: i32, mode: MoveMode) {
    if !st.engine.has_full_layout() {
        return;
    }
    let viewport_h = st.viewport_height;
    if viewport_h <= 0.0 {
        return;
    }
    let from = st.cursor.position();
    let caret = st.engine.caret_rect(st.shown_caret(), st.cursor_affinity);
    let line_height = caret[3].max(16.0);
    let center_y = caret[1] + caret[3] * 0.5;

    let x = st.preferred_x.unwrap_or(caret[0]);
    if st.preferred_x.is_none() {
        st.preferred_x = Some(caret[0]);
    }

    // Move by one viewport minus one line so the reader keeps a
    // line of visual context across the page jump.
    let page_step = (viewport_h - line_height).max(line_height);
    let target_y =
        (center_y + (direction as f32) * page_step).clamp(0.0, st.engine.content_height());

    if let Some(hit) = st.hit_test(teksilo_canvas::Point::new(x, target_y)) {
        st.cursor_affinity = if hit.position == from || move_moving_end(st, hit.position, mode) {
            hit.affinity
        } else {
            CursorAffinity::Downstream
        };
    }

    // Scroll so the new caret position is visible. We do a simple
    // viewport-height step on the scroll signal and let
    // `caret_moved_epilogue`'s `ensure_caret_visible` clamp it once
    // `handle_key` returns — the frame loop deliberately never calls it.
    let new_scroll =
        (st.scroll_y.get() + (direction as f32) * page_step).clamp(0.0, st.max_scroll_y.get());
    st.scroll_y.set(new_scroll);
}

// ---------------------------------------------------------------------------
// Table / list helpers
// ---------------------------------------------------------------------------

/// Whether the caret's current block belongs to a list.
///
/// Asked through `current_list`, which reads the block the caret stands in,
/// as every list command after this check does. The block at the caret's
/// character index is the next paragraph when the caret is at the end of an
/// item's text, so at the end of a list's last item Tab typed a tab character,
/// or nested the quote holding the next paragraph, instead of moving the item.
fn is_cursor_in_list(st: &EditorState) -> bool {
    st.cursor.current_list().is_some()
}

/// Move the current list item into its own list at `target_indent`,
/// preserving the parent list's style. Returns silently if the cursor
/// isn't in a list.
///
/// Why split instead of just updating the current list's indent:
/// `ListFormat::indent` applies to the whole list (all items share
/// one indent), so bumping it would shift every sibling. To indent
/// just the current item — the Tab/Shift-Tab behaviour users expect
/// from Word / Google Docs / Notion — we take the item out of its
/// current list and put it in a fresh list at the target depth.
///
/// Why `ListFormat::indent` and not `BlockFormat::indent`: the
/// typesetter reads list-item indentation from the **list's** format
/// (`text-typeset/src/bridge.rs`: `block.list_info.indent`), so
/// writing `BlockFormat::indent` on a list block has no visual effect.
///
/// Caveat: consecutive items Tabbed to the same depth each land in
/// their own list rather than sharing one. For bullet styles this is
/// visually indistinguishable; for ordered styles numbering restarts
/// per sublist. A future pass could merge with an adjacent sibling
/// list at the same `(style, indent)`.
///
/// The item may sit in the main text, a blockquote or a table cell:
/// `create_list` finds the caret's block in any of them from text-document
/// 1.12.3 on. Up to 1.12.2 it read the main text's own blocks only, so there
/// the item left its list and joined none.
fn nest_current_list_item(st: &mut EditorState, target_indent: u8) {
    let Some(list) = st.cursor.current_list() else {
        return;
    };
    let style = list.style();
    st.cursor.begin_edit_block();
    let _ = st.cursor.remove_current_block_from_list();
    let _ = st.cursor.create_list(style);
    let _ = st.cursor.set_current_list_format(&ListFormat {
        indent: Some(target_indent),
        ..Default::default()
    });
    st.cursor.end_edit_block();
}

/// Increase the current item's nesting depth by 1. Used by Tab
/// inside a list item. See [`nest_current_list_item`] for the split
/// rationale.
///
/// Does nothing at the list ceiling ([`nesting::MAX_LIST_DEPTH`]), which
/// [`nesting::deeper_list_indent`] decides.
///
/// Exposed `pub(super)` so the parent `rich_text` module can wire it
/// into `RichTextEditor::indent` / `EditorHandle::indent` — toolbar
/// buttons need the same behaviour as Tab without going through key
/// dispatch.
pub(super) fn indent_current_block(st: &mut EditorState) {
    if let Some(target) = nesting::deeper_list_indent(st) {
        nest_current_list_item(st, target);
    }
}

/// Decrease the current item's nesting depth by 1. Used by Shift+Tab
/// inside a list item. At depth 0 this is a no-op (the user can press
/// Backspace at block-start to exit the list entirely).
///
/// Exposed `pub(super)` for the same reason as
/// [`indent_current_block`].
pub(super) fn dedent_current_block(st: &mut EditorState) {
    let Some(list) = st.cursor.current_list() else {
        return;
    };
    let level = list.indent();
    if level == 0 {
        return;
    }
    nest_current_list_item(st, level - 1);
}

/// Navigate to the next / previous table cell by `direction` (+1 or
/// -1). Wraps from end-of-row to start-of-next-row; at the last cell
/// of the last row with `direction = +1`, inserts a new row below
/// and moves into its first cell. Matches the godot reference's
/// `navigate_table_cell`.
fn navigate_table_cell(
    st: &mut EditorState,
    table_id: usize,
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
    direction: i32,
) {
    let (target_row, target_col) = if direction > 0 {
        if col + 1 < cols {
            (row, col + 1)
        } else if row + 1 < rows {
            (row + 1, 0)
        } else {
            // Last cell of last row → insert a new row and move
            // into its first cell.
            let _ = st.cursor.insert_row_below();
            (row + 1, 0)
        }
    } else if col > 0 {
        (row, col - 1)
    } else if row > 0 {
        (row - 1, cols.saturating_sub(1))
    } else {
        return; // already at the first cell
    };

    move_cursor_to_cell_first_block(st, table_id, target_row, target_col);
}

/// Enter inside a table cell: move to the cell in the same column one
/// row down; on the last row, step out of the table to the first
/// block that follows. Matches the godot reference's `navigate_table_cell_down`.
fn navigate_table_cell_down(
    st: &mut EditorState,
    table_id: usize,
    row: usize,
    col: usize,
    rows: usize,
) {
    if row + 1 < rows {
        move_cursor_to_cell_first_block(st, table_id, row + 1, col);
    } else {
        move_cursor_after_table(st, table_id);
    }
}

/// Move the caret to the first block of `(table_id, row, col)`. Finds
/// the table by walking the document's flow (see the body for why the
/// cursor's own snapshot will not do), then takes the cell's first
/// block. No-op if the cell doesn't exist.
fn move_cursor_to_cell_first_block(st: &mut EditorState, table_id: usize, row: usize, col: usize) {
    // Re-resolve the table via `current_table_cell` is insufficient
    // because after `insert_row_below` the cursor may still be in
    // the old cell. Walk the document's flow to find the table by
    // id, then ask the table for (row, col). This keeps the helper
    // free of assumptions about where the cursor currently sits.
    if let Some(table) = find_table_by_id(st, table_id)
        && let Some(cell) = table.cell(row, col)
        && let Some(block) = cell.blocks().first()
    {
        st.cursor
            .set_position(block.position(), MoveMode::MoveAnchor);
    }
}

/// Step the caret to the first block immediately following the given
/// table, in reading order: the paragraph after it in its quotation, or
/// after the quotation when the table ends it. If nothing follows the table,
/// no-op.
fn move_cursor_after_table(st: &mut EditorState, table_id: usize) {
    use teksilo_text::text_document::FlowElement;
    let mut elements = ReadingOrder::of(&st.document);
    if !elements
        .by_ref()
        .any(|element| matches!(&element, FlowElement::Table(t) if t.id() == table_id))
    {
        return;
    }
    let next = elements.find_map(|element| match element {
        FlowElement::Block(block) => Some(block.position()),
        FlowElement::Table(t) => t
            .cell(0, 0)
            .and_then(|cell| cell.blocks().first().map(|block| block.position())),
        FlowElement::Frame(_) => None,
    });
    if let Some(position) = next {
        st.cursor.set_position(position, MoveMode::MoveAnchor);
    }
}

/// Look up a table by id in the document's flow, quotations included.
/// Returns `None` if no table has that id.
fn find_table_by_id(
    st: &EditorState,
    table_id: usize,
) -> Option<teksilo_text::text_document::TextTable> {
    use teksilo_text::text_document::FlowElement;
    ReadingOrder::of(&st.document).find_map(|element| match element {
        FlowElement::Table(t) if t.id() == table_id => Some(t),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// Cell-range selection (Shift+Arrow when at a cell boundary)
// ---------------------------------------------------------------------------

/// Try to extend (or start) a rectangular cell selection in the
/// direction `(dcol, drow)`. Returns `true` when the event was
/// consumed (the caller must skip the normal text-range selection
/// path); returns `false` when the caret isn't at a cell boundary
/// eligible to start/extend a cell selection.
///
/// Mirrors the godot reference's IME preedit handling. The widget's
/// `selected_cell_range` state survives across calls so repeated
/// Shift+Arrow presses keep extending the rectangle.
pub(super) fn try_extend_cell_selection(st: &mut EditorState, dcol: i32, drow: i32) -> bool {
    use teksilo_text::text_document::SelectionKind;

    let range = match st.cursor.selection_kind() {
        // Already in cell-selection mode: extend the existing range.
        SelectionKind::Cells(range) => range,
        // A selection holding a table whole, with text before it, after it or
        // both, is a text selection: its moving end steps as text, and the
        // document keeps the table whole or gives it back. Taken for a cell
        // range, every Shift and arrow turned it into one and dropped the text.
        // It is not a caret at a cell boundary either, even with its moving end
        // in an empty last cell, which is the start of a cell as well as its end.
        SelectionKind::Mixed { .. } => return false,
        SelectionKind::None | SelectionKind::Text => return start_cell_selection(st, dcol, drow),
    };

    // The table's size bounds the rectangle. The table is read from the cell
    // the caret stands in, which is one of its cells whenever the rectangle
    // was made by a key or a drag, and looked up by its id otherwise. Read
    // through `selected_cells`, every Shift and arrow looked each cell of the
    // rectangle up among all of the table's cells: two seconds a press over a
    // whole table of five thousand cells.
    let table = match st.cursor.current_table_cell() {
        Some(cell) if cell.table.id() == range.table_id => Some(cell.table),
        _ => find_table_by_id(st, range.table_id),
    };
    let Some(table) = table else {
        return false;
    };
    let rows = table.rows();
    let cols = table.columns();
    if rows == 0 || cols == 0 {
        return false;
    }

    let new_end_row = (range.end_row as i32 + drow).clamp(0, rows as i32 - 1) as usize;
    let new_end_col = (range.end_col as i32 + dcol).clamp(0, cols as i32 - 1) as usize;

    st.select_cell_range(
        range.table_id,
        range.start_row,
        range.start_col,
        new_end_row,
        new_end_col,
    );
    true
}

/// Start a cell selection when the caret stands at a cell boundary that the
/// arrow press `(dcol, drow)` would cross, taking in the cell beyond it.
/// Returns `false`, touching nothing, anywhere else.
fn start_cell_selection(st: &mut EditorState, dcol: i32, drow: i32) -> bool {
    let Some(cell_ref) = st.cursor.current_table_cell() else {
        return false;
    };
    let TableCellRef { table, row, column } = cell_ref;
    let at_start = st.cursor.at_block_start();
    let at_end = st.cursor.at_block_end();

    let should_activate = match (dcol, drow) {
        (-1, 0) => at_start && column > 0,
        (1, 0) => at_end && column + 1 < table.columns(),
        (0, -1) => at_start && row > 0,
        (0, 1) => at_end && row + 1 < table.rows(),
        _ => false,
    };
    if !should_activate {
        return false;
    }

    let table_id = table.id();
    let target_row = (row as i32 + drow).max(0) as usize;
    let target_col = (column as i32 + dcol).max(0) as usize;
    st.select_cell_range(
        table_id,
        row.min(target_row),
        column.min(target_col),
        row.max(target_row),
        column.max(target_col),
    );
    true
}

/// The link Tab (or, with `back`, Shift+Tab) selects next, as document
/// positions, or `None` when the selection is past the last (before the first).
///
/// The links are read off the text this view shows, so the selection is
/// compared in that space and the stop mapped back to the document's: a view
/// of a document another view composes in shows no composition, and mixing
/// the two spaces selected the wrong range by the composition's length. The
/// view's cached accessibility snapshot serves, built here when the walk has
/// not built it yet. The cache is cleared when the frame loop drains the
/// document's events, so a programmatic edit and a Tab in the same frame see
/// the links as they were before the edit; the next frame sees them as they
/// are.
pub(super) fn link_tab_stop(st: &EditorState, back: bool) -> Option<(usize, usize)> {
    let stop = {
        let mut cache = st.accessibility_flow_snapshot.borrow_mut();
        if cache.is_none() {
            *cache = Some(st.flow_snapshot_for_a11y());
        }
        let links = cache
            .as_ref()
            .map(|snap| super::links::document_links(&snap.elements))
            .unwrap_or_default();
        let (a, b) = st.shown_selection();
        let (from, to) = (a.min(b), a.max(b));
        if back {
            super::links::previous_link(&links, from, to).cloned()
        } else {
            super::links::next_link(&links, from, to).cloned()
        }
    }?;
    Some((
        st.document_position(stop.start),
        st.document_position(stop.end),
    ))
}

/// What a reader is told about an editor that keeps Tab: what Tab does here,
/// and the two chords that leave it, named as this platform and locale name
/// keys. The accessibility walk puts it in the editor node's description,
/// which AT-SPI, UIA and macOS all read on focus.
pub(super) fn tab_escape_hint() -> String {
    let (next, previous) = crate::common::editor_runtime::tab_escape_chords();
    teksilo_i18n::tr_widget!(rich_text_tab_escape_hint(next = next, previous = previous))
        .resolve_now()
}

#[cfg(test)]
mod motion_tests {
    use super::*;

    // The chord → motion reading lives in `common::text_nav` and is tested
    // there against both conventions. These cover the other half: that each
    // motion, once resolved, reaches the right primitive — including the two
    // (`LineEdge`, `Document`, `Paragraph`) that no chord on a Linux host can
    // produce, and that a macOS run would otherwise be the first to exercise.

    #[test]
    fn a_word_step_walks_words_and_a_character_step_walks_characters() {
        assert_eq!(
            horizontal_op(true, CaretStep::Word),
            MoveOperation::WordRight
        );
        assert_eq!(
            horizontal_op(false, CaretStep::Word),
            MoveOperation::WordLeft
        );
        assert_eq!(
            horizontal_op(true, CaretStep::Character),
            MoveOperation::Right
        );
        assert_eq!(
            horizontal_op(false, CaretStep::Character),
            MoveOperation::Left
        );
    }

    #[test]
    fn the_line_edge_follows_the_direction_pressed() {
        // ⌘← is visual on macOS, so in right-to-left text — where `forward` is
        // false for a press to the *right* — it must reach the logical end.
        assert_eq!(line_edge_op(false), LineEdge::Start);
        assert_eq!(line_edge_op(true), LineEdge::End);
    }

    #[test]
    fn a_line_edge_step_never_becomes_a_stepping_primitive() {
        // The caller routes `LineEdge` away before `horizontal_op` sees it.
        // If that ever changes, the caret must still move by one character
        // rather than silently jumping a word.
        assert_eq!(
            horizontal_op(true, CaretStep::LineEdge),
            MoveOperation::Right
        );
        assert_eq!(
            horizontal_op(false, CaretStep::LineEdge),
            MoveOperation::Left
        );
    }

    #[test]
    fn vertical_document_and_paragraph_steps_reach_their_edges() {
        assert_eq!(
            vertical_op(LineStep::Document, false),
            Some(MoveOperation::Start)
        );
        assert_eq!(
            vertical_op(LineStep::Document, true),
            Some(MoveOperation::End)
        );
        assert_eq!(
            vertical_op(LineStep::Paragraph, false),
            Some(MoveOperation::StartOfBlock)
        );
        assert_eq!(
            vertical_op(LineStep::Paragraph, true),
            Some(MoveOperation::EndOfBlock)
        );
    }

    #[test]
    fn an_ordinary_line_step_is_left_to_the_typesetter() {
        // One visual line is not a logical primitive — a wrapped paragraph has
        // many, and only the typesetter knows where. `None` says "not mine".
        assert_eq!(vertical_op(LineStep::Line, false), None);
        assert_eq!(vertical_op(LineStep::Line, true), None);
    }
}
