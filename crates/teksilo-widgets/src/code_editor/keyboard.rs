// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Keyboard dispatch: navigation, IME composition, and basic editing.
//!
//! Every branch is gated by the policy's command filter before it touches a
//! cursor, so a read-only viewer needs no `if read_only` anywhere — the filter
//! rejects the command and the key falls through to whatever else wants it.
//!
//! The code-flavoured *semantics* — auto-indent, smart Tab, comment toggling,
//! line duplication and movement, bracket handling — are not here. They arrive
//! in their own module, because they are the part that reads
//! [`CodeConfig`](super::CodeConfig) and this part must stay true for any
//! configuration, including none.
//!
//! # Multi-caret
//!
//! Navigation moves every caret. Two rules make that safe rather than
//! surprising:
//!
//! - Edits apply back-to-front (see `frame_loop::insert_at_every_caret`), so an
//!   insertion cannot invalidate the carets not yet handled.
//! - Carets that land on the same position after a move are merged. Without
//!   that, pressing Home with two carets on one line silently leaves two carets
//!   stacked at column 0, and the next keystroke types every character twice.

use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::widget::EventContext;
use teksilo_text::text_document::{MoveMode, MoveOperation, SelectionType};

use super::clipboard;
use super::completion::{self, Trigger};
use super::policy::CodeCommand;
use super::semantics;
use super::state::{CodeEditorState, SharedState};
use super::sync_cursor_signals;
use crate::common::editor_runtime::CaretPolicy;
use crate::common::text_nav::{CaretStep, LineStep, caret_step, deletes_word, line_step};

/// What the dispatch decided, so the epilogue knows what to preserve.
#[derive(Copy, Clone, PartialEq, Eq)]
enum KeyAction {
    /// Handled; the sticky vertical column is now meaningless.
    ClearPreferredX,
    /// Handled; keep the sticky column (vertical motion).
    KeepPreferredX,
    /// Not ours.
    Unhandled,
}

pub(super) fn handle_key(
    state: &SharedState,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    if let WidgetEvent::ImeComposition { text, .. } = event {
        return handle_ime_composition(state, ctx, text);
    }
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
    // The platform accelerator — ⌘ on macOS, Ctrl elsewhere — so one chord
    // table serves every platform.
    let ctrl = modifiers.command();
    let alt = modifiers.alt();
    let mode = if shift {
        MoveMode::KeepAnchor
    } else {
        MoveMode::MoveAnchor
    };

    // Ctrl+Space always requests completion, open or not. Physical Control is
    // accepted here on every platform: ⌘Space belongs to Spotlight and never
    // reaches an app, so the accelerator form alone would leave forced
    // completion unreachable on macOS.
    if (ctrl || modifiers.ctrl()) && matches!(key, Key::Space) {
        completion::react(state, ctx, Trigger::Forced);
        return EventResponse::Handled;
    }

    // While the completion popup is open it owns navigation, accept, and
    // dismiss; other keys fall through to ordinary editing and re-filter it
    // afterwards. The editor keeps focus throughout — the popup is a detached
    // overlay, so these keys never reach it by bubbling; the editor drives it.
    if state.borrow().completion.is_open() {
        match key {
            Key::ArrowDown if !ctrl && !alt => {
                completion::move_selection(state, 1);
                ctx.request_frame();
                return EventResponse::Handled;
            }
            Key::ArrowUp if !ctrl && !alt => {
                completion::move_selection(state, -1);
                ctx.request_frame();
                return EventResponse::Handled;
            }
            Key::PageDown => {
                completion::move_selection(state, 5);
                ctx.request_frame();
                return EventResponse::Handled;
            }
            Key::PageUp => {
                completion::move_selection(state, -5);
                ctx.request_frame();
                return EventResponse::Handled;
            }
            // `modifiers.ctrl()` as well as the accelerator: a ⌃-modified Tab
            // belongs to focus navigation on every platform, macOS included, so
            // the popup must not swallow it.
            Key::Enter | Key::Tab if !shift && !ctrl && !modifiers.ctrl() => {
                completion::accept_selected(state, ctx);
                return EventResponse::Handled;
            }
            Key::Escape => {
                completion::dismiss_suppress(state, ctx);
                return EventResponse::Handled;
            }
            _ => {}
        }
    }

    let action = {
        let mut st = state.borrow_mut();
        let filter = st.policy.command_filter;

        match key {
            // --- Code semantics (config-driven) ---
            //
            // Placed first so the Alt / Ctrl+Alt arrow chords are claimed here
            // before the plain-navigation arrows below would swallow them. Each
            // is gated by its own command so a read-only viewer rejects the
            // mutating ones and the chord falls through — Alt+Up in a viewer is
            // just Up, which the navigation arm then handles.
            Key::ArrowUp if ctrl && alt && filter.accepts(CodeCommand::AddCaretAbove) => {
                semantics::add_caret_above(&mut st);
                KeyAction::KeepPreferredX
            }
            Key::ArrowDown if ctrl && alt && filter.accepts(CodeCommand::AddCaretBelow) => {
                semantics::add_caret_below(&mut st);
                KeyAction::KeepPreferredX
            }
            Key::ArrowUp if alt && filter.accepts(CodeCommand::MoveLineUp) => {
                semantics::move_lines(&mut st, semantics::MoveDir::Up);
                KeyAction::ClearPreferredX
            }
            Key::ArrowDown if alt && filter.accepts(CodeCommand::MoveLineDown) => {
                semantics::move_lines(&mut st, semantics::MoveDir::Down);
                KeyAction::ClearPreferredX
            }
            Key::Tab if !shift && filter.accepts(CodeCommand::IndentLines) => {
                semantics::indent_or_tab(&mut st);
                KeyAction::ClearPreferredX
            }
            Key::Tab if shift && filter.accepts(CodeCommand::DedentLines) => {
                semantics::dedent(&mut st);
                KeyAction::ClearPreferredX
            }
            Key::Character('/') if ctrl && filter.accepts(CodeCommand::ToggleLineComment) => {
                semantics::toggle_line_comment(&mut st);
                KeyAction::ClearPreferredX
            }
            Key::D if ctrl && !shift && filter.accepts(CodeCommand::DuplicateSelection) => {
                semantics::duplicate(&mut st);
                KeyAction::ClearPreferredX
            }

            // --- Clipboard ---
            //
            // Copy is not a mutation, so a read-only viewer's filter accepts it
            // (and rejects Cut / Paste). With no selection, Copy and Cut act on
            // the whole current line — the desktop convention.
            Key::C if ctrl && filter.accepts(CodeCommand::Copy) => {
                clipboard::copy(&st, ctx);
                KeyAction::ClearPreferredX
            }
            Key::X if ctrl && filter.accepts(CodeCommand::Cut) => {
                clipboard::cut(&mut st, ctx);
                KeyAction::ClearPreferredX
            }
            Key::V if ctrl && filter.accepts(CodeCommand::Paste) => {
                clipboard::paste(&mut st, ctx);
                KeyAction::ClearPreferredX
            }

            // --- Horizontal ---
            // The motion comes from `common::text_nav`, not from a bare "is
            // the accelerator held?": on macOS word-jump is ⌥←/→ and ⌘←/→
            // reaches the line edge, which no single boolean can say.
            Key::ArrowLeft if filter.accepts(horizontal_command(caret_step(*modifiers), false)) => {
                match caret_step(*modifiers) {
                    CaretStep::LineEdge => smart_home(&mut st, mode),
                    CaretStep::Word => move_every_caret(&mut st, MoveOperation::WordLeft, mode),
                    CaretStep::Character => move_every_caret(&mut st, MoveOperation::Left, mode),
                }
                KeyAction::ClearPreferredX
            }
            Key::ArrowRight if filter.accepts(horizontal_command(caret_step(*modifiers), true)) => {
                let op = match caret_step(*modifiers) {
                    CaretStep::LineEdge => MoveOperation::EndOfLine,
                    CaretStep::Word => MoveOperation::WordRight,
                    CaretStep::Character => MoveOperation::Right,
                };
                move_every_caret(&mut st, op, mode);
                KeyAction::ClearPreferredX
            }

            // --- Vertical ---
            // ⌘↑/↓ reaches the document edges on macOS. ⌥↑/↓ stays on
            // move-line here rather than becoming a paragraph motion: that is
            // the binding every code editor ships, on macOS too, and it is
            // matched below.
            Key::ArrowUp
                if matches!(line_step(*modifiers), LineStep::Document)
                    && filter.accepts(CodeCommand::MoveDocStart) =>
            {
                st.clear_extra_carets();
                st.cursor.move_position(MoveOperation::Start, mode, 1);
                KeyAction::ClearPreferredX
            }
            Key::ArrowDown
                if matches!(line_step(*modifiers), LineStep::Document)
                    && filter.accepts(CodeCommand::MoveDocEnd) =>
            {
                st.clear_extra_carets();
                st.cursor.move_position(MoveOperation::End, mode, 1);
                KeyAction::ClearPreferredX
            }
            Key::ArrowUp if filter.accepts(CodeCommand::MoveUp) => {
                move_every_caret(&mut st, MoveOperation::Up, mode);
                KeyAction::KeepPreferredX
            }
            Key::ArrowDown if filter.accepts(CodeCommand::MoveDown) => {
                move_every_caret(&mut st, MoveOperation::Down, mode);
                KeyAction::KeepPreferredX
            }
            Key::PageUp if filter.accepts(CodeCommand::PageUp) => {
                move_page(&mut st, -1, mode);
                KeyAction::KeepPreferredX
            }
            Key::PageDown if filter.accepts(CodeCommand::PageDown) => {
                move_page(&mut st, 1, mode);
                KeyAction::KeepPreferredX
            }

            // --- Line / document edges ---
            Key::Home if ctrl && filter.accepts(CodeCommand::MoveDocStart) => {
                st.clear_extra_carets();
                st.cursor.move_position(MoveOperation::Start, mode, 1);
                KeyAction::ClearPreferredX
            }
            Key::Home if filter.accepts(CodeCommand::MoveLineStart) => {
                smart_home(&mut st, mode);
                KeyAction::ClearPreferredX
            }
            Key::End if ctrl && filter.accepts(CodeCommand::MoveDocEnd) => {
                st.clear_extra_carets();
                st.cursor.move_position(MoveOperation::End, mode, 1);
                KeyAction::ClearPreferredX
            }
            Key::End if filter.accepts(CodeCommand::MoveLineEnd) => {
                move_every_caret(&mut st, MoveOperation::EndOfLine, mode);
                KeyAction::ClearPreferredX
            }

            // --- Selection ---
            Key::A if ctrl && filter.accepts(CodeCommand::SelectAll) => {
                // No escalation ladder: that is a rich-text affordance for
                // climbing out of a table cell, and there are no cells here.
                st.clear_extra_carets();
                st.cursor.select(SelectionType::Document);
                KeyAction::ClearPreferredX
            }

            // --- Carets ---
            Key::Escape => {
                // Only claim Escape when there is something to collapse;
                // otherwise it must reach the dialog or popover that wants it.
                if st.clear_extra_carets() {
                    KeyAction::ClearPreferredX
                } else {
                    KeyAction::Unhandled
                }
            }

            // --- Deletion ---
            Key::Backspace if filter.accepts(CodeCommand::DeletePrev) => {
                // Backspace between an empty auto-closed pair (`(|)`) deletes
                // both in one keystroke; otherwise the ordinary delete runs.
                // Never on a word-delete, which is a different verb.
                let word = deletes_word(*modifiers);
                if !word && semantics::try_pair_backspace(&mut st) {
                    KeyAction::ClearPreferredX
                } else {
                    delete_at_every_caret(&mut st, Direction::Backward, word);
                    KeyAction::ClearPreferredX
                }
            }
            Key::Delete if filter.accepts(CodeCommand::DeleteNext) => {
                delete_at_every_caret(&mut st, Direction::Forward, deletes_word(*modifiers));
                KeyAction::ClearPreferredX
            }

            // --- Newline ---
            //
            // Routed through the semantics layer, which carries the line's
            // indentation and opens a bracket block where the caret sits between
            // a pair. With neither configured it is a plain break, so this is
            // correct with no config at all.
            Key::Enter if filter.accepts(CodeCommand::InsertNewline) => {
                semantics::newline(&mut st);
                KeyAction::ClearPreferredX
            }

            // --- History ---
            Key::Z if ctrl && !shift && filter.accepts(CodeCommand::Undo) => {
                // Undoing text under several carets would leave them pointing
                // at offsets the undone edit invented; collapse first.
                st.clear_extra_carets();
                let _ = st.document.undo();
                KeyAction::ClearPreferredX
            }
            Key::Y if ctrl && filter.accepts(CodeCommand::Redo) => {
                st.clear_extra_carets();
                let _ = st.document.redo();
                KeyAction::ClearPreferredX
            }
            Key::Z if ctrl && shift && filter.accepts(CodeCommand::Redo) => {
                st.clear_extra_carets();
                let _ = st.document.redo();
                KeyAction::ClearPreferredX
            }

            // --- Printable input ---
            _ => {
                if ctrl {
                    // A Ctrl-chord we do not know is somebody else's shortcut.
                    KeyAction::Unhandled
                } else if let Some(t) = text.as_deref() {
                    if filter.accepts(CodeCommand::InsertChar) {
                        // Strip control characters: a Tab or newline arriving as
                        // `text` must go through its own handler, not be typed
                        // in literally.
                        let clean: String = t.chars().filter(|c| !c.is_control()).collect();
                        if clean.is_empty() {
                            KeyAction::Unhandled
                        } else if semantics::wants_bracket_handling(&st, &clean) {
                            // A configured bracket with auto-closing on takes the
                            // document-aware path (auto-close / type-over /
                            // surround); everything else stays on the batched
                            // fast path, so plain typing pays nothing.
                            let ch = clean.chars().next().expect("non-empty");
                            semantics::type_bracket_char(&mut st, ch);
                            KeyAction::ClearPreferredX
                        } else {
                            st.pending_chars.push_str(&clean);
                            KeyAction::ClearPreferredX
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

    let response = match action {
        KeyAction::Unhandled => EventResponse::Ignored,
        KeyAction::ClearPreferredX => {
            state.borrow_mut().preferred_x = None;
            caret_moved(state, ctx)
        }
        KeyAction::KeepPreferredX => caret_moved(state, ctx),
    };

    // Re-evaluate completion after a handled edit or move. `react` is a no-op
    // without a provider, and only *typing* (or Ctrl+Space, handled above) opens
    // a closed popup — an edit or move merely updates or dismisses an open one.
    if action != KeyAction::Unhandled
        && let Some(trigger) = completion_trigger(key, ctrl)
    {
        completion::react(state, ctx, trigger);
    }

    response
}

/// How a handled key should drive completion, or `None` for keys that never do
/// (Enter/Tab when the popup is closed).
///
/// Ctrl-chords are **not** ignored: Ctrl+A / Ctrl+Z / Ctrl+X / Ctrl+V / Ctrl+D /
/// Ctrl+/ / add-caret all changed the document or selection, so an open popup
/// must re-evaluate against the new state (and dismiss) — otherwise a following
/// Enter would accept a stale suggestion against the wrong range. Ctrl+Space is
/// handled earlier and never reaches here.
fn completion_trigger(key: &Key, ctrl: bool) -> Option<Trigger> {
    Some(match key {
        Key::Backspace | Key::Delete | Key::Space => Trigger::Edited,
        Key::ArrowLeft
        | Key::ArrowRight
        | Key::Home
        | Key::End
        | Key::ArrowUp
        | Key::ArrowDown
        | Key::PageUp
        | Key::PageDown => Trigger::Moved,
        // Any other Ctrl-chord that got here edited or moved; treat as an edit.
        _ if ctrl => Trigger::Edited,
        _ if key.to_char().is_some() => Trigger::Typed,
        _ => return None,
    })
}

/// The command a horizontal arrow resolves to, so the policy filter is asked
/// about the motion that will actually run — a `MoveWordLeft` veto has to bite
/// on ⌥← the same way it bites on Ctrl+←.
///
/// Split from the chord-reading so the table stays testable on any host; only
/// [`caret_step`] itself is platform-dependent.
fn horizontal_command(step: CaretStep, forward: bool) -> CodeCommand {
    match (step, forward) {
        (CaretStep::Character, false) => CodeCommand::MoveLeft,
        (CaretStep::Character, true) => CodeCommand::MoveRight,
        (CaretStep::Word, false) => CodeCommand::MoveWordLeft,
        (CaretStep::Word, true) => CodeCommand::MoveWordRight,
        (CaretStep::LineEdge, false) => CodeCommand::MoveLineStart,
        (CaretStep::LineEdge, true) => CodeCommand::MoveLineEnd,
    }
}

/// Shared tail for every handled key: reveal the caret, keep the OS IME window
/// under it, publish the signals, and ask for a frame.
fn caret_moved(state: &SharedState, ctx: &mut EventContext) -> EventResponse {
    ensure_caret_visible(state);
    sync_cursor_signals(state);
    report_ime_cursor_area(state, ctx);
    ctx.request_frame();
    EventResponse::Handled
}

// --- Caret motion ---------------------------------------------------------

/// Move every caret, then merge any that collided.
fn move_every_caret(st: &mut CodeEditorState, op: MoveOperation, mode: MoveMode) {
    st.cursor.move_position(op, mode, 1);
    for c in &mut st.extra_carets {
        c.move_position(op, mode, 1);
    }
    st.merge_collided_carets();
}

/// Home: to the first non-whitespace character, or to column 0 if already
/// there.
///
/// The near-universal code-editor behaviour, and genuinely useful rather than
/// clever: indented code's *content* starts at the indent, so that is where the
/// caret is wanted nine times out of ten — but column 0 must stay reachable for
/// re-indenting. Needs no language knowledge, so it lives here rather than with
/// the configured semantics.
/// Derived from the caret's position rather than a remembered flag.
///
/// This matters with several carets: a stored "last Home went to the indent"
/// bit is a single bit for a set of carets that can each be somewhere
/// different, so it would drive them all off whatever the *primary* happened to
/// be doing. Deriving per caret, each one toggles about its own line, and the
/// rule survives the caret being moved by anything else — a click, a
/// programmatic jump — with no flag to fall out of sync.
fn smart_home(st: &mut CodeEditorState, mode: MoveMode) {
    let primary_target = home_target(st, st.cursor.position());
    st.cursor.set_position(primary_target, mode);

    for i in 0..st.extra_carets.len() {
        let pos = st.extra_carets[i].position();
        let t = home_target(st, pos);
        st.extra_carets[i].set_position(t, mode);
    }
    st.merge_collided_carets();
}

/// Where Home should land a caret currently at `pos`.
///
/// At the first non-whitespace character → column 0. Anywhere else (including
/// column 0) → the first non-whitespace character. A line with no indent has
/// both in the same place, so it is simply column 0.
///
/// Asks the document for the *one block* containing `pos` rather than reading
/// the whole text. One block is one line here, so the block's `position` is the
/// line start and its `text` is the line — which is all the question needs.
///
/// The first version of this read `to_plain_text()` and collected the entire
/// document into a `Vec<char>`, three times per caret (the two helpers each did
/// it, and one called the other). Measured at 20k lines: 58 µs per copy → 174 µs
/// per caret per keypress, 870 µs at five carets, and worse from cold since any
/// edit invalidates the text cache — so Home right after typing paid a full
/// re-serialization of the document to find where one line started. Held Home
/// at key-repeat made that visible.
fn home_target(st: &CodeEditorState, pos: usize) -> usize {
    // Highlights are irrelevant to counting whitespace, and asking for them
    // would make the snapshot do more work.
    let Some(block) = st
        .document
        .snapshot_block_at_position_without_highlights(pos)
    else {
        return pos;
    };
    let line_start = block.position;
    let indent_col = leading_whitespace_columns(&block.text);
    let indent_pos = line_start + indent_col;
    if pos == indent_pos {
        line_start
    } else {
        indent_pos
    }
}

/// Count of leading whitespace characters, or 0 for a line that is entirely
/// whitespace — a blank line has no content to stop at, so Home has nowhere to
/// go but column 0.
fn leading_whitespace_columns(line: &str) -> usize {
    let indent = line.chars().take_while(|c| c.is_whitespace()).count();
    if indent == line.chars().count() {
        0
    } else {
        indent
    }
}

/// Page up/down by the viewport's worth of lines.
fn move_page(st: &mut CodeEditorState, direction: i32, mode: MoveMode) {
    let line_h = st.engine.default_line_height().max(1.0);
    let lines = ((st.viewport_height / line_h).floor() as i32 - 1).max(1);
    let op = if direction < 0 {
        MoveOperation::Up
    } else {
        MoveOperation::Down
    };
    st.cursor.move_position(op, mode, lines as usize);
    for c in &mut st.extra_carets {
        c.move_position(op, mode, lines as usize);
    }
    st.merge_collided_carets();
}

// --- Editing --------------------------------------------------------------

#[derive(Copy, Clone, PartialEq, Eq)]
enum Direction {
    Backward,
    Forward,
}

/// Delete at every caret, back-to-front so earlier deletions cannot invalidate
/// the carets not yet handled. One undo step for the whole batch.
fn delete_at_every_caret(st: &mut CodeEditorState, dir: Direction, by_word: bool) {
    let multi = !st.extra_carets.is_empty();
    if multi {
        st.cursor.begin_edit_block();
    }

    let mut order: Vec<usize> = (0..=st.extra_carets.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(caret_pos(st, i)));
    for i in order {
        delete_one(st, i, dir, by_word);
    }

    if multi {
        st.cursor.end_edit_block();
    }
    st.merge_collided_carets();
}

fn delete_one(st: &mut CodeEditorState, i: usize, dir: Direction, by_word: bool) {
    let c = caret_at(st, i);
    if by_word && !c.has_selection() {
        let op = match dir {
            Direction::Backward => MoveOperation::WordLeft,
            Direction::Forward => MoveOperation::WordRight,
        };
        c.move_position(op, MoveMode::KeepAnchor, 1);
    }
    if c.has_selection() {
        let _ = c.remove_selected_text();
        return;
    }
    match dir {
        Direction::Backward => {
            let _ = c.delete_previous_char();
        }
        Direction::Forward => {
            let _ = c.delete_char();
        }
    }
}

fn caret_pos(st: &CodeEditorState, i: usize) -> usize {
    if i == 0 {
        st.cursor.position()
    } else {
        st.extra_carets[i - 1].position()
    }
}

fn caret_at(st: &mut CodeEditorState, i: usize) -> &mut teksilo_text::text_document::TextCursor {
    if i == 0 {
        &mut st.cursor
    } else {
        &mut st.extra_carets[i - 1]
    }
}

// --- Scroll following -----------------------------------------------------

/// Scroll so the primary caret is on screen.
///
/// The primary only: chasing several at once is undefined (they can be pages
/// apart), and the primary is the one the user last placed.
pub(super) fn ensure_caret_visible(state: &SharedState) {
    let mut st = state.borrow_mut();
    if !st.engine.has_full_layout() {
        return;
    }
    let current = st.scroll_y.get();
    st.engine.set_scroll_offset(current);
    if let Some(new_off) = st.engine.ensure_caret_visible() {
        st.scroll_y.set_if_changed(new_off);
    }
}

// --- IME ------------------------------------------------------------------

/// Handle an in-progress composition (CJK, dead keys).
///
/// The preedit is inserted into the document so it shapes, wraps, and reads to
/// AT exactly like committed text — the alternative, an overlay, would need a
/// parallel layout path. Each new preedit removes the previous one first, so
/// the document always reflects the current IME state rather than accumulating
/// every intermediate.
///
/// The whole remove-then-insert is one edit block, so undo treats a composition
/// as one step rather than replaying it keystroke by keystroke.
///
/// Composition is single-caret: an input method has one composition window, and
/// there is no sensible meaning for composing into several places at once.
fn handle_ime_composition(
    state: &SharedState,
    ctx: &mut EventContext,
    text: &str,
) -> EventResponse {
    let filter = state.borrow().policy.command_filter;
    if !filter.accepts(CodeCommand::InsertChar) {
        return EventResponse::Ignored;
    }
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    if clean.is_empty() && state.borrow().ime_preedit_range.is_none() {
        return EventResponse::Ignored;
    }
    {
        let mut st = state.borrow_mut();
        st.clear_extra_carets();
        st.cursor.begin_edit_block();
        remove_preedit(&mut st);
        if clean.is_empty() {
            st.ime_preedit = None;
        } else {
            let start = st.cursor.position();
            let _ = st.cursor.insert_text(&clean);
            let end = st.cursor.position();
            st.ime_preedit = Some(clean);
            st.ime_preedit_range = Some(start..end);
        }
        st.cursor.end_edit_block();
        st.preferred_x = None;
        st.cursor_affinity = teksilo_text::CursorAffinity::Downstream;
        st.pending_text_changed = true;
    }
    report_ime_cursor_area(state, ctx);
    ctx.request_frame();
    EventResponse::Handled
}

/// Drop an in-flight preedit from the document.
///
/// Clamped against the live length: the document can have shrunk under us
/// (an undo, or a programmatic edit) between the preedit landing and this
/// running, and a stale range would delete the wrong text.
fn remove_preedit(st: &mut CodeEditorState) {
    let Some(range) = st.ime_preedit_range.take() else {
        return;
    };
    let doc_end = st.document.character_count();
    let start = range.start.min(doc_end);
    let end = range.end.min(doc_end);
    if start < end {
        st.cursor.set_position(start, MoveMode::MoveAnchor);
        st.cursor.set_position(end, MoveMode::KeepAnchor);
        let _ = st.cursor.remove_selected_text();
    }
}

/// Cancel any composition, discarding the tentative text.
///
/// Called on focus loss and before a commit. Leaving a preedit in the document
/// when focus moves away would make tentative text permanent.
pub(super) fn clear_ime_preedit(state: &SharedState) {
    let mut st = state.borrow_mut();
    remove_preedit(&mut st);
    st.ime_preedit = None;
}

fn push_pending_chars(state: &SharedState, ctx: &mut EventContext, text: &str) -> EventResponse {
    if text.is_empty() {
        return EventResponse::Ignored;
    }
    let filter = state.borrow().policy.command_filter;
    if !filter.accepts(CodeCommand::InsertChar) {
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
        st.cursor_affinity = teksilo_text::CursorAffinity::Downstream;
    }
    report_ime_cursor_area(state, ctx);
    ctx.request_frame();
    EventResponse::Handled
}

/// Tell the OS where to put the IME candidate window.
///
/// Deduped against the last reported rect: re-sending an unchanged area is not
/// just waste — on some Linux IME backends (ibus/fcitx) it echoes back an empty
/// preedit, which is a self-sustaining loop.
pub(super) fn report_ime_cursor_area(state: &SharedState, ctx: &mut EventContext) {
    let rect = {
        let st = state.borrow();
        if !st.has_focus
            || st.policy.is_read_only()
            || matches!(st.policy.caret_policy, CaretPolicy::Hidden)
            || !st.engine.has_full_layout()
        {
            return;
        }
        let c = st
            .engine
            .caret_rect(st.cursor.position(), st.cursor_affinity);
        // Subtract BOTH scroll axes: `caret_rect` returns content-local
        // coordinates, and CodeEditor defaults to WrapMode::None, so horizontal
        // scroll is the everyday state — omitting scroll_x misplaces the IME
        // candidate window (and any caret-anchored popup) once scrolled right.
        teksilo_canvas::Rect::new(
            st.viewport_origin.x + c[0] - st.scroll_x.get(),
            st.viewport_origin.y + c[1] - st.scroll_y.get(),
            c[2].max(1.0),
            c[3],
        )
    };
    let changed = {
        let st = state.borrow();
        st.last_ime_area != Some(rect)
    };
    if !changed {
        return;
    }
    state.borrow_mut().last_ime_area = Some(rect);
    ctx.set_ime_cursor_area(rect);
}

/// A document position's rectangle in absolute window (tree) coordinates, or
/// `None` before a layout exists — the caret's `caret_rect` (content-local)
/// shifted by the body's window origin and both scroll offsets, matching the
/// paint path. Used to anchor a caret-relative popup (completion) at a chosen
/// position, e.g. the start of the word being completed rather than the moving
/// caret.
pub(super) fn window_rect_at(st: &CodeEditorState, pos: usize) -> Option<teksilo_canvas::Rect> {
    if !st.engine.has_full_layout() {
        return None;
    }
    let c = st.engine.caret_rect(pos, st.cursor_affinity);
    Some(teksilo_canvas::Rect::new(
        st.viewport_origin.x + c[0] - st.scroll_x.get(),
        st.viewport_origin.y + c[1] - st.scroll_y.get(),
        c[2].max(1.0),
        c[3],
    ))
}

/// Drive `smart_home` from the test module without a synthetic KeyDown.
///
/// The dispatch path is exercised elsewhere; this targets the toggle rule
/// itself, which is the part with a history of being written confusingly.
#[cfg(test)]
pub(super) fn smart_home_for_test(state: &SharedState) {
    let mut st = state.borrow_mut();
    smart_home(&mut st, MoveMode::MoveAnchor);
}

#[cfg(test)]
mod motion_tests {
    use super::*;

    // The chord → motion reading is covered in `common::text_nav`; what
    // matters here is that each motion reaches the command the policy filter
    // is asked about. On a Linux host `CaretStep::LineEdge` never arises from
    // a real chord, so the mapping is only ever exercised by naming the step
    // directly — which is the point of taking one.

    #[test]
    fn every_caret_step_maps_to_its_own_command() {
        assert_eq!(
            horizontal_command(CaretStep::Character, false),
            CodeCommand::MoveLeft
        );
        assert_eq!(
            horizontal_command(CaretStep::Character, true),
            CodeCommand::MoveRight
        );
        assert_eq!(
            horizontal_command(CaretStep::Word, false),
            CodeCommand::MoveWordLeft
        );
        assert_eq!(
            horizontal_command(CaretStep::Word, true),
            CodeCommand::MoveWordRight
        );
        assert_eq!(
            horizontal_command(CaretStep::LineEdge, false),
            CodeCommand::MoveLineStart
        );
        assert_eq!(
            horizontal_command(CaretStep::LineEdge, true),
            CodeCommand::MoveLineEnd
        );
    }

    #[test]
    fn the_command_a_step_reports_is_the_one_that_runs() {
        // A policy that forbids word motion must veto ⌥← on macOS exactly as
        // it vetoes Ctrl+← elsewhere. That only holds while the filter is
        // asked about the resolved motion rather than about `MoveLeft`.
        for forward in [false, true] {
            assert_ne!(
                horizontal_command(CaretStep::Word, forward),
                horizontal_command(CaretStep::Character, forward),
                "a word motion must not be filtered as a character motion"
            );
            assert_ne!(
                horizontal_command(CaretStep::LineEdge, forward),
                horizontal_command(CaretStep::Character, forward),
            );
        }
    }
}
