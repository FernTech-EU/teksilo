//! Keyboard dispatch for the rich text editor.
//!
//! This module owns the full `KeyDown` / `ImeComposition` / `ImeCommit` dispatch: navigation
//! (arrows, Home/End, PageUp/Down), editing (Backspace, Delete, Enter,
//! `Ctrl+Backspace` / `Ctrl+Delete` for word‑level deletion), format
//! toggles (`Ctrl+B` / `Ctrl+I` / `Ctrl+U`), undo/redo
//! (`Ctrl+Z` / `Ctrl+Y` / `Ctrl+Shift+Z`), clipboard commands
//! (`Ctrl+C` / `Ctrl+X` / `Ctrl+V`), and `Ctrl+A` with the table‑aware
//! escalation ladder.
//!
//! All functions take `&SharedState` so they can be called from inside
//! `HandlerSet` closures without borrowing the widget struct.

use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::widget::EventContext;
use bastyde_text::CursorAffinity;
use bastyde_text::text_document::{
    BlockFormat, ListFormat, MoveMode, MoveOperation, SelectionType, TableCellRef, TextFormat,
};

use super::clipboard;
use super::policy::EditCommandKind;
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
    /// resets `cursor_affinity` to `Downstream`. Covers Left/Right,
    /// Ctrl+Home/Ctrl+End, Backspace/Delete/Enter/typing, paste, etc.
    ClearPreferredX,
    /// Visual-line edge motion that went through
    /// [`move_cursor_to_line_edge`]. The helper already set
    /// `cursor_affinity` from the typesetter's hit-test, so the
    /// post-processing must NOT clobber it. Clears the sticky column.
    /// Covers non-Ctrl Home/End.
    LineEdgeMotion,
    /// Vertical motion (Up/Down/PageUp/PageDown): the sticky column
    /// must be preserved so repeated vertical presses land on the
    /// same visual column. The helpers also set `cursor_affinity`
    /// from hit-test, so post-processing must NOT clobber it. Ctrl+A
    /// also lives here since it leaves the caret position unchanged
    /// and any current affinity is still valid.
    KeepPreferredX,
    /// The key was not handled.
    Unhandled,
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
    if let WidgetEvent::ImeComposition { text, .. } = event {
        return handle_ime_composition(state, ctx, text);
    }

    // IME commit — the user picked a candidate or typed a printable
    // key that finalises the sequence. Clear any active preedit and
    // insert the committed text via the normal `pending_chars` batch.
    if let WidgetEvent::ImeCommit { text } = event {
        clear_ime_preedit(state);
        return push_pending_chars(state, ctx, text);
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

    let shift = modifiers.shift();
    let ctrl = modifiers.ctrl() || modifiers.super_key();
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
    // Ctrl+A is the only key that preserves `select_all_level`; every
    // other key resets it so a follow-up Ctrl+A starts at level 1.
    // Matches godot rich_text_edit.rs:520-521.
    let is_select_all = matches!(key, Key::A) && ctrl;
    let action: KeyAction = {
        let mut st = state.borrow_mut();
        if !is_select_all {
            st.select_all_level = 0;
            st.select_all_anchor_cell = None;
        }
        let filter = st.policy.command_filter;
        match key {
            Key::ArrowLeft if filter.accepts(EditCommandKind::MoveLeft) => {
                // Shift+Arrow at a cell boundary activates / extends a
                // rectangular cell selection (godot parity). Only
                // claims the event when we're actually at a cell
                // boundary; otherwise fall through to text-range
                // selection.
                if shift && try_extend_cell_selection(&mut st, -1, 0) {
                    KeyAction::ClearPreferredX
                } else {
                    let op = if ctrl {
                        MoveOperation::WordLeft
                    } else {
                        MoveOperation::Left
                    };
                    st.cursor.move_position(op, mode, 1);
                    KeyAction::ClearPreferredX
                }
            }
            Key::ArrowRight if filter.accepts(EditCommandKind::MoveRight) => {
                if shift && try_extend_cell_selection(&mut st, 1, 0) {
                    KeyAction::ClearPreferredX
                } else {
                    let op = if ctrl {
                        MoveOperation::WordRight
                    } else {
                        MoveOperation::Right
                    };
                    st.cursor.move_position(op, mode, 1);
                    KeyAction::ClearPreferredX
                }
            }
            Key::ArrowUp if filter.accepts(EditCommandKind::MoveUp) => {
                if shift && try_extend_cell_selection(&mut st, 0, -1) {
                    KeyAction::ClearPreferredX
                } else {
                    move_cursor_vertical(&mut st, -1, mode);
                    KeyAction::KeepPreferredX
                }
            }
            Key::ArrowDown if filter.accepts(EditCommandKind::MoveDown) => {
                if shift && try_extend_cell_selection(&mut st, 0, 1) {
                    KeyAction::ClearPreferredX
                } else {
                    move_cursor_vertical(&mut st, 1, mode);
                    KeyAction::KeepPreferredX
                }
            }
            Key::PageUp if filter.accepts(EditCommandKind::PageUp) => {
                move_cursor_page(&mut st, -1, mode);
                KeyAction::KeepPreferredX
            }
            Key::PageDown if filter.accepts(EditCommandKind::PageDown) => {
                move_cursor_page(&mut st, 1, mode);
                KeyAction::KeepPreferredX
            }
            Key::Home if filter.accepts(EditCommandKind::MoveHome) => {
                if ctrl {
                    st.cursor.move_position(MoveOperation::Start, mode, 1);
                    KeyAction::ClearPreferredX
                } else {
                    // `move_cursor_to_line_edge` sets `cursor_affinity`
                    // from the hit-test, so this path returns
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
                    // The helper sets `cursor_affinity` from
                    // hit_test → returns `LineEdgeMotion`.
                    move_cursor_to_line_edge(&mut st, LineEdge::End, mode);
                    KeyAction::LineEdgeMotion
                }
            }
            Key::A if ctrl && filter.accepts(EditCommandKind::SelectAll) => {
                apply_select_all_ladder(&mut st);
                // Horizontal motion is invalidated regardless.
                st.preferred_x = None;
                // KeepPreferredX skips the preferred_x reset in
                // post-processing — we already nulled it inline, and
                // crucially we must not clobber the `select_all_level`
                // which was just incremented.
                KeyAction::KeepPreferredX
            }
            Key::C if ctrl && filter.accepts(EditCommandKind::Copy) => {
                clipboard::copy(&mut st, ctx);
                KeyAction::ClearPreferredX
            }
            Key::X if ctrl && filter.accepts(EditCommandKind::Cut) => {
                clipboard::cut(&mut st, ctx);
                KeyAction::ClearPreferredX
            }
            Key::V if ctrl && shift && filter.accepts(EditCommandKind::PasteUnformatted) => {
                // Ctrl+Shift+V (⌘⇧V on macOS) — paste as plain text.
                // Matched before the plain Ctrl+V arm so the shift
                // modifier isn't absorbed by the regular paste.
                clipboard::paste_unformatted(&mut st, ctx);
                KeyAction::ClearPreferredX
            }
            Key::V if ctrl && !shift && filter.accepts(EditCommandKind::Paste) => {
                clipboard::paste(&mut st, ctx);
                KeyAction::ClearPreferredX
            }
            // --- Editor-preset mutating commands ---
            Key::Backspace if filter.accepts(EditCommandKind::DeletePrev) => {
                if ctrl {
                    // Ctrl+Backspace = delete word to the left.
                    // Select the word, then delete the selection —
                    // matches godot rich_text_edit.rs:580 (there is
                    // no dedicated delete-word API on TextCursor).
                    if !st.cursor.has_selection() {
                        st.cursor
                            .move_position(MoveOperation::WordLeft, MoveMode::KeepAnchor, 1);
                    }
                    let _ = st.cursor.remove_selected_text();
                } else if st.cursor.has_selection() {
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
                    //    paragraph). Matches godot rich_text_edit.rs:
                    //    566-584.
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
                } else {
                    let _ = st.cursor.delete_previous_char();
                }
                KeyAction::ClearPreferredX
            }
            Key::Delete if filter.accepts(EditCommandKind::DeleteNext) => {
                if ctrl {
                    if !st.cursor.has_selection() {
                        st.cursor
                            .move_position(MoveOperation::WordRight, MoveMode::KeepAnchor, 1);
                    }
                    let _ = st.cursor.remove_selected_text();
                } else if st.cursor.has_selection() {
                    let _ = st.cursor.remove_selected_text();
                } else {
                    let _ = st.cursor.delete_char();
                }
                KeyAction::ClearPreferredX
            }
            Key::Enter if ctrl && filter.accepts(EditCommandKind::InsertBlockForced) => {
                // Ctrl+Enter: always insert a new block, bypassing
                // table-cell navigation. Matches godot
                // rich_text_edit.rs:559-563.
                let _ = st.cursor.insert_block();
                KeyAction::ClearPreferredX
            }
            Key::Enter if !ctrl && filter.accepts(EditCommandKind::InsertBlock) => {
                // Enter (without Ctrl) inside a table cell: move to
                // the cell in the same column one row down, or (on
                // the last row) step out of the table to the block
                // that follows. Outside a table, fall through to
                // `insert_block`. Shift+Enter falls through to the
                // normal insert_block path — matches godot's
                // behaviour where Shift+Enter is treated like Enter.
                let cell_info = st
                    .cursor
                    .current_table_cell()
                    .map(|c| (c.table.id(), c.row, c.column, c.table.rows()));
                if let Some((table_id, row, col, rows)) = cell_info
                    && filter.accepts(EditCommandKind::NavigateTableCellDown)
                {
                    navigate_table_cell_down(&mut st, table_id, row, col, rows);
                } else {
                    let _ = st.cursor.insert_block();
                }
                KeyAction::ClearPreferredX
            }
            Key::Tab if !ctrl && shift && filter.accepts(EditCommandKind::NavigateTableCell) => {
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
                }
                KeyAction::ClearPreferredX
            }
            Key::Tab if !ctrl && !shift && filter.accepts(EditCommandKind::NavigateTableCell) => {
                // Tab (no Ctrl, no Shift):
                //  * Inside a table cell → next cell (wraps to next row;
                //    at last cell, insert a new row below).
                //  * Inside a list item → increase indent. Works from
                //    any caret position within the block so the user
                //    doesn't have to Home first.
                //  * Otherwise → insert a literal `\t`.
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
                    indent_current_block(&mut st);
                } else if filter.accepts(EditCommandKind::InsertTab) {
                    let _ = st.cursor.insert_text("\t");
                }
                KeyAction::ClearPreferredX
            }
            Key::B if ctrl && filter.accepts(EditCommandKind::ToggleBold) => {
                toggle_char_format(&mut st, FormatBit::Bold);
                KeyAction::ClearPreferredX
            }
            Key::I if ctrl && filter.accepts(EditCommandKind::ToggleItalic) => {
                toggle_char_format(&mut st, FormatBit::Italic);
                KeyAction::ClearPreferredX
            }
            Key::U if ctrl && filter.accepts(EditCommandKind::ToggleUnderline) => {
                toggle_char_format(&mut st, FormatBit::Underline);
                KeyAction::ClearPreferredX
            }
            Key::Z if ctrl && !shift && filter.accepts(EditCommandKind::Undo) => {
                let _ = st.document.undo();
                KeyAction::ClearPreferredX
            }
            Key::Y if ctrl && filter.accepts(EditCommandKind::Redo) => {
                let _ = st.document.redo();
                KeyAction::ClearPreferredX
            }
            Key::Z if ctrl && shift && filter.accepts(EditCommandKind::Redo) => {
                let _ = st.document.redo();
                KeyAction::ClearPreferredX
            }
            _ => {
                // Printable character fallback: winit populates
                // `KeyDown::text` with the character produced by the
                // key (post-layout mapping, so Shift / dead keys /
                // layout translations are already applied). Guard
                // against Ctrl / Super (Cmd) being held — some
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
            ensure_caret_visible(state);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        KeyAction::LineEdgeMotion => {
            // Home/End went through `move_cursor_to_line_edge`, which
            // already set `cursor_affinity` from the typesetter's
            // hit-test. Just clear the sticky column.
            {
                let mut st = state.borrow_mut();
                st.preferred_x = None;
            }
            ensure_caret_visible(state);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        KeyAction::KeepPreferredX => {
            // Up/Down/PageUp/PageDown's helpers already set affinity
            // from hit-test; Ctrl+A didn't move the caret. Preserve
            // both `preferred_x` and `cursor_affinity` unchanged.
            ensure_caret_visible(state);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
    }
}

/// Ctrl+A escalation ladder. Mirrors godot rich_text_edit.rs:690-727.
/// When the caret sits inside a table cell, four consecutive presses
/// widen the selection: paragraph → cell → table → document, then
/// wrap back to paragraph on the fifth. Outside a table Ctrl+A is
/// single-shot `SelectionType::Document` with `select_all_level = 0`.
///
/// **Mid-ladder boundary stability**: after level 1 applies
/// `select(BlockUnderCursor)`, the cursor's `position` lands at the
/// end of the selected block, which for a single-block cell's last
/// block is the boundary between the cell and whatever follows —
/// `current_table_cell()` may then return `None` and the ladder
/// would skip cell/table levels and jump straight to document. To
/// prevent that we cache the cell reference on the first ladder
/// call (`select_all_anchor_cell`) and reuse it for levels 2 and 3.
/// The cache is cleared whenever `select_all_level` resets.
pub(super) fn apply_select_all_ladder(st: &mut EditorState) {
    use super::state::SelectAllAnchorCell;
    let next_level = st.select_all_level + 1;

    // Resolve the cell snapshot for this ladder step. At level 1 we
    // read everything fresh from the cursor and cache it. At levels
    // 2 and 3 we reuse the cached snapshot so a boundary-adjacent
    // cursor (produced by level 1's `select(BlockUnderCursor)`)
    // doesn't short-circuit the ladder via a stale
    // `current_table_cell() == None` result.
    let cell_info: Option<SelectAllAnchorCell> = if next_level == 1 {
        st.cursor.current_table_cell().map(|c| SelectAllAnchorCell {
            table_id: c.table.id(),
            row: c.row,
            column: c.column,
            table_rows: c.table.rows(),
            table_columns: c.table.columns(),
        })
    } else {
        st.select_all_anchor_cell
    };

    let Some(cell) = cell_info else {
        st.cursor.select(SelectionType::Document);
        st.select_all_level = 0;
        st.select_all_anchor_cell = None;
        return;
    };

    if next_level == 1 {
        st.select_all_anchor_cell = Some(cell);
    }

    match next_level {
        1 => st.cursor.select(SelectionType::BlockUnderCursor),
        2 => st
            .cursor
            .select_table_cell(cell.table_id, cell.row, cell.column),
        3 => {
            st.cursor.select_cell_range(
                cell.table_id,
                0,
                0,
                cell.table_rows.saturating_sub(1),
                cell.table_columns.saturating_sub(1),
            );
        }
        _ => {
            st.cursor.select(SelectionType::Document);
        }
    }
    st.select_all_level = if next_level >= 4 { 0 } else { next_level };
    if st.select_all_level == 0 {
        st.select_all_anchor_cell = None;
    }
}

/// Which `TextFormat` bit a Ctrl+B/I/U toggle flips.
#[derive(Copy, Clone)]
enum FormatBit {
    Bold,
    Italic,
    Underline,
}

/// Toggle a single character-format bit at the caret, mirroring the
/// godot reference (rich_text_edit.rs:2089-2117): the decision to
/// turn a format on or off is read from the current caret format
/// (`char_format()`), not from a selection-wide consensus.
///
/// **Read-position subtlety**: `TextCursor::char_format()` reads
/// the inline element at `position()`. After a select-all the
/// caret sits at the *end* of the selection, which may be past the
/// last character (an empty "virtual" element with default format).
/// To get a meaningful read for the toggle decision we use
/// `selection_start()` when a selection is active — that position
/// is always the actual first character of the selected range.
fn toggle_char_format(st: &mut EditorState, bit: FormatBit) {
    let probe = st.document.cursor();
    if st.cursor.has_selection() {
        let start = st.cursor.selection_start();
        probe.set_position(start, MoveMode::MoveAnchor);
    } else {
        probe.set_position(st.cursor.position(), MoveMode::MoveAnchor);
    }
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

/// Apply an `ImeComposition` event. Removes the previous preedit
/// range from the document (if any), inserts the new preedit text,
/// and records the resulting range so the next composition event can
/// replace it. An empty composition string cancels the preedit and
/// clears the range without inserting anything.
fn handle_ime_composition(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
) -> EventResponse {
    let filter = state.borrow().policy.command_filter;
    if !filter.accepts(EditCommandKind::InsertChar) {
        return EventResponse::Ignored;
    }
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    {
        let mut st = state.borrow_mut();
        // Group the remove+reinsert as a single undo step so a user
        // undo after composition pops the entire preedit, not each
        // intermediate candidate.
        st.cursor.begin_edit_block();
        if let Some(range) = st.ime_preedit_range.take() {
            let doc_end = st.document.character_count();
            let start = range.start.min(doc_end);
            let end = range.end.min(doc_end);
            if start < end {
                st.cursor.set_position(start, MoveMode::MoveAnchor);
                st.cursor.set_position(end, MoveMode::KeepAnchor);
                let _ = st.cursor.remove_selected_text();
            }
        }
        if !clean.is_empty() {
            let start = st.cursor.position();
            let _ = st.cursor.insert_text(&clean);
            let end = st.cursor.position();
            st.ime_preedit = Some(clean);
            st.ime_preedit_range = Some(start..end);
        } else {
            st.ime_preedit = None;
        }
        st.cursor.end_edit_block();
        st.preferred_x = None;
        // Editing always lands the caret at a logical position; any
        // sticky upstream affinity from a previous click is invalid
        // now. Render at the default downstream placement.
        st.cursor_affinity = CursorAffinity::Downstream;
        st.pending_text_changed = true;
    }
    ctx.request_frame();
    EventResponse::Handled
}

/// Drop any active preedit bookkeeping without mutating the document.
/// Called on commit and on focus loss — the commit path then inserts
/// the finalised text via the normal `pending_chars` route, and the
/// document already contains the tentative preedit from the last
/// composition event, so commit just leaves that text in place.
fn clear_ime_preedit(state: &SharedState) {
    let mut st = state.borrow_mut();
    // On commit, the input method typically sends one final
    // composition with empty text followed by a commit with the final
    // string — but some backends skip the empty composition. If the
    // preedit range is still live, remove it so the commit insert
    // doesn't duplicate content.
    if let Some(range) = st.ime_preedit_range.take() {
        let doc_end = st.document.character_count();
        let start = range.start.min(doc_end);
        let end = range.end.min(doc_end);
        if start < end {
            st.cursor.set_position(start, MoveMode::MoveAnchor);
            st.cursor.set_position(end, MoveMode::KeepAnchor);
            let _ = st.cursor.remove_selected_text();
        }
    }
    st.ime_preedit = None;
}

/// Shared helper for printable-character ingestion: push the text
/// into `pending_chars`, clear sticky `preferred_x`, request a frame.
/// Reused by the IME commit path.
fn push_pending_chars(state: &SharedState, ctx: &mut EventContext, text: &str) -> EventResponse {
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
        st.pending_chars.push_str(&clean);
        st.preferred_x = None;
        // Typing always lands the caret at a logical position; any
        // sticky upstream affinity from a previous click is invalid.
        st.cursor_affinity = CursorAffinity::Downstream;
    }
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
    if !st.engine.has_full_layout() {
        return;
    }
    // Forward the current wheel-driven scroll so ensure_caret_visible
    // computes the correction relative to where the viewport actually
    // is, not where it was at last paint.
    let current = st.scroll_y.get();
    st.engine.set_scroll_offset(current);
    if let Some(new_off) = st.engine.ensure_caret_visible() {
        st.scroll_y.set(new_off);
    }
    // Horizontal caret visibility — matches godot's
    // `ensure_caret_h_visible` (rich_text_edit.rs:1935-1959). Margin
    // is 20 logical pixels on each side so the caret doesn't sit
    // flush against the viewport edge.
    ensure_caret_h_visible_locked(&mut st);
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

    let pos = st.cursor.position();
    let caret = st.engine.caret_rect(pos, st.cursor_affinity);
    let caret_x = caret[0]; // engine returns screen-space x
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

#[derive(Copy, Clone)]
enum LineEdge {
    Start,
    End,
}

/// Move the cursor to the start or end of the current visual line
/// using the typesetter's `hit_test`. Solves two bugs at once:
///  * A second End press after landing at line end is a no-op,
///    avoiding text-document's block-boundary ambiguity where
///    `get_block_at_position(block_end_pos)` returns the next block.
///  * Wrapped blocks stop at the wrap point (the standard editor
///    Home/End semantics).
///
/// Affinity handling: read the current `st.cursor_affinity` so the
/// `caret_rect` query sees the line the user is visually on (matters
/// at soft-wrap boundaries), then overwrite `st.cursor_affinity` with
/// the hit's affinity so the post-move caret renders on the matched
/// line (Home from end-of-K → start-of-K = Upstream-of-K+1's-start;
/// End from start-of-K+1 → end-of-K = Downstream-of-K's-end).
fn move_cursor_to_line_edge(st: &mut EditorState, edge: LineEdge, mode: MoveMode) {
    if !st.engine.has_full_layout() {
        return;
    }
    let pos = st.cursor.position();
    let caret = st.engine.caret_rect(pos, st.cursor_affinity);
    let line_y = caret[1] + caret[3] * 0.5;
    // Probe far outside the viewport horizontally; the typesetter
    // clamps the hit to the actual line extent and returns a valid
    // position at either edge.
    let probe_x = match edge {
        LineEdge::Start => -1.0e6,
        LineEdge::End => 1.0e6,
    };
    if let Some(hit) = st.engine.hit_test(probe_x, line_y) {
        if hit.position != pos {
            st.cursor.set_position(hit.position, mode);
        }
        st.cursor_affinity = hit.affinity;
    }
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
fn move_cursor_vertical(st: &mut EditorState, direction: i32, mode: MoveMode) {
    if !st.engine.has_full_layout() {
        return;
    }
    let pos = st.cursor.position();
    let caret = st.engine.caret_rect(pos, st.cursor_affinity);
    let line_height = caret[3].max(16.0);
    let center_y = caret[1] + caret[3] * 0.5;

    let x = st.preferred_x.unwrap_or(caret[0]);
    if st.preferred_x.is_none() {
        st.preferred_x = Some(caret[0]);
    }

    let target_y = center_y + (direction as f32) * line_height;
    if target_y < 0.0 || target_y > st.engine.content_height() {
        return;
    }

    if let Some(hit) = st.engine.hit_test(x, target_y) {
        if hit.position != pos {
            st.cursor.set_position(hit.position, mode);
        }
        st.cursor_affinity = hit.affinity;
    }
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
    let pos = st.cursor.position();
    let caret = st.engine.caret_rect(pos, st.cursor_affinity);
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

    if let Some(hit) = st.engine.hit_test(x, target_y) {
        if hit.position != pos {
            st.cursor.set_position(hit.position, mode);
        }
        st.cursor_affinity = hit.affinity;
    }

    // Scroll so the new caret position is visible. We do a simple
    // viewport-height step on the scroll signal and let the frame
    // loop's `ensure_caret_visible` path clamp it.
    let new_scroll =
        (st.scroll_y.get() + (direction as f32) * page_step).clamp(0.0, st.max_scroll_y.get());
    st.scroll_y.set(new_scroll);
}

// ---------------------------------------------------------------------------
// Table / list helpers
// ---------------------------------------------------------------------------

/// Whether the caret's current block belongs to a list.
fn is_cursor_in_list(st: &EditorState) -> bool {
    let pos = st.cursor.position();
    st.document
        .block_at_position(pos)
        .and_then(|b| b.list())
        .is_some()
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
/// Exposed `pub(super)` so the parent `rich_text` module can wire it
/// into `RichTextEditor::indent` / `EditorHandle::indent` — toolbar
/// buttons need the same behaviour as Tab without going through key
/// dispatch.
pub(super) fn indent_current_block(st: &mut EditorState) {
    let Some(list) = st.cursor.current_list() else {
        return;
    };
    let level = list.indent();
    let target = level.saturating_add(1);
    if target == level {
        return;
    }
    nest_current_list_item(st, target);
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
/// and moves into its first cell. Matches godot rich_text_edit.rs:
/// 1471-1512.
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
/// block that follows. Matches godot rich_text_edit.rs:1515-1535.
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

/// Move the caret to the first block of `(table_id, row, col)`. Uses
/// `table_cell_blocks_first_position` via the table handle obtained
/// through the current cursor's snapshot. No-op if the cell doesn't
/// exist.
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
/// table. If the table is the last element in the document, no-op.
fn move_cursor_after_table(st: &mut EditorState, table_id: usize) {
    use bastyde_text::text_document::FlowElement;
    let flow = st.document.flow();
    let mut found = false;
    for element in &flow {
        if found {
            match element {
                FlowElement::Block(block) => {
                    st.cursor
                        .set_position(block.position(), MoveMode::MoveAnchor);
                    return;
                }
                FlowElement::Table(t) => {
                    if let Some(cell) = t.cell(0, 0)
                        && let Some(block) = cell.blocks().first()
                    {
                        st.cursor
                            .set_position(block.position(), MoveMode::MoveAnchor);
                        return;
                    }
                }
                _ => {}
            }
        }
        if let FlowElement::Table(t) = element
            && t.id() == table_id
        {
            found = true;
        }
    }
}

/// Look up a table by id via the document's flow. Returns `None` if
/// the id isn't in the current flow.
fn find_table_by_id(
    st: &EditorState,
    table_id: usize,
) -> Option<bastyde_text::text_document::TextTable> {
    use bastyde_text::text_document::FlowElement;
    for element in st.document.flow() {
        if let FlowElement::Table(t) = element
            && t.id() == table_id
        {
            return Some(t);
        }
    }
    None
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
/// Mirrors godot rich_text_edit.rs:1755-1824. The widget's
/// `selected_cell_range` state survives across calls so repeated
/// Shift+Arrow presses keep extending the rectangle.
pub(super) fn try_extend_cell_selection(st: &mut EditorState, dcol: i32, drow: i32) -> bool {
    use bastyde_text::text_document::SelectionKind;

    // If already in cell-selection mode, extend the existing range.
    if let SelectionKind::Cells(range)
    | SelectionKind::Mixed {
        cell_range: range, ..
    } = st.cursor.selection_kind()
    {
        // Use the cached table dimensions from the first selected
        // cell. If the range is degenerate we can't know the table
        // bounds reliably, so bail.
        let cells = st.cursor.selected_cells();
        let Some(first) = cells.first() else {
            return false;
        };
        let rows = first.table.rows();
        let cols = first.table.columns();
        if rows == 0 || cols == 0 {
            return false;
        }

        let new_end_row = (range.end_row as i32 + drow).clamp(0, rows as i32 - 1) as usize;
        let new_end_col = (range.end_col as i32 + dcol).clamp(0, cols as i32 - 1) as usize;

        st.cursor.select_cell_range(
            range.table_id,
            range.start_row,
            range.start_col,
            new_end_row,
            new_end_col,
        );
        return true;
    }

    // Not yet in cell-selection mode: check if the caret is at a cell
    // boundary that an arrow press would cross.
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
    st.cursor.select_cell_range(
        table_id,
        row.min(target_row),
        column.min(target_col),
        row.max(target_row),
        column.max(target_col),
    );
    true
}
