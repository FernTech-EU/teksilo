// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Toolbar row that layers **composable** highlight sessions onto the shared
//! `TextDocument`.
//!
//! This is the demo for text-document's highlight **session registry**: a document holds any
//! number of highlight layers at once, so Syntax, Spell and Search are three independent
//! toggles here — turn all three on and keywords go bold-purple, misspellings gain a red wavy
//! underline, *and* the search term is boxed, simultaneously. (The old single-highlighter slot
//! could show only one at a time.)
//!
//! * **Syntax** / **Spell** — `SyntaxHighlighter` callbacks, each installed with
//!   [`TextDocument::add_syntax_session`] and retired with `remove_session`.
//! * **Search** — a [`FindSession`], a *range* session driven by text-document's own matcher
//!   (`find_all`), so it agrees with a project-wide search. The current match is boxed more
//!   strongly than the rest.
//!
//! Because every session lives on the *document*, the result shows in both editor panes at
//! once — and a per-view `HighlightMask` (not exercised here) could give each pane its own.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bastyde::core::widget::WidgetPlacement;
use bastyde::prelude::*;
use bastyde::text_document::{Color, FindOptions, HighlightFormat, SessionId, TextDocument};
use bastyde::widgets::rich_text::FindSession;
use bastyde::widgets::{Checkbox, Divider, MinSize, SearchField, TextWidget, Toolbar};

use crate::highlighters::{KeywordHighlighter, SpellCheckHighlighter};

/// Quiet period after the last search keystroke before the (full-document) re-search runs.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(120);

/// The current search match — a strong orange box.
fn current_match_format() -> HighlightFormat {
    HighlightFormat {
        background_color: Some(Color::rgba(255, 140, 0, 210)),
        ..Default::default()
    }
}

/// The other search matches — the classic translucent yellow.
fn other_match_format() -> HighlightFormat {
    HighlightFormat {
        background_color: Some(Color::rgba(255, 214, 0, 150)),
        ..Default::default()
    }
}

/// Debounce check: `true` (and disarms) once `now` reaches the pending deadline. The deadline
/// is *latest-wins* — each keystroke overwrites it, so the timer slides forward while typing
/// continues and only elapses after a real pause.
fn debounce_elapsed(due: &Cell<Option<Instant>>, now: Instant) -> bool {
    match due.get() {
        Some(deadline) if now >= deadline => {
            due.set(None);
            true
        }
        _ => false,
    }
}

/// Highlighter toolbar row, bound to a shared [`TextDocument`].
pub struct HighlightControls {
    doc: TextDocument,
    root: Option<WidgetId>,
}

impl HighlightControls {
    pub fn new(doc: &TextDocument) -> Self {
        Self {
            doc: doc.clone(),
            root: None,
        }
    }
}

impl std::fmt::Debug for HighlightControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HighlightControls").finish_non_exhaustive()
    }
}

impl Widget for HighlightControls {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // The showcase opens with Spell on and a demo query already boxed, so the composition
        // is visible without a click — two paint layers (yellow find boxes + red spell
        // squiggles) at once. Syntax is off by default (keyword-tinting prose is noise).
        let syntax_on = ctx.signal(false);
        let spell_on = ctx.signal(true);
        let query = ctx.signal(String::from("heading"));

        // A highlight change overlays the layout without an edit, so a bound editor's
        // frame-tick isn't re-armed on its own — pump one frame after each change.
        let frame_req = ctx.frame_request_handle();
        let search_due: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));

        // Each toggle owns its session id (Syntax / Spell) or the whole FindSession (Search),
        // kept across effect firings so the toggle-off path can retire exactly what it added.
        let syntax_sid: Rc<RefCell<Option<SessionId>>> = Rc::new(RefCell::new(None));
        let spell_sid: Rc<RefCell<Option<SessionId>>> = Rc::new(RefCell::new(None));
        let find: Rc<RefCell<Option<FindSession>>> = Rc::new(RefCell::new(None));

        // ── Syntax toggle → add / remove a keyword syntax session ──
        {
            let doc = self.doc.clone();
            let sid = syntax_sid.clone();
            let frame_req = frame_req.clone();
            ctx.effect(&syntax_on, move |on| {
                let mut slot = sid.borrow_mut();
                if *on && slot.is_none() {
                    *slot = Some(doc.add_syntax_session(Arc::new(KeywordHighlighter)));
                } else if !*on && let Some(id) = slot.take() {
                    doc.remove_session(id);
                }
                frame_req.set(true);
            });
        }

        // ── Spell toggle → add / remove a spell-check syntax session (composes with Syntax) ──
        {
            let doc = self.doc.clone();
            let sid = spell_sid.clone();
            let frame_req = frame_req.clone();
            ctx.effect(&spell_on, move |on| {
                let mut slot = sid.borrow_mut();
                if *on && slot.is_none() {
                    *slot = Some(doc.add_syntax_session(Arc::new(SpellCheckHighlighter)));
                } else if !*on && let Some(id) = slot.take() {
                    doc.remove_session(id);
                }
                frame_req.set(true);
            });
        }

        // ── query → arm the debounce (the re-search itself runs on the tick below) ──
        {
            let search_due = search_due.clone();
            let frame_req = frame_req.clone();
            ctx.effect(&query, move |_q| {
                search_due.set(Some(Instant::now() + SEARCH_DEBOUNCE));
                frame_req.set(true);
            });
        }

        // ── frame tick → run the debounced search when it elapses ──
        {
            let doc = self.doc.clone();
            let query = query.clone();
            let find = find.clone();
            let search_due = search_due.clone();
            let frame_req = frame_req.clone();
            let tick = ctx.frame_tick();
            ctx.effect(&tick, move |_delta| {
                if search_due.get().is_none() {
                    return;
                }
                if debounce_elapsed(&search_due, Instant::now()) {
                    let q = query.get();
                    let mut slot = find.borrow_mut();
                    if q.is_empty() {
                        // Drop the FindSession — its `Drop` retires the range session, so the
                        // document is left with no search layer at all.
                        *slot = None;
                    } else {
                        let fs = slot.get_or_insert_with(|| {
                            FindSession::new(&doc, current_match_format(), other_match_format())
                        });
                        fs.set_query(&q, &FindOptions::default());
                    }
                }
                // Re-arm to drain + repaint after the change just queued, or keep polling.
                frame_req.set(true);
            });
        }

        // Apply the initial toggle / query state now — `ctx.effect` fires only on *change*,
        // so a default-checked box or a pre-filled query would otherwise sit inert until the
        // user touched it. The editors pick these sessions up on their first layout.
        if syntax_on.get() {
            *syntax_sid.borrow_mut() = Some(self.doc.add_syntax_session(Arc::new(KeywordHighlighter)));
        }
        if spell_on.get() {
            *spell_sid.borrow_mut() =
                Some(self.doc.add_syntax_session(Arc::new(SpellCheckHighlighter)));
        }
        {
            let q = query.get();
            if !q.is_empty() {
                let mut fs = FindSession::new(&self.doc, current_match_format(), other_match_format());
                fs.set_query(&q, &FindOptions::default());
                *find.borrow_mut() = Some(fs);
            }
        }
        frame_req.set(true);

        // The SearchField's inner input is `MinSize::new(0, 0)`; give it an explicit floor so
        // it doesn't collapse in the Toolbar's intrinsic-width HStack.
        let search = MinSize::width(260.0)
            .child(SearchField::new(query.clone()).placeholder(lit!("Find in document…")));

        let root = ctx.add(
            Toolbar::new()
                .label(lit!("Highlighter"))
                .child(TextWidget::new(lit!("Highlight:")))
                .child(Checkbox::new(syntax_on.clone()).label(lit!("Syntax")))
                .child(Checkbox::new(spell_on.clone()).label(lit!("Spell")))
                .child(Divider::vertical())
                .child(search),
        );
        self.root = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        if let Some(root) = self.root
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return size.into();
        }
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde::core::widget_tree::WidgetTree;
    use bastyde::text_document::{FlowElement, FlowElementSnapshot, FragmentContent, UnderlineStyle};

    #[test]
    fn highlight_controls_builds_and_lays_out() {
        let doc = TextDocument::new();
        doc.set_plain_text("hello world").unwrap();
        let mut tree = WidgetTree::new();
        let id = tree.add(HighlightControls::new(&doc));
        tree.layout(SizeProposal::exact(700.0, 48.0));
        assert!(tree.bounds(id).width > 0.0);
    }

    /// Does any fragment of the first block carry a foreground colour (a keyword tint)?
    fn has_keyword_color(doc: &TextDocument) -> bool {
        match &doc.flow()[0] {
            FlowElement::Block(b) => b.display_fragments().iter().any(|f| {
                matches!(f, FragmentContent::Text { format, .. } if format.foreground_color.is_some())
            }),
            _ => false,
        }
    }

    /// Does the first block carry a spell-check underline? Reads the fully-merged
    /// `display_fragments` rather than the paint overlay, because when a metric session (the
    /// keyword bold) coexists the whole snapshot goes down the reshape path — every span,
    /// including this paint-only underline, is baked into the fragments and the paint overlay
    /// is empty. That is exactly the composition this test is about.
    fn has_spell_underline(doc: &TextDocument) -> bool {
        match &doc.flow()[0] {
            FlowElement::Block(b) => b.display_fragments().iter().any(|f| {
                matches!(
                    f,
                    FragmentContent::Text { format, .. }
                        if format.underline_style == Some(UnderlineStyle::SpellCheckUnderline)
                )
            }),
            _ => false,
        }
    }

    /// **The registry's headline:** Syntax and Spell compose — both layers show at once, which
    /// the old single-highlighter slot could never do.
    #[test]
    fn syntax_and_spell_compose_on_one_document() {
        // "recieve" is misspelled; "let"/"fn" are keywords.
        let doc = TextDocument::new();
        doc.set_plain_text("let x = 42; recieve fn").unwrap();

        // Drive the two toggle signals directly (the effects install the sessions).
        let syntax_on = Signal::new(false);
        let spell_on = Signal::new(false);

        // Reproduce the two effects the widget wires (headless — no tree needed for these).
        {
            let doc = doc.clone();
            let sid: Rc<RefCell<Option<SessionId>>> = Rc::new(RefCell::new(None));
            let s = syntax_on.clone();
            s.set(true);
            // Apply once, imperatively, mirroring the effect body:
            *sid.borrow_mut() = Some(doc.add_syntax_session(Arc::new(KeywordHighlighter)));
        }
        {
            let doc = doc.clone();
            spell_on.set(true);
            doc.add_syntax_session(Arc::new(SpellCheckHighlighter));
        }

        assert!(has_keyword_color(&doc), "the keyword session must colour `let`/`fn`");
        assert!(
            has_spell_underline(&doc),
            "the spell session must underline `recieve` — AT THE SAME TIME as the keywords"
        );
    }

    #[test]
    fn debounce_slides_forward_not_throttle() {
        let t0 = Instant::now();
        let due = Cell::new(None);
        let ms = |n| t0 + Duration::from_millis(n);

        due.set(Some(ms(0) + SEARCH_DEBOUNCE));
        assert!(!debounce_elapsed(&due, ms(60)));
        due.set(Some(ms(60) + SEARCH_DEBOUNCE));
        assert!(!debounce_elapsed(&due, ms(130)));
        assert!(debounce_elapsed(&due, ms(181)));
        assert!(due.get().is_none());
        assert!(!debounce_elapsed(&due, ms(500)));
    }

    /// A find session over the document highlights the query and distinguishes the current
    /// match — the Search half of the row, exercised directly.
    #[test]
    fn a_find_session_boxes_the_matches() {
        let doc = TextDocument::new();
        doc.set_plain_text("alpha beta alpha").unwrap();
        let mut fs = FindSession::new(&doc, current_match_format(), other_match_format());
        fs.set_query("alpha", &FindOptions::default());
        assert_eq!(fs.match_count(), 2);

        let spans = match &doc.snapshot_flow().elements[0] {
            FlowElementSnapshot::Block(b) => b.paint_highlights.clone(),
            _ => panic!("block"),
        };
        assert!(
            spans
                .iter()
                .any(|s| s.background_color == current_match_format().background_color),
            "the current match is boxed in the strong colour"
        );
        assert!(
            spans
                .iter()
                .any(|s| s.background_color == other_match_format().background_color),
            "the other match is boxed in the subtle colour"
        );
    }
}
