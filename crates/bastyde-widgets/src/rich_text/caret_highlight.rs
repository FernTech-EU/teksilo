// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! An ambient band behind the sentence — or paragraph — the caret is in.
//!
//! [`CaretHighlightSession`] owns a **range session** on a [`TextDocument`] (see
//! `add_range_session_with_priority`) holding at most one range: the extent of whatever the
//! caret is currently inside. It is the writing-comfort counterpart of
//! [`FindSession`](super::find_session::FindSession) — same registry, same staleness discipline,
//! opposite intent. Find answers "where is this text"; this answers "where am I".
//!
//! ## Only the focused view bands
//!
//! Two panes over one document each own a session, and an **unfocused view clears its range**.
//! So the union of what the document carries is exactly one band, at the caret of the pane
//! being written in, and neither view needs a [`HighlightMask`] to say so. That is also what
//! makes the band vanish the moment focus leaves the editor, which is what you want: the band
//! marks where you are *writing*, not where a caret happens to rest.
//!
//! ## Priority, not registration order
//!
//! The session registers at [`CARET_HIGHLIGHT_PRIORITY`], below every other layer, so a find
//! match or a spell squiggle always paints over the band. Registration order could not express
//! that: a view's session is registered when the view appears, so a split pane opened *after*
//! the find banner would outrank it in that pane and not in its sibling.
//!
//! ## Staleness on edit
//!
//! The range is an absolute char offset frozen at push time, and text-document does not
//! re-anchor a range session the way it re-anchors carets. Like `FindSession` this one
//! subscribes to its document and marks itself stale on any content edit; the host calls
//! [`refresh`](CaretHighlightSession::refresh) each frame with the live caret, which is
//! cheap — a caret that has not moved into a different sentence pushes nothing at all.
//!
//! [`HighlightMask`]: bastyde_text::text_document::HighlightMask

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bastyde_text::text_document::{
    DocumentEvent, HighlightFormat, RangeHighlight, SessionId, Subscription, TextDocument,
};

/// Where the caret-highlight session sits in the document's merge order.
///
/// Far below the default `0` every other layer takes, so anything meaningful — a find match, a
/// spell squiggle, a syntax colour — wins the overlap. The band is ambient; it must never hide
/// something the writer asked to see.
pub const CARET_HIGHLIGHT_PRIORITY: i32 = -1000;

/// How much text around the caret the band covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaretHighlightScope {
    /// The sentence the caret is in, in [`CaretHighlight::content_locale`]'s language.
    Sentence,
    /// The whole paragraph (block) the caret is in. Needs no language.
    Paragraph,
}

/// The band an editor should draw, or the absence of one.
///
/// Handed over whole rather than field by field, so a host pushes the same way whichever of
/// its inputs changed — the shape [`EditorTypographyDefaults`](bastyde_text::EditorTypographyDefaults)
/// already uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaretHighlight {
    pub scope: CaretHighlightScope,
    /// How the band paints. Should be **paint-only** — a background colour — so it stays a
    /// recolor rather than a reshape, and stays out of the accessibility tree.
    pub format: HighlightFormat,
    /// BCP-47-ish tag naming the language of the text, which selects the sentence tailoring
    /// (abbreviations, French spaced guillemets, the Greek question mark). Ignored by
    /// [`CaretHighlightScope::Paragraph`], which needs no language to find a block.
    pub content_locale: Option<String>,
}

/// The band layer for one view of one document.
pub(crate) struct CaretHighlightSession {
    doc: TextDocument,
    session: SessionId,
    /// What to draw, or `None` while the feature is off. Kept even when unfocused, so regaining
    /// focus needs no re-push from the host.
    config: RefCell<Option<CaretHighlight>>,
    focused: Cell<bool>,
    /// The range last pushed, so an unchanged recompute skips the push — and so a format-only
    /// change (a theme switch) can re-push the same extent without re-deriving it.
    last: Cell<Option<(usize, usize)>>,
    /// Set by the document subscription on any offset-moving edit; drained by
    /// [`refresh`](Self::refresh).
    dirty: Arc<AtomicBool>,
    /// Kept alive so the subscription lives as long as the session.
    _sub: Subscription,
}

impl CaretHighlightSession {
    /// Attach an idle band session to `doc`. Draws nothing until
    /// [`set_config`](Self::set_config) and [`set_focused`](Self::set_focused) both say so.
    pub(crate) fn new(doc: &TextDocument) -> Self {
        let session = doc.add_range_session_with_priority(CARET_HIGHLIGHT_PRIORITY);
        let dirty = Arc::new(AtomicBool::new(false));
        let sub = {
            let dirty = dirty.clone();
            doc.on_change(move |event| {
                // Only edits that MOVE char offsets stale the cached range. Format- and
                // highlight-only events leave the text where it was — and reacting to
                // `HighlightPaintChanged` here would loop, since this session's own
                // `set_session_ranges` emits exactly that. Same filter `FindSession` uses.
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
            config: RefCell::new(None),
            focused: Cell::new(false),
            last: Cell::new(None),
            dirty,
            _sub: sub,
        }
    }

    /// The document session this layer owns — for a view's
    /// [`HighlightMask`](bastyde_text::text_document::HighlightMask), should it name one.
    #[allow(dead_code)]
    pub(crate) fn session_id(&self) -> SessionId {
        self.session
    }

    /// What this session is currently configured to draw.
    pub(crate) fn config(&self) -> Option<CaretHighlight> {
        self.config.borrow().clone()
    }

    /// Set (or clear) what to draw. Returns `true` if a repaint is owed.
    ///
    /// A change to the **format alone** — what a light/dark switch does — re-pushes the extent
    /// already resolved rather than recomputing it, so a theme toggle costs one range write per
    /// open editor and no segmentation at all.
    pub(crate) fn set_config(&self, config: Option<CaretHighlight>) -> bool {
        let previous = self.config.replace(config.clone());
        if previous == config {
            return false;
        }
        match (&previous, &config) {
            (None, _) | (_, None) => {
                // Turned on or off: `refresh` resolves it (or `clear` empties it) next.
                if config.is_none() {
                    return self.clear();
                }
                self.last.set(None);
                true
            }
            (Some(before), Some(after)) if before.scope == after.scope
                && before.content_locale == after.content_locale =>
            {
                // Format-only: repaint the same extent in the new colour.
                match self.last.get() {
                    Some(range) => self.push(Some(range), after),
                    None => true,
                }
            }
            _ => {
                // The scope or the language changed: the extent has to be re-derived.
                self.last.set(None);
                true
            }
        }
    }

    /// Tell the session whether its view has focus. An unfocused view draws no band — see the
    /// module docs for why that is what makes split panes work without a mask.
    ///
    /// Returns `true` if a repaint is owed.
    pub(crate) fn set_focused(&self, focused: bool) -> bool {
        if self.focused.replace(focused) == focused {
            return false;
        }
        if focused { true } else { self.clear() }
    }

    /// Re-resolve the band for `caret` and push it if it moved. Returns `true` if the pushed
    /// range changed, so the caller can pump a frame.
    ///
    /// Cheap to call every frame: an unfocused or unconfigured session returns immediately, and
    /// a caret that stayed inside the same sentence pushes nothing.
    pub(crate) fn refresh(&self, caret: usize) -> bool {
        let stale = self.dirty.swap(false, Ordering::Relaxed);
        let config = self.config.borrow().clone();
        let Some(config) = config else {
            return false;
        };
        if !self.focused.get() {
            return false;
        }
        let range = self.resolve(caret, &config);
        // An edit can leave the extent numerically identical while the text under it changed
        // (typing inside a sentence that already ran to the block's end), so a staled session
        // pushes even when the range matches.
        if range == self.last.get() && !stale {
            return false;
        }
        self.push(range, &config)
    }

    /// The extent the caret is inside, per the scope.
    fn resolve(&self, caret: usize, config: &CaretHighlight) -> Option<(usize, usize)> {
        match config.scope {
            CaretHighlightScope::Sentence => self
                .doc
                .sentence_at(caret, config.content_locale.as_deref()),
            CaretHighlightScope::Paragraph => {
                let block = self.doc.block_at(caret).ok()?;
                let (start, len) = (block.start, block.length);
                (len > 0).then_some((start, start + len))
            }
        }
    }

    /// Write `range` to the document as this session's only highlight.
    fn push(&self, range: Option<(usize, usize)>, config: &CaretHighlight) -> bool {
        let ranges = match range {
            Some((start, end)) if end > start => vec![RangeHighlight {
                start,
                length: end - start,
                format: config.format.clone(),
            }],
            _ => Vec::new(),
        };
        self.doc.set_session_ranges(self.session, ranges);
        self.last.set(range);
        true
    }

    /// Drop the band without forgetting the configuration. Returns `true` if anything went away.
    fn clear(&self) -> bool {
        if self.last.get().is_none() {
            return false;
        }
        self.doc.set_session_ranges(self.session, Vec::new());
        self.last.set(None);
        true
    }
}

impl Drop for CaretHighlightSession {
    /// Retire the session, so a closed editor leaves no band behind on a document its siblings
    /// are still showing. The `Subscription`'s own drop stops callbacks but does **not** remove
    /// the highlight layer — exactly as `FindSession` documents.
    fn drop(&mut self) {
        self.doc.remove_session(self.session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_text::text_document::{Color, FlowElementSnapshot, HighlightMask};

    const BAND: Color = Color {
        red: 255,
        green: 254,
        blue: 235,
        alpha: 255,
    };
    const OTHER: Color = Color {
        red: 255,
        green: 140,
        blue: 0,
        alpha: 255,
    };

    fn doc(text: &str) -> TextDocument {
        let d = TextDocument::new();
        d.set_plain_text(text).unwrap();
        d
    }

    fn band(scope: CaretHighlightScope) -> CaretHighlight {
        CaretHighlight {
            scope,
            format: HighlightFormat {
                background_color: Some(BAND),
                ..Default::default()
            },
            content_locale: Some("en".into()),
        }
    }

    /// The band's paint spans on a block, as `(start, length)`.
    fn spans(doc: &TextDocument, block: usize) -> Vec<(usize, usize)> {
        match &doc.snapshot_flow_masked(&HighlightMask::all()).elements[block] {
            FlowElementSnapshot::Block(b) => b
                .paint_highlights
                .iter()
                .filter(|s| s.background_color == Some(BAND))
                .map(|s| (s.start, s.length))
                .collect(),
            _ => panic!("block"),
        }
    }

    /// A focused, configured session ready to band.
    fn live(d: &TextDocument, scope: CaretHighlightScope) -> CaretHighlightSession {
        let s = CaretHighlightSession::new(d);
        s.set_config(Some(band(scope)));
        s.set_focused(true);
        s
    }

    #[test]
    fn each_scope_bands_its_own_extent() {
        let d = doc("One is first. Two is second.");

        let s = live(&d, CaretHighlightScope::Sentence);
        s.refresh(16);
        assert_eq!(spans(&d, 0), [(14, 14)], "just \"Two is second.\"");

        let p = live(&d, CaretHighlightScope::Paragraph);
        // Two sessions now band the same document; look at the paragraph one's own extent by
        // dropping the sentence one first.
        drop(s);
        p.refresh(16);
        assert_eq!(spans(&d, 0), [(0, 28)], "the whole block");
    }

    #[test]
    fn the_band_follows_the_caret_between_sentences() {
        let d = doc("One is first. Two is second.");
        let s = live(&d, CaretHighlightScope::Sentence);

        s.refresh(2);
        assert_eq!(spans(&d, 0), [(0, 13)]);
        assert!(s.refresh(16), "moving to another sentence re-pushes");
        assert_eq!(spans(&d, 0), [(14, 14)]);
    }

    /// The cheap path: a caret moving *within* one sentence changes nothing, so a burst of
    /// keystrokes costs one push and not one per frame.
    #[test]
    fn a_caret_move_inside_the_same_sentence_pushes_nothing() {
        let d = doc("One is first. Two is second.");
        let s = live(&d, CaretHighlightScope::Sentence);
        assert!(s.refresh(2), "the first resolve always pushes");
        assert!(!s.refresh(3), "same sentence: no push");
        assert!(!s.refresh(10), "still the same sentence");
    }

    #[test]
    fn an_unfocused_view_draws_no_band() {
        let d = doc("One is first. Two is second.");
        let s = live(&d, CaretHighlightScope::Sentence);
        s.refresh(2);
        assert!(!spans(&d, 0).is_empty());

        assert!(s.set_focused(false), "losing focus is a repaint");
        assert!(spans(&d, 0).is_empty(), "the band goes away");
        assert!(!s.refresh(2), "and stays away while unfocused");
        assert!(spans(&d, 0).is_empty());

        s.set_focused(true);
        s.refresh(2);
        assert!(!spans(&d, 0).is_empty(), "focus brings it back");
    }

    #[test]
    fn clearing_the_config_clears_the_band() {
        let d = doc("One is first.");
        let s = live(&d, CaretHighlightScope::Sentence);
        s.refresh(2);
        assert!(!spans(&d, 0).is_empty());

        assert!(s.set_config(None));
        assert!(spans(&d, 0).is_empty());
        assert!(!s.refresh(2), "nothing to draw");
    }

    /// A theme switch changes only the colour, and must not need the extent re-derived.
    #[test]
    fn a_format_only_change_repaints_the_same_extent() {
        let d = doc("One is first. Two is second.");
        let s = live(&d, CaretHighlightScope::Sentence);
        s.refresh(16);
        let before = spans(&d, 0);

        let mut recoloured = band(CaretHighlightScope::Sentence);
        recoloured.format.background_color = Some(OTHER);
        assert!(s.set_config(Some(recoloured)));

        // Same extent, new colour — without any call to `refresh`.
        let after = match &d.snapshot_flow_masked(&HighlightMask::all()).elements[0] {
            FlowElementSnapshot::Block(b) => b.paint_highlights.clone(),
            _ => panic!("block"),
        };
        assert_eq!(after.len(), 1);
        assert_eq!((after[0].start, after[0].length), before[0]);
        assert_eq!(after[0].background_color, Some(OTHER));
    }

    #[test]
    fn changing_scope_re_resolves_the_extent() {
        let d = doc("One is first. Two is second.");
        let s = live(&d, CaretHighlightScope::Sentence);
        s.refresh(16);
        assert_eq!(spans(&d, 0), [(14, 14)]);

        s.set_config(Some(band(CaretHighlightScope::Paragraph)));
        s.refresh(16);
        assert_eq!(spans(&d, 0), [(0, 28)]);
    }

    /// Offsets are frozen at push time, so an edit ahead of the band must re-derive it — the
    /// same discipline `FindSession` follows.
    #[test]
    fn an_edit_stales_the_band_and_refresh_re_derives_it() {
        let d = doc("One is first. Two is second.");
        let s = live(&d, CaretHighlightScope::Sentence);
        s.refresh(16);
        assert_eq!(spans(&d, 0), [(14, 14)]);

        d.set_plain_text("XXXX. One is first. Two is second.").unwrap();
        assert!(s.refresh(22), "the edit staled it");
        assert_eq!(spans(&d, 0), [(20, 14)], "the band followed its text");
    }

    #[test]
    fn dropping_the_session_removes_the_layer() {
        let d = doc("One is first.");
        {
            let s = live(&d, CaretHighlightScope::Sentence);
            s.refresh(2);
            assert!(!spans(&d, 0).is_empty());
        }
        assert!(
            spans(&d, 0).is_empty(),
            "the band must not outlive its editor"
        );
    }

    /// The band is ambient and must lose every overlap, whichever layer was registered first.
    #[test]
    fn another_layer_paints_over_the_band() {
        for band_first in [true, false] {
            let d = doc("One is first.");
            let (s, other) = if band_first {
                let s = live(&d, CaretHighlightScope::Sentence);
                (s, d.add_range_session())
            } else {
                let o = d.add_range_session();
                (live(&d, CaretHighlightScope::Sentence), o)
            };
            s.refresh(2);
            d.set_session_ranges(
                other,
                vec![RangeHighlight {
                    start: 0,
                    length: 3,
                    format: HighlightFormat {
                        background_color: Some(OTHER),
                        ..Default::default()
                    },
                }],
            );

            let painted = match &d.snapshot_flow_masked(&HighlightMask::all()).elements[0] {
                FlowElementSnapshot::Block(b) => b.paint_highlights.clone(),
                _ => panic!("block"),
            };
            let at_zero = painted
                .iter()
                .filter(|s| s.start <= 0 && 0 < s.start + s.length)
                .next_back()
                .and_then(|s| s.background_color);
            assert_eq!(
                at_zero,
                Some(OTHER),
                "the other layer must win (band registered first: {band_first})"
            );
        }
    }

    /// An empty block has no sentence and no paragraph extent, and must not push a zero-length
    /// range — which would be a highlight nobody can see but everybody has to merge.
    #[test]
    fn an_empty_block_bands_nothing() {
        let d = doc("Text.\n\nMore.");
        let s = live(&d, CaretHighlightScope::Paragraph);
        let blank = "Text.\n".chars().count();
        s.refresh(blank);
        assert!(spans(&d, 1).is_empty());
    }
}
