//! Keyboard dispatch for the text input widget.
//!
//! Simplified from [`rich_text::keyboard`]: no vertical navigation, no
//! format toggles, no select-all escalation ladder, no preferred-X. Enter
//! fires the on_submit command instead of inserting a newline.

use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::widget::EventContext;
use fern_platform::clipboard::ClipboardHandle;
use fern_text::text_document::{MoveMode, MoveOperation, SelectionType};

use super::state::{SharedState, TextInputState, sync_cursor_signals};

pub(crate) fn handle_key(
    state: &SharedState,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    // IME commit — finalized grapheme cluster, batched into pending_chars.
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
                        // Strip newlines (single-line enforcement) and
                        // control characters.
                        let clean: String = t
                            .chars()
                            .filter(|c| !c.is_control() && *c != '\n' && *c != '\r')
                            .collect();
                        if !clean.is_empty() {
                            // Max-length enforcement: compute remaining capacity.
                            if let Some(max) = st.max_length {
                                let current_len = st.document.to_plain_text().unwrap_or_default().chars().count();
                                let sel_len = if st.cursor.has_selection() {
                                    st.cursor.selected_text().ok().unwrap_or_default().chars().count()
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
                            true
                        } else {
                            false
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
        ctx.request_frame();
        EventResponse::Handled
    } else {
        EventResponse::Ignored
    }
}

/// Batch pending characters from an IME commit.
fn push_pending_chars(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
) -> EventResponse {
    let read_only = state.borrow().read_only;
    if read_only {
        return EventResponse::Handled;
    }
    // Strip newlines — single-line enforcement.
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control() && *c != '\n' && *c != '\r')
        .collect();
    if clean.is_empty() {
        return EventResponse::Handled;
    }
    {
        let mut st = state.borrow_mut();
        // Max-length enforcement.
        if let Some(max) = st.max_length {
            let current_len = st.document.to_plain_text().unwrap_or_default().chars().count();
            let sel_len = if st.cursor.has_selection() {
                st.cursor.selected_text().ok().unwrap_or_default().chars().count()
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
    ctx.request_frame();
    EventResponse::Handled
}

// ── Plain-text clipboard helpers ────────────────────────────────────

pub(crate) fn clipboard_copy(state: &mut TextInputState, ctx: &EventContext) {
    if !state.cursor.has_selection() {
        return;
    }
    let plain = state.cursor.selected_text().ok().unwrap_or_default();
    if let Some(cb) = ctx.app_state::<ClipboardHandle>() {
        let _ = cb.set_text(&plain);
    }
}

pub(crate) fn clipboard_cut(state: &mut TextInputState, ctx: &EventContext) {
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
    // Strip newlines — single-line enforcement.
    let clean: String = system
        .chars()
        .filter(|c| *c != '\n' && *c != '\r')
        .collect();
    if clean.is_empty() {
        return;
    }
    // Max-length enforcement.
    if let Some(max) = state.max_length {
        let current_len = state.document.to_plain_text().unwrap_or_default().chars().count();
        let sel_len = if state.cursor.has_selection() {
            state.cursor.selected_text().ok().unwrap_or_default().chars().count()
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

