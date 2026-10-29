// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Caret motion that needs the layout: Up, Down, Page Up, Page Down and End.
//!
//! text-document moves Up and Down by whole blocks, with no idea where a
//! caret is drawn, and finds the end of a line from the block at the caret's
//! position, which at a line's end is the next line. Both are right for
//! nothing an editor shows, so these motions ask the engine instead: where
//! the caret is drawn, which visual line is next, where a point lands.
//!
//! **What stays put through a run of Up and Down is the caret's x**, not its
//! column. In a monospace face they are the same; with a tab, a wide
//! character or a soft wrap the x is what the eye follows. Every caret keeps
//! its own, in [`CodeEditorState::sticky_x`], keyed by the position its caret
//! was left at.

use teksilo_canvas::Point;
use teksilo_text::CursorAffinity;
use teksilo_text::text_document::{MoveMode, MoveOperation, TextCursor};

use super::state::CodeEditorState;
use crate::rich_text::neighbour_line_center_y;

fn caret_of(st: &mut CodeEditorState, i: usize) -> &mut TextCursor {
    if i == 0 {
        &mut st.cursor
    } else {
        &mut st.extra_carets[i - 1]
    }
}

/// The affinity to read caret `i` with. Only the primary has one.
fn affinity_of(st: &CodeEditorState, i: usize) -> CursorAffinity {
    if i == 0 {
        st.cursor_affinity
    } else {
        CursorAffinity::Downstream
    }
}

/// Move every caret one visual line, or one page, up (`direction < 0`) or
/// down, keeping each caret's x.
pub(super) fn move_vertically(
    st: &mut CodeEditorState,
    direction: i32,
    page: bool,
    mode: MoveMode,
) {
    if !st.engine.has_full_layout() {
        // No layout yet: blocks stand in for lines.
        let op = if direction < 0 {
            MoveOperation::Up
        } else {
            MoveOperation::Down
        };
        let count = if page { page_lines(st) } else { 1 };
        st.cursor.move_position(op, mode, count);
        for c in &mut st.extra_carets {
            c.move_position(op, mode, count);
        }
        st.merge_collided_carets();
        return;
    }
    // A run starts where nothing is remembered.
    if st.preferred_x.is_none() {
        st.sticky_x.clear();
    }

    let count = 1 + st.extra_carets.len();
    let mut plan = Vec::with_capacity(count);
    for i in 0..count {
        let caret = if i == 0 {
            &st.cursor
        } else {
            &st.extra_carets[i - 1]
        };
        let at = caret.position();
        let shown = st.shown_selection(caret).1;
        let affinity = affinity_of(st, i);
        let rect = st.engine.caret_rect(shown, affinity);
        let x = st
            .sticky_x
            .iter()
            .find(|(p, _)| *p == at)
            .map_or(rect[0], |(_, x)| *x);
        let hit = vertical_target(st, shown, affinity, rect, x, direction, page);
        plan.push((x, hit));
    }

    let mut kept = Vec::with_capacity(count);
    for (i, (x, hit)) in plan.into_iter().enumerate() {
        if let Some(hit) = hit {
            caret_of(st, i).set_position(hit.position, mode);
            if i == 0 {
                st.cursor_affinity = hit.affinity;
            }
        }
        if i == 0 {
            st.preferred_x = Some(x);
        }
        kept.push((caret_of(st, i).position(), x));
    }
    st.sticky_x = kept;
    st.merge_collided_carets();
}

/// Where a caret drawn at `rect` (position `shown` in the layout) goes when
/// moved a line or a page towards `direction`, holding x.
fn vertical_target(
    st: &CodeEditorState,
    shown: usize,
    affinity: CursorAffinity,
    rect: [f32; 4],
    x: f32,
    direction: i32,
    page: bool,
) -> Option<teksilo_text::HitTestResult> {
    // `caret_rect` and `hit_test` speak screen space, so the text's own band
    // is measured there too.
    let (top, bottom) = st.engine.content_band_screen();
    let target_y = if page {
        let line = rect[3].max(16.0);
        let step = (st.viewport_height - line).max(line);
        (rect[1] + rect[3] * 0.5 + direction as f32 * step).clamp(top, bottom)
    } else {
        let y = neighbour_line_center_y(&st.engine, affinity, shown, direction, rect)?;
        if y < top || y > bottom {
            return None;
        }
        y
    };
    st.hit_test(Point::new(x, target_y))
}

/// A page's worth of lines, one fewer than the viewport shows.
fn page_lines(st: &CodeEditorState) -> usize {
    let line_h = st.engine.default_line_height().max(1.0);
    ((st.viewport_height / line_h).floor() as usize)
        .saturating_sub(1)
        .max(1)
}

/// End: every caret to the end of its visual line. Pressed there, it stays.
pub(super) fn move_to_line_end(st: &mut CodeEditorState, mode: MoveMode) {
    if !st.engine.has_full_layout() {
        st.cursor.move_position(MoveOperation::EndOfLine, mode, 1);
        for c in &mut st.extra_carets {
            c.move_position(MoveOperation::EndOfLine, mode, 1);
        }
        st.merge_collided_carets();
        return;
    }
    let count = 1 + st.extra_carets.len();
    let mut targets = Vec::with_capacity(count);
    for i in 0..count {
        let caret = if i == 0 {
            &st.cursor
        } else {
            &st.extra_carets[i - 1]
        };
        let shown = st.shown_selection(caret).1;
        let end = st
            .engine
            .visual_line_range_at(shown, affinity_of(st, i))
            .map(|(_, end)| st.document_position(end));
        targets.push(end);
    }
    for (i, end) in targets.into_iter().enumerate() {
        if let Some(end) = end {
            caret_of(st, i).set_position(end, mode);
        }
    }
    // At a soft wrap the end of a line is also the start of the next; the
    // caret belongs to the line it just went to the end of.
    st.cursor_affinity = CursorAffinity::Downstream;
    st.merge_collided_carets();
}
