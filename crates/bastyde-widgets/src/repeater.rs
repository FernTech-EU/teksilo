// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Repeater — non-virtualized dynamic widget list driven by a `ListModel<T>`.
//!
//! `Repeater` creates one child widget per item in a [`ListModel<T>`](bastyde_data::ListModel)
//! using a caller-supplied factory closure, arranging them along one axis
//! ([`RepeaterLayout::Vertical`] by default) or as a wrapping flow
//! ([`RepeaterLayout::Wrap`]). It is **not virtualized**: every item has a live
//! widget at all times. That is a deliberate trade — it is what lets the
//! children keep real, stateful widgets (text editors, forms) mounted, which a
//! virtualizing [`ListView`](crate::ListView) cannot do because it recycles
//! off-screen rows.
//!
//! # [`Repeater::new`] — reconciling (the default)
//!
//! The factory takes `&item` and each child widget is **reused across model
//! changes**. When the model mutates, `Repeater` reads the
//! [`DataChange`] it emits and applies the *minimal* edit to its child set: an
//! insert builds one new widget, a remove reaps one, a move reorders, an
//! in-place update rebuilds only that item — every other child keeps its
//! existing widget, and with it its focus, selection, caret, scroll offset,
//! in-flight text edit, and undo history.
//!
//! This makes `Repeater` a fit for a **stack of editors** — e.g. a document
//! rendered as a column of [`RichTextEditor`](crate::rich_text::RichTextEditor)s,
//! one per scene/block:
//!
//! ```rust,ignore
//! Repeater::new(scenes, |scene| {
//!     Box::new(RichTextEditor::editor(scene.document()))
//! })
//! ```
//!
//! Inserting, deleting, or reordering a scene costs one widget's worth of work
//! instead of reshaping every editor in the document, and the editor the user is
//! typing in keeps its caret. Because the factory has no index, position shifts
//! are safe by construction: reuse can never leave a widget showing content
//! derived from a stale position. The one requirement is that an item's
//! *content* only changes through the model (via `set`/`replace_all`), which is
//! always true for a `ListModel`.
//!
//! ```rust
//! # use bastyde_widgets::Repeater;
//! # use bastyde_widgets::primitives::TextWidget;
//! # use bastyde_data::ListModel;
//! # use bastyde_i18n::lit;
//! let model: ListModel<u32> = ListModel::from_vec(vec![1, 2, 3]);
//! let _w = Repeater::new(model, |item| {
//!     Box::new(TextWidget::new(lit!(format!("item {item}"))))
//! })
//! .spacing(4.0);
//! ```
//!
//! # [`Repeater::indexed`] — full rebuild (position-in-content)
//!
//! When the content genuinely depends on position — a numbered list, "N of M",
//! a ranking that must renumber on reorder — use [`indexed`](Repeater::indexed).
//! Its factory takes `(index, &item)`, and on **any** model change the whole
//! child subtree is torn down and rebuilt, so the index every widget shows is
//! always current. This is the right pick for cheap, stateless, position-derived
//! rows; it does **not** preserve per-child state across changes (that is the
//! reason to prefer [`new`](Repeater::new) whenever the index isn't content).
//!
//! # Accessibility
//!
//! `Repeater` imposes **no** accessibility semantics of its own — it is a
//! transparent layout wrapper, so its children surface directly into the
//! surrounding AT subtree and their own roles decide how they read. When the
//! children genuinely form a named list, menu, or toolbar, opt in with the
//! standard builder overrides that every widget supports — these stay
//! locale-reactive:
//!
//! ```rust,ignore
//! use bastyde_core::accesskit::Role;
//! Repeater::new(tags, factory)
//!     .access_role(Role::List)
//!     .access_label(tr!(tags()))
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};

use bastyde_core::binding::BindingLevel;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

use bastyde_data::{DataChange, ListModel};

use crate::primitives::{HStack, VStack, Wrap};

/// How a [`Repeater`] arranges its item widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeaterLayout {
    /// A vertical column, top to bottom (default). Gap = [`Repeater::spacing`].
    #[default]
    Vertical,
    /// A horizontal row, leading to trailing (RTL-aware via `HStack`).
    /// Gap = [`Repeater::spacing`].
    Horizontal,
    /// A horizontal flow that wraps to the next line when items exceed the
    /// available width — chip rows, badge lists. [`Repeater::spacing`] is the
    /// inter-item gap, [`Repeater::line_spacing`] the inter-line gap.
    Wrap,
}

/// The caller-supplied widget factory, in one of the two build-mode shapes.
enum RepeaterFactory<T> {
    /// `&item` — used by [`Repeater::new`]; position-independent, so the widget
    /// can be reused when items shift (reconciling mode).
    Keyless(Rc<dyn Fn(&T) -> Box<dyn Widget>>),
    /// `(index, &item)` — used by [`Repeater::indexed`]; the whole subtree is
    /// rebuilt on every change so the index is always current.
    Indexed(Rc<dyn Fn(usize, &T) -> Box<dyn Widget>>),
}

/// One entry in the reconciliation table (reconciling mode only). Parallel to
/// the model: `slots[i]` describes the widget for model item `i`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemSlot {
    /// No widget yet — `build()` will construct one from the model item.
    Vacant,
    /// Reuse this already-mounted widget; its state is preserved.
    Filled(WidgetId),
}

/// A non-virtualized dynamic collection that creates one child widget per item in a `ListModel<T>`.
///
/// See the [module-level docs](self) for the two build modes, layout options,
/// and accessibility guidance.
pub struct Repeater<T: 'static> {
    model: ListModel<T>,
    factory: RepeaterFactory<T>,
    layout: RepeaterLayout,
    spacing: f32,
    line_spacing: f32,
    /// Reconciliation table — `Some` in reconciling mode ([`Repeater::new`]),
    /// `None` in full-rebuild mode ([`Repeater::indexed`]). Shared with the
    /// model-change observer, which applies each [`DataChange`] to it so the
    /// next `build()` can reuse surviving widgets. `Rc<RefCell<…>>` because the
    /// observer runs outside `build()`; the handle persists across rebuilds.
    slots: Option<Rc<RefCell<Vec<ItemSlot>>>>,
    // Internal state (set during build)
    container_id: Option<WidgetId>,
}

impl<T: 'static> Repeater<T> {
    /// Create a Repeater in **reconciling** mode (the default): item widgets are
    /// reused across model changes, so each child keeps its state (focus, caret,
    /// selection, scroll, undo history) when siblings are inserted, removed, or
    /// reordered.
    ///
    /// The `factory` receives `&item` only — it must not depend on the item's
    /// position, which is what makes reuse safe when items shift. This is the
    /// mode for a stack of stateful widgets such as `RichTextEditor`s. If the
    /// content genuinely depends on position (a numbered list), use
    /// [`Repeater::indexed`] instead. See the [module-level docs](self) for the
    /// full rationale.
    pub fn new(model: ListModel<T>, factory: impl Fn(&T) -> Box<dyn Widget> + 'static) -> Self {
        Self {
            model,
            factory: RepeaterFactory::Keyless(Rc::new(factory)),
            layout: RepeaterLayout::Vertical,
            spacing: 0.0,
            line_spacing: 0.0,
            slots: Some(Rc::new(RefCell::new(Vec::new()))),
            container_id: None,
        }
    }

    /// Create a Repeater in **full-rebuild** mode: the `factory` receives
    /// `(index, &item)` and the entire child subtree is rebuilt on any model
    /// change, so position-derived content stays current.
    ///
    /// Use this only when the content depends on the item's position (row
    /// numbers, "N of M", a ranking that renumbers on reorder). It does **not**
    /// preserve per-child state across changes — prefer [`Repeater::new`]
    /// whenever the index isn't part of what each item renders.
    pub fn indexed(
        model: ListModel<T>,
        factory: impl Fn(usize, &T) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self {
            model,
            factory: RepeaterFactory::Indexed(Rc::new(factory)),
            layout: RepeaterLayout::Vertical,
            spacing: 0.0,
            line_spacing: 0.0,
            slots: None,
            container_id: None,
        }
    }

    /// Choose how items are arranged (default [`RepeaterLayout::Vertical`]).
    pub fn layout(mut self, layout: RepeaterLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Arrange items horizontally — shorthand for `.layout(RepeaterLayout::Horizontal)`.
    pub fn horizontal(self) -> Self {
        self.layout(RepeaterLayout::Horizontal)
    }

    /// Arrange items as a wrapping flow — shorthand for `.layout(RepeaterLayout::Wrap)`.
    pub fn wrap(self) -> Self {
        self.layout(RepeaterLayout::Wrap)
    }

    /// Set the gap between items along the main axis (default 0.0). For
    /// [`RepeaterLayout::Wrap`] this is the inter-item (horizontal) gap.
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Set the gap between lines for [`RepeaterLayout::Wrap`] (default 0.0).
    /// Ignored by the single-axis layouts.
    pub fn line_spacing(mut self, line_spacing: f32) -> Self {
        self.line_spacing = line_spacing;
        self
    }

    /// Build the item widgets for `build()`, returning their ids in model order.
    ///
    /// In reconciling mode the reconciliation table is first squared with the
    /// current model length, then each `Filled` slot is reused as-is and each
    /// `Vacant` slot is constructed and recorded — so only genuinely new /
    /// changed items pay a build. In indexed mode every item is (re)built.
    fn build_item_ids(&self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        let count = self.model.len();
        let mut ids = Vec::with_capacity(count);

        match &self.factory {
            RepeaterFactory::Keyless(factory) => {
                let factory = factory.clone();
                let slots_rc = self
                    .slots
                    .clone()
                    .expect("reconciling mode always has a reconciliation table");
                let mut slots = slots_rc.borrow_mut();

                // Square the table with the model. Normally the observer keeps it
                // in lock-step; this handles the first build (empty table) and is
                // a defensive backstop against any missed notification. Surplus
                // `Filled` slots are dropped here — their widgets fall out of the
                // returned set and the reconciling rebuild reaps them.
                if slots.len() < count {
                    slots.resize(count, ItemSlot::Vacant);
                } else if slots.len() > count {
                    slots.truncate(count);
                }

                for i in 0..count {
                    let id = match slots[i] {
                        ItemSlot::Filled(id) => id,
                        ItemSlot::Vacant => {
                            let widget = self
                                .model
                                .with_item(i, |item| factory(item))
                                .expect("index < len() so with_item yields Some");
                            let id = ctx.add_boxed(widget);
                            slots[i] = ItemSlot::Filled(id);
                            id
                        }
                    };
                    ids.push(id);
                }
            }
            RepeaterFactory::Indexed(factory) => {
                let factory = factory.clone();
                for i in 0..count {
                    // `i < count == len()`, so `with_item` always yields `Some`;
                    // the guard is a total-safety fallback, never taken here.
                    if let Some(widget) = self.model.with_item(i, |item| factory(i, item)) {
                        ids.push(ctx.add_boxed(widget));
                    }
                }
            }
        }

        ids
    }

    /// Wrap the ordered item ids in the container primitive for this layout.
    fn build_container(
        &self,
        ctx: &mut bastyde_core::build_context::BuildContext,
        item_ids: &[WidgetId],
    ) -> WidgetId {
        match self.layout {
            RepeaterLayout::Vertical => {
                let mut container = VStack::new().spacing(self.spacing);
                for &id in item_ids {
                    container = container.add_child(id);
                }
                ctx.add(container)
            }
            RepeaterLayout::Horizontal => {
                let mut container = HStack::new().spacing(self.spacing);
                for &id in item_ids {
                    container = container.add_child(id);
                }
                ctx.add(container)
            }
            RepeaterLayout::Wrap => {
                let mut container = Wrap::new()
                    .spacing(self.spacing)
                    .line_spacing(self.line_spacing);
                for &id in item_ids {
                    container = container.add_child(id);
                }
                ctx.add(container)
            }
        }
    }
}

/// Fold a single [`DataChange`] into the reconciliation table so the next
/// `build()` reuses surviving widgets and rebuilds only what actually changed.
/// All index arithmetic is bounds-clamped: a malformed range can never panic
/// here, only under- or over-reconcile (which `build_item_ids` then squares up).
fn apply_data_change(slots: &mut Vec<ItemSlot>, change: &DataChange) {
    match change {
        DataChange::ItemsInserted { range } => {
            let start = range.start.min(slots.len());
            let n = range.len();
            slots.splice(start..start, std::iter::repeat_n(ItemSlot::Vacant, n));
        }
        DataChange::ItemsRemoved { range } => {
            let start = range.start.min(slots.len());
            let end = range.end.min(slots.len());
            if start < end {
                slots.drain(start..end);
            }
        }
        DataChange::ItemsMoved { from, to, count } => {
            let (from, to, count) = (*from, *to, *count);
            if count == 0 || from >= slots.len() {
                return;
            }
            // Remove the block at `from`, then reinsert so its first item lands
            // at `to` (a post-removal index) — mirrors `ListModel::move_item`.
            let end = (from + count).min(slots.len());
            let moved: Vec<ItemSlot> = slots.drain(from..end).collect();
            let insert_at = to.min(slots.len());
            slots.splice(insert_at..insert_at, moved);
        }
        DataChange::ItemUpdated { index } => {
            // Content changed in place — rebuild just this widget. Dropping the
            // old id from the table lets the reconciling rebuild reap it.
            if *index < slots.len() {
                slots[*index] = ItemSlot::Vacant;
            }
        }
        DataChange::WindowLoaded { range } => {
            // A `ListModel` never emits this (only windowed `ListDataSource`s do),
            // but a `Repeater` can be pointed at one via a wrapper — treat the
            // window as needing (re)build.
            for i in range.clone() {
                if i < slots.len() {
                    slots[i] = ItemSlot::Vacant;
                }
            }
        }
        DataChange::Reset => {
            // Discard everything; `build_item_ids` re-fills to the new length.
            slots.clear();
        }
    }
}

impl<T: 'static> std::fmt::Debug for Repeater<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repeater")
            .field("item_count", &self.model.len())
            .field("layout", &self.layout)
            .field("spacing", &self.spacing)
            .field(
                "mode",
                &if self.slots.is_some() {
                    "reconciling"
                } else {
                    "indexed"
                },
            )
            .finish()
    }
}

impl<T: 'static> Widget for Repeater<T> {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        // A version counter bound at `Rebuild` level: every model mutation bumps
        // it, dirtying this widget for a rebuild on the next pass.
        let version = ctx.signal(0_u64);
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Observe model changes. In reconciling mode the observer *also* folds
        // each change into the reconciliation table so the upcoming rebuild
        // reuses surviving widgets; in indexed mode it just bumps the version.
        let slots_for_observer = self.slots.clone();
        let version_for_observer = version.clone();
        let handle = self.model.observe_changes(move |change| {
            if let Some(slots) = &slots_for_observer {
                apply_data_change(&mut slots.borrow_mut(), change);
            }
            version_for_observer.set(version_for_observer.get().wrapping_add(1));
        });
        ctx.own_handle(handle);

        let item_ids = self.build_item_ids(ctx);
        let root = self.build_container(ctx, &item_ids);
        self.container_id = Some(root);
        vec![root]
    }

    /// Reconciling mode reuses item widgets across rebuilds by re-attaching them
    /// to a freshly-built container. Returning `true` makes the reconciling
    /// rebuild path preserve any child still present after `build()` instead of
    /// tearing the whole subtree down first — the reused widgets survive with
    /// their state, and only the items the new build dropped are reaped.
    fn preserves_children_on_rebuild(&self) -> bool {
        self.slots.is_some()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.container_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Exactly one child (the container); fill this widget's bounds with it.
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.container_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::Size;
    use bastyde_core::widget_tree::WidgetTree;
    use std::cell::Cell;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    /// A leaf that increments a shared counter each time it is *constructed*.
    /// Lets a test tell a rebuilt widget from a reused one.
    #[derive(Debug)]
    struct CountingLeaf {
        _tag: u32,
    }
    impl CountingLeaf {
        fn new(tag: u32, builds: &Rc<Cell<u32>>) -> Self {
            builds.set(builds.get() + 1);
            Self { _tag: tag }
        }
    }
    impl Widget for CountingLeaf {
        fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            Size::new(50.0, 20.0).into()
        }
    }

    /// The item widgets are the grandchildren: Repeater -> container -> items.
    fn item_ids(tree: &WidgetTree, repeater_id: WidgetId) -> Vec<WidgetId> {
        let container = tree.children(repeater_id)[0];
        tree.children(container)
    }

    /// A reconciling Repeater over `&str`, counting factory invocations.
    fn counting_repeater(
        model: &ListModel<&'static str>,
        builds: &Rc<Cell<u32>>,
    ) -> Repeater<&'static str> {
        let builds_f = builds.clone();
        Repeater::new(model.clone(), move |item: &&str| {
            Box::new(CountingLeaf::new(item.len() as u32, &builds_f))
        })
    }

    // ---- Structure & layout (reconciling `new`) -------------------------

    #[test]
    fn creates_children_from_model() {
        let model = ListModel::from_vec(vec!["a", "b", "c"]);
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(Repeater::new(model, |_item| {
            Box::new(FixedLeaf(100.0, 30.0))
        }));
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let repeater_children = tree.children(repeater_id);
        assert_eq!(repeater_children.len(), 1); // the container
        assert_eq!(item_ids(&tree, repeater_id).len(), 3);
    }

    #[test]
    fn empty_model_creates_no_children() {
        let model: ListModel<&str> = ListModel::new();
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(Repeater::new(model, |_item| {
            Box::new(FixedLeaf(100.0, 30.0))
        }));
        tree.layout(SizeProposal::exact(200.0, 400.0));
        assert_eq!(item_ids(&tree, repeater_id).len(), 0);
    }

    #[test]
    fn push_adds_a_child() {
        let model = ListModel::from_vec(vec!["a", "b"]);
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(Repeater::new(model.clone(), |_item| {
            Box::new(FixedLeaf(100.0, 30.0))
        }));
        tree.layout(SizeProposal::exact(200.0, 400.0));
        assert_eq!(item_ids(&tree, repeater_id).len(), 2);

        model.push("c");
        tree.layout(SizeProposal::exact(200.0, 400.0));
        assert_eq!(item_ids(&tree, repeater_id).len(), 3);
    }

    #[test]
    fn spacing_is_applied() {
        let model = ListModel::from_vec(vec!["a", "b", "c"]);
        let mut tree = WidgetTree::new();

        let repeater_id =
            tree.add(Repeater::new(model, |_item| Box::new(FixedLeaf(100.0, 20.0))).spacing(10.0));
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let children = item_ids(&tree, repeater_id);
        assert_eq!(children.len(), 3);
        let y0 = tree.bounds(children[0]).y;
        let y1 = tree.bounds(children[1]).y;
        let y2 = tree.bounds(children[2]).y;
        assert!((y1 - y0 - 30.0).abs() < 0.01); // 20 height + 10 spacing
        assert!((y2 - y1 - 30.0).abs() < 0.01);
    }

    #[test]
    fn horizontal_layout_places_children_across() {
        let model = ListModel::from_vec(vec!["a", "b", "c"]);
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(
            Repeater::new(model, |_item| Box::new(FixedLeaf(40.0, 20.0)))
                .horizontal()
                .spacing(10.0),
        );
        tree.layout(SizeProposal::exact(400.0, 100.0));

        let children = item_ids(&tree, repeater_id);
        assert_eq!(children.len(), 3);
        let x0 = tree.bounds(children[0]).x;
        let x1 = tree.bounds(children[1]).x;
        let x2 = tree.bounds(children[2]).x;
        // Same row, advancing by width + spacing.
        assert!((tree.bounds(children[0]).y - tree.bounds(children[1]).y).abs() < 0.01);
        assert!((x1 - x0 - 50.0).abs() < 0.01); // 40 width + 10 spacing
        assert!((x2 - x1 - 50.0).abs() < 0.01);
    }

    #[test]
    fn wrap_layout_flows_to_next_line() {
        let model = ListModel::from_vec(vec!["a", "b", "c", "d"]);
        let mut tree = WidgetTree::new();

        // Width fits two 40 px items per line (with 10 px gap) but not three.
        let repeater_id = tree.add(
            Repeater::new(model, |_item| Box::new(FixedLeaf(40.0, 20.0)))
                .wrap()
                .spacing(10.0)
                .line_spacing(6.0),
        );
        tree.layout(SizeProposal::exact(100.0, 200.0));

        let children = item_ids(&tree, repeater_id);
        assert_eq!(children.len(), 4);
        let y_first = tree.bounds(children[0]).y;
        assert!(
            tree.bounds(children[2]).y > y_first + 0.01,
            "third item should wrap to the next line"
        );
    }

    // ---- Reconciliation: state-preserving `new` -------------------------

    #[test]
    fn reuses_widget_ids_on_insert() {
        let model = ListModel::from_vec(vec!["a", "b", "c"]);
        let builds = Rc::new(Cell::new(0));
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(counting_repeater(&model, &builds));
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let before = item_ids(&tree, repeater_id);
        assert_eq!(before.len(), 3);
        assert_eq!(builds.get(), 3, "each item built once");

        // Insert at the front: a, b, c must keep their widget ids.
        model.insert(0, "z");
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let after = item_ids(&tree, repeater_id);
        assert_eq!(after.len(), 4);
        assert_eq!(builds.get(), 4, "only the inserted item built anew");
        assert_eq!(&after[1..], &before[..], "survivors keep their widgets");
        assert!(!before.contains(&after[0]), "index 0 is a fresh widget");
    }

    #[test]
    fn reorder_preserves_all_widgets() {
        let model = ListModel::from_vec(vec!["a", "b", "c"]);
        let builds = Rc::new(Cell::new(0));
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(counting_repeater(&model, &builds));
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let before = item_ids(&tree, repeater_id);
        assert_eq!(builds.get(), 3);

        model.move_item(0, 2);
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let after = item_ids(&tree, repeater_id);
        assert_eq!(builds.get(), 3, "reorder builds nothing");
        assert_eq!(after, vec![before[1], before[2], before[0]]);
    }

    #[test]
    fn remove_reaps_only_removed() {
        let model = ListModel::from_vec(vec!["a", "b", "c"]);
        let builds = Rc::new(Cell::new(0));
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(counting_repeater(&model, &builds));
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let before = item_ids(&tree, repeater_id);

        model.remove(1);
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let after = item_ids(&tree, repeater_id);
        assert_eq!(builds.get(), 3, "remove builds nothing");
        assert_eq!(after, vec![before[0], before[2]]);
        assert!(!tree.is_active(before[1]), "removed widget reaped");
    }

    #[test]
    fn update_rebuilds_only_that_item() {
        let model = ListModel::from_vec(vec!["a", "b", "c"]);
        let builds = Rc::new(Cell::new(0));
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(counting_repeater(&model, &builds));
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let before = item_ids(&tree, repeater_id);

        model.set(1, "beta");
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let after = item_ids(&tree, repeater_id);
        assert_eq!(builds.get(), 4, "exactly one extra build for the update");
        assert_eq!(after[0], before[0], "unchanged neighbours reused");
        assert_eq!(after[2], before[2], "unchanged neighbours reused");
        assert_ne!(after[1], before[1], "updated item is a fresh widget");
        assert!(!tree.is_active(before[1]), "stale widget reaped");
    }

    #[test]
    fn reset_rebuilds_all() {
        let model = ListModel::from_vec(vec!["a", "b"]);
        let builds = Rc::new(Cell::new(0));
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(counting_repeater(&model, &builds));
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let before = item_ids(&tree, repeater_id);
        assert_eq!(builds.get(), 2);

        model.replace_all(vec!["x", "y", "z"]);
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let after = item_ids(&tree, repeater_id);
        assert_eq!(after.len(), 3);
        assert_eq!(builds.get(), 5, "all three rebuilt after a reset");
        for id in &before {
            assert!(!after.contains(id), "no widget survives a reset");
        }
    }

    #[test]
    fn preserves_child_signal_state_across_insert() {
        // The point of reconciling mode: a child that owns mutable state keeps
        // it when a sibling is inserted. We model "state" as a signal the child
        // holds; reuse ⟺ the same signal value survives.
        use bastyde_core::signal::Signal;

        #[derive(Debug)]
        struct Stateful {
            state: Signal<u32>,
        }
        impl Widget for Stateful {
            fn build(
                &mut self,
                ctx: &mut bastyde_core::build_context::BuildContext,
            ) -> Vec<WidgetId> {
                // Relayout when the state changes, so a mid-test `set` actually
                // re-measures. The binding rides on this widget's own id and thus
                // survives the Repeater's reconciling rebuild (the child is reused,
                // not rebuilt, so its bindings are never torn down).
                self.state.bind_to(
                    ctx.self_id(),
                    ctx.binding_registry(),
                    BindingLevel::Relayout,
                );
                vec![]
            }
            fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
                // Encode the live state into the height so the tree can read it.
                Size::new(20.0, self.state.get() as f32).into()
            }
        }

        let seeds: Rc<RefCell<Vec<Signal<u32>>>> = Rc::new(RefCell::new(Vec::new()));
        let model = ListModel::from_vec(vec![1_u32, 2, 3]);
        let mut tree = WidgetTree::new();

        let seeds_f = seeds.clone();
        let repeater_id = tree.add(Repeater::new(model.clone(), move |item: &u32| {
            let state = Signal::new(*item);
            seeds_f.borrow_mut().push(state.clone());
            Box::new(Stateful { state })
        }));

        tree.layout(SizeProposal::exact(200.0, 400.0));
        let before = item_ids(&tree, repeater_id);
        // Mutate the middle child's live state to a sentinel.
        seeds.borrow()[1].set(999);
        tree.layout(SizeProposal::exact(200.0, 400.0));
        assert!((tree.bounds(before[1]).height - 999.0).abs() < 0.01);

        // Insert at the front; the middle child (now at index 2) must keep 999.
        model.insert(0, 0);
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let after = item_ids(&tree, repeater_id);
        assert_eq!(after[2], before[1], "the stateful child was reused");
        assert!(
            (tree.bounds(after[2]).height - 999.0).abs() < 0.01,
            "reused child kept its mutated state"
        );
    }

    #[test]
    fn preserves_focus_across_insert() {
        // The Skribisto guarantee: the editor the user is in keeps focus when a
        // scene is inserted elsewhere in the document.
        use bastyde_core::widget_builder::WidgetBuilder;

        let model = ListModel::from_vec(vec!["a", "b", "c"]);
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(Repeater::new(model.clone(), |_item: &&str| {
            Box::new(FixedLeaf(50.0, 20.0).focusable(true))
        }));

        tree.layout(SizeProposal::exact(200.0, 400.0));
        let before = item_ids(&tree, repeater_id);

        // Focus the middle child, then insert a sibling above it.
        tree.focus(before[1]);
        assert_eq!(tree.focused(), Some(before[1]));

        model.insert(0, "z");
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let after = item_ids(&tree, repeater_id);
        assert_eq!(after[2], before[1], "the focused child was reused");
        assert_eq!(
            tree.focused(),
            Some(before[1]),
            "focus stays on the same widget across the insert"
        );
    }

    // ---- Full-rebuild `indexed` -----------------------------------------

    #[test]
    fn indexed_factory_receives_index_and_item() {
        let model = ListModel::from_vec(vec![10.0_f32, 20.0, 30.0]);
        let mut tree = WidgetTree::new();

        let repeater_id = tree.add(Repeater::indexed(model, |i, item| {
            // Width encodes the index, height encodes the item value.
            Box::new(FixedLeaf(i as f32, *item))
        }));
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let children = item_ids(&tree, repeater_id);
        assert_eq!(children.len(), 3);
        assert!((tree.bounds(children[0]).height - 10.0).abs() < 0.01);
        assert!((tree.bounds(children[1]).height - 20.0).abs() < 0.01);
        assert!((tree.bounds(children[2]).height - 30.0).abs() < 0.01);
    }

    #[test]
    fn indexed_rebuilds_every_child_on_change() {
        // Indexed mode does NOT preserve widgets — every child is rebuilt on any
        // change, which is what keeps position-derived content correct.
        let model = ListModel::from_vec(vec!["a", "b", "c"]);
        let builds = Rc::new(Cell::new(0));
        let mut tree = WidgetTree::new();

        let builds_f = builds.clone();
        let repeater_id = tree.add(Repeater::indexed(model.clone(), move |_i, item: &&str| {
            Box::new(CountingLeaf::new(item.len() as u32, &builds_f))
        }));

        tree.layout(SizeProposal::exact(200.0, 400.0));
        let before = item_ids(&tree, repeater_id);
        assert_eq!(builds.get(), 3);

        // A single insert rebuilds the whole subtree (4 fresh widgets), and none
        // of the old widget ids survive.
        model.insert(0, "z");
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let after = item_ids(&tree, repeater_id);
        assert_eq!(after.len(), 4);
        assert_eq!(builds.get(), 7, "3 initial + 4 rebuilt");
        for id in &before {
            assert!(!after.contains(id), "no widget is reused in indexed mode");
        }
    }

    // ---- Accessibility: transparent by default, opt-in via overrides ----

    #[test]
    fn accepts_standard_access_overrides() {
        use bastyde_core::accesskit::Role;
        use bastyde_core::widget_builder::WidgetBuilder;
        use bastyde_i18n::lit;

        let model = ListModel::from_vec(vec!["a", "b"]);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());

        let repeater_id = tree.add(
            Repeater::new(model, |_item| Box::new(FixedLeaf(100.0, 20.0)))
                .access_role(Role::List)
                .access_label(lit!("Tags")),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let node = tree.accessibility_node(repeater_id);
        assert_eq!(node.role(), Role::List);
        assert_eq!(node.name(), Some("Tags"));
    }
}
