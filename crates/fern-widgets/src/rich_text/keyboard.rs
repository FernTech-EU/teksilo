//! Keyboard dispatch for the rich text editor.
//!
//! This module owns the full `KeyDown` / `ImeCommit` dispatch: navigation
//! (arrows, Home/End, PageUp/Down), editing (Backspace, Delete, Enter,
//! `Ctrl+Backspace` / `Ctrl+Delete` for word‑level deletion), format
//! toggles (`Ctrl+B` / `Ctrl+I` / `Ctrl+U`), undo/redo
//! (`Ctrl+Z` / `Ctrl+Y` / `Ctrl+Shift+Z`), clipboard commands
//! (`Ctrl+C` / `Ctrl+X` / `Ctrl+V`), and `Ctrl+A` with the table‑aware
//! escalation ladder.
//!
//! All functions take `&SharedState` so they can be called from inside
//! `HandlerSet` closures without borrowing the widget struct.

use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::widget::EventContext;
use fern_text::text_document::{MoveMode, MoveOperation, SelectionType, TextFormat};

use super::clipboard;
use super::policy::EditCommandKind;
use super::state::{EditorState, SharedState};
use super::sync_cursor_signals;

/// Kind of key action taken by `handle_key`, used to decide whether to
/// clear the sticky preferred-X afterwards.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) enum KeyAction {
    /// The key caused horizontal motion, a selection change, or
    /// something else that invalidates the preferred column.
    ClearPreferredX,
    /// Vertical motion (Up/Down/PageUp/PageDown): the sticky column
    /// must be preserved so repeated vertical presses land on the
    /// same visual column.
    KeepPreferredX,
    /// The key was not handled.
    Unhandled,
}

pub(super) fn handle_key(
    state: &SharedState,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    // IME commit — one string per composition, already a finalized
    // grapheme cluster. Treated identically to a KeyDown with printable
    // text: batched into `pending_chars`, flushed next frame.
    if let WidgetEvent::ImeCommit { text } = event {
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
                let op = if ctrl {
                    MoveOperation::WordLeft
                } else {
                    MoveOperation::Left
                };
                st.cursor.move_position(op, mode, 1);
                KeyAction::ClearPreferredX
            }
            Key::ArrowRight if filter.accepts(EditCommandKind::MoveRight) => {
                let op = if ctrl {
                    MoveOperation::WordRight
                } else {
                    MoveOperation::Right
                };
                st.cursor.move_position(op, mode, 1);
                KeyAction::ClearPreferredX
            }
            Key::ArrowUp if filter.accepts(EditCommandKind::MoveUp) => {
                move_cursor_vertical(&mut st, -1, mode);
                KeyAction::KeepPreferredX
            }
            Key::ArrowDown if filter.accepts(EditCommandKind::MoveDown) => {
                move_cursor_vertical(&mut st, 1, mode);
                KeyAction::KeepPreferredX
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
                } else {
                    move_cursor_to_line_edge(&mut st, LineEdge::Start, mode);
                }
                KeyAction::ClearPreferredX
            }
            Key::End if filter.accepts(EditCommandKind::MoveEnd) => {
                if ctrl {
                    st.cursor.move_position(MoveOperation::End, mode, 1);
                } else {
                    // Use the typesetter to find end-of-visual-line
                    // rather than text-document's EndOfBlock. Two
                    // wins: (a) a second End press from an already-
                    // at-end cursor is a no-op, avoiding the
                    // block-advance bug where `get_block_at_position`
                    // returns the *next* block when queried at a
                    // boundary; (b) wrapped blocks stop at the wrap
                    // point, which is the standard editor behaviour.
                    move_cursor_to_line_edge(&mut st, LineEdge::End, mode);
                }
                KeyAction::ClearPreferredX
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
            Key::V
                if ctrl
                    && shift
                    && filter.accepts(EditCommandKind::PasteUnformatted) =>
            {
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
            Key::Enter if filter.accepts(EditCommandKind::InsertBlock) => {
                let _ = st.cursor.insert_block();
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
                        let clean: String =
                            t.chars().filter(|c| !c.is_control()).collect();
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
            }
            ensure_caret_visible(state);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        KeyAction::KeepPreferredX => {
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
        2 => st.cursor.select_table_cell(cell.table_id, cell.row, cell.column),
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

/// Shared helper for printable-character ingestion: push the text
/// into `pending_chars`, clear sticky `preferred_x`, request a frame.
/// Reused by the IME commit path.
fn push_pending_chars(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
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
        st.pending_chars.push_str(&clean);
        st.preferred_x = None;
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
fn move_cursor_to_line_edge(st: &mut EditorState, edge: LineEdge, mode: MoveMode) {
    if !st.engine.has_full_layout() {
        return;
    }
    let pos = st.cursor.position();
    let caret = st.engine.caret_rect(pos);
    let line_y = caret[1] + caret[3] * 0.5;
    // Probe far outside the viewport horizontally; the typesetter
    // clamps the hit to the actual line extent and returns a valid
    // position at either edge.
    let probe_x = match edge {
        LineEdge::Start => -1.0e6,
        LineEdge::End => 1.0e6,
    };
    if let Some(hit) = st.engine.hit_test(probe_x, line_y)
        && hit.position != pos
    {
        st.cursor.set_position(hit.position, mode);
    }
}

/// Move the cursor up or down by one visual line, using the
/// typesetter's layout and `caret_rect` for the source position and
/// `hit_test` at the target Y to find the position on the next line.
/// Uses a sticky `preferred_x` so repeated vertical presses stay on
/// the same visual column even across short lines.
///
/// Called from `handle_key` with `state.borrow_mut()` already held.
fn move_cursor_vertical(st: &mut EditorState, direction: i32, mode: MoveMode) {
    if !st.engine.has_full_layout() {
        return;
    }
    let pos = st.cursor.position();
    let caret = st.engine.caret_rect(pos);
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

    if let Some(hit) = st.engine.hit_test(x, target_y)
        && hit.position != pos
    {
        st.cursor.set_position(hit.position, mode);
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
    let caret = st.engine.caret_rect(pos);
    let line_height = caret[3].max(16.0);
    let center_y = caret[1] + caret[3] * 0.5;

    let x = st.preferred_x.unwrap_or(caret[0]);
    if st.preferred_x.is_none() {
        st.preferred_x = Some(caret[0]);
    }

    // Move by one viewport minus one line so the reader keeps a
    // line of visual context across the page jump.
    let page_step = (viewport_h - line_height).max(line_height);
    let target_y = (center_y + (direction as f32) * page_step).clamp(0.0, st.engine.content_height());

    if let Some(hit) = st.engine.hit_test(x, target_y)
        && hit.position != pos
    {
        st.cursor.set_position(hit.position, mode);
    }

    // Scroll so the new caret position is visible. We do a simple
    // viewport-height step on the scroll signal and let the frame
    // loop's `ensure_caret_visible` path clamp it.
    let new_scroll =
        (st.scroll_y.get() + (direction as f32) * page_step).clamp(0.0, st.max_scroll_y.get());
    st.scroll_y.set(new_scroll);
}

