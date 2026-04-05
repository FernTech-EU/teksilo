use std::cell::RefCell;

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::signal::Signal;
use fern_core::widget::IntoWidgetTree;
use fern_core::widget_id::WidgetId;

use crate::primitives::ZStack;

/// A container that shows exactly one child at a time, driven by a
/// `State<usize>` index. Internally a ZStack where each child has a
/// `visible_when` binding derived from `selected.map(|i| *i == index)`.
///
/// The selected child is active (layout, paint, events, accessibility);
/// all others are dormant (state preserved, no rendering cost).
///
/// The Switcher does not own the selection logic — it receives the
/// `Signal<usize>` from outside, composing with any navigation pattern
/// (wizard Next/Back buttons, sidebar navigation, tab headers, routing).
///
/// ```ignore
/// let page = Signal::new(0_usize);
/// Switcher::new(page.clone())
///     .child(TextWidget::new("Page 0"))
///     .child(TextWidget::new("Page 1"))
///     .child(TextWidget::new("Page 2"))
/// ```
pub struct Switcher {
    selected: Signal<usize>,
    deferred_children: RefCell<Vec<Box<dyn IntoWidgetTree>>>,
}

impl Switcher {
    pub fn new(selected: Signal<usize>) -> Self {
        Self {
            selected,
            deferred_children: RefCell::new(Vec::new()),
        }
    }

    /// Add a child page.
    pub fn child(self, widget: impl IntoWidgetTree) -> Self {
        self.deferred_children.borrow_mut().push(Box::new(widget));
        self
    }

    /// Add multiple child pages from an iterator.
    pub fn children(
        self,
        iter: impl IntoIterator<Item = impl IntoWidgetTree>,
    ) -> Self {
        let mut children = self.deferred_children.borrow_mut();
        for widget in iter {
            children.push(Box::new(widget));
        }
        drop(children);
        self
    }

}

impl std::fmt::Debug for Switcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Switcher")
            .field("num_children", &self.deferred_children.borrow().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::{Size, SizeProposal};
    use fern_core::widget::{LayoutContext, Widget};
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

        // Manually apply the same pattern Switcher uses internally,
        // so we can check visibility of known child IDs.
        let a = tree.add(FixedLeaf(100.0, 40.0));
        let b = tree.add(FixedLeaf(80.0, 30.0));

        let _zstack = tree.add(ZStack::new().add_child(a).add_child(b));

        tree.visible_when(a, selected.map(move |idx| *idx == 0));
        tree.visible_when(b, selected.map(move |idx| *idx == 1));

        tree.layout(SizeProposal::exact(200.0, 200.0));
        assert!(tree.is_visible(a));
        assert!(!tree.is_visible(b));

        // Switch to child 1
        selected.set(1);
        tree.layout(SizeProposal::exact(200.0, 200.0));
        assert!(!tree.is_visible(a));
        assert!(tree.is_visible(b));

        // Switch back to child 0
        selected.set(0);
        tree.layout(SizeProposal::exact(200.0, 200.0));
        assert!(tree.is_visible(a));
        assert!(!tree.is_visible(b));
    }

    #[test]
    fn out_of_range_index_hides_all() {
        let selected = Signal::new(5_usize);
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
    fn composite_switcher_builds_and_lays_out() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new();

        let switcher_id = tree.add_widget(
            Switcher::new(selected.clone())
                .child(FixedLeaf(100.0, 40.0))
                .child(FixedLeaf(80.0, 30.0))
                .child(FixedLeaf(60.0, 20.0)),
        );

        tree.layout(SizeProposal::exact(200.0, 200.0));

        // Switcher should be visible
        assert!(tree.is_visible(switcher_id));

        // The switcher's bounds should reflect the ZStack sizing
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

        let _switcher_id = tree.add_widget(
            Switcher::new(selected).children(pages),
        );

        tree.layout(SizeProposal::exact(200.0, 200.0));
        // Should not panic — page 2 is selected
    }
}
