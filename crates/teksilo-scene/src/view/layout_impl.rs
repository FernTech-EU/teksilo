// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Layout pass for [`SceneView`]: size negotiation, per-child placement at
//! scene coordinates, and drag / handler snapshot refresh.
//!
//! `layout_response_impl` reports the view's desired size to the parent and,
//! as a side-effect, refreshes the per-layout snapshots that pointer-event
//! dispatch and drag hit-testing rely on (these snapshots cannot live in
//! `place_children` alone because a scene with only lightweight items has no
//! heavyweight children and `place_children` would never run). `place_children_impl`
//! positions each materialised heavyweight child at its pure scene-coordinate
//! origin, mirroring `bounds.origin` into a signal so the renderer's transform
//! stack composes it in automatically; it also culls invisible children by
//! collapsing their layout size to zero.

use super::*;

impl SceneView {
    pub(super) fn layout_response_impl(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> LayoutResponse {
        // When `adopt_scene_size` is set, the view sizes itself to
        // the scene's resolved extent so the entire scene fits
        // inside the view's bounds. Falls back to `default_size`
        // when the scene has no extent declared and no items.
        // We size to the extent's width/height, NOT its right/bottom
        // — for scenes with items at non-origin (e.g. negative) scene
        // coordinates, right/bottom is inflated by the bounding rect's
        // origin offset.
        let (default_w, default_h) = if self.adopt_scene_size {
            match self.scene().scene_rect_extent() {
                Some(r) => (r.width, r.height),
                None => (self.default_size.width, self.default_size.height),
            }
        } else {
            (self.default_size.width, self.default_size.height)
        };
        let size = proposal.resolve(default_w, default_h);
        // Cache for `fit_to_content` and friends. `bounds_origin` is
        // refreshed in `place_children`, which runs whenever the
        // SceneView has at least one child — i.e. always in real
        // apps, since an empty SceneView doesn't render anything to
        // interact with. The set is gated by an equality check so
        // unchanged sizes don't spuriously fire viewport observers.
        if self.last_viewport.get() != size {
            self.last_viewport.set(size);
        }

        // Refresh the lightweight-bounds snapshot used
        // by the on_drag closure for hit-test. Done here (rather
        // than in `place_children`) because `place_children` only
        // runs when the SceneView has at least one heavyweight
        // child — a scene with only lightweight items would never
        // get its snapshot populated. `layout_response` runs every
        // layout pass regardless.
        {
            // Snapshot of *draggable* lightweight items. Decorative
            // items (background tiles, group chrome, connector paths,
            // captions) opt into drag via `.draggable(true)` on the
            // built-in builders or by overriding `is_draggable()` on
            // a custom impl; everything else stays anchored, which
            // is the default. Without this filter, every visible
            // RectItem would respond to drags and the scene would
            // feel unstable to the user.
            let mut snapshot = self.lightweight_bounds_snapshot.borrow_mut();
            snapshot.clear();
            // Snapshot draggable lightweight items' narrow-phase hit geometry
            // (AABB + shape predicate + transform) for the drag-start hit-test
            // and the grab cursor. Refreshed each layout pass so a parent move
            // between drag events doesn't leave the snapshot stale.
            let scene = self.model.0.borrow();
            for id in scene.ids() {
                let Some(item) = scene.item(id) else {
                    continue;
                };
                let Some(flags) = scene.flags(id) else {
                    continue;
                };
                if !flags.contains(crate::flags::ItemFlags::IS_DRAGGABLE) {
                    continue;
                }
                // `IS_VISIBLE` says an item is "neither painted nor
                // hit-tested"; `IS_ENABLED` says a disabled one "passes clicks
                // through". A grab is a click, so both apply here and in the
                // handler snapshot below, from one predicate.
                if !scene.is_hit_testable(id) {
                    continue;
                }
                let Some(scene_rect) = scene.scene_rect(id) else {
                    continue;
                };
                let Some(key) = scene.paint_key(id) else {
                    continue;
                };
                let scene_xform = scene.scene_transform(id);
                let ignores_xform =
                    flags.contains(crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS);
                let scene_anchor = if ignores_xform {
                    scene_xform.apply_point(Point::ZERO)
                } else {
                    Point::ZERO
                };
                snapshot.push(super::DraggableSnapshotEntry {
                    id,
                    scene_rect,
                    scene_transform: scene_xform,
                    shape: item.shape(),
                    ignores_xform,
                    scene_anchor,
                    local_bounds: scene.local_bounds(id).unwrap_or(Rect::ZERO),
                    key,
                });
            }
            // Topmost-first in the view's ONE paint order, so the first shape
            // match in `hit_draggable_item` is the entry the user sees on top.
            // A *stable descending* sort on `z` alone — which is what this used
            // to be — leaves equal-z ties in the ascending order `scene.ids()`
            // produced, so the first match was the oldest, i.e. the bottom-most
            // item: paint and hit were inverted on ties. Comparing whole
            // `PaintKey`s cannot have that failure mode.
            snapshot.sort_by_key(|e| std::cmp::Reverse(e.key));
        }

        // Refresh the handler-dispatch snapshot used by
        // `on_pointer_event` to route hover / tap / context-menu
        // events to the item under the pointer. Every hit-testable item is
        // included, not only the ones carrying handlers: a handler-less item
        // painted on top still occludes what is beneath it inside the
        // lightweight tier (the hit test resolves the topmost *entry* and only
        // then looks for handlers), and an item without handlers can still
        // carry a per-item cursor. `claims_press` — which is a different and
        // narrower question — is recorded per entry and read by exactly one
        // thing, the `Over`-band veto.
        {
            let mut snap = self.handler_snapshot.borrow_mut();
            snap.clear();
            let mut has_over_claimant = false;
            let scene = self.model.0.borrow();
            for id in scene.ids() {
                let Some(item) = scene.item(id) else {
                    continue;
                };
                // Same two flag contracts as the draggable snapshot above.
                if !scene.is_hit_testable(id) {
                    continue;
                }
                let Some(scene_rect) = scene.scene_rect(id) else {
                    continue;
                };
                let Some(key) = scene.paint_key(id) else {
                    continue;
                };
                let scene_xform = scene.scene_transform(id);
                let claims_press = crate::pick::claims_press(
                    scene.handlers(id),
                    scene.flags(id).unwrap_or_default(),
                );
                has_over_claimant |= claims_press && key.rank() == crate::pick::RANK_OVER;
                let handlers = scene.handlers(id).cloned().map(Box::new);
                // Take the item's geometry as a value, so the snapshot can
                // answer the narrow phase without holding a borrow on the
                // Scene. `ItemShape` is O(1) to clone by construction — the
                // path kind is one refcount bump on memoised geometry — which
                // is what makes doing this every layout pass free.
                let shape = item.shape();
                let local_bounds = scene.local_bounds(id).unwrap_or(Rect::ZERO);
                let flags = scene.flags(id).unwrap_or_default();
                let ignores_xform =
                    flags.contains(crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS);
                // For IGNORES items the scene_anchor is fixed across
                // pan/zoom (it lives in scene coords); the dispatch
                // closure projects it through the live view transform
                // at event time to obtain the current screen anchor.
                let scene_anchor = if ignores_xform {
                    scene_xform.apply_point(Point::ZERO)
                } else {
                    Point::ZERO
                };
                snap.push(HandlerSnapshotEntry {
                    id,
                    scene_rect,
                    scene_transform: scene_xform,
                    shape,
                    key,
                    claims_press,
                    handlers,
                    ignores_xform,
                    scene_anchor,
                    local_bounds,
                });
            }
            // Topmost-first in the view's one paint order — see the draggable
            // snapshot above for why sorting on `z` alone got equal-z ties
            // exactly backwards.
            snap.sort_by_key(|e| std::cmp::Reverse(e.key));
            // The veto gate and its memo both key off this snapshot, so they
            // are refreshed with it and never outlive it.
            self.over_claimants.set(has_over_claimant);
            self.snapshot_generation
                .set(self.snapshot_generation.get().wrapping_add(1));
            self.veto_memo.set(None);
        }

        LayoutResponse::rigid(size)
    }

    pub(super) fn place_children_impl(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Mirror the parent's choice of `bounds.origin` into a signal
        // so the derived view-transform picks it up. The signal is
        // bound at `BindingLevel::RepaintOnly` via `set_content_transform`,
        // so changes only trigger repaint — never relayout — which
        // keeps idle behaviour intact when the SceneView is at rest.
        let new_origin = Vec2::new(bounds.x, bounds.y);
        if self.bounds_origin_signal.get() != new_origin {
            self.bounds_origin_signal.set(new_origin);
        }

        // Drain any pending marquee commit posted by the
        // on_drag closure on the previous `Ended`. We do it here
        // (not in the closure) because `place_children` has direct
        // access to `&self.scene` for the spatial-index query
        // — keeping `Scene` plain instead of `Rc<RefCell<Scene>>`.
        // After commit, clear the in-flight marquee so paint stops
        // overlaying the rect.
        // Take first: holding the `RefMut` across the commit would make any
        // future re-entrant post from an observer a panic rather than a queue.
        let pending = self.pending_marquee_commit.borrow_mut().take();
        if let Some((region, mode, additive)) = pending {
            self.selection.commit_marquee_region(
                &self.model.0.borrow(),
                &region,
                mode,
                self.view_scale(),
                additive,
            );
            self.marquee.set(None);
        }

        // Drain any pending drag-to-move commit, applied
        // via the public `flush_pending_mutations` helper to keep
        // the borrow tractable (`place_children` takes `&self`,
        // and `Scene::set_local_pos` needs `&mut Scene`). The
        // framework calls layout from `&mut tree`, which gives
        // `&mut self` access elsewhere — but inside this trait
        // method we have only `&self`. Defer to a separate
        // `flush_pending_mutations(&mut self)` step instead. For
        // headless tests that drive the closure directly, the
        // public `flush_marquee_commit` / `flush_pending_mutations`
        // methods materialise the result.

        // (Lightweight-bounds snapshot is refreshed in
        // `layout_response` so it's available even when the
        // SceneView has zero heavyweight children — see comment
        // there.)

        // Place each child at its **pure scene coordinate** — not
        // offset by `bounds.origin`. The renderer's transform stack
        // composes `bounds.origin` in via the view transform's final
        // translate, so a child at scene (sx, sy) lands visually at
        // (bounds.x + zoom*sx + pan.x, bounds.y + zoom*sy + pan.y).
        // The transform-aware hit-test routes through the same
        // scope automatically.
        //
        // Cull: compute the visible scene-coord region by
        // inverse-transforming the SceneView's screen-space rect,
        // then collapse the size of any child whose `scene_rect`
        // doesn't intersect it. The placement's `origin` stays at
        // its canonical scene-coord position (so focus-follow /
        // scroll-into-view see consistent coordinates whether or not
        // the child is visible); only `size` flips to zero, which
        // short-circuits the recursive layout walk under that child
        // and skips its paint entirely. Heavyweight children stay
        // materialised — true demand-load is a follow-up once
        // the lightweight tier is in place.
        let visible_ids = self.compute_visible_ids(bounds);
        for placement in children.iter_mut() {
            let Some(&item_id) = self.widget_to_item.get(&placement.id) else {
                continue;
            };
            let Some(rect) = self.scene().scene_rect(item_id) else {
                continue;
            };
            placement.origin = Point::new(rect.x, rect.y);
            placement.size = if visible_ids.contains(&item_id) {
                Size::new(rect.width, rect.height)
            } else {
                Size::ZERO
            };
        }
    }

    /// The scene-coord region currently inside the viewport, given
    /// the view transform's current value. Used by `place_children`
    /// to decide which items to lay out at full size and which to
    /// collapse to zero. Falls back to a degenerate-but-non-empty
    /// rect at the SceneView's screen position when the view
    /// transform is singular (zoom = 0); zero zoom collapses
    /// everything visually anyway, so the cull fallback is a
    /// safe-by-default choice.
    pub(super) fn visible_scene_region(&self, bounds: Rect) -> Rect {
        // The view transform now folds in `bounds.origin`, so to find
        // the visible scene region we inverse-apply against the
        // SceneView's full screen-space rect (origin and size).
        // Works correctly for both root SceneView (`bounds.origin =
        // (0, 0)`) and nested SceneView at a non-zero parent offset.
        let viewport_screen = Rect::new(bounds.x, bounds.y, bounds.width, bounds.height);
        match self.view_transform().inverse() {
            Some(inv) => inv.apply_rect(viewport_screen),
            None => Rect::ZERO,
        }
    }

    fn compute_visible_ids(&self, bounds: Rect) -> HashSet<ItemId> {
        let region = self.visible_scene_region(bounds);
        self.scene().items_in_rect(region).into_iter().collect()
    }
}
