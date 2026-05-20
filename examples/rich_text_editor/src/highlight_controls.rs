//! Toolbar row that switches a live [`SyntaxHighlighter`] on the shared
//! [`TextDocument`].
//!
//! Demonstrates the document-level highlighter: a `SegmentedControl` picks
//! between Off / Search / Syntax / Spell-check, and (in Search mode) a
//! `SearchField` feeds a live query. Because the highlighter is attached to the
//! *document*, the result shows in both editor panes at once.
//!
//! Two `ctx.effect`s do the wiring:
//! * mode changes → [`TextDocument::set_syntax_highlighter`] (installs/clears).
//! * query changes → write the shared query + [`TextDocument::rehighlight`].
//!
//! `SyntaxHighlighter` is `Send + Sync`, so the search query cannot be a
//! (`Rc`-based) `Signal`; it lives in an `Arc<RwLock<String>>` shared between
//! this widget and the [`SearchHighlighter`].

use std::sync::{Arc, RwLock};

use bastyde::core::widget::WidgetPlacement;
use bastyde::prelude::*;
use bastyde::text_document::TextDocument;
use bastyde::widgets::{Divider, MinSize, SearchField, SegmentedControl, TextWidget, Toolbar};

use crate::highlighters::{KeywordHighlighter, SearchHighlighter, SpellCheckHighlighter};

// Segmented-control indices.
const MODE_OFF: usize = 0;
const MODE_SEARCH: usize = 1;
const MODE_SYNTAX: usize = 2;
const MODE_SPELL: usize = 3;

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
        let mode = ctx.signal(MODE_OFF);
        let query = ctx.signal(String::new());
        // Shared with the SearchHighlighter (Send + Sync — can't hold a Signal).
        let shared_query = Arc::new(RwLock::new(String::new()));
        // Highlight changes overlay the layout without an edit, so a bound
        // editor's frame-tick (which drains the change + repaints) is not
        // re-armed on its own. The editor's `on_change` callback is `Send +
        // Sync` and can't touch the non-`Send` frame-request handle, so the
        // wake has to come from here: pump one frame after each change.
        let frame_req = ctx.frame_request_handle();

        // ── mode → install / clear the highlighter on the document ──
        {
            let doc = self.doc.clone();
            let query = query.clone();
            let shared_query = shared_query.clone();
            let frame_req = frame_req.clone();
            ctx.effect(&mode, move |m| {
                match *m {
                    MODE_SEARCH => {
                        *shared_query.write().unwrap() = query.get();
                        doc.set_syntax_highlighter(Some(Arc::new(SearchHighlighter {
                            query: shared_query.clone(),
                        })));
                    }
                    MODE_SYNTAX => doc.set_syntax_highlighter(Some(Arc::new(KeywordHighlighter))),
                    MODE_SPELL => doc.set_syntax_highlighter(Some(Arc::new(SpellCheckHighlighter))),
                    _ => doc.set_syntax_highlighter(None),
                }
                frame_req.set(true);
            });
        }

        // ── query → push into shared cell + re-highlight (Search mode only) ──
        {
            let doc = self.doc.clone();
            let mode = mode.clone();
            let shared_query = shared_query.clone();
            let frame_req = frame_req.clone();
            ctx.effect(&query, move |q| {
                if mode.get() == MODE_SEARCH {
                    *shared_query.write().unwrap() = q.clone();
                    doc.rehighlight();
                    frame_req.set(true);
                }
            });
        }

        let segmented = SegmentedControl::new(
            vec![
                "Off".to_string(),
                "Search".to_string(),
                "Syntax".to_string(),
                "Spell".to_string(),
            ],
            mode.clone(),
        );

        // The SearchField's inner input is `MinSize::new(0, 0)`, so in a
        // Toolbar's HStack (intrinsic width, no stretch) it collapses to ~zero
        // width — the placeholder paints past the box but the clickable area is
        // gone (still Tab-reachable). Give it an explicit width floor.
        let search = MinSize::width(260.0)
            .child(SearchField::new(query.clone()).placeholder_literal("Find in document…"));

        let root = ctx.add(
            Toolbar::new()
                .label_literal("Highlighter")
                .child(TextWidget::new_literal("Highlighter:"))
                .child(segmented)
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

    #[test]
    fn highlight_controls_builds_and_lays_out() {
        let doc = TextDocument::new();
        doc.set_plain_text("hello world").unwrap();
        let mut tree = WidgetTree::new();
        let id = tree.add(HighlightControls::new(&doc));
        tree.layout(SizeProposal::exact(600.0, 48.0));
        assert!(tree.bounds(id).width > 0.0);
    }

    fn all_descendants(tree: &WidgetTree, id: WidgetId, out: &mut Vec<WidgetId>) {
        for child in tree.children(id) {
            out.push(child);
            all_descendants(tree, child, &mut *out);
        }
    }

    fn first_block_has_color(doc: &TextDocument) -> bool {
        use bastyde::text_document::{FlowElement, FragmentContent};
        match &doc.flow()[0] {
            FlowElement::Block(b) => b.fragments().iter().any(|f| {
                matches!(
                    f,
                    FragmentContent::Text { format, .. } if format.foreground_color.is_some()
                )
            }),
            _ => false,
        }
    }

    #[test]
    fn clicking_a_segment_applies_highlighting_and_focus() {
        let doc = TextDocument::new();
        doc.set_plain_text("let x = 42").unwrap();
        let mut tree = WidgetTree::new();
        let root = tree.add(HighlightControls::new(&doc));
        tree.layout(SizeProposal::exact(1100.0, 48.0));

        assert!(!first_block_has_color(&doc), "no highlight before interaction");

        let mut nodes = Vec::new();
        all_descendants(&tree, root, &mut nodes);

        let frame_flag = tree.frame_request_handle();

        // Click each descendant; one of them is the "Syntax" segment, whose
        // on_tap sets the mode signal -> effect installs KeywordHighlighter ->
        // the document re-highlights "let" with a foreground color.
        let mut applied_via_click = false;
        let mut got_focus = false;
        let mut requested_frame = false;
        for id in nodes {
            frame_flag.set(false);
            tree.click(id);
            if tree.focused().is_some() {
                got_focus = true;
            }
            if first_block_has_color(&doc) {
                applied_via_click = true;
                // The effect must pump a frame so a bound editor drains the
                // highlight change and repaints without waiting for scroll/focus.
                requested_frame = frame_flag.get();
                break;
            }
        }

        assert!(
            applied_via_click,
            "clicking the Syntax segment should highlight the document"
        );
        assert!(got_focus, "clicking a control should move focus into the row");
        assert!(
            requested_frame,
            "switching highlighter must request a frame so editors repaint immediately"
        );
    }

    #[test]
    fn search_field_is_clickable_by_position() {
        let doc = TextDocument::new();
        doc.set_plain_text("hello").unwrap();
        let mut tree = WidgetTree::new();
        let root = tree.add(HighlightControls::new(&doc));
        tree.layout(SizeProposal::exact(1100.0, 48.0));

        // The search field is the rightmost element; probe inside its
        // 260px box. Before the MinSize floor it collapsed to ~0 width and
        // this point hit nothing (mouse-unreachable, Tab-only).
        let b = tree.bounds(root);
        let probe = Point::new(b.x + b.width - 130.0, b.y + b.height / 2.0);
        let hit = tree
            .hit_test(probe)
            .expect("search field region must be hittable, not collapsed");
        tree.click(hit);
        assert!(
            tree.focused().is_some(),
            "clicking the search field should focus it"
        );
    }
}
