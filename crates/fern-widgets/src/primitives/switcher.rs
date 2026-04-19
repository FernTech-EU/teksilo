use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

use crate::primitives::ZStack;

/// A container that shows exactly one child at a time, driven by a
/// `Signal<usize>` index. Internally a ZStack where each child has a
/// `visible_when` binding derived from `selected.map(|i| i == index)`.
///
/// ```ignore
/// let page = Signal::new(0_usize);
/// Switcher::new(page.clone())
///     .child(TextWidget::new_literal("Page 0"))
///     .child(TextWidget::new_literal("Page 1"))
///     .child(TextWidget::new_literal("Page 2"))
/// ```
pub struct Switcher {
    selected: Signal<usize>,
    deferred_children: Vec<Box<dyn Widget>>,
    root_child_id: Option<WidgetId>,
    /// Optional external buffer populated during `build()` with each
    /// child's `WidgetId` in insertion order. Callers that need to
    /// reference the pages later — e.g. `TabWidget` wiring the
    /// Tab → TabPanel accessibility relation — install a shared
    /// `Rc<RefCell<Vec<_>>>` via `capture_child_ids_into` and read
    /// it back after `ctx.add(switcher)` returns.
    child_ids_out: Option<Rc<RefCell<Vec<WidgetId>>>>,
}

impl Switcher {
    pub fn new(selected: Signal<usize>) -> Self {
        Self {
            selected,
            deferred_children: Vec::new(),
            root_child_id: None,
            child_ids_out: None,
        }
    }

    /// Capture each child's `WidgetId` into an externally owned
    /// buffer during `build()`. Use when the caller needs to
    /// reference the pages after they're added to the arena — e.g.
    /// for accessibility relations like Tab → TabPanel, where the
    /// tab must publish `push_controlled(panel_id)` but the
    /// `WidgetId` only exists after Switcher's `build()` runs.
    ///
    /// The buffer is cleared and repopulated on every rebuild.
    pub fn capture_child_ids_into(mut self, out: Rc<RefCell<Vec<WidgetId>>>) -> Self {
        self.child_ids_out = Some(out);
        self
    }

    /// Add a child page.
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.deferred_children.push(Box::new(widget));
        self
    }

    /// Add a pre-boxed child page.
    pub fn child_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.deferred_children.push(widget);
        self
    }

    /// Add multiple child pages from an iterator.
    pub fn children(mut self, iter: impl IntoIterator<Item = impl Widget + 'static>) -> Self {
        for widget in iter {
            self.deferred_children.push(Box::new(widget));
        }
        self
    }
}

impl std::fmt::Debug for Switcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Switcher")
            .field("num_children", &self.deferred_children.len())
            .finish()
    }
}

impl Widget for Switcher {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        let children = std::mem::take(&mut self.deferred_children);
        if let Some(ref out) = self.child_ids_out {
            out.borrow_mut().clear();
        }

        // Add each child to the tree and bind visibility to the selected index
        let mut zstack = ZStack::new();
        for (i, child_widget) in children.into_iter().enumerate() {
            let child_id = ctx.add_boxed(child_widget);
            if let Some(ref out) = self.child_ids_out {
                out.borrow_mut().push(child_id);
            }
            let idx = i;
            let vis = self.selected.map(move |s| *s == idx);
            ctx.visible_when(child_id, vis);
            zstack = zstack.add_child(child_id);
        }

        let root = ctx.add(zstack);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn switches_active_child_on_state_change() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new();

        let a = tree.add(FixedLeaf(100.0, 40.0));
        let b = tree.add(FixedLeaf(80.0, 30.0));

        let _zstack = tree.add(ZStack::new().add_child(a).add_child(b));

        tree.visible_when(a, selected.map(move |idx| *idx == 0));
        tree.visible_when(b, selected.map(move |idx| *idx == 1));

        tree.layout(SizeProposal::exact(200.0, 200.0));

        assert!(tree.is_visible(a));
        assert!(!tree.is_visible(b));

        selected.set(1);
        tree.layout(SizeProposal::exact(200.0, 200.0));

        assert!(!tree.is_visible(a));
        assert!(tree.is_visible(b));
    }

    #[test]
    fn all_dormant_when_invalid_index() {
        let selected = Signal::new(99_usize);
        let mut tree = WidgetTree::new();

        let a = tree.add(FixedLeaf(100.0, 40.0));
        let b = tree.add(FixedLeaf(80.0, 30.0));

        let _zstack = tree.add(ZStack::new().add_child(a).add_child(b));
        tree.visible_when(a, selected.map(move |idx| *idx == 0));
        tree.visible_when(b, selected.map(move |idx| *idx == 1));

        tree.layout(SizeProposal::exact(200.0, 200.0));
        assert!(!tree.is_visible(a));
        assert!(!tree.is_visible(b));
    }

    #[test]
    fn switcher_builds_and_lays_out() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new();

        let switcher_id = tree.add(
            Switcher::new(selected.clone())
                .child(FixedLeaf(100.0, 40.0))
                .child(FixedLeaf(80.0, 30.0))
                .child(FixedLeaf(60.0, 20.0)),
        );

        tree.layout(SizeProposal::exact(200.0, 200.0));

        assert!(tree.is_visible(switcher_id));
        let bounds = tree.bounds(switcher_id);
        assert!(bounds.width > 0.0);
        assert!(bounds.height > 0.0);
    }

    #[test]
    fn switcher_with_children_iterator() {
        let selected = Signal::new(2_usize);
        let mut tree = WidgetTree::new();

        let pages: Vec<FixedLeaf> = vec![
            FixedLeaf(100.0, 40.0),
            FixedLeaf(80.0, 30.0),
            FixedLeaf(60.0, 20.0),
        ];

        let _switcher_id = tree.add(Switcher::new(selected).children(pages));

        tree.layout(SizeProposal::exact(200.0, 200.0));
    }
}
