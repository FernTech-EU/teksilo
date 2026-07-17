// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The code-flavoured editing operations.
//!
//! Everything here reads [`CodeConfig`](super::CodeConfig), which is why it is
//! not in `keyboard.rs`: that module must stay correct for *any* configuration,
//! including none, so the part that consults the injected indent width, comment
//! token, and bracket pairs is quarantined here. The keyboard layer decides
//! *which* operation a chord means; this layer decides what the operation does
//! given the application's language.
//!
//! Nothing here names a language. `toggle_line_comment` knows how to put a token
//! in front of a run of lines; it does not know that the token is `//`. The
//! whole point of the separation is that adding a language is a value in the
//! caller's `CodeConfig`, never a change to this file.
//!
//! # One block is one line
//!
//! The document is a list of blocks and this editor renders one block per line
//! with no wrapping, so "the line at `pos`" is "the block at `pos`" — obtained
//! from [`snapshot_block_at_position_without_highlights`], whose `position` is
//! the line's first character and whose `text` is the line's content with no
//! trailing separator. Between two blocks sits exactly one separator position
//! (verified: for `"Hello\nWorld"` the second block starts at 6, not 5), so the
//! next line begins at `block.position + block.length + 1`.
//!
//! [`snapshot_block_at_position_without_highlights`]:
//!     bastyde_text::text_document::TextDocument::snapshot_block_at_position_without_highlights
//!
//! # Multi-caret
//!
//! Line operations run over the *union* of the lines every caret touches, and
//! character operations run at every caret. Both apply their document mutations
//! back-to-front so an edit never invalidates a caret not yet handled, and both
//! finish by merging carets that collided — the same invariant the rest of the
//! editor holds. Two operations are single-caret by decision, not omission:
//! moving lines and pair-backspace are ambiguous or not worth the complexity
//! with disjoint carets, so they collapse to the primary and say so.

use bastyde_text::text_document::{MoveMode, MoveOperation};

use super::config::CodeConfig;
use super::state::CodeEditorState;

// ─────────────────────────────────────────────────────────────────────────
// Line model
// ─────────────────────────────────────────────────────────────────────────

/// One line's geometry and content, as the semantics need it.
#[derive(Debug, Clone)]
struct LineInfo {
    /// Absolute position of the line's first character.
    start: usize,
    /// Character length of the line's content (no trailing separator).
    len: usize,
    /// The line's text.
    text: String,
}

impl LineInfo {
    /// Absolute position just past the last character — where the caret sits at
    /// end-of-line, and the separator to the next line begins.
    fn end(&self) -> usize {
        self.start + self.len
    }
}

/// The line containing `pos`, or `None` past the end of the document.
fn line_at(st: &CodeEditorState, pos: usize) -> Option<LineInfo> {
    let block = st
        .document
        .snapshot_block_at_position_without_highlights(pos)?;
    Some(LineInfo {
        start: block.position,
        len: block.length,
        text: block.text,
    })
}

/// The line after `line`, or `None` when `line` is the last.
///
/// A distinct helper because `line_at` **clamps** a position past the end of the
/// document to the last block rather than returning `None`. The next line
/// genuinely exists only when the block at `line.end() + 1` starts exactly
/// there; a clamped read comes back with an earlier start, which is how "there
/// is nothing below" is told apart from a real next line.
fn next_line(st: &CodeEditorState, line: &LineInfo) -> Option<LineInfo> {
    let next_start = line.end() + 1;
    let next = line_at(st, next_start)?;
    (next.start == next_start).then_some(next)
}

/// Leading run of spaces and tabs, as a string. Only ASCII indentation counts —
/// a code editor indents with spaces and tabs, and treating a stray non-break
/// space as indentation would carry an invisible character onto the next line.
fn leading_whitespace(line: &str) -> String {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

/// The character at `col` within `line`, or `None` at or past its end. Column is
/// a character index; positions in this document are character indices, so for a
/// caret at `pos` on a line starting at `start`, its column is `pos - start`.
fn char_in_line(line: &LineInfo, col: usize) -> Option<char> {
    if col >= line.len {
        return None;
    }
    line.text.chars().nth(col)
}

/// The character immediately after a caret at `pos`, staying **within the line**:
/// `None` at end-of-line even when another line follows. Type-over must not step
/// a closer that lives on the next line, and pair-backspace must not treat a
/// bracket across the separator as adjacent.
fn char_after(st: &CodeEditorState, pos: usize) -> Option<char> {
    let line = line_at(st, pos)?;
    char_in_line(&line, pos - line.start)
}

/// The character immediately before a caret at `pos`, staying within the line:
/// `None` at the start of the line.
fn char_before(st: &CodeEditorState, pos: usize) -> Option<char> {
    let line = line_at(st, pos)?;
    let col = pos - line.start;
    if col == 0 {
        return None;
    }
    line.text.chars().nth(col - 1)
}

// ─────────────────────────────────────────────────────────────────────────
// Caret indexing (0 = primary)
// ─────────────────────────────────────────────────────────────────────────

fn caret_pos(st: &CodeEditorState, i: usize) -> usize {
    if i == 0 {
        st.cursor.position()
    } else {
        st.extra_carets[i - 1].position()
    }
}

fn caret_span(st: &CodeEditorState, i: usize) -> (usize, usize) {
    let c = if i == 0 {
        &st.cursor
    } else {
        &st.extra_carets[i - 1]
    };
    (c.selection_start(), c.selection_end())
}

fn caret_at(st: &mut CodeEditorState, i: usize) -> &mut bastyde_text::text_document::TextCursor {
    if i == 0 {
        &mut st.cursor
    } else {
        &mut st.extra_carets[i - 1]
    }
}

/// Indices of every caret sorted by descending position, so mutating them in
/// order never shifts a caret still to be handled.
fn carets_back_to_front(st: &CodeEditorState) -> Vec<usize> {
    let mut order: Vec<usize> = (0..=st.extra_carets.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(caret_pos(st, i)));
    order
}

/// Indices of every caret sorted by descending *trailing edge* (selection end),
/// so an operation that edits at or beyond a caret's selection cannot shift a
/// caret still to be handled. The right order for anything that may act on a
/// selection (surround, duplicate, type-over-a-selection); plain
/// [`carets_back_to_front`] orders by the head, which is enough only when the
/// edit is at the caret point.
fn carets_by_trailing_edge(st: &CodeEditorState) -> Vec<usize> {
    let mut order: Vec<usize> = (0..=st.extra_carets.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(caret_span(st, i).1));
    order
}

// ─────────────────────────────────────────────────────────────────────────
// Line-set operations: indent, dedent, comment
// ─────────────────────────────────────────────────────────────────────────

/// Every line any caret touches, de-duplicated and sorted, so an operation over
/// "the selected lines" is well defined for one caret or many.
///
/// A selection that ends exactly at the first column of a line does not touch
/// that line — you selected up to it, not into it — which is why the range walk
/// stops when the next line begins at or after the selection end. Without that
/// rule, selecting three lines by dragging to the start of the fourth would
/// indent four.
fn touched_lines(st: &CodeEditorState) -> Vec<LineInfo> {
    let mut lines: Vec<LineInfo> = Vec::new();
    let mut seen: Vec<usize> = Vec::new();

    for i in 0..=st.extra_carets.len() {
        let (a, b) = caret_span(st, i);
        let Some(first) = line_at(st, a) else {
            continue;
        };
        let mut cur = first;
        loop {
            if !seen.contains(&cur.start) {
                seen.push(cur.start);
                lines.push(cur.clone());
            }
            let next_start = cur.end() + 1;
            // Continue only when the selection reaches strictly past the next
            // line's first column; a selection ending exactly there leaves the
            // next line untouched.
            if b > next_start
                && let Some(next) = line_at(st, next_start)
            {
                cur = next;
            } else {
                break;
            }
        }
    }

    lines.sort_by_key(|l| l.start);
    lines
}

// Line-set operations edit through a scratch cursor at each line's front and
// let the document's own cursor bookkeeping carry every live caret with the
// text. `TextDocument` tracks all its cursors and, on each edit, shifts a caret
// after the edit point right by the inserted count (or left by the removed
// count, clamping a caret inside a removed region to its start) — a position
// exactly at the edit point stays put, so a selection anchored at column 0 keeps
// the new indent inside it. That is precisely the fixup a hand-rolled remap
// would compute, so there is none: the carets are already correct once the edits
// land, and only a collision merge remains. Editing through a scratch cursor
// rather than the carets themselves is what keeps a mid-line caret at its column
// instead of dragging it to the indent it inserted.

/// Indent every touched line by one level, or — with no selection — insert to
/// the next tab stop at each caret.
///
/// The split is the near-universal Tab behaviour: with something selected, Tab
/// shifts the block right; with just a caret, Tab is an insert. Soft tabs insert
/// the spaces that reach the next multiple of the indent width from the caret's
/// column, so Tab lands on the grid even from a ragged column; hard tabs insert
/// one tab character.
pub(super) fn indent_or_tab(st: &mut CodeEditorState) {
    let any_selection = st.all_carets().any(|c| c.has_selection());
    if any_selection {
        indent_lines(st);
    } else {
        insert_tab_at_carets(st);
    }
}

/// Insert to the next tab stop at every caret (no selection).
fn insert_tab_at_carets(st: &mut CodeEditorState) {
    let config = st.config.clone();
    let multi = !st.extra_carets.is_empty();
    if multi {
        st.cursor.begin_edit_block();
    }
    for i in carets_back_to_front(st) {
        let pos = caret_pos(st, i);
        let col = line_at(st, pos).map(|l| pos - l.start).unwrap_or(0);
        let text = tab_insertion(&config, col);
        let _ = caret_at(st, i).insert_text(&text);
    }
    if multi {
        st.cursor.end_edit_block();
    }
    st.merge_collided_carets();
}

/// The text a Tab inserts at `col`: for spaces, enough to reach the next
/// multiple of the width; for tabs, one tab character.
fn tab_insertion(config: &CodeConfig, col: usize) -> String {
    match config.indent {
        super::config::IndentStyle::Spaces(n) => {
            let n = n.max(1) as usize;
            let to_next = n - (col % n);
            " ".repeat(to_next)
        }
        super::config::IndentStyle::Tabs { .. } => "\t".to_string(),
    }
}

/// Insert one indent unit at the front of every touched line.
fn indent_lines(st: &mut CodeEditorState) {
    let unit = st.config.indent.unit();
    let lines = touched_lines(st);
    if lines.is_empty() {
        return;
    }

    let ed = st.document.cursor();
    let grouped = lines.len() > 1;
    if grouped {
        ed.begin_edit_block();
    }
    // Back-to-front so an earlier insertion cannot shift a line not yet edited.
    for line in lines.iter().rev() {
        ed.set_position(line.start, MoveMode::MoveAnchor);
        let _ = ed.insert_text(&unit);
    }
    if grouped {
        ed.end_edit_block();
    }
    st.merge_collided_carets();
}

/// Remove one indent level from every touched line (Shift+Tab), or from the
/// caret's line when there is no selection.
///
/// A level is at most `width` columns and never crosses into the line's content:
/// the removal stops at the first non-whitespace character, so dedenting an
/// under-indented line strips what indentation it has rather than eating code.
/// Tabs are removed one character per level; spaces are removed up to the width,
/// stopping early on a tab so a mixed-indent line loses one step, not a jumble.
pub(super) fn dedent(st: &mut CodeEditorState) {
    let width = st.config.indent.width().max(1) as usize;
    let lines = touched_lines(st);
    if lines.is_empty() {
        return;
    }

    let ed = st.document.cursor();
    let grouped = lines.len() > 1;
    if grouped {
        ed.begin_edit_block();
    }
    for line in lines.iter().rev() {
        let remove = dedent_count(&line.text, width);
        if remove == 0 {
            continue;
        }
        ed.set_position(line.start, MoveMode::MoveAnchor);
        ed.set_position(line.start + remove, MoveMode::KeepAnchor);
        let _ = ed.remove_selected_text();
    }
    if grouped {
        ed.end_edit_block();
    }
    st.merge_collided_carets();
}

/// How many leading characters one dedent removes: up to `width` spaces, or a
/// single leading tab, whichever comes first. Zero for a line with no leading
/// whitespace.
fn dedent_count(line: &str, width: usize) -> usize {
    let mut removed = 0;
    for c in line.chars() {
        if removed >= width {
            break;
        }
        match c {
            '\t' => {
                // A tab is a whole level; if we have already taken some spaces,
                // stop before it so we remove exactly one step.
                if removed == 0 {
                    removed = 1;
                }
                break;
            }
            ' ' => removed += 1,
            _ => break,
        }
    }
    removed
}

/// Comment or uncomment every touched line with the configured token.
///
/// A no-op when no token is configured — the editor knows the operation, the
/// application supplies the language, and guessing `//` would corrupt a file
/// whose comment is `#`. The toggle direction is decided by the block: if every
/// non-blank touched line is already commented, the whole block uncomments;
/// otherwise it comments. Tokens are inserted at the shallowest common
/// indentation so a commented block stays visually aligned, matching what a
/// reader expects from selecting a nested region and pressing the key once.
pub(super) fn toggle_line_comment(st: &mut CodeEditorState) {
    let Some(token) = st.config.line_comment.clone() else {
        return;
    };
    if token.is_empty() {
        return;
    }
    let token_len = token.chars().count();
    let lines = touched_lines(st);
    if lines.is_empty() {
        return;
    }

    let non_blank: Vec<&LineInfo> = lines.iter().filter(|l| !l.text.trim().is_empty()).collect();
    if non_blank.is_empty() {
        return;
    }

    let all_commented = non_blank
        .iter()
        .all(|l| l.text.trim_start().starts_with(&token));

    let ed = st.document.cursor();
    let grouped = non_blank.len() > 1;
    if grouped {
        ed.begin_edit_block();
    }

    if all_commented {
        // Uncomment: strip the token, and the single space we would have added,
        // at each line's first non-whitespace column.
        for line in non_blank.iter().rev() {
            let indent = leading_whitespace(&line.text).chars().count();
            let after_token: String = line.text.chars().skip(indent + token_len).collect();
            let space = usize::from(after_token.starts_with(' '));
            let remove = token_len + space;
            let at = line.start + indent;
            ed.set_position(at, MoveMode::MoveAnchor);
            ed.set_position(at + remove, MoveMode::KeepAnchor);
            let _ = ed.remove_selected_text();
        }
    } else {
        // Comment: insert `token ` at the shallowest common indentation.
        let min_indent = non_blank
            .iter()
            .map(|l| leading_whitespace(&l.text).chars().count())
            .min()
            .unwrap_or(0);
        let insertion = format!("{token} ");
        for line in non_blank.iter().rev() {
            let at = line.start + min_indent;
            ed.set_position(at, MoveMode::MoveAnchor);
            let _ = ed.insert_text(&insertion);
        }
    }

    if grouped {
        ed.end_edit_block();
    }
    st.merge_collided_carets();
}

// ─────────────────────────────────────────────────────────────────────────
// Newline with indent + bracket expansion
// ─────────────────────────────────────────────────────────────────────────

/// Break the line at every caret, carrying indentation and opening a bracket
/// block where the caret sits between a pair.
///
/// Two behaviours, both language-supplied:
///
/// - **Auto-indent** (`config.auto_indent`) carries the current line's leading
///   whitespace onto the new line, so the caret lands under the code it came
///   from instead of at column 0.
/// - **Bracket expansion** (a configured pair with the caret between its open
///   and close, e.g. `{|}`) splits into three lines — the opener's line, an
///   indented empty middle line with the caret, and the closer on its own line
///   at the original indentation. The shape a reader means by pressing Enter
///   inside a fresh block.
///
/// With neither in play this is a plain break, which is why the keyboard layer
/// can route every Enter through here regardless of configuration.
pub(super) fn newline(st: &mut CodeEditorState) {
    let config = st.config.clone();
    // Always one undo step: even a single caret's break can be two mutations
    // (break + carried indent) or four (bracket expansion), and "undo my Enter"
    // must reverse all of them at once. Grouping a lone mutation is harmless.
    st.cursor.begin_edit_block();
    for i in carets_back_to_front(st) {
        newline_one(st, i, &config);
    }
    st.cursor.end_edit_block();
    st.merge_collided_carets();
}

fn newline_one(st: &mut CodeEditorState, i: usize, config: &CodeConfig) {
    let pos = caret_pos(st, i);
    let (sel_start, sel_end) = caret_span(st, i);
    let has_sel = sel_start != sel_end;

    // The break lands at the selection start (insert_block deletes any selection
    // then splits there), so the indentation to carry is that surviving line's —
    // not the caret head's, which for a backward selection is a different line.
    let indent = if config.auto_indent {
        line_at(st, sel_start)
            .map(|l| leading_whitespace(&l.text))
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Bracket expansion only with a bare caret between a configured pair; a
    // selection means the user is replacing text, not opening a block.
    let expand = !has_sel
        && match (char_before(st, pos), char_after(st, pos)) {
            (Some(open), Some(close)) => config.closing_for(open) == Some(close),
            _ => false,
        };

    let c = caret_at(st, i);
    if expand {
        let unit = config.indent.unit();
        // {  ->  {
        //          <caret>
        //        }
        let _ = c.insert_block();
        let _ = c.insert_text(&format!("{indent}{unit}"));
        // Remember where the caret should end (the indented middle line), then
        // push the closer down onto its own line and re-indent it.
        let middle = c.position();
        let _ = c.insert_block();
        let _ = c.insert_text(&indent);
        c.set_position(middle, MoveMode::MoveAnchor);
    } else {
        let _ = c.insert_block();
        if !indent.is_empty() {
            let _ = c.insert_text(&indent);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Duplicate
// ─────────────────────────────────────────────────────────────────────────

/// Duplicate the selection inline, or — with no selection — the whole line
/// below.
///
/// The two are the same intent at two granularities: with text selected the
/// copy lands right after it and stays selected, so pressing the key again keeps
/// duplicating; with a bare caret the line is copied onto a new line below and
/// the caret follows the copy at the same column, so a held key walks copies
/// downward. Multi-caret duplicates each caret's line or selection.
pub(super) fn duplicate(st: &mut CodeEditorState) {
    // Always one undo step: a single caret's duplicate is two mutations (break +
    // copied text), which must undo together. Order by the trailing edge so a
    // lower caret's positions survive a higher caret's insertion.
    st.cursor.begin_edit_block();
    for i in carets_by_trailing_edge(st) {
        duplicate_one(st, i);
    }
    st.cursor.end_edit_block();
    st.merge_collided_carets();
}

fn duplicate_one(st: &mut CodeEditorState, i: usize) {
    let has_sel = caret_pos(st, i) != {
        let c = if i == 0 {
            &st.cursor
        } else {
            &st.extra_carets[i - 1]
        };
        c.anchor()
    };

    if has_sel {
        let (s, e) = caret_span(st, i);
        let text = {
            let c = if i == 0 {
                &st.cursor
            } else {
                &st.extra_carets[i - 1]
            };
            c.selected_text().unwrap_or_default()
        };
        let len = text.chars().count();
        let c = caret_at(st, i);
        c.set_position(e, MoveMode::MoveAnchor);
        let _ = c.insert_text(&text);
        // Select the copy so a repeat keeps going.
        c.set_position(s + len, MoveMode::MoveAnchor);
        c.set_position(e + len, MoveMode::KeepAnchor);
    } else {
        let pos = caret_pos(st, i);
        let Some(line) = line_at(st, pos) else { return };
        let col = pos - line.start;
        let text = line.text.clone();
        let c = caret_at(st, i);
        c.set_position(line.end(), MoveMode::MoveAnchor);
        let _ = c.insert_block();
        let _ = c.insert_text(&text);
        // Land on the copy at the same column: the copy starts one past the
        // original line's end (the separator).
        c.set_position(line.end() + 1 + col, MoveMode::MoveAnchor);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Move lines
// ─────────────────────────────────────────────────────────────────────────

/// Whether to move the caret's line(s) up or down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MoveDir {
    Up,
    Down,
}

/// Move the caret's line, or the selected span of lines, past its neighbour.
///
/// Single-caret by decision: with disjoint carets the operation is ambiguous —
/// two carets on adjacent lines both moving up would collide — and resolving
/// that is complexity a source editor does not owe. Extra carets collapse to the
/// primary first. The selection rides with the lines, shifted by the neighbour's
/// size, so a held Alt+Up walks the same block upward.
pub(super) fn move_lines(st: &mut CodeEditorState, dir: MoveDir) {
    st.clear_extra_carets();
    let a = st.cursor.selection_start();
    let b = st.cursor.selection_end();
    let Some(first) = line_at(st, a) else { return };
    // The last touched line: walk from `first` until the span ends.
    let last = last_touched_line(st, &first, b);

    match dir {
        MoveDir::Up => {
            if first.start == 0 {
                return; // nothing above
            }
            // The separator before the block sits at first.start - 1, inside the
            // previous line.
            let Some(prev) = line_at(st, first.start - 1) else {
                return;
            };
            let shift = prev.len + 1;
            let block_end = last.end();
            let ed = st.document.cursor();
            ed.begin_edit_block();
            // Delete the previous line and its separator.
            ed.set_position(prev.start, MoveMode::MoveAnchor);
            ed.set_position(first.start, MoveMode::KeepAnchor);
            let _ = ed.remove_selected_text();
            // Re-insert it just past the block, which has shifted up by `shift`.
            ed.set_position(block_end - shift, MoveMode::MoveAnchor);
            let _ = ed.insert_block();
            let _ = ed.insert_text(&prev.text);
            ed.end_edit_block();
            st.cursor.set_position(a - shift, MoveMode::MoveAnchor);
            st.cursor.set_position(b - shift, MoveMode::KeepAnchor);
        }
        MoveDir::Down => {
            let Some(next) = next_line(st, &last) else {
                return; // nothing below
            };
            let shift = next.len + 1;
            let ed = st.document.cursor();
            ed.begin_edit_block();
            // Delete the next line and the separator before it.
            ed.set_position(last.end(), MoveMode::MoveAnchor);
            ed.set_position(next.end(), MoveMode::KeepAnchor);
            let _ = ed.remove_selected_text();
            // Re-insert it before the block, pushing the block down by `shift`.
            ed.set_position(first.start, MoveMode::MoveAnchor);
            let _ = ed.insert_text(&next.text);
            let _ = ed.insert_block();
            ed.end_edit_block();
            st.cursor.set_position(a + shift, MoveMode::MoveAnchor);
            st.cursor.set_position(b + shift, MoveMode::KeepAnchor);
        }
    }
}

/// The last line of the span `[first.start, b]`, walking one line at a time.
fn last_touched_line(st: &CodeEditorState, first: &LineInfo, b: usize) -> LineInfo {
    let mut cur = first.clone();
    loop {
        let next_start = cur.end() + 1;
        if b > next_start
            && let Some(next) = line_at(st, next_start)
        {
            cur = next;
        } else {
            return cur;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Brackets: auto-close, type-over, pair-backspace
// ─────────────────────────────────────────────────────────────────────────

/// Whether a typed character needs the bracket-aware path rather than the
/// batched insert. Only when bracket auto-closing is on and the character is a
/// configured delimiter — every other keystroke stays on the fast batched path,
/// so plain text and a `PlainTextEditor` pay nothing.
pub(super) fn wants_bracket_handling(st: &CodeEditorState, s: &str) -> bool {
    if !st.config.auto_close_brackets {
        return false;
    }
    let mut chars = s.chars();
    let (Some(ch), None) = (chars.next(), chars.next()) else {
        return false; // only single characters
    };
    st.config.closing_for(ch).is_some() || st.config.opening_for(ch).is_some()
}

/// Handle a typed bracket character: auto-close an opener, type over a closer,
/// or surround a selection.
///
/// Flushes any pending batched characters first, so the neighbour checks see the
/// document the user actually typed rather than a stale one.
pub(super) fn type_bracket_char(st: &mut CodeEditorState, ch: char) {
    let batch = std::mem::take(&mut st.pending_chars);
    if !batch.is_empty() {
        super::frame_loop::insert_at_every_caret(st, &batch);
    }
    let config = st.config.clone();

    if let Some(close) = config.closing_for(ch) {
        auto_close_open(st, ch, close);
    } else if let Some(_open) = config.opening_for(ch) {
        type_close(st, ch);
    }
}

/// Insert an opener: wrap a selection, or place the pair and sit between it.
fn auto_close_open(st: &mut CodeEditorState, open: char, close: char) {
    let config = st.config.clone();
    // Always one undo step: the surround branch is two inserts, which must undo
    // together. Descending by trailing edge so lower carets stay valid.
    st.cursor.begin_edit_block();
    for i in carets_by_trailing_edge(st) {
        let (s, e) = caret_span(st, i);
        if s != e {
            // Surround the selection, keeping it selected.
            let c = caret_at(st, i);
            c.set_position(e, MoveMode::MoveAnchor);
            let _ = c.insert_text(&close.to_string());
            c.set_position(s, MoveMode::MoveAnchor);
            let _ = c.insert_text(&open.to_string());
            c.set_position(s + 1, MoveMode::MoveAnchor);
            c.set_position(e + 1, MoveMode::KeepAnchor);
        } else {
            let pos = caret_pos(st, i);
            // Only auto-close where a closer would not strand a following word:
            // at end of line, before whitespace, or before another closer.
            let after = char_after(st, pos);
            let should_close = match after {
                None => true,
                Some(a) => a.is_whitespace() || config.opening_for(a).is_some(),
            };
            let c = caret_at(st, i);
            if should_close {
                let _ = c.insert_text(&format!("{open}{close}"));
                c.set_position(pos + 1, MoveMode::MoveAnchor);
            } else {
                let _ = c.insert_text(&open.to_string());
            }
        }
    }
    st.cursor.end_edit_block();
    st.merge_collided_carets();
}

/// Insert a closer, or step over one already present (so the closer that
/// auto-close inserted is not doubled when the user types it themselves).
fn type_close(st: &mut CodeEditorState, close: char) {
    // One mutation per caret, so a lone caret needs no grouping; group only when
    // there are several. Trailing-edge order so a caret whose selection is
    // replaced does not shift one still to be handled.
    let multi = !st.extra_carets.is_empty();
    if multi {
        st.cursor.begin_edit_block();
    }
    for i in carets_by_trailing_edge(st) {
        let pos = caret_pos(st, i);
        let (s, e) = caret_span(st, i);
        let has_sel = s != e;
        if !has_sel && char_after(st, pos) == Some(close) {
            caret_at(st, i).move_position(MoveOperation::Right, MoveMode::MoveAnchor, 1);
        } else {
            let _ = caret_at(st, i).insert_text(&close.to_string());
        }
    }
    if multi {
        st.cursor.end_edit_block();
    }
    st.merge_collided_carets();
}

/// Delete an empty bracket pair in one keystroke when Backspace lands between
/// its open and close (`(|)` → ``).
///
/// Single-caret and selection-free by decision: it is the undo for auto-close a
/// beat after it fired, which is a single-caret gesture, and folding it into the
/// multi-caret delete path would complicate that path for a marginal case.
/// Returns whether it consumed the keystroke; when it did not, the ordinary
/// delete runs.
pub(super) fn try_pair_backspace(st: &mut CodeEditorState) -> bool {
    if !st.config.auto_close_brackets || !st.extra_carets.is_empty() {
        return false;
    }
    if st.cursor.has_selection() {
        return false;
    }
    // A stale batch would sit between the caret and the closer; flush it so the
    // neighbour check is honest, then re-test.
    let batch = std::mem::take(&mut st.pending_chars);
    if !batch.is_empty() {
        let _ = st.cursor.insert_text(&batch);
    }
    let pos = st.cursor.position();
    let (Some(before), Some(after)) = (char_before(st, pos), char_after(st, pos)) else {
        return false;
    };
    if st.config.closing_for(before) != Some(after) {
        return false;
    }
    st.cursor.begin_edit_block();
    let _ = st.cursor.delete_char(); // the closer
    let _ = st.cursor.delete_previous_char(); // the opener
    st.cursor.end_edit_block();
    true
}

// ─────────────────────────────────────────────────────────────────────────
// Add caret above / below
// ─────────────────────────────────────────────────────────────────────────

/// Add a caret one line above the topmost caret, at the same column.
///
/// Column editing: the new caret lands under the current one, clamped to the
/// shorter line's end. A no-op at the top of the document, and a no-op if the
/// line above already has a caret at that column.
pub(super) fn add_caret_above(st: &mut CodeEditorState) {
    let top = st
        .all_carets()
        .map(|c| c.position())
        .min()
        .unwrap_or_else(|| st.cursor.position());
    let Some(line) = line_at(st, top) else { return };
    if line.start == 0 {
        return;
    }
    let col = top - line.start;
    let Some(prev) = line_at(st, line.start - 1) else {
        return;
    };
    add_caret_at_column(st, &prev, col);
}

/// Add a caret one line below the bottommost caret, at the same column.
pub(super) fn add_caret_below(st: &mut CodeEditorState) {
    let bottom = st
        .all_carets()
        .map(|c| c.position())
        .max()
        .unwrap_or_else(|| st.cursor.position());
    let Some(line) = line_at(st, bottom) else {
        return;
    };
    let col = bottom - line.start;
    let Some(next) = next_line(st, &line) else {
        return;
    };
    add_caret_at_column(st, &next, col);
}

fn add_caret_at_column(st: &mut CodeEditorState, line: &LineInfo, col: usize) {
    let target = line.start + col.min(line.len);
    if st.cursor.position() == target || st.extra_carets.iter().any(|c| c.position() == target) {
        return;
    }
    let c = st.document.cursor();
    c.set_position(target, MoveMode::MoveAnchor);
    st.extra_carets.push(c);
}

// ─────────────────────────────────────────────────────────────────────────
// Bracket matching (computation only; the paint of the pair rides with the
// current-line band in the paint phase)
// ─────────────────────────────────────────────────────────────────────────

/// The bracket adjacent to the primary caret and its match, or `None`.
///
/// Prefers the character just before the caret (where the caret sits after
/// typing a bracket), then the one just after. Scans outward one block at a time
/// counting nesting of the same delimiter, capped so a caret next to an unmatched
/// bracket in a large document does not walk the whole thing. Pure computation —
/// no document mutation — so the caller can run it on every caret move.
pub(super) fn current_bracket_match(st: &CodeEditorState) -> Option<(usize, usize)> {
    if st.config.brackets.is_empty() {
        return None;
    }
    let pos = st.cursor.position();

    // Prefer the delimiter behind the caret.
    if let Some(ch) = char_before(st, pos)
        && let Some(m) = match_from(st, pos - 1, ch)
    {
        return Some((pos - 1, m));
    }
    if let Some(ch) = char_after(st, pos)
        && let Some(m) = match_from(st, pos, ch)
    {
        return Some((pos, m));
    }
    None
}

/// How far outward bracket matching scans before giving up. A match beyond this
/// is not visually useful, and the cap keeps an unmatched bracket cheap.
const MATCH_SCAN_CHAR_CAP: usize = 50_000;

/// The position of the delimiter matching the one at `at`, if `ch` is a
/// configured bracket. Scans forward for an opener, backward for a closer.
fn match_from(st: &CodeEditorState, at: usize, ch: char) -> Option<usize> {
    if let Some(close) = st.config.closing_for(ch) {
        scan(st, at, ch, close, true)
    } else if let Some(open) = st.config.opening_for(ch) {
        scan(st, at, open, ch, false)
    } else {
        None
    }
}

/// Walk from `origin` counting `open`/`close` depth, returning the position that
/// balances the delimiter at `origin`. `forward` scans toward the end of the
/// document (matching an opener); otherwise toward the start.
///
/// Crossing a block boundary uses [`next_line`] (forward) and the in-range
/// separator position (backward), both of which report the true end of the
/// document — a plain `line_at` past the end would *clamp* to the last block and
/// spin the last line under the scan until the cap, which is exactly the case an
/// unmatched opener hits while you are still typing its partner. Each line's
/// characters are collected once so indexing within a line is O(1) rather than a
/// fresh `chars().nth(col)` per position.
fn scan(
    st: &CodeEditorState,
    origin: usize,
    open: char,
    close: char,
    forward: bool,
) -> Option<usize> {
    let mut depth = 0i32;
    let mut scanned = 0usize;

    let mut line = line_at(st, origin)?;
    let mut chars: Vec<char> = line.text.chars().collect();
    let mut col = origin - line.start;

    loop {
        if scanned > MATCH_SCAN_CHAR_CAP {
            return None;
        }
        if col < line.len {
            let c = chars[col];
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
            }
            if depth == 0 {
                return Some(line.start + col);
            }
            scanned += 1;
        }

        if forward {
            col += 1;
            if col >= line.len {
                line = next_line(st, &line)?;
                chars = line.text.chars().collect();
                col = 0;
            }
        } else {
            if col == 0 {
                if line.start == 0 {
                    return None;
                }
                // The separator before this line is in range, so this never
                // clamps.
                line = line_at(st, line.start - 1)?;
                chars = line.text.chars().collect();
                if line.len == 0 {
                    // An empty predecessor: step further back next iteration
                    // rather than index into nothing.
                    continue;
                }
                col = line.len - 1;
            } else {
                col -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_whitespace_stops_at_content() {
        assert_eq!(leading_whitespace("    code"), "    ");
        assert_eq!(leading_whitespace("\t\tcode"), "\t\t");
        assert_eq!(leading_whitespace("code"), "");
        // A whole-whitespace line has no content to stop at, so all of it counts.
        assert_eq!(leading_whitespace("    "), "    ");
    }

    #[test]
    fn tab_insertion_reaches_the_next_stop() {
        let c = CodeConfig {
            indent: super::super::config::IndentStyle::Spaces(4),
            ..CodeConfig::default()
        };
        assert_eq!(tab_insertion(&c, 0), "    ", "column 0 → 4 spaces");
        assert_eq!(tab_insertion(&c, 2), "  ", "column 2 → 2 spaces to reach 4");
        assert_eq!(
            tab_insertion(&c, 4),
            "    ",
            "column 4 → a full 4 to reach 8"
        );
        assert_eq!(tab_insertion(&c, 5), "   ", "column 5 → 3 to reach 8");
    }

    #[test]
    fn hard_tab_inserts_one_character() {
        let c = CodeConfig {
            indent: super::super::config::IndentStyle::Tabs { width: 4 },
            ..CodeConfig::default()
        };
        assert_eq!(tab_insertion(&c, 3), "\t");
    }

    #[test]
    fn dedent_count_removes_one_level_of_spaces() {
        assert_eq!(dedent_count("        x", 4), 4, "8 spaces → remove 4");
        assert_eq!(
            dedent_count("  x", 4),
            2,
            "under-indented → remove what is there"
        );
        assert_eq!(dedent_count("x", 4), 0, "no indent → nothing");
    }

    #[test]
    fn dedent_count_removes_one_tab() {
        assert_eq!(dedent_count("\t\tx", 4), 1, "one tab is one level");
        assert_eq!(dedent_count("\tx", 8), 1);
    }
}
