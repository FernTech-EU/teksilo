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

use teksilo_core::accessibility::text_runs::{TextRunEmission, TextRunSource};
use teksilo_core::accesskit::NodeId;

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
