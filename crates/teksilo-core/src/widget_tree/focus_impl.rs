// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

impl WidgetTree {
    /// Set focus to a specific widget with the given origin, invoking
    /// `on_focus_lost` / `on_focus_gained` handlers through the
    /// caller-supplied [`WindowOps`](crate::window::WindowOps) sink.
    ///
    /// `teksilo-app` drives in-dispatch focus changes through this method so
    /// that focus-triggered handlers *can* synchronously call
    /// `ctx.open_window(...)`. Standalone callers (programmatic focus from
    /// framework code paths, tests) use
    /// [`focus_with_origin`](Self::focus_with_origin) which wraps with
    /// [`NoopWindowOps`](crate::window::NoopWindowOps).
    ///
    /// **WCAG 3.2.1 (On Focus).** The capability above is a footgun: an
    /// `on_focus` handler that opens a window, navigates, or otherwise changes
    /// context *merely because a control received focus* is a Success Criterion
    /// 3.2.1 failure — keyboard users tabbing through the UI would trigger it
    /// unexpectedly. `on_focus` should only update local visual/reactive state.
    /// A debug-only guard ([`EventContext::open_window`]) warns if a synchronous
    /// context change is attempted from inside focus dispatch.
    pub fn focus_with_origin_ops(
        &mut self,
        id: WidgetId,
        origin: crate::focus::FocusOrigin,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if self.focused == Some(id) {
            return;
        }
        // Debug-only WCAG 3.2.1 guard: flag that we're inside focus dispatch so
        // `EventContext::open_window`/`focus_window` can warn if a focus handler
        // synchronously changes context. RAII-cleared so a panicking handler
        // (in tests) still resets the flag; the guard owns an `Rc` clone so it
        // doesn't borrow `self`.
        struct ClearOnDrop(std::rc::Rc<std::cell::Cell<bool>>);
        impl Drop for ClearOnDrop {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        self.in_focus_dispatch.set(true);
        let _focus_dispatch_guard = ClearOnDrop(self.in_focus_dispatch.clone());

        let previously_focused = self.focused;
        if let Some(old) = self.focused {
            let old_overlay = self.overlay_ancestor_for_widget(old);
            let new_overlay = self.overlay_ancestor_for_widget(id);
            let moving_into_descendant_overlay = match (old_overlay, new_overlay) {
                (Some(old_overlay), Some(new_overlay)) => self
                    .overlay_manager
                    .is_descendant_of(new_overlay, old_overlay),
                _ => false,
            };

            if moving_into_descendant_overlay {
                self.dispatch_to_widget_direct(old, &WidgetEvent::FocusLost, &mut *ops);
            } else {
                self.dispatch_to_widget(old, &WidgetEvent::FocusLost, &mut *ops);
            }
        }
        self.set_focused(Some(id));
        self.focus_origin = Some(origin);
        // `:focus-visible` is one tree-level signal, so the assignment itself
        // has to declare the modality: a direct pointer's focus lands on the
        // release, an event kind the dispatch root's modality sniff does not
        // name, and an assistive `Action::Focus` carries no input event at all.
        // Keying the write on the origin makes "the ring moved" and "focus
        // moved" the same event. `Programmatic` declares nothing — a scripted
        // focus leaves the ring where the user's last real interaction left it,
        // which is what `:focus-visible` does for `element.focus()`.
        if let Some(visible) = origin.focus_visible()
            && self.focus_visible.get() != visible
        {
            self.focus_visible.set(visible);
        }
        self.a11y_dirty = true;
        self.update_focus_within_signals(previously_focused, Some(id));
        self.update_view_focus_signals(previously_focused, Some(id));
        self.dispatch_to_widget(id, &WidgetEvent::FocusGained { origin }, &mut *ops);
        // Non-modal overlays do not contain focus — they follow it out. A menu,
        // popover or dropdown panel the keyboard has walked out of closes here,
        // rather than lingering over the focus ring that left it. Runs after
        // `set_focused` on purpose: `dormant_dismissed_content` re-fires
        // `FocusLost` and clears focus only for a widget *inside* the subtree it
        // parks, and the rule below never dismisses the overlay the new target
        // lives in — so the focus just installed is never disturbed. Running it
        // before the tooltip pass also means that pass sees already-reset
        // `self.tooltips` entries instead of chasing a dismissed overlay id.
        self.dismiss_overlays_left_by_focus(previously_focused, id, &mut *ops);
        // Focus-driven tooltip machinery: close any previously-shown
        // focus-promoted rich tooltip whose scope no longer contains
        // the focus target, then immediately surface+sticky the rich
        // tooltip (if any) attached to the new focus target. See
        // `tooltip_focus_enter` / `tooltip_focus_leave_outside` for
        // the full rationale.
        self.tooltip_focus_leave_outside(Some(id), &mut *ops);
        self.tooltip_focus_enter(id);
        // A pointer press focuses a widget the user just clicked — it is already
        // visible, so auto-scrolling is wrong: it yanks a tall, own-scroll-
        // suppressed editor to its far end on the stale pre-click caret (the
        // pointer sets the real caret *after* focus, and the widget's own
        // caret-chase then keeps it visible). Reveal only for keyboard /
        // programmatic focus, where the newly-focused target may be off-screen.
        if !origin.is_pointer() {
            self.scroll_focused_into_view(id, &mut *ops);
        }
    }

    /// Set focus using [`NoopWindowOps`](crate::window::NoopWindowOps).
    /// Programmatic / framework-internal callers. Handlers triggered
    /// from this path cannot `ctx.open_window(...)`.
    pub fn focus_with_origin(&mut self, id: WidgetId, origin: crate::focus::FocusOrigin) {
        let mut noop = crate::window::NoopWindowOps;
        self.focus_with_origin_ops(id, origin, &mut noop);
    }

    /// After setting focus, ensure the focused widget is visible inside
    /// all ancestor scroll areas (clips_children containers).
    fn scroll_focused_into_view(
        &mut self,
        focused_id: WidgetId,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let focused_bounds = self.arena.bounds(focused_id);
        // Let the focused widget nominate a sub-rectangle to reveal instead of
        // its whole box (a caret line, a selected row). A tall widget — e.g. a
        // `RichTextEditor` grown inside a page `ScrollArea` — would otherwise
        // scroll the page to its bottom on a click that only placed the caret.
        let reveal = self
            .arena
            .get(focused_id)
            .and_then(|node| node.widget.focus_reveal_rect(focused_bounds))
            .unwrap_or(focused_bounds);
        self.scroll_rect_into_view(
            focused_id,
            reveal,
            0.0,
            crate::event::ScrollAlign::Minimal,
            crate::event::ScrollMotion::Instant,
            &mut *ops,
        );
    }

    /// Reveal `rect` — stated in `from`'s own bounds space, which is window
    /// coordinates everywhere except inside a content transform, where it is
    /// that node's content space — inside every
    /// `clips_children` scroll container above `from`, walking strictly
    /// outward (`from` itself is excluded). For each such container whose
    /// viewport does not already contain the margin-expanded rect, dispatch
    /// [`WidgetEvent::ScrollIntoView`] so it adjusts its offset; nested scroll
    /// areas each get a turn (outermost included).
    ///
    /// This is the shared engine behind both the focus follow
    /// ([`scroll_focused_into_view`](Self::scroll_focused_into_view), which
    /// passes the focused widget's own bounds and a zero margin) and the
    /// caller-driven [`EventContext::ensure_visible`](crate::widget::EventContext::ensure_visible),
    /// which passes an arbitrary interior rectangle (a caret, a virtualized
    /// row, a scrolled-off tab header) queued from a handler and drained in
    /// `collect_from_ctx`. Excluding `from` is deliberate: a scrollable widget
    /// is responsible for revealing an interior rect inside its *own*
    /// viewport, so `ensure_visible` only touches the containers enclosing it
    /// — no double-scroll, no feedback loop with the widget's internal follow.
    ///
    /// **Nested scroll areas.** Each handling container reports how far it
    /// scrolled through the `applied_scroll` back-channel on the
    /// [`ScrollIntoView`](crate::event::WidgetEvent::ScrollIntoView) event; the
    /// walk shifts `rect` by the negated delta before asking the next (outer)
    /// container, so the outer targets where the child will land once the
    /// inner's deferred scroll applies — not its pre-scroll position. A handler
    /// that doesn't report a delta (leaves the cell zero) simply gets no
    /// re-targeting, which is exact for the common single-enclosing-scroller
    /// case.
    ///
    /// **Coordinate spaces.** The walk carries `rect` from one ancestor's
    /// space into the next as it climbs, so each container is asked in the
    /// space it can act in and compared against its viewport in the space that
    /// viewport is stated in. Those two differ for a *content* transform (a
    /// fixed window onto moving content, whose own bounds stay in its parent's
    /// space) and coincide for a *self* transform and for the identity case
    /// that is the rest of the tree. Without that bookkeeping a focused card
    /// inside a panned `SceneView` tested as already visible however far the
    /// camera had gone, and anything further out received a rectangle from a
    /// coordinate system it had never heard of.
    ///
    /// **Alignment applies to the innermost scrolling ancestor only.** A
    /// [`ScrollAlign::Fraction`] request names a height in *one* viewport; the
    /// containers further out have their own, differently-sized viewports and no
    /// claim on where the rect should sit inside them, so they fall back to
    /// [`ScrollAlign::Minimal`] — their job is to bring the inner viewport on
    /// screen. A `Fraction` request also bypasses the already-visible gate on
    /// that innermost container: pinning is unconditional by definition, whereas
    /// `Minimal` keeps the "don't scroll what's already visible" behaviour.
    ///
    /// "Scrolling" means a clipping ancestor with an `on_scroll` handler, the
    /// only kind of node `ScrollIntoView` can reach at all. A clipper without
    /// one (a `MaxSize` capping a text column, an animation wrapper) is deaf to
    /// the event, so it neither holds the pin nor spends it: the pin passes
    /// through to the scroller above it. Spending it there would degrade every
    /// pin under such a wrapper to a plain reveal, with nothing reporting it.
    ///
    /// [`ScrollAlign::Fraction`]: crate::event::ScrollAlign::Fraction
    /// [`ScrollAlign::Minimal`]: crate::event::ScrollAlign::Minimal
    pub(super) fn scroll_rect_into_view(
        &mut self,
        from: WidgetId,
        rect: Rect,
        margin: f32,
        align: crate::event::ScrollAlign,
        motion: crate::event::ScrollMotion,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // Shared back-channel: each handling scroll container reports how far it
        // scrolled, so we can shift `rect` for the next (outer) ancestor to
        // where the target will land once the inner's deferred scroll applies.
        // `Arc<Mutex>` (not `Rc<Cell>`) keeps `WidgetEvent: Send`; always
        // uncontended (single-threaded dispatch).
        let applied = std::sync::Arc::new(std::sync::Mutex::new(Point::ZERO));
        let mut rect = rect;
        let mut current = self.arena.parent(from);
        // Consumed by the first scrolling ancestor reached; every one after it
        // reveals minimally.
        let mut pending_align = align;
        while let Some(ancestor_id) = current {
            // The transform between this ancestor's children and its parent.
            // Identity for the overwhelming majority of the tree; a `SceneView`
            // (content) or a `Scale` / `Rotate` (self) makes it real.
            //
            // `rect` arrives in `ancestor_id`'s **content** space — the space
            // its children's bounds are stated in — because that is the space
            // the widget below it was measured in and every step of this walk
            // restores the invariant on its way out. Without that bookkeeping a
            // focused card inside a panned `SceneView` was compared, scene
            // coordinates against window coordinates, with the view's own
            // viewport: a card at scene (100, 100) under a −90 px pan tested as
            // "already visible" and the reveal never fired, and any outer
            // scroller then received a rectangle from a coordinate system it
            // had never heard of.
            let to_parent = self
                .arena
                .get(ancestor_id)
                .and_then(|n| n.transform_prop.as_ref())
                .map(|t| t.get())
                .filter(|t| !t.is_identity());
            let content_transform = self
                .arena
                .get(ancestor_id)
                .is_some_and(|n| n.content_transform);

            // Only a node with an `on_scroll` handler can act on
            // `ScrollIntoView`, so a clipper without one is passed over rather
            // than asked. Asking would reach no handler and change nothing, but
            // it would spend the pin, and every scroller above would then be
            // handed a plain reveal. That is how a `MaxSize` capping a text
            // column turned typewriter scrolling into ordinary caret-following.
            if let Some(node) = self.arena.get(ancestor_id)
                && node.clips_children
                && (node.handlers.on_scroll.is_some() || node.external_handlers.on_scroll.is_some())
            {
                let viewport = node.bounds;
                // Which space the viewport is stated in decides which rectangle
                // it is comparable with. A **content** transform is a fixed
                // window onto moving content, so its own bounds stay in its
                // parent's space and the target has to be projected to meet
                // them. A **self** transform moves with its own bounds, so both
                // are already in the same space.
                let against = match (content_transform, to_parent.as_ref()) {
                    (true, Some(t)) => t.apply_rect(rect),
                    _ => rect,
                };
                let align =
                    std::mem::replace(&mut pending_align, crate::event::ScrollAlign::Minimal);
                // A pin must re-assert itself every time, so it never consults
                // whether the target already happens to be on screen.
                let needs_scroll = matches!(align, crate::event::ScrollAlign::Fraction(_))
                    || against.y - margin < viewport.y
                    || against.bottom() + margin > viewport.bottom()
                    || against.x - margin < viewport.x
                    || against.right() + margin > viewport.right();

                if needs_scroll {
                    *applied.lock().unwrap() = Point::ZERO;
                    // Addressed, not bubbled. `dispatch_to_widget` previews the
                    // event through the target's own ancestors first, which
                    // hands every outer container the *inner* one's reveal —
                    // out of order, before the inner has scrolled, and (now
                    // that a content transform is in the picture) in a
                    // coordinate system that is not the outer container's. This
                    // walk already visits each clipping ancestor in turn, in
                    // its own space and after re-targeting by what the one
                    // inside it actually scrolled, so the preview pass was a
                    // second, worse copy of the same job. Every widget in the
                    // framework that handles `WidgetEvent::ScrollIntoView` —
                    // `ScrollArea` and `SceneView` — clips, so each is reached
                    // by the walk itself either way.
                    self.dispatch_to_widget_direct(
                        ancestor_id,
                        &WidgetEvent::ScrollIntoView {
                            // The handler's own content space, which is the
                            // only one it can act in: a `ScrollArea` subtracts
                            // its viewport origin from this, and a `SceneView`
                            // hands it to `ensure_visible`, which is scene
                            // coordinates by signature.
                            target_bounds: rect,
                            margin,
                            align,
                            motion,
                            applied_scroll: Some(applied.clone()),
                        },
                        &mut *ops,
                    );
                    // The container scrolled its content by `+delta`, moving the
                    // target `-delta` — in the same space the event was stated
                    // in, since that is the space the handler worked in.
                    let delta = *applied.lock().unwrap();
                    if delta != Point::ZERO {
                        rect =
                            Rect::new(rect.x - delta.x, rect.y - delta.y, rect.width, rect.height);
                    }
                }
            }
            // Leaving this ancestor: restore the invariant for the next one up.
            // Both kinds of transform map this node's content into its parent's
            // space — they differ only in whether the node's *own* bounds went
            // with it, which is what the comparison above had to know and this
            // does not.
            if let Some(t) = to_parent {
                rect = t.apply_rect(rect);
            }
            current = self.arena.parent(ancestor_id);
        }
    }

    /// Set focus to a specific widget (programmatic origin, no ops).
    pub fn focus(&mut self, id: WidgetId) {
        self.focus_with_origin(id, crate::focus::FocusOrigin::Programmatic);
    }

    /// Set focus — the dispatch-path variant that threads `ops` through
    /// to any on_focus_lost / on_focus_gained handlers.
    pub fn focus_ops(&mut self, id: WidgetId, ops: &mut dyn crate::window::WindowOps) {
        self.focus_with_origin_ops(id, crate::focus::FocusOrigin::Programmatic, ops);
    }

    /// Get the currently focused widget.
    pub fn focused(&self) -> Option<WidgetId> {
        self.focused
    }

    /// The OS-IME descriptor of the currently focused widget, if it is a
    /// text-input surface. `None` when nothing is focused or the focused
    /// node is not text-editing. The platform layer reads this at
    /// focus-change time to enable/disable the OS input method and pick its
    /// purpose. See [`crate::ime`].
    pub fn ime_context_for_focused(&self) -> Option<crate::ime::ImeContext> {
        self.focused.and_then(|id| self.arena.ime_context(id))
    }

    /// Find the first focusable widget within a subtree, in **traversal
    /// order** — the widget Tab would land on first. Respects nested
    /// `FocusScope`s and scoped `tab_index` (not merely raw DFS order), so a
    /// modal's initial focus matches its Tab order.
    pub fn first_focusable_descendant(&self, root: WidgetId) -> Option<WidgetId> {
        if !self.arena.is_active(root) {
            return None;
        }
        let mut entries = Vec::new();
        self.collect_scope_entries(root, &mut entries);
        sort_scope_entries(&mut entries);
        let scope = ScopeNode {
            policy: crate::focus::TraversalScopePolicy::Cycle,
            entries,
        };
        enter_scope_edge(&scope, false)
    }

    /// Whether the given widget id currently exists and is active in the
    /// tree (not dormant, not destroyed). Callers that need to validate a
    /// user-supplied `WidgetId` before acting on it — e.g. the modal
    /// presentation path validating `ModalRequest::focus_target` — use
    /// this.
    pub fn is_active(&self, id: WidgetId) -> bool {
        self.arena.is_active(id)
    }

    /// Walk the subtree rooted at `id` in depth-first order, returning
    /// the first widget-reported `initial_focus_hint` that resolves to
    /// an active descendant of `id`.
    ///
    /// Used by the modal presentation pipeline to let a deferred-built
    /// content widget (e.g. `MessageBox`) direct focus to a specific
    /// descendant after build — even when wrapped in a surface widget
    /// like `ModalContainer` that doesn't itself know the default
    /// button's id. The framework walks in to find the first hint
    /// under the content root, which is tighter than falling all the
    /// way back to `first_focusable_descendant`.
    ///
    /// Hints pointing at inactive or out-of-subtree ids are ignored;
    /// the walk continues so a shallow wrapper's stale hint doesn't
    /// hide a deeper child's valid one.
    pub fn widget_initial_focus_hint(&self, id: WidgetId) -> Option<WidgetId> {
        if !self.arena.is_active(id) {
            return None;
        }
        if let Some(node) = self.arena.get(id) {
            if let Some(target) = node.widget.initial_focus_hint()
                && self.arena.is_active(target)
                && self.is_descendant_of(target, id)
            {
                return Some(target);
            }
            for &child in &node.children {
                if let Some(found) = self.widget_initial_focus_hint(child) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// How the currently focused widget gained focus.
    pub fn focus_origin(&self) -> Option<crate::focus::FocusOrigin> {
        self.focus_origin
    }

    /// Input-modality "focus-visible" signal: `true` after keyboard input,
    /// `false` after pointer input. Focus rings observe this so they show only
    /// during keyboard navigation. See [`BuildContext::focus_visible`](crate::BuildContext::focus_visible).
    pub fn focus_visible_signal(&self) -> crate::signal::Signal<bool> {
        self.focus_visible.clone()
    }

    /// Cycle focus to the next/previous focusable widget (Tab/Shift-Tab),
    /// honoring nested **traversal scopes** (`FocusScope`).
    ///
    /// Builds a scope tree on demand (depth-first; `tab_index` scoped per
    /// scope), then walks it with [`navigate_scope`]. The root is an implicit
    /// `Cycle` scope (whole-tree last↔first wrap). A centered modal overlay
    /// folds into the same mechanism: its content subtree becomes the root
    /// `Cycle` scope, so Tab is confined to the modal.
    pub(super) fn cycle_focus(&mut self, reverse: bool, ops: &mut dyn crate::window::WindowOps) {
        let mut entries = Vec::new();
        if let Some(modal_overlay) = self.overlay_manager.topmost_centered() {
            let content_id = modal_overlay.content_id;
            self.collect_scope_entries(content_id, &mut entries);
        } else {
            let roots = self.arena.roots();
            for root in roots {
                // Tooltip surfaces are spliced in below, next to the anchor
                // they belong to — never collected as bare roots. A tooltip's
                // content is `ctx.add`-ed parentless, so collecting it here put
                // it in the Tab cycle at whatever position it happened to be
                // inserted at: `sort_scope_entries` orders by explicit
                // `tab_index` only, and entries without one compare `Equal`,
                // so a stable sort leaves insertion order to decide. That made
                // a tooltip's slot in the cycle an emergent property of build
                // order rather than of the control it describes.
                if self.tooltip_content_root(root).is_some() {
                    continue;
                }
                self.collect_scope_entries(root, &mut entries);
            }
        }
        sort_scope_entries(&mut entries);
        self.splice_sticky_tooltips_after_anchors(&mut entries);
        let root_scope = ScopeNode {
            policy: crate::focus::TraversalScopePolicy::Cycle,
            entries,
        };

        if let StepResult::Found(next_id) = navigate_scope(&root_scope, self.focused, reverse, true)
        {
            self.focus_with_origin_ops(next_id, crate::focus::FocusOrigin::Keyboard, &mut *ops);
        }
        // `Escaped` only reaches here for an empty tree (the root Cycle scope
        // wraps at its ends) — nothing to focus, so leave focus unchanged.
    }

    /// The tooltip entry whose content root is `id`, if any.
    fn tooltip_content_root(&self, id: WidgetId) -> Option<usize> {
        self.tooltips.iter().position(|e| e.content_id == id)
    }

    /// Place every **sticky** tooltip surface immediately after the entry
    /// holding its anchor, and leave every non-sticky one out entirely.
    ///
    /// Two rules, one place:
    ///
    /// * An *unpromoted* tip is informational. It appeared because the pointer
    ///   paused or focus arrived — not because the user asked to enter it — so
    ///   it takes no Tab stop, matching the ARIA tooltip pattern (its text
    ///   reaches assistive tech through the anchor's description instead).
    /// * A *promoted* one was earned, by a 2 s dwell in either modality, and
    ///   its whole point is that its content is reachable. It belongs directly
    ///   after the control it describes, the way a disclosure's panel follows
    ///   its button — never at some position decided by arena insertion order.
    ///
    /// Anchors nested inside a traversal scope are handled by locating the
    /// top-level entry that *contains* the anchor, so the panel still lands
    /// immediately after that whole group rather than being dropped.
    fn splice_sticky_tooltips_after_anchors(&self, entries: &mut Vec<ScopeEntry>) {
        let sticky: Vec<(WidgetId, WidgetId)> = self
            .tooltips
            .iter()
            .filter(|e| e.is_sticky && e.overlay_id.is_some())
            .map(|e| (e.anchor_id, e.content_id))
            .collect();

        for (anchor_id, content_id) in sticky {
            let mut panel = Vec::new();
            self.collect_scope_entries(content_id, &mut panel);
            if panel.is_empty() {
                continue;
            }
            sort_scope_entries(&mut panel);

            // The anchor is often not itself the Tab stop: composing controls
            // (`Button`) keep focus on their outer node and attach the tip to
            // an inner body root, so resolve to the focusable that actually
            // appears in the cycle before looking for its entry.
            let stop = self
                .find_focusable_at_or_above(anchor_id)
                .unwrap_or(anchor_id);
            let at = entries
                .iter()
                .position(|entry| scope_entry_contains(entry, stop))
                // An anchor with no Tab stop above it at all (a plain container
                // that merely carries a tip) has no entry to follow; put the
                // panel at the end rather than dropping it, so its content
                // stays reachable.
                .map_or(entries.len(), |i| i + 1);

            for (offset, item) in panel.into_iter().enumerate() {
                entries.insert(at + offset, item);
            }
        }
    }

    /// Whether `id` participates in Tab traversal, honoring a `tab_stop`
    /// flag set anywhere on its ancestor chain. Walks up to the nearest
    /// node (including `id`) carrying an explicit `tab_stop` prop and
    /// returns its current value; defaults to `true` when no ancestor
    /// constrains it. This makes `set_tab_stop` on a composite control
    /// (whose focusable node is an inner leaf) govern the whole subtree —
    /// the basis of the roving-tabindex pattern in `Toolbar` / `TabBar`.
    pub(super) fn tab_stop_effective(&self, id: WidgetId) -> bool {
        let mut current = Some(id);
        while let Some(cur) = current {
            let Some(node) = self.arena.get(cur) else {
                break;
            };
            if let Some(prop) = node.tab_stop.as_ref() {
                return prop.get();
            }
            current = node.parent;
        }
        true
    }

    /// Check if a node is focusable (set via HandlerSet `.focusable(true)` in build).
    pub(super) fn is_node_focusable(&self, node: &crate::arena::WidgetNode) -> bool {
        node.node_focusable.unwrap_or(false)
    }

    /// Find the nearest focusable widget at or above the given ID.
    pub(super) fn find_focusable_at_or_above(&self, id: WidgetId) -> Option<WidgetId> {
        let mut current = Some(id);
        while let Some(current_id) = current {
            if let Some(node) = self.arena.get(current_id)
                && self.is_node_focusable(node)
            {
                return Some(current_id);
            }
            current = self.arena.parent(current_id);
        }
        None
    }

    /// Collect the traversal entries of the subtree rooted at `id`, in
    /// depth-first (document) order, into the current scope's `out` list.
    ///
    /// - A node carrying `node_traversal_scope` becomes a single
    ///   [`ScopeEntryKind::Scope`] — its descendants are collected into a *nested*
    ///   ordered list and the recursion does not flow past it at this level.
    ///   (The scope node itself is never a focusable; the `FocusScope` wrapper
    ///   forces `node_focusable = false`.)
    /// - Any other node that is focusable and an effective Tab stop becomes a
    ///   [`ScopeEntryKind::Focusable`].
    ///
    /// Disabled subtrees and dormant/destroyed nodes are skipped entirely, as
    /// in the previous flat collector. The `tab_stop` ancestor-walk
    /// (`tab_stop_effective`) is applied here at collection time rather than as
    /// a post-pass `retain`.
    fn collect_scope_entries(&self, id: WidgetId, out: &mut Vec<ScopeEntry>) {
        if !self.arena.is_active(id) {
            return;
        }
        let Some(node) = self.arena.get(id) else {
            return;
        };
        if node
            .enabled_state
            .as_ref()
            .map(|s| !s.get())
            .unwrap_or(false)
        {
            return;
        }

        // A traversal-scope boundary: collect its subtree as an independent,
        // internally-ordered group; do not descend past it into `out`.
        if let Some(policy) = node.node_traversal_scope {
            let mut child_entries = Vec::new();
            for &child in &node.children {
                self.collect_scope_entries(child, &mut child_entries);
            }
            sort_scope_entries(&mut child_entries);
            out.push(ScopeEntry {
                tab_index: node.node_tab_index,
                kind: ScopeEntryKind::Scope(ScopeNode {
                    policy,
                    entries: child_entries,
                }),
            });
            return;
        }

        // A normal node: a Tab stop iff focusable and not tab_stop-suppressed.
        if self.is_node_focusable(node) && self.tab_stop_effective(id) {
            out.push(ScopeEntry {
                tab_index: node.node_tab_index,
                kind: ScopeEntryKind::Focusable(id),
            });
        }
        for &child in &node.children {
            self.collect_scope_entries(child, out);
        }
    }

    /// Build the chain of strict ancestors of `id` (i.e. starting at
    /// the parent of `id`, walking up to a root). Returns an empty
    /// vector when `id` is `None` or has no parent. Used by the
    /// `focus_within` / `hover_within` chain-diff helpers.
    pub(super) fn strict_ancestors_of(&self, id: Option<WidgetId>) -> Vec<WidgetId> {
        let mut chain = Vec::new();
        if let Some(start) = id {
            let mut current = self.arena.parent(start);
            while let Some(parent) = current {
                chain.push(parent);
                current = self.arena.parent(parent);
            }
        }
        chain
    }

    /// Update every `focus_within_signal` whose owning node moved
    /// in or out of the focused widget's strict-ancestor chain
    /// between `old` and `new`. Strict ancestors only — the
    /// focused widget's own signal (if any) is never written.
    pub(crate) fn update_focus_within_signals(
        &mut self,
        old: Option<WidgetId>,
        new: Option<WidgetId>,
    ) {
        let old_chain = self.strict_ancestors_of(old);
        let new_chain = self.strict_ancestors_of(new);
        // Nodes leaving the chain → false.
        for &id in &old_chain {
            if !new_chain.contains(&id)
                && let Some(node) = self.arena.get(id)
                && let Some(sig) = node.focus_within_signal.clone()
            {
                sig.set(false);
            }
        }
        // Nodes entering the chain → true.
        for &id in &new_chain {
            if !old_chain.contains(&id)
                && let Some(node) = self.arena.get(id)
                && let Some(sig) = node.focus_within_signal.clone()
            {
                sig.set(true);
            }
        }
    }

    /// Inclusive ancestor chain of `id`: `id` itself, then its strict
    /// ancestors. Empty when `id` is `None`.
    pub(super) fn inclusive_ancestors_of(&self, id: Option<WidgetId>) -> Vec<WidgetId> {
        let mut chain = Vec::new();
        if let Some(start) = id {
            chain.push(start);
            chain.extend(self.strict_ancestors_of(Some(start)));
        }
        chain
    }

    /// Mirror of [`update_focus_within_signals`](Self::update_focus_within_signals)
    /// for `view_focus_signal`, using *inclusive* ancestor chains — so a node
    /// that is itself the focused widget (e.g. a data view holding focus
    /// directly) sees its own scope signal flip `true`.
    pub(crate) fn update_view_focus_signals(
        &mut self,
        old: Option<WidgetId>,
        new: Option<WidgetId>,
    ) {
        let old_chain = self.inclusive_ancestors_of(old);
        let new_chain = self.inclusive_ancestors_of(new);
        for &id in &old_chain {
            if !new_chain.contains(&id)
                && let Some(node) = self.arena.get(id)
                && let Some(sig) = node.view_focus_signal.clone()
            {
                sig.set(false);
            }
        }
        for &id in &new_chain {
            if !old_chain.contains(&id)
                && let Some(node) = self.arena.get(id)
                && let Some(sig) = node.view_focus_signal.clone()
            {
                sig.set(true);
            }
        }
    }

    /// Get-or-create the `view_focus_signal` on `node_id`, initialised to the
    /// node's current focus-containment (`focused` is `node_id` or a descendant).
    pub(crate) fn view_focus_signal_for(
        &mut self,
        node_id: WidgetId,
    ) -> crate::signal::Signal<bool> {
        if let Some(node) = self.arena.get(node_id)
            && let Some(sig) = node.view_focus_signal.clone()
        {
            return sig;
        }
        let active = self.inclusive_ancestors_of(self.focused).contains(&node_id);
        let sig = crate::signal::Signal::new(active);
        if let Some(node) = self.arena.get_mut(node_id) {
            node.view_focus_signal = Some(sig.clone());
        }
        sig
    }

    /// Reactive signal that is `true` when the nearest focusable ancestor of
    /// `node_id` (its "focus scope" — e.g. the enclosing data view) holds
    /// keyboard focus. With no focusable ancestor, returns a constant-`true`
    /// signal so selection renders active (the legacy behaviour for items
    /// outside any focus scope). Drives focus-aware selection in `StandardItem`.
    pub fn view_focus_active_for(&mut self, node_id: WidgetId) -> crate::signal::Signal<bool> {
        match self.find_focusable_at_or_above(node_id) {
            Some(scope) => self.view_focus_signal_for(scope),
            None => crate::signal::Signal::new(true),
        }
    }

    /// Push `node_id`'s focus scope onto the build-time scope stack (creating its
    /// `view_focus_signal` if absent) so descendants built before arena
    /// parenting is wired (docked / virtualized rows) still read the correct
    /// view focus. Pair with [`end_view_focus`](Self::end_view_focus).
    pub fn begin_view_focus(&mut self, node_id: WidgetId) -> crate::signal::Signal<bool> {
        let sig = self.view_focus_signal_for(node_id);
        self.view_focus_stack.push(sig.clone());
        sig
    }

    /// Pop the innermost focus scope pushed by [`begin_view_focus`](Self::begin_view_focus).
    pub fn end_view_focus(&mut self) {
        self.view_focus_stack.pop();
    }

    /// The innermost active build-time focus scope, if any.
    pub fn current_view_focus(&self) -> Option<crate::signal::Signal<bool>> {
        self.view_focus_stack.last().cloned()
    }

    /// Single point of mutation for the hovered widget. Writes it onto the
    /// **hover owner**'s entry in the pointer table and updates the
    /// externally-observable Signal so debug tooling (the inspector's hover
    /// tooltip) doesn't have to poll.
    ///
    /// Hover is hover-owner-only, so this is a no-op on the table side when no
    /// hovering-capable pointer is live — a touch-only device has nothing that
    /// hovers, and writing a contact's target here would make every hover
    /// affordance fire on tap. The signal is still kept honest.
    ///
    /// Does **not** call `update_hover_within_signals` — call sites remain in
    /// charge of dispatching enter/leave because some sites (e.g. post-layout
    /// hover recovery) intentionally skip it.
    pub(crate) fn set_hovered(&mut self, value: Option<WidgetId>) {
        if let Some(entry) = self.pointers.hover_owner_mut() {
            entry.hovered = value;
        }
        if self.hovered_signal.get() != value {
            self.hovered_signal.set(value);
        }
    }

    /// Single point of mutation for `self.focused`. Mirror of
    /// `set_hovered` for the focused chain. Drives the inspector's
    /// Focus tab without requiring the tab to poll. Does not touch
    /// `focus_origin` or focus-within signals — call sites remain
    /// responsible for those (the bookkeeping varies by mutation
    /// path, e.g. focus loss vs. arena destruction).
    pub(crate) fn set_focused(&mut self, value: Option<WidgetId>) {
        self.focused = value;
        if self.focused_signal.get() != value {
            self.focused_signal.set(value);
        }
    }

    /// Mirror of [`update_focus_within_signals`](Self::update_focus_within_signals)
    /// for the hovered chain.
    pub(crate) fn update_hover_within_signals(
        &mut self,
        old: Option<WidgetId>,
        new: Option<WidgetId>,
    ) {
        let old_chain = self.strict_ancestors_of(old);
        let new_chain = self.strict_ancestors_of(new);
        // Record the chain on the hover owner's entry: it is the pointer that
        // put those signals to `true`, so it is the one whose teardown (and
        // whose post-rebuild scrub) has to know about them.
        if let Some(entry) = self.pointers.hover_owner_mut() {
            entry.hover_within = new_chain.clone();
        }
        for &id in &old_chain {
            if !new_chain.contains(&id)
                && let Some(node) = self.arena.get(id)
                && let Some(sig) = node.hover_within_signal.clone()
            {
                sig.set(false);
            }
        }
        for &id in &new_chain {
            if !old_chain.contains(&id)
                && let Some(node) = self.arena.get(id)
                && let Some(sig) = node.hover_within_signal.clone()
            {
                sig.set(true);
            }
        }
    }
}

// ─── Traversal scope tree ────────────────────────────────────────────────
//
// `cycle_focus` builds a transient tree of these on each Tab press. A
// `ScopeNode` is an ordered list of `ScopeEntry`s; an entry is either a
// focusable leaf or a nested `ScopeNode`. Within a node, entries are ordered
// by `tab_index` (scoped — only compared among siblings) then DFS order.
// `navigate_scope` walks this tree, applying each scope's `policy` at its ends.

/// One member of a traversal scope: a focusable leaf or a nested scope, plus
/// the `tab_index` used to position it among its siblings (`None` sorts last,
/// preserving DFS order).
struct ScopeEntry {
    tab_index: Option<i32>,
    kind: ScopeEntryKind,
}

enum ScopeEntryKind {
    Focusable(WidgetId),
    Scope(ScopeNode),
}

/// An ordered group of entries with a boundary policy.
struct ScopeNode {
    policy: crate::focus::TraversalScopePolicy,
    entries: Vec<ScopeEntry>,
}

/// Outcome of stepping within a scope.
enum StepResult {
    /// Focus should move to this widget.
    Found(WidgetId),
    /// Tab ran off this scope's end and the policy permits leaving — the
    /// caller (parent scope) should continue stepping from this scope's slot.
    /// Never produced by a `Cycle` scope or the root scope (they wrap).
    Escaped,
}

/// Whether a scope wraps (vs. lets focus escape) when Tab hits its boundary.
/// The root scope always wraps; otherwise only `Cycle` scopes do.
fn scope_wraps(policy: crate::focus::TraversalScopePolicy, is_root: bool) -> bool {
    is_root || matches!(policy, crate::focus::TraversalScopePolicy::Cycle)
}

/// Sort entries by scoped `tab_index`: `Some` before `None` (ascending);
/// stable, so the DFS order is preserved within each group.
fn sort_scope_entries(entries: &mut [ScopeEntry]) {
    entries.sort_by(|a, b| match (a.tab_index, b.tab_index) {
        (Some(ia), Some(ib)) => ia.cmp(&ib),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
}

/// Whether the focused widget `f` lies anywhere within `entry`.
fn scope_entry_contains(entry: &ScopeEntry, f: WidgetId) -> bool {
    match &entry.kind {
        ScopeEntryKind::Focusable(id) => *id == f,
        ScopeEntryKind::Scope(child) => child.entries.iter().any(|e| scope_entry_contains(e, f)),
    }
}

/// First focusable when entering `scope` from its leading (forward) or
/// trailing (reverse) edge, descending into nested scopes and skipping empty
/// ones. `None` if the scope holds no focusable members.
fn enter_scope_edge(scope: &ScopeNode, reverse: bool) -> Option<WidgetId> {
    let n = scope.entries.len();
    for k in 0..n {
        let idx = if reverse { n - 1 - k } else { k };
        match &scope.entries[idx].kind {
            ScopeEntryKind::Focusable(id) => return Some(*id),
            ScopeEntryKind::Scope(child) => {
                if let Some(id) = enter_scope_edge(child, reverse) {
                    return Some(id);
                }
            }
        }
    }
    None
}

/// Move focus to the next/previous focusable within `scope`, recursing into
/// the nested scope that currently holds focus before stepping to siblings.
/// `is_root` marks the implicit top scope (always wraps, never escapes).
fn navigate_scope(
    scope: &ScopeNode,
    focused: Option<WidgetId>,
    reverse: bool,
    is_root: bool,
) -> StepResult {
    if scope.entries.is_empty() {
        return StepResult::Escaped;
    }

    let cur = focused.and_then(|f| {
        scope
            .entries
            .iter()
            .position(|e| scope_entry_contains(e, f))
    });

    let from = match cur {
        None => {
            // Focus is not in this scope (root with nothing focused, or the
            // focused widget was destroyed): enter from the edge.
            return match enter_scope_edge(scope, reverse) {
                Some(id) => StepResult::Found(id),
                None => StepResult::Escaped,
            };
        }
        Some(i) => i,
    };

    // Focus is inside a nested scope: try to advance within it first.
    // (Escaped — fall through and step to the sibling after this scope.)
    if let ScopeEntryKind::Scope(child) = &scope.entries[from].kind
        && let StepResult::Found(id) = navigate_scope(child, focused, reverse, false)
    {
        return StepResult::Found(id);
    }

    step_to_sibling(scope, from, reverse, is_root)
}

/// Step from entry `from` to the next/previous *non-empty* sibling, applying
/// the boundary policy (wrap vs. escape) and entering the chosen entry from
/// its near edge. Bounded to one full lap.
fn step_to_sibling(scope: &ScopeNode, from: usize, reverse: bool, is_root: bool) -> StepResult {
    let n = scope.entries.len();
    let mut idx = from;
    for _ in 0..n {
        let next = if reverse {
            if idx == 0 {
                if scope_wraps(scope.policy, is_root) {
                    n - 1
                } else {
                    return StepResult::Escaped;
                }
            } else {
                idx - 1
            }
        } else if idx + 1 >= n {
            if scope_wraps(scope.policy, is_root) {
                0
            } else {
                return StepResult::Escaped;
            }
        } else {
            idx + 1
        };

        match &scope.entries[next].kind {
            ScopeEntryKind::Focusable(id) => return StepResult::Found(*id),
            ScopeEntryKind::Scope(child) => {
                if let Some(id) = enter_scope_edge(child, reverse) {
                    return StepResult::Found(id);
                }
                // Empty nested scope: keep stepping past it.
                idx = next;
            }
        }
    }
    StepResult::Escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;
    use crate::widget_builder::WidgetBuilder;

    /// **A reveal walks the rect's owner's ancestors, not the asker's.**
    ///
    /// The default — walk from whoever handled the event — is right whenever a widget
    /// reveals something inside itself, and silently wrong the moment the rect belongs
    /// elsewhere. A find banner's Next button sits *beside* the scrolling page, so a walk
    /// from the button climbs out through the banner and never meets the scroll container
    /// the match is in: the match is selected, the counter moves, and the viewport does
    /// not follow. Naming the owner is what fixes that, and this pins both halves.
    #[test]
    fn a_reveal_walks_the_ancestors_of_whoever_owns_the_rect() {
        use crate::event::{EventResponse, ScrollAlign, ScrollMotion};
        use std::cell::Cell;
        use std::rc::Rc;

        let asked = Rc::new(Cell::new(0usize));
        let mut tree = WidgetTree::new();
        // A clipping container that records every reveal it is asked for.
        let viewport = {
            let asked = asked.clone();
            tree.add(
                // `on_scroll` first: `clips_children` is also a `Widget` trait *method*,
                // so calling it on the bare widget resolves to the getter.
                FillWidget::new()
                    .on_scroll(move |event, _ctx| {
                        if matches!(event, WidgetEvent::ScrollIntoView { .. }) {
                            asked.set(asked.get() + 1);
                        }
                        EventResponse::Ignored
                    })
                    .clips_children(true),
            )
        };
        let inside = tree.add_child(viewport, FillWidget::new());
        // The "button": a sibling of the viewport, not a descendant — the banner's shape.
        let button = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Far below the viewport, so a container that hears about it must scroll.
        let target = Rect::new(0.0, 5_000.0, 10.0, 10.0);
        let mut ops = crate::window::NoopWindowOps;

        tree.scroll_rect_into_view(
            button,
            target,
            0.0,
            ScrollAlign::Minimal,
            ScrollMotion::Instant,
            &mut ops,
        );
        assert_eq!(
            asked.get(),
            0,
            "walking from the asker never reaches a viewport it is not inside"
        );

        tree.scroll_rect_into_view(
            inside,
            target,
            0.0,
            ScrollAlign::Minimal,
            ScrollMotion::Instant,
            &mut ops,
        );
        assert_eq!(
            asked.get(),
            1,
            "walking from the rect's owner reaches the viewport enclosing it"
        );
    }

    /// A `clips_children` container around `child`, recording every reveal it
    /// is asked for and the rectangle it was asked with.
    fn recorder(
        tree: &mut WidgetTree,
        child: WidgetId,
        seen: std::rc::Rc<std::cell::RefCell<Vec<Rect>>>,
    ) -> WidgetId {
        use crate::event::EventResponse;
        use crate::test_widgets::StackWidget;
        tree.add(
            StackWidget::new()
                .child(child)
                .on_scroll(move |event, _ctx| {
                    if let WidgetEvent::ScrollIntoView { target_bounds, .. } = event {
                        seen.borrow_mut().push(*target_bounds);
                    }
                    EventResponse::Ignored
                })
                .clips_children(true),
        )
    }

    /// **The reveal walk crosses a content transform.**
    ///
    /// A `SceneView` is a fixed viewport over content in a coordinate system of
    /// its own: a card at scene (100, 100) keeps arena bounds of (100, 100)
    /// however far the camera has panned. The walk used to compare that
    /// rectangle directly against the view's *window* viewport and conclude
    /// there was nothing to reveal — which is why focus-follow inside an
    /// embedded editor never fired at a non-zero pan, and why anything further
    /// out then received a rectangle from a coordinate system it had never
    /// heard of.
    #[test]
    fn a_reveal_projects_the_rect_as_it_climbs_out_of_a_content_transform() {
        use crate::event::{ScrollAlign, ScrollMotion};
        use std::cell::RefCell;
        use std::rc::Rc;
        use teksilo_canvas::Transform2D;

        let outer_seen: Rc<RefCell<Vec<Rect>>> = Rc::new(RefCell::new(Vec::new()));
        let view_seen: Rc<RefCell<Vec<Rect>>> = Rc::new(RefCell::new(Vec::new()));

        let mut tree = WidgetTree::new();
        let card = tree.add(FillWidget::new());
        let view = recorder(&mut tree, card, view_seen.clone());
        let _outer = recorder(&mut tree, view, outer_seen.clone());
        tree.layout(SizeProposal::exact(400.0, 300.0));
        // The camera: panned 90 px left, so scene x = 100 paints at window
        // x = 10 — inside the viewport, and not where the arena says it is.
        tree.set_content_transform(view, Transform2D::translate(-90.0, 0.0));

        let card_rect = Rect::new(100.0, 100.0, 50.0, 50.0);
        let mut ops = crate::window::NoopWindowOps;
        tree.scroll_rect_into_view(
            card,
            card_rect,
            0.0,
            ScrollAlign::Minimal,
            ScrollMotion::Instant,
            &mut ops,
        );

        assert!(
            view_seen.borrow().is_empty(),
            "the card paints at window x = 10, inside the 400×300 viewport — \
             comparing its scene rect against the viewport is what used to \
             produce a spurious verdict here, in either direction"
        );
        assert!(
            outer_seen.borrow().is_empty(),
            "and nothing outside it has a reason to scroll either"
        );

        // Now pan so the card is genuinely off-screen to the left. Its arena
        // bounds have not moved — only the camera has — so a walk that ignores
        // the transform still sees a card at (100, 100) and does nothing.
        tree.set_content_transform(view, Transform2D::translate(-600.0, 0.0));
        tree.scroll_rect_into_view(
            card,
            card_rect,
            0.0,
            ScrollAlign::Minimal,
            ScrollMotion::Instant,
            &mut ops,
        );
        assert_eq!(
            view_seen.borrow().as_slice(),
            &[card_rect],
            "the view is asked to reveal the card, in the view's own content \
             space — the space `SceneView::ensure_visible` takes by signature"
        );
        assert_eq!(
            outer_seen.borrow().as_slice(),
            &[Rect::new(-500.0, 100.0, 50.0, 50.0)],
            "and everything further out is asked in window space, which is the \
             only space it can compare against its own viewport"
        );
    }

    /// The control for the test above: with no transform in the chain the walk
    /// is unchanged, which is the case the whole rest of the framework is.
    #[test]
    fn a_reveal_with_no_transform_in_the_chain_passes_the_rect_through() {
        use crate::event::{ScrollAlign, ScrollMotion};
        use std::cell::RefCell;
        use std::rc::Rc;

        let seen: Rc<RefCell<Vec<Rect>>> = Rc::new(RefCell::new(Vec::new()));
        let mut tree = WidgetTree::new();
        let inside = tree.add(FillWidget::new());
        let _viewport = recorder(&mut tree, inside, seen.clone());
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let target = Rect::new(0.0, 5_000.0, 10.0, 10.0);
        let mut ops = crate::window::NoopWindowOps;
        tree.scroll_rect_into_view(
            inside,
            target,
            0.0,
            ScrollAlign::Minimal,
            ScrollMotion::Instant,
            &mut ops,
        );
        assert_eq!(seen.borrow().as_slice(), &[target]);
    }

    #[test]
    fn focus_widget() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);
        assert_eq!(tree.focused(), Some(widget));
    }

    #[test]
    fn focus_change() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(a);
        assert_eq!(tree.focused(), Some(a));
        tree.focus(b);
        assert_eq!(tree.focused(), Some(b));
    }

    /// A container that rebuilds its focusable children whenever `epoch` bumps
    /// — the shape of every data-driven view: a `ListView` on a model update, a
    /// popover re-scanning its content when it opens.
    #[derive(Debug)]
    struct RebuildingRows {
        epoch: crate::signal::Signal<u64>,
        rows: Vec<WidgetId>,
    }

    impl Widget for RebuildingRows {
        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            let sid = ctx.self_id();
            let reg = ctx.binding_registry();
            self.epoch
                .bind_to(sid, reg, crate::binding::BindingLevel::Rebuild);
            self.rows = (0..3)
                .map(|_| ctx.add(FillWidget::new().focusable()))
                .collect();
            self.rows.clone()
        }

        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn children(&self) -> Vec<WidgetId> {
            self.rows.clone()
        }
    }

    /// A rebuild must keep focus **inside the subtree that owned it**.
    ///
    /// A rebuild destroys its children and allocates fresh `WidgetId`s, so the
    /// focused node dies. Dropping focus to `None` there (what
    /// `revalidate_interaction_state` does with any dead focus) kicks the user
    /// clean out of the widget they were in — most visibly, a popover that
    /// refreshes its content when it opens would throw away the very row the
    /// popover had just focused, and the menu would come up with no keyboard
    /// focus at all: no arrow keys, no Enter.
    #[test]
    fn a_rebuild_keeps_focus_inside_the_subtree_that_had_it() {
        let mut tree = WidgetTree::new();
        let epoch = crate::signal::Signal::new(0u64);
        let root = tree.add(RebuildingRows {
            epoch: epoch.clone(),
            rows: Vec::new(),
        });
        tree.layout(SizeProposal::exact(100.0, 60.0));

        let first_row = tree
            .first_focusable_descendant(root)
            .expect("the rows are focusable");
        tree.focus(first_row);
        assert_eq!(tree.focused(), Some(first_row));

        // Rebuild: every row is destroyed and re-allocated.
        epoch.set(1);
        tree.layout(SizeProposal::exact(100.0, 60.0));

        let focused = tree
            .focused()
            .expect("a rebuild must not drop focus out of the rebuilt subtree");
        assert_ne!(
            focused, first_row,
            "the old row is dead — focus must have moved to a freshly built one"
        );
        assert!(
            tree.is_descendant_of(focused, root),
            "focus must land back inside the rebuilt subtree"
        );
        assert!(tree.is_active(focused), "the focused node must be live");
    }

    /// The restore is scoped: a rebuild that did *not* own focus must leave
    /// focus exactly where it was, rather than yanking it into the rebuilt
    /// subtree. Otherwise any background list update would steal the caret out
    /// of whatever the user was typing in.
    #[test]
    fn a_rebuild_elsewhere_does_not_steal_focus() {
        let mut tree = WidgetTree::new();
        let epoch = crate::signal::Signal::new(0u64);
        let outsider = tree.add(FillWidget::new().focusable());
        let _rows = tree.add(RebuildingRows {
            epoch: epoch.clone(),
            rows: Vec::new(),
        });
        tree.layout(SizeProposal::exact(100.0, 60.0));

        tree.focus(outsider);
        assert_eq!(tree.focused(), Some(outsider));

        epoch.set(1);
        tree.layout(SizeProposal::exact(100.0, 60.0));

        assert_eq!(
            tree.focused(),
            Some(outsider),
            "a rebuild that never held focus must not pull focus into itself"
        );
    }

    #[test]
    fn first_focusable_descendant_prefers_first_focusable_child() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let _not_focusable = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new().focusable());
        let root = tree.add(crate::test_widgets::StackWidget::new().child(a).child(b));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.first_focusable_descendant(root), Some(a));
    }

    #[test]
    fn tab_cycles_through_focusable_widgets() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        let c = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.focused(), None);

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(b));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(c));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));
    }

    #[test]
    fn tab_stop_on_composite_excludes_focusable_descendant() {
        // The roving-tabindex case: a composite control (here a StackWidget
        // standing in for ComboBox / IconButton) carries the `tab_stop` flag
        // on its composing node, but its real focusable node is an inner
        // leaf. Suppressing the composite must remove that leaf from Tab.
        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new().focusable());
        let composite = tree.add(crate::test_widgets::StackWidget::new().child(leaf));
        let other = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.set_tab_stop(composite, false);

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(
            tree.focused(),
            Some(other),
            "Tab must skip the suppressed composite's inner leaf"
        );
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(
            tree.focused(),
            Some(other),
            "only the un-suppressed control participates in Tab"
        );

        // Re-enabling the composite brings its inner leaf back into Tab.
        tree.set_tab_stop(composite, true);
        tree.set_focused(None);
        tree.press_key(Key::Tab, Modifiers::NONE);
        let first = tree.focused();
        tree.press_key(Key::Tab, Modifiers::NONE);
        let second = tree.focused();
        assert_ne!(
            first, second,
            "with the composite re-enabled, Tab visits both controls"
        );
    }

    #[test]
    fn shift_tab_cycles_backwards() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        let c = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));

        tree.press_key(Key::Tab, Modifiers::SHIFT);
        assert_eq!(tree.focused(), Some(c));

        tree.press_key(Key::Tab, Modifiers::SHIFT);
        assert_eq!(tree.focused(), Some(b));
    }

    #[test]
    fn tab_skips_non_focusable_widgets() {
        let mut tree = WidgetTree::new();
        let _not_focusable = tree.add(FillWidget::new());
        let a = tree.add(FillWidget::new().focusable());
        let _also_not = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(b));
    }

    /// `focus` is a command, not a query: it moves focus to the node it is
    /// handed without asking whether traversal could ever land there.
    ///
    /// This is why a test claiming **keyboard reachability** has to read the
    /// traversal graph — `tab_stops_within` — rather than focus its subject and
    /// press a key. The latter is green for a control no keyboard user can
    /// reach, which is how a widget once lost `focusable(true)` with all six of
    /// its tests still passing. If this behaviour ever grows a guard, the
    /// reachability recipe in `docs/a11y/non-drag-alternatives.md` and the
    /// comments citing it are what to revisit.
    #[test]
    fn focus_does_not_check_that_the_node_is_focusable() {
        let mut tree = WidgetTree::new();
        let inert = tree.add(FillWidget::new()); // no `.focusable()`
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(
            tree.tab_stops_within(inert).is_empty(),
            "the subject has to be unreachable for the point to be made"
        );

        tree.focus(inert);
        assert_eq!(
            tree.focused(),
            Some(inert),
            "focus lands on a node Tab traversal can never offer"
        );
    }

    #[test]
    fn tab_focus_has_keyboard_origin() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(
            tree.focus_origin(),
            Some(crate::focus::FocusOrigin::Keyboard)
        );
    }

    #[test]
    fn tab_skips_focusable_inside_disabled_ancestor() {
        use crate::signal::Signal;
        use crate::test_widgets::StackWidget;

        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let inner = tree.add(FillWidget::new().focusable());
        let disabled_container = tree.add(StackWidget::new().child(inner));
        let c = tree.add(FillWidget::new().focusable());
        tree.enabled_when(disabled_container, Signal::new(false));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(
            tree.focused(),
            Some(c),
            "tab should skip the focusable widget nested inside the disabled container"
        );
    }

    #[test]
    fn dormant_widget_not_in_focus_cycle() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        let c = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.focus(a);
        tree.set_dormant(b);

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(c));
    }

    /// Parking a focused widget dormant (Switcher / `visible_when`) must
    /// deliver `FocusLost` so the widget clears local focus state. Without
    /// that, a rich-text editor keeps `has_focus` and schedules caret wakes
    /// forever — the multi-tab CPU creep Skribisto hit on rapid tab switches.
    #[test]
    fn revalidate_delivers_focus_lost_when_focused_widget_goes_dormant() {
        use std::cell::Cell;
        use std::rc::Rc;

        let lost = Rc::new(Cell::new(0_u32));
        let gained = Rc::new(Cell::new(0_u32));
        let lost_c = lost.clone();
        let gained_c = gained.clone();

        let mut tree = WidgetTree::new();
        let editor = tree.add(
            FillWidget::new()
                .focusable()
                .on_focus(move |is_gained, _ctx| {
                    if is_gained {
                        gained_c.set(gained_c.get() + 1);
                    } else {
                        lost_c.set(lost_c.get() + 1);
                    }
                }),
        );
        let _other = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.focus(editor);
        assert_eq!(tree.focused(), Some(editor));
        assert_eq!(gained.get(), 1, "focus() delivers FocusGained");
        assert_eq!(lost.get(), 0);

        // Park the focused editor dormant — the Switcher / tab-switch path.
        tree.set_dormant(editor);
        // Revalidate is what layout runs after the visibility pass.
        let mut noop = crate::window::NoopWindowOps;
        tree.revalidate_interaction_state(&mut noop);

        assert_eq!(
            tree.focused(),
            None,
            "tree focus must clear when the target is dormant"
        );
        assert_eq!(
            lost.get(),
            1,
            "FocusLost must reach the dormant widget so it can clear has_focus / caret blink"
        );
    }

    #[test]
    fn tab_cycles_focus_in_tree_order() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.focus(a);
        assert_eq!(tree.focused(), Some(a));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(b));

        tree.press_key(Key::Tab, Modifiers::SHIFT);
        assert_eq!(tree.focused(), Some(a));
    }

    #[test]
    fn focus_survives_theme_switch() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let _b = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.focus(a);
        assert_eq!(tree.focused(), Some(a));

        tree.set_theme(crate::presets::intui::dark());
        tree.layout(SizeProposal::exact(200.0, 80.0));

        assert_eq!(
            tree.focused(),
            Some(a),
            "theme switch must not clobber focus"
        );
    }

    #[test]
    fn focus_survives_locale_switch() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.focus(a);
        tree.set_locale("fr-FR".to_string());
        tree.layout(SizeProposal::exact(200.0, 80.0));

        assert_eq!(
            tree.focused(),
            Some(a),
            "locale switch must not clobber focus"
        );
    }

    // ── focus_within / hover_within ─────────────────────────────

    #[test]
    fn focus_within_flips_when_descendant_takes_focus() {
        use crate::signal::Signal;
        use crate::test_widgets::StackWidget;
        use crate::widget_builder::WidgetBuilder;

        let halo = Signal::new(false);

        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new().focusable());
        let mid = tree.add(StackWidget::new().child(leaf));
        let _outer = tree.add(StackWidget::new().child(mid).focus_within(halo.clone()));

        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(!halo.get(), "no focus yet → signal is false");

        tree.focus(leaf);
        assert!(halo.get(), "leaf now focused, outer is its strict ancestor");
    }

    #[test]
    fn focus_within_strict_ancestors_only() {
        // A widget that is itself focused must NOT see its own
        // focus_within signal flipped to true.
        use crate::signal::Signal;
        use crate::widget_builder::WidgetBuilder;

        let halo = Signal::new(false);

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable().focus_within(halo.clone()));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        assert!(
            !halo.get(),
            "focusing the widget itself must not set its own focus_within"
        );
    }

    #[test]
    fn view_focus_active_is_inclusive_for_the_view_itself() {
        // A non-focusable item inside a focusable "view" reads the view's scope
        // focus: true when the view OR a descendant holds focus (inclusive),
        // unlike `focus_within` (strict descendants only). This is what powers
        // focus-aware selection — the data view holds focus directly, yet its
        // selected rows must still render "active".
        use crate::test_widgets::StackWidget;
        use crate::widget_builder::WidgetBuilder;

        let mut tree = WidgetTree::new();
        let item = tree.add(FillWidget::new()); // a row item — NOT focusable
        let view = tree.add(StackWidget::new().child(item).focusable(true));
        let outside = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let active = tree.view_focus_active_for(item);
        assert!(!active.get(), "no focus yet → scope inactive");

        tree.focus(view);
        assert!(
            active.get(),
            "view focused directly → scope active (focus_within would be false here)"
        );

        tree.focus(outside);
        assert!(
            !active.get(),
            "focus moved outside the view → scope inactive"
        );
    }

    #[test]
    fn begin_view_focus_for_keys_on_the_view_root_not_the_building_pane() {
        // TableView / TreeTableView / GridView build their rows inside a
        // separate, non-focusable body *pane*; keyboard focus lands on the
        // focusable view *root* (the pane's ancestor). A row's focus scope must
        // therefore be keyed on the root via `begin_view_focus_for(root)`, so
        // its selection reads "active" when the view is focused. Keying on the
        // pane — which never holds focus and is not an ancestor of the focused
        // root — would read constant-false (the latent bug this fixes).
        use crate::test_widgets::StackWidget;
        use crate::widget_builder::WidgetBuilder;

        let mut tree = WidgetTree::new();
        let row = tree.add(FillWidget::new()); // row item — NOT focusable
        let pane = tree.add(StackWidget::new().child(row)); // body pane — NOT focusable
        let root = tree.add(StackWidget::new().child(pane).focusable(true));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // What the pane opens for its rows: a scope keyed on the root.
        let keyed_on_root = tree.begin_view_focus(root);
        tree.end_view_focus();
        // What keying on the pane itself would have produced (the bug).
        let keyed_on_pane = tree.view_focus_signal_for(pane);

        assert!(!keyed_on_root.get(), "no focus yet → inactive");
        tree.focus(root);
        assert!(keyed_on_root.get(), "view root focused → row scope active");
        assert!(
            !keyed_on_pane.get(),
            "pane-keyed scope stays false: the focused root is the pane's ancestor, not its descendant",
        );
    }

    #[test]
    fn focus_visible_tracks_input_modality() {
        // `:focus-visible` — keyboard input reveals focus rings, pointer input
        // hides them. The recipe gates the row focus ring on this signal.
        use crate::event::{Key, Modifiers};
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let vis = tree.focus_visible_signal();
        assert!(!vis.get(), "starts not focus-visible");

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert!(vis.get(), "keyboard input turns focus-visible ON");

        tree.click(w);
        assert!(!vis.get(), "pointer input turns focus-visible OFF");

        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert!(vis.get(), "keyboard input turns it back ON");
    }

    #[test]
    fn view_focus_active_without_focusable_ancestor_is_constant_true() {
        // An item with no focusable ancestor (e.g. a static list) always reads
        // active, so its selection chrome is never muted.
        let mut tree = WidgetTree::new();
        let item = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(tree.view_focus_active_for(item).get());
    }

    #[test]
    fn focus_within_diff_across_siblings() {
        // Tree: root → mid_a [sig_a] → leaf_a, root → mid_b [sig_b] → leaf_b.
        // Move focus from leaf_a to leaf_b: sig_a → false, sig_b → true.
        use crate::signal::Signal;
        use crate::test_widgets::StackWidget;
        use crate::widget_builder::WidgetBuilder;

        let sig_a = Signal::new(false);
        let sig_b = Signal::new(false);

        let mut tree = WidgetTree::new();
        let leaf_a = tree.add(FillWidget::new().focusable());
        let leaf_b = tree.add(FillWidget::new().focusable());
        let mid_a = tree.add(StackWidget::new().child(leaf_a).focus_within(sig_a.clone()));
        let mid_b = tree.add(StackWidget::new().child(leaf_b).focus_within(sig_b.clone()));
        let _root = tree.add(StackWidget::new().child(mid_a).child(mid_b));

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(leaf_a);
        assert!(sig_a.get(), "sig_a true after focusing leaf_a");
        assert!(!sig_b.get(), "sig_b false: leaf_a is not its descendant");

        tree.focus(leaf_b);
        assert!(!sig_a.get(), "sig_a flipped to false on focus move");
        assert!(sig_b.get(), "sig_b flipped to true on focus move");
    }

    #[test]
    fn focus_within_clears_when_focused_widget_destroyed() {
        use crate::signal::Signal;
        use crate::test_widgets::StackWidget;
        use crate::widget_builder::WidgetBuilder;

        let halo = Signal::new(false);

        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new().focusable());
        let _outer = tree.add(StackWidget::new().child(leaf).focus_within(halo.clone()));

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(leaf);
        assert!(halo.get());

        tree.destroy_subtree(leaf);
        assert!(
            !halo.get(),
            "destroying the focused widget must clear focus_within on its ancestors"
        );
    }

    #[test]
    fn hover_within_flips_via_pointer_move() {
        use crate::signal::Signal;
        use crate::test_widgets::StackWidget;
        use crate::widget_builder::WidgetBuilder;
        use teksilo_canvas::Point;

        let glow = Signal::new(false);

        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new());
        let _outer = tree.add(StackWidget::new().child(leaf).hover_within(glow.clone()));

        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(!glow.get());

        tree.pointer_move(Point::new(50.0, 25.0));
        assert!(glow.get(), "pointer over leaf → outer.hover_within = true");

        tree.pointer_move(Point::new(500.0, 500.0));
        assert!(
            !glow.get(),
            "pointer moves outside the tree → hover_within clears"
        );
    }

    #[test]
    fn hover_within_strict_ancestors_only() {
        use crate::signal::Signal;
        use crate::widget_builder::WidgetBuilder;
        use teksilo_canvas::Point;

        let glow = Signal::new(false);

        let mut tree = WidgetTree::new();
        let _w = tree.add(FillWidget::new().hover_within(glow.clone()));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.pointer_move(Point::new(50.0, 25.0));

        assert!(
            !glow.get(),
            "hovering the widget itself must not set its own hover_within"
        );
    }
}

#[cfg(test)]
mod tests_scope {
    //! Scope-aware Tab traversal (`FocusScope` / `set_traversal_scope`).
    //! These drive the scope marker directly via `WidgetTree::set_traversal_scope`
    //! so the algorithm is exercised with no dependency on the widgets crate.

    use super::*;
    use crate::focus::TraversalScopePolicy::{Continue, Cycle};
    use crate::test_widgets::{FillWidget, StackWidget};
    use crate::widget_builder::WidgetBuilder;

    /// A focusable leaf with an explicit scoped `tab_index`.
    fn indexed(tree: &mut WidgetTree, idx: i32) -> WidgetId {
        tree.add(FillWidget::new().focusable().tab_index(idx))
    }

    fn tab(tree: &mut WidgetTree) {
        tree.press_key(Key::Tab, Modifiers::NONE);
    }
    fn shift_tab(tree: &mut WidgetTree) {
        tree.press_key(Key::Tab, Modifiers::SHIFT);
    }

    #[test]
    fn overlapping_tab_index_in_sibling_scopes_does_not_interleave() {
        let mut tree = WidgetTree::new();
        let a1 = indexed(&mut tree, 1);
        let a2 = indexed(&mut tree, 2);
        let scope_a = tree.add(StackWidget::new().child(a1).child(a2));
        let b1 = indexed(&mut tree, 1);
        let b2 = indexed(&mut tree, 2);
        let scope_b = tree.add(StackWidget::new().child(b1).child(b2));
        let _root = tree.add(StackWidget::new().child(scope_a).child(scope_b));
        tree.set_traversal_scope(scope_a, Continue);
        tree.set_traversal_scope(scope_b, Continue);
        tree.layout(SizeProposal::exact(100.0, 100.0));

        // Grouped: a1,a2 then b1,b2 — never a1,b1,a2,b2.
        for expected in [a1, a2, b1, b2, a1] {
            tab(&mut tree);
            assert_eq!(tree.focused(), Some(expected));
        }
    }

    #[test]
    fn continue_scope_flows_out_at_the_ends() {
        // root[A, scopeC(Continue)[c1,c2], B] — no tab_index, DFS order.
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let c1 = tree.add(FillWidget::new().focusable());
        let c2 = tree.add(FillWidget::new().focusable());
        let scope_c = tree.add(StackWidget::new().child(c1).child(c2));
        let b = tree.add(FillWidget::new().focusable());
        let _root = tree.add(StackWidget::new().child(a).child(scope_c).child(b));
        tree.set_traversal_scope(scope_c, Continue);
        tree.layout(SizeProposal::exact(100.0, 100.0));

        for expected in [a, c1, c2, b, a] {
            tab(&mut tree);
            assert_eq!(tree.focused(), Some(expected));
        }
        // Reverse: from c1, Shift+Tab leaves the scope to A (not c2).
        tree.focus(c1);
        shift_tab(&mut tree);
        assert_eq!(tree.focused(), Some(a));
    }

    #[test]
    fn cycle_scope_wraps_and_never_escapes() {
        // root[A, scopeD(Cycle)[d1,d2]].
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let d1 = tree.add(FillWidget::new().focusable());
        let d2 = tree.add(FillWidget::new().focusable());
        let scope_d = tree.add(StackWidget::new().child(d1).child(d2));
        let _root = tree.add(StackWidget::new().child(a).child(scope_d));
        tree.set_traversal_scope(scope_d, Cycle);
        tree.layout(SizeProposal::exact(100.0, 100.0));

        tree.focus(d1);
        for _ in 0..10 {
            tab(&mut tree);
            let f = tree.focused();
            assert!(
                f == Some(d1) || f == Some(d2),
                "Cycle scope must trap Tab inside {{d1,d2}}, got {f:?}"
            );
        }
        // Forward d1→d2→d1 and reverse d1→d2 (wrap at the start).
        tree.focus(d1);
        tab(&mut tree);
        assert_eq!(tree.focused(), Some(d2));
        shift_tab(&mut tree);
        assert_eq!(tree.focused(), Some(d1));
        shift_tab(&mut tree);
        assert_eq!(tree.focused(), Some(d2), "Shift+Tab at start wraps to last");
    }

    #[test]
    fn empty_scope_is_skipped() {
        // root[A, emptyScope(Continue)[], B].
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let empty = tree.add(StackWidget::new());
        let b = tree.add(FillWidget::new().focusable());
        let _root = tree.add(StackWidget::new().child(a).child(empty).child(b));
        tree.set_traversal_scope(empty, Continue);
        tree.layout(SizeProposal::exact(100.0, 100.0));

        for expected in [a, b, a] {
            tab(&mut tree);
            assert_eq!(tree.focused(), Some(expected));
        }
    }

    #[test]
    fn single_member_cycle_scope_stays_put() {
        let mut tree = WidgetTree::new();
        let e = tree.add(FillWidget::new().focusable());
        let scope_e = tree.add(StackWidget::new().child(e));
        tree.set_traversal_scope(scope_e, Cycle);
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.focus(e);
        tab(&mut tree);
        assert_eq!(tree.focused(), Some(e));
        shift_tab(&mut tree);
        assert_eq!(tree.focused(), Some(e));
    }

    #[test]
    fn nested_continue_in_cycle_flows_out_to_outer() {
        // outer(Cycle)[X, inner(Continue)[i1,i2], Y] — outer is the only root.
        let mut tree = WidgetTree::new();
        let x = tree.add(FillWidget::new().focusable());
        let i1 = tree.add(FillWidget::new().focusable());
        let i2 = tree.add(FillWidget::new().focusable());
        let inner = tree.add(StackWidget::new().child(i1).child(i2));
        let y = tree.add(FillWidget::new().focusable());
        let outer = tree.add(StackWidget::new().child(x).child(inner).child(y));
        tree.set_traversal_scope(inner, Continue);
        tree.set_traversal_scope(outer, Cycle);
        tree.layout(SizeProposal::exact(100.0, 100.0));

        for expected in [x, i1, i2, y, x] {
            tab(&mut tree);
            assert_eq!(tree.focused(), Some(expected));
        }
        // Shift+Tab from i1 escapes inner to X.
        tree.focus(i1);
        shift_tab(&mut tree);
        assert_eq!(tree.focused(), Some(x));
    }

    #[test]
    fn nested_cycle_in_cycle_traps_in_the_inner_scope() {
        // outer(Cycle)[X, inner(Cycle)[i1,i2], Y]: once inside inner, Y is
        // unreachable via Tab.
        let mut tree = WidgetTree::new();
        let x = tree.add(FillWidget::new().focusable());
        let i1 = tree.add(FillWidget::new().focusable());
        let i2 = tree.add(FillWidget::new().focusable());
        let inner = tree.add(StackWidget::new().child(i1).child(i2));
        let y = tree.add(FillWidget::new().focusable());
        let outer = tree.add(StackWidget::new().child(x).child(inner).child(y));
        tree.set_traversal_scope(inner, Cycle);
        tree.set_traversal_scope(outer, Cycle);
        tree.layout(SizeProposal::exact(100.0, 100.0));

        tree.focus(i1);
        for _ in 0..6 {
            tab(&mut tree);
            let f = tree.focused();
            assert!(
                f == Some(i1) || f == Some(i2),
                "inner Cycle must trap Tab; reached {f:?}"
            );
        }
    }

    #[test]
    fn scoped_tab_index_orders_within_a_scope() {
        // scopeF(Cycle)[f3#3, f1#1, f2#2] added out of order → visited 1,2,3.
        let mut tree = WidgetTree::new();
        let f3 = indexed(&mut tree, 3);
        let f1 = indexed(&mut tree, 1);
        let f2 = indexed(&mut tree, 2);
        let scope_f = tree.add(StackWidget::new().child(f3).child(f1).child(f2));
        tree.set_traversal_scope(scope_f, Cycle);
        tree.layout(SizeProposal::exact(100.0, 100.0));

        for expected in [f1, f2, f3, f1] {
            tab(&mut tree);
            assert_eq!(tree.focused(), Some(expected));
        }
    }

    #[test]
    fn destroyed_focused_widget_re_enters_at_first() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.focus(b);
        tree.destroy_subtree(b);
        tab(&mut tree);
        assert_eq!(tree.focused(), Some(a), "Tab after destroy enters at first");
    }

    #[test]
    fn set_traversal_scope_forces_node_non_focusable() {
        // A scope marker on an otherwise-focusable node must drop it from Tab
        // (it is a boundary, not a stop).
        let mut tree = WidgetTree::new();
        let inner = tree.add(FillWidget::new().focusable());
        let scope = tree.add(StackWidget::new().child(inner).focusable(true));
        tree.set_traversal_scope(scope, Continue);
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Only `inner` is a Tab stop; the scope node itself never gets focus.
        tab(&mut tree);
        assert_eq!(tree.focused(), Some(inner));
        tab(&mut tree);
        assert_eq!(tree.focused(), Some(inner));
    }

    #[test]
    fn no_scopes_behaves_like_a_flat_cycle() {
        // Regression: without any scopes, traversal is a flat wrapping ring,
        // ordered by tab_index then DFS — identical to the pre-scope behavior.
        let mut tree = WidgetTree::new();
        let a = indexed(&mut tree, 2);
        let b = indexed(&mut tree, 1);
        let c = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // b(#1), a(#2), then c(no index) — wrapping.
        for expected in [b, a, c, b] {
            tab(&mut tree);
            assert_eq!(tree.focused(), Some(expected));
        }
    }

    #[test]
    fn centered_modal_confines_tab_to_its_content() {
        // A centered overlay folds into the traversal model as an implicit
        // Cycle scope rooted at its content — Tab must stay inside it.
        let mut tree = WidgetTree::new();
        let outside1 = tree.add(FillWidget::new().focusable());
        let outside2 = tree.add(FillWidget::new().focusable());
        let m1 = tree.add(FillWidget::new().focusable());
        let m2 = tree.add(FillWidget::new().focusable());
        let content = tree.add(StackWidget::new().child(m1).child(m2));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor: outside1,
            placement: crate::overlay::OverlayPlacement::Centered,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        for _ in 0..6 {
            tab(&mut tree);
            let f = tree.focused();
            assert!(
                f == Some(m1) || f == Some(m2),
                "modal must trap Tab inside its content, reached {f:?}"
            );
        }
        assert_ne!(tree.focused(), Some(outside1));
        assert_ne!(tree.focused(), Some(outside2));
    }
}

#[cfg(test)]
mod tests_focus_out_dismissal {
    //! Non-modal overlays follow focus out instead of trapping it.
    //!
    //! Driven against bare `OverlayRequest`s so the rule is exercised with no
    //! dependency on the widgets crate; the real `MenuBar` / `PopoverButton` /
    //! `ComboBox` behaviour is pinned over in `teksilo-widgets`.

    use super::*;
    use crate::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
    use crate::test_widgets::{FillWidget, StackWidget};

    fn tab(tree: &mut WidgetTree) {
        tree.press_key(Key::Tab, Modifiers::NONE);
    }

    /// Show `content` anchored to `anchor`, the way a popover or menu does.
    fn show_anchored(
        tree: &mut WidgetTree,
        content: WidgetId,
        anchor: WidgetId,
        parent: Option<crate::overlay::OverlayId>,
    ) -> crate::overlay::OverlayId {
        tree.show_overlay(OverlayRequest {
            content_id: content,
            anchor,
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::EscapeOrClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: parent,
            on_dismiss: None,
            fade_duration: None,
        })
    }

    #[test]
    fn tab_out_of_a_non_modal_overlay_dismisses_it() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new().focusable());
        let after = tree.add(FillWidget::new().focusable());
        let inner = tree.add(FillWidget::new().focusable());
        let content = tree.add(StackWidget::new().child(inner));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        show_anchored(&mut tree, content, anchor, None);

        tree.focus(inner);
        tab(&mut tree);

        assert_ne!(tree.focused(), Some(inner), "focus genuinely left");
        assert!(
            tree.active_overlays().is_empty(),
            "an overlay must not stay open over the focus ring that left it"
        );
        assert!(tree.focused() == Some(after) || tree.focused() == Some(anchor));
    }

    /// The centered modal keeps its trap — that pattern *does* contain focus.
    #[test]
    fn a_centered_modal_is_never_dismissed_by_focus_moving() {
        let mut tree = WidgetTree::new();
        let outside = tree.add(FillWidget::new().focusable());
        let m1 = tree.add(FillWidget::new().focusable());
        let content = tree.add(StackWidget::new().child(m1));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.show_overlay(OverlayRequest {
            content_id: content,
            anchor: outside,
            placement: OverlayPlacement::Centered,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        tree.focus(m1);
        // Force focus out programmatically — Tab could not do this, but an
        // AccessKit action or app code can, and the modal must survive it.
        tree.focus(outside);

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "a modal is the one overlay that legitimately contains focus"
        );
    }

    /// The scrim is anchored to whatever opened the modal, so an anchor-aware
    /// rule could mistake it for a panel orbiting that widget. `FullViewport`
    /// is what tells it apart.
    #[test]
    fn the_modal_scrim_survives_focus_moving_inside_the_modal() {
        let mut tree = WidgetTree::new();
        let opener = tree.add(FillWidget::new().focusable());
        let m1 = tree.add(FillWidget::new().focusable());
        let m2 = tree.add(FillWidget::new().focusable());
        let scrim = tree.add(FillWidget::new());
        let content = tree.add(StackWidget::new().child(m1).child(m2));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.show_overlay(OverlayRequest {
            content_id: scrim,
            anchor: opener,
            placement: OverlayPlacement::FullViewport,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        tree.show_overlay(OverlayRequest {
            content_id: content,
            anchor: opener,
            placement: OverlayPlacement::Centered,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        tree.focus(opener);
        tree.focus(m1);
        tab(&mut tree);

        assert_eq!(
            tree.active_overlays().len(),
            2,
            "scrim and modal both stand while focus moves within the modal"
        );
    }

    /// A submenu is a *sibling* arena subtree linked only by `parent_overlay`.
    /// Ask the arena instead and the parent dies the moment its own child opens.
    #[test]
    fn focus_moving_into_a_child_overlay_keeps_the_parent_open() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new().focusable());
        let parent_item = tree.add(FillWidget::new().focusable());
        let parent_content = tree.add(StackWidget::new().child(parent_item));
        let child_item = tree.add(FillWidget::new().focusable());
        let child_content = tree.add(StackWidget::new().child(child_item));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let parent = show_anchored(&mut tree, parent_content, anchor, None);
        show_anchored(&mut tree, child_content, parent_item, Some(parent));

        tree.focus(parent_item);
        tree.focus(child_item);

        assert_eq!(
            tree.active_overlays().len(),
            2,
            "opening a submenu is not leaving the menu that owns it"
        );
    }

    /// Backing out to a shallower level of the same cascade closes only what
    /// sits below it.
    #[test]
    fn focus_back_to_the_parent_overlay_closes_only_the_child() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new().focusable());
        let parent_item = tree.add(FillWidget::new().focusable());
        let parent_content = tree.add(StackWidget::new().child(parent_item));
        let child_item = tree.add(FillWidget::new().focusable());
        let child_content = tree.add(StackWidget::new().child(child_item));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let parent = show_anchored(&mut tree, parent_content, anchor, None);
        show_anchored(&mut tree, child_content, parent_item, Some(parent));

        tree.focus(child_item);
        tree.focus(parent_item);

        assert_eq!(
            tree.active_overlays(),
            vec![parent],
            "the submenu goes, the menu that owns it stays"
        );
    }

    /// Leaving the whole cascade closes every level in one move — APG's
    /// "closes all menus and submenus", plural and unqualified.
    #[test]
    fn leaving_a_nested_cascade_closes_every_level() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new().focusable());
        let away = tree.add(FillWidget::new().focusable());
        let parent_item = tree.add(FillWidget::new().focusable());
        let parent_content = tree.add(StackWidget::new().child(parent_item));
        let child_item = tree.add(FillWidget::new().focusable());
        let child_content = tree.add(StackWidget::new().child(child_item));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let parent = show_anchored(&mut tree, parent_content, anchor, None);
        show_anchored(&mut tree, child_content, parent_item, Some(parent));

        tree.focus(child_item);
        tree.focus(away);

        assert!(
            tree.active_overlays().is_empty(),
            "one move out of the cascade must leave nothing behind"
        );
    }

    /// A dropdown opened *inside a modal* still follows focus out.
    ///
    /// The regression this pins: a `ComboBox` keeps focus on its own trigger
    /// while its panel is up, so the rule has to find that panel through its
    /// **anchor**. But when the trigger lives inside a modal — Settings, say —
    /// the by-content lookup succeeds first and answers with the *modal*, whose
    /// whole point is that it does not follow focus out. That shadowed the
    /// anchor lookup entirely, and the dropdown was left open over the
    /// Settings pane after Tab had moved on.
    #[test]
    fn a_dropdown_inside_a_modal_still_follows_focus_out() {
        let mut tree = WidgetTree::new();
        let opener = tree.add(FillWidget::new().focusable());
        let trigger = tree.add(FillWidget::new().focusable());
        let next = tree.add(FillWidget::new().focusable());
        let modal_content = tree.add(StackWidget::new().child(trigger).child(next));
        let panel = tree.add(StackWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.show_overlay(OverlayRequest {
            content_id: modal_content,
            anchor: opener,
            placement: OverlayPlacement::Centered,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        // The dropdown, anchored to a trigger that sits *within* the modal.
        show_anchored(&mut tree, panel, trigger, None);

        tree.focus(trigger);
        assert_eq!(
            tree.active_overlays().len(),
            2,
            "precondition: modal + panel"
        );

        tab(&mut tree);

        assert_eq!(
            tree.focused(),
            Some(next),
            "Tab moves on within the modal, as it should"
        );
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "the dropdown must go — only the modal hosting it stays"
        );
    }

    /// A snackbar is shown *from* a focused button and leaves that button
    /// focused — so an anchor-aware rule would tear it down on the user's very
    /// next keystroke. Its lifetime belongs to its timer, not to the keyboard.
    ///
    /// This is why eligibility asks whether an overlay is positioned *at* its
    /// anchor rather than merely whether it has one: `BottomCenter` is placed
    /// against the viewport, and its anchor is bookkeeping.
    #[test]
    fn a_viewport_placed_notification_ignores_focus_moving() {
        let mut tree = WidgetTree::new();
        let trigger = tree.add(FillWidget::new().focusable());
        let elsewhere = tree.add(FillWidget::new().focusable());
        let snack = tree.add(StackWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.show_overlay(OverlayRequest {
            content_id: snack,
            anchor: trigger,
            placement: OverlayPlacement::BottomCenter,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        tree.focus(trigger);
        tab(&mut tree);

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "a snackbar outlives the keystroke that moved focus off its trigger"
        );
        assert_ne!(tree.focused(), Some(trigger), "and focus did move");
        assert_eq!(tree.focused(), Some(elsewhere));
    }

    /// A menu must never take its host down with it. The upward walk stops at
    /// a host surface — a hosting dialog, composite tooltip, or revealed
    /// menubar — so tabbing out of the inner menu closes the menu alone.
    #[test]
    fn leaving_a_hosted_menu_spares_the_host() {
        let mut tree = WidgetTree::new();
        let opener = tree.add(FillWidget::new().focusable());
        let away = tree.add(FillWidget::new().focusable());
        let modal_item = tree.add(FillWidget::new().focusable());
        let modal_content = tree.add(StackWidget::new().child(modal_item));
        let menu_item = tree.add(FillWidget::new().focusable());
        let menu_content = tree.add(StackWidget::new().child(menu_item));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // A centered modal is unconditionally a host surface.
        let host = tree.show_overlay(OverlayRequest {
            content_id: modal_content,
            anchor: opener,
            placement: OverlayPlacement::Centered,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        show_anchored(&mut tree, menu_content, modal_item, Some(host));

        tree.focus(menu_item);
        tree.focus(away);

        assert_eq!(
            tree.active_overlays(),
            vec![host],
            "the menu goes; the modal hosting it stays"
        );
    }

    /// An overlay whose focus never enters it is still reachable through its
    /// **anchor** — the non-searchable `ComboBox` / `SearchField` shape, where
    /// focus stays on the trigger the whole time the panel is up.
    #[test]
    fn leaving_the_anchor_dismisses_a_panel_focus_never_entered() {
        let mut tree = WidgetTree::new();
        let trigger = tree.add(FillWidget::new().focusable());
        let after = tree.add(FillWidget::new().focusable());
        let content = tree.add(StackWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));
        show_anchored(&mut tree, content, trigger, None);

        tree.focus(trigger);
        assert_eq!(tree.active_overlays().len(), 1, "precondition: panel is up");

        tree.focus(after);
        assert!(
            tree.active_overlays().is_empty(),
            "leaving the trigger is leaving the dropdown it owns"
        );
    }
}
