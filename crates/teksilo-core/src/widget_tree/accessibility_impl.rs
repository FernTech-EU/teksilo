// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

use crate::accessibility::{AccessNodeBuilder, AccessibilityInfo};

/// A Teksilo rect as AccessKit expresses one: two corners rather than an
/// origin and a size.
pub(super) fn to_accesskit_rect(rect: teksilo_canvas::Rect) -> accesskit::Rect {
    accesskit::Rect {
        x0: rect.x as f64,
        y0: rect.y as f64,
        x1: (rect.x + rect.width) as f64,
        y1: (rect.y + rect.height) as f64,
    }
}

impl WidgetTree {
    /// Build an AccessKit `TreeUpdate` from the current state of all active
    /// widgets. Call this once per frame, between layout and paint, and push
    /// the result to the `accesskit_winit::Adapter`.
    /// Caches the result and rebuilds only when something that actually
    /// changes the AT tree has happened: a focus move, an overlay change, a
    /// widget rebuild, an active↔dormant transition, an `AccessibilityOnly`
    /// binding flip, a shortcut rebind, a locale switch, a queued
    /// announcement, or an explicit
    /// [`request_accessibility_update`](Self::request_accessibility_update).
    /// A plain relayout does not invalidate the cache.
    pub fn sync_accessibility(&mut self) -> accesskit::TreeUpdate {
        // Explicit re-walk request (e.g. `SceneView` materialised / destroyed a
        // scene widget, or an a11y-only scene mutation). A relayout no longer
        // sets `a11y_dirty` on its own, so this is the lever; drain it before
        // the cache check below.
        if self.a11y_update_requested.replace(false) {
            self.a11y_dirty = true;
        }
        // Shortcut registry rebinds bump
        // `ShortcutRegistry::version()`. The `access_shortcut_id`
        // resolution in the walker reads the live registry, so any
        // rebind must invalidate the AT-tree cache too — otherwise
        // a rebind would not surface in the announced shortcut
        // until something else dirties the tree (a layout, a focus
        // change, …). Cheap: one u64 compare per `sync_accessibility`.
        let current_shortcut_version = self.shortcut_registry.version().get();
        if current_shortcut_version != self.last_synced_shortcut_version {
            self.a11y_dirty = true;
            self.last_synced_shortcut_version = current_shortcut_version;
        }

        // Locale switches make `access_label(tr!(...))` (stored as a
        // locale-bound `Prop<String>`) resolve to a new value. The
        // override props are read in `apply()` during the walk, so the
        // tree must re-walk for the new announcement to surface —
        // otherwise the screen reader keeps the old-locale string until
        // something else dirties the tree. Mirror the shortcut-version
        // guard above. (Same-direction switches don't rebuild the
        // composite, so this is the only thing that refreshes AT labels.)
        let current_locale = self.locale_signal.get();
        if current_locale != self.last_synced_locale {
            self.a11y_dirty = true;
            self.last_synced_locale = current_locale;
        }

        // The framework's own live regions. Advancing them here, before the
        // cache check, is what makes a queued announcement able to wake a tree
        // that is otherwise clean: `announce` sets `a11y_update_requested`,
        // which the drain above turned into `a11y_dirty`, and the step below
        // decides what this update will say. Each message costs two updates —
        // one that exposes its node, one that retracts it — so an announcer
        // that is still busy asks for another sync and another frame.
        let announcers_busy = self.step_announcers();

        if !self.a11y_dirty
            && let Some(cached) = &self.cached_a11y
        {
            // Nothing about a widget's *content* depends on where it sits,
            // so a pure translation is re-placed in the cached tree rather
            // than rebuilt: the alternative — walking on every scroll frame
            // — was too expensive to do, so it was not done, and every
            // node's bounds went stale the moment anything scrolled.
            //
            // The full-tree contract is unchanged. Around twenty callers
            // read this return value as a complete description of the tree
            // (`wait_for_condition`, `snapshot_json`, `find_node`, the
            // headless harness), and the consumer panics on an update
            // naming a node it has never seen, so a partial update is not
            // an option here.
            let _ = cached;
            let moved = self.patch_accessibility_bounds();
            // A move alone can still announce: a live node its clipping
            // parent brings back into view enters the filtered tree, and the
            // adapters get this update as they get any other. Replaying costs
            // a pass of the consumer over every node, on every frame of a
            // scroll, so it is done only when something besides the
            // announcers is live: nothing else can be announced by a move,
            // and a live node that appears later arrives through a walk,
            // which is always replayed. A tree that records nothing skips
            // even the scan.
            if moved
                && self.announcement_ring.is_recording()
                && let Some(patched) = &self.cached_a11y
                && patched.nodes.iter().any(|(id, node)| {
                    !crate::announcer::is_announcer_node(*id)
                        && node.live().is_some_and(|live| live != accesskit::Live::Off)
                })
            {
                self.announcement_ring.observe(patched);
            }
            return self
                .cached_a11y
                .as_ref()
                .expect("the cache was present a line ago")
                .clone();
        }

        let (update, parents, local_bounds, descriptions) = self.build_accessibility_tree();
        // What the adapters will announce from this update. Here, in the
        // `&mut self` half and not in the `&self` walk, because the ring
        // replays exactly the updates this method hands out, in order. A tree
        // that does not record only notes that it missed one.
        self.announcement_ring.observe(&update);
        // Bump the AT version only when the tree's *content* actually changed.
        // A rebuild can be triggered by a shortcut-rebind / locale switch that
        // produces a byte-for-byte identical `TreeUpdate`; bumping then would
        // make `WaitCondition::AtVersionAtLeast` wake on no real change.
        // `TreeUpdate: PartialEq`, so this is a precise (if O(n)) comparison —
        // acceptable since it only runs on a real rebuild, not a cache hit.
        let content_changed = self.cached_a11y.as_ref() != Some(&update);
        self.cached_a11y = Some(update.clone());
        self.synthetic_parent_map = parents;
        self.synthetic_local_bounds = local_bounds;
        // What the next update's descriptions are measured against: only a
        // delivered update is something a screen reader has heard. A node
        // focus has just left held a text back from this update, so as not to
        // say it on the way out; nothing else may rebuild the tree before
        // focus comes back, and the text is owed to anyone who reads the node
        // without focusing it, so the next update is asked for here.
        let descriptions_catching_up = descriptions.catching_up;
        self.description_memory = descriptions;
        self.a11y_dirty = false;
        // A walk has just described every node's current position, so the
        // moves recorded up to now are already reflected.
        self.arena.take_a11y_moved();
        self.arena.take_a11y_resized();
        self.a11y_walk_generation = self.a11y_walk_generation.saturating_add(1);
        if content_changed {
            // Mirror of the shortcut-registry version signal; saturating so the
            // documented "monotonic" contract holds even past `u64::MAX`.
            self.at_version.set(self.at_version.get().saturating_add(1));
        }
        if announcers_busy || descriptions_catching_up {
            self.request_accessibility_update();
            self.request_frame();
        }
        update
    }

    /// How many accessibility walks have happened.
    ///
    /// A delivery that has not seen the latest walk is holding a tree whose
    /// *shape* may be stale, not merely its geometry — so it must push the
    /// full tree rather than rely on the geometry patch.
    pub fn a11y_walk_generation(&self) -> u64 {
        self.a11y_walk_generation
    }

    /// Re-place the nodes that moved, in the cached tree, without walking.
    ///
    /// Only geometry changes here: `at_version` is not bumped (its
    /// contract is *semantic* change), no announcement is collected, and
    /// the next full walk's `PartialEq` comparison stays correct because
    /// the cache already holds the new bounds.
    ///
    /// A moved id absent from the cache is skipped rather than inserted:
    /// it is a node the walk deliberately left out — a pruned layout
    /// stack, an excluded or merged descendant — and inventing an entry
    /// for it would put a node in the update that has no parent.
    ///
    /// Returns whether anything was re-placed.
    fn patch_accessibility_bounds(&mut self) -> bool {
        let moved = self.arena.take_a11y_moved();
        if moved.is_empty() {
            return false;
        }
        let Some(cached) = self.cached_a11y.as_mut() else {
            return false;
        };
        let index: std::collections::HashMap<accesskit::NodeId, usize> = cached
            .nodes
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (*id, i))
            .collect();

        for &id in moved.keys() {
            let bounds = self.arena.bounds(id);
            if let Some(&slot) = index.get(&crate::accessibility::widget_id_to_node_id(id)) {
                cached.nodes[slot].1.set_bounds(to_accesskit_rect(bounds));
            }
        }

        // Synthetic children ride their owner. Those that declared local
        // bounds are re-derived from the owner's new origin — exact, and
        // immune to a delta that accumulated across several passes. The
        // rest hold absolute rects and are shifted by the owner's own delta.
        for (&syn_id, &owner) in &self.synthetic_parent_map {
            let Some(delta) = moved.get(&owner) else {
                continue;
            };
            let Some(&slot) = index.get(&syn_id) else {
                continue;
            };
            let node = &mut cached.nodes[slot].1;
            // Unless the child names its own coordinate space. A scene item
            // states its rectangle in scene coordinates and declares the
            // camera beside it; the owner's window-space delta is not a
            // quantity in that space, and adding it would slide the item by
            // however far its *view* moved. Such a node is re-placed by a walk
            // — the view transform folds in its own origin, so a view that
            // moved has already dirtied the tree — and never by this patch.
            if node.transform().is_some() {
                continue;
            }
            if let Some(local) = self.synthetic_local_bounds.get(&syn_id) {
                let origin = self.arena.bounds(owner).origin();
                node.set_bounds(to_accesskit_rect(teksilo_canvas::Rect::new(
                    origin.x + local.x,
                    origin.y + local.y,
                    local.width,
                    local.height,
                )));
            } else if let Some(rect) = node.bounds() {
                node.set_bounds(accesskit::Rect {
                    x0: rect.x0 + delta.x as f64,
                    y0: rect.y0 + delta.y as f64,
                    x1: rect.x1 + delta.x as f64,
                    y1: rect.y1 + delta.y as f64,
                });
            }
        }
        true
    }

    /// Build a `TreeUpdate` describing the tree right now, without touching any
    /// state.
    ///
    /// For a caller that wants to *look* at the accessibility tree rather than
    /// deliver it: an automation query, a screenshot's blind-spot check, a test
    /// assertion. Unlike [`sync_accessibility`](Self::sync_accessibility) this
    /// neither caches, nor bumps the AT version, nor records announcements, nor
    /// advances the framework's live regions — which matters, because a caller
    /// that consumed an announcer step and then dropped the update would have
    /// silently eaten a message the user was meant to hear.
    ///
    /// It is a full walk every time; `sync_accessibility` is the one with the
    /// cache.
    pub fn accessibility_tree_snapshot(&self) -> accesskit::TreeUpdate {
        self.build_accessibility_tree().0
    }

    /// Advance both announcers one step and report whether either still has
    /// work. See [`crate::announcer`].
    fn step_announcers(&mut self) -> bool {
        // Both are stepped, not just the busy one: an announcer that is Idle
        // and empty returns false and emits the same hidden node it emitted
        // last time, so this costs nothing when nobody is announcing.
        let polite = self.announcer_polite.step();
        let assertive = self.announcer_assertive.step();
        if polite || assertive {
            self.a11y_dirty = true;
        }
        polite || assertive
    }

    /// Dispatch a synthetic AccessKit action to the node identified by
    /// `node_id` (which may be a *synthetic*, widget-emitted child node —
    /// e.g. a rich-text `TextRun`). Resolves the owning widget exactly the
    /// way the platform AT adapter does
    /// ([`node_id_to_widget_id_maybe`](crate::accessibility::node_id_to_widget_id_maybe)
    /// then [`widget_for_synthetic`](Self::widget_for_synthetic)), builds a
    /// [`crate::event::WidgetEvent::AccessAction`],
    /// and dispatches it through `ops` so actions that open windows /
    /// dialogs work.
    ///
    /// Returns `true` when the action was **consumed** — a handler claimed it,
    /// focus moved, or the context-menu fallback opened a menu — and `false`
    /// when nothing acted on it, including when the target node resolves to no
    /// live widget. Callers are expected to surface that: an action a node
    /// never handles is a caller error, and reporting it as success (which is
    /// what "a widget existed at the target" amounted to) leaves an automation
    /// client chasing timing and coordinates for a UI that was never going to
    /// move.
    ///
    /// This is the in-process equivalent of an OS screen reader invoking an
    /// action — the channel an [automation](crate::WidgetTree) harness uses
    /// to *drive* the UI without the OS AT layer.
    pub fn dispatch_access_action(
        &mut self,
        node_id: accesskit::NodeId,
        action: accesskit::Action,
        data: Option<accesskit::ActionData>,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        let target = crate::accessibility::node_id_to_widget_id_maybe(node_id)
            .or_else(|| self.widget_for_synthetic(node_id));
        let event = crate::event::WidgetEvent::AccessAction {
            action,
            target,
            target_node: node_id,
            data,
        };
        self.access_action_handled = false;
        self.dispatch_event_with_ops(event, ops);
        self.access_action_handled
    }

    // (helper `is_presentational_container` is a module-level free fn in
    // `accessibility_emit_impl.rs`)

    /// Look up the owning widget for a synthetic AccessKit `NodeId`
    /// emitted by `push_text_run_child` / `push_paragraph_child`.
    /// Used by `handle_accessibility_actions` to route an
    /// `ActionRequest` targeting a TextRun child back to the
    /// editor that owns it.
    pub fn widget_for_synthetic(&self, node_id: accesskit::NodeId) -> Option<WidgetId> {
        self.synthetic_parent_map.get(&node_id).copied()
    }

    /// Resolve a tooltip's content node past a deferred host.
    ///
    /// A deferred tooltip body answers for itself while un-built (see
    /// `DeferredSubtree::accessibility`), but once the user has hovered it the
    /// host has handed its widget value to a real child and has nothing left to
    /// say. Follow it, or a tooltip's description would be readable exactly
    /// until the first time it was shown.
    /// The text a tooltip would announce, harvested from its (dormant)
    /// content widget so an anchor can carry it as a static AT description
    /// while the tooltip is not shown.
    ///
    /// All three tiers publish their body through `accessibility()` as the
    /// node *name* (`TooltipWidget::accessibility` → `set_name(text)`), so
    /// probing the widget is enough — no parallel copy of the text has to be
    /// threaded through `attach_tooltip*` and kept in sync. Reads at walk
    /// time, so a locale change or a `Signal<String>` swap is picked up by the
    /// same AT re-walk that already tracks them.
    /// Resolve a tooltip's content node past a deferred host.
    ///
    /// A deferred tooltip body answers for itself while un-built (see
    /// `DeferredSubtree::accessibility`), but once the user has hovered it the
    /// host has handed its widget value to a real child and has nothing left to
    /// say. Follow it, or a tooltip's description would be readable exactly
    /// until the first time it was shown.
    pub(crate) fn tooltip_content_node(&self, content_id: WidgetId) -> WidgetId {
        self.arena
            .get(content_id)
            .and_then(|node| node.widget.as_any())
            .and_then(|any| any.downcast_ref::<crate::deferred_subtree::DeferredSubtree>())
            .and_then(|d| d.materialized_child())
            .unwrap_or(content_id)
    }

    pub(crate) fn tooltip_access_description(&self, content_id: WidgetId) -> Option<String> {
        let content_id = self.tooltip_content_node(content_id);
        let node = self.arena.get(content_id)?;
        let mut probe = AccessNodeBuilder::for_name_probe(content_id);
        node.widget.accessibility(&mut probe);
        if let Some(ov) = node.access_overrides.as_deref() {
            ov.apply(&mut probe);
        }
        probe
            .name()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
    }

    pub fn accessibility_node(&self, id: WidgetId) -> AccessibilityInfo {
        // Panics on an inactive id, like every caller of this has relied on.
        let builder = self.build_overridden_builder(id);
        let role = builder.role();
        let name = builder.name().map(|s| s.to_string());
        let actions = builder.actions().to_vec();
        let mut info = AccessibilityInfo::new(role, name, actions);
        if let Some(toggled) = builder.toggled() {
            info = info.with_toggled(toggled);
        }
        if let Some(expanded) = builder.expanded() {
            info = info.with_expanded(expanded);
        }
        if let Some(selected) = builder.selected() {
            info = info.with_selected(selected);
        }
        // Mirror the framework gate at `build_accessibility_recursive`:
        // arena-driven disabled wins unless the override explicitly
        // asks for `access_disabled(false)`. The override in force is the
        // one the walker applies, which for a proxy may be a composite's.
        let disabled = self.access_disabled_override(
            id,
            self.live_accessibility_proxy(id).is_some(),
            &self.composites_standing_behind(id),
        );
        let force_clear_disabled = disabled == Some(false);
        let disabled_arena = !self.arena.is_enabled(id) && !force_clear_disabled;
        // Override `Some(true)` already called `set_disabled()` inside
        // the override apply; we only need to surface it here as a
        // separate signal because `AccessNodeBuilder` doesn't expose a
        // `is_disabled()` getter on the builder side.
        let disabled_override = disabled == Some(true);
        if disabled_arena || disabled_override {
            info = info.with_disabled(true);
        }
        if builder.is_hidden() {
            info = info.with_hidden(true);
        }
        info
    }

    pub fn find_by_role(&self, role: accesskit::Role) -> Option<WidgetId> {
        self.arena
            .active_ids_iter()
            .find(|&id| self.build_overridden_builder(id).role() == role)
    }

    pub fn find_by_label(&self, label: &str) -> Option<WidgetId> {
        self.arena
            .active_ids_iter()
            .find(|&id| self.build_overridden_builder(id).name() == Some(label))
    }

    pub fn find_by_action(&self, action: accesskit::Action) -> Option<WidgetId> {
        self.arena.active_ids_iter().find(|&id| {
            self.build_overridden_builder(id)
                .actions()
                .contains(&action)
        })
    }

    /// Get the text content of a widget from its accessibility name.
    /// Equivalent to the label set via `AccessNodeBuilder::set_name`,
    /// after override application.
    pub fn text_content(&self, id: WidgetId) -> Option<String> {
        self.arena.get(id)?;
        self.build_overridden_builder(id)
            .name()
            .map(|s| s.to_string())
    }

    /// Get the text value of a widget from its accessibility value.
    /// Equivalent to the value set via `AccessNodeBuilder::set_value`,
    /// after override application.
    pub fn text_value(&self, id: WidgetId) -> Option<String> {
        self.arena.get(id)?;
        self.build_overridden_builder(id)
            .value()
            .map(|s| s.to_string())
    }
}

/// Put the window's HiDPI scale on the root node, so every rectangle in the
/// tree can stay logical.
///
/// AccessKit reads a node's `bounds` in the coordinate space of the nearest
/// ancestor carrying a `transform`, and expects the *transformed* result to be
/// in physical pixels relative to the window origin. Teksilo lays out, paints,
/// hit-tests and drives automation in logical pixels throughout, so every
/// emitter — this crate's walker, `teksilo-charts`' marks, `teksilo-scene`'s
/// items and magnets — writes a logical rectangle and none of them knows the
/// display scale.
///
/// One transform on the root reconciles the two. `accesskit_consumer`
/// accumulates a node's transform with every ancestor's up to the root, so a
/// single scale here reaches every descendant, **including synthetic children**
/// (text runs, chart marks, scene items) that hang off a widget with no
/// transform of its own. It also scales the per-character positions and widths
/// a text run carries, which live in the same node space and which a
/// per-rectangle fix-up would silently miss.
///
/// The corollary is a rule for every emitter: **never multiply an emitted
/// rectangle by a scale factor**. Doing so double-scales it here. The walker's
/// own conformance to that rule is pinned by
/// `emitted_bounds_stay_logical_at_scale_two`.
///
/// The translation term in `accesskit_winit`'s own example is an iOS-only
/// outer-versus-inner window offset and is zero on every desktop platform;
/// Teksilo's [`WidgetTree::safe_area`] is a *content* inset and is emphatically
/// not the same quantity, so it does not belong here.
pub(crate) fn emit_root_transform(root: &mut accesskit::Node, device_scale_factor: f32) {
    root.set_transform(accesskit::Affine::scale(device_scale_factor as f64));
}

#[cfg(test)]
pub(crate) mod test_helpers {
    /// Feed a `TreeUpdate` into `accesskit_consumer::Tree`, which runs the
    /// same validation that every platform AT (VoiceOver, NVDA, …) runs on
    /// activation. Panics on duplicate children, dangling relationship
    /// targets, orphaned nodes, and invalid focus — turning those runtime
    /// crashes into CI failures.
    pub(crate) fn assert_a11y_tree_valid(update: &accesskit::TreeUpdate) {
        accesskit_consumer::Tree::new(update.clone(), false);
    }

    /// Assert that every NodeId referenced by a relation of any node is
    /// present in the tree. This is the invariant our post-processing pass
    /// enforces; having a test here means a future refactor can't silently
    /// drop the pass and regress it.
    ///
    /// `next_on_line` / `previous_on_line` are checked alongside the
    /// relation lists: a text run linked to a run that never reached the
    /// tree hangs `accesskit_consumer`'s line walk instead of ending it.
    pub(crate) fn assert_no_dangling_relationships(update: &accesskit::TreeUpdate) {
        let emitted: std::collections::HashSet<accesskit::NodeId> =
            update.nodes.iter().map(|(id, _)| *id).collect();
        for (parent_id, node) in &update.nodes {
            for &target in node.controls() {
                assert!(
                    emitted.contains(&target),
                    "node {parent_id:?} has controls() → {target:?} which is absent from the tree"
                );
            }
            for &target in node.described_by() {
                assert!(
                    emitted.contains(&target),
                    "node {parent_id:?} has described_by() → {target:?} which is absent from the tree"
                );
            }
            for &target in node.labelled_by() {
                assert!(
                    emitted.contains(&target),
                    "node {parent_id:?} has labelled_by() → {target:?} which is absent from the tree"
                );
            }
            if let Some(target) = node.next_on_line() {
                assert!(
                    emitted.contains(&target),
                    "node {parent_id:?} has next_on_line() → {target:?} which is absent from the tree"
                );
            }
            if let Some(target) = node.previous_on_line() {
                assert!(
                    emitted.contains(&target),
                    "node {parent_id:?} has previous_on_line() → {target:?} which is absent from the tree"
                );
            }
        }
    }

    /// Return all NodeIds whose role matches `role`.
    #[allow(dead_code)]
    pub(crate) fn nodes_with_role(
        update: &accesskit::TreeUpdate,
        role: accesskit::Role,
    ) -> Vec<accesskit::NodeId> {
        update
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == role)
            .map(|(id, _)| *id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::*;
    use super::*;
    use crate::test_widgets::{FillWidget, StackWidget};

    /// The two framework-owned live-region nodes every `TreeUpdate` carries.
    /// Named rather than inlined so a node-count assertion says what it is
    /// counting. See [`crate::announcer`].
    const ANNOUNCER_NODES: usize = 2;

    // The framework's announcer.
    //
    // These assert through `accesskit_consumer` directly, on the node the
    // announcer emits, as well as through the tree's announcement ring. The
    // ring used to read `value().or(label())` without the filter, so a live
    // region could pass the in-process check and be silent on all three
    // platforms, which is how two of this framework's own live regions
    // shipped mute. The ring now replays the adapters' rules (see
    // `crate::accessibility::announcements`), and these keep checking the
    // one thing the adapters key on: whether the node is in the filtered tree.

    /// Is the announcer node one an adapter would see, and what does it say?
    ///
    /// `accesskit_consumer::Tree` runs the same filter each platform adapter
    /// runs. A hidden node is `FilterResult::ExcludeSubtree`, so it is absent
    /// from the filtered walk and no adapter will announce it.
    fn announced_by_consumer(update: &accesskit::TreeUpdate) -> Vec<(String, accesskit::Live)> {
        let consumer = accesskit_consumer::Tree::new(update.clone(), false);
        let state = consumer.state();
        let mut found = Vec::new();
        let mut stack = vec![state.root()];
        while let Some(node) = stack.pop() {
            if node.live() != accesskit::Live::Off
                && let Some(label) = node.label()
            {
                found.push((label, node.live()));
            }
            for child in node.filtered_children(&accesskit_consumer::common_filter) {
                stack.push(child);
            }
        }
        found
    }

    /// Every live node an adapter can reach in the filtered walk, by id.
    ///
    /// Distinct from [`announced_by_consumer`] on purpose: a node that stays in
    /// the tree with its label cleared looks "silent" to a label-based check
    /// while remaining present. Present-but-unlabelled is exactly what does NOT
    /// work: no adapter speaks a name that did not change, so only a node that
    /// genuinely leaves and re-enters the filtered tree says the same message
    /// twice.
    fn live_nodes_in_filtered_tree(update: &accesskit::TreeUpdate) -> Vec<String> {
        let consumer = accesskit_consumer::Tree::new(update.clone(), false);
        let state = consumer.state();
        let mut found = Vec::new();
        let mut stack = vec![state.root()];
        while let Some(node) = stack.pop() {
            if node.live() != accesskit::Live::Off {
                found.push(format!("{:?}", node.id()));
            }
            for child in node.filtered_children(&accesskit_consumer::common_filter) {
                stack.push(child);
            }
        }
        found.sort();
        found
    }

    fn tree_with_one_widget() -> WidgetTree {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("content"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree
    }

    /// The headline behaviour: `announce` reaches the filtered tree exactly
    /// once, then leaves it again.
    #[test]
    fn an_announcement_enters_the_filtered_tree_then_leaves_it() {
        let mut tree = tree_with_one_widget();
        // Nothing announced yet: both live regions are hidden, so a platform
        // adapter sees neither.
        let update = tree.sync_accessibility();
        assert_eq!(announced_by_consumer(&update), Vec::new());
        assert_eq!(
            live_nodes_in_filtered_tree(&update),
            Vec::<String>::new(),
            "an idle announcer must be outside the filtered tree"
        );
        assert_a11y_tree_valid(&update);

        tree.announce("Event added");

        let exposed = tree.sync_accessibility();
        assert_eq!(
            announced_by_consumer(&exposed),
            vec![("Event added".to_string(), accesskit::Live::Polite)],
            "the message must be a label on a live node inside the filtered tree"
        );
        assert_eq!(
            live_nodes_in_filtered_tree(&exposed).len(),
            1,
            "exactly the polite announcer must have entered the filtered tree"
        );
        assert_a11y_tree_valid(&exposed);

        let retracted = tree.sync_accessibility();
        assert_eq!(
            announced_by_consumer(&retracted),
            Vec::new(),
            "the node must stop carrying the message"
        );
        // The load-bearing half. Clearing the label would satisfy the check
        // above while leaving the node in the tree, and a node that never
        // leaves the tree cannot say the same message again: every adapter
        // announces an arrival or a changed name, and a repeat is neither. So
        // assert the node is genuinely gone from the filtered walk.
        assert_eq!(
            live_nodes_in_filtered_tree(&retracted),
            Vec::<String>::new(),
            "the live node must leave the filtered tree entirely, not merely \
             lose its label"
        );
        assert_a11y_tree_valid(&retracted);
    }

    /// The case the retract exists for. Both the Windows and the macOS adapter
    /// only announce an update whose label *changed*, so the same string twice
    /// in a row would be spoken once. Leaving and re-entering the tree is what
    /// makes the second one a fresh arrival.
    #[test]
    fn the_same_message_announced_twice_is_exposed_twice() {
        let mut tree = tree_with_one_widget();
        tree.announce("Saved");
        tree.announce("Saved");

        let mut exposures = 0;
        for _ in 0..6 {
            let update = tree.sync_accessibility();
            if announced_by_consumer(&update)
                .iter()
                .any(|(text, _)| text == "Saved")
            {
                exposures += 1;
            }
        }
        assert_eq!(exposures, 2, "each announcement needs its own exposure");
    }

    #[test]
    fn an_assertive_announcement_uses_the_assertive_live_setting() {
        let mut tree = tree_with_one_widget();
        tree.announce_with("Could not save", crate::announcer::Politeness::Assertive);
        let update = tree.sync_accessibility();
        assert_eq!(
            announced_by_consumer(&update),
            vec![("Could not save".to_string(), accesskit::Live::Assertive)]
        );
    }

    /// The two levels are independent nodes, so an urgent message does not have
    /// to wait behind a polite one.
    #[test]
    fn the_two_politeness_levels_announce_independently() {
        let mut tree = tree_with_one_widget();
        tree.announce("Event added");
        tree.announce_with("Could not save", crate::announcer::Politeness::Assertive);

        let mut spoken = announced_by_consumer(&tree.sync_accessibility());
        spoken.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            spoken,
            vec![
                ("Could not save".to_string(), accesskit::Live::Assertive),
                ("Event added".to_string(), accesskit::Live::Polite),
            ],
            "both levels must be exposed on the same update"
        );
    }

    /// Announcing must wake a tree that has nothing else to redraw, or a
    /// message raised by a handler that changed nothing visible would sit
    /// unspoken until something unrelated happened.
    #[test]
    fn announcing_requests_the_syncs_it_needs() {
        let mut tree = tree_with_one_widget();
        let _ = tree.sync_accessibility();
        tree.a11y_update_requested.set(false);
        tree.frame_tick_requested.set(false);

        tree.announce("Event added");
        assert!(
            tree.a11y_update_requested.get(),
            "an announcement must dirty the accessibility tree"
        );
        assert!(
            tree.frame_requested(),
            "an announcement must ask for the frame that carries it"
        );

        // Exposing leaves a retract still to do, so it asks again.
        tree.a11y_update_requested.set(false);
        tree.frame_tick_requested.set(false);
        let _ = tree.sync_accessibility();
        assert!(tree.a11y_update_requested.get());
        assert!(tree.frame_requested());

        // After the retract there is nothing more to schedule.
        tree.a11y_update_requested.set(false);
        tree.frame_tick_requested.set(false);
        let _ = tree.sync_accessibility();
        assert!(!tree.a11y_update_requested.get());
        assert!(!tree.frame_requested());
    }

    /// Asking for a frame is only half of it: the frame has to come soon. A
    /// visible widget ticking once a minute (a calendar row re-reading the
    /// clock) used to set the pace of *every* requested frame, so a queue of
    /// messages drained one exposure per minute, and whatever was still queued
    /// reached the user glued to the front of their next key press.
    ///
    /// Frames are driven in the order `teksilo-app` runs them (layout, then
    /// the accessibility sync, then paint), and the wait before each one is
    /// read from the deadline the event loop would sleep to.
    #[test]
    fn a_throttled_subscriber_does_not_hold_back_an_announcement() {
        /// One frame at 60 Hz, with room for rounding.
        const NORMAL_FRAME: std::time::Duration = std::time::Duration::from_micros(17_500);
        const CLOCK: std::time::Duration = std::time::Duration::from_secs(60);

        fn frame(tree: &mut WidgetTree) -> accesskit::TreeUpdate {
            tree.layout(SizeProposal::exact(200.0, 100.0));
            let update = tree.sync_accessibility();
            let _ = tree.render();
            update
        }
        fn wait_for_next_frame(tree: &WidgetTree) -> std::time::Duration {
            let deadline = tree
                .frame_tick_deadline()
                .expect("a visible subscriber keeps a frame scheduled");
            let last = tree.last_frame_time.expect("a frame has run");
            deadline.saturating_duration_since(last)
        }

        let mut tree = WidgetTree::new();
        let row = tree.add(FillWidget::new().label("content"));
        let _clock = tree.subscribe_frame_tick_throttled(row, CLOCK);
        let _ = frame(&mut tree);
        let _ = frame(&mut tree);
        assert!(
            wait_for_next_frame(&tree) >= CLOCK - NORMAL_FRAME,
            "precondition: with nothing else going on, the clock alone paces the loop"
        );

        let messages = ["Annulé : l'ajout d'un évènement", "3 évènements"];
        for message in messages {
            tree.announce(message);
        }
        // Checked before any frame runs, because not every announcement comes
        // from a key press that redraws at once: one raised by a timer or an
        // async completion waits for exactly this deadline.
        assert!(
            wait_for_next_frame(&tree) <= NORMAL_FRAME,
            "an announcement must be carried by the next normal frame, not the clock's \
             (waited {:?})",
            wait_for_next_frame(&tree)
        );

        let mut exposed = Vec::new();
        let mut frames = 0;
        let settled = loop {
            let update = frame(&mut tree);
            frames += 1;
            exposed.extend(
                announced_by_consumer(&update)
                    .into_iter()
                    .map(|(text, _)| text),
            );
            let wait = wait_for_next_frame(&tree);
            if wait > NORMAL_FRAME {
                break wait;
            }
            assert!(frames < 16, "the announcer never went idle");
        };
        assert_eq!(
            exposed, messages,
            "every queued message must be exposed before the loop goes back to sleeping on the \
             clock; one left behind is heard only at the next key press"
        );
        assert!(
            settled >= CLOCK - NORMAL_FRAME,
            "once the queue is spoken the clock must pace alone again, not free-run at 60 Hz \
             (waited {settled:?})"
        );
    }

    /// The in-process ring is what the automation bridge reports, so it has to
    /// hear the announcer as the adapters do: once, as the node arrives.
    #[test]
    fn an_announcement_reaches_the_automation_ring() {
        let mut tree = tree_with_one_widget();
        let before = tree.announcements_since(0).len() as u64;
        tree.announce("Event added");
        let _ = tree.sync_accessibility();
        let got = tree.announcements_since(before);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "Event added");
        assert!(!got[0].assertive);
    }

    /// A screenshot inspects the tree and throws the update away. If it went
    /// through `sync_accessibility` it would consume the exposure step and the
    /// user would never hear the message.
    #[test]
    fn a_tree_snapshot_does_not_consume_a_pending_announcement() {
        let mut tree = tree_with_one_widget();
        tree.announce("Event added");

        let snapshot = tree.accessibility_tree_snapshot();
        assert_eq!(
            announced_by_consumer(&snapshot),
            Vec::new(),
            "a snapshot must not advance the announcer"
        );

        let delivered = tree.sync_accessibility();
        assert_eq!(
            announced_by_consumer(&delivered),
            vec![("Event added".to_string(), accesskit::Live::Polite)],
            "the message must still be there to deliver"
        );
    }

    /// An empty message is dropped rather than costing an exposure that says
    /// nothing.
    #[test]
    fn an_empty_announcement_is_not_exposed() {
        let mut tree = tree_with_one_widget();
        let _ = tree.sync_accessibility();
        tree.announce("   ");
        let update = tree.sync_accessibility();
        assert_eq!(announced_by_consumer(&update), Vec::new());
    }

    #[derive(Debug)]
    struct ActionWidget;

    impl Widget for ActionWidget {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn accessibility(&self, builder: &mut AccessNodeBuilder) {
            builder.set_role(accesskit::Role::Button);
            builder.set_name("Save");
            builder.add_action(accesskit::Action::Click);
            builder.add_action(accesskit::Action::Focus);
        }
    }

    #[derive(Debug)]
    struct ClickableWidget;

    impl Widget for ClickableWidget {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn accessibility(&self, builder: &mut crate::accessibility::AccessNodeBuilder) {
            builder.set_role(accesskit::Role::Button);
            builder.set_name("Click Me");
            builder.add_action(accesskit::Action::Click);
        }
    }

    /// A focusable node advertises `Action::Focus` whether or not its widget
    /// remembered to. The dispatcher services the action for any focusable
    /// node, so deriving the advertisement from the arena flag is what keeps
    /// the two in step — see `announce_focusable`.
    #[test]
    fn a_focusable_node_advertises_focus_without_the_widget_saying_so() {
        let mut tree = WidgetTree::new();
        // `ClickableWidget` names Click and nothing else.
        let id = tree.add(ClickableWidget.focusable(true));
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let info = tree.accessibility_node(id);
        assert!(
            info.actions().contains(&accesskit::Action::Focus),
            "the arena knows the node is focusable; the AT node must say so"
        );

        // Advertised means serviceable, not merely announced.
        let mut ops = crate::window::NoopWindowOps;
        assert!(tree.dispatch_access_action(
            crate::accessibility::widget_id_to_node_id(id),
            accesskit::Action::Focus,
            None,
            &mut ops,
        ));
        assert_eq!(tree.focused(), Some(id));
    }

    /// A composite that is not itself focusable but publishes one AT node and
    /// keeps its keyboard focus on an inner leaf: the shape `ComboBox` and
    /// `DateEdit` have, and `SpinBox` had before it published through its
    /// field. What marks it as one is that it advertises `Action::Focus` from
    /// its own `accessibility()`, which is exactly what those two do.
    #[derive(Debug)]
    struct CompositeWidget {
        child_ids: Vec<WidgetId>,
    }

    impl Widget for CompositeWidget {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn place_children(
            &self,
            bounds: Rect,
            _proposal: SizeProposal,
            children: &mut [crate::widget::WidgetPlacement],
            _ctx: &LayoutContext,
        ) {
            for child in children.iter_mut() {
                child.origin = bounds.origin();
                child.size = bounds.size();
            }
        }

        fn children(&self) -> Vec<WidgetId> {
            self.child_ids.clone()
        }

        fn accessibility(&self, builder: &mut crate::accessibility::AccessNodeBuilder) {
            builder.set_role(accesskit::Role::SpinButton);
            builder.add_action(accesskit::Action::Focus);
        }
    }

    /// An assistive-tech `Focus` on a composite lands on the node that takes
    /// the keys, not on the root that publishes the AT node.
    ///
    /// `ctx.request_focus` has always walked to the first focusable descendant,
    /// precisely because a composite like `TextInput` or `SpinBox` keeps its
    /// focus on an inner leaf. The AT path did not, so a screen reader — or the
    /// automation `focus_node` tool — parked `self.focused` on a root that
    /// accepts no keystrokes. The second assertion is the one that bites:
    /// `on_key_preview` fires only on *strict* ancestors of the focused node, so
    /// a composite focused on its own root also loses its own stepping keys.
    #[test]
    fn an_at_focus_on_a_non_focusable_composite_lands_on_its_focusable_leaf() {
        let mut tree = WidgetTree::new();
        let leaf = tree.add(ClickableWidget.focusable(true));
        let root = tree.add(CompositeWidget {
            child_ids: vec![leaf],
        });
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let mut ops = crate::window::NoopWindowOps;
        assert!(tree.dispatch_access_action(
            crate::accessibility::widget_id_to_node_id(root),
            accesskit::Action::Focus,
            None,
            &mut ops,
        ));
        assert_eq!(
            tree.focused(),
            Some(leaf),
            "focus must reach the focusable leaf, not the composite root"
        );
    }

    /// …and only a composite gets that walk.
    ///
    /// A plain container — a `Panel`, a `GroupBox`, a landmark, a label — is
    /// not focusable and offers no `Focus`, so an assistive technology asking
    /// to focus it is asking for something the node never advertised. Walking
    /// in anyway moved the keyboard onto the first control inside it, which the
    /// technology could have named itself and did not, and then reported
    /// success. The honest answer is that the action was not handled.
    #[test]
    fn an_at_focus_on_a_plain_container_does_not_walk_into_it() {
        let mut tree = WidgetTree::new();
        let leaf = tree.add(ClickableWidget.focusable(true));
        let root = tree.add(StackWidget::new().child(leaf));
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let mut ops = crate::window::NoopWindowOps;
        assert!(
            !tree.dispatch_access_action(
                crate::accessibility::widget_id_to_node_id(root),
                accesskit::Action::Focus,
                None,
                &mut ops,
            ),
            "a node that never advertised `Focus` must report the action unhandled"
        );
        assert_eq!(
            tree.focused(),
            None,
            "focus must stay where it was, not fall into the container's first control"
        );
    }

    /// The walk is a fallback, not a redirect: a focusable node still focuses
    /// itself, so every leaf control is untouched by the composite fix.
    #[test]
    fn an_at_focus_on_a_focusable_node_still_lands_on_that_node() {
        let mut tree = WidgetTree::new();
        let outer = tree.add(ClickableWidget.focusable(true));
        tree.layout(SizeProposal::exact(100.0, 100.0));

        let mut ops = crate::window::NoopWindowOps;
        assert!(tree.dispatch_access_action(
            crate::accessibility::widget_id_to_node_id(outer),
            accesskit::Action::Focus,
            None,
            &mut ops,
        ));
        assert_eq!(tree.focused(), Some(outer));
    }

    /// The derivation is a default, not a decree: `access_remove_action` still
    /// takes it away, because it is applied before the overrides.
    #[test]
    fn a_derived_focus_action_is_still_removable_by_an_override() {
        let mut tree = WidgetTree::new();
        let id = tree.add(
            ClickableWidget
                .focusable(true)
                .access_remove_action(accesskit::Action::Focus),
        );
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert!(
            !tree
                .accessibility_node(id)
                .actions()
                .contains(&accesskit::Action::Focus)
        );
    }

    /// A node nobody can focus must not advertise focus — it would be an
    /// action that declines to land.
    #[test]
    fn a_non_focusable_node_does_not_advertise_focus() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ClickableWidget);
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert!(
            !tree
                .accessibility_node(id)
                .actions()
                .contains(&accesskit::Action::Focus)
        );
    }

    #[test]
    fn labeled_widget_has_accessibility() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().label("Hello"));
        tree.layout(SizeProposal::exact(100.0, 20.0));
        let info = tree.accessibility_node(widget);
        assert_eq!(info.role(), accesskit::Role::Label);
        assert_eq!(info.name(), Some("Hello"));
    }

    #[test]
    fn find_by_label_works() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().label("Save"));
        tree.layout(SizeProposal::exact(100.0, 20.0));
        assert_eq!(tree.find_by_label("Save"), Some(widget));
    }

    #[test]
    fn label_role_emits_text_as_value_not_label() {
        // accesskit contract: a `Role::Label` node carries its text in the
        // `value` property, not `label`. Widgets set the accessible name via
        // `set_name` (-> `label`); the builder re-serializes it to `value`
        // when the role is `Role::Label`, otherwise the Name is empty under
        // Windows UIA and stray under macOS AXStaticText. The logical
        // introspection name (used by `find_by_label` etc.) is unchanged.
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().label("Section")); // Role::Label
        tree.layout(SizeProposal::exact(100.0, 20.0));

        // Logical view still reports the accessible name.
        assert_eq!(tree.accessibility_node(widget).name(), Some("Section"));

        // The *emitted* node carries the text in `value`, with `label` cleared.
        let update = tree.sync_accessibility();
        let node = &update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == crate::accessibility::widget_id_to_node_id(widget))
            .expect("label node must be present in the tree update")
            .1;
        assert_eq!(node.role(), accesskit::Role::Label);
        assert_eq!(
            node.value(),
            Some("Section"),
            "Role::Label text must be exposed via `value`"
        );
        assert_eq!(
            node.label(),
            None,
            "Role::Label must not leave text in the `label` property"
        );
    }

    #[test]
    fn find_by_role_works() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("Text"));
        tree.layout(SizeProposal::exact(100.0, 20.0));
        assert!(tree.find_by_role(accesskit::Role::Label).is_some());
    }

    #[test]
    fn access_hidden_bound_to_signal_toggles_reactively() {
        use crate::signal::Signal;
        use crate::widget_builder::WidgetBuilder;
        let hidden = Signal::new(false);
        let mut tree = WidgetTree::new();
        let w = tree.add(ClickableWidget.access_hidden(hidden.clone()));
        tree.layout(SizeProposal::exact(100.0, 30.0));
        assert!(
            !tree.accessibility_node(w).is_hidden(),
            "node should be visible to AT while the signal is false"
        );
        hidden.set(true);
        // `apply()` reads the prop fresh, so the pulled node reflects the flip.
        assert!(
            tree.accessibility_node(w).is_hidden(),
            "node should hide from AT when the bound signal flips to true"
        );
        hidden.set(false);
        assert!(!tree.accessibility_node(w).is_hidden());
    }

    /// **A widget with a context menu says so.** The dispatcher already opened
    /// the menu for `Action::ShowContextMenu`; nothing advertised the action, and
    /// an assistive technology offers what a node advertises.
    #[test]
    fn a_widget_with_a_context_menu_advertises_the_action() {
        let mut tree = WidgetTree::new();
        let plain = tree.add(FillWidget::new().label("no menu"));
        let with_menu = tree.add(
            FillWidget::new()
                .label("has menu")
                .context_menu(|_pos, _ctx| Some(Box::new(FillWidget::new()) as Box<dyn Widget>)),
        );
        tree.layout(SizeProposal::exact(100.0, 20.0));

        assert!(
            tree.accessibility_node(with_menu)
                .actions()
                .contains(&Action::ShowContextMenu),
            "a node owning a context-menu factory must announce it"
        );
        assert!(
            !tree
                .accessibility_node(plain)
                .actions()
                .contains(&Action::ShowContextMenu),
            "and a node without one must not"
        );
    }

    /// The action is derived BEFORE the overrides, so an author who explicitly
    /// takes it away still has it taken away.
    #[test]
    fn a_removed_context_menu_action_stays_removed() {
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .label("suppressed")
                .context_menu(|_pos, _ctx| Some(Box::new(FillWidget::new()) as Box<dyn Widget>))
                .access_remove_action(Action::ShowContextMenu),
        );
        tree.layout(SizeProposal::exact(100.0, 20.0));
        assert!(
            !tree
                .accessibility_node(id)
                .actions()
                .contains(&Action::ShowContextMenu)
        );
    }

    /// A descendant of a widget with a menu does **not** advertise it, even
    /// though the ancestor walk would open one there. Otherwise every nested box
    /// under a row with a menu would offer that menu, and an AT would read a
    /// dozen of them.
    #[test]
    fn a_child_does_not_advertise_its_parents_context_menu() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().label("inner"));
        let parent = tree.add(
            StackWidget::new()
                .child(child)
                .context_menu(|_pos, _ctx| Some(Box::new(FillWidget::new()) as Box<dyn Widget>)),
        );
        tree.layout(SizeProposal::exact(100.0, 20.0));

        assert!(
            tree.accessibility_node(parent)
                .actions()
                .contains(&Action::ShowContextMenu)
        );
        assert!(
            !tree
                .accessibility_node(child)
                .actions()
                .contains(&Action::ShowContextMenu),
            "the menu belongs to the widget that declared it"
        );
    }

    #[test]
    fn accessibility_node_collects_actions() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(ActionWidget);
        tree.layout(SizeProposal::exact(100.0, 40.0));

        let info = tree.accessibility_node(widget);
        assert_eq!(info.role(), accesskit::Role::Button);
        assert_eq!(info.name(), Some("Save"));
        assert_eq!(info.actions().len(), 2);
        assert!(info.actions().contains(&accesskit::Action::Click));
        assert!(info.actions().contains(&accesskit::Action::Focus));
    }

    #[test]
    fn sync_accessibility_produces_tree_update() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("First"));
        tree.add(FillWidget::new().label("Second"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let update = tree.sync_accessibility();
        // Root, the two widgets, and the two framework live regions, which are
        // always present (and, here, hidden). See `crate::announcer`.
        assert_eq!(update.nodes.len(), 3 + ANNOUNCER_NODES);
        assert_eq!(update.nodes[0].0, accesskit::NodeId(0));
        assert!(update.tree.is_some());
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn root_node_carries_locale_as_language() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("Bonjour"));

        // No locale set yet → no language hint (VoiceOver uses its default).
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let update = tree.sync_accessibility();
        assert_eq!(update.nodes[0].0, accesskit::NodeId(0));
        assert_eq!(
            update.nodes[0].1.language(),
            None,
            "no language before a locale is set"
        );

        // Once the app sets the locale, the root Window node carries it as a
        // BCP-47 language tag, which AccessKit propagates to the whole subtree.
        tree.set_locale("fr-FR".to_string());
        let update = tree.sync_accessibility();
        assert_eq!(
            update.nodes[0].1.language(),
            Some("fr-FR"),
            "root node must advertise the active locale as its language"
        );
    }

    #[test]
    fn sync_accessibility_excludes_dormant_widgets() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("Active"));
        let dormant = tree.add(FillWidget::new().label("Dormant"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.set_dormant(dormant);

        let update = tree.sync_accessibility();
        assert_eq!(update.nodes.len(), 2 + ANNOUNCER_NODES);
        assert_a11y_tree_valid(&update);
    }

    /// Regression test: a Relayout-only signal flip (no activation
    /// change, no role/label/value change, no focus change, no overlay
    /// activation) must NOT dirty the AccessKit cache. Previously
    /// `layout()` set `a11y_dirty = true` unconditionally on every
    /// layout pass, which fired ~60 Hz on any scene with a Pulse /
    /// Cycle animation.
    #[test]
    fn relayout_without_activation_change_does_not_dirty_a11y() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().label("Static"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // First sync clears `a11y_dirty` and populates the cache.
        let _ = tree.sync_accessibility();
        assert!(
            !tree.a11y_dirty,
            "sync_accessibility must clear the dirty flag"
        );

        // Simulate a Relayout-binding flip: the widget needs
        // re-layout, but its accessibility shape (active set, focus,
        // role, label, value) is unchanged.
        tree.arena.mark_needs_layout(id);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert!(
            !tree.a11y_dirty,
            "pure Relayout (no activation / focus / overlay / a11y-binding change) must not dirty the AT cache"
        );
    }

    /// A container that places its single child at a settable offset,
    /// without changing its size — the shape of a scroll.
    #[derive(Debug)]
    struct Mover {
        offset: crate::signal::Signal<f32>,
        child: WidgetId,
    }

    impl crate::widget::Widget for Mover {
        fn children(&self) -> Vec<WidgetId> {
            vec![self.child]
        }
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(200.0, 100.0).into()
        }
        fn place_children(
            &self,
            bounds: teksilo_canvas::Rect,
            _proposal: SizeProposal,
            children: &mut [crate::widget::WidgetPlacement],
            _ctx: &crate::widget::LayoutContext,
        ) {
            if let Some(placement) = children.first_mut() {
                placement.origin =
                    teksilo_canvas::Point::new(bounds.x + self.offset.get(), bounds.y);
                placement.size = teksilo_canvas::Size::new(50.0, 20.0);
            }
        }
    }

    /// A widget that only moved is re-placed in the cached tree.
    ///
    /// This is the scroll case, and the reason the AT tree's bounds used to
    /// go stale: re-walking on every scroll frame was too expensive to do,
    /// so it was not done, and every node in a scrolled view reported the
    /// position it had before the scroll.
    #[test]
    fn translating_a_widget_patches_the_cache_without_a_walk() {
        let mut tree = WidgetTree::new();
        let inner = tree.add(FillWidget::new().label("Row"));
        let offset = crate::signal::Signal::new(0.0f32);
        let root = tree.add(Mover {
            offset: offset.clone(),
            child: inner,
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let update = tree.sync_accessibility();
        let walks = tree.a11y_walk_generation();
        let before = find_node(&update, inner)
            .unwrap()
            .bounds()
            .expect("the node carries bounds");

        // Move the row without resizing it.
        offset.set(20.0);
        tree.arena.mark_needs_layout(root);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let update = tree.sync_accessibility();
        assert_eq!(
            tree.a11y_walk_generation(),
            walks,
            "a pure translation must not cost a walk"
        );
        let after = find_node(&update, inner)
            .unwrap()
            .bounds()
            .expect("the node still carries bounds");
        assert!(
            (after.x0 - before.x0 - 20.0).abs() < 0.01,
            "the cached node must follow the move: {before:?} -> {after:?}"
        );
    }

    #[test]
    fn at_version_does_not_bump_for_a_move() {
        // `at_version`'s contract is semantic change. A widget that moved
        // says the same thing from somewhere else, and a harness waiting on
        // the version must not wake for it.
        let mut tree = WidgetTree::new();
        let inner = tree.add(FillWidget::new().label("Row"));
        let offset = crate::signal::Signal::new(0.0f32);
        let root = tree.add(Mover {
            offset: offset.clone(),
            child: inner,
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();
        let version = tree.at_version().get();

        offset.set(20.0);
        tree.arena.mark_needs_layout(root);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();

        assert_eq!(tree.at_version().get(), version);
    }

    #[test]
    fn resizing_a_widget_dirties_the_accessibility_tree() {
        // A wrapped label re-wraps at a new width, so its lines — and
        // therefore its text runs — are a different set, not the same set
        // somewhere else. Only a full walk can produce them.
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().label("Static"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();
        let walks = tree.a11y_walk_generation();

        tree.layout(SizeProposal::exact(300.0, 100.0));
        let _ = tree.sync_accessibility();

        assert!(
            tree.a11y_walk_generation() > walks,
            "a resize must be walked, not patched"
        );
        let _ = id;
    }

    #[test]
    fn changing_the_theme_dirties_the_accessibility_tree() {
        // Typography is re-resolved, so every label re-shapes — inside
        // bounds the layout pass may leave untouched, which records no
        // resize and would otherwise leave the runs describing the old
        // metrics.
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("Static"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();
        let walks = tree.a11y_walk_generation();

        tree.set_theme(crate::presets::intui::dark());
        let _ = tree.sync_accessibility();

        assert!(tree.a11y_walk_generation() > walks);
    }

    #[test]
    fn changing_the_text_scale_dirties_the_accessibility_tree() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("Static"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();
        let walks = tree.a11y_walk_generation();

        tree.set_user_text_scale(1.5);
        let _ = tree.sync_accessibility();

        assert!(tree.a11y_walk_generation() > walks);
    }

    #[test]
    fn a_moved_node_absent_from_the_cache_is_skipped() {
        // A layout stack the walker prunes, an excluded or merged
        // descendant: the id moved, but the tree never had a node for it.
        // Inventing one would put a node in the update with no parent,
        // which panics the consumer's tree builder.
        use crate::widget_builder::WidgetBuilder;
        let mut tree = WidgetTree::new();
        let hidden = tree.add(FillWidget::new().label("Inner"));
        let offset = crate::signal::Signal::new(0.0f32);
        let root = tree.add(
            Mover {
                offset: offset.clone(),
                child: hidden,
            }
            .access_exclude_subtree(),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let update = tree.sync_accessibility();
        assert!(
            find_node(&update, hidden).is_none(),
            "the excluded descendant must be absent for this test to mean anything"
        );

        offset.set(20.0);
        tree.arena.mark_needs_layout(root);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let update = tree.sync_accessibility();

        assert!(find_node(&update, hidden).is_none());
        assert_no_dangling_relationships(&update);
    }

    #[test]
    fn focus_on_a_node_the_walk_did_not_emit_resolves_to_an_emitted_ancestor() {
        // Alive is not the same as emitted. A focusable widget inside an
        // `access_exclude_subtree` is active — dispatch reaches it, it takes
        // keystrokes — and the walk emits nothing for it. Naming it as the
        // update's focus is not a lost link but a broken tree: the consumer
        // resolves `focus` against the nodes it was handed, and every platform
        // adapter is built on that consumer.
        //
        // The same shape reaches here from `SceneView`, which publishes fewer
        // cards than it keeps alive, so this is the general guarantee behind
        // that specific one.
        use crate::widget_builder::WidgetBuilder;
        let mut tree = WidgetTree::new();
        let inner = tree.add(ClickableWidget);
        let outer = tree.add(
            Mover {
                offset: crate::signal::Signal::new(0.0f32),
                child: inner,
            }
            .access_exclude_subtree(),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(inner);

        let update = tree.sync_accessibility();
        assert!(
            find_node(&update, inner).is_none(),
            "precondition: the excluded descendant emits nothing"
        );
        let emitted: std::collections::HashSet<_> =
            update.nodes.iter().map(|(id, _)| *id).collect();
        assert!(
            emitted.contains(&update.focus),
            "the published focus must be a node in the published tree"
        );
        assert_eq!(
            update.focus,
            crate::accessibility::widget_id_to_node_id(outer),
            "…and specifically the nearest ancestor that did emit"
        );
        // The check every platform adapter runs on activation.
        accesskit_consumer::Tree::new(update, false);
    }

    /// Companion to the regression above: the dormant→active path
    /// MUST still dirty the AT cache, because the accessibility walk
    /// skips dormant nodes.
    #[test]
    fn activation_transition_does_dirty_a11y() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().label("Toggle"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();
        assert!(!tree.a11y_dirty);

        tree.set_dormant(id);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert!(
            tree.a11y_dirty,
            "active→dormant transition must dirty the AT cache so the dormant node is removed"
        );
    }

    #[test]
    fn sync_accessibility_includes_focus() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable().label("Focused"));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        let update = tree.sync_accessibility();
        let expected_focus = crate::accessibility::widget_id_to_node_id(widget);
        assert_eq!(update.focus, expected_focus);
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn sync_accessibility_parent_child_relationship() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().label("Child"));
        // A label keeps the parent from being collapsed as a presentational
        // container, so this exercises the parent→child push (not pruning).
        let parent = tree.add(
            StackWidget::new()
                .child(child)
                .access_label_literal("Parent"),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let update = tree.sync_accessibility();
        assert_eq!(update.nodes.len(), 3 + ANNOUNCER_NODES);

        let parent_node_id = crate::accessibility::widget_id_to_node_id(parent);
        let parent_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == parent_node_id)
            .map(|(_, node)| node)
            .unwrap();

        let child_node_id = crate::accessibility::widget_id_to_node_id(child);
        assert!(parent_node.children().contains(&child_node_id));
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn presentational_containers_collapse_and_promote_children() {
        // A chain of bare presentational containers (StackWidget → empty
        // `Role::Unknown`) wrapping a labeled leaf collapses entirely: the
        // leaf is promoted to its nearest semantic ancestor, and no empty
        // grouping node remains (VoiceOver would announce one as "group").
        // A *labeled* container is semantic and survives.
        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new().label("Leaf")); // Role::Label
        let inner = tree.add(StackWidget::new().child(leaf)); // bare → pruned
        let outer = tree.add(StackWidget::new().child(inner)); // bare → pruned
        let labeled = tree.add(
            StackWidget::new()
                .child(outer)
                .access_label_literal("Group"),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let update = tree.sync_accessibility();

        assert!(find_node(&update, inner).is_none(), "bare inner pruned");
        assert!(find_node(&update, outer).is_none(), "bare outer pruned");

        let labeled_node = find_node(&update, labeled).expect("labeled group survives");
        let leaf_nid = crate::accessibility::widget_id_to_node_id(leaf);
        assert!(
            labeled_node.children().contains(&leaf_nid),
            "leaf promoted past both bare containers to the labeled ancestor"
        );
        assert!(find_node(&update, leaf).is_some(), "labeled leaf kept");
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn find_by_action_finds_clickable() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(ClickableWidget);
        tree.layout(SizeProposal::exact(100.0, 40.0));

        assert_eq!(tree.find_by_action(accesskit::Action::Click), Some(widget));
        assert_eq!(tree.find_by_action(accesskit::Action::Focus), None);
    }

    #[test]
    fn text_content_returns_accessibility_name() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().label("Hello World"));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.text_content(widget), Some("Hello World".to_string()));
    }

    #[test]
    fn text_content_returns_none_without_label() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.text_content(widget), None);
    }

    #[test]
    fn descendant_of_disabled_ancestor_reports_disabled() {
        use crate::signal::Signal;

        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().label("Child"));
        let parent = tree.add(StackWidget::new().child(child));
        tree.enabled_when(parent, Signal::new(false));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert!(
            tree.accessibility_node(child).is_disabled(),
            "descendant should report disabled when ancestor is disabled"
        );
    }

    #[test]
    fn text_value_returns_accessibility_value() {
        #[derive(Debug)]
        struct ValueWidget;

        impl Widget for ValueWidget {
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> crate::widget::LayoutResponse {
                proposal.resolve(0.0, 0.0).into()
            }

            fn accessibility(&self, builder: &mut crate::accessibility::AccessNodeBuilder) {
                builder.set_role(accesskit::Role::Slider);
                builder.set_name("Volume");
                builder.set_value("75%");
            }
        }

        let mut tree = WidgetTree::new();
        let widget = tree.add(ValueWidget);
        tree.layout(SizeProposal::exact(100.0, 40.0));

        assert_eq!(tree.text_value(widget), Some("75%".to_string()));
        assert_eq!(tree.text_content(widget), Some("Volume".to_string()));
    }

    #[test]
    fn sync_accessibility_has_no_duplicate_children() {
        // Regression test for the AccessKit "duplicate child" crash (VoiceOver/NVDA).
        // assert_a11y_tree_valid already catches this via the consumer, but the
        // manual check here provides a more actionable failure message.
        let mut tree = WidgetTree::new();
        let grandchild = tree.add(FillWidget::new().label("Grandchild"));
        let child_a = tree.add(StackWidget::new().child(grandchild));
        let child_b = tree.add(FillWidget::new().label("Sibling"));
        let _root = tree.add(StackWidget::new().child(child_a).child(child_b));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let update = tree.sync_accessibility();

        let mut all_children: std::collections::HashMap<accesskit::NodeId, accesskit::NodeId> =
            std::collections::HashMap::new();
        for (parent_id, node) in &update.nodes {
            for &child_id in node.children() {
                let prev = all_children.insert(child_id, *parent_id);
                assert!(
                    prev.is_none(),
                    "duplicate child NodeId {child_id:?}: claimed by both {prev:?} and {parent_id:?}"
                );
            }
        }
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn no_dangling_relationships_in_basic_tree() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().label("Child"));
        let _parent = tree.add(StackWidget::new().child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let update = tree.sync_accessibility();
        assert_no_dangling_relationships(&update);
        assert_a11y_tree_valid(&update);
    }

    // ── Builder-level accessibility override tests ───────────────────
    //
    // Tests for `WidgetBuilder::access_*` methods.

    use crate::widget_builder::WidgetBuilder;
    use accesskit::{Action, AriaCurrent, HasPopup, Live, Orientation, Role};

    /// Find a node in a TreeUpdate by WidgetId.
    fn find_node(update: &accesskit::TreeUpdate, id: WidgetId) -> Option<&accesskit::Node> {
        let nid = crate::accessibility::widget_id_to_node_id(id);
        update
            .nodes
            .iter()
            .find(|(node_id, _)| *node_id == nid)
            .map(|(_, n)| n)
    }

    /// A widget that calls set_hidden() unconditionally — used to test
    /// `access_hidden(false)` clears widget-emitted hidden state.
    #[derive(Debug)]
    struct AlwaysHiddenWidget;
    impl Widget for AlwaysHiddenWidget {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
        fn accessibility(&self, builder: &mut AccessNodeBuilder) {
            builder.set_role(Role::GenericContainer);
            builder.set_hidden();
        }
    }

    // Test 1
    #[test]
    fn access_label_replaces_widget_label() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ClickableWidget.access_label_literal("Publish"));
        tree.layout(SizeProposal::exact(100.0, 40.0));
        assert_eq!(tree.accessibility_node(id).name(), Some("Publish"));
    }

    // Test 2
    #[test]
    fn access_description_appears_on_bare_widget() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_description_literal("Decorative"));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).expect("node present");
        assert_eq!(node.description(), Some("Decorative"));
    }

    // Test 3
    #[test]
    fn access_value_replaces_widget_value() {
        #[derive(Debug)]
        struct SliderWidget;
        impl Widget for SliderWidget {
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> crate::widget::LayoutResponse {
                proposal.resolve(0.0, 0.0).into()
            }
            fn accessibility(&self, builder: &mut AccessNodeBuilder) {
                builder.set_role(Role::Slider);
                builder.set_value("50");
            }
        }
        let mut tree = WidgetTree::new();
        let id = tree.add(SliderWidget.access_value_literal("Custom"));
        tree.layout(SizeProposal::exact(100.0, 40.0));
        assert_eq!(tree.text_value(id), Some("Custom".to_string()));
    }

    // Test 4
    #[test]
    fn access_role_overrides_widget_role() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().label("H").access_role(Role::Heading));
        tree.layout(SizeProposal::exact(100.0, 40.0));
        assert_eq!(tree.accessibility_node(id).role(), Role::Heading);
    }

    #[test]
    fn an_unshown_tooltip_still_describes_its_anchor_for_screen_readers() {
        // The pass used to wire `described_by` only while the tooltip's
        // overlay was live. A plain tooltip is never auto-shown on focus, and
        // a dormant content node is excluded from the emitted tree (so it
        // cannot be a relation target anyway) — which left the whole plain
        // tier with no screen-reader path at all. With no hover, the anchor
        // must still carry the text as a static description.
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new().label("Save"));
        let tip = tree.add(FillWidget::new().label("Save the file"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.attach_tooltip(anchor, tip, std::time::Duration::from_millis(500));

        assert!(tree.active_overlays().is_empty(), "not hovered, not shown");
        let update = tree.sync_accessibility();
        let node = find_node(&update, anchor).expect("anchor node present");
        assert_eq!(
            node.description(),
            Some("Save the file"),
            "the anchor must describe itself with its tooltip text while unshown"
        );
    }

    #[test]
    fn a_shown_tooltip_switches_the_anchor_to_the_described_by_relation() {
        // Once the content is genuinely in the tree, the richer relation is
        // used instead of the copied string. The relation reaches no screen
        // reader through AccessKit, so the tree writes the content's text back
        // into the description (`accessibility_description_impl`): showing the
        // tooltip used to take the only description a reader had away.
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new().label("Save"));
        let tip = tree.add(FillWidget::new().label("Save the file"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.attach_tooltip(anchor, tip, std::time::Duration::from_millis(100));

        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(std::time::Duration::from_millis(150));
        assert_eq!(tree.active_overlays().len(), 1, "tooltip shown");

        let update = tree.sync_accessibility();
        let node = find_node(&update, anchor).expect("anchor node present");
        assert!(
            !node.described_by().is_empty(),
            "a shown tooltip is wired as a described_by relation"
        );
        assert_eq!(
            node.description(),
            Some("Save the file"),
            "and the anchor still describes itself with the tooltip's text"
        );
    }

    // Test 5
    #[test]
    fn access_hint_alias_writes_description_field() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_hint_literal("Tip"));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.description(), Some("Tip"));
    }

    // Test 6
    #[test]
    fn access_identifier_writes_author_id() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_identifier("save-button"));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.author_id(), Some("save-button"));
    }

    // Test 7a — literal shortcut variant
    #[test]
    fn access_shortcut_literal_set() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ClickableWidget.access_shortcut_literal("Ctrl+S"));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.keyboard_shortcut(), Some("Ctrl+S"));
    }

    // Test 7b — id-based shortcut resolves through ShortcutRegistry,
    // including auto-refresh on rebind.
    #[test]
    fn access_shortcut_id_resolves_and_tracks_rebinds() {
        use crate::event::Key;
        use crate::shortcut::{KeyStroke, Shortcut};

        let mut tree = WidgetTree::new();
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .name("Save")
                .primary(KeyStroke::command(Key::S))
                .build(),
        );
        let id = tree.add(ClickableWidget.access_shortcut_id("app.save"));
        tree.layout(SizeProposal::exact(50.0, 50.0));

        // The chord is declared on the primary accelerator, so the announced
        // name is the key the user is actually looking at: ⌘ on macOS, Ctrl
        // everywhere else (see `Modifiers`' `Display`).
        let accel = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        };

        // Initial registration: AT announces the default keystroke.
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(
            node.keyboard_shortcut(),
            Some(format!("{accel}+S").as_str())
        );

        // Simulate a user rebind: the AT announcement should track it.
        tree.shortcut_registry_mut()
            .rebind_primary("app.save", Some(KeyStroke::command(Key::Q)));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(
            node.keyboard_shortcut(),
            Some(format!("{accel}+Q").as_str())
        );
    }

    // Test 7c — silently omits the announcement when the id has no
    // registered default. Same fallback behavior as `MenuItem::for_shortcut`.
    #[test]
    fn access_shortcut_id_unknown_id_omits_announcement() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ClickableWidget.access_shortcut_id("never.registered"));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.keyboard_shortcut(), None);
    }

    // Test 8
    #[test]
    fn access_hidden_true_hides_widget() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ClickableWidget.access_hidden(true));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        assert!(tree.accessibility_node(id).is_hidden());
    }

    // Test 9
    #[test]
    fn access_hidden_false_clears_widget_set_hidden() {
        let mut tree = WidgetTree::new();
        let id = tree.add(AlwaysHiddenWidget.access_hidden(false));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        assert!(
            !tree.accessibility_node(id).is_hidden(),
            "access_hidden(false) should clear widget-emitted hidden"
        );
    }

    // Test 10
    #[test]
    fn access_disabled_true_marks_disabled() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_disabled(true));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        assert!(tree.accessibility_node(id).is_disabled());
    }

    // Test 11
    #[test]
    fn access_disabled_false_clears_arena_driven_disabled() {
        use crate::signal::Signal;
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().label("X").access_disabled(false));
        tree.enabled_when(id, Signal::new(false));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        assert!(
            !tree.accessibility_node(id).is_disabled(),
            "access_disabled(false) should clear even arena-driven disabled"
        );
    }

    // Test 12
    #[test]
    fn access_controls_appends() {
        let mut tree = WidgetTree::new();
        let target = tree.add(FillWidget::new().label("Target"));
        let controller = tree.add(
            FillWidget::new()
                .label("Controller")
                .access_controls(target),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, controller).unwrap();
        let target_nid = crate::accessibility::widget_id_to_node_id(target);
        assert!(
            node.controls().contains(&target_nid),
            "controls list should contain the target NodeId"
        );
    }

    // Test 13
    #[test]
    fn access_described_by_appends() {
        let mut tree = WidgetTree::new();
        let other = tree.add(FillWidget::new().label("Desc"));
        let id = tree.add(FillWidget::new().label("Main").access_described_by(other));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        let other_nid = crate::accessibility::widget_id_to_node_id(other);
        assert!(node.described_by().contains(&other_nid));
    }

    // Test 14
    #[test]
    fn access_labelled_by_appends() {
        let mut tree = WidgetTree::new();
        let other = tree.add(FillWidget::new().label("Lbl"));
        let id = tree.add(FillWidget::new().label("Main").access_labelled_by(other));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        let other_nid = crate::accessibility::widget_id_to_node_id(other);
        assert!(node.labelled_by().contains(&other_nid));
    }

    /// The structural twin of the relation strip above, and a harder failure:
    /// a relation that dangles loses a link, a *child* that dangles breaks the
    /// walk, because everything under an unresolvable child is unreachable.
    ///
    /// The walker cannot produce one on its own — it pushes a child only after
    /// `arena.is_active`. `AccessNodeBuilder::attach_scene_child_under` can,
    /// and by design: it is called from a widget's `accessibility()`, which
    /// sees no arena, to graft another widget's node under a synthetic parent
    /// of its own. `teksilo-scene` does exactly that for a heavyweight card
    /// with a declared logical parent, and a card parked by a viewport cull is
    /// absent from the update.
    #[test]
    fn a_child_attached_to_a_node_that_never_reached_the_tree_is_stripped() {
        use crate::accessibility::{AccessNodeBuilder, SyntheticKind, widget_id_to_node_id};

        /// Grafts its one child under a synthetic group of its own, the way a
        /// `SceneView` grafts a card under an `A11yGroup`.
        #[derive(Debug)]
        struct Grafter {
            child: std::cell::Cell<Option<WidgetId>>,
        }

        impl Widget for Grafter {
            fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
                let child = ctx.add(FillWidget::new().label("Card"));
                self.child.set(Some(child));
                vec![child]
            }
            fn layout_response(
                &self,
                _p: SizeProposal,
                _c: &crate::widget::LayoutContext,
            ) -> crate::widget::LayoutResponse {
                teksilo_canvas::Size::new(100.0, 100.0).into()
            }
            fn children(&self) -> Vec<WidgetId> {
                self.child.get().into_iter().collect()
            }
            fn wants_descendant_redirects(&self) -> bool {
                true
            }
            fn a11y_redirect_descendant(
                &self,
                _self_id: WidgetId,
                descendant: WidgetId,
            ) -> Option<accesskit::NodeId> {
                // Claimed unconditionally — exactly as the scene's hook does,
                // which cannot see dormancy either.
                (self.child.get() == Some(descendant)).then(|| widget_id_to_node_id(descendant))
            }
            fn accessibility(&self, builder: &mut AccessNodeBuilder) {
                builder.set_role(accesskit::Role::Pane);
                let group =
                    builder.push_scene_child_under(None, 7, SyntheticKind::SceneGroup, |c| {
                        c.set_role(accesskit::Role::GenericContainer);
                        c.set_name("Group".to_string());
                    });
                if let Some(child) = self.child.get() {
                    builder.attach_scene_child_under(group, widget_id_to_node_id(child));
                }
            }
        }

        let mut tree = WidgetTree::new();
        let grafter = tree.add(Grafter {
            child: std::cell::Cell::new(None),
        });
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let child = tree.children(grafter)[0];

        // While the child is live the graft is a real edge.
        let update = tree.sync_accessibility();
        let child_nid = widget_id_to_node_id(child);
        assert!(
            update.nodes.iter().any(|(id, _)| *id == child_nid),
            "precondition: the grafted child is in the tree"
        );
        assert!(
            update
                .nodes
                .iter()
                .any(|(_, n)| n.children().contains(&child_nid)),
            "precondition: something names it as a child"
        );
        assert_a11y_tree_valid(&update);

        // Park it. The widget's `accessibility()` still grafts it — it has no
        // way to know — so the strip is the only thing between here and a
        // published tree naming a node that is not in it.
        tree.set_dormant(child);
        let update = tree.sync_accessibility();
        assert!(
            !update.nodes.iter().any(|(id, _)| *id == child_nid),
            "a dormant widget emits no node"
        );
        let emitted: std::collections::HashSet<accesskit::NodeId> =
            update.nodes.iter().map(|(id, _)| *id).collect();
        for (parent, node) in &update.nodes {
            for c in node.children() {
                assert!(
                    emitted.contains(c),
                    "node {parent:?} names child {c:?}, absent from the tree"
                );
            }
        }
        // And the consumer — the same validation every platform AT runs on
        // activation — accepts it.
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn a_labelled_by_target_that_went_dormant_is_stripped() {
        // A dangling `labelled_by` is worse than a dangling `controls`: the
        // consumer builds the node's *name* by concatenating its targets'
        // values, and walks the relation to do it. `accesskit_consumer`'s
        // relation iterator unwraps the lookup, so a target that never
        // reached the tree panics the walk rather than merely losing a link.
        let mut tree = WidgetTree::new();
        let title = tree.add(FillWidget::new().label("Preferences"));
        let dialog = tree.add(FillWidget::new().access_labelled_by(title));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let update = tree.sync_accessibility();
        assert!(
            !find_node(&update, dialog).unwrap().labelled_by().is_empty(),
            "the relation is live while the target is in the tree"
        );

        tree.set_dormant(title);
        let update = tree.sync_accessibility();
        assert!(
            find_node(&update, dialog).unwrap().labelled_by().is_empty(),
            "a target absent from the tree must not survive in the relation"
        );
        assert_no_dangling_relationships(&update);
    }

    #[test]
    fn re_registering_a_labelled_by_relation_does_not_duplicate_it() {
        // A composite that names itself from its own content registers the
        // relation from `build()`, which runs again on every rebuild. Appending
        // blindly would concatenate the title into the name once per rebuild.
        let mut tree = WidgetTree::new();
        let title = tree.add(FillWidget::new().label("Name"));
        let field = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        for _ in 0..3 {
            tree.push_access_labelled_by(field, title);
        }
        let update = tree.sync_accessibility();
        assert_eq!(
            find_node(&update, field).unwrap().labelled_by().len(),
            1,
            "the same target registered repeatedly is one relation"
        );
    }

    #[test]
    fn re_registering_a_described_by_relation_does_not_duplicate_it() {
        let mut tree = WidgetTree::new();
        let hint = tree.add(FillWidget::new().label("Must be unique"));
        let field = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        for _ in 0..3 {
            tree.push_access_described_by(field, hint);
        }
        let update = tree.sync_accessibility();
        assert_eq!(
            find_node(&update, field).unwrap().described_by().len(),
            1,
            "the same target registered repeatedly is one relation"
        );
    }

    // Test 15
    #[test]
    fn access_live_assertive_set() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_live(Live::Assertive));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.live(), Some(Live::Assertive));
    }

    // Test 16
    #[test]
    fn access_aria_current_set() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_current(AriaCurrent::Page));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.aria_current(), Some(AriaCurrent::Page));
    }

    // Test 17
    #[test]
    fn access_has_popup_set() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ClickableWidget.access_has_popup(HasPopup::Menu));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.has_popup(), Some(HasPopup::Menu));
    }

    // Test 18
    #[test]
    fn access_orientation_set() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_orientation(Orientation::Vertical));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.orientation(), Some(Orientation::Vertical));
    }

    // Test 19
    #[test]
    fn access_numeric_value_and_range() {
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .access_role(Role::Slider)
                .access_numeric_value(50.0)
                .access_numeric_range(0.0, 100.0)
                .access_numeric_step(5.0),
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.numeric_value(), Some(50.0));
        assert_eq!(node.min_numeric_value(), Some(0.0));
        assert_eq!(node.max_numeric_value(), Some(100.0));
        assert_eq!(node.numeric_value_step(), Some(5.0));
    }

    // Test 20
    #[test]
    fn access_action_advertises_and_routes() {
        use crate::signal::Signal;
        let flag = Signal::new(false);
        let flag_for_cb = flag.clone();
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .access_action(Action::ShowContextMenu, move |_ctx| flag_for_cb.set(true)),
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let info = tree.accessibility_node(id);
        assert!(info.actions().contains(&Action::ShowContextMenu));
        tree.dispatch_event(crate::event::WidgetEvent::AccessAction {
            action: Action::ShowContextMenu,
            target: Some(id),
            target_node: crate::accessibility::widget_id_to_node_id(id),
            data: None,
        });
        assert!(flag.get(), "callback should have been invoked");
    }

    // Test 21
    #[test]
    fn access_two_actions_both_route() {
        use crate::signal::Signal;
        let click = Signal::new(false);
        let increment = Signal::new(false);
        let click_cb = click.clone();
        let inc_cb = increment.clone();
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .access_action(Action::ShowContextMenu, move |_| click_cb.set(true))
                .access_action(Action::Increment, move |_| inc_cb.set(true)),
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));
        tree.dispatch_event(crate::event::WidgetEvent::AccessAction {
            action: Action::ShowContextMenu,
            target: Some(id),
            target_node: crate::accessibility::widget_id_to_node_id(id),
            data: None,
        });
        assert!(click.get() && !increment.get(), "only first action fired");
        tree.dispatch_event(crate::event::WidgetEvent::AccessAction {
            action: Action::Increment,
            target: Some(id),
            target_node: crate::accessibility::widget_id_to_node_id(id),
            data: None,
        });
        assert!(
            click.get() && increment.get(),
            "both actions fired exactly once each"
        );
    }

    // Test 22
    #[test]
    fn access_remove_action_suppresses_widget_action() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ActionWidget.access_remove_action(Action::Click));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let info = tree.accessibility_node(id);
        assert!(!info.actions().contains(&Action::Click));
        assert!(info.actions().contains(&Action::Focus));
    }

    // Test 23
    #[test]
    fn access_custom_action_uses_localized_label() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_custom_action_literal("Reply", |_ctx| {}));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        let actions = node.custom_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, 0);
        assert_eq!(actions[0].description.as_str(), "Reply");
    }

    // Test 24
    #[test]
    fn access_custom_action_routes_by_index() {
        use crate::signal::Signal;
        let first = Signal::new(false);
        let second = Signal::new(false);
        let f = first.clone();
        let s = second.clone();
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .access_custom_action_literal("First", move |_| f.set(true))
                .access_custom_action_literal("Second", move |_| s.set(true)),
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));
        tree.dispatch_event(crate::event::WidgetEvent::AccessAction {
            action: Action::CustomAction,
            target: Some(id),
            target_node: crate::accessibility::widget_id_to_node_id(id),
            data: Some(accesskit::ActionData::CustomAction(1)),
        });
        assert!(!first.get(), "first should not fire");
        assert!(second.get(), "second should fire (index 1)");
    }

    // Test 25
    #[test]
    fn access_action_layered_with_on_access_action() {
        use crate::signal::Signal;
        let from_override = Signal::new(false);
        let from_user = Signal::new(false);
        let ov_cb = from_override.clone();
        let user_cb = from_user.clone();
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .access_action(Action::ShowContextMenu, move |_| ov_cb.set(true))
                .on_access_action(move |_action, _ctx| {
                    user_cb.set(true);
                    crate::event::EventResponse::Handled
                }),
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));
        tree.dispatch_event(crate::event::WidgetEvent::AccessAction {
            action: Action::ShowContextMenu,
            target: Some(id),
            target_node: crate::accessibility::widget_id_to_node_id(id),
            data: None,
        });
        assert!(from_override.get(), "override callback fired");
        assert!(from_user.get(), "user catch-all fired");
    }

    // Test 26 — i18n integration via teksilo_i18n's
    // `From<LocalizedString> for Prop<String>` impl. We don't import
    // teksilo-i18n here (it depends on teksilo-core), but the conversion
    // works the same way for any `Into<Prop<String>>`. This stand-in
    // covers the same code path the FTL-bundle case takes.
    #[test]
    fn access_label_accepts_resolved_string_via_into() {
        // Simulate a `LocalizedString`-like wrapper: any type that
        // `impl Into<Prop<String>>`. The override surface stores the
        // prop and the walker reads its current value.
        struct ResolvedAtCall(String);
        impl From<ResolvedAtCall> for crate::signal::Prop<String> {
            fn from(v: ResolvedAtCall) -> crate::signal::Prop<String> {
                crate::signal::Prop::Static(v.0)
            }
        }
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_label(ResolvedAtCall("Save".to_string())));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        assert_eq!(tree.accessibility_node(id).name(), Some("Save"));
    }

    // Test 27
    #[test]
    fn access_exclude_subtree_prunes_children_from_at_tree() {
        let mut tree = WidgetTree::new();
        let inner1 = tree.add(FillWidget::new().label("A"));
        let inner2 = tree.add(FillWidget::new().label("B"));
        let outer = tree.add(
            StackWidget::new()
                .child(inner1)
                .child(inner2)
                // A label keeps `outer` from being collapsed as a
                // presentational container, so the test exercises Exclude
                // (not the new presentational-pruning pass).
                .access_label_literal("Section")
                .access_exclude_subtree(),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let update = tree.sync_accessibility();
        // Outer is present; inner1 and inner2 are pruned.
        assert!(find_node(&update, outer).is_some());
        assert!(find_node(&update, inner1).is_none(), "inner1 pruned");
        assert!(find_node(&update, inner2).is_none(), "inner2 pruned");
        let outer_node = find_node(&update, outer).unwrap();
        assert!(
            outer_node.children().is_empty(),
            "outer should have no AT children when excluded"
        );
    }

    // Test 28
    #[test]
    fn access_merge_subtree_concatenates_descendant_labels() {
        let mut tree = WidgetTree::new();
        let title = tree.add(FillWidget::new().label("Title"));
        let subtitle = tree.add(FillWidget::new().label("Subtitle"));
        let card = tree.add(
            StackWidget::new()
                .child(title)
                .child(subtitle)
                .access_merge_subtree(),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        // After merge: card's name is "Title Subtitle", children pruned.
        assert_eq!(tree.text_content(card), Some("Title Subtitle".to_string()));
        let update = tree.sync_accessibility();
        assert!(find_node(&update, title).is_none());
        assert!(find_node(&update, subtitle).is_none());
    }

    #[test]
    fn access_merge_subtree_takes_nothing_from_under_a_hidden_descendant() {
        // A hidden node hides its subtree: the consumer every platform
        // adapter reads through treats a node as hidden when any ancestor
        // is. A merged name that still took the text under a hidden
        // wrapper would say aloud what the widget hid from the reader.
        let mut tree = WidgetTree::new();
        let title = tree.add(FillWidget::new().label("Title"));
        let secret = tree.add(FillWidget::new().label("Secret"));
        let hidden = tree.add(StackWidget::new().child(secret).access_hidden(true));
        let card = tree.add(
            StackWidget::new()
                .child(title)
                .child(hidden)
                .access_merge_subtree(),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert_eq!(tree.text_content(card), Some("Title".to_string()));
    }

    // Test 29
    #[test]
    fn access_merge_subtree_unions_actions() {
        let mut tree = WidgetTree::new();
        let click_a = tree.add(ClickableWidget);
        let click_b = tree.add(ClickableWidget);
        let card = tree.add(
            StackWidget::new()
                .child(click_a)
                .child(click_b)
                .access_merge_subtree(),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let actions = tree.accessibility_node(card).actions().to_vec();
        let click_count = actions.iter().filter(|a| **a == Action::Click).count();
        assert_eq!(
            click_count, 1,
            "Click should be present exactly once after merge (deduplicated)"
        );
    }

    // Test 30 — covered by the i18n integration mechanism documented in
    // `From<LocalizedString> for String`. Since teksilo-core can't reference
    // LocalizedString, the merged-localized-label case is exercised by
    // tests 26 + 28 in combination: each child's resolved-at-call-time
    // String contributes to the merged label. The full FTL-bundle
    // round-trip is tested in the teksilo-i18n / teksilo-widgets integration
    // tests, not here.

    // Test 31
    #[test]
    fn access_merge_subtree_first_nonempty_value_wins() {
        #[derive(Debug)]
        struct ValueWidget(&'static str);
        impl Widget for ValueWidget {
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> crate::widget::LayoutResponse {
                proposal.resolve(0.0, 0.0).into()
            }
            fn accessibility(&self, builder: &mut AccessNodeBuilder) {
                builder.set_role(Role::Slider);
                builder.set_value(self.0);
            }
        }
        let mut tree = WidgetTree::new();
        let v1 = tree.add(ValueWidget("first"));
        let v2 = tree.add(ValueWidget("second"));
        let card = tree.add(
            StackWidget::new()
                .child(v1)
                .child(v2)
                .access_merge_subtree(),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert_eq!(tree.text_value(card), Some("first".to_string()));
    }

    // Test 32
    #[test]
    fn access_exclude_inside_merge() {
        let mut tree = WidgetTree::new();
        let visible = tree.add(FillWidget::new().label("VISIBLE"));
        let pruned = tree.add(FillWidget::new().label("PRUNED"));
        let inner_excluded = tree.add(StackWidget::new().child(pruned).access_exclude_subtree());
        let card = tree.add(
            StackWidget::new()
                .child(visible)
                .child(inner_excluded)
                .access_merge_subtree(),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let merged = tree.text_content(card).unwrap_or_default();
        // VISIBLE present, PRUNED absent (because inner_excluded
        // pruned its own subtree before the merge could absorb it).
        assert!(merged.contains("VISIBLE"));
        assert!(
            !merged.contains("PRUNED"),
            "excluded subtree should not contribute to merge"
        );
    }

    // Test 33
    #[test]
    fn access_merge_inside_merge() {
        let mut tree = WidgetTree::new();
        let inner_a = tree.add(FillWidget::new().label("a"));
        let inner_b = tree.add(FillWidget::new().label("b"));
        let inner_card = tree.add(
            StackWidget::new()
                .child(inner_a)
                .child(inner_b)
                .access_merge_subtree(),
        );
        let outer_extra = tree.add(FillWidget::new().label("X"));
        let outer = tree.add(
            StackWidget::new()
                .child(inner_card)
                .child(outer_extra)
                .access_merge_subtree(),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        // Outer absorbs inner_card's already-merged label ("a b") AND
        // outer_extra ("X"), giving something like "a b X" or "X a b".
        // Order is descendant-walk order; we just verify all parts
        // appear and inner children are NOT double-counted.
        let merged = tree.text_content(outer).unwrap_or_default();
        assert!(merged.contains("a"));
        assert!(merged.contains("b"));
        assert!(merged.contains("X"));
        // Inner children should not appear AS THEIR OWN nodes:
        let update = tree.sync_accessibility();
        assert!(find_node(&update, inner_a).is_none());
        assert!(find_node(&update, inner_b).is_none());
        assert!(find_node(&update, inner_card).is_none());
        assert!(find_node(&update, outer_extra).is_none());
    }

    // Test 34
    #[test]
    fn access_customize_runs_last() {
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .access_label_literal("A")
                .access_customize(|b| b.set_name("B")),
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));
        assert_eq!(tree.accessibility_node(id).name(), Some("B"));
    }

    // Test 35
    #[test]
    fn access_customize_can_reach_inner_mut() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_customize(|b| {
            b.inner_mut().set_author_id("from-customize");
        }));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.author_id(), Some("from-customize"));
    }

    // Test 36 — sanity guard. WidgetNode size delta when no overrides.
    #[test]
    fn access_overrides_zero_cost_when_unused() {
        // The override fields add only `Option<Box<...>>` (8 bytes for
        // the pointer-sized null) + `AccessSubtreeMode` (1 byte enum,
        // padded). Sanity: at most 16 bytes added.
        // We don't assert a specific size because struct layout shifts
        // with rustc versions; we just confirm both fields are
        // pointer/byte sized.
        use std::mem::size_of;
        assert!(size_of::<Option<Box<crate::widget_builder::AccessibilityOverrides>>>() <= 16);
        assert!(size_of::<crate::widget_builder::AccessSubtreeMode>() <= 4);
    }

    #[test]
    fn accessibility_children_overrides_at_reading_order() {
        // Audit G17: a widget can present a different child ORDER to assistive
        // tech than its layout/paint child order via `accessibility_children()`
        // — the mechanism TableView/TreeTableView use to read the header before
        // the body even though they build the body first (z-order).
        #[derive(Debug)]
        struct NamedLeaf(&'static str);
        impl Widget for NamedLeaf {
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> crate::widget::LayoutResponse {
                proposal.resolve(10.0, 10.0).into()
            }
            fn accessibility(&self, builder: &mut AccessNodeBuilder) {
                // Button keeps `set_name` as the node's label (Role::Label
                // routes text to `value` instead), so the test can find the
                // leaves by name.
                builder.set_role(accesskit::Role::Button);
                builder.set_name(self.0);
            }
        }

        #[derive(Debug, Default)]
        struct ReorderContainer {
            a: Option<WidgetId>,
            b: Option<WidgetId>,
        }
        impl Widget for ReorderContainer {
            fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
                let a = ctx.add(NamedLeaf("A"));
                let b = ctx.add(NamedLeaf("B"));
                self.a = Some(a);
                self.b = Some(b);
                vec![a, b]
            }
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> crate::widget::LayoutResponse {
                proposal.resolve(100.0, 100.0).into()
            }
            fn accessibility(&self, builder: &mut AccessNodeBuilder) {
                // A real role so the container itself isn't dropped as a
                // presentational node — we need to inspect its child order.
                builder.set_role(accesskit::Role::Group);
                builder.set_name("Container");
            }
            fn children(&self) -> Vec<WidgetId> {
                [self.a, self.b].into_iter().flatten().collect()
            }
            fn accessibility_children(&self) -> Option<Vec<WidgetId>> {
                // Reverse of build/layout order.
                Some([self.b, self.a].into_iter().flatten().collect())
            }
        }

        let mut tree = WidgetTree::new();
        let container = tree.add(ReorderContainer::default());
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let update = tree.sync_accessibility();

        let a_nid = update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("A"))
            .map(|(id, _)| *id)
            .expect("A node present");
        let b_nid = update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("B"))
            .map(|(id, _)| *id)
            .expect("B node present");
        let cnid = crate::accessibility::widget_id_to_node_id(container);
        let cnode = update
            .nodes
            .iter()
            .find(|(id, _)| *id == cnid)
            .map(|(_, n)| n)
            .expect("container node present");
        let kids = cnode.children();
        let pa = kids.iter().position(|k| *k == a_nid).expect("A is a child");
        let pb = kids.iter().position(|k| *k == b_nid).expect("B is a child");
        assert!(
            pb < pa,
            "accessibility_children() must set AT order B-before-A; got {kids:?}"
        );
    }

    // ── the root transform: logical rectangles, physical coordinates ──────
    //
    // These set `device_scale_factor` to 2.0 explicitly. CI runs at 1.0, where
    // a missing or wrong scale is invisible — which is how a whole tree of
    // half-size AT rectangles survived this long.

    /// The rectangle `accesskit_consumer` computes for the node labelled
    /// `label` — the same accumulate-every-ancestor-transform walk every
    /// platform adapter performs, so this is literally what an assistive
    /// technology is told.
    ///
    /// Found by label rather than by id because `FullNodeId` cannot be built
    /// from outside the consumer crate.
    fn consumer_bounding_box(update: &accesskit::TreeUpdate, label: &str) -> accesskit::Rect {
        let consumer = accesskit_consumer::Tree::new(update.clone(), false);
        let state = consumer.state();
        let mut stack = vec![state.root()];
        while let Some(node) = stack.pop() {
            // A `Role::Label`'s text lives in `value` — `label_comes_from_value`
            // is true for exactly that role — so look in both.
            let text = node.value().or_else(|| node.label());
            if text.as_deref() == Some(label) {
                return node.bounding_box().expect("node has bounds");
            }
            for child in node.children() {
                stack.push(child);
            }
        }
        panic!("no node labelled {label:?} in the consumer tree");
    }

    fn tree_at_scale(scale: f32) -> (WidgetTree, WidgetId) {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().label("content"));
        tree.set_device_scale_factor(scale);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        (tree, id)
    }

    #[test]
    fn the_root_carries_the_device_scale_as_its_transform() {
        let (mut tree, _) = tree_at_scale(2.0);
        let update = tree.sync_accessibility();
        let root = update
            .nodes
            .iter()
            .find(|(id, _)| *id == crate::accessibility::root_node_id())
            .map(|(_, n)| n)
            .expect("root node present");
        assert_eq!(
            root.transform().copied(),
            Some(accesskit::Affine::scale(2.0)),
            "AccessKit wants physical coordinates; the tree emits logical ones"
        );
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn emitted_bounds_stay_logical_at_scale_two() {
        // The rule every emitter in the workspace has to obey — this crate's
        // walker, teksilo-charts' marks, teksilo-scene's items — is that a
        // rectangle handed to AccessKit is the same logical rectangle the
        // layout produced. Multiplying by the scale at the emitter would
        // double-scale it once the root transform is applied. This is the
        // fence for the sites this crate owns; the other two crates' emitters
        // are unreachable from here (the dependency runs the other way) and
        // are covered by the same statement in `emit_root_transform`'s doc.
        let (mut tree, id) = tree_at_scale(2.0);
        let update = tree.sync_accessibility();
        let layout = tree.bounds(id);
        let nid = crate::accessibility::widget_id_to_node_id(id);
        let raw = update
            .nodes
            .iter()
            .find(|(n, _)| *n == nid)
            .and_then(|(_, n)| n.bounds())
            .expect("widget node has bounds");
        assert_eq!(
            (raw.x0, raw.y0, raw.x1, raw.y1),
            (
                layout.x as f64,
                layout.y as f64,
                (layout.x + layout.width) as f64,
                (layout.y + layout.height) as f64,
            ),
            "an emitted rectangle must be the logical layout rectangle, unscaled"
        );
    }

    /// The same fence for an **interactive** node — the one a touch-target
    /// change actually reaches.
    ///
    /// The AccessKit hit test stays exact and is never slop-expanded. A
    /// finger's grip outsets belong to the pointer hit path
    /// (`WidgetTree::widget_hit_outset` and the miss-only slop pass), not to
    /// the AT tree: an assistive technology is told where the control *is*, so
    /// inflating that rectangle would put VoiceOver's cursor and Narrator's
    /// highlight over ground the control does not own and would overlap its
    /// neighbours — and, since AccessKit resolves its own hit tests from these
    /// rectangles, would hand a touch-explore probe the wrong control near
    /// every edge.
    ///
    /// A sibling of `emitted_bounds_stay_logical_at_scale_two` rather than an
    /// extra assertion in it, because that test's fixture is a bare
    /// `FillWidget` advertising no action at all: the natural shape of this
    /// mistake — "inflate every node that advertises `Action::Click` so touch
    /// targets are easier to hit" — passes it untouched.
    #[test]
    fn an_interactive_node_emits_its_exact_layout_rectangle() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ActionWidget);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let update = tree.sync_accessibility();
        let nid = crate::accessibility::widget_id_to_node_id(id);
        let node = update
            .nodes
            .iter()
            .find(|(n, _)| *n == nid)
            .map(|(_, n)| n)
            .expect("the interactive node is in the update");
        assert!(
            node.supports_action(accesskit::Action::Click),
            "the fixture has to be the clickable shape the rule is about, \
             or this test guards nothing"
        );
        let layout = tree.bounds(id);
        assert!(
            layout.width > 0.0 && layout.height > 0.0,
            "a real, non-degenerate rectangle to compare against: {layout:?}"
        );
        let raw = node.bounds().expect("the interactive node has bounds");
        assert_eq!(
            (raw.x0, raw.y0, raw.x1, raw.y1),
            (
                layout.x as f64,
                layout.y as f64,
                (layout.x + layout.width) as f64,
                (layout.y + layout.height) as f64,
            ),
            "a clickable node's emitted rectangle is its layout rectangle, \
             not an outset one"
        );
    }

    #[test]
    fn what_an_assistive_technology_reads_is_the_physical_rectangle() {
        // Same widget, two scales: the rectangle the consumer computes — which
        // is the one VoiceOver / Narrator / Orca get — doubles, while the
        // emitted rectangle above did not.
        let (mut one, _) = tree_at_scale(1.0);
        let at_one = consumer_bounding_box(&one.sync_accessibility(), "content");
        let (mut two, _) = tree_at_scale(2.0);
        let at_two = consumer_bounding_box(&two.sync_accessibility(), "content");
        assert_eq!(at_two.x0, at_one.x0 * 2.0);
        assert_eq!(at_two.y0, at_one.y0 * 2.0);
        assert_eq!(at_two.x1, at_one.x1 * 2.0);
        assert_eq!(at_two.y1, at_one.y1 * 2.0);
        assert!(at_two.x1 > at_two.x0, "a real, non-degenerate rectangle");
    }

    #[test]
    fn a_scale_change_invalidates_the_accessibility_cache() {
        // Dragging a window from a 1x to a 2x monitor. A plain relayout does
        // not dirty the AT cache, so without the guard in
        // `set_device_scale_factor` the second sync would return the cached
        // update and report every rectangle at the old display's scale
        // for the rest of the window's life.
        let (mut tree, _) = tree_at_scale(1.0);
        let _ = tree.sync_accessibility();
        tree.set_device_scale_factor(2.0);
        let update = tree.sync_accessibility();
        let root = update
            .nodes
            .iter()
            .find(|(id, _)| *id == crate::accessibility::root_node_id())
            .map(|(_, n)| n)
            .expect("root node present");
        assert_eq!(
            root.transform().copied(),
            Some(accesskit::Affine::scale(2.0))
        );
    }

    #[test]
    fn setting_the_same_scale_does_not_dirty_the_tree() {
        // The dirty-mark is behind an equality check, so a set that changes
        // nothing costs nothing and — more to the point — cannot make the
        // "content changed" version signal move when the content did not.
        let (mut tree, _) = tree_at_scale(2.0);
        let first = tree.sync_accessibility();
        tree.set_device_scale_factor(2.0);
        let second = tree.sync_accessibility();
        assert_eq!(first, second);
    }

    // ── the explore-by-touch gate ─────────────────────────────────────────

    #[test]
    fn explore_by_touch_is_off_by_default() {
        let tree = WidgetTree::new();
        assert_eq!(tree.explore_by_touch(), crate::ExploreByTouch::Off);
        assert_eq!(
            tree.screen_reader_state(),
            crate::ScreenReaderState::Unknown
        );
        assert!(!tree.explore_by_touch_active());
    }

    #[test]
    fn auto_follows_the_operating_systems_screen_reader_flag() {
        let mut tree = WidgetTree::new();
        tree.set_explore_by_touch(crate::ExploreByTouch::Auto);
        assert!(!tree.explore_by_touch_active(), "Unknown is not a yes");
        tree.set_screen_reader_state(crate::ScreenReaderState::Inactive);
        assert!(!tree.explore_by_touch_active());
        tree.set_screen_reader_state(crate::ScreenReaderState::Active);
        assert!(tree.explore_by_touch_active());
    }

    #[test]
    fn on_ignores_the_screen_reader_flag_and_off_ignores_everything() {
        let mut tree = WidgetTree::new();
        tree.set_explore_by_touch(crate::ExploreByTouch::On);
        assert!(tree.explore_by_touch_active(), "On is the app's own choice");
        tree.set_explore_by_touch(crate::ExploreByTouch::Off);
        tree.set_screen_reader_state(crate::ScreenReaderState::Active);
        assert!(!tree.explore_by_touch_active());
    }

    #[test]
    fn an_attached_accesskit_client_does_not_switch_exploring_on() {
        // The whole point of the gate. A magnifier, a voice-control front end,
        // a UI-automation inspector and this repository's own automation
        // harness all activate an AccessKit adapter. If activation implied a
        // screen reader, attaching any of them would turn every touch into a
        // probe and make the application untouchable.
        let mut tree = WidgetTree::new();
        tree.set_explore_by_touch(crate::ExploreByTouch::Auto);
        tree.set_at_client_attached(true);
        assert!(tree.at_client_attached());
        assert_eq!(
            tree.screen_reader_state(),
            crate::ScreenReaderState::Unknown,
            "attaching says nothing about screen readers"
        );
        assert!(!tree.explore_by_touch_active());
    }

    #[test]
    fn touch_still_reaches_a_handler_with_an_accesskit_client_attached() {
        // The acceptance criterion, as a behaviour rather than a flag: a tap
        // must arrive at its handler at the `Off` default with an assistive
        // technology attached. Nothing gates the pointer path on
        // `explore_by_touch_active()` today; this is the fence for whoever
        // wires touch-as-probe later.
        use crate::Modifiers;
        use crate::pointer::{
            BackendDeviceKey, EventTime, PointerIdAllocator, PointerInfo, PointerPhase,
            PointerSample,
        };
        use crate::widget_builder::WidgetBuilder;
        use std::cell::Cell;
        use std::rc::Rc;

        let taps = Rc::new(Cell::new(0u32));
        let seen = Rc::clone(&taps);
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .label("target")
                .on_tap(move |_e, _ctx| seen.set(seen.get() + 1)),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.set_at_client_attached(true);
        assert!(!tree.explore_by_touch_active());

        let at = Point::new(50.0, 50.0);
        let pid = PointerIdAllocator::global().begin(BackendDeviceKey::new(0x0A11), 1);
        let contact = |phase| PointerSample {
            pointer: PointerInfo::touch(pid, EventTime::ZERO),
            phase,
            position: at,
            button: None,
            modifiers: Modifiers::NONE,
            coalesced: Vec::new(),
        };
        tree.dispatch_pointer(contact(PointerPhase::Down));
        tree.dispatch_pointer(contact(PointerPhase::Up));
        assert!(tree.bounds(id).width > 0.0);
        assert_eq!(taps.get(), 1, "a touch tap must still activate its handler");
    }

    #[test]
    fn a_detaching_client_is_evidence_that_no_screen_reader_is_reading() {
        // The asymmetry: attaching proves nothing, detaching proves something.
        // An `Auto` window stops exploring the moment the last client goes,
        // without waiting for the next OS query.
        let mut tree = WidgetTree::new();
        tree.set_explore_by_touch(crate::ExploreByTouch::Auto);
        tree.set_screen_reader_state(crate::ScreenReaderState::Active);
        tree.set_at_client_attached(true);
        assert!(tree.explore_by_touch_active());
        tree.set_at_client_attached(false);
        assert_eq!(
            tree.screen_reader_state(),
            crate::ScreenReaderState::Inactive
        );
        assert!(!tree.explore_by_touch_active());
    }

    #[test]
    fn a_repeated_detach_notice_does_not_clobber_a_fresh_os_reading() {
        // `teksilo-app` pushes the attachment flag every frame. Only the
        // true→false edge counts: if every `false` forced `Inactive`, an OS
        // query that found a screen reader would be undone on the next frame.
        let mut tree = WidgetTree::new();
        tree.set_at_client_attached(false);
        tree.set_screen_reader_state(crate::ScreenReaderState::Active);
        tree.set_at_client_attached(false);
        assert_eq!(tree.screen_reader_state(), crate::ScreenReaderState::Active);
    }

    // ── announcements ─────────────────────────────────────────────────────

    #[test]
    fn a_density_switch_announces_once_in_the_applications_own_words() {
        use teksilo_tokens::TargetDensity;
        let mut tree = tree_with_one_widget();
        tree.set_density_announcement(Some(std::rc::Rc::new(|d: TargetDensity| {
            format!("layout {d:?}")
        })));
        let _ = tree.sync_accessibility();

        tree.set_input_density(TargetDensity::Comfortable);
        let update = tree.sync_accessibility();
        assert_eq!(
            announced_by_consumer(&update),
            vec![("layout Comfortable".to_string(), accesskit::Live::Polite)]
        );

        // Once per *switch*: setting the density it already has says nothing.
        let _ = tree.sync_accessibility(); // let the announcer retract
        tree.set_input_density(TargetDensity::Comfortable);
        let update = tree.sync_accessibility();
        assert_eq!(announced_by_consumer(&update), Vec::new());
    }

    #[test]
    fn a_density_switch_is_silent_without_a_registered_wording() {
        // The framework cannot word this itself: `teksilo-i18n` depends on this
        // crate, so an English sentence is all core could hardcode, and an
        // English sentence spoken into a French screen reader is worse than
        // silence.
        use teksilo_tokens::TargetDensity;
        let mut tree = tree_with_one_widget();
        let _ = tree.sync_accessibility();
        tree.set_input_density(TargetDensity::Comfortable);
        let update = tree.sync_accessibility();
        assert_eq!(announced_by_consumer(&update), Vec::new());
    }

    #[test]
    fn an_announcement_stays_quiet_when_the_widget_already_speaks() {
        let mut tree = WidgetTree::new();
        use crate::widget_builder::WidgetBuilder;
        let widget = tree.add(
            FillWidget::new()
                .label("Saved")
                .access_live(accesskit::Live::Polite),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        // Build the tree the check reads.
        let _ = tree.sync_accessibility();

        assert!(
            !tree.announce_unless_widget_speaks(widget, "Saved"),
            "a widget with its own live region must not be doubled"
        );
        let update = tree.sync_accessibility();
        // One live node in the filtered tree — the widget's own. If the
        // framework had queued its copy, its announcer node would have entered
        // the filtered tree beside it and the user would hear "Saved" twice.
        let live = live_nodes_in_filtered_tree(&update);
        assert_eq!(live.len(), 1, "exactly one voice, not two: {live:?}");
    }

    #[test]
    fn an_announcement_goes_ahead_when_the_widget_is_silent() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().label("Row"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();

        assert!(tree.announce_unless_widget_speaks(widget, "Moved to 3 of 12"));
        let update = tree.sync_accessibility();
        assert_eq!(
            announced_by_consumer(&update),
            vec![("Moved to 3 of 12".to_string(), accesskit::Live::Polite)]
        );
    }

    #[test]
    fn a_tree_that_records_no_announcements_still_hears_the_widget_speak() {
        // The check reads the last update through the consumer on the spot,
        // not through the announcement ring's replay, which a window of an
        // application nobody drives does not keep. The long-press menu
        // announcement goes through here in every application.
        let mut tree = WidgetTree::new();
        tree.set_records_announcements(false);
        use crate::widget_builder::WidgetBuilder;
        let widget = tree.add(
            FillWidget::new()
                .label("Saved")
                .access_live(accesskit::Live::Polite),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();
        assert!(!tree.announce_unless_widget_speaks(widget, "Saved"));
    }

    #[test]
    fn a_live_region_no_platform_hears_does_not_count_as_speaking() {
        // A `Status` takes its name from its label. One that carries its text
        // only as a value has no name, so every adapter stays silent about it,
        // and letting it stand in for the framework's message left the user
        // with nothing at all.
        let mut tree = WidgetTree::new();
        use crate::widget_builder::WidgetBuilder;
        let widget = tree.add(
            FillWidget::new()
                .access_role(accesskit::Role::Status)
                .access_value("Saved")
                .access_live(accesskit::Live::Polite),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();
        assert!(tree.announce_unless_widget_speaks(widget, "Saved"));
    }

    #[test]
    fn a_hidden_live_region_does_not_count_as_speaking() {
        // Hidden is outside the filtered tree, where no adapter announces.
        let mut tree = WidgetTree::new();
        use crate::widget_builder::WidgetBuilder;
        let widget = tree.add(
            FillWidget::new()
                .label("Saved")
                .access_live(accesskit::Live::Polite)
                .access_hidden(true),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();
        assert!(tree.announce_unless_widget_speaks(widget, "Saved"));
    }

    #[test]
    fn a_live_region_around_the_widget_does_not_count_as_the_widget_speaking() {
        // A button inside somebody else's live region, a toast's action or a
        // row of a live palette, inherits the region's politeness: the adapters
        // would speak it if its own name changed. That is the region talking,
        // not the button, and the menu the button just opened is still news
        // nobody else is giving.
        let mut tree = WidgetTree::new();
        use crate::widget_builder::WidgetBuilder;
        let region = tree.add(
            StackWidget::new()
                .access_role(accesskit::Role::Group)
                .access_label("Notification")
                .access_live(accesskit::Live::Polite),
        );
        let button = tree.add_child(
            region,
            FillWidget::new()
                .label("Annuler")
                .access_role(accesskit::Role::Button),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();
        assert!(tree.announce_unless_widget_speaks(button, "Menu des actions ouvert"));
    }

    #[test]
    fn a_live_region_with_no_text_does_not_count_as_speaking() {
        // A live region that is present but empty announces nothing on any
        // platform, so it must not silence the framework's message.
        let mut tree = WidgetTree::new();
        use crate::widget_builder::WidgetBuilder;
        let widget = tree.add(FillWidget::new().access_live(accesskit::Live::Polite));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();
        assert!(tree.announce_unless_widget_speaks(widget, "Something happened"));
    }

    /// A widget that emits a live-region **synthetic** child, the way
    /// `teksilo-scene` marks a scene item as a live region: a named item
    /// pushed through `push_scene_child`, then made live.
    ///
    /// It used to be an annotation child, a `Role::Comment` holding its text
    /// as a value. That node has no name, so no platform speaks it, and the
    /// check this fixture feeds was passing on a region nobody could hear.
    #[derive(Debug)]
    struct SpeakingSynthetic;

    impl crate::widget::Widget for SpeakingSynthetic {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn accessibility(&self, builder: &mut AccessNodeBuilder) {
            builder.set_role(accesskit::Role::Group);
            let child = builder.push_scene_child(
                1,
                crate::accessibility::SyntheticKind::SceneItem,
                |item| {
                    item.set_role(accesskit::Role::Status);
                    item.set_name("2 items selected");
                },
            );
            builder.with_collected_node(child, |node| node.set_live(accesskit::Live::Polite));
        }
    }

    #[test]
    fn a_live_synthetic_child_counts_as_the_widget_speaking() {
        // A scene can mark one of its lightweight items as a live region. The
        // item has no widget of its own, so the check has to resolve it back
        // to its owner through the synthetic-parent map — the same map the
        // AccessKit action router uses.
        let mut tree = WidgetTree::new();
        let widget = tree.add(SpeakingSynthetic);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let update = tree.sync_accessibility();
        assert!(
            update
                .nodes
                .iter()
                .any(|(id, _)| crate::accessibility::is_synthetic(*id)),
            "the fixture must actually emit a synthetic child"
        );
        assert!(!tree.announce_unless_widget_speaks(widget, "2 items selected"));
    }

    #[test]
    fn a_descendants_live_region_silences_its_ancestor_too() {
        // The check is over the whole subtree: a composite whose inner status
        // line speaks is a composite that speaks.
        let mut tree = WidgetTree::new();
        let container = tree.add(StackWidget::new());
        use crate::widget_builder::WidgetBuilder;
        let inner = FillWidget::new()
            .label("3 results")
            .access_live(accesskit::Live::Polite);
        tree.add_child(container, inner);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();
        assert!(!tree.announce_unless_widget_speaks(container, "3 results"));
    }
}
