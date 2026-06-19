// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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
                let Some(scene_rect) = scene.scene_rect(id) else {
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
                    shape_contains: item.clone_shape_test().into(),
                    ignores_xform,
                    scene_anchor,
                    local_bounds: scene.local_bounds(id).unwrap_or(Rect::ZERO),
                    z: scene.z(id).unwrap_or(0.0),
                });
            }
            // Topmost-first so the first shape match in `hit_draggable_item`
            // wins, matching the handler-snapshot hit-test ordering.
            snapshot.sort_by(|a, b| b.z.partial_cmp(&a.z).unwrap_or(std::cmp::Ordering::Equal));
        }

        // Refresh the handler-dispatch snapshot used by
        // `on_pointer_event` to route hover / tap / context-menu
        // events to the item under the pointer. Only items with a
        // handler set installed need to be considered for routing,
        // but we include every item so cursor-over-item-without-
        // handler can still consult the per-item cursor field.
        {
            let mut snap = self.handler_snapshot.borrow_mut();
            snap.clear();
            let scene = self.model.0.borrow();
            for id in scene.ids() {
                let Some(item) = scene.item(id) else {
                    continue;
                };
                let Some(scene_rect) = scene.scene_rect(id) else {
                    continue;
                };
                let scene_xform = scene.scene_transform(id);
                let z = scene.z(id).unwrap_or(0.0);
                let handlers = scene.handlers(id).cloned().map(Box::new);
                // Capture the item's shape-test as a stand-alone
                // closure so the snapshot can answer narrow-phase
                // hit-test without holding a borrow on the Scene.
                // Items with non-AABB geometry (PathItem stroke-only,
                // GroupItem logical-only) override `clone_shape_test`
                // to capture the data they need; default impl returns
                // an AABB predicate over `local_bounds`.
                let shape_contains: Rc<dyn Fn(Point, f32) -> bool> = item.clone_shape_test().into();
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
                    shape_contains,
                    z,
                    handlers,
                    ignores_xform,
                    scene_anchor,
                    local_bounds,
                });
            }
            // Sort by z descending so hit-test picks topmost first.
            snap.sort_by(|a, b| b.z.partial_cmp(&a.z).unwrap_or(std::cmp::Ordering::Equal));
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
        if let Some((rect, additive)) = self.pending_marquee_commit.take() {
            self.selection
                .commit_marquee(&self.model.0.borrow(), rect, additive);
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
