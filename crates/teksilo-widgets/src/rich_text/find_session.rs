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
//! ## Staleness on edit
//!
//! Matches are **absolute char offsets**, frozen at [`set_query`](FindSession::set_query). An
//! edit anywhere before a match shifts the text underneath those offsets, and text-document
//! does **not** re-anchor a range session the way it re-anchors carets — so the boxes would
//! drift onto the wrong characters. To make that impossible to forget, a `FindSession`
//! subscribes to its document and marks itself stale on any content edit; the host calls
//! [`refresh_if_stale`](FindSession::refresh_if_stale) (e.g. once per frame) to re-derive. The
//! matcher is cheap over a single open document, and re-deriving is the same discipline the
//! rest of search follows: never carry an offset across an edit.
//!
//! [`HighlightMask`]: teksilo_text::text_document::HighlightMask

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use teksilo_text::text_document::{
    DocumentEvent, FindMatch, FindOptions, HighlightFormat, RangeHighlight, SessionId,
    Subscription, TextDocument,
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
    /// The last query + options, kept so [`refresh_if_stale`](Self::refresh_if_stale) can
    /// re-run them after an edit without the caller re-passing them.
    query: String,
    options: FindOptions,
    /// Set by the document subscription on any offset-moving edit; drained by
    /// [`refresh_if_stale`](Self::refresh_if_stale). `Arc` because the `on_change` callback is
    /// `Send + Sync`.
    dirty: Arc<AtomicBool>,
    /// Kept alive so the subscription lives as long as the session (dropping it unsubscribes).
    _sub: Subscription,
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
        let dirty = Arc::new(AtomicBool::new(false));
        let sub = {
            let dirty = dirty.clone();
            doc.on_change(move |event| {
                // Only edits that MOVE char offsets stale the cached matches. Format- and
                // highlight-only events leave positions where they were — and reacting to
                // `HighlightPaintChanged` here would loop, since this session's own
                // `set_session_ranges` emits exactly that.
                if matches!(
                    event,
                    DocumentEvent::ContentsChanged { .. }
                        | DocumentEvent::DocumentReset
                        | DocumentEvent::BlockCountChanged(_)
                        | DocumentEvent::FlowElementsInserted { .. }
                        | DocumentEvent::FlowElementsRemoved { .. }
                ) {
                    dirty.store(true, Ordering::Relaxed);
                }
            })
        };
        Self {
            doc: doc.clone(),
            session,
            matches: Vec::new(),
            current: 0,
            current_format,
            other_format,
            query: String::new(),
            options: FindOptions::default(),
            dirty,
            _sub: sub,
        }
    }

    /// The document session this layer owns — put it in a view's
    /// [`HighlightMask`](teksilo_text::text_document::HighlightMask) to render it there.
    pub fn session_id(&self) -> SessionId {
        self.session
    }

    /// Re-run `query` and highlight every match; the first becomes current. An empty query
    /// clears the highlighting.
    pub fn set_query(&mut self, query: &str, options: &FindOptions) {
        self.query = query.to_string();
        self.options = options.clone();
        self.rerun();
        self.current = 0;
        self.apply();
    }

    /// Re-derive the matches for the stored query **if** an edit has staled them since the last
    /// run. Returns `true` if it re-derived (so the caller can request a repaint). Cheap to
    /// call every frame: a no-op when nothing has changed.
    ///
    /// The current-match index is clamped, not reset — an edit should not throw away where the
    /// writer was in the match list, only re-locate the matches.
    pub fn refresh_if_stale(&mut self) -> bool {
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return false;
        }
        self.rerun();
        if self.current >= self.matches.len() {
            self.current = self.matches.len().saturating_sub(1);
        }
        self.apply();
        true
    }

    /// Run the stored query against the document now, into `self.matches`, and clear the dirty
    /// flag. Does not touch `current` or push ranges — callers do that.
    fn rerun(&mut self) {
        self.matches = if self.query.is_empty() {
            Vec::new()
        } else {
            // Best-effort: a search that errors (e.g. a malformed regex) simply highlights
            // nothing, rather than propagating into a banner that just wanted to draw boxes.
            self.doc
                .find_all(&self.query, &self.options)
                .unwrap_or_default()
        };
        self.dirty.store(false, Ordering::Relaxed);
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
        self.query.clear();
        self.matches.clear();
        self.current = 0;
        self.dirty.store(false, Ordering::Relaxed);
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
    use teksilo_text::text_document::{Color, FlowElementSnapshot, HighlightMask};

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

    fn paint_spans(doc: &TextDocument) -> Vec<teksilo_text::text_document::PaintHighlightSpan> {
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

    /// **The staleness fix.** An edit that shifts the text must not leave the highlights on the
    /// old offsets — `refresh_if_stale` re-derives against the edited document.
    #[test]
    fn an_edit_stales_the_matches_and_refresh_re_derives_them() {
        let d = doc("the cat sat");
        let mut fs = FindSession::new(&d, bg(CURRENT), bg(OTHER));
        fs.set_query("cat", &FindOptions::default());
        let before = fs.current_match().unwrap();
        assert_eq!(before.position, 4, "`cat` starts at char 4");

        // Insert four chars at the very front: "cat" shifts to char 8.
        d.set_plain_text("XXXXthe cat sat").unwrap();

        // The old match is now stale. A refresh re-locates it.
        assert!(
            fs.refresh_if_stale(),
            "the edit must have marked the session stale"
        );
        let after = fs.current_match().unwrap();
        assert_eq!(after.position, 8, "the match followed the text it names");

        // …and a second refresh with no edit is a cheap no-op.
        assert!(!fs.refresh_if_stale());
    }

    /// A refresh whose re-run drops the match the writer was on clamps the index rather than
    /// panicking or resetting to the top.
    #[test]
    fn refresh_clamps_the_current_index_when_matches_shrink() {
        let d = doc("a a a");
        let mut fs = FindSession::new(&d, bg(CURRENT), bg(OTHER));
        fs.set_query("a", &FindOptions::default());
        fs.next_match();
        fs.next_match(); // current = 2 (the last)
        assert_eq!(fs.current_index(), 2);

        d.set_plain_text("a").unwrap(); // only one match now
        assert!(fs.refresh_if_stale());
        assert_eq!(fs.match_count(), 1);
        assert_eq!(fs.current_index(), 0, "clamped to the one remaining match");
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
