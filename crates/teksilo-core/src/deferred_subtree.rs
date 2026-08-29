// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`DeferredSubtree`] — a child whose subtree is not built until it is first
//! revealed, and is retained from then on.
//!
//! ## The cost this exists to remove
//!
//! A widget that owns overlay content — a popover's panel, a combo box's
//! dropdown, a menu item's submenu, a date field's calendar — has until now
//! written it the same way:
//!
//! ```ignore
//! let content_id = ctx.add(panel);   // builds the WHOLE subtree, now
//! ctx.set_dormant(content_id);       // ...and immediately parks it
//! ```
//!
//! That is correct and it is what `Arena::set_dormant`
//! documents: dormancy is about *activation*, not construction, and a parked
//! subtree keeps its state. What it costs is a full `build()` of content the
//! user may never open — on **every rebuild of the owner**.
//!
//! In a single dialog that is invisible. In a virtualized collection it is the
//! dominant cost, because the owner is a per-row delegate: a table cell hosting
//! a `PopoverIconButton` builds its entire menu once per row, per rebuild.
//! Measured on a 40-row table whose cells each carried a four-item menu:
//!
//! | per rebuild | |
//! |---|---|
//! | cells with the eager popover | 325–552 ms |
//! | same cells, content not added to the arena | 61–73 ms |
//! | no such column at all | 42–46 ms |
//!
//! Roughly **85% of the cost is `ctx.add`**, not constructing the widget value
//! that is handed to it. Deferring the insertion is therefore the whole win, and
//! it needs no change to what callers pass.
//!
//! ## The contract
//!
//! * **Built at most once.** The first `build()` that sees `reveal == true`
//!   materializes the subtree; every later rebuild of the host returns the same
//!   child id. So state inside the content survives close/reopen exactly as the
//!   eager-then-dormant version did — that guarantee is why this defers
//!   construction rather than rebuilding per open.
//! * **The id is stable from the start.** [`BuildContext::add_deferred`] returns a real arena
//!   node immediately, so everything downstream — `set_dormant` / `activate`,
//!   `visible_when`, `OverlayRequest::content_id`, descendant checks, dismissal —
//!   is unchanged. Only *when* the subtree below that id exists has moved.
//! * **Layout-transparent.** Reports the child's size, and nothing (a zero-size
//!   node) while still unbuilt.
//!
//! ## Why an explicit `reveal` signal, and not activation
//!
//! A node's own activation is observable ([`BuildContext::activation_signal`]),
//! and every caller here already gates the content on a signal — that is what
//! `visible_when` is given. Binding *that* signal is what makes the content
//! arrive in the right frame: a `Signal` set inside an event handler marks the
//! host `needs_rebuild` during dispatch, so the rebuild pass of the very next
//! layout builds the content **before** the overlay it belongs to is measured
//! and placed. `activation_signal` is flushed at the *end* of the visibility
//! pass (see `WidgetTree::flush_activation_signals`), which is after that
//! frame's rebuilds — the content would land a frame late, and the overlay would
//! be placed against an empty panel first.
//!
//! (The framework re-runs `process_pending_rebuilds` after overlay activation
//! for exactly this class of problem, and `needs_rebuild_iter` is deliberately
//! not gated on the node already having children — so a host that starts empty
//! is rebuilt correctly either way. The explicit signal is what makes the
//! timing tight rather than merely eventually-correct.)

use teksilo_canvas::{Point, Rect, Size, SizeProposal};

use crate::binding::BindingLevel;
use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use crate::widget_id::WidgetId;

/// A child subtree built the first time `reveal` is `true`, then retained.
///
/// Construct through [`BuildContext::add_deferred`] and its siblings rather
/// than directly — they are what give the host its stable id.
pub struct DeferredSubtree {
    /// The un-built content. `None` once materialized (or once taken by a
    /// build that found `reveal` true).
    pending: Option<Box<dyn Widget>>,
    /// The materialized child, once built. Retained across rebuilds.
    child: Option<WidgetId>,
    /// The caller's reveal gate. `None` for content the *framework* decides to
    /// materialize — a tooltip body, which has no widget-visible open signal
    /// and is instead forced by the tree when a dwell matures.
    reveal: Option<Signal<bool>>,
    /// Set by [`force`](Self::force) when the framework needs the content now.
    forced: bool,
}

impl DeferredSubtree {
    pub(crate) fn new(reveal: Option<Signal<bool>>, content: Box<dyn Widget>) -> Self {
        Self {
            pending: Some(content),
            child: None,
            reveal,
            forced: false,
        }
    }

    /// Materialize on the next build regardless of the reveal gate.
    ///
    /// For content the framework shows on its own initiative — a tooltip body,
    /// whose dwell has no signal a widget could hand over. Pair it with a
    /// rebuild of this host (`WidgetTree::materialize_deferred` does both).
    pub fn force(&mut self) {
        self.forced = true;
    }

    /// Whether the subtree has been built. Test hook, and the honest answer to
    /// "did deferring actually defer".
    pub fn is_materialized(&self) -> bool {
        self.child.is_some()
    }

    /// The materialized child, if the subtree has been built.
    ///
    /// Lets a caller that must inspect the *content* — the accessibility walk
    /// reading a tooltip's text off the node it is attached to — resolve past
    /// this host instead of probing the host itself and finding nothing.
    pub fn materialized_child(&self) -> Option<WidgetId> {
        self.child
    }
}

impl std::fmt::Debug for DeferredSubtree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeferredSubtree")
            .field("materialized", &self.child.is_some())
            .finish()
    }
}

impl Widget for DeferredSubtree {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Bound on every build, including the ones that return nothing: the
        // binding is what turns the reveal into a rebuild, so a host that
        // skipped it while asleep would never wake.
        if let Some(reveal) = &self.reveal {
            reveal.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        }

        if let Some(id) = self.child {
            // Already materialized. Return the same id rather than rebuilding:
            // re-adding would discard whatever state the content holds, which
            // is the one thing the eager-then-dormant version got right.
            return vec![id];
        }
        if !self.forced && !self.reveal.as_ref().is_some_and(|r| r.get()) {
            return Vec::new();
        }
        let Some(content) = self.pending.take() else {
            // Revealed, but the content was already consumed and no child came
            // of it. Nothing to do — and nothing to build twice.
            return Vec::new();
        };
        let id = ctx.add_boxed(content);
        self.child = Some(id);
        vec![id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        match self.child {
            // `child_size` returns `None` for a **dormant** child as well as an
            // unbuilt one, and a materialized-then-closed popover (or a tooltip
            // body between dwells) is exactly that. So this fallback is the same
            // hazard as the arm below and takes the same answer: `Size::ZERO`,
            // never `proposal.resolve(0.0, 0.0)`.
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or(Size::new(0.0, 0.0))
                .into(),
            // **`Size::ZERO`, never `proposal.resolve(0.0, 0.0)`.** `resolve`
            // defers to whichever axis the proposal specifies, so under an
            // `exact` proposal it hands back the parent's full box — an unbuilt
            // popover panel would then claim the whole cell it is a sibling of
            // and shove the trigger out of the row.
            None => Size::new(0.0, 0.0).into(),
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Layout-transparent: the child occupies the host's whole box, so the
        // node adds a level to the arena and nothing to the geometry.
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = bounds.size();
        }
    }

    /// **Required, not an optimisation.** The default rebuild path destroys a
    /// widget's children *before* calling `build()`, so a host that caches its
    /// child id and returns it again would hand back a dead node — the content
    /// would vanish the first time anything rebuilt this host. Retaining the
    /// materialized subtree across rebuilds is the whole contract here.
    fn preserves_children_on_rebuild(&self) -> bool {
        true
    }

    /// **Delegates to the un-built content**, which is the whole reason this can
    /// be layered under a tooltip at all.
    ///
    /// A plain tooltip is never auto-shown on focus, so the description copied
    /// onto the anchor *is* the entire screen-reader path for that tier — and it
    /// is read by probing the content widget's own `accessibility`. Deferring
    /// the subtree must not cost that: the widget value is right here, un-built,
    /// and answering from it needs no arena node. Without this the tips still
    /// appear on hover and every screen-reader user silently loses them, which
    /// is the kind of regression that ships.
    fn accessibility(&self, builder: &mut crate::accessibility::AccessNodeBuilder) {
        if let Some(pending) = &self.pending {
            pending.accessibility(builder);
        }
    }

    /// Same delegation as [`accessibility`](Self::accessibility), for the
    /// emptiness guard: a tooltip with nothing to say must not open a bubble,
    /// and the un-built body is the only thing that knows.
    fn tooltip_has_content(&self) -> bool {
        match &self.pending {
            Some(pending) => pending.tooltip_has_content(),
            None => true,
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    /// Required by `WidgetTree::materialize_deferred`, which reaches this
    /// widget by id to force it.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget_tree::WidgetTree;

    /// Counts how many times it is built, so a test can prove the difference
    /// between "not shown" and "not built".
    #[derive(Debug)]
    struct BuildCounter {
        builds: Signal<u32>,
    }

    impl Widget for BuildCounter {
        fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
            self.builds.set(self.builds.get() + 1);
            Vec::new()
        }

        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(20.0, 12.0).into()
        }
    }

    /// A host that owns one deferred child, so the test drives the same shape a
    /// popover does: the host rebuilds, the child must not.
    #[derive(Debug)]
    struct Host {
        reveal: Signal<bool>,
        builds: Signal<u32>,
        host_builds: Signal<u32>,
        child: Option<WidgetId>,
    }

    impl Widget for Host {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            self.host_builds.set(self.host_builds.get() + 1);
            // Rebuild this host whenever `reveal` moves, exactly as a popover's
            // trigger does — the case that must not starve the child of its own
            // rebuild.
            self.reveal
                .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
            let id = self.child.unwrap_or_else(|| {
                ctx.add_deferred(
                    self.reveal.clone(),
                    BuildCounter {
                        builds: self.builds.clone(),
                    },
                )
            });
            self.child = Some(id);
            vec![id]
        }

        fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
            self.child
                .and_then(|id| ctx.child_size(id, proposal))
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
                .into()
        }

        fn preserves_children_on_rebuild(&self) -> bool {
            true
        }
    }

    fn tree_with_host() -> (WidgetTree, Signal<bool>, Signal<u32>, Signal<u32>, WidgetId) {
        let reveal = Signal::new(false);
        let builds = Signal::new(0);
        let host_builds = Signal::new(0);
        let mut tree = WidgetTree::new();
        let id = tree.add(Host {
            reveal: reveal.clone(),
            builds: builds.clone(),
            host_builds: host_builds.clone(),
            child: None,
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));
        (tree, reveal, builds, host_builds, id)
    }

    /// **The whole point: unrevealed content is never built.**
    ///
    /// Not "built and parked" — not built. The eager form this replaces would
    /// report one build here, and one more on every rebuild of the host.
    #[test]
    fn content_is_not_built_until_it_is_revealed() {
        let (_tree, _reveal, builds, _host_builds, _id) = tree_with_host();
        assert_eq!(builds.get(), 0, "content built while it was never revealed");
    }

    /// And rebuilding the host — what a table cell does constantly — still does
    /// not build it. This is the case the measurement in the module doc is about.
    #[test]
    fn rebuilding_the_host_does_not_build_unrevealed_content() {
        let (mut tree, _reveal, builds, host_builds, id) = tree_with_host();
        for _ in 0..5 {
            tree.arena_mark_needs_rebuild_for_testing(id);
            tree.layout(SizeProposal::exact(200.0, 100.0));
        }
        assert!(
            host_builds.get() >= 5,
            "the host itself must really have rebuilt; got {}",
            host_builds.get()
        );
        assert_eq!(
            builds.get(),
            0,
            "the host rebuilt {} times and dragged its unopened content along",
            host_builds.get()
        );
    }

    /// Revealing builds it — once — and it is retained afterwards.
    #[test]
    fn revealing_builds_the_content_once_and_keeps_it() {
        let (mut tree, reveal, builds, _host_builds, id) = tree_with_host();
        reveal.set(true);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(builds.get(), 1, "revealing must build the content");

        // Close and reopen: the content is retained, so it is not rebuilt.
        // That retention is what the eager-then-dormant form bought, and what
        // deferring must not give up — state inside a popover survives a close.
        reveal.set(false);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        reveal.set(true);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(
            builds.get(),
            1,
            "content was rebuilt on reopen — its state would have been lost"
        );

        // And a plain rebuild of the host keeps it too.
        tree.arena_mark_needs_rebuild_for_testing(id);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert_eq!(builds.get(), 1);
    }

    /// The host is layout-transparent once built, and zero-size while not.
    #[test]
    fn the_host_reports_its_childs_size_and_nothing_before_that() {
        let (mut tree, reveal, _builds, _host_builds, host) = tree_with_host();
        // Laid out under a proposal that leaves the height free, so the root's
        // height follows what the subtree *reports*. Under `exact` every node is
        // stretched to the box by its parent's placement, and the assertion
        // would be measuring the placement policy rather than this widget.
        tree.layout(SizeProposal::with_width(200.0));
        assert_eq!(
            tree.bounds(host).height,
            0.0,
            "an unbuilt deferred subtree must take no space"
        );

        reveal.set(true);
        tree.layout(SizeProposal::with_width(200.0));
        assert_eq!(
            tree.bounds(host).height,
            12.0,
            "once built it is layout-transparent — the child's size, not its own"
        );
    }
}
