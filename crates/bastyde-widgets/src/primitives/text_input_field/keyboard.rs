// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Keyboard dispatch for the text input widget.
//!
//! Simplified from `rich_text::keyboard`: no vertical navigation, no
//! format toggles, no select-all escalation ladder, no preferred-X. Enter
//! fires the on_submit command instead of inserting a newline.

use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::widget::EventContext;
use bastyde_platform::clipboard::ClipboardHandle;
use bastyde_text::CursorAffinity;
use bastyde_text::text_document::{MoveMode, MoveOperation, SelectionType};

use super::state::{SharedState, TextInputState, sync_cursor_signals};

/// Report the caret's window-space rectangle to the platform so the OS IME
/// candidate window tracks the insertion point. No-op when unfocused or the
/// engine has not been laid out yet. Called whenever the caret moves.
pub(crate) fn report_ime_cursor_area(state: &SharedState, ctx: &mut EventContext) {
    let area = {
        let st = state.borrow();
        if !st.has_focus || !st.engine.has_full_layout() {
            return;
        }
        let caret = st
            .engine
            .caret_rect(st.cursor.position(), CursorAffinity::Downstream);
        bastyde_canvas::Rect::new(
            st.viewport_origin.x + caret[0] - st.scroll_x,
            st.viewport_origin.y + caret[1],
            caret[2].max(1.0),
            caret[3],
        )
    };
    // Dedup against the last reported area. The platform-side
    // `WindowOps::set_ime_cursor_area` does not dedup; re-forwarding an
    // unchanged area is wasted work and, on some winit IME backends (ibus /
    // fcitx), echoes back a fresh empty `Ime::Preedit`, sustaining a feedback
    // loop. Only forward a genuinely new position.
    {
        let mut st = state.borrow_mut();
        if st.last_ime_area == Some(area) {
            return;
        }
        st.last_ime_area = Some(area);
    }
    ctx.set_ime_cursor_area(area);
}

pub(crate) fn handle_key(
    state: &SharedState,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    // IME composition (preedit) — tentative, replaceable text shown inline.
    if let WidgetEvent::ImeComposition { text, .. } = event {
        return handle_ime_composition(state, ctx, text);
    }
    // IME commit — finalized grapheme cluster. Drop any live preedit first
    // (some backends skip the empty-preedit clear), then batch into
    // pending_chars like ordinary typed input.
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

    let read_only = state.borrow().read_only;

    let handled = {
        let mut st = state.borrow_mut();
        match key {
            // ── Navigation ──────────────────────────────────────────
            Key::ArrowLeft => {
                let op = if ctrl {
                    MoveOperation::WordLeft
                } else {
                    MoveOperation::Left
                };
                st.cursor.move_position(op, mode, 1);
                true
            }
            Key::ArrowRight => {
                let op = if ctrl {
                    MoveOperation::WordRight
                } else {
                    MoveOperation::Right
                };
                st.cursor.move_position(op, mode, 1);
                true
            }
            Key::Home => {
                // Single-line: Home = start of document.
                st.cursor.move_position(MoveOperation::Start, mode, 1);
                true
            }
            Key::End => {
                // Single-line: End = end of document.
                st.cursor.move_position(MoveOperation::End, mode, 1);
                true
            }

            // ── Select all ──────────────────────────────────────────
            Key::A if ctrl => {
                st.cursor.select(SelectionType::Document);
                true
            }

            // ── Clipboard ───────────────────────────────────────────
            Key::C if ctrl => {
                clipboard_copy(&mut st, ctx);
                true
            }
            Key::X if ctrl && !read_only => {
                clipboard_cut(&mut st, ctx);
                true
            }
            Key::V if ctrl && !read_only => {
                clipboard_paste(&mut st, ctx);
                true
            }

            // ── Editing (gated on !read_only) ───────────────────────
            Key::Backspace if !read_only => {
                if ctrl {
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
                true
            }
            Key::Delete if !read_only => {
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
                true
            }

            // ── Undo / Redo ─────────────────────────────────────────
            Key::Z if ctrl && !shift && !read_only => {
                let _ = st.document.undo();
                true
            }
            Key::Y if ctrl && !read_only => {
                let _ = st.document.redo();
                true
            }
            Key::Z if ctrl && shift && !read_only => {
                let _ = st.document.redo();
                true
            }

            // ── Enter → on_submit ───────────────────────────────────
            Key::Enter => {
                if let Some(ref on_submit) = st.on_submit {
                    let submit = on_submit.clone();
                    drop(st);
                    submit(ctx);
                    return EventResponse::Handled;
                }
                // No on_submit: still consume Enter to prevent it
                // from bubbling and accidentally triggering dialogs.
                true
            }

            // ── Tab → let it bubble for focus navigation ────────────
            Key::Tab => {
                return EventResponse::Ignored;
            }

            // ── Printable characters ────────────────────────────────
            _ => {
                if ctrl {
                    false // don't eat unhandled ctrl combos
                } else if let Some(t) = text.as_deref() {
                    if !read_only {
                        // Strip newlines (single-line enforcement),
                        // control characters, and anything the
                        // optional per-character filter rejects.
                        let clean: String = t
                            .chars()
                            .filter(|c| !c.is_control() && *c != '\n' && *c != '\r')
                            .filter(|c| st.char_filter_admits(*c))
                            .collect();
                        if !clean.is_empty() {
                            // Max-length enforcement: compute remaining capacity.
                            if let Some(max) = st.max_length {
                                let current_len = st
                                    .document
                                    .to_plain_text()
                                    .unwrap_or_default()
                                    .chars()
                                    .count();
                                let sel_len = if st.cursor.has_selection() {
                                    st.cursor
                                        .selected_text()
                                        .ok()
                                        .unwrap_or_default()
                                        .chars()
                                        .count()
                                } else {
                                    0
                                };
                                let remaining =
                                    max.saturating_sub(current_len.saturating_sub(sel_len));
                                if remaining == 0 {
                                    return EventResponse::Handled;
                                }
                                let truncated: String = clean.chars().take(remaining).collect();
                                st.pending_chars.push_str(&truncated);
                            } else {
                                st.pending_chars.push_str(&clean);
                            }
                            true
                        } else {
                            // Whole input was rejected (filter or
                            // control-only); swallow the keystroke
                            // so it doesn't bubble into a shortcut
                            // match on a single-char rejected key.
                            return EventResponse::Handled;
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
        }
    };

    if handled {
        sync_cursor_signals(state);
        report_ime_cursor_area(state, ctx);
        ctx.request_frame();
        EventResponse::Handled
    } else {
        EventResponse::Ignored
    }
}

/// Apply an `ImeComposition` event. Removes the previous preedit range (if
/// any) from the document, inserts the new cleaned preedit text at the
/// cursor, and records the resulting range so the next composition event can
/// replace it. Single-line rules apply (newlines / control chars stripped,
/// `char_filter` enforced, `max_length` respected); secure-field masking is
/// handled at the engine layer so a password preedit renders as bullets. An
/// empty composition cancels the preedit without inserting.
fn handle_ime_composition(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
) -> EventResponse {
    if state.borrow().read_only {
        return EventResponse::Handled;
    }
    // A composition carrying no insertable text while there is no active preedit
    // is a genuine no-op. Some Linux IME backends (ibus / fcitx via winit) flood
    // empty `Ime::Preedit("")` events while a field is focused; processing each
    // one — an undo block, a signal sync, an un-deduped IME-area report that
    // re-arms that very loop, a repaint — is pure waste. Bail before any of it.
    {
        let st = state.borrow();
        let empty = text
            .chars()
            .all(|c| c.is_control() || c == '\n' || c == '\r');
        if empty && st.ime_preedit_range.is_none() {
            return EventResponse::Ignored;
        }
    }
    {
        let mut st = state.borrow_mut();
        // Group remove+reinsert as one undo step so an undo after
        // composition pops the whole preedit, not each candidate.
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
        // Single-line cleaning + per-character filter.
        let clean: String = text
            .chars()
            .filter(|c| !c.is_control() && *c != '\n' && *c != '\r')
            .filter(|c| st.char_filter_admits(*c))
            .collect();
        // Max-length: cap against remaining capacity (the old preedit has
        // already been removed, so the doc length here excludes it).
        let clean = if let Some(max) = st.max_length {
            let current_len = st.document.character_count();
            let sel_len = if st.cursor.has_selection() {
                st.cursor
                    .selected_text()
                    .ok()
                    .unwrap_or_default()
                    .chars()
                    .count()
            } else {
                0
            };
            let remaining = max.saturating_sub(current_len.saturating_sub(sel_len));
            clean.chars().take(remaining).collect::<String>()
        } else {
            clean
        };
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
        st.pending_text_changed = true;
    }
    sync_cursor_signals(state);
    report_ime_cursor_area(state, ctx);
    ctx.request_frame();
    EventResponse::Handled
}

/// Drop any active preedit, removing its tentative text from the document.
/// Called on commit (before the finalised insert) and on focus loss.
pub(crate) fn clear_ime_preedit(state: &SharedState) {
    let mut st = state.borrow_mut();
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

/// Batch pending characters from an IME commit.
fn push_pending_chars(state: &SharedState, ctx: &mut EventContext, text: &str) -> EventResponse {
    let read_only = state.borrow().read_only;
    if read_only {
        return EventResponse::Handled;
    }
    // Strip newlines (single-line enforcement), control characters,
    // and anything the optional filter rejects.
    let clean: String = {
        let st = state.borrow();
        text.chars()
            .filter(|c| !c.is_control() && *c != '\n' && *c != '\r')
            .filter(|c| st.char_filter_admits(*c))
            .collect()
    };
    if clean.is_empty() {
        return EventResponse::Handled;
    }
    {
        let mut st = state.borrow_mut();
        // Max-length enforcement.
        if let Some(max) = st.max_length {
            let current_len = st
                .document
                .to_plain_text()
                .unwrap_or_default()
                .chars()
                .count();
            let sel_len = if st.cursor.has_selection() {
                st.cursor
                    .selected_text()
                    .ok()
                    .unwrap_or_default()
                    .chars()
                    .count()
            } else {
                0
            };
            let remaining = max.saturating_sub(current_len.saturating_sub(sel_len));
            if remaining == 0 {
                return EventResponse::Handled;
            }
            let truncated: String = clean.chars().take(remaining).collect();
            st.pending_chars.push_str(&truncated);
        } else {
            st.pending_chars.push_str(&clean);
        }
    }
    sync_cursor_signals(state);
    report_ime_cursor_area(state, ctx);
    ctx.request_frame();
    EventResponse::Handled
}

// ── Plain-text clipboard helpers ────────────────────────────────────

pub(crate) fn clipboard_copy(state: &mut TextInputState, ctx: &EventContext) {
    // Secure fields suppress copy while masked — the plaintext must not
    // reach the system clipboard. Allowed when revealed or opted in.
    if !state.copy_allowed() {
        return;
    }
    if !state.cursor.has_selection() {
        return;
    }
    let plain = state.cursor.selected_text().ok().unwrap_or_default();
    if let Some(cb) = ctx.app_state::<ClipboardHandle>() {
        let _ = cb.set_text(&plain);
    }
}

pub(crate) fn clipboard_cut(state: &mut TextInputState, ctx: &EventContext) {
    // Block the whole cut (not just the copy half) on a masked secure
    // field, so the delete doesn't happen without the clipboard write.
    if !state.copy_allowed() {
        return;
    }
    if !state.cursor.has_selection() {
        return;
    }
    clipboard_copy(state, ctx);
    let _ = state.cursor.remove_selected_text();
    state.pending_text_changed = true;
}

pub(crate) fn clipboard_paste(state: &mut TextInputState, ctx: &EventContext) {
    let Some(cb) = ctx.app_state::<ClipboardHandle>() else {
        return;
    };
    let Ok(system) = cb.get_text() else {
        return;
    };
    if system.is_empty() {
        return;
    }
    // Strip control characters (single-line enforcement — covers newlines,
    // tabs, NUL, …) and anything the optional filter rejects. This matches the
    // typing / IME paths exactly; previously paste let tabs and other control
    // chars through, an asymmetry a single-line field shouldn't have (and the
    // `char_filter` hook can't re-admit them — it only rejects).
    let clean: String = system
        .chars()
        .filter(|c| !c.is_control() && *c != '\n' && *c != '\r')
        .filter(|c| state.char_filter_admits(*c))
        .collect();
    if clean.is_empty() {
        return;
    }
    // Max-length enforcement.
    if let Some(max) = state.max_length {
        let current_len = state
            .document
            .to_plain_text()
            .unwrap_or_default()
            .chars()
            .count();
        let sel_len = if state.cursor.has_selection() {
            state
                .cursor
                .selected_text()
                .ok()
                .unwrap_or_default()
                .chars()
                .count()
        } else {
            0
        };
        let remaining = max.saturating_sub(current_len.saturating_sub(sel_len));
        if remaining == 0 {
            return;
        }
        let truncated: String = clean.chars().take(remaining).collect();
        let _ = state.cursor.insert_text(&truncated);
    } else {
        let _ = state.cursor.insert_text(&clean);
    }
    state.cursor.clear_selection();
    state.pending_text_changed = true;
}
