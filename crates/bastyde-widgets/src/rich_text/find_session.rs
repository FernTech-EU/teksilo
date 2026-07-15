// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A find-highlight layer over one document.
//!
//! [`FindSession`] owns a **range session** on a [`TextDocument`] (see
//! `add_range_session`) and keeps its ranges in step with the matches of a query — the
//! *current* match formatted one way, the others another. It is the search side of the
//! highlight registry: the document holds the layer, a per-view [`HighlightMask`] decides
//! which panes render it, and this drives what it contains.
//!
//! It is deliberately **pure over the document** — no widget, no theme. The colours come in as
//! two [`HighlightFormat`]s (the caller resolves them from semantic theme roles), and the
//! matcher is text-document's own, so a project-wide search and this in-editor find can never
//! disagree about what a match is. A view wires it up by including
//! [`session_id`](FindSession::session_id) in its mask (the default `all()` already shows it)
//! and calling `select_range` / `reveal_range` on the current match.
//!
//! [`HighlightMask`]: bastyde_text::text_document::HighlightMask

use bastyde_text::text_document::{
    FindMatch, FindOptions, HighlightFormat, RangeHighlight, SessionId, TextDocument,
};

/// A search-highlight layer: the matches of a query, as a document range session, with the
/// current match distinguished.
pub struct FindSession {
    doc: TextDocument,
    session: SessionId,
    matches: Vec<FindMatch>,
    /// Index into `matches` of the current match. Meaningless when `matches` is empty.
    current: usize,
    current_format: HighlightFormat,
    other_format: HighlightFormat,
}

impl FindSession {
    /// Attach a fresh, empty find session to `doc`. `current_format` styles the current match
    /// (e.g. the editor selection colour); `other_format` styles the rest (e.g. a subtle
    /// accent). Both should be **paint-only** (background / underline) so the highlight stays
    /// out of the accessibility tree — a screen-reader user navigates matches by count, not by
    /// colour.
    pub fn new(
        doc: &TextDocument,
        current_format: HighlightFormat,
        other_format: HighlightFormat,
    ) -> Self {
        let session = doc.add_range_session();
        Self {
            doc: doc.clone(),
            session,
            matches: Vec::new(),
            current: 0,
            current_format,
            other_format,
        }
    }

    /// The document session this layer owns — put it in a view's
    /// [`HighlightMask`](bastyde_text::text_document::HighlightMask) to render it there.
    pub fn session_id(&self) -> SessionId {
        self.session
    }

    /// Re-run `query` and highlight every match; the first becomes current. An empty query
    /// clears the highlighting.
    pub fn set_query(&mut self, query: &str, options: &FindOptions) {
        self.matches = if query.is_empty() {
            Vec::new()
        } else {
            // Best-effort: a search that errors (e.g. a malformed regex) simply highlights
            // nothing, rather than propagating into a banner that just wanted to draw boxes.
            self.doc.find_all(query, options).unwrap_or_default()
        };
        self.current = 0;
        self.apply();
    }

    /// How many matches the last query found.
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// The current match's 0-based index (`0` when there are no matches).
    pub fn current_index(&self) -> usize {
        self.current
    }

    /// The current match, if any.
    pub fn current_match(&self) -> Option<FindMatch> {
        self.matches.get(self.current).cloned()
    }

    /// Advance to the next match, wrapping past the end, and return it. `None` if there are no
    /// matches.
    pub fn next_match(&mut self) -> Option<FindMatch> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = (self.current + 1) % self.matches.len();
        self.apply();
        self.current_match()
    }

    /// Step to the previous match, wrapping past the start, and return it.
    pub fn prev_match(&mut self) -> Option<FindMatch> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = (self.current + self.matches.len() - 1) % self.matches.len();
        self.apply();
        self.current_match()
    }

    /// Clear all highlighting (the query went away, or the banner closed).
    pub fn clear(&mut self) {
        self.matches.clear();
        self.current = 0;
        self.apply();
    }

    /// Push the current match set to the document as range highlights — current match in one
    /// format, the rest in the other. The current range is emitted **last**, so where matches
    /// abut, its format wins the registry's last-writer-per-field merge.
    fn apply(&self) {
        let mut ranges: Vec<RangeHighlight> = Vec::with_capacity(self.matches.len());
        for (i, m) in self.matches.iter().enumerate() {
            if i == self.current {
                continue;
            }
            ranges.push(RangeHighlight {
                start: m.position,
                length: m.length,
                format: self.other_format.clone(),
            });
        }
        if let Some(cur) = self.matches.get(self.current) {
            ranges.push(RangeHighlight {
                start: cur.position,
                length: cur.length,
                format: self.current_format.clone(),
            });
        }
        self.doc.set_session_ranges(self.session, ranges);
    }
}

impl Drop for FindSession {
    /// Retire the session so a closed find banner leaves no highlight layer behind on the
    /// shared document.
    fn drop(&mut self) {
        self.doc.remove_session(self.session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_text::text_document::{Color, FlowElement, FlowElementSnapshot, HighlightMask};

    fn bg(color: Color) -> HighlightFormat {
        HighlightFormat {
            background_color: Some(color),
            ..Default::default()
        }
    }

    const CURRENT: Color = Color {
        red: 0,
        green: 120,
        blue: 255,
        alpha: 255,
    };
    const OTHER: Color = Color {
        red: 255,
        green: 214,
        blue: 0,
        alpha: 150,
    };

    fn doc(text: &str) -> TextDocument {
        let d = TextDocument::new();
        d.set_plain_text(text).unwrap();
        d
    }

    fn paint_spans(doc: &TextDocument) -> Vec<bastyde_text::text_document::PaintHighlightSpan> {
        match &doc.snapshot_flow_masked(&HighlightMask::all()).elements[0] {
            FlowElementSnapshot::Block(b) => b.paint_highlights.clone(),
            _ => panic!("block"),
        }
    }

    #[test]
    fn a_query_highlights_every_match_with_the_current_distinguished() {
        let d = doc("elena and Elena and ELENA");
        let mut fs = FindSession::new(&d, bg(CURRENT), bg(OTHER));
        fs.set_query("elena", &FindOptions::default());

        assert_eq!(fs.match_count(), 3, "case-folded: all three");
        let spans = paint_spans(&d);
        // Three highlighted ranges; exactly one carries the current colour.
        let current: Vec<_> = spans
            .iter()
            .filter(|s| s.background_color == Some(CURRENT))
            .collect();
        let others: Vec<_> = spans
            .iter()
            .filter(|s| s.background_color == Some(OTHER))
            .collect();
        assert_eq!(current.len(), 1, "one current match");
        assert_eq!(others.len(), 2, "two other matches");
        // The current match is the first occurrence.
        assert_eq!(current[0].start, 0);
    }

    #[test]
    fn next_and_prev_move_the_current_match_and_wrap() {
        let d = doc("a x a x a");
        let mut fs = FindSession::new(&d, bg(CURRENT), bg(OTHER));
        fs.set_query("a", &FindOptions::default());
        assert_eq!(fs.match_count(), 3);
        assert_eq!(fs.current_index(), 0);

        assert_eq!(fs.next_match().unwrap().position, 4); // second "a" at char 4
        assert_eq!(fs.current_index(), 1);
        fs.next_match();
        assert_eq!(fs.current_index(), 2);
        fs.next_match(); // wraps
        assert_eq!(fs.current_index(), 0);
        fs.prev_match(); // wraps back to the end
        assert_eq!(fs.current_index(), 2);
    }

    #[test]
    fn an_empty_query_clears_the_highlighting() {
        let d = doc("hello hello");
        let mut fs = FindSession::new(&d, bg(CURRENT), bg(OTHER));
        fs.set_query("hello", &FindOptions::default());
        assert!(!paint_spans(&d).is_empty());
        fs.set_query("", &FindOptions::default());
        assert!(paint_spans(&d).is_empty(), "cleared");
        assert_eq!(fs.match_count(), 0);
    }

    #[test]
    fn dropping_the_session_removes_the_layer() {
        let d = doc("hello hello");
        {
            let mut fs = FindSession::new(&d, bg(CURRENT), bg(OTHER));
            fs.set_query("hello", &FindOptions::default());
            assert!(!paint_spans(&d).is_empty());
        } // fs dropped here
        assert!(
            paint_spans(&d).is_empty(),
            "the find session's layer must not outlive it"
        );
    }
}
