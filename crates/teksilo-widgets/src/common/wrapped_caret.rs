// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where a caret on a soft wrap goes in an editor's text runs.
//!
//! A position on a wrap is the end of one line and the start of the next, and
//! the affinity says which the caret is shown on. `TextRunEmission::position_of`
//! gives the earlier run, which is right for a caret shown at the end of the
//! line above. A caret shown at the start of the line below (after Down from a
//! line's start, Home on a wrapped line, or a click there) is in that line's
//! first run: a reader asked for the caret's line otherwise heard the line
//! above again, and the next Down left the paragraph with the rest of it
//! unread.
//!
//! Both ends of the selection have to say so. UIA's `GetSelection` range starts
//! at the anchor, and NVDA expands its line from that start, so a caret whose
//! anchor stays on the line above is read as that line.
//!
//! A non-empty selection is on its text: its lower end is on the line of the
//! first character it holds, and its upper end on the line of the last.
//!
//! A hard break leaves no choice. The position after it is on the next line
//! whatever the affinity, and the end of the run that holds the break would
//! read as the line before it.

use teksilo_core::accessibility::text_runs::{TextRunEmission, TextRunSource};
use teksilo_core::accesskit::NodeId;
use teksilo_text::{CursorAffinity, RichTextEngine};

/// The position of the line that starts at character `offset` of `source`, when
/// one does. A position the runs split on one line (a 255-character chunk) is
/// no wrap and has none, and neither has the start of the source, where no line
/// ends.
pub(crate) fn line_starting_at(
    emission: &TextRunEmission,
    source: &TextRunSource<'_>,
    offset: usize,
) -> Option<(NodeId, usize)> {
    if offset == 0 {
        return None;
    }
    emission
        .runs
        .iter()
        .find(|run| {
            run.char_range.start == offset
                && source
                    .lines
                    .iter()
                    .any(|line| line.byte_range.start == run.byte_range.start)
        })
        .map(|run| (run.id, 0))
}

/// Whether character `offset` of `source` is just after a hard break. A run
/// carries its line's break in its characters and not in its text.
pub(crate) fn follows_hard_break(
    emission: &TextRunEmission,
    source: &TextRunSource<'_>,
    offset: usize,
) -> bool {
    let text = source.text.as_ref();
    emission.runs.iter().any(|run| {
        run.char_range.end == offset
            && run.char_range.len() > text[run.byte_range.clone()].chars().count()
    })
}

/// Whether the laid-out position `shown` is a soft wrap, where two lines hold
/// the caret and the affinity says which.
pub(crate) fn is_soft_wrap(engine: &RichTextEngine, shown: usize) -> bool {
    let line = |affinity| engine.visual_line_range_at(shown, affinity);
    line(CursorAffinity::Downstream) != line(CursorAffinity::Upstream)
}

/// The affinity for a caret a character step, or a removal, has just left at
/// the laid-out position `shown`: on a soft wrap the start of the next line, so
/// no line's first character is skipped.
pub(crate) fn affinity_after_step(engine: &RichTextEngine, shown: usize) -> CursorAffinity {
    if is_soft_wrap(engine, shown) {
        CursorAffinity::Upstream
    } else {
        CursorAffinity::Downstream
    }
}

/// The affinity for a caret an assistive technology put at `character_index` of
/// a run. Its position names the line: the end of a line's last run is that
/// line, the start of a line's first run is the line it starts.
pub(crate) fn affinity_of_run_position(character_index: usize) -> CursorAffinity {
    if character_index == 0 {
        CursorAffinity::Upstream
    } else {
        CursorAffinity::Downstream
    }
}
