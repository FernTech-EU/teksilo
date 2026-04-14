//! A non-virtualized dynamic collection widget.
//!
//! `Repeater` creates one child widget per item in a `ListModel<T>`.
//! When the model changes, the entire child subtree is rebuilt. This is
//! suitable for small collections (<100 items) where full rebuild is
//! acceptable (toolbar buttons, chapter lists, tag lists).
//!
//! For large collections, use `ListView` instead (Phase C).

use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

use fern_data::ListModel;

use crate::primitives::VStack;

/// A non-virtualized dynamic collection that creates one child widget
/// per item in a `ListModel<T>`.
///
/// ```ignore
/// Repeater::new(model, |index, item| {
///     Box::new(TextWidget::new_literal(&item.title))
/// })
/// .spacing(8.0)
/// ```
pub struct Repeater<T: 'static> {
    model: ListModel<T>,
    factory: Rc<dyn Fn(usize, &T) -> Box<dyn Widget>>,
    spacing: f32,
    // Internal state (set during build)
    container_id: Option<WidgetId>,
}

impl<T: 'static> Repeater<T> {
    /// Create a new Repeater backed by a `ListModel<T>`.
    ///
    /// The `factory` closure receives `(index, &item)` and returns a boxed widget
    /// for that item.
    pub fn new(
        model: ListModel<T>,
        factory: impl Fn(usize, &T) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self {
            model,
            factory: Rc::new(factory),
            spacing: 0.0,
            container_id: None,
        }
    }

    /// Set the spacing between items (default 0.0).
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }
}

impl<T: 'static> std::fmt::Debug for Repeater<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repeater")
            .field("item_count", &self.model.len())
            .field("spacing", &self.spacing)
            .finish()
    }
}

impl<T: 'static> Widget for Repeater<T> {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        // Create a version counter signal that triggers rebuild when incremented.
        let version = ctx.signal(0_u64);
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Observe model changes — increment version on any DataChange.
        let version_for_observer = version.clone();
        let current_version = Rc::new(Cell::new(0_u64));
        let handle = self.model.observe_changes(move |_change| {
            let next = current_version.get() + 1;
            current_version.set(next);
            version_for_observer.set(next);
        });
        ctx.own_handle(handle);

        // Build child widgets from the current model state.
        let mut container = VStack::new().spacing(self.spacing);

        let count = self.model.len();
        for i in 0..count {
            let factory = &self.factory;
            if let Some(widget) = self.model.with_item(i, |item| factory(i, item)) {
                let child_id = ctx.add_boxed(widget);
                container = container.add_child(child_id);
            }
        }

        let root = ctx.add(container);
        self.container_id = Some(root);
        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.container_id
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
        builder.set_role(fern_core::accesskit::Role::List);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.container_id.into_iter().collect()
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
    fn creates_children_from_model() {
        let model = ListModel::from_vec(vec!["a", "b", "c"]);
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(Repeater::new(model.clone(), |_i, _item| {
            Box::new(FixedLeaf(100.0, 30.0))
        }));

        tree.layout(SizeProposal::exact(200.0, 400.0));

        // Repeater -> VStack -> 3 FixedLeaf children
        let repeater_children = tree.children(repeater_id);
        assert_eq!(repeater_children.len(), 1); // The VStack container
        let vstack_children = tree.children(repeater_children[0]);
        assert_eq!(vstack_children.len(), 3);
    }

    #[test]
    fn push_triggers_rebuild() {
        let model = ListModel::from_vec(vec!["a", "b"]);
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(Repeater::new(model.clone(), |_i, _item| {
            Box::new(FixedLeaf(100.0, 30.0))
        }));

        tree.layout(SizeProposal::exact(200.0, 400.0));

        let vstack_id = tree.children(repeater_id)[0];
        assert_eq!(tree.children(vstack_id).len(), 2);

        // Push a new item
        model.push("c");

        // Layout triggers process_state_changes which rebuilds the Repeater
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let vstack_id = tree.children(repeater_id)[0];
        assert_eq!(tree.children(vstack_id).len(), 3);
    }

    #[test]
    fn remove_triggers_rebuild() {
        let model = ListModel::from_vec(vec!["a", "b", "c"]);
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(Repeater::new(model.clone(), |_i, _item| {
            Box::new(FixedLeaf(100.0, 30.0))
        }));

        tree.layout(SizeProposal::exact(200.0, 400.0));

        let vstack_id = tree.children(repeater_id)[0];
        assert_eq!(tree.children(vstack_id).len(), 3);

        model.remove(1);
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let vstack_id = tree.children(repeater_id)[0];
        assert_eq!(tree.children(vstack_id).len(), 2);
    }

    #[test]
    fn empty_model_creates_no_children() {
        let model: ListModel<&str> = ListModel::new();
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(Repeater::new(model, |_i, _item| {
            Box::new(FixedLeaf(100.0, 30.0))
        }));

        tree.layout(SizeProposal::exact(200.0, 400.0));

        let vstack_id = tree.children(repeater_id)[0];
        assert_eq!(tree.children(vstack_id).len(), 0);
    }

    #[test]
    fn factory_receives_correct_index_and_item() {
        let model = ListModel::from_vec(vec![10.0_f32, 20.0, 30.0]);
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(Repeater::new(model.clone(), |i, item| {
            // Width encodes the index, height encodes the item value
            Box::new(FixedLeaf(i as f32, *item))
        }));

        tree.layout(SizeProposal::exact(200.0, 400.0));

        let vstack_id = tree.children(repeater_id)[0];
        let children = tree.children(vstack_id);
        assert_eq!(children.len(), 3);

        // Each child's bounds.height should reflect the item value (10, 20, 30)
        // because FixedLeaf uses item value as height.
        let h0 = tree.bounds(children[0]).height;
        let h1 = tree.bounds(children[1]).height;
        let h2 = tree.bounds(children[2]).height;
        assert!((h0 - 10.0).abs() < 0.01);
        assert!((h1 - 20.0).abs() < 0.01);
        assert!((h2 - 30.0).abs() < 0.01);
    }

    #[test]
    fn spacing_is_applied() {
        let model = ListModel::from_vec(vec!["a", "b", "c"]);
        let mut tree = WidgetTree::new();

        let repeater_id = tree
            .add(Repeater::new(model, |_i, _item| Box::new(FixedLeaf(100.0, 20.0))).spacing(10.0));

        tree.layout(SizeProposal::exact(200.0, 400.0));

        // The VStack container fills its parent, but its children should be
        // positioned with spacing between them.
        let vstack_id = tree.children(repeater_id)[0];
        let children = tree.children(vstack_id);
        assert_eq!(children.len(), 3);

        // Child 0 starts at top
        let y0 = tree.bounds(children[0]).y;
        let y1 = tree.bounds(children[1]).y;
        let y2 = tree.bounds(children[2]).y;

        // Each child is 20px tall with 10px spacing between them
        assert!((y1 - y0 - 30.0).abs() < 0.01); // 20 (height) + 10 (spacing)
        assert!((y2 - y1 - 30.0).abs() < 0.01);
    }

    #[test]
    fn replace_all_triggers_rebuild() {
        let model = ListModel::from_vec(vec!["a", "b"]);
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(Repeater::new(model.clone(), |_i, _item| {
            Box::new(FixedLeaf(100.0, 30.0))
        }));

        tree.layout(SizeProposal::exact(200.0, 400.0));

        let vstack_id = tree.children(repeater_id)[0];
        assert_eq!(tree.children(vstack_id).len(), 2);

        model.replace_all(vec!["x", "y", "z", "w"]);
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let vstack_id = tree.children(repeater_id)[0];
        assert_eq!(tree.children(vstack_id).len(), 4);
    }
}
