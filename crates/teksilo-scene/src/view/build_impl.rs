// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`SceneView::build_impl`] — the `build()` entry point for `SceneView`.
//!
//! Runs on every rebuild: drains pending item-move and marquee-commit closures,
//! materialises or destroys heavyweight widgets for `Once`/`Delegated` scene
//! entries, sorts them by z-order, wires all reactive signal bindings and
//! event-handler sets (scroll, pinch, drag, pointer-hover), and gates
//! AccessKit re-walks behind a mutation-version delta to avoid per-frame AT
//! churn during animated pan/zoom.

use super::*;
use teksilo_core::widget_builder::WidgetBuilder as _;

impl SceneView {
    pub(super) fn build_impl(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // A transaction is a synchronous scope. One still open when a frame
        // starts is a guard that was stashed somewhere instead of dropped, and
        // the symptom — edits that never reach the edit sink, salvage that
        // accumulates — points nowhere near the cause. Nothing can catch it in
        // release, but this catches the shape it actually takes.
        debug_assert_eq!(
            self.model.open_transaction_depth(),
            0,
            "a SceneTransaction was still open when SceneView::build ran. A \
             transaction groups one synchronous burst of edits and commits when \
             its guard drops; holding one across a frame means its record never \
             reaches the edit sink."
        );
        // Drain any pending drag-to-move commit. The drop closure
        // queued `(target_id, delta)` and bumped `reconcile_dirty` which
        // flagged this widget for rebuild. Translate the dragged
        // item's `local_pos` by the queued delta — descendants
        // follow automatically because their `local_pos` is
        // unchanged but their `scene_pos` derives from the parent
        // chain. Clear `drag_target` here (not on `Ended`) so paint
        // keeps translating the item to its dragged position until
        // the move actually lands; otherwise the item would visibly
        // "snap back" between drag-end and the rebuild.
        // Drain a committed transform. One gesture posted one delta, and this
        // applies it through one `Scene::apply_transform_delta` — the single
        // place the controller writes the model, which is what makes the
        // transaction boundary structural instead of a convention. An app that
        // wants the edit to be reversible records it from
        // `TransformConfig::on_end`; the history belongs to the data layer.
        let pending_transform = self.transform_rt.pending_commit.borrow_mut().take();
        if let Some((roots, delta)) = pending_transform {
            // One gesture, one transaction, stamped `User`: without this the
            // app cannot tell a finished transform from a programmatic move,
            // because both arrive as bare geometry changes. The guard is held
            // outside every model borrow, so the edit sink it commits to runs
            // with the scene free.
            let _txn = self.model.user_edit();
            self.model.apply_transform_delta(&roots, &delta);
        }

        if let Some((target_id, delta)) = self.pending_item_move.take() {
            // The whole selection when the grab was on a selected item — see
            // `SceneView::drag_group`, which the paint feedback reads too. One
            // `User` transaction for the whole group, so undoing a multi-item
            // drag is one step rather than one per item.
            let _txn = self.model.user_edit();
            for id in self.drag_group(target_id) {
                if let Some(local_pos) = self.model.local_pos(id) {
                    let new_local_pos = Point::new(local_pos.x + delta.x, local_pos.y + delta.y);
                    self.model.set_local_pos(id, new_local_pos);
                }
            }
            self.drag_target.set(None);
        }

        // Drain any pending marquee commit posted by the on_drag
        // closure on its `Ended` branch, then clear the in-flight
        // marquee Cell so paint stops overlaying the rect. Without
        // this the lasso would linger on screen until something
        // else triggered a layout pass (next user drag, etc.).
        let pending_marquee = self.pending_marquee_commit.borrow_mut().take();
        if let Some((region, mode, additive)) = pending_marquee {
            {
                let scene = self.model.0.borrow();
                self.selection.commit_marquee_region(
                    &scene,
                    &region,
                    mode,
                    self.view_scale(),
                    additive,
                );
            }
            self.marquee.set(None);
        }

        // Drain the payload-dirty set (filled by the item_change observer on
        // `ItemChange::PayloadChanged`). Each `Delegated` item whose payload
        // changed is destroyed here so the materialise loop below rebuilds it
        // via the delegate with the fresh payload. `Once` / removed ids are
        // left untouched — a single-view widget has no source to rebuild from.
        let payload_dirty_ids: Vec<ItemId> = self.payload_dirty.borrow_mut().drain().collect();
        for id in payload_dirty_ids {
            if self.model.payload(id).is_some()
                && let Some(wid) = self.materialized.remove(&id)
            {
                ctx.destroy_subtree(wid);
                self.widget_to_item.remove(&wid);
            }
        }

        // --- AccessKit re-walk gate (version delta) ----------------------
        // Snapshot the model mutation version *before* refreshing dynamic
        // bounds. If it advanced since the build that last walked AT, a discrete
        // mutation happened — an app add / remove / move / reparent, a logical-AT
        // change, or the drag-to-move drained just above — so the (separate)
        // AccessKit tree must be re-walked. The per-frame dynamic-bounds churn
        // from `refresh_dynamic_bounds` below is deliberately *excluded* from
        // this comparison: an actively-animating `add_item_dynamic` item rebuilds
        // every frame, and re-walking AT 60×/s for sub-pixel bounds drift is
        // pure waste a screen reader can't use. `None` (first build) compares
        // unequal, so the initial AT population is never gated out.
        let version_before_refresh = self.model.mutation_version();
        let structural_at_change = self.last_at_version != Some(version_before_refresh);

        // Pull fresh `local_bounds` for every item flagged
        // `dynamic_bounds` (added via [`Scene::add_item_dynamic`]).
        // Static items pay nothing here; dynamic items get their
        // signal-driven AABBs read back into the entry + spatial
        // index so hit-test and viewport-cull stay correct.
        let dynamic_changed = self.model.refresh_dynamic_bounds();
        // The one AT update the version gate would otherwise miss: when a
        // dynamic-bounds animation *settles* (changing last build, steady now),
        // walk its final bounds into AT exactly once so the resting geometry is
        // correct for assistive tech.
        let dynamic_settled = self.dynamic_churning && !dynamic_changed;
        self.dynamic_churning = dynamic_changed;
        // Baseline for the next build's `structural_at_change` test: the version
        // *after* this build's own (excluded) dynamic-bounds churn. Every scene
        // mutation inside build() happens at or above this point (the drains and
        // the refresh); materialise / orphan-reap / z-sort below only touch the
        // arena, not the Scene model — so this snapshot is stable to end-of-build.
        self.last_at_version = Some(self.model.mutation_version());

        // Bind the drag-rebuild signal so the next drop triggers a
        // rebuild and the drains above run. `BindingLevel::Rebuild`
        // is the level that re-runs `build()` on signal change.
        self.reconcile_dirty
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Bind the appearance signal at `RepaintOnly`: a lightweight item's
        // live colour/style change (via `set_item_fill` / `set_item_stroke`)
        // dirties paint only — `paint_band` re-runs `item.paint` and re-resolves
        // the `ColorProp` — with no relayout or rebuild.
        self.appearance_dirty.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Bind the measured-size signal at `Relayout`: a content-driven size
        // change re-places children (which is where the measurement itself
        // lives) without re-running `build()`. See `SceneView::measure_dirty`.
        self.measure_dirty.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        // The transform controller's two triggers, and the reason they are two.
        //
        // `tick` is bumped on every session change and bound at **Relayout**:
        // `place_children` re-runs on a relayout and on nothing weaker, and the
        // heavyweight tier's preview lives there. A repaint-only trigger — which
        // is all a plain `Cell` can ever be — would have previewed the frame and
        // the lightweight items while the cards sat still until the commit.
        //
        // `at_tick` is bumped only on discrete steps (session start and end,
        // keyboard roving) and bound at **AccessibilityOnly**. Deliberately not
        // per pointer sample: a full accessibility re-walk at pointer rate would
        // cost the live set on every sample to move ten nodes, and no
        // screen-reader user is driving a pointer drag. The handles' published
        // rectangles are therefore stale for the duration of a pointer gesture,
        // and the commit's own model change re-walks at the end.
        //
        // The two ticks are what the controller writes. Everything below them
        // is what the controller *reads*: the memo already keys on all three
        // (see `ChromeKey`, whose doc says so), but a right-keyed memo only
        // answers correctly when something asks — and nothing asked. Each is
        // bound twice on purpose, because the answer feeds two walkers that no
        // single level covers: `place_children` and `post_paint` follow
        // `Relayout`, the AccessKit walk follows `AccessibilityOnly` and
        // nothing else, and the two buckets are independent in the registry.
        if let Some(cfg) = self.transform.clone() {
            let id = ctx.self_id();
            let registry = ctx.binding_registry();
            self.transform_rt
                .tick
                .bind_to(id, registry, BindingLevel::Relayout);
            self.transform_rt
                .at_tick
                .bind_to(id, registry, BindingLevel::AccessibilityOnly);
            // `enabled` decides whether there is any chrome at all. Unbound, a
            // toolbar toggle wired to `enabled_signal()` — which the setter
            // invites — left the frame and its handles painted and left the
            // published tree serving a handle whose actions the flag had just
            // made refuse: a screen reader offering a control that silently
            // does nothing.
            cfg.enabled.bind_to(id, registry, BindingLevel::Relayout);
            cfg.enabled
                .bind_to(id, registry, BindingLevel::AccessibilityOnly);
            // The selection decides what the chrome is drawn *around*. Bound
            // only when a controller is installed, and that is not a shortcut:
            // it is the one thing this view publishes that depends on the
            // selection. A lightweight item's own colours bind the same signal
            // themselves (see `SceneSelection`), and no item's AccessKit node
            // carries selection state — so without a controller there is
            // nothing here for a selection change to invalidate.
            //
            // A pointer selection change happened to be covered, because the
            // press that made it also relayouts; a *programmatic* one —
            // "select all", a search result, or the second pane of a shared
            // `SceneSelection` — was not, and both panes went on describing the
            // previous selection at its previous position.
            let selection = self.selection.selection_signal();
            selection.bind_to(id, registry, BindingLevel::Relayout);
            selection.bind_to(id, registry, BindingLevel::AccessibilityOnly);
            // The two reactive resize knobs. They bear only on a *live*
            // gesture's resolution, which a pointer sample re-derives anyway —
            // but a keyboard or assistive-technology session takes no samples,
            // so an app toggling "lock aspect ratio" from a toolbar mid-gesture
            // would have shown the old preview until the next key. Free when
            // the knob is a plain `bool`.
            cfg.keep_ratio
                .register_if_bound(id, registry, BindingLevel::Relayout);
            cfg.centered_scaling
                .register_if_bound(id, registry, BindingLevel::Relayout);
            // The names the handles announce, for the app that binds a plain
            // `Signal<String>` rather than a `tr!` — a locale switch dirties
            // the accessibility tree by itself, an arbitrary signal does not.
            cfg.labels
                .register_bindings(id, registry, BindingLevel::AccessibilityOnly);
        }

        // Magnetism's `enabled` is the same knob one door over, and it was the
        // same defect: `MagnetismConfig::enabled_signal()` extends the same
        // invitation, `SceneView::magnetism_active` reads it at use time, and it
        // gates both the marker post-paint and the synthetic magnet AT nodes.
        // Unbound, turning magnetism off on a live view left its markers drawn
        // and left a screen reader still offered a "Port" node — while a fresh
        // walk correctly returned none, which is the tell that the gate works
        // and only the invalidation was missing.
        if let Some(cfg) = self.magnetism.clone() {
            let id = ctx.self_id();
            let registry = ctx.binding_registry();
            cfg.enabled.bind_to(id, registry, BindingLevel::Relayout);
            cfg.enabled
                .bind_to(id, registry, BindingLevel::AccessibilityOnly);
        }

        // Wire the item-coordinate cache invalidation observer.
        // Cached frames are recorded in **local** coordinates, so
        // only changes that alter the local-coord paint output
        // dirty an entry: `LocalBoundsChanged` (geometry redraw)
        // and `Removed` (entry orphaned). Opacity, transform, z,
        // local_pos, flags don't bake into the cached frame —
        // they're applied as wrapping scopes at replay time.
        // The handle is held by `Self`; dropping the previous
        // handle on rebuild un-installs the prior observer before
        // re-installing.
        {
            let cache = self.item_cache.clone();
            let reconcile_dirty = self.reconcile_dirty.clone();
            let appearance_dirty = self.appearance_dirty.clone();
            let measure_dirty = self.measure_dirty.clone();
            let measure_state = self.measure_state_handle();
            let payload_dirty = self.payload_dirty.clone();
            let hit_sync = self.hit_sync.clone();
            let handle = self
                .model
                .item_change_signal()
                .observe(move |scene_change| {
                    use crate::scene::ItemChange;
                    // The view reconciles from the change itself; the envelope's
                    // transaction id, source and history are for the app's data
                    // layer, and `ephemeral` is already accounted for by the
                    // AT-re-walk gate's `structural_version`.
                    let change = &scene_change.change;
                    // Hit-snapshot invalidation, recorded FIRST and for every
                    // variant — including the ones that return early below. It also
                    // counts the delivery, which is what proves to the next layout
                    // pass that this view saw the whole change stream and may
                    // therefore patch its snapshots instead of rebuilding them.
                    hit_sync.borrow_mut().record(change);
                    // The item cache holds *local-coordinate* paint output, so only
                    // a geometry change or a removal can invalidate a cached frame;
                    // pos / transform / opacity / z / layer / flags are re-applied
                    // as wrapping scopes at replay and don't bake into the cache.
                    match *change {
                    // An entry that is gone takes its measurement history with
                    // it: the per-view map is keyed by `ItemId`, and `ItemId`s
                    // are never reused, so a stale row is a leak rather than a
                    // wrong answer — but a leak of one row per removed card,
                    // which a long editing session accumulates.
                    ItemChange::Removed { id } => {
                        cache.borrow_mut().evict(id);
                        measure_state.borrow_mut().remove(&id);
                    }
                    ItemChange::LocalBoundsChanged { id, .. }
                    // A different item box at the same id repaints from
                    // scratch: the cached frame is the *old* item's drawing.
                    // No measurement history to retire with it —
                    // `Scene::replace_item` refuses a heavyweight entry, and
                    // only a heavyweight entry can carry a `SizePolicy`. The
                    // heavyweight twin of this change is `PayloadChanged`,
                    // which does.
                    | ItemChange::ItemReplaced { id, .. } => {
                        cache.borrow_mut().evict(id);
                    }
                    // `IS_ENABLED` feeds `ColorProp::resolve` (disabled-role
                    // colours), so it bakes into a cached frame just like the
                    // geometry does — a flag flip must evict, or the item would
                    // replay its stale enabled-state colours.
                    ItemChange::FlagsChanged { id, old, new } => {
                        use crate::flags::ItemFlags;
                        if old.contains(ItemFlags::IS_ENABLED)
                            != new.contains(ItemFlags::IS_ENABLED)
                        {
                            cache.borrow_mut().evict(id);
                        }
                    }
                    // A `Delegated` item's data changed: queue a targeted rebuild
                    // so the next build re-invokes the delegate for just that id.
                    // The delegate hands back a **new widget** — a new arena
                    // node, born `needs_layout` — so the pass that first measures
                    // it is one `MeasureTrack` already reads as externally
                    // driven, and the newcomer's answer is taken rather than
                    // weighed against its predecessor's. Nothing to retire here.
                    ItemChange::PayloadChanged { id, .. } => {
                        payload_dirty.borrow_mut().insert(id);
                    }
                    // Paint-only appearance change: colour bakes into the cached
                    // local-coord frame, so evict it, then repaint WITHOUT a
                    // rebuild — return before the shared `reconcile_dirty` bump.
                    ItemChange::AppearanceChanged { id, .. } => {
                        cache.borrow_mut().evict(id);
                        appearance_dirty.set(appearance_dirty.get().wrapping_add(1));
                        return;
                    }
                    // Who decides this entry's box changed, so the question the
                    // view asks its body changed with it — an `Intrinsic` card
                    // is asked for both axes at no width at all, where a
                    // `HeightForWidth` one is asked for a height at the width
                    // the model holds. The same widget is still there, so the
                    // pass that answers the new question can be a perfectly
                    // clean one, and a retained history would read the new
                    // answer as the old body contradicting itself and resolve
                    // the two together — leaving a card that should have
                    // shrink-wrapped at its old width for ever. The history goes
                    // with the question. Then fall through to the ordinary
                    // reconcile, because this one *is* a structural change and
                    // the accessibility rectangles that follow from it have to
                    // be re-walked.
                    ItemChange::SizePolicyChanged { id, .. } => {
                        measure_state.borrow_mut().remove(&id);
                    }
                    // A size **derived** from content: geometry moved, so the
                    // cached local-coord frame is stale, but nothing was
                    // materialised or reaped and no delegate needs re-running.
                    // Relayout and return, before the `reconcile_dirty` bump
                    // below — a full rebuild per line wrap is the storm a note
                    // page would otherwise produce. The AT tree is not re-walked
                    // either: `set_measured_size` counts itself out of
                    // `structural_version`, so `structural_at_change` above
                    // stays false, and the placement rectangles the walker
                    // projects are recomputed by the relayout regardless.
                    ItemChange::MeasuredSizeChanged { id, .. } => {
                        cache.borrow_mut().evict(id);
                        measure_dirty.set(measure_dirty.get().wrapping_add(1));
                        return;
                    }
                    _ => {}
                }
                    // EVERY model mutation drives a reconcile pass. A relayout
                    // re-runs `build()` (materialise pending widgets, reap orphaned
                    // ones), re-places children (so screen-projected AccessKit
                    // bounds track moves/transforms), and — via `build()`'s
                    // `request_accessibility_update()` — forces an AT re-walk. A
                    // relayout alone no longer re-walks AT, and the visual scene
                    // and the *separate* AccessKit tree must both follow add /
                    // remove / move / reparent / visibility / opacity / z / layer:
                    // letting any variant fall through silently would desync
                    // assistive tech (and paint) from the model.
                    reconcile_dirty.set(reconcile_dirty.get().wrapping_add(1));
                });
            *self._item_cache_observer.borrow_mut() = Some(handle);
        }

        // Second observer: logical-AT-structure mutations (groups, parents,
        // relations, live, landmarks, categories) don't fire `item_change_signal`
        // because they aren't item geometry. Drive a reconcile pass so the next
        // `build()` calls `request_accessibility_update()` and the (separate)
        // AccessKit tree re-walks — even for a mutation with no visual change.
        {
            let reconcile_dirty = self.reconcile_dirty.clone();
            let handle = self.model.a11y_change_signal().observe(move |_| {
                reconcile_dirty.set(reconcile_dirty.get().wrapping_add(1));
            });
            *self._a11y_observer.borrow_mut() = Some(handle);
        }

        // An item's `ColorProp` resolves against `ctx.theme` (already projected
        // for window-active / high-contrast) at paint time, so a theme swap or a
        // window-activation flip changes an item's *painted* colours — and those
        // colours bake into a `CacheMode::ItemCoordinate` frame, which is keyed
        // only by `(id, raster_scale)`. The framework repaints on both events, but
        // `paint_band` would replay the stale cached frame and the item would keep
        // its old (e.g. still-saturated) colour while every widget around it
        // desaturates. Clear the cache so the next paint re-records at the new
        // theme / window-active state. Items on the default `CacheMode::None`
        // re-resolve every paint and are unaffected either way.
        {
            let cache = self.item_cache.clone();
            let handle = ctx.theme_signal().observe(move |_| {
                cache.borrow_mut().clear();
            });
            ctx.own_handle(handle);
        }
        {
            let cache = self.item_cache.clone();
            let handle = ctx.window_active_signal().observe(move |_| {
                cache.borrow_mut().clear();
            });
            ctx.own_handle(handle);
        }

        // Materialise heavyweight widgets. Two paths, neither holding a model
        // borrow across `ctx.add_boxed` or a delegate call (the reentrancy
        // contract): both `drain_all_once` and `delegated_payloads` return owned
        // Vecs with the model borrow already dropped.
        //
        // 1. Single-view `Once` widgets: drain the boxed instance. Only the
        //    first `SceneView` over a shared model gets it; a second view's
        //    drain returns nothing for that id (it's single-view by design).
        for (id, widget) in self.model.drain_all_once() {
            let wid = ctx.add_boxed(widget);
            self.materialized.insert(id, wid);
            self.widget_to_item.insert(wid, id);
        }

        // 2. Multi-view `Delegated` items: each view builds its OWN instance via
        //    its delegate. Already-materialised ids are skipped (the
        //    payload-dirty drain above destroyed any that need rebuilding, so
        //    they fall through here as fresh).
        let delegated = self.model.delegated_payloads();
        if let Some(delegate) = self.delegate.clone() {
            for (id, payload) in &delegated {
                if self.materialized.contains_key(id) {
                    continue;
                }
                if let Some(widget) = delegate(&**payload, *id) {
                    let wid = ctx.add_boxed(widget);
                    self.materialized.insert(*id, wid);
                    self.widget_to_item.insert(wid, *id);
                }
            }
        } else {
            debug_assert!(
                delegated
                    .iter()
                    .all(|(id, _)| self.materialized.contains_key(id)),
                "SceneView has `Delegated` items but no delegate installed — \
                 call `.delegate_typed::<P>(..)` (or `.delegate(..)`) before adding it to the tree"
            );
        }

        // Assemble child ids in scene (entry) order, then reap orphans — reusing
        // one heavyweight-id snapshot. A removed entry is absent from
        // `heavy_ids`, so it never enters `child_ids`; SceneView preserves its
        // children on rebuild, so we must destroy our own orphan arena nodes or
        // they (and their signal / animation / shortcut registrations) leak.
        let heavy_ids = self.model.heavyweight_ids();
        let mut child_ids: Vec<WidgetId> = heavy_ids
            .iter()
            .filter_map(|id| self.materialized.get(id).copied())
            .collect();
        let live_widget_ids: std::collections::HashSet<ItemId> = heavy_ids.into_iter().collect();
        if self.materialized.len() > live_widget_ids.len() {
            let orphans: Vec<(ItemId, WidgetId)> = self
                .materialized
                .iter()
                .filter(|(item_id, _)| !live_widget_ids.contains(*item_id))
                .map(|(item_id, wid)| (*item_id, *wid))
                .collect();
            for (item_id, wid) in orphans {
                ctx.destroy_subtree(wid);
                self.materialized.remove(&item_id);
                self.widget_to_item.remove(&wid);
            }
        }

        // A relayout no longer re-walks the AccessKit tree, and this build may
        // have materialised, reaped, moved, or reparented scene content — each
        // changes the (separate) AT tree or its screen-projected bounds. Re-walk
        // when the model actually changed since the last walk
        // (`structural_at_change`, computed from the mutation-version delta
        // above) or a dynamic-bounds animation just settled (`dynamic_settled`).
        // `build()` is otherwise interaction-driven — pan / zoom animate via
        // relayout, not rebuild — and a per-frame `add_item_dynamic` rebuild is
        // gated out here, so this is not a per-frame AT cost.
        if structural_at_change || dynamic_settled {
            ctx.request_accessibility_update();
        }

        // Paint nodes for the `Interleaved` band. One per item, materialised
        // and reaped on the same rule as the heavyweight cards above — a band
        // change is an `ItemChange`, so this reconciles on the next build.
        //
        // They are children, so the arena paints them *between* the cards, at
        // the z the sort below gives them. They are paint-only: transparent to
        // the pointer, absent from the accessibility child list, not focusable.
        // See `view::paint_node`.
        let interleaved = self.model.0.borrow().interleaved_ids();
        let live_interleaved: std::collections::HashSet<ItemId> =
            interleaved.iter().copied().collect();
        if self.interleaved_nodes.len() > live_interleaved.len()
            || self
                .interleaved_nodes
                .keys()
                .any(|id| !live_interleaved.contains(id))
        {
            let orphans: Vec<(ItemId, WidgetId)> = self
                .interleaved_nodes
                .iter()
                .filter(|(item_id, _)| !live_interleaved.contains(*item_id))
                .map(|(item_id, wid)| (*item_id, *wid))
                .collect();
            for (item_id, wid) in orphans {
                ctx.destroy_subtree(wid);
                self.interleaved_nodes.remove(&item_id);
            }
        }
        for id in &interleaved {
            if self.interleaved_nodes.contains_key(id) {
                continue;
            }
            let proxy = super::paint_node::SceneBandProxy::new(self.paint_bridge(), *id);
            let wid = ctx.add(proxy.event_pass_through(true));
            self.interleaved_nodes.insert(*id, wid);
        }
        child_ids.extend(
            interleaved
                .iter()
                .filter_map(|id| self.interleaved_nodes.get(id).copied()),
        );

        // Paint order for the arena's child walk. The comparator is
        // [`PaintKey`](crate::PaintKey) — the *same* value every hit test in
        // the crate compares — so "what the user sees on top" and "what the
        // pointer picks" stay one rule across both tiers. It used to be a bare
        // `z` compare over the cards alone, which was the same order for the
        // cards and had nothing to say about an interleaved item.
        //
        // Reordering `node.children` (rather than destroying / recreating the
        // widgets) preserves each card's focus, text-edit and animation state.
        let keymap: HashMap<WidgetId, crate::PaintKey> = {
            let scene = self.model.0.borrow();
            self.widget_to_item
                .iter()
                .filter_map(|(wid, id)| scene.paint_key(*id).map(|k| (*wid, k)))
                .chain(
                    self.interleaved_nodes
                        .iter()
                        .filter_map(|(id, wid)| scene.paint_key(*id).map(|k| (*wid, k))),
                )
                .collect()
        };
        // An id with no key sorts to the bottom of the widget rank rather than
        // being dropped: a card whose entry has gone is still a live arena node
        // this pass, and losing it here would lose its child.
        let floor = crate::PaintKey::rank_floor(crate::RANK_WIDGET);
        child_ids.sort_by_key(|wid| keymap.get(wid).copied().unwrap_or(floor));
        // The same snapshot the sort above read, published for
        // `accepts_child_hit` — which runs on the pointer's hot path and cannot
        // re-enter the model to ask. One snapshot, so the order the arena
        // paints in and the order the veto compares against cannot drift apart.
        *self.child_keys.borrow_mut() = keymap;

        // The wet surface, last — over every card and every interleaved item,
        // under the `Over` band and the view's own chrome. See the
        // `view::paint_node` module docs for the whole order.
        if let Some(layer) = self.wet_layer.clone() {
            let wid = match self.wet_node {
                Some(wid) => wid,
                None => {
                    let node =
                        super::paint_node::WetLayerNode::new(self.paint_bridge(), layer.clone());
                    let wid = ctx.add(node.event_pass_through(true));
                    self.wet_node = Some(wid);
                    // The window, so `WetLayer::request_repaint` can tell this
                    // mount from one the same layer has in another window —
                    // whose `WidgetId`s are a different arena's slot keys and
                    // would mark the wrong node here. `None` in a headless
                    // tree; see `WetMount`.
                    self.wet_mount = Some(layer.attach(wid, ctx.window().map(|w| w.id())));
                    wid
                }
            };
            child_ids.push(wid);
        }

        // Register the four animated signals with the scheduler so
        // they participate in idle gating (paint-epoch visibility,
        // window-inactive pause, drop-cancel). Idempotent — a re-build
        // updates the owner registration in place.
        ctx.register_animated_signal(&self.pan_x);
        ctx.register_animated_signal(&self.pan_y);
        ctx.register_animated_signal(&self.zoom);
        ctx.register_animated_signal(&self.rotation);

        // Walk every lightweight item and let it register its own
        // reactive bindings against this SceneView. Items with
        // signal-bound state (e.g. `TextItem::text`) call
        // `signal.bind_to(scene_view_id, registry, RepaintOnly)`
        // here so a signal change dirties our paint and the next
        // walk reads the current value. Items without bindings
        // default to a no-op `register_bindings`.
        let self_id_for_items = ctx.self_id();
        {
            let scene = self.model.0.borrow();
            for entry in scene.entries.iter() {
                if let crate::scene::SceneEntryKind::Item(item) = &entry.kind {
                    item.register_bindings(ctx, self_id_for_items);
                }
            }
        }

        // Bind the four signals at Relayout on this node so
        // `place_children` re-runs and the viewport-cull set is
        // recomputed when pan/zoom/rotation change. The Repaint
        // binding from `set_content_transform` below is kept in addition;
        // it's what dirties the renderer's transform stack so
        // already-laid-out children re-paint at their new visual
        // positions.  Without this Relayout binding, a `pan` or
        // `zoom` change would only repaint the *currently visible*
        // children — items the cull collapsed to zero would stay
        // collapsed even if the new view brings them into view.
        let registry = ctx.binding_registry();
        let self_id_for_relayout = ctx.self_id();
        self.pan_x
            .bind_to(self_id_for_relayout, registry, BindingLevel::Relayout);
        self.pan_y
            .bind_to(self_id_for_relayout, registry, BindingLevel::Relayout);
        self.zoom
            .bind_to(self_id_for_relayout, registry, BindingLevel::Relayout);
        self.rotation
            .bind_to(self_id_for_relayout, registry, BindingLevel::Relayout);
        // …and once more at `AccessibilityOnly`, because a relayout does not
        // invalidate the AccessKit cache and a camera move changes nothing
        // else that would.
        //
        // Every rectangle this view publishes is camera-dependent: a
        // lightweight item's node is written in scene coordinates under the
        // view transform, and a heavyweight card's node carries that transform
        // itself (see the `set_transform` emission in the framework walker).
        // `sync_accessibility` — the function every platform adapter is fed,
        // as opposed to `accessibility_tree_snapshot` — hands back the cached
        // tree unless something set `a11y_dirty`, so without this the tree an
        // assistive technology holds keeps describing the camera position the
        // last *structural* change happened at. Measured before this binding
        // existed: pan the view 90 px and the published rectangle does not
        // move, for either tier.
        //
        // One derived signal rather than four: `view_transform_signal` changes
        // once per camera change however many of pan/zoom/rotation moved, and
        // a signal that resolves to the same transform notifies nobody. The
        // cost is one AT walk per frame of a drag-pan, bounded by the *live*
        // set (an off-screen card is parked and never walked), and the walk's
        // `TreeUpdate` comparison keeps `at_version` from bumping when the
        // published tree is unchanged.
        self.view_transform_signal.bind_to(
            self_id_for_relayout,
            registry,
            BindingLevel::AccessibilityOnly,
        );

        // The view-transform signal is constructed once in `new`
        // (so it's stable across rebuilds and exposable via
        // `view_transform_signal()`). Bind it as a `set_content_transform`
        // scope on this widget; the render walker pushes it around
        // our entire subtree. The composition folds `bounds.origin`
        // into the final translate so a SceneView at a non-zero
        // parent offset still maps scene-coord (sx, sy) to screen
        // (bounds.x + zoom*sx + pan.x, bounds.y + zoom*sy + pan.y).
        let self_id = ctx.self_id();
        // A *content* transform: the SceneView's bounds are a fixed screen
        // viewport and this pan/zoom only moves the scene content. Marking it
        // as such keeps the whole viewport hit-testable at any pan (a *self*
        // transform like Scale/Rotate would shift the hittable region with the
        // content — see `WidgetNode::content_transform`).
        ctx.set_content_transform(self_id, self.view_transform_signal.clone());
        // Capture for the AT-redirect auto-graft hook.
        // The hook is `&self`; without a stash here it has no way
        // to derive its own `WidgetId` to compute synthetic NodeIds.
        self.self_widget_id.set(Some(self_id));

        // Wire scroll + pinch handlers. Captures are by clone so they
        // outlive the build call. Reactive constraint signals
        // (pan_axes, zoom_range, pan_bounds, zoomable) are captured
        // as Signal clones — the closures read `.get()` per event,
        // so runtime mutations of the underlying signals take effect
        // on the next gesture without rebuilding the view.
        let prefers_reduced = ctx.prefers_reduced_motion();
        let line_height = self.line_height;
        let pan_dur = self.pan_anim_duration;
        let overscroll = self.overscroll_behavior;

        // Reusable tooltip surface for lightweight scene items. Items
        // have no `WidgetId`, so the per-widget `.tooltip()` attach path
        // (arena-hover-keyed, `NearAnchor`-positioned) can't be used.
        // Instead we keep ONE dormant `TooltipWidget` whose body is bound
        // to a `Signal<String>`; the hover seam below sets the text and
        // shows/dismisses it as a point-anchored (`AtPointer`) overlay,
        // mirroring how the per-item cursor override is applied. Resolving
        // the item's `LocalizedString` at show time keeps it locale-correct.
        let tooltip_text = ctx.signal(String::new());
        let tooltip_shown = ctx.signal(false);
        // Built the first time an item's tooltip is actually shown, not on every
        // rebuild of the view. See `teksilo_core::deferred_subtree::DeferredSubtree`.
        let tooltip_content_id = ctx.add_deferred(
            tooltip_shown.clone(),
            teksilo_widgets::TooltipWidget::bound(tooltip_text.clone()),
        );
        ctx.set_dormant(tooltip_content_id);
        let tooltip_fade = if prefers_reduced {
            None
        } else {
            Some(ctx.theme().motion.duration_fast)
        };
        // Scene hover is exploratory — the pointer sweeps across many items
        // while the eye pans — so lightweight-item tips use the heavier
        // (longer) tooltip dwell to avoid flashing during that sweep.
        let tooltip_delay = ctx.theme().motion.tooltip_delay_heavy;

        // Every grab tolerance in the view reads this snapshot. Safe to snapshot
        // because a density change marks the tree for rebuild, so a stale copy
        // cannot outlive the density it came from.
        let input_tokens = ctx.theme().input;

        let mut handlers = HandlerSet::new();
        handlers = self.register_pointer_handlers(
            handlers,
            self_id,
            super::gestures_impl::TooltipWiring {
                content_id: tooltip_content_id,
                text: tooltip_text,
                shown: tooltip_shown,
                fade: tooltip_fade,
                delay: tooltip_delay,
            },
            input_tokens,
        );

        // Registered unconditionally: the scroll slot also carries the reveal
        // arm, which a non-interactive view still owes a focused descendant.
        // The wheel and the keyboard camera inside it are gated on the same
        // flag as before — see `register_camera_handlers`.
        handlers = self.register_camera_handlers(
            handlers,
            line_height,
            pan_dur,
            overscroll,
            prefers_reduced,
            self.interactive,
        );
        if self.interactive {
            // The claim is what puts this node on the chain a synthesised pan
            // walks; without it the scroll handler above would answer a wheel
            // and never see a finger. Both axes are claimed unconditionally
            // even though the scene's `pan_axes` policy is a live signal — a
            // claim on an axis the policy has closed costs nothing, because
            // the handler zeroes that axis' delta and the resulting `Ignored`
            // re-offers the whole event to the next container outward. A
            // build-time snapshot of a signal that changes at runtime would
            // instead leave the surface deaf on an axis it had just re-opened.
            handlers = handlers.pan_claim(teksilo_core::pointer::touch_action::PanClaim {
                axes: teksilo_core::pointer::touch_action::PanAxes::BOTH,
                devices: teksilo_tokens::PointerKindMask::DIRECT,
                kinetic: true,
            });
        }

        // The on_drag handler drives both marquee / drag-to-move selection
        // AND magnetism (item-drag snap + port-drag wires), so register it
        // when either selection is enabled or magnetism is configured.
        if !matches!(
            self.selection.mode(),
            crate::selection::SceneSelectionMode::None
        ) || self.magnetism.is_some()
        {
            handlers = self.register_drag_handlers(handlers, input_tokens);
        }

        // The transform controller's accessibility-action route. Installed
        // whenever a controller exists — the enabled signal is live, and the
        // handler re-reads it, so a controller toggled on at runtime does not
        // need a rebuild to become reachable from assistive technology.
        handlers = self.register_transform_handlers(handlers, self_id);

        ctx.apply_self_handlers(handlers);

        child_ids
    }
}
