// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Keyboard dispatch for the text input widget.
//!
//! Simplified from `rich_text::keyboard`: no vertical navigation, no
//! format toggles, no select-all escalation ladder, no preferred-X. Enter
//! fires the on_submit command instead of inserting a newline.

use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::shortcut::StandardEdit;
use teksilo_core::widget::EventContext;
use teksilo_platform::clipboard::ClipboardHandle;
use teksilo_text::CursorAffinity;
use teksilo_text::text_document::{MoveMode, MoveOperation, SelectionType};

use crate::EditSource;
use crate::common::composition::position_after_commit;
use crate::common::text_nav::{CaretStep, caret_step, deletes_word};

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
        teksilo_canvas::Rect::new(
            st.viewport_origin.x + caret[0] - st.scroll_x,
            st.viewport_origin.y + st.text_offset_y + caret[1],
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
    if let WidgetEvent::ImeComposition {
        text,
        cursor,
        replacement,
    } = event
    {
        return handle_ime_composition(state, ctx, text, cursor.as_ref(), replacement.as_ref());
    }
    // IME commit: a finalized grapheme cluster, or text the input system wrote
    // on its own account (dictation, Writing Tools, the emoji viewer). The
    // commit of a composition takes its place; text with no composition before
    // it is batched into pending_chars like ordinary typed input, or replaces
    // the range the input system named.
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

    let shift = modifiers.shift();
    // The platform accelerator — ⌘ on macOS, Ctrl elsewhere. See the
    // rich-text sibling for why this is not `ctrl() || super_key()`.
    let ctrl = modifiers.command();
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
            // Word-jump rides ⌥ on macOS and Ctrl elsewhere, and ⌘←/→ reach
            // the field's edges — the single-line reading of "line edge". See
            // `common::text_nav`.
            Key::ArrowLeft => {
                let op = match caret_step(*modifiers) {
                    CaretStep::Character => MoveOperation::Left,
                    CaretStep::Word => MoveOperation::WordLeft,
                    CaretStep::LineEdge => MoveOperation::Start,
                };
                st.cursor.move_position(op, mode, 1);
                true
            }
            Key::ArrowRight => {
                let op = match caret_step(*modifiers) {
                    CaretStep::Character => MoveOperation::Right,
                    CaretStep::Word => MoveOperation::WordRight,
                    CaretStep::LineEdge => MoveOperation::End,
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

            // Select all, the clipboard and undo / redo are not keys here:
            // they arrive as the `edit.*` actions `run_standard_edit` serves,
            // so the user can rebind them.

            // ── Editing (gated on !read_only) ───────────────────────
            Key::Backspace if !read_only => {
                if deletes_word(*modifiers) {
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
                if deletes_word(*modifiers) {
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
                            .filter(|c| single_line_admits(*c))
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
                                st.pending_arrivals
                                    .note(EditSource::Keyboard, truncated.chars().count());
                                st.pending_chars.push_str(&truncated);
                            } else {
                                st.pending_arrivals
                                    .note(EditSource::Keyboard, clean.chars().count());
                                st.pending_chars.push_str(&clean);
                            }
                            true
                        } else if t.chars().any(single_line_admits) {
                            // Real text, rejected wholesale by the
                            // filter or the max length: swallow it, so a
                            // digits-only field doesn't let a rejected
                            // "s" fall through and match a Save
                            // shortcut.
                            return EventResponse::Handled;
                        } else {
                            // Nothing but control characters, so this
                            // key never carried text at all — the
                            // platform simply attaches some to a named
                            // key, and winit hands Escape through as
                            // U+001B. Stripping that to nothing is not a
                            // rejection, and swallowing it on that
                            // ground is what stopped Escape from ever
                            // leaving a focused field: every "Escape
                            // closes this" affordance above one was dead
                            // — the cell editor that would not shut, and
                            // any dialog whose cancel sits outside the
                            // field. Bubble instead, the way `Key::Tab`
                            // above already does.
                            //
                            // Not keyed on the `Key` variant: a typed
                            // letter arrives as `Key::A`, not
                            // `Key::Character('a')`, so "is it a
                            // character key" would quietly stop
                            // swallowing rejected letters.
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
        report_ime_cursor_area(state, ctx);
        ctx.request_frame();
        EventResponse::Handled
    } else {
        EventResponse::Ignored
    }
}

/// Whether one of the standard edit commands has anything to act on in the
/// field now, which is what greys its menu rows: Copy a selection a masked
/// field does not hide, Cut that in an editable field, Paste an editable
/// field, Select All some text, Undo and Redo an editable field with history
/// that way. Never asked by a dispatch, which runs
/// [`run_standard_edit`] whatever this answers. `true` while the state is
/// borrowed, the answer from before there was a question.
pub(crate) fn standard_edit_available(state: &SharedState, command: StandardEdit) -> bool {
    let Ok(st) = state.try_borrow() else {
        return true;
    };
    let editable = !st.read_only;
    let copyable = st.cursor.has_selection() && st.copy_allowed();
    match command {
        StandardEdit::Copy => copyable,
        StandardEdit::Cut => editable && copyable,
        StandardEdit::Paste | StandardEdit::PastePlain => editable,
        StandardEdit::SelectAll => st.document.character_count() > 0,
        StandardEdit::Undo => editable && st.document.can_undo(),
        StandardEdit::Redo => editable && st.document.can_redo(),
        _ => false,
    }
}

/// Whether the field declines a standard edit command now, which passes the
/// intent on: a read-only field declines every command that would change its
/// text, and with `empty_history_passes_on` (the tree's
/// [`TextSurfaces::set_empty_history_passes_on`](teksilo_core::text_surface::TextSurfaces::set_empty_history_passes_on))
/// it declines Undo with nothing to undo and Redo with nothing to redo.
/// [`run_standard_edit`] declines exactly these, and a menu row asks this to
/// know whether the field is where its command stops. `false` while the state
/// is borrowed.
pub(crate) fn standard_edit_passes_on(
    state: &SharedState,
    command: StandardEdit,
    empty_history_passes_on: bool,
) -> bool {
    state
        .try_borrow()
        .is_ok_and(|st| declines(&st, command, empty_history_passes_on))
}

fn declines(st: &TextInputState, command: StandardEdit, empty_history_passes_on: bool) -> bool {
    match command {
        StandardEdit::SelectAll | StandardEdit::Copy => false,
        StandardEdit::Cut | StandardEdit::Paste | StandardEdit::PastePlain => st.read_only,
        StandardEdit::Undo => st.read_only || (empty_history_passes_on && !st.document.can_undo()),
        StandardEdit::Redo => st.read_only || (empty_history_passes_on && !st.document.can_redo()),
        _ => true,
    }
}

/// Whether the field takes `command` at all, from any route: what its keys
/// decline (a read-only field takes no cut, paste or history step), and a cut
/// or a copy of a field whose text may not leave it (a masked password).
/// `TextSurface::accepts` answers with it, and the surface's own commands obey
/// it.
pub(crate) fn accepts_standard_edit(st: &TextInputState, command: StandardEdit) -> bool {
    !declines(st, command, false)
        && match command {
            StandardEdit::Cut | StandardEdit::Copy => st.copy_allowed(),
            _ => true,
        }
}

/// Run one of the standard edit commands for the field, as its `edit.*`
/// action does. Returns whether the command ran: it declines what
/// [`standard_edit_passes_on`] says it does, which passes the intent on.
pub(crate) fn run_standard_edit(
    state: &SharedState,
    command: StandardEdit,
    empty_history_passes_on: bool,
    ctx: &mut EventContext,
) -> bool {
    // A command that edits the text ends a composition in progress first, its
    // text kept, as a click elsewhere does (see `finish_composition`). Left
    // alive, the edit would take the composition out of the text while the
    // input method still held it, and its next update would bring it back at
    // the caret.
    let edits_text = matches!(
        command,
        StandardEdit::Cut
            | StandardEdit::Paste
            | StandardEdit::PastePlain
            | StandardEdit::Undo
            | StandardEdit::Redo
    );
    if edits_text && !declines(&state.borrow(), command, false) {
        finish_composition(state, ctx);
    }
    {
        let mut st = state.borrow_mut();
        if declines(&st, command, empty_history_passes_on) {
            return false;
        }
        match command {
            StandardEdit::SelectAll => st.cursor.select(SelectionType::Document),
            StandardEdit::Copy => clipboard_copy(&mut st, ctx),
            StandardEdit::Cut => clipboard_cut(&mut st, ctx),
            // A plain field has no formatting to strip.
            StandardEdit::Paste | StandardEdit::PastePlain => clipboard_paste(&mut st, ctx),
            StandardEdit::Undo => {
                let _ = st.document.undo();
            }
            StandardEdit::Redo => {
                let _ = st.document.redo();
            }
            _ => return false,
        }
    }
    sync_cursor_signals(state);
    report_ime_cursor_area(state, ctx);
    ctx.request_frame();
    true
}

/// Apply an `ImeComposition` event: show its text as the composition, a
/// transient edit no undo history records (see [`crate::common::composition`]).
/// Each update replaces the text of the one before. Single-line rules apply
/// (newlines / control chars stripped, `char_filter` enforced, `max_length`
/// respected, counting neither the composition's earlier text nor the text it
/// takes the place of); secure-field masking is handled at the engine layer
/// so a password preedit renders as bullets. An update the rules leave
/// nothing of cancels the composition.
///
/// A first update that names a `replacement` (the input method marked text
/// already in the field, as Korean Hanja conversion does) shows the
/// composition in place of that text, which a cancel puts back. The empty
/// update leaves the composition's text where it is: winit sends one right
/// before every commit too, so the commit that follows decides (see
/// [`commit_composition`]), and anything else cancels it (see
/// [`settle_cancelled_composition`]).
fn handle_ime_composition(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
    cursor: Option<&std::ops::Range<usize>>,
    replacement: Option<&std::ops::Range<usize>>,
) -> EventResponse {
    if state.borrow().read_only {
        return EventResponse::Handled;
    }
    if text.chars().all(|c| !single_line_admits(c)) {
        // The empty update of a live composition: leave its text where it is
        // until the next event says whether it is committed. The frame asked
        // for here settles a cancel if nothing else does first.
        //
        // With no live composition it is a genuine no-op. Some Linux IME
        // backends (ibus / fcitx via winit) flood empty `Ime::Preedit("")`
        // events while a field is focused; processing each one (a signal sync,
        // an un-deduped IME-area report that re-arms that very loop, a repaint)
        // is pure waste. Bail before any of it. An ended composition waits on:
        // the flood must not cancel what the commit after it would commit.
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
        // The preedit goes in after what was typed before it in this frame,
        // as in the rich text editor: the layout caught up below inserts that
        // typing too, and would otherwise put it after the preedit. A recorded
        // insertion, which would revert a live composition: there is none,
        // unless keys typed during it, and it is then started again.
        st.apply_pending_chars();
        st.composition.reconcile(&st.document);
        let live = st.composition.is_live(&st.document);
        if !live {
            st.composition.begin(&st.document);
            // The first update of a composition that takes the place of text
            // already in the field. Later updates name the same text, which
            // the composition now stands in for: they replace its text.
            if let Some(range) = replacement {
                let doc_end = st.document.character_count();
                let start = range.start.min(doc_end);
                let end = range.end.min(doc_end).max(start);
                st.cursor.set_position(start, MoveMode::MoveAnchor);
                st.cursor.set_position(end, MoveMode::KeepAnchor);
            }
        }
        // Single-line cleaning + per-character filter.
        let clean: String = text
            .chars()
            .filter(|c| single_line_admits(*c))
            .filter(|c| st.char_filter_admits(*c))
            .collect();
        // Max-length: cap against the text the composition leaves in place.
        // What it replaces is its own earlier text when it is live, else the
        // selection, which is the text a replacement names.
        let clean = if let Some(max) = st.max_length {
            let taken = if live {
                st.document.transient_range().map_or(0, |range| range.len())
            } else {
                st.cursor.position().abs_diff(st.cursor.anchor())
            };
            let kept = st.document.character_count().saturating_sub(taken);
            clean
                .chars()
                .take(max.saturating_sub(kept))
                .collect::<String>()
        } else {
            clean
        };
        if clean.is_empty() {
            // Nothing of it is the field's to show.
            st.composition.cancel(&st.document);
        } else if st.composition.show(&st.document, &st.cursor, &clean) {
            // Which part the input method is on: the segment it converts, or
            // the caret's place inside the composition.
            let kept: Vec<bool> = text
                .chars()
                .map(|c| single_line_admits(c) && st.char_filter_admits(c))
                .collect();
            st.composition.mark(text, cursor, &kept);
            st.composition.place_caret(&st.document, &st.cursor);
        }
        st.pending_text_changed = true;
        // Lay the preedit out now, not at the next frame: the area reported
        // below is where the platform opens its candidate window, and it is
        // read off the layout. Without the preedit in it, the caret's offset
        // after the preedit lies that many characters further into the old
        // text, off by the difference between their widths.
        super::catch_up_layout(st);
        // And scroll it into view as the next paint will: at the end of a
        // field that overflows, the caret after the preedit is past the
        // trailing edge until the text scrolls.
        if let Some(follow) = st.caret_follow {
            super::ensure_caret_visible_h(st, follow);
        }
    }
    sync_cursor_signals(state);
    report_ime_cursor_area(state, ctx);
    ctx.request_frame();
    EventResponse::Handled
}

/// Cancel a composition the input method ended with its empty update and did
/// not commit: the field back as it was before it, the text it took the place
/// of included, nothing recorded. Called with whatever comes instead of the
/// commit (a key, another composition, a press, a focus change) and at the
/// start of every frame. Returns whether it changed the field.
pub(crate) fn settle_cancelled_composition(st: &mut TextInputState) -> bool {
    st.composition.reconcile(&st.document);
    if !st.composition.is_ended(&st.document) {
        return false;
    }
    st.pending_text_changed = true;
    st.composition.cancel(&st.document)
}

/// The commit of a composition: the committed text takes the composition's
/// place, and the place of the text it stood in for, as one undo step of its
/// own (see [`Composition::commit`](crate::common::composition::Composition::commit)),
/// under the rules typing follows (the single-line strip, the `char_filter`,
/// the `max_length` cap, counted without the composition and what it
/// replaces). Reported under `source`. A commit the rules leave nothing of,
/// and an empty one, commit nothing: the composition ends as a cancel.
fn commit_composition(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
    source: EditSource,
) -> EventResponse {
    {
        let mut guard = state.borrow_mut();
        let st = &mut *guard;
        if st.read_only {
            // The field stopped taking text while the composition was shown.
            st.composition.cancel(&st.document);
            return EventResponse::Handled;
        }
        let clean = committable(st, text);
        let arrived = st
            .composition
            .commit(&st.document, &st.cursor, &clean, false);
        st.report_inserted_chars(source, arrived);
        st.pending_text_changed = true;
        // A press may follow before the next frame (the Microsoft Korean IME
        // commits its syllable on the mouse-down itself), and it hit-tests the
        // layout: bring it up to date, as the area reported below needs too.
        super::catch_up_layout(st);
    }
    sync_cursor_signals(state);
    report_ime_cursor_area(state, ctx);
    ctx.request_frame();
    EventResponse::Handled
}

/// What of `text`, committed in place of the composition, the field takes: the
/// rules typing follows (the single-line strip, the `char_filter`, the
/// `max_length` cap, counted without the composition and what it replaces).
fn committable(st: &TextInputState, text: &str) -> String {
    let clean: String = text
        .chars()
        .filter(|c| single_line_admits(*c))
        .filter(|c| st.char_filter_admits(*c))
        .collect();
    match st.max_length {
        Some(max) => {
            let taken = st.document.transient_range().map_or(0, |range| range.len());
            let kept = st.document.character_count().saturating_sub(taken);
            clean.chars().take(max.saturating_sub(kept)).collect()
        }
        None => clean,
    }
}

/// Insert the typing and the input method's commit batched for this frame
/// where they were typed, before something other than a key moves the caret
/// or the selection to `offsets` (a programmatic selection, an assistive
/// client's), read from the text without the batch: each offset is carried
/// across it as the document carries a cursor. Left for the next frame, the
/// batch went in at the caret the move had just placed, over the selection it
/// had just made.
pub(crate) fn apply_pending_chars_keeping(st: &mut TextInputState, offsets: &mut [usize]) {
    if st.pending_chars.is_empty() {
        return;
    }
    let held: Vec<_> = offsets
        .iter()
        .map(|&at| st.document.cursor_at(at))
        .collect();
    st.apply_pending_chars();
    for (offset, cursor) in offsets.iter_mut().zip(held) {
        *offset = cursor.position();
    }
}

/// Before a pointer moves the caret or starts a selection: insert the typing
/// and the input system's commits batched for this frame (those with no
/// composition before them: a dead key, the emoji panel) where they were
/// typed, and lay the text out with them, so that the pointer hit-tests the
/// text the screen showed. Returns whether there was a batch.
///
/// Left for the next frame, the batch went in at the caret the pointer had
/// just placed. The commit of a composition is not batched: it takes the
/// composition's place as it arrives (see [`commit_composition`]), which is
/// where the Microsoft Korean IME's commit on the mouse-down itself lands.
pub(crate) fn insert_typing_before_pointer(st: &mut TextInputState) -> bool {
    if st.pending_chars.is_empty() {
        return false;
    }
    super::catch_up_layout(st);
    true
}

/// Before a pointer moves the caret or starts a selection (a press, a tap's
/// release, a hold, a multi-tap): the typing and the input method's commit
/// batched for this frame go in where they were typed and are laid out (see
/// [`insert_typing_before_pointer`]), then a composition in progress ends with
/// its text kept (see [`finish_composition`]).
pub(crate) fn settle_before_press(state: &SharedState, ctx: &mut EventContext) {
    if insert_typing_before_pointer(&mut state.borrow_mut()) {
        sync_cursor_signals(state);
        ctx.request_frame();
    }
    finish_composition(state, ctx);
}

/// End the composition the input method still holds, keeping it as committed
/// text, for a press in the field or a focus change: what a native text field
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
pub(crate) fn finish_composition(state: &SharedState, ctx: &mut EventContext) {
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
        super::catch_up_layout(st);
    }
    sync_cursor_signals(state);
    ctx.request_frame();
}

/// Commit the live composition where it stands, as its own commit would,
/// reported as [`EditSource::Ime`]: as `committed`, the input method's commit
/// for it, under the rules a commit follows ([`committable`]), or as the text
/// it showed when there is none or the rules leave nothing of it. The caret
/// ends after the text, unless it stood outside the composition (a
/// right-click put it there before the menu took focus), where it stays.
fn keep_composition(st: &mut TextInputState, committed: Option<&str>) {
    let caret = (st.cursor.anchor(), st.cursor.position());
    let range = st.composition.range(&st.document);
    let outside = range.as_ref().is_some_and(|range| {
        let inside = |at: usize| range.start <= at && at <= range.end;
        !inside(caret.0) || !inside(caret.1)
    });
    // The text shown was held to the field's rules when it was shown; a
    // written composition's (the text service's, which it was asked to end
    // and did not) meets them now.
    let shown = if st.composition.is_written(&st.document) {
        st.composition
            .written_text(&st.document)
            .map(|text| committable(st, &text))
    } else {
        st.composition.text(&st.document).map(str::to_owned)
    };
    let Some(shown) = shown else {
        return;
    };
    let text = committed
        .map(|text| committable(st, text))
        .filter(|text| !text.is_empty())
        .unwrap_or(shown);
    let arrived = st
        .composition
        .commit(&st.document, &st.cursor, &text, false);
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
    st.pending_text_changed = true;
}

/// Batch pending characters from an IME commit, counted under `source`.
fn push_pending_chars(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
    source: EditSource,
) -> EventResponse {
    let read_only = state.borrow().read_only;
    if read_only {
        return EventResponse::Handled;
    }
    // Strip newlines (single-line enforcement), control characters,
    // and anything the optional filter rejects.
    let clean: String = {
        let st = state.borrow();
        text.chars()
            .filter(|c| single_line_admits(*c))
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
            st.pending_arrivals.note(source, truncated.chars().count());
            st.pending_chars.push_str(&truncated);
        } else {
            st.pending_arrivals.note(source, clean.chars().count());
            st.pending_chars.push_str(&clean);
        }
    }
    sync_cursor_signals(state);
    report_ime_cursor_area(state, ctx);
    ctx.request_frame();
    EventResponse::Handled
}

/// Replace `range` with `text`, as the input system asked: a dictation
/// correction, a Writing Tools rewrite. One undo step, reported once under
/// `source`.
///
/// `range` is in character offsets, the units of `TextCursor::position`, and
/// is clamped to the text: the input system read it a moment ago and may name
/// a range an edit since has shortened. The text goes through the rules typing
/// does (the single-line strip, the `char_filter`, the `max_length` cap, here
/// counted with the range already gone), and what survives is what is
/// reported. A text the rules refuse entirely leaves the field as it was, as a
/// refused keystroke or paste does; an empty text deletes the range.
fn replace_from_input_system(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
    range: std::ops::Range<usize>,
    source: EditSource,
) -> EventResponse {
    if state.borrow().read_only {
        return EventResponse::Handled;
    }
    {
        let mut st = state.borrow_mut();
        // The typing batched ahead of this commit lands first, so the range is
        // read against the text the input system saw.
        st.apply_pending_chars();
        let doc_end = st.document.character_count();
        let start = range.start.min(doc_end);
        let end = range.end.min(doc_end).max(start);
        let clean: String = text
            .chars()
            .filter(|c| single_line_admits(*c))
            .filter(|c| st.char_filter_admits(*c))
            .collect();
        let clean: String = match st.max_length {
            Some(max) => {
                let kept = doc_end - (end - start);
                clean.chars().take(max.saturating_sub(kept)).collect()
            }
            None => clean,
        };
        if clean.is_empty() && !text.is_empty() {
            // Refused by the field's rules: nothing lands, nothing goes.
            return EventResponse::Handled;
        }
        st.cursor.begin_edit_block();
        st.cursor.set_position(start, MoveMode::MoveAnchor);
        st.cursor.set_position(end, MoveMode::KeepAnchor);
        if clean.is_empty() {
            if start < end {
                let _ = st.cursor.remove_selected_text();
            }
        } else {
            let _ = st.cursor.insert_text(&clean);
        }
        st.cursor.end_edit_block();
        st.pending_text_changed = true;
        st.report_inserted_chars(source, clean.chars().count());
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

/// Whether a single-line field keeps `c`.
///
/// Not a control character (a `\n`, a tab, a NUL), nor a line or paragraph
/// separator (U+2028, U+2029): the typesetter breaks the line at either, and a
/// single-line field would show two. Typing, input methods and paste all ask
/// this, so they keep out the same characters.
pub(super) fn single_line_admits(c: char) -> bool {
    !c.is_control() && !matches!(c, '\u{2028}' | '\u{2029}')
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
        .filter(|c| single_line_admits(*c))
        .filter(|c| state.char_filter_admits(*c))
        .collect();
    if clean.is_empty() {
        return;
    }
    // A paste is an undo step of its own, as in a native field: it goes in
    // with the call typing uses, whose merge rule would otherwise join it to
    // the word typed just before it, and the text typed just after it to it.
    state.document.break_undo_merge();
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
        state.report_inserted_chars(EditSource::Clipboard, truncated.chars().count());
    } else {
        let _ = state.cursor.insert_text(&clean);
        state.report_inserted_chars(EditSource::Clipboard, clean.chars().count());
    }
    state.document.break_undo_merge();
    state.cursor.clear_selection();
    state.pending_text_changed = true;
}
