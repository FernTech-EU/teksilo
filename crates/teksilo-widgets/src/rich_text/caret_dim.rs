// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Fade everything but the sentence, or paragraph, the caret is in.
//!
//! The writing-comfort "focus" effect: the unit being written stays at full strength and the
//! rest of the text recedes. It is the counterpart of the [caret band](super::caret_highlight),
//! which shades the same unit from behind; the two combine.
//!
//! ## A property of the view, not of the text
//!
//! Unlike the band, nothing here touches the document. The range kept bright is handed to this
//! editor's own typesetter ([`RichTextEngine::set_dim_outside`]), which bakes the fade into the
//! glyphs, images and strokes it emits for this view alone. So two panes over one document never
//! see each other's focus, and an editor that goes away leaves nothing behind for its siblings
//! to clean up.
//!
//! ## Resolved at paint time
//!
//! Every event that can move the bright range (a caret move, an edit, focus coming or going, a
//! selection starting or ending) already repaints the editor. So the resolver
//! (`CaretDimResolver`) is asked from the paint pass with the live caret, and answers from a cache
//! unless one of those inputs changed, which keeps a caret blink free. The typesetter then redraws
//! only what the change touched.
//!
//! ## Only while writing
//!
//! Like the band, the fade shows only while this editor has focus and there is no selection (of
//! text or of table cells). A selection says where the writer is more precisely and may span
//! several paragraphs they need to see; an unfocused view is being read, not written in.
//!
//! One exception, which the host asks for: a page made of several editors (a manuscript shown
//! as one continuous stream of scenes, a synopsis above its text) is one page to the writer.
//! While one of its editors is being written in, the others should fade whole, not stay bright
//! around the one place that is. The host, which knows which editors form a page, gives them
//! one [`CaretDimPage`], and an unfocused member fades whole while another member is being
//! written in. Done here rather than by the host pushing a flag on focus changes: an editor's
//! focus changes while its own state is borrowed, so a host effect pushing back into it from
//! there would borrow it twice.
//!
//! Each editor notes its focus in the page **when the focus changes** (and when it is parked
//! out of sight), not when it paints. Editors paint in tree order, so an editor above the one
//! just clicked would otherwise paint first, still recorded as the writer while no longer
//! focused, and draw itself plainly for that whole frame. The paint pass notes it again, which
//! only matters if the focus changed by some other route; the members it wakes are then
//! repainted on the next frame.
//!
//! [`RichTextEngine::set_dim_outside`]: teksilo_text::RichTextEngine::set_dim_outside

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use teksilo_core::Signal;
use teksilo_core::signal::ObserverHandle;
use teksilo_text::DimOutside;
use teksilo_text::text_document::{DocumentEvent, Subscription, TextDocument};

use super::caret_highlight::CaretHighlightScope;

/// What an editor should fade, or the absence of it.
///
/// Handed over whole, the shape [`CaretHighlight`](super::caret_highlight::CaretHighlight)
/// already uses, so a host pushes the same way whichever of its inputs changed.
#[derive(Debug, Clone, PartialEq)]
pub struct CaretDim {
    /// The unit kept at full strength around the caret. The same vocabulary as the band, so
    /// "around the caret" means one thing in both.
    pub scope: CaretHighlightScope,
    /// How much of the rest of the text remains, `0.0`–`1.0`. The editor mixes the faded ink
    /// this far toward the page it is drawn on, in sRGB, so a value means the same visible fade
    /// on a light and a dark theme (see [`DimOutside`]).
    pub opacity: f32,
    /// BCP-47-ish tag naming the language of the text, which selects the sentence tailoring.
    /// Ignored by [`CaretHighlightScope::Paragraph`].
    pub content_locale: Option<String>,
    /// The page this editor is one part of, if it is one of several editors on one page.
    ///
    /// While another member of the page is being written in, this one is part of "the rest of
    /// the text" and fades whole. `None` (an unfocused editor shows its text plainly) is right
    /// for an editor that is a page of its own, such as one pane of a split view.
    pub page: Option<CaretDimPage>,
}

/// Ids for pages and for their members; only ever compared for equality.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// The editors that make up one page, for the fade around the caret.
///
/// Give every editor of a page a clone of the same value in its [`CaretDim::page`]. The page
/// records which member is being written in (the one with the focus), so the others can fade
/// whole. Cheap to clone; two values are equal when they are the same page.
#[derive(Clone)]
pub struct CaretDimPage {
    id: u64,
    /// The member being written in, by the id its resolver drew.
    writer: Signal<Option<u64>>,
}

impl CaretDimPage {
    pub fn new() -> Self {
        Self {
            id: next_id(),
            writer: Signal::new(None),
        }
    }

    /// Whether one of the page's editors is being written in right now.
    pub fn is_written_in(&self) -> bool {
        self.writer.get().is_some()
    }

    /// Record whether `member` has the focus. A member that loses it only clears the record
    /// when the record is its own: the member gaining the focus may have written first.
    fn note(&self, member: u64, focused: bool) {
        let current = self.writer.get();
        if focused {
            if current != Some(member) {
                self.writer.set(Some(member));
            }
        } else if current == Some(member) {
            self.writer.set(None);
        }
    }
}

impl Default for CaretDimPage {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for CaretDimPage {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl std::fmt::Debug for CaretDimPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaretDimPage")
            .field("id", &self.id)
            .field("writer", &self.writer.get())
            .finish()
    }
}

/// The range an editor that keeps nothing bright is given: past every position any block can
/// have, so no glyph and no list marker is inside or next to it.
const NOTHING_BRIGHT: (usize, usize) = (usize::MAX, usize::MAX);

/// What [`CaretDimResolver::range`] last answered, and for which inputs.
#[derive(Clone, Copy, PartialEq)]
struct Resolved {
    caret: usize,
    focused: bool,
    selecting: bool,
    /// Whether another member of this editor's page was being written in.
    sibling_writing: bool,
    /// The document's `content_revision` when this was resolved.
    revision: u64,
    range: Option<DimOutside>,
}

/// Turns the live caret into the range to keep bright, for one view of one document.
pub(crate) struct CaretDimResolver {
    doc: TextDocument,
    config: RefCell<CaretDim>,
    last: Cell<Option<Resolved>>,
    /// This editor's id within its page.
    member: u64,
    /// The editor's `paint_version`, bumped when another member of the page starts or stops
    /// being written in: nothing this editor watches changes then, so without it a member
    /// would keep its old look until something else repainted it.
    paint_version: Signal<u64>,
    /// The tree's frame-request flag, once the editor is mounted, armed with each bump above.
    /// A bump is only read when a frame is laid out; one made while a frame is being painted
    /// (another member noting its focus there) would otherwise wait for the next caret blink,
    /// or forever with a caret that does not blink. Shared with the page watch, which outlives
    /// a remount.
    frame_request: Rc<RefCell<Option<Rc<Cell<bool>>>>>,
    /// The watch on the page's writer, replaced with the page.
    page_watch: RefCell<Option<ObserverHandle>>,
    /// Set by the document subscription on a reset (a load replaces the whole text without
    /// moving `content_revision`, which counts edits). Edits themselves are caught by the
    /// revision, which advances when they are made, even while the document is holding its
    /// events back for an input method.
    dirty: Arc<AtomicBool>,
    /// Kept alive so the subscription lives as long as the resolver.
    _sub: Subscription,
}

impl CaretDimResolver {
    pub(crate) fn new(
        doc: &TextDocument,
        config: CaretDim,
        paint_version: Signal<u64>,
        frame_request: Option<Rc<Cell<bool>>>,
    ) -> Self {
        let dirty = Arc::new(AtomicBool::new(false));
        let sub = {
            let dirty = dirty.clone();
            doc.on_change(move |event| {
                // Offset-moving events, the filter the band and `FindSession` use. The edits
                // among them are also caught by the revision; a reset only here.
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
        let resolver = Self {
            doc: doc.clone(),
            config: RefCell::new(config),
            last: Cell::new(None),
            member: next_id(),
            paint_version,
            frame_request: Rc::new(RefCell::new(frame_request)),
            page_watch: RefCell::new(None),
            dirty,
            _sub: sub,
        };
        resolver.watch_page();
        resolver
    }

    /// Watch the configured page's writer, so this member repaints when another member starts
    /// or stops being written in.
    fn watch_page(&self) {
        let watch = self.config.borrow().page.as_ref().map(|page| {
            let member = self.member;
            let paint_version = self.paint_version.clone();
            let frame_request = self.frame_request.clone();
            page.writer.observe(move |writer| {
                // The member that just took the focus repaints for that already.
                if *writer != Some(member) {
                    paint_version.set(paint_version.get().wrapping_add(1));
                    if let Some(handle) = frame_request.borrow().as_ref() {
                        handle.set(true);
                    }
                }
            })
        });
        self.page_watch.replace(watch);
    }

    /// Withdraw from the configured page, if this member was the one being written in.
    fn leave_page(&self) {
        self.note_focus(false);
    }

    /// Record in the page, if any, whether this editor has the focus. Called when the focus
    /// changes and when the editor is parked, so the page is right before anything paints.
    pub(crate) fn note_focus(&self, focused: bool) {
        if let Some(page) = &self.config.borrow().page {
            page.note(self.member, focused);
        }
    }

    /// Hand over the tree's frame-request flag, once the editor is mounted (again).
    pub(crate) fn set_frame_request(&self, handle: Option<Rc<Cell<bool>>>) {
        self.frame_request.replace(handle);
    }

    pub(crate) fn config(&self) -> CaretDim {
        self.config.borrow().clone()
    }

    /// Replace what to fade. The next [`range`](Self::range) re-resolves.
    pub(crate) fn set_config(&self, config: CaretDim) {
        if *self.config.borrow() == config {
            return;
        }
        let new_page = config.page != self.config.borrow().page;
        if new_page {
            self.leave_page();
        }
        self.config.replace(config);
        self.last.set(None);
        if new_page {
            self.watch_page();
        }
    }

    /// What to fade for an editor whose caret is at `caret`, given whether it has the focus and
    /// whether a selection (of text or of cells) is up, or `None` when nothing should fade.
    ///
    /// Focused without a selection: the caret's sentence or paragraph stays bright. Focused with
    /// one: nothing fades. Unfocused: nothing fades, unless another editor of this one's
    /// [page](CaretDim::page) is being written in, when all of it does. Also notes this
    /// editor's focus in its page again, in case it changed without
    /// [`note_focus`](Self::note_focus) being told.
    pub(crate) fn range(&self, caret: usize, focused: bool, selecting: bool) -> Option<DimOutside> {
        let sibling_writing = match &self.config.borrow().page {
            Some(page) => {
                page.note(self.member, focused);
                page.writer
                    .get()
                    .is_some_and(|writer| writer != self.member)
            }
            None => false,
        };
        let stale = self.dirty.swap(false, Ordering::Relaxed);
        let revision = self.doc.content_revision();
        if !stale
            && let Some(last) = self.last.get()
            && last.caret == caret
            && last.focused == focused
            && last.selecting == selecting
            && last.sibling_writing == sibling_writing
            && last.revision == revision
        {
            return last.range;
        }
        let range = match (focused, selecting) {
            (true, false) => Some(self.resolve(caret)),
            (true, true) => None,
            (false, _) => sibling_writing.then(|| {
                let opacity = self.config.borrow().opacity;
                DimOutside::new(NOTHING_BRIGHT.0, NOTHING_BRIGHT.1, opacity)
            }),
        };
        self.last.set(Some(Resolved {
            caret,
            focused,
            selecting,
            sibling_writing,
            revision,
            range,
        }));
        range
    }

    fn resolve(&self, caret: usize) -> DimOutside {
        let config = self.config.borrow();
        let (start, end) = match config.scope {
            CaretHighlightScope::Sentence => self
                .doc
                .sentence_at(caret, config.content_locale.as_deref())
                // No sentence: an empty or blank paragraph. Nothing in it to keep bright, but
                // the rest must still fade, so an empty range at the caret rather than `None`.
                .unwrap_or((caret, caret)),
            CaretHighlightScope::Paragraph => match self.doc.block_at_caret(caret) {
                // `block_at_caret`, not `block_at`: the latter reads its argument as a character
                // index, where the separator belongs to the next block, so a caret at the end of
                // a paragraph would light the one after it. In a table it is the cell's block.
                Ok(block) => (block.start, block.start + block.length),
                Err(_) => (caret, caret),
            },
        };
        DimOutside::new(start, end, config.opacity)
    }
}

impl Drop for CaretDimResolver {
    /// A resolver goes away when its fade is turned off or its editor is dropped; if its editor
    /// was the one being written in, the rest of the page must stop fading.
    fn drop(&mut self) {
        self.leave_page();
    }
}
