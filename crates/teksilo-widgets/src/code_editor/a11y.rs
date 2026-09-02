// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The accessibility walk shared by the editor and log bodies.
//!
//! Both surfaces present their text to assistive technology the same way — a
//! `Role::Paragraph` per line, a `Role::TextRun` per formatting run under it,
//! each run carrying the per-character byte lengths, word starts, and geometry a
//! screen reader needs to speak and navigate character-by-character. The two
//! differ only in *which* lines they walk: the editor walks the whole (bounded)
//! document; the log walks only the visible window, because re-emitting a
//! paragraph per line of a 100 000-line buffer on every appended line would be
//! O(N) per line.
//!
//! Four things this does that a naive per-fragment emit does not:
//!
//! - **Links runs on a line.** Syntax highlighting splits one source line into
//!   several runs; without `next_on_line`/`previous_on_line` a screen reader
//!   navigating by line would stop at each colour boundary. The runs of a line
//!   are linked into one chain.
//! - **Ends each line with a newline.** AccessKit's contract puts the hard line
//!   break at the end of a line's last run (in the value and the length slices);
//!   the caret can never address it, but line navigation needs it.
//! - **Chunks runs over 255 characters.** `word_starts` are character indices
//!   stored as `u8`, so a run longer than 255 characters silently loses every
//!   word boundary past 255 — real for long log lines. Such a run is split into
//!   linked ≤255-character runs, each with its own valid word starts.
//! - **Numbers the lines.** Each paragraph carries `position_in_set`, and the
//!   body's own node carries the matching `size_of_set`. That is the split
//!   `set_child_position_in_set` writes, because AccessKit resolves a set size
//!   by walking up from the item; together they read "line 42 of 200".

use std::collections::HashMap;

use teksilo_core::accessibility::{AccessNodeBuilder, TextRunAttributes};
use teksilo_core::accesskit::{Action, ActionData, NodeId, Role, TextPosition};
use teksilo_core::event::EventResponse;
use teksilo_core::widget::EventContext;
use teksilo_text::RichTextEngine;
use teksilo_text::text_document::{
    BlockSnapshot, FlowElementSnapshot, FragmentContent, MoveMode, TextFormat,
};

use super::state::{CodeEditorState, SharedState, SyntheticElementRef};
use crate::common::editor_runtime::AccessibilityRole;

/// The most characters one `Role::TextRun` may hold. `word_starts` are character
/// indices stored as `u8`, so a word starting past 255 is inexpressible; split
/// longer runs at this boundary.
const MAX_RUN_CHARS: usize = 255;

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

/// The editor's walk: a paragraph + runs per block of the whole document. The
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

    let mut acc = WalkAcc::new(st);
    if let Some(snap) = snap {
        let total = snap
            .elements
            .iter()
            .filter(|e| matches!(e, FlowElementSnapshot::Block(_)))
            .count();
        let mut line = 0usize;
        for elem in &snap.elements {
            if let FlowElementSnapshot::Block(block) = elem {
                emit_block(builder, &st.engine, block, line, total, &mut acc);
                line += 1;
            }
        }
    }
    finish(st, builder, acc);
}

/// The log's walk: a paragraph + runs per line of the *visible window* only,
/// numbered by global line so a screen reader still hears "line 41 002 of
/// 128 449". Fresh each walk (not cached) — the window moves, and caching the
/// whole document would defeat the point.
pub(crate) fn build_log_a11y(st: &CodeEditorState, builder: &mut AccessNodeBuilder) {
    set_role(st, builder);

    let (first, total, snaps) = super::log_stream::a11y_window(st);
    let mut acc = WalkAcc::new(st);
    for (i, block) in snaps.iter().enumerate() {
        emit_block(builder, &st.engine, block, first + i, total, &mut acc);
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

/// Emit one block as a `Role::Paragraph` with its `Role::TextRun` children.
fn emit_block(
    builder: &mut AccessNodeBuilder,
    engine: &RichTextEngine,
    block: &BlockSnapshot,
    line_index: usize,
    total_lines: usize,
    acc: &mut WalkAcc,
) {
    let para_id = builder.push_paragraph_child(block.block_id as u64);
    if let Some(level) = block.block_format.heading_level {
        builder.set_paragraph_as_heading(para_id, level);
    }
    builder.set_child_position_in_set(para_id, line_index + 1, total_lines.max(1));

    // Split the block's text fragments into ≤255-char run chunks. Each carries
    // its block-relative char start (for geometry + the disambiguating NodeId).
    struct Chunk<'a> {
        text: &'a str,
        char_start: usize,
        char_count: usize,
        element_id: u64,
        format: &'a TextFormat,
        // The fragment's own (UAX-29) word starts, reused verbatim when the whole
        // fragment fits one run; `None` forces a recompute for a chunk.
        own_word_starts: Option<&'a Vec<u8>>,
        // Whether the character before this chunk was whitespace — so a chunk
        // split mid-word does not report a spurious word start at index 0.
        prev_ws: bool,
    }
    let mut chunks: Vec<Chunk> = Vec::new();
    for frag in &block.fragments {
        if let FragmentContent::Text {
            text,
            offset,
            length,
            element_id,
            word_starts,
            format,
        } = frag
        {
            if *length == 0 {
                continue;
            }
            let char_byte: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
            let nchars = char_byte.len();
            let whole_fits = nchars <= MAX_RUN_CHARS;
            let mut cs = 0usize;
            let mut prev_ws = true; // the run start counts as a word boundary
            while cs < nchars {
                let ce = (cs + MAX_RUN_CHARS).min(nchars);
                let bs = char_byte[cs];
                let be = char_byte.get(ce).copied().unwrap_or(text.len());
                let chunk_text = &text[bs..be];
                chunks.push(Chunk {
                    text: chunk_text,
                    char_start: *offset + cs,
                    char_count: ce - cs,
                    element_id: *element_id,
                    format,
                    own_word_starts: whole_fits.then_some(word_starts),
                    prev_ws,
                });
                prev_ws = chunk_text.chars().last().is_some_and(|c| c.is_whitespace());
                cs = ce;
            }
        }
    }

    let mut run_ids: Vec<NodeId> = Vec::with_capacity(chunks.len().max(1));
    let default_format = TextFormat::default();
    if chunks.is_empty() {
        // An empty line still needs a run carrying the line break, so line
        // navigation has a node and a boundary.
        run_ids.push(emit_run(
            builder,
            engine,
            para_id,
            block,
            0,
            "",
            0,
            block.block_id as u64,
            &default_format,
            None,
            true,
            true,
            acc,
        ));
    } else {
        let n = chunks.len();
        for (i, ch) in chunks.iter().enumerate() {
            run_ids.push(emit_run(
                builder,
                engine,
                para_id,
                block,
                ch.char_start,
                ch.text,
                ch.char_count,
                ch.element_id,
                ch.format,
                ch.own_word_starts,
                ch.prev_ws,
                i + 1 == n,
                acc,
            ));
        }
    }
    // Link the runs into one line so AT line navigation is continuous. A block is
    // one line here — for a wrapped block (a `PlainTextEditor`), the runs still
    // read as one AT line, and their per-character geometry is per-visual-line, a
    // limitation shared with the rich text editor.
    builder.link_runs_on_line(&run_ids);
}

/// Emit one `Role::TextRun`, appending the line-break to the line's last run,
/// and resolve the caret / anchor if they fall inside it.
#[allow(clippy::too_many_arguments)]
fn emit_run(
    builder: &mut AccessNodeBuilder,
    engine: &RichTextEngine,
    para_id: NodeId,
    block: &BlockSnapshot,
    char_start: usize,
    text: &str,
    char_count: usize,
    element_id: u64,
    format: &TextFormat,
    own_word_starts: Option<&Vec<u8>>,
    prev_ws: bool,
    is_last: bool,
    acc: &mut WalkAcc,
) -> NodeId {
    let mut value = text.to_string();
    let mut char_lengths: Vec<u8> = text.chars().map(|c| c.len_utf8() as u8).collect();
    let word_starts: Vec<u8> = match own_word_starts {
        Some(ws) => ws.clone(),
        None => word_starts_for(text, prev_ws),
    };

    let geom = engine.character_geometry(block.block_id, char_start, char_start + char_count);
    let mut char_positions: Vec<f32> = geom.iter().map(|g| g.position).collect();
    let mut char_widths: Vec<f32> = geom.iter().map(|g| g.width).collect();

    if is_last {
        // AccessKit line-break contract: the last run of a line ends with the
        // newline, counted as one character. The caret can never address it.
        value.push('\n');
        char_lengths.push(1);
        if !char_positions.is_empty() {
            let end = char_positions.last().copied().unwrap_or(0.0)
                + char_widths.last().copied().unwrap_or(0.0);
            char_positions.push(end);
            char_widths.push(0.0);
        }
    }

    let attrs = TextRunAttributes {
        font_weight: format.font_weight.map(|w| w as u16),
        bold: format.font_bold.unwrap_or(false),
        italic: format.font_italic.unwrap_or(false),
        underline: format.font_underline.unwrap_or(false),
        strikethrough: format.font_strikeout.unwrap_or(false),
    };

    // AccessKit's contract requires the geometry slices, when present, to have
    // exactly one entry per character (the same length as `character_lengths`).
    // Pass them only when they line up — an unlaid-out block yields none, and a
    // partial measurement must not be handed over as if it were complete.
    let n = char_lengths.len();
    let positions = (char_positions.len() == n).then_some(char_positions);
    let widths = (char_widths.len() == n).then_some(char_widths);
    let node_id = builder.push_text_run_child(
        para_id,
        element_id,
        char_start,
        value,
        char_lengths,
        Some(word_starts),
        positions,
        widths,
        attrs,
    );

    // Remember where this run lives so an AT-driven SetTextSelection resolves to
    // a document position. `text` here excludes the synthetic newline.
    let absolute_start = block.position + char_start;
    acc.syn_map.insert(
        node_id,
        SyntheticElementRef {
            element_id,
            absolute_start,
            text: text.to_string(),
        },
    );

    // Positions are character offsets, so the AT character index within this run
    // is simply the document offset minus the run's start. A caret at the very
    // end lands on the newline slot (index == char_count), which is the AT-correct
    // end-of-line focus.
    let run_end = absolute_start + char_count;
    if acc.user_pos >= absolute_start && acc.user_pos <= run_end {
        acc.caret_pair = Some((node_id, acc.user_pos - absolute_start));
    }
    if acc.user_anchor >= absolute_start && acc.user_anchor <= run_end {
        acc.anchor_pair = Some((node_id, acc.user_anchor - absolute_start));
    }
    node_id
}

/// Word starts (character indices) for a run, for the chunked path where the
/// fragment's own UAX-29 list would be truncated at 255. A word starts at each
/// non-whitespace character following whitespace. `prev_ws` is whether the
/// character *before* this chunk was whitespace, so a chunk that splits a word
/// mid-way does not report a spurious word start at its index 0. Every index is
/// < the chunk length (≤ 255), so it always fits `u8`.
fn word_starts_for(text: &str, prev_ws: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev_ws = prev_ws;
    for (ci, ch) in text.chars().enumerate() {
        let ws = ch.is_whitespace();
        if !ws && prev_ws {
            match u8::try_from(ci) {
                Ok(idx) => out.push(idx),
                Err(_) => break,
            }
        }
        prev_ws = ws;
    }
    out
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
