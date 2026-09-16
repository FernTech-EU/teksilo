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
        ctx: &LayoutContext,
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

        // Bring the two hit-test snapshots up to date. Done here rather than
        // in `place_children` because `place_children` only runs when the
        // SceneView has at least one heavyweight child — a scene of only
        // lightweight items would never get its snapshots populated.
        // `layout_response` runs every layout pass regardless.
        //
        // "Up to date" is usually nothing at all: the snapshots are functions
        // of the model, and a pan changes no item. See
        // [`hit_snapshot`](super::hit_snapshot).
        self.refresh_hit_snapshots();

        let _ = ctx;
        LayoutResponse::rigid(size)
    }

    /// The scene-coord region inside which a heavyweight card stays live —
    /// `None` meaning "everything stays live".
    ///
    /// Three regions are in play and they are deliberately different. Each is
    /// contained in the next, and each answers its own question:
    ///
    /// * [`visible_scene_region`](Self::visible_scene_region) — the tight
    ///   viewport, zero margin. *Who is drawn.* Decides who is laid out at
    ///   full size and who collapses to `Size::ZERO`. Unchanged by retention.
    /// * `A11yOffScreenMode::at_visible_region` — *who is enumerated.* The
    ///   app's statement about how much of an off-screen scene a screen reader
    ///   should be offered.
    /// * this one — *who exists.* The tight viewport grown by
    ///   [`retention_margin`](Self::retention_margin), unioned with the
    ///   accessibility region.
    ///
    /// It contains the tight region by construction, so a card being laid out
    /// is never simultaneously parked, and it contains the accessibility
    /// region by construction, so the AT walk is never asked to describe a
    /// card the arena has parked. That second containment is what lets
    /// `A11yOffScreenMode::AllItems` keep its documented promise of a complete
    /// table of contents: it returns `None` here, and nothing is ever parked.
    ///
    /// The containment is one-directional on purpose. Where this region is
    /// *strictly* larger than the accessibility one — `ViewportOnly` with a
    /// non-zero margin — the difference is a band of cards that are alive and
    /// not enumerated. `place_children_impl` records that narrower set
    /// separately (`SceneView::at_heavy`); growing the margin must never grow
    /// what assistive technology is asked to walk.
    pub(super) fn retention_scene_region(&self, bounds: Rect) -> Option<Rect> {
        let tight = self.visible_scene_region(bounds);
        // `AllItems` → the whole model must stay enumerable → retain all.
        let at_region = self.a11y_off_screen_mode.at_visible_region(tight)?;
        // Screen pixels → scene units through the current zoom, so the margin
        // is a constant on-screen distance however far the view is zoomed in.
        let scale = self.view_scale();
        let m = if scale > f32::EPSILON {
            self.retention_margin / scale
        } else {
            0.0
        };
        let grown = Rect::new(
            tight.x - m,
            tight.y - m,
            tight.width + m * 2.0,
            tight.height + m * 2.0,
        );
        Some(crate::scene::union_two_rects(grown, at_region))
    }

    pub(super) fn place_children_impl(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
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
        //
        // The cull is asked **per child**, not by enumerating the visible set:
        // a widget with no children still gets `place_children`, and querying
        // the spatial index there cost one `Vec` + one `HashSet` over every
        // item in the viewport on every pan sample — 104 µs at 540 visible
        // items, to answer a question about zero cards. Testing each child's
        // own `scene_rect` (already computed on the line above) against the
        // region gives the identical answer — `items_in_rect` narrow-phases on
        // exactly that predicate — for the price of the children this view
        // actually has.
        //
        // **And the two wider decisions.** `SceneView::culls_children` is
        // `true`, so this slice carries the parked cards too and
        // `WidgetPlacement::dormant` is read back: a card outside the
        // *retention* region is parked, which is the difference between a
        // widget that is invisible and one that is not there. Zero size costs
        // a card its geometry; dormancy costs it its existence — it leaves
        // paint, the layout recursion, the AccessKit tree and the Tab ring,
        // and keeps all its state for when the camera brings it back.
        //
        // Between the two sits the *accessibility* region, which decides what
        // is published rather than what exists. All three are layered, never
        // merged, each containing the one before it: a card being laid out is
        // never also parked, and a card the walk is asked to describe is never
        // one the arena has taken away.
        if children.is_empty() {
            // Recorded rather than left alone: a view whose last card was
            // destroyed would otherwise keep publishing the previous pass's
            // list of dead ids. The walker drops them (nothing is active at
            // those ids), but "empty because there is nothing" and "empty
            // because nothing has been decided" are different states and only
            // one of them is true here.
            *self.at_heavy.borrow_mut() = Some(HashSet::new());
            *self.at_children.borrow_mut() = Some(Vec::new());
            return;
        }
        let region = self.visible_scene_region(bounds);
        let retention = self.retention_scene_region(bounds);
        // The third region, and the one the app actually asked for: what
        // `A11yOffScreenMode` says a screen reader should be offered. It is
        // contained in `retention` by construction, and where it is *strictly*
        // contained — `ViewportOnly` with a non-zero margin — the difference is
        // a band of cards that are alive and not enumerated. That is the
        // intended reading of the two knobs: the margin is a lifecycle hint,
        // the mode is the accessibility statement, and a lifecycle hint must
        // not widen what assistive technology is asked to walk.
        let at_region = self.a11y_off_screen_mode.at_visible_region(region);
        // Whatever the user is in the middle of stays live wherever the camera
        // goes: parking it would clear focus and cancel its pointer with
        // `CancelReason::SubtreeParked`, because the *view* moved. Read from
        // the live interactions upward, so this costs the interactions (almost
        // always none) and not the cards.
        let mut pinned: Vec<WidgetId> = Vec::new();
        ctx.for_each_interaction_ancestor(|wid| {
            if self.widget_to_item.contains_key(&wid) {
                pinned.push(wid);
            }
        });

        let mut at_heavy: HashSet<ItemId> = HashSet::new();
        let mut at_children: Vec<WidgetId> = Vec::new();
        let scene = self.model.0.borrow();
        for placement in children.iter_mut() {
            let Some(&item_id) = self.widget_to_item.get(&placement.id) else {
                // Not a card this view placed. The cull has no opinion about
                // it, and `at_children` is the whole accessibility child list,
                // so leaving it out would quietly hide it from AT rather than
                // leave it alone.
                at_children.push(placement.id);
                continue;
            };
            let Some(rect) = scene.scene_rect(item_id) else {
                // No resolvable geometry is not a reason to take a card away —
                // nor to hide it from assistive technology, which is the one
                // audience that cannot see that it is nowhere.
                placement.dormant = false;
                at_heavy.insert(item_id);
                at_children.push(placement.id);
                continue;
            };
            // Origin and size are written for a parked card too. It costs a
            // parent-chain walk per parked card per pass, and it is not
            // optional: the placement's origin is the canonical scene
            // coordinate `scroll_into_view` and focus-follow read, and a card
            // whose position changed while it was parked would otherwise come
            // back — or be scrolled to — at a stale one.
            placement.origin = Point::new(rect.x, rect.y);
            placement.size = if crate::scene::rects_intersect(rect, region) {
                Size::new(rect.width, rect.height)
            } else {
                Size::ZERO
            };
            // Whatever the user is in the middle of survives both decisions.
            // For retention that is about not destroying their work; for
            // emission it is about not publishing a tree whose focus names a
            // node the walk left out.
            let is_pinned = pinned.contains(&placement.id);
            let keep = match retention {
                None => true,
                Some(r) => crate::scene::rects_intersect(rect, r) || is_pinned,
            };
            placement.dormant = !keep;
            if !keep {
                continue;
            }
            let enumerate = match at_region {
                None => true,
                Some(r) => crate::scene::rects_intersect(rect, r) || is_pinned,
            };
            if enumerate {
                at_heavy.insert(item_id);
                at_children.push(placement.id);
            }
        }
        drop(scene);
        // Handed to the accessibility walk, which must describe exactly these
        // cards and no others — see `SceneView::at_heavy`. Both halves are
        // written here, from the one decision above, so the set the scene's own
        // logical graft consults and the list the framework walker is handed
        // cannot disagree.
        //
        // This set can widen without any child parking or waking — a focus move
        // pins a card into it — so the framework's park/wake invalidation does
        // not cover every reason it changes. `WidgetTree::invalidate_culls_for_moved_interaction`
        // dirties the accessibility cache for exactly that case, on the same
        // signal that forces this pass to run at all.
        *self.at_heavy.borrow_mut() = Some(at_heavy);
        *self.at_children.borrow_mut() = Some(at_children);
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
}
