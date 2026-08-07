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

impl SceneView {
    pub(super) fn build_impl(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
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
        if let Some((target_id, delta)) = self.pending_item_move.take() {
            if let Some(local_pos) = self.model.local_pos(target_id) {
                let new_local_pos = Point::new(local_pos.x + delta.x, local_pos.y + delta.y);
                self.model.set_local_pos(target_id, new_local_pos);
            }
            self.drag_target.set(None);
        }

        // Drain any pending marquee commit posted by the on_drag
        // closure on its `Ended` branch, then clear the in-flight
        // marquee Cell so paint stops overlaying the rect. Without
        // this the lasso would linger on screen until something
        // else triggered a layout pass (next user drag, etc.).
        if let Some((rect, additive)) = self.pending_marquee_commit.take() {
            {
                let scene = self.model.0.borrow();
                self.selection.commit_marquee(&scene, rect, additive);
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
            let payload_dirty = self.payload_dirty.clone();
            let handle = self.model.item_change_signal().observe(move |change| {
                use crate::scene::ItemChange;
                // The item cache holds *local-coordinate* paint output, so only
                // a geometry change or a removal can invalidate a cached frame;
                // pos / transform / opacity / z / layer / flags are re-applied
                // as wrapping scopes at replay and don't bake into the cache.
                match *change {
                    ItemChange::Removed { id } | ItemChange::LocalBoundsChanged { id, .. } => {
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
                    ItemChange::PayloadChanged { id } => {
                        payload_dirty.borrow_mut().insert(id);
                    }
                    // Paint-only appearance change: colour bakes into the cached
                    // local-coord frame, so evict it, then repaint WITHOUT a
                    // rebuild — return before the shared `reconcile_dirty` bump.
                    ItemChange::AppearanceChanged { id } => {
                        cache.borrow_mut().evict(id);
                        appearance_dirty.set(appearance_dirty.get().wrapping_add(1));
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

        // Heavyweight z-order: sort the arena children by their scene-entry
        // z so higher-z cards paint later (on top). Equal-z keeps insertion
        // order (stable sort). This is the heavyweight-tier analogue of the
        // lightweight `sort_by_z` — `Scene::set_z` / `bring_to_front` on a
        // widget entry restacks the cards here, on the next rebuild.
        // Reordering `node.children` (rather than destroying / recreating the
        // widgets) preserves each card's focus, text-edit and animation state.
        let zmap: HashMap<ItemId, f32> = {
            let scene = self.model.0.borrow();
            self.widget_to_item
                .values()
                .map(|id| (*id, scene.z(*id).unwrap_or(0.0)))
                .collect()
        };
        child_ids.sort_by(|a, b| {
            let za = self
                .widget_to_item
                .get(a)
                .and_then(|id| zmap.get(id).copied())
                .unwrap_or(0.0);
            let zb = self
                .widget_to_item
                .get(b)
                .and_then(|id| zmap.get(id).copied())
                .unwrap_or(0.0);
            za.partial_cmp(&zb).unwrap_or(std::cmp::Ordering::Equal)
        });

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
        let tooltip_content_id =
            ctx.add(teksilo_widgets::TooltipWidget::bound(tooltip_text.clone()));
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

        let mut handlers = HandlerSet::new();
        handlers = self.register_pointer_handlers(
            handlers,
            self_id,
            tooltip_content_id,
            tooltip_text,
            tooltip_fade,
            tooltip_delay,
        );

        if self.interactive {
            handlers = self.register_scroll_pinch_key_handlers(
                handlers,
                line_height,
                pan_dur,
                overscroll,
                prefers_reduced,
            );
        }

        // The on_drag handler drives both marquee / drag-to-move selection
        // AND magnetism (item-drag snap + port-drag wires), so register it
        // when either selection is enabled or magnetism is configured.
        if !matches!(
            self.selection.mode(),
            crate::selection::SceneSelectionMode::None
        ) || self.magnetism.is_some()
        {
            handlers = self.register_drag_handlers(handlers);
        }

        ctx.apply_self_handlers(handlers);

        child_ids
    }
}
