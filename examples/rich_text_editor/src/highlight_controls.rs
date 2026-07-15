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
                // Keep the find highlights anchored across document edits — cheap no-op when
                // nothing changed. (A real find banner does this from a document-change hook;
                // the per-frame poll is fine for a demo.)
                if find
                    .borrow_mut()
                    .as_mut()
                    .is_some_and(|fs| fs.refresh_if_stale())
                {
                    frame_req.set(true);
                }

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
            *syntax_sid.borrow_mut() =
                Some(self.doc.add_syntax_session(Arc::new(KeywordHighlighter)));
        }
        if spell_on.get() {
            *spell_sid.borrow_mut() =
                Some(self.doc.add_syntax_session(Arc::new(SpellCheckHighlighter)));
        }
        {
            let q = query.get();
            if !q.is_empty() {
                let mut fs =
                    FindSession::new(&self.doc, current_match_format(), other_match_format());
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
    use bastyde::text_document::{
        FlowElement, FlowElementSnapshot, FragmentContent, UnderlineStyle,
    };

    fn all_descendants(tree: &WidgetTree, id: WidgetId, out: &mut Vec<WidgetId>) {
        for child in tree.children(id) {
            out.push(child);
            all_descendants(tree, child, out);
        }
    }

    /// Every paint span across every block of the document, under the all-sessions view.
    fn all_paint_spans(doc: &TextDocument) -> Vec<bastyde::text_document::PaintHighlightSpan> {
        doc.snapshot_flow()
            .elements
            .into_iter()
            .flat_map(|e| match e {
                FlowElementSnapshot::Block(b) => b.paint_highlights,
                _ => Vec::new(),
            })
            .collect()
    }

    /// Any fragment carrying a foreground colour (the keyword tint), fully-merged view.
    fn has_keyword_color(doc: &TextDocument) -> bool {
        doc.flow().iter().any(|el| match el {
            FlowElement::Block(b) => b.display_fragments().iter().any(|f| {
                matches!(f, FragmentContent::Text { format, .. } if format.foreground_color.is_some())
            }),
            _ => false,
        })
    }

    #[test]
    fn highlight_controls_builds_and_lays_out() {
        let doc = TextDocument::new();
        doc.set_plain_text("hello world").unwrap();
        let mut tree = WidgetTree::new();
        let id = tree.add(HighlightControls::new(&doc));
        tree.layout(SizeProposal::exact(700.0, 48.0));
        assert!(tree.bounds(id).width > 0.0);
    }

    /// **Building the real widget applies its initial state** — the default Spell-on + the
    /// demo query "heading" — through `build()`'s own imperative apply, and both paint layers
    /// show at once. This drives the actual widget, not a hand-copy of its effect bodies.
    #[test]
    fn building_the_row_applies_and_composes_the_default_highlights() {
        let doc = TextDocument::new();
        // A misspelled word for Spell, and the demo query word for Search.
        doc.set_plain_text("This heading has a recieve typo")
            .unwrap();

        let mut tree = WidgetTree::new();
        tree.add(HighlightControls::new(&doc));
        tree.layout(SizeProposal::exact(900.0, 48.0));

        let spans = all_paint_spans(&doc);
        assert!(
            spans.iter().any(|s| s.background_color.is_some()),
            "the Search find session must box the query word — from build()'s initial apply"
        );
        assert!(
            spans
                .iter()
                .any(|s| s.underline_style == Some(UnderlineStyle::SpellCheckUnderline)),
            "the Spell session must underline `recieve` — AT THE SAME TIME (composition)"
        );
    }

    /// Toggling the **Syntax** checkbox in the live widget installs the keyword session on top
    /// of the defaults — the real Checkbox → ctx.effect → add_syntax_session chain.
    #[test]
    fn clicking_the_syntax_toggle_adds_a_keyword_layer() {
        let doc = TextDocument::new();
        doc.set_plain_text("let x = 42").unwrap();
        let mut tree = WidgetTree::new();
        let root = tree.add(HighlightControls::new(&doc));
        tree.layout(SizeProposal::exact(1100.0, 48.0));

        assert!(
            !has_keyword_color(&doc),
            "no keyword tint before toggling Syntax"
        );

        let mut nodes = Vec::new();
        all_descendants(&tree, root, &mut nodes);

        // One of the descendants is the Syntax checkbox; clicking it flips syntax_on -> the
        // effect installs KeywordHighlighter -> "let" gets a foreground colour.
        let mut applied = false;
        for id in nodes {
            tree.click(id);
            if has_keyword_color(&doc) {
                applied = true;
                break;
            }
        }
        assert!(
            applied,
            "clicking the Syntax checkbox must install the keyword session"
        );
    }

    /// The search field must remain mouse-reachable (it collapses to ~0 width without the
    /// MinSize floor). Regression guard restored from the pre-rewrite suite.
    #[test]
    fn search_field_is_clickable_by_position() {
        let doc = TextDocument::new();
        doc.set_plain_text("hello").unwrap();
        let mut tree = WidgetTree::new();
        let root = tree.add(HighlightControls::new(&doc));
        tree.layout(SizeProposal::exact(1100.0, 48.0));

        // The row's content is left-aligned; the 260px-floored search field sits after the two
        // checkboxes (around x=300 for this layout width). Probe inside it — before the MinSize
        // floor it collapsed to ~0 width and no point here hit anything (Tab-only).
        let b = tree.bounds(root);
        let probe = Point::new(b.x + 300.0, b.y + b.height / 2.0);
        let hit = tree
            .hit_test(probe)
            .expect("search field region must be hittable, not collapsed");
        tree.click(hit);
        assert!(
            tree.focused().is_some(),
            "clicking the search field should focus it"
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
