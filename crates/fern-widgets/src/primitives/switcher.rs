use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Point, Rect, Size, SizeProposal};

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// One entry inside a Switcher. `Pending` holds a page that has never
/// been selected yet — its widget stays Boxed (zero arena footprint,
/// zero `build()` cost) until the matching index becomes selected for
/// the first time. `Mounted` carries the arena id from then on.
enum Slot {
    /// Deferred page: lives outside the arena until first selection.
    Pending(Box<dyn Widget>),
    /// Pre-mounted page: caller registered the widget themselves and
    /// handed us the id; we eagerly wire `visible_when` and treat it
    /// as immediately mounted (lazy semantics don't help here — the
    /// construction cost has already been paid upstream).
    PreMounted(WidgetId),
    /// Page that has been mounted into the arena (either from
    /// `PreMounted` on first build, or from `Pending` on first
    /// selection). The id is preserved across Switcher rebuilds via
    /// [`Widget::preserves_children_on_rebuild`].
    Mounted(WidgetId),
}

/// A container that shows exactly one child at a time, driven by a
/// `Signal<usize>` index.
///
/// **Lazy mount.** A page added via [`Self::child`] / [`Self::child_boxed`]
/// / [`Self::children`] stays unconstructed until its index is first
/// selected. Once mounted, the page's subtree persists for the
/// Switcher's lifetime — switching away then back finds it in the
/// state the user left it (focus, scroll, text-input contents, …).
/// Pages added via [`Self::child_id`] are pre-mounted by the caller
/// and treated eagerly: no lazy benefit, no semantic change.
///
/// The Switcher itself reports the maximum natural size across every
/// currently-mounted page and stretches each placed child to its own
/// bounds (top-leading, RTL-aware). Hidden pages keep their subtree
/// laid out but invisible via per-page `visible_when` bindings.
///
/// ```ignore
/// let page = Signal::new(0_usize);
/// Switcher::new(page.clone())
///     .child(TextWidget::new_literal("Page 0"))   // built at startup
///     .child(TextWidget::new_literal("Page 1"))   // built when page.set(1)
///     .child(TextWidget::new_literal("Page 2"))   // built when page.set(2)
/// ```
pub struct Switcher {
    selected: Signal<usize>,
    slots: Vec<Slot>,
    /// Optional external buffer populated during `build()` with the
    /// `WidgetId` of every currently-mounted page in declaration order.
    /// `Pending` slots contribute nothing — callers that need every
    /// page's id available before first selection must pre-mount via
    /// [`Self::child_id`].
    child_ids_out: Option<Rc<RefCell<Vec<WidgetId>>>>,
}

impl Switcher {
    pub fn new(selected: Signal<usize>) -> Self {
        Self {
            selected,
            slots: Vec::new(),
            child_ids_out: None,
        }
    }

    /// Capture each mounted page's `WidgetId` into an externally owned
    /// buffer during `build()`. Use when the caller needs to reference
    /// pages after they're added to the arena — e.g. for accessibility
    /// relations like Tab → TabPanel.
    ///
    /// The buffer reflects the **currently-mounted** set, not every
    /// declared page. With lazy mount, a page added via `child(...)`
    /// only appears in the buffer once it has been selected for the
    /// first time. Callers that need every id up front should pass
    /// pre-mounted ids via [`Self::child_id`] instead — those are
    /// eagerly recorded.
    pub fn capture_child_ids_into(mut self, out: Rc<RefCell<Vec<WidgetId>>>) -> Self {
        self.child_ids_out = Some(out);
        self
    }

    /// Add a child page. The widget stays Boxed until its index is
    /// selected for the first time, then is mounted into the arena
    /// and kept alive across selection changes.
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.slots.push(Slot::Pending(Box::new(widget)));
        self
    }

    /// Add a pre-boxed child page (lazy, same as [`Self::child`]).
    pub fn child_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.slots.push(Slot::Pending(widget));
        self
    }

    /// Add a child page by its already-allocated `WidgetId`. Pre-mounted
    /// pages are wired eagerly — the lazy path doesn't apply because
    /// the caller has already paid the construction cost.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.slots.push(Slot::PreMounted(id));
        self
    }

    /// Add multiple child pages from an iterator (lazy, same as
    /// [`Self::child`]).
    pub fn children(mut self, iter: impl IntoIterator<Item = impl Widget + 'static>) -> Self {
        for widget in iter {
            self.slots.push(Slot::Pending(Box::new(widget)));
        }
        self
    }
}

impl std::fmt::Debug for Switcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Switcher")
            .field("num_children", &self.slots.len())
            .finish()
    }
}

impl Widget for Switcher {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();

        // `selected` flips drive Switcher rebuilds: a flip onto an
        // already-mounted page costs an idempotent re-registration of
        // visibility bindings (cheap); a flip onto a `Pending` slot
        // triggers the lazy `ctx.add_boxed` below. Rebuild level is
        // load-bearing — without it the framework would only repaint
        // and the unmounted page would never get built.
        self.selected
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);

        let current = self.selected.get();

        // Walk every still-Pending slot's static
        // `Widget::declare_shortcuts` and pre-register the metadata
        // owned by this Switcher. This makes shortcuts buried inside
        // a not-yet-selected page visible to `ShortcutSettings` (and
        // any other registry consumer) from the moment the Switcher
        // builds — without paying the cost of mounting the page. When
        // the page is eventually mounted, the framework's insert-time
        // declaration walk re-registers the same ids owned by the
        // page widget; the registry upserts cleanly.
        for slot in self.slots.iter() {
            if let Slot::Pending(widget) = slot {
                let declared = widget.declare_shortcuts();
                if !declared.is_empty() {
                    ctx.register_pending_shortcuts(declared);
                }
            }
        }

        // Materialize: promote PreMounted → Mounted on first build,
        // and promote Pending → Mounted when its index becomes
        // selected. Pending slots untouched here stay Pending; they
        // contribute zero work to the arena until visited.
        for (i, slot) in self.slots.iter_mut().enumerate() {
            match slot {
                Slot::PreMounted(id) => {
                    *slot = Slot::Mounted(*id);
                }
                Slot::Pending(_) if i == current => {
                    let widget = match std::mem::replace(slot, Slot::Mounted(WidgetId::default())) {
                        Slot::Pending(w) => w,
                        _ => unreachable!(),
                    };
                    let id = ctx.add_boxed(widget);
                    *slot = Slot::Mounted(id);
                }
                _ => {}
            }
        }

        // Wire `visible_when` on every mounted page. The binding
        // registry deduplicates per `(widget_id, source_id, level)`
        // tuple, so calling this on every rebuild collapses to the
        // same single entry — no accumulation.
        for (i, slot) in self.slots.iter().enumerate() {
            if let Slot::Mounted(id) = slot {
                let idx = i;
                let vis = self.selected.map(move |s| *s == idx);
                ctx.visible_when(*id, vis);
            }
        }

        // Publish currently-mounted ids to the external buffer.
        if let Some(ref out) = self.child_ids_out {
            let mut buf = out.borrow_mut();
            buf.clear();
            for slot in &self.slots {
                if let Slot::Mounted(id) = slot {
                    buf.push(*id);
                }
            }
        }

        // Children: every mounted page, in declaration order. The
        // framework calls `preserves_children_on_rebuild` and skips
        // the subtree teardown that would otherwise destroy the
        // mounted pages' state on every selection change.
        self.slots
            .iter()
            .filter_map(|s| match s {
                Slot::Mounted(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    fn preserves_children_on_rebuild(&self) -> bool {
        // Mounted pages survive selection-driven rebuilds. The
        // alternative — letting the framework destroy them — would
        // wipe focus, scroll offsets, text-input contents, and any
        // signal subscriptions every time the user clicked a
        // different tab.
        true
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        // Mirror ZStack's intrinsic-sizing rule: max of mounted
        // children's natural sizes under an unspecified proposal.
        // Background-style pages that return 0×0 for unspecified
        // don't inflate the stack; content pages report their real
        // wants.
        let mut max_w: f32 = 0.0;
        let mut max_h: f32 = 0.0;
        let mut any = false;
        for slot in &self.slots {
            if let Slot::Mounted(id) = slot
                && let Some(child_size) = ctx.child_size(*id, SizeProposal::unspecified())
            {
                max_w = max_w.max(child_size.width);
                max_h = max_h.max(child_size.height);
                any = true;
            }
        }
        if any {
            Size::new(max_w, max_h)
        } else {
            proposal.resolve(0.0, 0.0)
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // Top-leading layout (matches the ZStack-with-TOP_LEADING
        // alignment the previous wrapper used). Background widgets
        // that take the exact proposal fill the bounds; widgets with
        // an intrinsic natural size sit at the top-leading corner
        // (RTL-aware).
        let rtl = ctx.is_rtl();
        let exact_proposal = SizeProposal::exact(bounds.width, bounds.height);
        for child in children.iter_mut() {
            let child_size = ctx
                .child_size(child.id, exact_proposal)
                .unwrap_or_else(|| bounds.size());
            let dx = if rtl {
                bounds.width - child_size.width
            } else {
                0.0
            };
            child.origin = Point::new(bounds.x + dx, bounds.y);
            child.size = child_size;
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.slots
            .iter()
            .filter_map(|s| match s {
                Slot::Mounted(id) => Some(*id),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::Size;
    use fern_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> fern_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    /// Counts `build()` invocations so we can assert lazy-mount
    /// semantics: a page should `build()` at most once, and only
    /// after its index has been selected.
    #[derive(Debug)]
    struct CountingLeaf {
        build_calls: Rc<std::cell::Cell<u32>>,
        size: (f32, f32),
    }
    impl CountingLeaf {
        fn new(w: f32, h: f32) -> (Self, Rc<std::cell::Cell<u32>>) {
            let counter = Rc::new(std::cell::Cell::new(0));
            (
                Self {
                    build_calls: counter.clone(),
                    size: (w, h),
                },
                counter,
            )
        }
    }
    impl Widget for CountingLeaf {
        fn build(&mut self, _ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
            self.build_calls.set(self.build_calls.get() + 1);
            Vec::new()
        }
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> fern_core::widget::LayoutResponse {
            Size::new(self.size.0, self.size.1).into()
        }
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

    /// Only the initially-selected page should ever have its
    /// `build()` called. Unvisited pages stay `Pending` and pay no
    /// arena / construction cost.
    #[test]
    fn unvisited_pages_never_build() {
        let selected = Signal::new(0_usize);
        let (page0, c0) = CountingLeaf::new(50.0, 50.0);
        let (page1, c1) = CountingLeaf::new(60.0, 60.0);
        let (page2, c2) = CountingLeaf::new(70.0, 70.0);

        let mut tree = WidgetTree::new();
        let _id = tree.add(
            Switcher::new(selected.clone())
                .child(page0)
                .child(page1)
                .child(page2),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));

        assert_eq!(c0.get(), 1, "selected page must be built");
        assert_eq!(c1.get(), 0, "unvisited page must not build");
        assert_eq!(c2.get(), 0, "unvisited page must not build");
    }

    /// Switching to a previously-unvisited index mounts that page
    /// lazily; older pages stay alive (their `build()` count must
    /// not increment again — they are preserved, not rebuilt).
    #[test]
    fn switching_mounts_lazily_and_preserves_prior_pages() {
        let selected = Signal::new(0_usize);
        let (page0, c0) = CountingLeaf::new(50.0, 50.0);
        let (page1, c1) = CountingLeaf::new(60.0, 60.0);
        let (page2, c2) = CountingLeaf::new(70.0, 70.0);

        let mut tree = WidgetTree::new();
        let _id = tree.add(
            Switcher::new(selected.clone())
                .child(page0)
                .child(page1)
                .child(page2),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));
        assert_eq!((c0.get(), c1.get(), c2.get()), (1, 0, 0));

        selected.set(1);
        tree.layout(SizeProposal::exact(200.0, 200.0));
        assert_eq!(
            (c0.get(), c1.get(), c2.get()),
            (1, 1, 0),
            "page 1 mounts on first visit; page 0 is preserved (not rebuilt)"
        );

        selected.set(0);
        tree.layout(SizeProposal::exact(200.0, 200.0));
        assert_eq!(
            (c0.get(), c1.get(), c2.get()),
            (1, 1, 0),
            "returning to page 0 must reuse the existing subtree"
        );

        selected.set(2);
        tree.layout(SizeProposal::exact(200.0, 200.0));
        assert_eq!(
            (c0.get(), c1.get(), c2.get()),
            (1, 1, 1),
            "page 2 mounts on first visit"
        );
    }

    /// `Widget::declare_shortcuts` returned by a Pending Switcher page
    /// must be registered in the shortcut registry before the page is
    /// mounted — settings UIs depend on seeing the full keystroke
    /// catalog without forcing every lazy branch to build.
    #[test]
    fn switcher_pending_pages_declare_shortcuts_eagerly() {
        use fern_core::event::Key;
        use fern_core::shortcut::{KeyStroke, Shortcut};

        #[derive(Debug)]
        struct LazyWithShortcuts(Rc<std::cell::Cell<u32>>);
        impl Widget for LazyWithShortcuts {
            fn declare_shortcuts(&self) -> Vec<Shortcut> {
                vec![
                    Shortcut::new("__test.lazy.action")
                        .name("Lazy Action")
                        .primary(KeyStroke::ctrl(Key::L))
                        .build(),
                ]
            }
            fn build(
                &mut self,
                _ctx: &mut fern_core::build_context::BuildContext,
            ) -> Vec<WidgetId> {
                self.0.set(self.0.get() + 1);
                Vec::new()
            }
            fn layout_response(
                &self,
                _proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> fern_core::widget::LayoutResponse {
                Size::new(10.0, 10.0).into()
            }
        }

        let selected = Signal::new(0_usize);
        let build_count = Rc::new(std::cell::Cell::new(0));
        let mut tree = WidgetTree::new();
        let _id = tree.add(
            Switcher::new(selected.clone())
                .child(FixedLeaf(50.0, 50.0))
                .child(LazyWithShortcuts(build_count.clone())),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));

        assert_eq!(
            build_count.get(),
            0,
            "lazy page must not have built — index 1 was never selected"
        );
        assert!(
            tree.shortcut_registry()
                .get_default("__test.lazy.action")
                .is_some(),
            "Switcher must pre-register Pending pages' declared shortcuts"
        );
    }
}
