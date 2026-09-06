// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The accessibility walk shared by the editor and log bodies.
//!
//! Both surfaces present their text the same way: one `Role::TextRun` per
//! visual line of a block, hung directly off the body's own node, carrying the
//! per-character lengths, word starts and geometry a screen reader needs to
//! speak and navigate by character, word and line. Everything about how a run
//! is shaped — the 255-character cap, the hard break at the end of a line,
//! UAX #29 word starts, the `next_on_line` chain, the degenerate box for text
//! the layout never measured — belongs to
//! [`teksilo_core::accessibility::text_runs`], which this walk feeds one
//! [`TextRunSource`] per block.
//!
//! The two surfaces differ only in *which* blocks they walk: the editor walks
//! the whole (bounded) document; the log walks only the visible window, because
//! re-emitting a node per line of a 100 000-line buffer on every appended line
//! would be O(N) per line.
//!
//! Runs are direct children of the body, with no `Role::Paragraph` between.
//! `accesskit_consumer`'s `common_filter` excludes `Role::TextRun` but not
//! `Role::Paragraph`, so one would be a visible object-navigation stop; worse,
//! a run's text-change event routes to its *filtered* parent, and a paragraph's
//! `supports_text_ranges()` is false, so macOS, Windows and AT-SPI would each
//! drop the event. A heading block is the one exception: it earns a
//! `Role::Heading` node of its own with its runs beneath it, because the
//! heading level is information no run carries.
//!
//! **Removed with the paragraphs: the "line 42 of 200" ordinal.** Carrying it
//! as `position_in_set` required a node per line, which is exactly what breaks
//! text-change events. Line position is reachable through line navigation on
//! every platform, and visible in `CodeGutter`.
//!
//! Per-run formatting (bold / italic / underline) is not announced either: a
//! source declares one set of attributes for all its runs and this walk builds
//! one source per block. Neither surface's own highlighter sets those flags —
//! the code editor colours tokens, it does not embolden them.

use std::collections::HashMap;

use teksilo_canvas::{LineEnd, Point, Rect, TextGeometry};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accessibility::text_runs::{TextRunSource, push_text_runs};
use teksilo_core::accesskit::{Action, ActionData, NodeId, Role, TextPosition};
use teksilo_core::event::EventResponse;
use teksilo_core::widget::EventContext;
use teksilo_text::RichTextEngine;
use teksilo_text::text_document::{BlockSnapshot, FlowElementSnapshot, MoveMode};

use super::state::{CodeEditorState, SharedState, SyntheticElementRef};
use crate::common::editor_runtime::AccessibilityRole;

/// Set the role and read-only flag — the header both bodies share.
pub(crate) fn set_role(st: &CodeEditorState, builder: &mut AccessNodeBuilder) {
    let role = match st.policy.access_role {
        AccessibilityRole::Editor => Role::MultilineTextInput,
        AccessibilityRole::Document => Role::Document,
    };
    builder.set_role(role);
    if st.policy.is_read_only() {
        builder.set_read_only();
    }
}

/// The document selection the AT tree should report: the IME composition while
/// composing, else the primary caret (secondary carets are editing-only and the
/// AT tree reports just the primary, as documented on the state).
fn user_selection(st: &CodeEditorState) -> (usize, usize) {
    match st.ime_preedit_range.clone() {
        Some(range) => (range.start, range.end),
        None => (st.cursor.anchor(), st.cursor.position()),
    }
}

/// Accumulated while walking: the synthetic-node → document-range map (for
/// resolving an AT selection back to a cursor) and the run + offset the user's
/// caret / anchor resolved to.
struct WalkAcc {
    user_pos: usize,
    user_anchor: usize,
    caret_pair: Option<(NodeId, usize)>,
    anchor_pair: Option<(NodeId, usize)>,
    syn_map: HashMap<NodeId, SyntheticElementRef>,
}

impl WalkAcc {
    fn new(st: &CodeEditorState) -> Self {
        let (user_anchor, user_pos) = user_selection(st);
        Self {
            user_pos,
            user_anchor,
            caret_pair: None,
            anchor_pair: None,
            syn_map: HashMap::new(),
        }
    }
}

/// The editor's walk: the runs of every block of the whole document. The
/// snapshot is cached (rebuilt only when an edit cleared it) since the document
/// is bounded.
pub(crate) fn build_editor_a11y(st: &CodeEditorState, builder: &mut AccessNodeBuilder) {
    set_role(st, builder);

    let snap = {
        let mut cache = st.accessibility_flow_snapshot.borrow_mut();
        if cache.is_none() {
            *cache = Some(st.flow_snapshot_for_a11y());
        }
        cache.as_ref().cloned()
    };

    let scroll_y = st.scroll_y.get();
    let mut acc = WalkAcc::new(st);
    if let Some(snap) = snap {
        for elem in &snap.elements {
            if let FlowElementSnapshot::Block(block) = elem {
                emit_block(builder, &st.engine, block, scroll_y, &mut acc);
            }
        }
    }
    finish(st, builder, acc);
}

/// The log's walk: the runs of the *visible window* only. Fresh each walk (not
/// cached) — the window moves, and caching the whole document would defeat the
/// point.
pub(crate) fn build_log_a11y(st: &CodeEditorState, builder: &mut AccessNodeBuilder) {
    set_role(st, builder);

    let (_first, _total, snaps) = super::log_stream::a11y_window(st);
    let scroll_y = st.scroll_y.get();
    let mut acc = WalkAcc::new(st);
    for block in &snaps {
        emit_block(builder, &st.engine, block, scroll_y, &mut acc);
    }
    finish(st, builder, acc);
}

/// Apply the resolved selection, publish the synthetic map, advertise actions.
fn finish(st: &CodeEditorState, builder: &mut AccessNodeBuilder, acc: WalkAcc) {
    // Report the selection only when both ends resolved to an emitted run. When
    // they did not — an empty document, or (for the log) a caret outside the
    // shaped window — report no selection rather than a self-relative one: this
    // node carries no value of its own, so document character offsets addressed
    // against it would be meaningless.
    if let (Some(anchor), Some(caret)) = (acc.anchor_pair, acc.caret_pair) {
        builder.set_text_selection_to(anchor, caret);
    }
    *st.synthetic_to_element.borrow_mut() = acc.syn_map;

    builder.add_action(Action::Focus);
    builder.add_action(Action::ScrollIntoView);
    builder.add_action(Action::SetTextSelection);
    if matches!(st.policy.access_role, AccessibilityRole::Editor) {
        builder.add_action(Action::SetValue);
        builder.add_action(Action::ReplaceSelectedText);
    }
}

/// Emit one block's runs, and resolve the caret / anchor if they fall in it.
fn emit_block(
    builder: &mut AccessNodeBuilder,
    engine: &RichTextEngine,
    block: &BlockSnapshot,
    scroll_y: f32,
    acc: &mut WalkAcc,
) {
    // The separator between two blocks is a character of the document, and
    // AccessKit's line-navigation contract puts it at the end of the line's
    // last run; a block snapshot's own text stops short of it. Appending it
    // here also makes the source's character space identical to the document's
    // — `position + length` addresses the same character in both — so a caret
    // maps across with a subtraction and no correction.
    let mut text = block.text.clone();
    text.push('\n');

    let mut lines = engine.block_line_geometry(block.block_id, &block.text);
    if let Some(last) = lines.last_mut() {
        last.byte_range.end = text.len();
        last.char_range.end += 1;
        last.end = LineEnd::HardBreak { chars: 1, bytes: 1 };
    }

    // The layout reports line boxes from the block's own top edge, so the
    // block's place in the scrolled document is what turns them into the body's
    // coordinate space; `AccessNodeBuilder::build` takes them the rest of the
    // way once the walker has written the body's window-space box.
    let top = engine
        .block_visual_info(block.block_id)
        .map(|info| info.y - scroll_y)
        .unwrap_or(0.0);

    let source = if lines.is_empty() {
        // Nothing was shaped for this block — it sits outside the log's laid-out
        // window, or no layout has run yet. The text is still announced and
        // still reviewable, with a caret-shaped box rather than none at all:
        // one geometry-less run empties `bounding_boxes()` for every range that
        // touches it.
        let mut flat = TextRunSource::flat(&text, block.block_id as u64);
        flat.lines[0].end = LineEnd::HardBreak { chars: 1, bytes: 1 };
        flat.with_fallback_rect(Rect::new(0.0, top, 0.0, engine.default_line_height()))
    } else {
        let geometry = TextGeometry {
            lines,
            dropped_lines: 0,
            source_len: text.len(),
            rendered_text: None,
            links: Vec::new(),
        };
        TextRunSource::from_geometry(
            &text,
            &geometry,
            Point::new(0.0, top),
            block.block_id as u64,
        )
    };

    // A heading is the one block that keeps a node of its own: its level is
    // information no run carries. Every other block hangs its runs straight off
    // the body, where a text-change event can reach a parent that supports text
    // ranges.
    let parent = match block.block_format.heading_level {
        Some(level) if !builder.emits_no_children() => {
            let id = builder.push_paragraph_child(block.block_id as u64);
            builder.set_paragraph_as_heading(id, level);
            Some(id)
        }
        _ => None,
    };

    let emission = push_text_runs(builder, parent, &source);

    for run in &emission.runs {
        // Remember where this run lives so an AT-driven SetTextSelection
        // resolves to a document position. `text` here excludes the block
        // separator, so a caret clamped against it can never land on it.
        acc.syn_map.insert(
            run.id,
            SyntheticElementRef {
                element_id: block.block_id as u64,
                absolute_start: block.position + run.char_range.start,
                text: text.get(run.byte_range.clone()).unwrap_or("").to_string(),
            },
        );
    }

    // `position + length` is the end-of-line caret, which the emitter places on
    // the separator's slot — the AT-correct end-of-line focus.
    let end = block.position + block.length;
    if acc.user_pos >= block.position && acc.user_pos <= end {
        acc.caret_pair = emission.position_of(acc.user_pos - block.position);
    }
    if acc.user_anchor >= block.position && acc.user_anchor <= end {
        acc.anchor_pair = emission.position_of(acc.user_anchor - block.position);
    }
}

/// Resolve an AT-initiated action against the editor. Wired on both wrappers via
/// `on_access_action_request`.
pub(crate) fn handle_access_action(
    state: &SharedState,
    action: Action,
    _target: NodeId,
    data: Option<ActionData>,
    ctx: &mut EventContext,
) -> EventResponse {
    match (action, data) {
        (Action::SetTextSelection, Some(ActionData::SetTextSelection(sel))) => {
            let resolve = |pos: TextPosition| -> Option<usize> {
                let st = state.borrow();
                let map = st.synthetic_to_element.borrow();
                let er = map.get(&pos.node)?;
                // character_index is a char index into the run; positions are char
                // offsets, so add directly, clamped to the run's real characters
                // (never onto the synthetic newline).
                let char_count = er.text.chars().count();
                Some(er.absolute_start + pos.character_index.min(char_count))
            };
            match (resolve(sel.anchor), resolve(sel.focus)) {
                (Some(anchor), Some(focus)) => {
                    {
                        // Collapse to a single caret — an AT selection replaces
                        // the whole caret set, honouring the no-two-carets
                        // invariant the rest of the editor holds.
                        let mut st = state.borrow_mut();
                        st.clear_extra_carets();
                        st.cursor.set_position(anchor, MoveMode::MoveAnchor);
                        st.cursor.set_position(focus, MoveMode::KeepAnchor);
                    }
                    super::sync_cursor_signals(state);
                    ctx.request_frame();
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            }
        }
        (Action::SetValue, Some(ActionData::Value(value))) => {
            if state.borrow().policy.is_read_only() {
                return EventResponse::Ignored;
            }
            let _ = state.borrow().document.set_plain_text(&value);
            super::sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        (Action::ReplaceSelectedText, Some(ActionData::Value(value))) => {
            if state.borrow().policy.is_read_only() {
                return EventResponse::Ignored;
            }
            {
                let st = state.borrow();
                let _ = st.cursor.insert_text(&value);
            }
            super::sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        _ => EventResponse::Ignored,
    }
}
